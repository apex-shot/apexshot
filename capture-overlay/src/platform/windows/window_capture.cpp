#include "platform/windows/window_capture.h"

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif

#include <windows.h>

#include <d3d11.h>
#include <dxgi.h>
#include <inspectable.h>
#include <roapi.h>
#include <windows.graphics.capture.interop.h>
#include <windows.graphics.directx.direct3d11.interop.h>

#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Graphics.Capture.h>
#include <winrt/Windows.Graphics.DirectX.Direct3D11.h>
#include <winrt/Windows.Graphics.DirectX.h>
#include <winrt/Windows.Graphics.h>
#include <winrt/base.h>

#include <QUuid>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstring>
#include <mutex>
#include <thread>
#include <vector>

namespace {

QString hresultText(const char* what, HRESULT result)
{
    return QStringLiteral("%1 failed (0x%2)")
        .arg(QString::fromLatin1(what))
        .arg(static_cast<qulonglong>(result), 8, 16, QLatin1Char('0'));
}

QString winrtText(const char* what, const winrt::hresult_error& error)
{
    return QStringLiteral("%1 failed: %2")
        .arg(QString::fromLatin1(what), QString::fromWCharArray(error.message().c_str()));
}

QString lastErrorText(const char* what)
{
    return QStringLiteral("%1 failed (Win32 error %2)")
        .arg(QString::fromLatin1(what))
        .arg(GetLastError());
}

bool writeAll(HANDLE pipe, const uint8_t* data, size_t size)
{
    while (size > 0) {
        const DWORD chunk = static_cast<DWORD>(std::min<size_t>(size, 1u << 20));
        DWORD written = 0;
        if (!WriteFile(pipe, data, chunk, &written, nullptr) || written == 0)
            return false;
        data += written;
        size -= written;
    }
    return true;
}

struct CaptureState {
    winrt::com_ptr<ID3D11Device> device;
    winrt::com_ptr<ID3D11DeviceContext> context;
    winrt::com_ptr<ID3D11Texture2D> staging;
    UINT stagingWidth = 0;
    UINT stagingHeight = 0;

    std::mutex d3dMutex;
    std::mutex frameMutex;
    std::vector<uint8_t> latest;
    std::vector<uint8_t> last;
    bool hasFrame = false;

    std::mutex errorMutex;
    QString error;
    bool loggedSizeMismatch = false;

    HANDLE pipe = INVALID_HANDLE_VALUE;
    int targetWidth = 0;
    int targetHeight = 0;
    int fps = 30;
    std::atomic<bool> running{true};

    void fail(const QString& message)
    {
        std::lock_guard<std::mutex> lock(errorMutex);
        if (error.isEmpty())
            error = message;
        running = false;
    }

    void copyFrame(const winrt::Windows::Graphics::Capture::Direct3D11CaptureFrame& frame)
    {
        if (!running)
            return;

        const winrt::Windows::Graphics::SizeInt32 content = frame.ContentSize();
        if (content.Width <= 0 || content.Height <= 0)
            return;

        const auto surface = frame.Surface();
        const auto access =
            surface.as<::Windows::Graphics::DirectX::Direct3D11::IDirect3DDxgiInterfaceAccess>();
        winrt::com_ptr<ID3D11Texture2D> texture;
        winrt::check_hresult(access->GetInterface(IID_PPV_ARGS(texture.put())));

        D3D11_TEXTURE2D_DESC description{};
        texture->GetDesc(&description);
        if (description.Width == 0 || description.Height == 0)
            return;

        std::lock_guard<std::mutex> d3dLock(d3dMutex);
        if (!staging || stagingWidth != description.Width || stagingHeight != description.Height) {
            D3D11_TEXTURE2D_DESC stagingDescription{};
            stagingDescription.Width = description.Width;
            stagingDescription.Height = description.Height;
            stagingDescription.MipLevels = 1;
            stagingDescription.ArraySize = 1;
            stagingDescription.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
            stagingDescription.SampleDesc.Count = 1;
            stagingDescription.Usage = D3D11_USAGE_STAGING;
            stagingDescription.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
            staging = nullptr;
            winrt::check_hresult(
                device->CreateTexture2D(&stagingDescription, nullptr, staging.put()));
            stagingWidth = description.Width;
            stagingHeight = description.Height;
        }

        context->CopyResource(staging.get(), texture.get());
        D3D11_MAPPED_SUBRESOURCE mapped{};
        winrt::check_hresult(context->Map(staging.get(), 0, D3D11_MAP_READ, 0, &mapped));

        const size_t targetBytes =
            static_cast<size_t>(targetWidth) * static_cast<size_t>(targetHeight) * 4;
        {
            std::lock_guard<std::mutex> frameLock(frameMutex);
            if (latest.size() != targetBytes)
                latest.resize(targetBytes);

            const UINT copyWidth =
                std::min<UINT>(static_cast<UINT>(content.Width), static_cast<UINT>(targetWidth));
            const UINT copyHeight =
                std::min<UINT>(static_cast<UINT>(content.Height), static_cast<UINT>(targetHeight));
            const size_t targetRow = static_cast<size_t>(targetWidth) * 4;
            const size_t sourceRow = static_cast<size_t>(copyWidth) * 4;

            if (copyWidth != static_cast<UINT>(targetWidth)
                || copyHeight != static_cast<UINT>(targetHeight)) {
                std::fill(latest.begin(), latest.end(), 0);
                if (content.Width != targetWidth || content.Height != targetHeight) {
                    std::lock_guard<std::mutex> errorLock(errorMutex);
                    if (!loggedSizeMismatch) {
                        loggedSizeMismatch = true;
                        error = QStringLiteral(
                                    "the window is %1x%2 but the recording is %3x%4; "
                                    "the frame is cropped or letterboxed")
                                    .arg(content.Width)
                                    .arg(content.Height)
                                    .arg(targetWidth)
                                    .arg(targetHeight);
                    }
                }
            }

            const auto* source = static_cast<const uint8_t*>(mapped.pData);
            for (UINT row = 0; row < copyHeight; ++row) {
                std::memcpy(latest.data() + row * targetRow,
                            source + static_cast<size_t>(row) * mapped.RowPitch,
                            sourceRow);
            }
            hasFrame = true;
        }

        context->Unmap(staging.get(), 0);
    }
};

} // namespace

namespace Platform {

struct WindowCaptureSource::Impl {
    quintptr windowId = 0;
    QSize size;
    int fps = 30;
    QString pipePath;
    HANDLE pipe = INVALID_HANDLE_VALUE;
    std::shared_ptr<CaptureState> state;
    std::thread thread;

    std::mutex initMutex;
    std::condition_variable initChanged;
    bool initDone = false;
    bool initOk = false;
    QString initError;

    void signalInit(bool ok, const QString& error)
    {
        {
            std::lock_guard<std::mutex> lock(initMutex);
            initDone = true;
            initOk = ok;
            initError = error;
        }
        initChanged.notify_all();
    }
};

} // namespace

namespace Platform {

WindowCaptureSource::WindowCaptureSource()
    : d(std::make_unique<Impl>())
{
}

WindowCaptureSource::~WindowCaptureSource()
{
    stop();
}

bool WindowCaptureSource::supported(QString* error)
{
    static const QString supportError = [] {
        try {
            if (!winrt::Windows::Graphics::Capture::GraphicsCaptureSession::IsSupported())
                return QStringLiteral("Window Graphics Capture is not supported");
            return QString();
        } catch (const winrt::hresult_error& failure) {
            return winrtText("Window Graphics Capture", failure);
        }
    }();
    if (!supportError.isEmpty()) {
        if (error)
            *error = supportError;
        return false;
    }
    return true;
}

bool WindowCaptureSource::prepare(quintptr windowId, const QSize& size, int fps, QString* error)
{
    stop();
    d->windowId = windowId;
    d->size = size;
    d->fps = std::max(1, fps);

    if (!windowId || !IsWindow(reinterpret_cast<HWND>(windowId))) {
        if (error)
            *error = QStringLiteral("the window is no longer available");
        return false;
    }
    if (size.width() < 2 || size.height() < 2) {
        if (error)
            *error = QStringLiteral("the window is too small to capture");
        return false;
    }
    if (!supported(error))
        return false;

    const QString suffix =
        QUuid::createUuid().toString(QUuid::WithoutBraces).remove(QLatin1Char('-'));
    d->pipePath = QStringLiteral("\\\\.\\pipe\\apexshot-window-%1-%2")
                      .arg(GetCurrentProcessId())
                      .arg(suffix);
    const std::wstring pipeName = d->pipePath.toStdWString();
    d->pipe = CreateNamedPipeW(pipeName.c_str(),
                               PIPE_ACCESS_OUTBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                               PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                               1,
                               4 * 1024 * 1024,
                               0,
                               0,
                               nullptr);
    if (d->pipe == INVALID_HANDLE_VALUE) {
        if (error)
            *error = lastErrorText("CreateNamedPipe");
        return false;
    }
    return true;
}

bool WindowCaptureSource::start(QString* error)
{
    if (!isPrepared()) {
        if (error)
            *error = QStringLiteral("the window capture source is not prepared");
        return false;
    }
    if (d->thread.joinable())
        return true;

    auto state = std::make_shared<CaptureState>();
    state->pipe = d->pipe;
    state->targetWidth = d->size.width();
    state->targetHeight = d->size.height();
    state->fps = d->fps;
    state->latest.resize(static_cast<size_t>(state->targetWidth) * state->targetHeight * 4);
    d->state = state;

    {
        std::lock_guard<std::mutex> lock(d->initMutex);
        d->initDone = false;
        d->initOk = false;
        d->initError.clear();
    }

    Impl* impl = d.get();
    d->thread = std::thread([impl, state] {
        try {
            winrt::init_apartment(winrt::apartment_type::multi_threaded);

            const HRESULT deviceResult =
                D3D11CreateDevice(nullptr,
                                  D3D_DRIVER_TYPE_HARDWARE,
                                  nullptr,
                                  D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                                  nullptr,
                                  0,
                                  D3D11_SDK_VERSION,
                                  state->device.put(),
                                  nullptr,
                                  state->context.put());
            if (FAILED(deviceResult)) {
                impl->signalInit(false, hresultText("D3D11CreateDevice", deviceResult));
                return;
            }

            const auto dxgiDevice = state->device.as<IDXGIDevice>();
            winrt::com_ptr<IInspectable> inspectable;
            winrt::check_hresult(
                CreateDirect3D11DeviceFromDXGIDevice(dxgiDevice.get(), inspectable.put()));
            const auto direct3DDevice =
                inspectable.as<winrt::Windows::Graphics::DirectX::Direct3D11::IDirect3DDevice>();

            auto interop =
                winrt::get_activation_factory<winrt::Windows::Graphics::Capture::GraphicsCaptureItem,
                                              IGraphicsCaptureItemInterop>();
            winrt::Windows::Graphics::Capture::GraphicsCaptureItem item{nullptr};
            winrt::check_hresult(interop->CreateForWindow(
                reinterpret_cast<HWND>(impl->windowId),
                winrt::guid_of<ABI::Windows::Graphics::Capture::IGraphicsCaptureItem>(),
                winrt::put_abi(item)));
            if (!item)
                throw winrt::hresult_error(E_FAIL, L"the window has no capture item");

            auto framePool =
                winrt::Windows::Graphics::Capture::Direct3D11CaptureFramePool::CreateFreeThreaded(
                    direct3DDevice,
                    winrt::Windows::Graphics::DirectX::DirectXPixelFormat::B8G8R8A8UIntNormalized,
                    2,
                    item.Size());
            auto session = framePool.CreateCaptureSession(item);
            session.IsCursorCaptureEnabled(false);

            const auto handler =
                [state](const winrt::Windows::Graphics::Capture::Direct3D11CaptureFramePool& sender,
                        const winrt::Windows::Foundation::IInspectable&) {
                    try {
                        const auto frame = sender.TryGetNextFrame();
                        if (frame)
                            state->copyFrame(frame);
                    } catch (const winrt::hresult_error& failure) {
                        state->fail(winrtText("reading a captured frame", failure));
                    } catch (...) {
                        state->fail(QStringLiteral("reading a captured frame failed"));
                    }
                };
            const auto token = framePool.FrameArrived(handler);

            impl->signalInit(true, QString());

            const BOOL connected = ConnectNamedPipe(state->pipe, nullptr);
            if (!connected && GetLastError() != ERROR_PIPE_CONNECTED) {
                state->fail(lastErrorText("ConnectNamedPipe"));
            } else {
                session.StartCapture();
                const auto interval = std::chrono::microseconds(
                    1000000 / std::max(1, state->fps));
                auto next = std::chrono::steady_clock::now();
                std::vector<uint8_t> frame;
                while (state->running) {
                    next += interval;
                    std::this_thread::sleep_until(next);
                    if (!state->running)
                        break;

                    {
                        std::lock_guard<std::mutex> lock(state->frameMutex);
                        if (state->hasFrame) {
                            frame.swap(state->latest);
                            state->hasFrame = false;
                        } else {
                            frame.swap(state->last);
                        }
                    }
                    if (frame.empty())
                        continue;
                    if (!writeAll(state->pipe, frame.data(), frame.size())) {
                        state->fail(lastErrorText("WriteFile"));
                        break;
                    }
                    {
                        std::lock_guard<std::mutex> lock(state->frameMutex);
                        state->last.swap(frame);
                    }
                    if (std::chrono::steady_clock::now() > next + interval * 2)
                        next = std::chrono::steady_clock::now();
                }
            }

            framePool.FrameArrived(token);
            session.Close();
            framePool.Close();
        } catch (const winrt::hresult_error& failure) {
            impl->signalInit(false, winrtText("starting window capture", failure));
        } catch (...) {
            impl->signalInit(false, QStringLiteral("starting window capture failed"));
        }

        if (state->pipe != INVALID_HANDLE_VALUE) {
            CloseHandle(state->pipe);
            state->pipe = INVALID_HANDLE_VALUE;
        }
    });

    QString failure;
    {
        std::unique_lock<std::mutex> lock(d->initMutex);
        if (!d->initChanged.wait_for(lock, std::chrono::seconds(5), [this] { return d->initDone; }))
            failure = QStringLiteral("window capture did not initialize");
        else if (!d->initOk)
            failure = d->initError;
    }
    if (!failure.isEmpty()) {
        if (error)
            *error = failure;
        stop();
        return false;
    }
    return true;
}

void WindowCaptureSource::stop()
{
    if (d->state)
        d->state->running = false;
    if (d->pipe != INVALID_HANDLE_VALUE)
        CancelIoEx(d->pipe, nullptr);
    if (d->thread.joinable())
        d->thread.join();
    d->pipe = INVALID_HANDLE_VALUE;
    d->state.reset();
}

bool WindowCaptureSource::isPrepared() const
{
    return d->pipe != INVALID_HANDLE_VALUE;
}

QString WindowCaptureSource::pipePath() const
{
    return d->pipePath;
}

QSize WindowCaptureSource::size() const
{
    return d->size;
}

QString WindowCaptureSource::lastError() const
{
    if (!d->state)
        return {};
    std::lock_guard<std::mutex> lock(d->state->errorMutex);
    return d->state->error;
}

} // namespace Platform
