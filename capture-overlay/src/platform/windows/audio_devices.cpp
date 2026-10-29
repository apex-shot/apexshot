#include "platform/windows/win_util.h"

#include "platform/windows/audio_devices.h"

#include <dshow.h>
#include <mmdeviceapi.h>
#include <audioclient.h>
#include <audiopolicy.h>
#include <audioclientactivationparams.h>
#include <functiondiscoverykeys_devpkey.h>
#include <shellapi.h>
#include <tlhelp32.h>
#include <uuids.h>

#include <QFile>
#include <QFileInfo>
#include <QHash>
#include <QtEndian>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <future>
#include <iterator>
#include <thread>

namespace {

// ------------------------------------------------------------ COM ----------
/// Initialise COM for one scope, and only undo it when this scope was the one
/// that initialised the thread. Qt already puts the GUI thread in an STA, and
/// the capture threads want an MTA of their own.
class ComScope
{
public:
    explicit ComScope(DWORD model)
    {
        const HRESULT result = CoInitializeEx(nullptr, model);
        m_owned = SUCCEEDED(result);
    }
    ~ComScope()
    {
        if (m_owned)
            CoUninitialize();
    }

    ComScope(const ComScope&) = delete;
    ComScope& operator=(const ComScope&) = delete;

    bool owns() const { return m_owned; }

private:
    bool m_owned = false;
};

template <typename T>
void release(T*& pointer)
{
    if (pointer) {
        pointer->Release();
        pointer = nullptr;
    }
}

QString hresultText(HRESULT result)
{
    return QStringLiteral("0x%1").arg(static_cast<quint32>(result), 8, 16, QLatin1Char('0'));
}

// ------------------------------------------------------ static devices -----
QString directShowFriendlyName(IMoniker* moniker)
{
    IPropertyBag* bag = nullptr;
    if (FAILED(moniker->BindToStorage(
            nullptr, nullptr, IID_IPropertyBag, reinterpret_cast<void**>(&bag))))
        return {};
    if (!bag)
        return {};

    QString name;
    VARIANT value;
    value.vt = VT_EMPTY;
    if (SUCCEEDED(bag->Read(L"FriendlyName", &value, nullptr)) && value.vt == VT_BSTR
        && value.bstrVal) {
        name = QString::fromWCharArray(value.bstrVal).trimmed();
    }
    VariantClear(&value);
    bag->Release();
    return name;
}

QString wasapiFriendlyName(IMMDevice* device)
{
    if (!device)
        return {};
    IPropertyStore* store = nullptr;
    if (FAILED(device->OpenPropertyStore(STGM_READ, &store)) || !store)
        return {};

    QString name;
    PROPVARIANT value;
    value.vt = VT_EMPTY;
    if (SUCCEEDED(store->GetValue(PKEY_Device_FriendlyName, &value))
        && value.vt == VT_LPWSTR && value.pwszVal) {
        name = QString::fromWCharArray(value.pwszVal).trimmed();
    }
    PropVariantClear(&value);
    store->Release();
    return name;
}

QString processExecutable(quint32 processId)
{
    if (processId == 0)
        return {};
    HANDLE process =
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, processId);
    if (!process)
        return {};

    wchar_t path[4096];
    DWORD size = static_cast<DWORD>(std::size(path));
    QString executable;
    if (QueryFullProcessImageNameW(process, 0, path, &size) && size > 0)
        executable = QString::fromWCharArray(path, static_cast<int>(size));
    CloseHandle(process);
    return executable;
}

QString processDisplayName(quint32 processId, const QString& executable)
{
    if (!executable.isEmpty()) {
        SHFILEINFOW info{};
        if (SHGetFileInfoW(reinterpret_cast<const wchar_t*>(executable.utf16()),
                           0,
                           &info,
                           sizeof(info),
                           SHGFI_DISPLAYNAME)) {
            const QString name = QString::fromWCharArray(info.szDisplayName).trimmed();
            if (!name.isEmpty())
                return name;
        }
        const QString base = QFileInfo(executable).completeBaseName().trimmed();
        if (!base.isEmpty())
            return base;
    }
    return QStringLiteral("Unknown app (%1)").arg(processId);
}

/// The topmost ancestor that runs the same executable, so choosing Chrome
/// captures every tab rather than the one renderer that owns the session.
quint32 sameExecutableRoot(quint32 processId, const QString& executable)
{
    const QString fileName = QFileInfo(executable).fileName();
    if (processId == 0 || fileName.isEmpty())
        return processId;

    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snapshot == INVALID_HANDLE_VALUE)
        return processId;

    QHash<quint32, quint32> parents;
    PROCESSENTRY32W entry{};
    entry.dwSize = sizeof(entry);
    if (Process32FirstW(snapshot, &entry)) {
        do {
            parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
        } while (Process32NextW(snapshot, &entry));
    }
    CloseHandle(snapshot);

    quint32 root = processId;
    for (int depth = 0; depth < 8; ++depth) {
        const quint32 parent = parents.value(root, 0);
        if (parent == 0 || parent == root)
            break;
        const QString parentExe = processExecutable(parent);
        if (QFileInfo(parentExe).fileName().compare(fileName, Qt::CaseInsensitive) != 0)
            break;
        root = parent;
    }
    return root;
}

// ------------------------------------------------------------ formats ------
bool isFloatFormat(const WAVEFORMATEX* format)
{
    if (!format)
        return false;
    if (format->wFormatTag == WAVE_FORMAT_IEEE_FLOAT)
        return true;
    if (format->wFormatTag == WAVE_FORMAT_EXTENSIBLE
        && format->cbSize >= sizeof(WAVEFORMATEXTENSIBLE) - sizeof(WAVEFORMATEX)) {
        const auto* extensible = reinterpret_cast<const WAVEFORMATEXTENSIBLE*>(format);
        // The first field of every KSDATAFORMAT_SUBTYPE_* GUID is its format
        // tag: 1 for PCM and 3 for IEEE float.
        return extensible->SubFormat.Data1 == WAVE_FORMAT_IEEE_FLOAT;
    }
    return false;
}

quint32 channelMaskFor(const WAVEFORMATEX* format)
{
    if (!format)
        return 0;
    if (format->wFormatTag == WAVE_FORMAT_EXTENSIBLE
        && format->cbSize >= sizeof(WAVEFORMATEXTENSIBLE) - sizeof(WAVEFORMATEX)) {
        return reinterpret_cast<const WAVEFORMATEXTENSIBLE*>(format)->dwChannelMask;
    }
    if (format->nChannels == 1)
        return 0x4; // SPEAKER_FRONT_CENTER
    if (format->nChannels == 2)
        return 0x3; // SPEAKER_FRONT_LEFT | SPEAKER_FRONT_RIGHT
    return 0;
}

float peakOf(const BYTE* data, UINT32 frames, const WAVEFORMATEX* format)
{
    if (!data || frames == 0 || !format)
        return 0.0f;

    const quint64 samples =
        static_cast<quint64>(frames) * std::max<int>(1, format->nChannels);
    float peak = 0.0f;

    if (isFloatFormat(format) && format->wBitsPerSample == 32) {
        const auto* values = reinterpret_cast<const float*>(data);
        for (quint64 i = 0; i < samples; ++i)
            peak = std::max(peak, std::abs(values[i]));
    } else if (format->wBitsPerSample == 16) {
        const auto* values = reinterpret_cast<const qint16*>(data);
        for (quint64 i = 0; i < samples; ++i)
            peak = std::max(peak, std::abs(values[i] / 32768.0f));
    } else if (format->wBitsPerSample == 32) {
        const auto* values = reinterpret_cast<const qint32*>(data);
        for (quint64 i = 0; i < samples; ++i)
            peak = std::max(peak, std::abs(values[i] / 2147483648.0f));
    } else if (format->wBitsPerSample == 24) {
        for (quint64 i = 0; i < samples; ++i) {
            const BYTE* sample = data + i * 3;
            qint32 value = (static_cast<qint32>(sample[2]) << 16)
                | (static_cast<qint32>(sample[1]) << 8) | static_cast<qint32>(sample[0]);
            if (value & 0x00800000)
                value |= static_cast<qint32>(0xFF000000);
            peak = std::max(peak, std::abs(value / 8388608.0f));
        }
    } else if (format->wBitsPerSample == 8) {
        for (quint64 i = 0; i < samples; ++i)
            peak = std::max(peak, std::abs((static_cast<int>(data[i]) - 128) / 128.0f));
    } else {
        return 0.0f;
    }

    return std::min(1.0f, peak);
}

// ---------------------------------------------------------------- WAV ------
void writeLe16(QFile& file, quint16 value)
{
    const quint16 little = qToLittleEndian(value);
    file.write(reinterpret_cast<const char*>(&little), sizeof(little));
}

void writeLe32(QFile& file, quint32 value)
{
    const quint32 little = qToLittleEndian(value);
    file.write(reinterpret_cast<const char*>(&little), sizeof(little));
}

void writeWaveHeader(QFile& file, const WAVEFORMATEX* format, quint32 dataBytes)
{
    const quint16 channels = std::max<quint16>(1, format->nChannels);
    const quint32 rate = format->nSamplesPerSec > 0 ? format->nSamplesPerSec : 48000;
    const quint16 blockAlign = std::max<quint16>(
        1, format->nBlockAlign > 0 ? format->nBlockAlign : static_cast<quint16>(channels * 2));
    const quint16 bits = std::max<quint16>(
        8, format->wBitsPerSample > 0 ? format->wBitsPerSample : 16);
    const bool floating = isFloatFormat(format);
    const quint32 riffBytes = 60 + dataBytes;

    file.write("RIFF", 4);
    writeLe32(file, riffBytes);
    file.write("WAVE", 4);
    file.write("fmt ", 4);
    writeLe32(file, 40);
    writeLe16(file, WAVE_FORMAT_EXTENSIBLE);
    writeLe16(file, channels);
    writeLe32(file, rate);
    writeLe32(file, rate * blockAlign);
    writeLe16(file, blockAlign);
    writeLe16(file, bits);
    writeLe16(file, 22);
    writeLe16(file, bits);
    writeLe32(file, channelMaskFor(format));
    const quint32 formatTag = floating ? WAVE_FORMAT_IEEE_FLOAT : WAVE_FORMAT_PCM;
    writeLe32(file, formatTag);
    writeLe16(file, 0x0000);
    writeLe16(file, 0x0010);
    const uchar tail[8] = {0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71};
    file.write(reinterpret_cast<const char*>(tail), sizeof(tail));
    file.write("data", 4);
    writeLe32(file, dataBytes);
}

void patchWaveHeader(QFile& file, quint32 dataBytes)
{
    if (!file.isOpen())
        return;
    file.seek(4);
    writeLe32(file, 60 + dataBytes);
    file.seek(64);
    writeLe32(file, dataBytes);
    file.flush();
}

// ------------------------------------------------------ async activation ---
class ActivationHandler : public IActivateAudioInterfaceCompletionHandler
{
public:
    ActivationHandler()
        : m_event(CreateEventW(nullptr, TRUE, FALSE, nullptr))
    {
    }

    ~ActivationHandler()
    {
        if (m_event)
            CloseHandle(m_event);
        release(m_client);
    }

    HANDLE event() const { return m_event; }
    HRESULT result() const { return m_result; }
    IAudioClient* takeClient()
    {
        IAudioClient* client = m_client;
        m_client = nullptr;
        return client;
    }

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** object) override
    {
        if (!object)
            return E_POINTER;
        // Windows calls the completion handler from its own MTA worker and
        // refuses the activation without an agile object. A free-threaded
        // handler satisfies that by reporting IAgileObject as this object.
        if (riid == __uuidof(IUnknown) || riid == __uuidof(IAgileObject)
            || riid == __uuidof(IActivateAudioInterfaceCompletionHandler)) {
            *object = static_cast<IActivateAudioInterfaceCompletionHandler*>(this);
            AddRef();
            return S_OK;
        }
        *object = nullptr;
        return E_NOINTERFACE;
    }

    ULONG STDMETHODCALLTYPE AddRef() override { return ++m_references; }

    ULONG STDMETHODCALLTYPE Release() override
    {
        const ULONG remaining = --m_references;
        if (remaining == 0)
            delete this;
        return remaining;
    }

    HRESULT STDMETHODCALLTYPE
    ActivateCompleted(IActivateAudioInterfaceAsyncOperation* operation) override
    {
        IUnknown* activated = nullptr;
        HRESULT result = E_FAIL;
        if (operation)
            operation->GetActivateResult(&result, &activated);
        if (SUCCEEDED(result) && activated) {
            m_result = activated->QueryInterface(__uuidof(IAudioClient),
                                                 reinterpret_cast<void**>(&m_client));
            if (FAILED(m_result))
                m_client = nullptr;
        } else {
            m_result = result;
        }
        if (activated)
            activated->Release();
        if (m_event)
            SetEvent(m_event);
        return S_OK;
    }

private:
    std::atomic<ULONG> m_references{1};
    HANDLE m_event = nullptr;
    HRESULT m_result = E_FAIL;
    IAudioClient* m_client = nullptr;
};

/// Activate a process-loopback client for one process tree.
IAudioClient* activateProcessLoopback(quint32 processId, QString& error)
{
    AUDIOCLIENT_ACTIVATION_PARAMS params{};
    params.ActivationType = AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK;
    params.ProcessLoopbackParams.TargetProcessId = processId;
    params.ProcessLoopbackParams.ProcessLoopbackMode =
        PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE;

    PROPVARIANT property;
    property.vt = VT_BLOB;
    property.blob.cbSize = sizeof(params);
    property.blob.pBlobData = reinterpret_cast<BYTE*>(&params);

    auto* handler = new ActivationHandler;
    IActivateAudioInterfaceAsyncOperation* operation = nullptr;
    const HRESULT started = ActivateAudioInterfaceAsync(
        VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
        __uuidof(IAudioClient),
        &property,
        handler,
        &operation);
    if (FAILED(started)) {
        error = QStringLiteral("process-loopback activation failed (%1)")
                    .arg(hresultText(started));
        handler->Release();
        return nullptr;
    }

    const DWORD waited = WaitForSingleObject(handler->event(), 5000);
    if (waited != WAIT_OBJECT_0) {
        error = QStringLiteral("process-loopback activation timed out");
        release(operation);
        handler->Release();
        return nullptr;
    }

    const HRESULT result = handler->result();
    IAudioClient* client = SUCCEEDED(result) ? handler->takeClient() : nullptr;
    if (!client)
        error = QStringLiteral("the app's audio could not be opened (%1)")
                    .arg(hresultText(result));
    release(operation);
    handler->Release();
    return client;
}

IMMDevice* defaultEndpoint(IMMDeviceEnumerator* enumerator, EDataFlow flow)
{
    IMMDevice* device = nullptr;
    if (!enumerator
        || FAILED(enumerator->GetDefaultAudioEndpoint(flow, eConsole, &device)))
        return nullptr;
    return device;
}

IMMDevice* captureEndpointByName(IMMDeviceEnumerator* enumerator, const QString& name)
{
    if (!enumerator)
        return nullptr;
    if (name.trimmed().isEmpty())
        return defaultEndpoint(enumerator, eCapture);

    IMMDeviceCollection* collection = nullptr;
    if (FAILED(enumerator->EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE, &collection))
        || !collection)
        return defaultEndpoint(enumerator, eCapture);

    IMMDevice* fallback = nullptr;
    IMMDevice* match = nullptr;
    UINT count = 0;
    collection->GetCount(&count);
    for (UINT i = 0; i < count; ++i) {
        IMMDevice* device = nullptr;
        if (FAILED(collection->Item(i, &device)) || !device)
            continue;
        if (wasapiFriendlyName(device).compare(name, Qt::CaseInsensitive) == 0) {
            match = device;
            break;
        }
        if (!fallback)
            fallback = device;
        else
            device->Release();
    }
    release(collection);

    if (match) {
        if (fallback)
            fallback->Release();
        return match;
    }
    if (fallback)
        return fallback;
    return defaultEndpoint(enumerator, eCapture);
}

/// The format the loopback client renders into. Process loopback does not
/// implement GetMixFormat, so the client asks for one fixed, universally
/// readable format and lets the audio engine convert into it.
WAVEFORMATEX loopbackFormat()
{
    WAVEFORMATEX format{};
    format.wFormatTag = WAVE_FORMAT_PCM;
    format.nChannels = 2;
    format.nSamplesPerSec = 44100;
    format.wBitsPerSample = 16;
    format.nBlockAlign = static_cast<WORD>(format.nChannels * format.wBitsPerSample / 8);
    format.nAvgBytesPerSec = format.nSamplesPerSec * format.nBlockAlign;
    return format;
}

bool initialiseLoopbackClient(IAudioClient* client,
                              IAudioCaptureClient** capture,
                              HANDLE audioEvent,
                              QString& error)
{
    if (!client) {
        error = QStringLiteral("the audio endpoint was not available");
        return false;
    }

    const WAVEFORMATEX format = loopbackFormat();
    const DWORD flags = AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
        | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
    HRESULT result =
        client->Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 200000, 0, &format, nullptr);
    if (FAILED(result)) {
        error = QStringLiteral("the audio device could not start (%1)")
                    .arg(hresultText(result));
        return false;
    }

    result = client->SetEventHandle(audioEvent);
    if (FAILED(result)) {
        error = QStringLiteral("the audio event could not be attached (%1)")
                    .arg(hresultText(result));
        return false;
    }

    result = client->GetService(__uuidof(IAudioCaptureClient),
                                reinterpret_cast<void**>(capture));
    if (FAILED(result) || !*capture) {
        error = QStringLiteral("the audio capture service was refused (%1)")
                    .arg(hresultText(result));
        return false;
    }

    result = client->Start();
    if (FAILED(result)) {
        error = QStringLiteral("the audio device could not start (%1)")
                    .arg(hresultText(result));
        return false;
    }
    return true;
}

/// Capture endpoints have a real mix format, which the meter reads directly.
bool initialiseCaptureClient(IAudioClient* client,
                             WAVEFORMATEX** format,
                             IAudioCaptureClient** capture,
                             HANDLE audioEvent,
                             QString& error)
{
    if (!client) {
        error = QStringLiteral("the audio endpoint was not available");
        return false;
    }

    HRESULT result = client->GetMixFormat(format);
    if (FAILED(result) || !*format) {
        error = QStringLiteral("the audio format could not be read (%1)")
                    .arg(hresultText(result));
        return false;
    }

    result = client->Initialize(AUDCLNT_SHAREMODE_SHARED,
                                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                                200000,
                                0,
                                *format,
                                nullptr);
    if (FAILED(result)) {
        error = QStringLiteral("the audio device could not start (%1)")
                    .arg(hresultText(result));
        return false;
    }

    result = client->SetEventHandle(audioEvent);
    if (FAILED(result)) {
        error = QStringLiteral("the audio event could not be attached (%1)")
                    .arg(hresultText(result));
        return false;
    }

    result = client->GetService(__uuidof(IAudioCaptureClient),
                                reinterpret_cast<void**>(capture));
    if (FAILED(result) || !*capture) {
        error = QStringLiteral("the audio capture service was refused (%1)")
                    .arg(hresultText(result));
        return false;
    }

    result = client->Start();
    if (FAILED(result)) {
        error = QStringLiteral("the audio device could not start (%1)")
                    .arg(hresultText(result));
        return false;
    }
    return true;
}

}  // namespace

namespace Platform {

QStringList directShowDevices(bool video)
{
    QStringList names;
    ComScope com(COINIT_APARTMENTTHREADED);

    ICreateDevEnum* deviceEnumerator = nullptr;
    const HRESULT created = CoCreateInstance(CLSID_SystemDeviceEnum,
                                             nullptr,
                                             CLSCTX_INPROC_SERVER,
                                             IID_ICreateDevEnum,
                                             reinterpret_cast<void**>(&deviceEnumerator));
    if (FAILED(created) || !deviceEnumerator)
        return names;

    IEnumMoniker* monikers = nullptr;
    const GUID& category = video ? CLSID_VideoInputDeviceCategory
                                 : CLSID_AudioInputDeviceCategory;
    if (SUCCEEDED(deviceEnumerator->CreateClassEnumerator(category, &monikers, 0))
        && monikers) {
        IMoniker* moniker = nullptr;
        while (monikers->Next(1, &moniker, nullptr) == S_OK) {
            const QString name = directShowFriendlyName(moniker);
            if (!name.isEmpty() && !names.contains(name, Qt::CaseInsensitive))
                names.append(name);
            moniker->Release();
        }
        monikers->Release();
    }
    deviceEnumerator->Release();
    return names;
}

QString defaultMicrophoneName()
{
    ComScope com(COINIT_APARTMENTTHREADED);
    IMMDeviceEnumerator* enumerator = nullptr;
    if (FAILED(CoCreateInstance(__uuidof(MMDeviceEnumerator),
                                nullptr,
                                CLSCTX_ALL,
                                __uuidof(IMMDeviceEnumerator),
                                reinterpret_cast<void**>(&enumerator)))
        || !enumerator)
        return {};

    IMMDevice* device = defaultEndpoint(enumerator, eCapture);
    const QString name = wasapiFriendlyName(device);
    release(device);
    enumerator->Release();
    return name;
}

QVector<AudioApplication> audioApplications()
{
    QVector<AudioApplication> applications;
    ComScope com(COINIT_APARTMENTTHREADED);

    IMMDeviceEnumerator* enumerator = nullptr;
    if (FAILED(CoCreateInstance(__uuidof(MMDeviceEnumerator),
                                nullptr,
                                CLSCTX_ALL,
                                __uuidof(IMMDeviceEnumerator),
                                reinterpret_cast<void**>(&enumerator)))
        || !enumerator)
        return applications;

    IMMDevice* device = defaultEndpoint(enumerator, eRender);
    IAudioSessionManager2* manager = nullptr;
    if (!device
        || FAILED(device->Activate(__uuidof(IAudioSessionManager2),
                                   CLSCTX_ALL,
                                   nullptr,
                                   reinterpret_cast<void**>(&manager)))
        || !manager) {
        release(device);
        enumerator->Release();
        return applications;
    }

    IAudioSessionEnumerator* sessions = nullptr;
    if (SUCCEEDED(manager->GetSessionEnumerator(&sessions)) && sessions) {
        int count = 0;
        sessions->GetCount(&count);

        QHash<quint32, AudioApplication> byProcess;
        const quint32 ownProcess = GetCurrentProcessId();
        for (int i = 0; i < count; ++i) {
            IAudioSessionControl* control = nullptr;
            if (FAILED(sessions->GetSession(i, &control)) || !control)
                continue;

            IAudioSessionControl2* control2 = nullptr;
            if (FAILED(control->QueryInterface(__uuidof(IAudioSessionControl2),
                                               reinterpret_cast<void**>(&control2)))
                || !control2) {
                control->Release();
                continue;
            }

            AudioSessionState state = AudioSessionStateExpired;
            control->GetState(&state);
            DWORD processId = 0;
            control2->GetProcessId(&processId);

            if (state != AudioSessionStateExpired && processId != 0
                && processId != ownProcess && !byProcess.contains(processId)) {
                const QString executable = processExecutable(processId);
                const QString executableName = QFileInfo(executable).fileName().toLower();
                // Shell hosts own sessions for interface sounds; they are not
                // apps a person records.
                static const QStringList shellHosts = {
                    QStringLiteral("shellexperiencehost.exe"),
                    QStringLiteral("startmenuexperiencehost.exe"),
                    QStringLiteral("searchhost.exe"),
                    QStringLiteral("textinputhost.exe"),
                    QStringLiteral("lockapp.exe"),
                    QStringLiteral("audiodg.exe"),
                };
                if (shellHosts.contains(executableName)) {
                    control2->Release();
                    control->Release();
                    continue;
                }
                AudioApplication application;
                application.processId =
                    sameExecutableRoot(processId, executable);
                application.executable = executable;
                application.name = processDisplayName(processId, executable);
                application.active = state == AudioSessionStateActive;

                if (byProcess.isEmpty() || !byProcess.contains(application.processId)) {
                    byProcess.insert(application.processId, application);
                    applications.append(application);
                }
            }

            control2->Release();
            control->Release();
        }
        sessions->Release();
    }

    manager->Release();
    release(device);
    enumerator->Release();

    std::sort(applications.begin(),
              applications.end(),
              [](const AudioApplication& left, const AudioApplication& right) {
                  if (left.active != right.active)
                      return left.active;
                  return left.name.compare(right.name, Qt::CaseInsensitive) < 0;
              });
    return applications;
}

// ------------------------------------------------------- level monitor -----
struct AudioLevelMonitor::Impl
{
    std::thread thread;
    std::atomic<bool> running{false};
    std::atomic<bool> opened{false};
    std::atomic<float> peak{0.0f};
    HANDLE stopEvent = nullptr;

    ~Impl() { stop(); }

    void stop()
    {
        running.store(false);
        if (stopEvent)
            SetEvent(stopEvent);
        if (thread.joinable())
            thread.join();
        if (stopEvent) {
            CloseHandle(stopEvent);
            stopEvent = nullptr;
        }
        opened.store(false);
        peak.store(0.0f);
    }

    void run(QString deviceName)
    {
        ComScope com(COINIT_MULTITHREADED);
        if (!com.owns()) {
            return;
        }

        IMMDeviceEnumerator* enumerator = nullptr;
        if (FAILED(CoCreateInstance(__uuidof(MMDeviceEnumerator),
                                    nullptr,
                                    CLSCTX_ALL,
                                    __uuidof(IMMDeviceEnumerator),
                                    reinterpret_cast<void**>(&enumerator)))
            || !enumerator) {
            return;
        }

        IMMDevice* device = captureEndpointByName(enumerator, deviceName);
        IAudioClient* client = nullptr;
        if (!device
            || FAILED(device->Activate(__uuidof(IAudioClient),
                                       CLSCTX_ALL,
                                       nullptr,
                                       reinterpret_cast<void**>(&client)))
            || !client) {
            release(device);
            enumerator->Release();
            return;
        }

        HANDLE audioEvent = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        WAVEFORMATEX* format = nullptr;
        IAudioCaptureClient* capture = nullptr;
        QString error;
        if (!initialiseCaptureClient(client, &format, &capture, audioEvent, error)) {
            if (format)
                CoTaskMemFree(format);
            release(capture);
            release(client);
            release(device);
            CloseHandle(audioEvent);
            enumerator->Release();
            return;
        }

        opened.store(true);
        HANDLE events[2] = {stopEvent, audioEvent};
        while (running.load()) {
            const DWORD waited = WaitForMultipleObjects(2, events, FALSE, 100);
            if (waited != WAIT_OBJECT_0 + 1)
                continue;

            float packetPeak = 0.0f;
            UINT32 packet = 0;
            while (SUCCEEDED(capture->GetNextPacketSize(&packet)) && packet > 0) {
                BYTE* data = nullptr;
                UINT32 frames = 0;
                DWORD flags = 0;
                if (FAILED(capture->GetBuffer(&data, &frames, &flags, nullptr, nullptr)))
                    break;
                if (!(flags & AUDCLNT_BUFFERFLAGS_SILENT))
                    packetPeak = std::max(packetPeak, peakOf(data, frames, format));
                capture->ReleaseBuffer(frames);
            }
            peak.store(std::min(1.0f, packetPeak));
        }

        client->Stop();
        release(capture);
        release(client);
        release(device);
        CoTaskMemFree(format);
        if (audioEvent)
            CloseHandle(audioEvent);
        enumerator->Release();
    }
};

AudioLevelMonitor::AudioLevelMonitor()
    : d(new Impl)
{
}

AudioLevelMonitor::~AudioLevelMonitor() = default;

bool AudioLevelMonitor::listen(const QString& deviceName)
{
    d->stop();
    d->stopEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    if (!d->stopEvent)
        return false;

    d->running.store(true);
    d->thread = std::thread([this, deviceName] { d->run(deviceName); });

    for (int i = 0; i < 160 && !d->opened.load(); ++i) {
        if (!d->thread.joinable())
            break;
        Sleep(10);
    }
    if (!d->opened.load()) {
        d->stop();
        return false;
    }
    return true;
}

void AudioLevelMonitor::stop()
{
    d->stop();
}

float AudioLevelMonitor::peak() const
{
    return d->peak.load();
}

// --------------------------------------------------- system audio capture --
struct SystemAudioCapture::Impl
{
    std::thread thread;
    std::atomic<bool> running{false};
    std::atomic<bool> opened{false};
    std::atomic<bool> paused{false};
    HANDLE stopEvent = nullptr;
    QString path;
    Mode mode = Mode::AllApps;
    quint32 processId = 0;
    std::promise<QString> ready;

    ~Impl() { stop(); }

    void stop()
    {
        running.store(false);
        if (stopEvent)
            SetEvent(stopEvent);
        if (thread.joinable())
            thread.join();
        if (stopEvent) {
            CloseHandle(stopEvent);
            stopEvent = nullptr;
        }
        opened.store(false);
        paused.store(false);
    }

    IAudioClient* openClient(QString& error) const
    {
        if (mode == Mode::Application) {
            if (processId == 0) {
                error = QStringLiteral("no app audio source was selected");
                return nullptr;
            }
            IAudioClient* client = activateProcessLoopback(processId, error);
            if (client)
                return client;

            // The process-loopback device is Windows 10 2004 and later. A
            // system-wide track is better than an app's recording silently
            // losing the sound it was meant to capture.
            return nullptr;
        }

        IMMDeviceEnumerator* enumerator = nullptr;
        if (FAILED(CoCreateInstance(__uuidof(MMDeviceEnumerator),
                                    nullptr,
                                    CLSCTX_ALL,
                                    __uuidof(IMMDeviceEnumerator),
                                    reinterpret_cast<void**>(&enumerator)))
            || !enumerator) {
            error = QStringLiteral("the default output could not be opened");
            return nullptr;
        }
        IMMDevice* device = defaultEndpoint(enumerator, eRender);
        IAudioClient* client = nullptr;
        if (!device
            || FAILED(device->Activate(__uuidof(IAudioClient),
                                       CLSCTX_ALL,
                                       nullptr,
                                       reinterpret_cast<void**>(&client)))) {
            error = QStringLiteral("the default output could not be opened");
        }
        release(device);
        enumerator->Release();
        return client;
    }

    void run()
    {
        ComScope com(COINIT_MULTITHREADED);
        if (!com.owns()) {
            ready.set_value(QStringLiteral("COM could not be started for audio capture"));
            return;
        }

        QFile file(path);
        if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
            ready.set_value(QStringLiteral("the system audio track could not be written"));
            return;
        }

        QString error;
        IAudioClient* client = openClient(error);
        if (!client) {
            ready.set_value(error);
            file.close();
            QFile::remove(path);
            return;
        }

        HANDLE audioEvent = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        IAudioCaptureClient* capture = nullptr;
        if (!initialiseLoopbackClient(client, &capture, audioEvent, error)) {
            release(capture);
            release(client);
            CloseHandle(audioEvent);
            file.close();
            QFile::remove(path);
            ready.set_value(error);
            return;
        }

        const WAVEFORMATEX format = loopbackFormat();
        writeWaveHeader(file, &format, 0);
        opened.store(true);
        ready.set_value(QString());

        quint64 dataBytes = 0;
        HANDLE events[2] = {stopEvent, audioEvent};
        while (running.load()) {
            const DWORD waited = WaitForMultipleObjects(2, events, FALSE, 100);
            if (waited == WAIT_OBJECT_0)
                break;
            if (waited != WAIT_OBJECT_0 + 1)
                continue;

            UINT32 packet = 0;
            while (SUCCEEDED(capture->GetNextPacketSize(&packet)) && packet > 0) {
                BYTE* data = nullptr;
                UINT32 frames = 0;
                DWORD flags = 0;
                if (FAILED(capture->GetBuffer(&data, &frames, &flags, nullptr, nullptr)))
                    break;
                if (!paused.load() && frames > 0) {
                    const quint32 bytes = frames * format.nBlockAlign;
                    if (flags & AUDCLNT_BUFFERFLAGS_SILENT)
                        file.write(QByteArray(static_cast<int>(bytes), '\0'));
                    else if (data)
                        file.write(reinterpret_cast<const char*>(data), bytes);
                    dataBytes += bytes;
                }
                capture->ReleaseBuffer(frames);
            }
        }

        client->Stop();
        release(capture);
        release(client);
        if (audioEvent)
            CloseHandle(audioEvent);

        patchWaveHeader(file, static_cast<quint32>(std::min<quint64>(dataBytes, 0xFFFFFFFFu)));
        file.close();
    }
};

SystemAudioCapture::SystemAudioCapture()
    : d(new Impl)
{
}

SystemAudioCapture::~SystemAudioCapture() = default;

bool SystemAudioCapture::start(Mode mode,
                               quint32 processId,
                               const QString& wavPath,
                               QString* error)
{
    d->stop();
    if (wavPath.isEmpty()) {
        if (error)
            *error = QStringLiteral("no system audio path was given");
        return false;
    }

    d->path = wavPath;
    d->mode = mode;
    d->processId = processId;
    d->paused.store(false);
    d->opened.store(false);
    d->stopEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    if (!d->stopEvent) {
        if (error)
            *error = QStringLiteral("the audio capture event could not be created");
        return false;
    }

    d->ready = std::promise<QString>();
    std::future<QString> result = d->ready.get_future();
    d->running.store(true);
    d->thread = std::thread([this] { d->run(); });

    if (result.wait_for(std::chrono::seconds(4)) != std::future_status::ready) {
        d->stop();
        if (error)
            *error = QStringLiteral("audio capture did not start in time");
        return false;
    }

    const QString failure = result.get();
    if (!failure.isEmpty()) {
        d->stop();
        QFile::remove(wavPath);
        if (error)
            *error = failure;
        return false;
    }
    return true;
}

void SystemAudioCapture::setPaused(bool paused)
{
    d->paused.store(paused);
}

void SystemAudioCapture::stop()
{
    d->stop();
}

bool SystemAudioCapture::isActive() const
{
    return d->opened.load();
}

QString SystemAudioCapture::path() const
{
    return d->path;
}

}  // namespace Platform
