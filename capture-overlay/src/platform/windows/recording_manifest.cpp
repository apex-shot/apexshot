#include "platform/windows/recording_manifest.h"

#include "platform/windows/pointer_tracker.h"

#include <QDir>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonValue>
#include <QSaveFile>

#include <iterator>

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <dwmapi.h>

namespace Platform {
namespace {

QJsonValue rectJson(const QRect& rect)
{
    if (!rect.isValid())
        return QJsonValue(QJsonValue::Null);
    return QJsonObject{{QStringLiteral("x"), rect.x()},
                       {QStringLiteral("y"), rect.y()},
                       {QStringLiteral("w"), rect.width()},
                       {QStringLiteral("h"), rect.height()}};
}

} // namespace

QJsonObject recordingManifestJson(const RecordingManifest& manifest)
{
    QJsonArray sessions;
    for (const RecordingManifest::Session& session : manifest.sessions) {
        QJsonObject entry{{QStringLiteral("startedAtUnixMs"), session.startedAtUnixMs},
                          {QStringLiteral("endedAtUnixMs"), session.endedAtUnixMs},
                          {QStringLiteral("startMs"), session.startMs},
                          {QStringLiteral("endMs"), session.endMs}};
        if (session.windowBounds.isValid())
            entry.insert(QStringLiteral("windowBounds"), rectJson(session.windowBounds));
        sessions.append(entry);
    }

    QJsonArray markers;
    for (const qint64 marker : manifest.markers)
        markers.append(marker);

    QJsonObject root{
        {QStringLiteral("version"), 1},
        {QStringLiteral("startedAtUnixMs"), manifest.startedAtUnixMs},
        {QStringLiteral("finishedAtUnixMs"), manifest.finishedAtUnixMs},
        {QStringLiteral("durationMs"), manifest.durationMs},
        {QStringLiteral("target"), manifest.target},
        {QStringLiteral("region"), rectJson(manifest.region)},
        {QStringLiteral("fps"), manifest.fps},
        {QStringLiteral("displayScale"), manifest.displayScale},
        {QStringLiteral("displayRefreshRate"), manifest.displayRefreshRate},
        {QStringLiteral("output"),
         QJsonObject{{QStringLiteral("width"), manifest.outputSize.width()},
                     {QStringLiteral("height"), manifest.outputSize.height()}}},
        {QStringLiteral("encoder"), manifest.encoder},
        {QStringLiteral("crf"), manifest.crf},
        {QStringLiteral("captureBackend"), manifest.captureBackend},
        {QStringLiteral("audio"),
         QJsonObject{{QStringLiteral("microphone"), manifest.microphone},
                     {QStringLiteral("microphoneDevice"), manifest.microphoneDevice},
                     {QStringLiteral("systemAudio"), manifest.systemAudio},
                     {QStringLiteral("systemAudioApplication"), manifest.systemAudioApplication},
                     {QStringLiteral("mono"), manifest.monoAudio}}},
        {QStringLiteral("cursor"), QJsonObject{{QStringLiteral("keys"), manifest.keys}}},
        {QStringLiteral("sessions"), sessions},
        {QStringLiteral("markers"), markers},
    };

    if (manifest.hasWindow) {
        root.insert(QStringLiteral("window"),
                    QJsonObject{{QStringLiteral("id"),
                                 QString::number(static_cast<qulonglong>(manifest.windowId))},
                                {QStringLiteral("title"), manifest.windowTitle},
                                {QStringLiteral("app"), manifest.windowApp},
                                {QStringLiteral("bounds"), rectJson(manifest.windowBounds)}});
    } else {
        root.insert(QStringLiteral("window"), QJsonValue(QJsonValue::Null));
    }

    if (manifest.hasCamera) {
        root.insert(QStringLiteral("camera"),
                    QJsonObject{{QStringLiteral("device"), manifest.cameraDevice},
                                {QStringLiteral("width"), manifest.cameraWidth},
                                {QStringLiteral("height"), manifest.cameraHeight},
                                {QStringLiteral("fps"), manifest.cameraFps},
                                {QStringLiteral("startMs"), manifest.cameraStartMs},
                                {QStringLiteral("durationMs"), manifest.cameraDurationMs},
                                {QStringLiteral("processDelayMs"),
                                 manifest.cameraProcessDelayMs}});
    } else {
        root.insert(QStringLiteral("camera"), QJsonValue(QJsonValue::Null));
    }
    return root;
}

QString recordingManifestPathFor(const QString& videoPath)
{
    const QString sidecar = sidecarPathFor(videoPath);
    return sidecar.isEmpty() ? QString() : sidecar + QStringLiteral(".recording.json");
}

bool writeRecordingManifest(const QString& videoPath,
                            const RecordingManifest& manifest,
                            QString* error)
{
    const QString path = recordingManifestPathFor(videoPath);
    if (path.isEmpty()) {
        if (error)
            *error = QStringLiteral("The recording has no sidecar location.");
        return false;
    }
    QDir().mkpath(QFileInfo(path).absolutePath());
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly)) {
        if (error)
            *error = file.errorString();
        return false;
    }
    file.write(QJsonDocument(recordingManifestJson(manifest)).toJson(QJsonDocument::Indented));
    if (!file.commit()) {
        if (error)
            *error = file.errorString();
        return false;
    }
    return true;
}

WindowIdentity windowIdentity(quintptr windowId)
{
    WindowIdentity identity;
    HWND window = reinterpret_cast<HWND>(windowId);
    if (!window || !IsWindow(window))
        return identity;

    wchar_t title[512] = {};
    const int length = GetWindowTextW(window, title, static_cast<int>(std::size(title)));
    if (length > 0)
        identity.title = QString::fromWCharArray(title, length);

    DWORD processId = 0;
    GetWindowThreadProcessId(window, &processId);
    if (processId != 0) {
        HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, processId);
        if (process) {
            wchar_t image[MAX_PATH] = {};
            DWORD size = MAX_PATH;
            if (QueryFullProcessImageNameW(process, 0, image, &size) && size > 0)
                identity.app = QFileInfo(QString::fromWCharArray(image, int(size))).fileName();
            CloseHandle(process);
        }
    }

    RECT frame{};
    if (SUCCEEDED(DwmGetWindowAttribute(window, DWMWA_EXTENDED_FRAME_BOUNDS, &frame, sizeof(frame)))
        || GetWindowRect(window, &frame)) {
        identity.bounds = QRect(QPoint(frame.left, frame.top),
                                QSize(frame.right - frame.left, frame.bottom - frame.top));
    }
    return identity;
}

int displayRefreshRateAt(const QPoint& point)
{
    HMONITOR monitor = MonitorFromPoint(POINT{point.x(), point.y()}, MONITOR_DEFAULTTONEAREST);
    MONITORINFOEXW info{};
    info.cbSize = sizeof(info);
    if (!monitor || !GetMonitorInfoW(monitor, &info))
        return 0;
    DEVMODEW mode{};
    mode.dmSize = sizeof(mode);
    if (!EnumDisplaySettingsW(info.szDevice, ENUM_CURRENT_SETTINGS, &mode))
        return 0;
    return mode.dmDisplayFrequency > 1 ? static_cast<int>(mode.dmDisplayFrequency) : 0;
}

} // namespace Platform
