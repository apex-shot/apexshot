// Windows-only smoke test for the ffmpeg-backed recorder.
//
// Records a small desktop region twice: once straight through, once with a
// pause and a resume. Each run must produce a non-empty MP4. Intentional
// console output; this is not part of the shipped binary.

#include "platform/platform.h"
#include "platform/recording.h"
#include "platform/windows/disk_space.h"
#include "platform/windows/pointer_tracker.h"
#include "platform/windows/recording_manifest.h"
#include "platform/windows/webcam_layout.h"

#include <QCoreApplication>
#include <QDir>
#include <QElapsedTimer>
#include <QEventLoop>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QImage>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QPainter>
#include <QProcess>
#include <QRasterWindow>
#include <QRegularExpression>
#include <QSet>
#include <QStandardPaths>
#include <QStringList>
#include <QTemporaryDir>
#include <QTimer>
#include <QUuid>

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <dwmapi.h>
#include <cstdio>
#include <memory>

namespace {

const QRect kRegion(24, 24, 320, 240);

bool moveCursor(const QPoint& position)
{
    return SetCursorPos(position.x(), position.y()) != 0;
}

bool setLeftButton(bool down)
{
    INPUT input{};
    input.type = INPUT_MOUSE;
    input.mi.dwFlags = down ? MOUSEEVENTF_LEFTDOWN : MOUSEEVENTF_LEFTUP;
    return SendInput(1, &input, sizeof(input)) == 1;
}

bool sendKey(WORD key, bool down)
{
    INPUT input{};
    input.type = INPUT_KEYBOARD;
    input.ki.wVk = key;
    input.ki.dwFlags = down ? 0 : KEYEVENTF_KEYUP;
    return SendInput(1, &input, sizeof(input)) == 1;
}

bool sendWheel(int delta)
{
    INPUT input{};
    input.type = INPUT_MOUSE;
    input.mi.dwFlags = MOUSEEVENTF_WHEEL;
    input.mi.mouseData = static_cast<DWORD>(delta);
    return SendInput(1, &input, sizeof(input)) == 1;
}

/// A cursor an application installed itself: a handle that is none of the
/// standard system cursors, which is what a browser's custom hand or a
/// toolkit's drawn resize handle reports.
HCURSOR createApplicationCursor()
{
    ICONINFO info{};
    info.fIcon = FALSE;
    info.hbmMask = CreateBitmap(32, 32, 1, 1, nullptr);
    info.hbmColor = CreateBitmap(32, 32, 1, 1, nullptr);
    HCURSOR cursor = info.hbmMask && info.hbmColor ? CreateIconIndirect(&info) : nullptr;
    if (info.hbmMask)
        DeleteObject(info.hbmMask);
    if (info.hbmColor)
        DeleteObject(info.hbmColor);
    return cursor;
}

/// The sidecar's kind string is what the editor parses, so it must be one of
/// the shared names and nothing else.
bool checkCursorKinds()
{
    struct Expectation {
        LPCSTR id;
        const char* name;
        const char* kind;
    };
    const Expectation expectations[] = {
        {IDC_HAND, "IDC_HAND", "hand"},
        {IDC_IBEAM, "IDC_IBEAM", "text"},
        {IDC_CROSS, "IDC_CROSS", "crosshair"},
        {IDC_WAIT, "IDC_WAIT", "wait"},
        {IDC_APPSTARTING, "IDC_APPSTARTING", "wait"},
        {IDC_SIZEWE, "IDC_SIZEWE", "resize"},
        {IDC_SIZENS, "IDC_SIZENS", "resize"},
        {IDC_SIZENWSE, "IDC_SIZENWSE", "resize"},
        {IDC_SIZENESW, "IDC_SIZENESW", "resize"},
        {IDC_SIZEALL, "IDC_SIZEALL", "resize"},
        // Standard cursors with no shared shape stay on the neutral kind.
        {IDC_ARROW, "IDC_ARROW", "default"},
        {IDC_UPARROW, "IDC_UPARROW", "default"},
        {IDC_NO, "IDC_NO", "default"},
    };
    for (const Expectation& expectation : expectations) {
        // The build defines no UNICODE, so the IDC_* macros are char resource
        // pointers and the ANSI entry point is the one that takes them directly.
        const Platform::CursorKind actual =
            Platform::cursorKindFor(LoadCursorA(nullptr, expectation.id));
        const QString kind = Platform::cursorKindName(actual);
        if (kind != QLatin1String(expectation.kind)) {
            std::printf("FAIL %s maps to %s, expected %s\n",
                        expectation.name,
                        qPrintable(kind),
                        expectation.kind);
            return false;
        }
    }

    if (Platform::cursorKindFor(nullptr) != Platform::CursorKind::Default) {
        std::printf("FAIL a null cursor handle is not default\n");
        return false;
    }

    const HCURSOR applicationCursor = createApplicationCursor();
    if (applicationCursor) {
        const QString kind = Platform::cursorKindName(Platform::cursorKindFor(applicationCursor));
        DestroyCursor(applicationCursor);
        if (kind != QStringLiteral("default")) {
            std::printf("FAIL an application cursor maps to %s, expected default\n",
                        qPrintable(kind));
            return false;
        }
    } else {
        std::printf("SKIP: could not create an application cursor for the default check\n");
    }

    std::printf("CURSOR_KINDS=ok\n");
    return true;
}

/// The manifest JSON: two sessions, a camera, and no window.
bool checkRecordingManifestJson()
{
    Platform::RecordingManifest manifest;
    manifest.target = QStringLiteral("area");
    manifest.region = QRect(10, 20, 300, 200);
    manifest.fps = 30;
    manifest.encoder = QStringLiteral("libx264");
    manifest.hasCamera = true;
    manifest.cameraWidth = 1280;
    manifest.cameraHeight = 720;
    manifest.cameraStartMs = 120;
    manifest.devicePixelRatio = 1.5;
    Platform::RecordingManifest::Session first;
    first.startMs = 0;
    first.endMs = 800;
    Platform::RecordingManifest::Session second;
    second.startMs = 800;
    second.endMs = 1800;
    manifest.sessions = {first, second};

    const QJsonObject json = Platform::recordingManifestJson(manifest);
    const QJsonArray sessions = json.value(QStringLiteral("sessions")).toArray();
    const QJsonObject region = json.value(QStringLiteral("region")).toObject();
    const QJsonObject camera = json.value(QStringLiteral("camera")).toObject();
    const bool ok = json.value(QStringLiteral("version")).toInt() == 1
        && json.value(QStringLiteral("target")).toString() == QStringLiteral("area")
        && region.value(QStringLiteral("w")).toInt() == 300
        && json.value(QStringLiteral("devicePixelRatio")).toDouble() == 1.5
        && sessions.size() == 2
        && sessions.at(1).toObject().value(QStringLiteral("startMs")).toInt() == 800
        && !sessions.at(0).toObject().contains(QStringLiteral("windowBounds"))
        && camera.value(QStringLiteral("width")).toInt() == 1280
        && camera.value(QStringLiteral("startMs")).toInt() == 120
        && json.value(QStringLiteral("window")).isNull();
    if (!ok) {
        std::printf("FAIL manifest json: %s\n",
                    QJsonDocument(json).toJson(QJsonDocument::Compact).constData());
        return false;
    }
    std::printf("OK manifest json\n");
    return true;
}

bool validateSidecar(const QString& videoPath, double recordedSeconds)
{
    const QString canonicalPath = Platform::canonicalVideoPathFor(videoPath);
    const QString path = Platform::sidecarPathFor(videoPath);
    if (!canonicalPath.startsWith(QStringLiteral("\\\\?\\"))) {
        std::printf("FAIL canonical path lacks the Windows verbatim prefix: %s\n",
                    qPrintable(canonicalPath));
        return false;
    }
    if (path.isEmpty() || !QFileInfo::exists(path)) {
        std::printf("FAIL sidecar does not exist: %s\n", qPrintable(path));
        return false;
    }

    const QFileInfo sidecarInfo(path);
    // A tagged recording keeps its pointer data in a folder named after its
    // ID; an older video still resolves through the path hash. Both shapes are
    // recognised, and a recording made by this check must be the tagged one.
    static const QRegularExpression hashName(QStringLiteral("^[0-9a-f]{64}$"));
    const bool hashedShape = hashName.match(sidecarInfo.fileName()).hasMatch()
        && sidecarInfo.dir().dirName() == QStringLiteral("pointer-sidecars");
    QDir directory = sidecarInfo.dir();
    const QString recordingId = Platform::recordingIdOf(videoPath);
    if (recordingId.isEmpty()) {
        std::printf("FAIL recording has no recording id tag\n");
        return false;
    }
    const QString taggedPath = Platform::recordingSidecarPathFor(recordingId);
    const bool taggedShape = !taggedPath.isEmpty() && path == taggedPath;
    if (!hashedShape && !taggedShape) {
        std::printf("FAIL sidecar path is neither the hash nor the id form: %s\n",
                    qPrintable(path));
        return false;
    }
    if (!taggedShape) {
        std::printf("FAIL recording kept the hashed sidecar path: %s (expected %s)\n",
                    qPrintable(path),
                    qPrintable(taggedPath));
        return false;
    }
    if (sidecarInfo.fileName() != QStringLiteral("pointer.json")
        || directory.dirName() != recordingId) {
        std::printf("FAIL sidecar id path is malformed: %s\n", qPrintable(path));
        return false;
    }
    directory.cdUp();
    if (directory.dirName() != QStringLiteral("recordings")) {
        std::printf("FAIL sidecar is not under the recordings root: %s\n", qPrintable(path));
        return false;
    }
    directory.cdUp();
    if (directory.dirName() != QStringLiteral("apexshot")
        || QDir::cleanPath(directory.absolutePath())
            != QDir::cleanPath(QDir(qEnvironmentVariable("LOCALAPPDATA"))
                                   .filePath(QStringLiteral("apexshot")))) {
        std::printf("FAIL sidecar is outside LOCALAPPDATA/apexshot: %s\n", qPrintable(path));
        return false;
    }

    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) {
        std::printf("FAIL could not read sidecar: %s\n", qPrintable(path));
        return false;
    }
    const QByteArray bytes = file.readAll();
    QJsonParseError parseError;
    const QJsonDocument document = QJsonDocument::fromJson(bytes, &parseError);
    if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
        std::printf("FAIL sidecar JSON parse: %s\n", qPrintable(parseError.errorString()));
        return false;
    }

    const QJsonObject object = document.object();
    const QJsonObject region = object.value(QStringLiteral("region")).toObject();
    const QJsonArray pointer = object.value(QStringLiteral("pointer")).toArray();
    const QJsonArray clicks = object.value(QStringLiteral("clicks")).toArray();
    const QJsonArray presses = object.value(QStringLiteral("presses")).toArray();
    const QJsonArray visibility = object.value(QStringLiteral("visibility")).toArray();
    const QJsonArray scrolls = object.value(QStringLiteral("scrolls")).toArray();
    const QJsonArray keys = object.value(QStringLiteral("keys")).toArray();
    const QJsonArray cursors = object.value(QStringLiteral("cursors")).toArray();
    const QString cursorFolderName = object.value(QStringLiteral("cursor_images")).toString();
    if (object.value(QStringLiteral("version")).toInt() != 2 || pointer.size() < 4
        || clicks.isEmpty() || presses.isEmpty() || visibility.isEmpty() || cursors.isEmpty()
        || cursorFolderName.isEmpty()
        || QFileInfo(cursorFolderName).fileName() != cursorFolderName
        || !object.value(QStringLiteral("t0_monotonic_us")).isDouble()
        || object.value(QStringLiteral("t0_monotonic_us")).toDouble() <= 0.0
        || region.value(QStringLiteral("x")).toInt() != kRegion.x()
        || region.value(QStringLiteral("y")).toInt() != kRegion.y()
        || region.value(QStringLiteral("w")).toInt() != kRegion.width()
        || region.value(QStringLiteral("h")).toInt() != kRegion.height()
        || object.value(QStringLiteral("source")).toString() != QStringLiteral("recorded")) {
        std::printf("FAIL sidecar version, sample counts, or cursor catalog are invalid\n");
        return false;
    }
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_KEYSTROKES") && keys.size() < 6) {
        std::printf("FAIL keystrokes were lost: keys=%d\n", keys.size());
        return false;
    }
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_SCROLL") && scrolls.size() < 2) {
        std::printf("FAIL scroll events were lost: scrolls=%d\n", scrolls.size());
        return false;
    }
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_RAPID_CLICKS")
        && (clicks.size() < 20 || presses.size() < 20)) {
        std::printf("FAIL rapid clicks were lost: clicks=%d presses=%d\n",
                    clicks.size(),
                    presses.size());
        return false;
    }

    static const QRegularExpression cursorIdPattern(QStringLiteral("^(std|custom)-[0-9a-f]{16}$"));
    const QDir cursorFolder = sidecarInfo.dir().filePath(cursorFolderName);
    QSet<QString> cursorIds;
    for (const QJsonValue& value : cursors) {
        const QJsonObject cursor = value.toObject();
        const QString id = cursor.value(QStringLiteral("id")).toString();
        const QString file = cursor.value(QStringLiteral("file")).toString();
        const int width = cursor.value(QStringLiteral("width")).toInt();
        const int height = cursor.value(QStringLiteral("height")).toInt();
        const int hotspotX = cursor.value(QStringLiteral("hotspot_x")).toInt();
        const int hotspotY = cursor.value(QStringLiteral("hotspot_y")).toInt();
        const QImage image(cursorFolder.filePath(file));
        bool hasVisiblePixel = false;
        for (int y = 0; y < image.height() && !hasVisiblePixel; ++y) {
            const QRgb* line = reinterpret_cast<const QRgb*>(image.constScanLine(y));
            for (int x = 0; x < image.width(); ++x) {
                if (qAlpha(line[x]) != 0) {
                    hasVisiblePixel = true;
                    break;
                }
            }
        }
        if (!cursorIdPattern.match(id).hasMatch() || file.isEmpty() || image.isNull()
            || image.width() != width || image.height() != height || !hasVisiblePixel
            || width <= 0 || height <= 0 || hotspotX < 0 || hotspotX >= width
            || hotspotY < 0 || hotspotY >= height) {
            std::printf("FAIL cursor catalog entry is invalid\n");
            return false;
        }
        cursorIds.insert(id);
    }
    if (clicks.first().toObject().value(QStringLiteral("button")).toInt() != 1
        || presses.first().toObject().value(QStringLiteral("button")).toInt() != 1) {
        std::printf("FAIL left mouse button did not use sidecar button 1\n");
        return false;
    }

    const double latestTime = recordedSeconds + 0.5;
    const auto validTime = [latestTime](double t) {
        return std::isfinite(t) && t >= 0.0 && t <= latestTime;
    };
    const auto validPosition = [](const QJsonObject& sample) {
        const double x = sample.value(QStringLiteral("x")).toDouble();
        const double y = sample.value(QStringLiteral("y")).toDouble();
        return std::isfinite(x) && std::isfinite(y) && x >= -64.0
            && x <= kRegion.width() + 64.0 && y >= -64.0 && y <= kRegion.height() + 64.0;
    };
    for (const QJsonValue& value : visibility) {
        const QJsonObject sample = value.toObject();
        if (!validTime(sample.value(QStringLiteral("t")).toDouble())
            || !sample.value(QStringLiteral("visible")).isBool()) {
            std::printf("FAIL cursor visibility sample is invalid\n");
            return false;
        }
    }
    for (const QJsonValue& value : scrolls) {
        const QJsonObject sample = value.toObject();
        if (!validTime(sample.value(QStringLiteral("t")).toDouble()) || !validPosition(sample)
            || !sample.value(QStringLiteral("delta")).isDouble()
            || !sample.value(QStringLiteral("horizontal")).isBool()) {
            std::printf("FAIL scroll sample is invalid\n");
            return false;
        }
    }
    bool sawModifiedKey = false;
    for (const QJsonValue& value : keys) {
        const QJsonObject sample = value.toObject();
        const QString type = sample.value(QStringLiteral("type")).toString();
        if (!validTime(sample.value(QStringLiteral("t")).toDouble())
            || (type != QStringLiteral("key_down") && type != QStringLiteral("key_up"))
            || !sample.value(QStringLiteral("key")).isDouble()
            || !sample.value(QStringLiteral("scan_code")).isDouble()) {
            std::printf("FAIL key sample is invalid\n");
            return false;
        }
        const QJsonArray modifiers = sample.value(QStringLiteral("modifiers")).toArray();
        if (modifiers.contains(QStringLiteral("shift"))
            && modifiers.contains(QStringLiteral("control"))) {
            sawModifiedKey = true;
        }
    }
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_KEYSTROKES") && !sawModifiedKey) {
        std::printf("FAIL the key stream carries no shift/control modifiers\n");
        return false;
    }
    // The editor's kind parse falls back to default for anything it does not
    // know, so a sample has to carry one of the six shared names exactly.
    static const QStringList sharedKinds = {QStringLiteral("default"),
                                            QStringLiteral("hand"),
                                            QStringLiteral("text"),
                                            QStringLiteral("crosshair"),
                                            QStringLiteral("wait"),
                                            QStringLiteral("resize")};
    int defaultKinds = 0;
    for (const QJsonValue& value : pointer) {
        const QJsonObject sample = value.toObject();
        if (!validTime(sample.value(QStringLiteral("t")).toDouble()) || !validPosition(sample)) {
            std::printf("FAIL pointer sample has an invalid time or position\n");
            return false;
        }
        const QJsonValue kind = sample.value(QStringLiteral("kind"));
        if (!kind.isString() || !sharedKinds.contains(kind.toString())) {
            std::printf("FAIL pointer sample kind is not a shared cursor kind: %s\n",
                        qPrintable(kind.toVariant().toString()));
            return false;
        }
        if (!cursorIds.contains(sample.value(QStringLiteral("cursor_id")).toString())) {
            std::printf("FAIL pointer sample has no cursor catalog entry\n");
            return false;
        }
        if (kind.toString() == QStringLiteral("default"))
            ++defaultKinds;
    }
    for (const QJsonValue& value : clicks) {
        const QJsonObject sample = value.toObject();
        if (!validTime(sample.value(QStringLiteral("t")).toDouble()) || !validPosition(sample)) {
            std::printf("FAIL click sample has an invalid time or position\n");
            return false;
        }
        if (!cursorIds.contains(sample.value(QStringLiteral("cursor_id")).toString())) {
            std::printf("FAIL click sample has no cursor catalog entry\n");
            return false;
        }
    }
    for (const QJsonValue& value : presses) {
        const QJsonObject sample = value.toObject();
        if (!validTime(sample.value(QStringLiteral("down")).toDouble())
            || !validTime(sample.value(QStringLiteral("up")).toDouble())
            || sample.value(QStringLiteral("up")).toDouble()
                < sample.value(QStringLiteral("down")).toDouble()) {
            std::printf("FAIL press interval has an invalid time\n");
            return false;
        }
    }

    QJsonObject excerpt;
    excerpt.insert(QStringLiteral("pointer_first"), pointer.first().toObject());
    excerpt.insert(QStringLiteral("clicks_tail"), clicks.last().toObject());
    excerpt.insert(QStringLiteral("presses_tail"), presses.last().toObject());
    std::printf("SIDECAR_PATH=%s\n", qPrintable(path));
    std::printf("SIDECAR_BYTES=%lld\n", static_cast<long long>(bytes.size()));
    std::printf("SIDECAR_JSON=valid\n");
    std::printf("SIDECAR_COUNTS pointer=%d clicks=%d presses=%d\n",
                pointer.size(),
                clicks.size(),
                presses.size());
    std::printf("SIDECAR_KINDS default=%d shaped=%d\n",
                defaultKinds,
                pointer.size() - defaultKinds);
    std::printf("SIDECAR_EXCERPT:\n%s\n",
                qPrintable(QString::fromUtf8(
                    QJsonDocument(excerpt).toJson(QJsonDocument::Indented))));
    return true;
}

/// Clean up a test recording the way the recorder lays it out: the video, its
/// pointer sidecar and manifest, and the recording ID folder holding them. The
/// sidecar path comes from the MP4 tag, so it is resolved before the video is
/// removed.
void removeRecordingOutputs(const QString& video)
{
    const QString sidecar = Platform::sidecarPathFor(video);
    const QString manifest = Platform::recordingManifestPathFor(video);
    QFile::remove(video);
    if (sidecar.isEmpty())
        return;
    QFile::remove(sidecar);
    QFile::remove(manifest);
    QDir(sidecar + QStringLiteral(".cursors")).removeRecursively();
    // A tagged recording owns a whole folder under the recordings root.
    if (QFileInfo(sidecar).fileName() == QStringLiteral("pointer.json"))
        QDir(QFileInfo(sidecar).absolutePath()).removeRecursively();
}

int recordOnce(bool pauseAndResume)
{
    Platform::RecordingSession session;
    QString path = QDir(QDir::tempPath())
                       .filePath(pauseAndResume
                                     ? QStringLiteral("apexshot-record-pause.mp4")
                                     : QStringLiteral("apexshot-record-basic.mp4"));
    const QString requestedPath = qEnvironmentVariable("APEXSHOT_TEST_OUTPUT").trimmed();
    if (!requestedPath.isEmpty())
        path = requestedPath;
    const QString oldSidecarPath = Platform::sidecarPathFor(path);
    if (!oldSidecarPath.isEmpty()) {
        QFile::remove(oldSidecarPath);
        if (QFileInfo(oldSidecarPath).fileName() == QStringLiteral("pointer.json"))
            QDir(QFileInfo(oldSidecarPath).absolutePath()).removeRecursively();
    }
    QFile::remove(path);
    QDir().mkpath(QFileInfo(path).absolutePath());

    // The test drives the real pointer, so put it back where it was found.
    POINT originalCursor{};
    const bool haveOriginalCursor = GetCursorPos(&originalCursor) != 0;
    const auto restoreCursor = [haveOriginalCursor, originalCursor] {
        if (haveOriginalCursor)
            SetCursorPos(originalCursor.x, originalCursor.y);
    };

    QPoint guardedCursorPosition(kRegion.x() + 80, kRegion.y() + 80);
    if (!pauseAndResume
        && !moveCursor(guardedCursorPosition)) {
        std::printf("SKIP: desktop session refused SetCursorPos input injection\n");
        return 77;
    }

    QRect region = kRegion;
    int fps = 30;
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_FULL_SCREEN")) {
        const QVector<Platform::MonitorInfo> monitors = Platform::monitors();
        if (!monitors.isEmpty()) {
            region = monitors.first().geometry;
            fps = 60;
        }
    }

    Platform::RecordingRequest request;
    request.region = region;
    request.outputPath = path;
    // The editor draws the pointer from the sidecar, so the frames carry none.
    request.includeCursor = false;
    request.captureKeystrokes = !qEnvironmentVariableIsEmpty("APEXSHOT_TEST_KEYSTROKES");
    request.includeMicrophone = !qEnvironmentVariableIsEmpty("APEXSHOT_TEST_MIC");
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_SYSTEM_AUDIO"))
        request.systemAudio = Platform::RecordingRequest::SystemAudio::AllApps;
    request.fps = fps;
    request.crf = 28;
    request.hevc = !qEnvironmentVariableIsEmpty("APEXSHOT_TEST_HEVC");
    const int requestedCrf = qEnvironmentVariableIntValue("APEXSHOT_TEST_CRF");
    if (requestedCrf > 0)
        request.crf = requestedCrf;

    int durationMs = 2400;
    const int requestedSeconds = qEnvironmentVariableIntValue("APEXSHOT_TEST_SECONDS");
    if (!pauseAndResume && requestedSeconds > 0)
        durationMs = requestedSeconds * 1000;

    int result = 1;
    bool inputInjectionRefused = false;
    bool extraInputRefused = false;
    QString inputRefusalReason;
    double recordedSeconds = 0.0;
    QElapsedTimer recordingClock;
    bool leftButtonDownInjected = false;
    QTimer cursorGuard;
    cursorGuard.setTimerType(Qt::PreciseTimer);
    cursorGuard.setInterval(10);
    QObject::connect(&cursorGuard, &QTimer::timeout, &session, [&] {
        if (!moveCursor(guardedCursorPosition)) {
            inputInjectionRefused = true;
            inputRefusalReason = QStringLiteral("SetCursorPos");
            cursorGuard.stop();
            session.discard();
        }
    });
    QEventLoop loop;

    QObject::connect(&session,
                     &Platform::RecordingSession::finished,
                     &session,
                     [&](bool kept, const QString& output, const QString& error) {
                         const QFileInfo info(output);
                         if (inputInjectionRefused || extraInputRefused) {
                             std::printf("SKIP: desktop session refused %s input injection\n",
                                         inputInjectionRefused
                                             ? qPrintable(inputRefusalReason)
                                             : "synthetic");
                             result = 77;
                             QFile::remove(output);
                             loop.quit();
                             return;
                         }
                         if (!kept || output.isEmpty() || !info.exists() || info.size() <= 0) {
                             std::printf("FAIL %s: %s\n",
                                         pauseAndResume ? "pause/resume" : "basic",
                                         error.isEmpty() ? "no output" : qPrintable(error));
                             result = 1;
                         } else {
                             std::printf("OK %s: %s (%lld bytes)\n",
                                         pauseAndResume ? "pause/resume" : "basic",
                                         qPrintable(output),
                                         static_cast<long long>(info.size()));
                             result = 0;
                             if (!pauseAndResume
                                 && qEnvironmentVariableIsEmpty("APEXSHOT_TEST_FULL_SCREEN_ONCE")
                                 && !validateSidecar(output, recordedSeconds))
                                 result = 1;
                             if (!pauseAndResume) {
                                 const QString trackBase = Platform::sidecarPathFor(output);
                                 QStringList soundTracks;
                                 if (request.includeMicrophone)
                                     soundTracks << trackBase + QStringLiteral(".mic.m4a");
                                 if (request.systemAudio
                                     != Platform::RecordingRequest::SystemAudio::Off)
                                     soundTracks << trackBase + QStringLiteral(".system.m4a");
                                 for (const QString& track : soundTracks) {
                                     const QFileInfo trackInfo(track);
                                     if (trackInfo.exists() && trackInfo.size() > 0) {
                                         std::printf("OK sound track: %s (%lld bytes)\n",
                                                     qPrintable(track),
                                                     static_cast<long long>(trackInfo.size()));
                                     } else {
                                         std::printf("FAIL sound track missing: %s\n",
                                                     qPrintable(track));
                                         result = 1;
                                     }
                                 }
                             }
                             if (pauseAndResume) {
                                 QFile manifestFile(Platform::recordingManifestPathFor(output));
                                 const QJsonObject manifest =
                                     manifestFile.open(QIODevice::ReadOnly)
                                     ? QJsonDocument::fromJson(manifestFile.readAll()).object()
                                     : QJsonObject();
                                 const QJsonArray sessions =
                                     manifest.value(QStringLiteral("sessions")).toArray();
                                 const qint64 firstEnd = sessions.size() == 2
                                     ? sessions.at(0).toObject().value(QStringLiteral("endMs"))
                                           .toVariant().toLongLong()
                                     : -1;
                                 const qint64 secondStart = sessions.size() == 2
                                     ? sessions.at(1).toObject().value(QStringLiteral("startMs"))
                                           .toVariant().toLongLong()
                                     : -1;
                                 if (manifest.value(QStringLiteral("version")).toInt() != 1
                                     || sessions.size() != 2 || firstEnd <= 0
                                     || secondStart != firstEnd) {
                                     std::printf("FAIL pause/resume manifest: %d sessions\n",
                                                 int(sessions.size()));
                                     result = 1;
                                 } else {
                                     std::printf("OK pause/resume manifest: pause at %lld ms\n",
                                                 static_cast<long long>(firstEnd));
                                     const QJsonArray markers =
                                         manifest.value(QStringLiteral("markers")).toArray();
                                     const qint64 first = markers.size() == 2
                                         ? markers.at(0).toVariant().toLongLong()
                                         : -1;
                                     const qint64 second = markers.size() == 2
                                         ? markers.at(1).toVariant().toLongLong()
                                         : -1;
                                     // 400 ms in, and 400 ms after the
                                     // resume at media time ~800 ms.
                                     if (first < 250 || first > 650 || second < 1000
                                         || second > 1500) {
                                         std::printf("FAIL markers: %lld, %lld\n",
                                                     static_cast<long long>(first),
                                                     static_cast<long long>(second));
                                         result = 1;
                                     } else {
                                         std::printf("OK markers at %lld and %lld ms\n",
                                                     static_cast<long long>(first),
                                                     static_cast<long long>(second));
                                     }
                                 }
                             }
                             if (qEnvironmentVariableIsEmpty("APEXSHOT_KEEP_TEST_OUTPUT"))
                                 removeRecordingOutputs(output);
                         }
                         loop.quit();
                     });

    QString error;
    if (!session.start(request, error)) {
        std::printf("FAIL start: %s\n", qPrintable(error));
        return 1;
    }
    recordingClock.start();
    if (!pauseAndResume)
        cursorGuard.start();

    if (pauseAndResume) {
        QTimer::singleShot(400, &session, [&session] { session.addMarker(); });
        QTimer::singleShot(800, &session, [&session] { session.pause(); });
        QTimer::singleShot(1800, &session, [&session] { session.resume(); });
        QTimer::singleShot(2200, &session, [&session] { session.addMarker(); });
        QTimer::singleShot(2800, &session, [&session] { session.stop(); });
    } else {
        const auto inject = [&session,
                             &inputInjectionRefused,
                             &inputRefusalReason,
                             &leftButtonDownInjected,
                             &guardedCursorPosition](int delay,
                                                    QPoint position,
                                                    int buttonAction) {
            QTimer::singleShot(delay, &session, [&session,
                                                 &inputInjectionRefused,
                                                 &inputRefusalReason,
                                                 &leftButtonDownInjected,
                                                 &guardedCursorPosition,
                                                 position,
                                                 buttonAction] {
                guardedCursorPosition = position;
                bool ok = moveCursor(position);
                if (!ok)
                    inputRefusalReason = QStringLiteral("SetCursorPos");
                if (ok && buttonAction != 0) {
                    ok = setLeftButton(buttonAction > 0);
                    if (!ok)
                        inputRefusalReason = QStringLiteral("SendInput");
                    else
                        leftButtonDownInjected = buttonAction > 0;
                }
                if (!ok) {
                    inputInjectionRefused = true;
                    if (leftButtonDownInjected) {
                        setLeftButton(false);
                        leftButtonDownInjected = false;
                    }
                    session.discard();
                }
            });
        };
        inject(200, QPoint(kRegion.x() + 90, kRegion.y() + 90), 0);
        inject(450, QPoint(kRegion.x() + 120, kRegion.y() + 110), 0);
        inject(700, QPoint(kRegion.x() + 140, kRegion.y() + 130), 1);
        inject(1000, QPoint(kRegion.x() + 175, kRegion.y() + 160), 0);
        inject(1250, QPoint(kRegion.x() + 175, kRegion.y() + 160), -1);
        inject(1450, QPoint(kRegion.x() + 230, kRegion.y() + 180), 0);
        if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_RAPID_CLICKS")) {
            for (int index = 0; index < 30; ++index) {
                const int delay = 1600 + index * 4;
                QTimer::singleShot(delay, &session, [&extraInputRefused] {
                    if (!setLeftButton(true))
                        extraInputRefused = true;
                });
                QTimer::singleShot(delay + 2, &session, [&extraInputRefused] {
                    if (!setLeftButton(false))
                        extraInputRefused = true;
                });
            }
        }
        if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_KEYSTROKES")) {
            QTimer::singleShot(1800, &session, [&extraInputRefused] {
                const bool ok = sendKey(VK_CONTROL, true) && sendKey(VK_SHIFT, true)
                    && sendKey(static_cast<WORD>('K'), true)
                    && sendKey(static_cast<WORD>('K'), false)
                    && sendKey(VK_SHIFT, false) && sendKey(VK_CONTROL, false);
                if (!ok)
                    extraInputRefused = true;
            });
        }
        if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_SCROLL")) {
            QTimer::singleShot(1900, &session, [&extraInputRefused] {
                if (!sendWheel(-WHEEL_DELTA) || !sendWheel(WHEEL_DELTA))
                    extraInputRefused = true;
            });
        }
        QTimer::singleShot(durationMs,
                           &session,
                           [&session, &recordingClock, &recordedSeconds, &cursorGuard] {
            recordedSeconds = recordingClock.elapsed() / 1000.0;
            cursorGuard.stop();
            session.stop();
        });
    }

    QTimer::singleShot(durationMs + 30000, &session, [&loop] {
        std::printf("FAIL timeout\n");
        loop.quit();
    });

    loop.exec();
    restoreCursor();
    return result;
}

struct WindowSearch {
    QString needle;
    HWND window = nullptr;
    qint64 area = 0;
};

BOOL CALLBACK findWindowProc(HWND window, LPARAM userData)
{
    auto* search = reinterpret_cast<WindowSearch*>(userData);
    if (!search || !IsWindowVisible(window))
        return TRUE;

    wchar_t title[512]{};
    GetWindowTextW(window, title, 512);
    if (!QString::fromWCharArray(title).contains(search->needle, Qt::CaseInsensitive))
        return TRUE;

    RECT frame{};
    if (!GetWindowRect(window, &frame))
        return TRUE;
    const qint64 area = static_cast<qint64>(frame.right - frame.left)
        * static_cast<qint64>(frame.bottom - frame.top);
    if (area > search->area) {
        search->window = window;
        search->area = area;
    }
    return TRUE;
}

QRect windowFrame(HWND window)
{
    RECT frame{};
    if (SUCCEEDED(DwmGetWindowAttribute(window,
                                        DWMWA_EXTENDED_FRAME_BOUNDS,
                                        &frame,
                                        sizeof(frame)))) {
        if (frame.right > frame.left && frame.bottom > frame.top)
            return QRect(QPoint(frame.left, frame.top),
                         QSize(frame.right - frame.left, frame.bottom - frame.top));
    }
    if (GetWindowRect(window, &frame))
        return QRect(QPoint(frame.left, frame.top),
                     QSize(frame.right - frame.left, frame.bottom - frame.top));
    return {};
}

enum class WindowRunResult {
    Kept,
    Discarded,
    Failed,
};

WindowRunResult runWindowSession(HWND window,
                                 const QRect& region,
                                 const QString& path,
                                 int seconds,
                                 int fps,
                                 int crf,
                                 int discardAfterMs,
                                 QString* failure)
{
    QFile::remove(path);
    const QString oldSidecar = Platform::sidecarPathFor(path);
    if (!oldSidecar.isEmpty()) {
        QFile::remove(oldSidecar);
        if (QFileInfo(oldSidecar).fileName() == QStringLiteral("pointer.json"))
            QDir(QFileInfo(oldSidecar).absolutePath()).removeRecursively();
    }

    Platform::RecordingSession session;
    Platform::RecordingRequest request;
    request.target = Platform::RecordingRequest::Target::Window;
    request.windowId = reinterpret_cast<quintptr>(window);
    request.region = region;
    request.outputPath = path;
    request.includeCursor = false;
    request.fps = fps;
    request.crf = crf;

    bool finished = false;
    bool kept = false;
    QEventLoop loop;
    QObject::connect(&session,
                     &Platform::RecordingSession::finished,
                     &session,
                     [&](bool wasKept, const QString& output, const QString& error) {
                         finished = true;
                         kept = wasKept;
                         const QFileInfo info(output);
                         if (wasKept && info.exists() && info.size() > 0) {
                             std::printf("OK window: %s (%lld bytes)\n",
                                         qPrintable(output),
                                         static_cast<long long>(info.size()));
                         } else if (discardAfterMs <= 0) {
                             if (failure)
                                 *failure = error.isEmpty() ? QStringLiteral("no output") : error;
                             std::printf("FAIL window recording: %s\n",
                                         failure ? qPrintable(*failure) : "no output");
                         }
                         loop.quit();
                     });

    QString error;
    if (!session.start(request, error)) {
        if (failure)
            *failure = error;
        std::printf("FAIL window start: %s\n", qPrintable(error));
        return WindowRunResult::Failed;
    }

    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_WINDOW_PAUSE")) {
        QTimer::singleShot(800, &session, [&session] { session.pause(); });
        QTimer::singleShot(1800, &session, [&session] { session.resume(); });
    }
    if (discardAfterMs > 0)
        QTimer::singleShot(discardAfterMs, &session, [&session] { session.discard(); });
    else
        QTimer::singleShot(seconds * 1000, &session, [&session] { session.stop(); });
    QTimer::singleShot((seconds + 30) * 1000, &session, [&loop] {
        std::printf("FAIL window recording timeout\n");
        loop.quit();
    });
    loop.exec();

    if (!finished)
        return WindowRunResult::Failed;
    if (discardAfterMs > 0)
        return kept ? WindowRunResult::Failed : WindowRunResult::Discarded;
    return kept && QFileInfo(path).exists() && QFileInfo(path).size() > 0
        ? WindowRunResult::Kept
        : WindowRunResult::Failed;
}

int recordDemo()
{
    const QString output = qEnvironmentVariable("APEXSHOT_TEST_OUTPUT").trimmed();
    if (output.isEmpty()) {
        std::printf("FAIL set APEXSHOT_TEST_OUTPUT to record a demo clip\n");
        return 1;
    }

    QRect region;
    const QString regionSpec = qEnvironmentVariable("APEXSHOT_TEST_DEMO_REGION").trimmed();
    if (!regionSpec.isEmpty()) {
        const QStringList parts = regionSpec.split(QLatin1Char(','));
        if (parts.size() == 4)
            region = QRect(parts[0].toInt(), parts[1].toInt(), parts[2].toInt(), parts[3].toInt());
    }
    if (region.isEmpty()) {
        const QVector<Platform::MonitorInfo> monitors = Platform::monitors();
        if (monitors.isEmpty()) {
            std::printf("FAIL no monitor is available for a demo recording\n");
            return 1;
        }
        region = monitors.first().geometry;
    }

    int seconds = qEnvironmentVariableIntValue("APEXSHOT_TEST_SECONDS");
    if (seconds <= 0)
        seconds = 10;

    QFile::remove(output);
    QDir().mkpath(QFileInfo(output).absolutePath());

    Platform::RecordingRequest request;
    request.target = Platform::RecordingRequest::Target::Screen;
    request.region = region;
    request.outputPath = output;
    request.includeCursor = true;
    request.captureKeystrokes = true;
    request.fps = 30;
    request.crf = 20;

    Platform::RecordingSession session;
    QEventLoop loop;
    int result = 1;
    QObject::connect(&session,
                     &Platform::RecordingSession::finished,
                     &session,
                     [&](bool kept, const QString& path, const QString& error) {
                         const QFileInfo info(path);
                         if (!kept || path.isEmpty() || !info.exists() || info.size() <= 0) {
                             std::printf("FAIL demo: %s\n",
                                         error.isEmpty() ? "no output" : qPrintable(error));
                             loop.quit();
                             return;
                         }
                         std::printf("OK demo: %s (%lld bytes)\n",
                                     qPrintable(path),
                                     static_cast<long long>(info.size()));
                         const QString sidecarPath = Platform::sidecarPathFor(path);
                         QFile sidecar(sidecarPath);
                         if (sidecar.open(QIODevice::ReadOnly)) {
                             const QJsonDocument document = QJsonDocument::fromJson(sidecar.readAll());
                             const QJsonObject object = document.object();
                             std::printf("DEMO pointer=%d clicks=%d presses=%d scrolls=%d keys=%d "
                                         "visibility=%d cursors=%d\n",
                                         object.value(QStringLiteral("pointer")).toArray().size(),
                                         object.value(QStringLiteral("clicks")).toArray().size(),
                                         object.value(QStringLiteral("presses")).toArray().size(),
                                         object.value(QStringLiteral("scrolls")).toArray().size(),
                                         object.value(QStringLiteral("keys")).toArray().size(),
                                         object.value(QStringLiteral("visibility")).toArray().size(),
                                         object.value(QStringLiteral("cursors")).toArray().size());
                         }
                         result = 0;
                         loop.quit();
                     });

    QString error;
    if (!session.start(request, error)) {
        std::printf("FAIL demo start: %s\n", qPrintable(error));
        return 1;
    }
    std::printf("DEMO recording %dx%d at %d,%d for %ds\n",
                region.width(),
                region.height(),
                region.x(),
                region.y(),
                seconds);
    QTimer::singleShot(seconds * 1000, &session, [&session] { session.stop(); });
    QTimer::singleShot((seconds + 30) * 1000, &loop, [&] {
        std::printf("FAIL demo timeout\n");
        loop.quit();
    });
    loop.exec();
    return result;
}

int recordWindow()
{
    const QString needle = qEnvironmentVariable("APEXSHOT_TEST_WINDOW_TITLE").trimmed();
    if (needle.isEmpty()) {
        std::printf("SKIP: set APEXSHOT_TEST_WINDOW_TITLE to record a window\n");
        return 77;
    }

    WindowSearch search;
    search.needle = needle;
    EnumWindows(findWindowProc, reinterpret_cast<LPARAM>(&search));
    if (!search.window) {
        std::printf("FAIL no visible window title contains \"%s\"\n", qPrintable(needle));
        return 1;
    }

    if (IsIconic(search.window)) {
        ShowWindow(search.window, SW_RESTORE);
        Sleep(300);
    }

    const QRect region = windowFrame(search.window);
    if (region.width() < 2 || region.height() < 2) {
        std::printf("FAIL the matched window has no usable frame\n");
        return 1;
    }

    int seconds = qEnvironmentVariableIntValue("APEXSHOT_TEST_WINDOW_SECONDS");
    if (seconds <= 0)
        seconds = 30;
    const QString path = qEnvironmentVariable("APEXSHOT_TEST_WINDOW_OUTPUT").trimmed();
    if (path.isEmpty()) {
        std::printf("SKIP: set APEXSHOT_TEST_WINDOW_OUTPUT to name the recording\n");
        return 77;
    }

    std::printf("window: %s %dx%d at %d,%d for %d seconds\n",
                qPrintable(needle),
                region.width(),
                region.height(),
                region.x(),
                region.y(),
                seconds);

    int crf = qEnvironmentVariableIntValue("APEXSHOT_TEST_WINDOW_CRF");
    if (crf <= 0)
        crf = 20;

    QString failure;
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_WINDOW_RESTART")) {
        const QString firstPath = path + QStringLiteral(".first.mp4");
        const WindowRunResult first = runWindowSession(search.window,
                                                       region,
                                                       firstPath,
                                                       2,
                                                       60,
                                                       crf,
                                                       1000,
                                                       &failure);
        QFile::remove(firstPath);
        const QString firstSidecar = Platform::sidecarPathFor(firstPath);
        if (!firstSidecar.isEmpty()) {
            QFile::remove(firstSidecar);
            if (QFileInfo(firstSidecar).fileName() == QStringLiteral("pointer.json"))
                QDir(QFileInfo(firstSidecar).absolutePath()).removeRecursively();
        }
        if (first != WindowRunResult::Discarded) {
            std::printf("FAIL restart discard: %s\n", qPrintable(failure));
            return 1;
        }
        std::printf("OK restart: first take discarded\n");

        const WindowRunResult second =
            runWindowSession(search.window, region, path, seconds, 60, crf, 0, &failure);
        if (second != WindowRunResult::Kept) {
            std::printf("FAIL restart recording: %s\n", qPrintable(failure));
            return 1;
        }
        std::printf("OK restart: second take kept\n");
        return 0;
    }

    const WindowRunResult result =
        runWindowSession(search.window, region, path, seconds, 60, crf, 0, &failure);
    return result == WindowRunResult::Kept ? 0 : 1;
}

} // namespace

namespace {

/// The live camera drawn on screen, so a screen recording shows the camera
/// beside its own track and the two can be lined up after the fact.
class PreviewWindow final : public QRasterWindow
{
public:
    void setFrame(const QImage& frame)
    {
        m_frame = frame;
        update();
    }

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.fillRect(QRect(QPoint(0, 0), size()), Qt::black);
        if (!m_frame.isNull())
            painter.drawImage(QRect(QPoint(0, 0), size()), m_frame);
    }

private:
    QImage m_frame;
};

/// A full-screen window that flips between black and white at irregular
/// intervals. The screen recording sees each flip on the frame it lands, and
/// the camera sees it as light on whoever sits in front of the screen, so the
/// two can be lined up without trusting either pipeline.
class ScreenFlash final : public QRasterWindow
{
public:
    ScreenFlash()
    {
        m_timer.setSingleShot(true);
        QObject::connect(&m_timer, &QTimer::timeout, &m_timer, [this] {
            m_white = !m_white;
            update();
            // A fixed sequence of uneven gaps keeps the pattern unambiguous.
            m_step = (m_step * 1103515245u + 12345u) & 0x7fffffffu;
            m_timer.start(350 + int(m_step % 900u));
        });
        m_timer.start(500);
    }

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.fillRect(QRect(QPoint(0, 0), size()), m_white ? Qt::white : Qt::black);
    }

private:
    QTimer m_timer;
    bool m_white = false;
    quint32 m_step = 7u;
};

} // namespace

/// Record a short clip with the webcam track enabled and check the track, the
/// sidecar block and the live frames the bubble draws.
///
/// The camera name comes from APEXSHOT_TEST_CAMERA so the test uses the machine
/// it runs on instead of guessing a friendly name.
int recordWithWebcam()
{
    const QString camera = qEnvironmentVariable("APEXSHOT_TEST_CAMERA").trimmed();
    if (camera.isEmpty()) {
        std::printf("SKIP: set APEXSHOT_TEST_CAMERA to check the webcam track\n");
        return 77;
    }

    Platform::RecordingSession session;
    QString path =
        QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-record-webcam.mp4"));
    const QString requestedPath = qEnvironmentVariable("APEXSHOT_TEST_WEBCAM_OUTPUT").trimmed();
    if (!requestedPath.isEmpty())
        path = requestedPath;
    const QString existingSidecar = Platform::sidecarPathFor(path);
    if (!existingSidecar.isEmpty()) {
        QFile::remove(existingSidecar);
        if (QFileInfo(existingSidecar).fileName() == QStringLiteral("pointer.json"))
            QDir(QFileInfo(existingSidecar).absolutePath()).removeRecursively();
    }
    QFile::remove(path);
    QDir().mkpath(QFileInfo(path).absolutePath());

    QRect region = kRegion;
    int fps = 30;
    int crf = 28;
    int milliseconds = 2500;
    bool cursor = false;
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_WEBCAM_FULL_SCREEN")) {
        const QVector<Platform::MonitorInfo> monitors = Platform::monitors();
        if (!monitors.isEmpty()) {
            region = monitors.first().geometry;
            fps = 60;
            crf = 20;
            milliseconds = 10000;
            cursor = true;
        }
    }
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_WEBCAM_SECONDS")) {
        milliseconds = qEnvironmentVariableIntValue("APEXSHOT_TEST_WEBCAM_SECONDS") * 1000;
    }
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_WEBCAM_CRF"))
        crf = qEnvironmentVariableIntValue("APEXSHOT_TEST_WEBCAM_CRF");

    const Platform::WebcamLayout layout =
        Platform::webcamLayoutFor(region, 2, 1, 0.0, 1.0, true);

    Platform::RecordingRequest request;
    request.region = region;
    request.outputPath = path;
    request.includeCursor = cursor;
    request.fps = fps;
    request.crf = crf;
    request.webcamEnabled = true;
    request.webcamDevice = camera;
    request.webcamLayout = layout;

    // APEXSHOT_TEST_WEBCAM_SHOW draws the live camera in the middle of the
    // captured screen, so the sync of the track can be checked against it.
    std::unique_ptr<PreviewWindow> preview;
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_WEBCAM_SHOW")
        && qobject_cast<QGuiApplication*>(QCoreApplication::instance())) {
        preview = std::make_unique<PreviewWindow>();
        preview->setFlags(Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint | Qt::Tool);
        preview->setGeometry(QRect(region.center() - QPoint(240, 240), QSize(480, 480)));
        preview->show();
    }
    // APEXSHOT_TEST_WEBCAM_FLASH flashes the whole captured screen so the
    // camera sees each flip as light on the face in front of it.
    std::unique_ptr<ScreenFlash> flash;
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_WEBCAM_FLASH")
        && qobject_cast<QGuiApplication*>(QCoreApplication::instance())) {
        flash = std::make_unique<ScreenFlash>();
        flash->setFlags(Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint | Qt::Tool);
        flash->setGeometry(region);
        flash->show();
    }

    int frames = 0;
    QSize frameSize;
    QObject::connect(&session,
                     &Platform::RecordingSession::webcamFrame,
                     &session,
                     [&](const QImage& frame) {
                         ++frames;
                         frameSize = frame.size();
                         if (preview)
                             preview->setFrame(frame);
                     });
    QObject::connect(&session,
                     &Platform::RecordingSession::warning,
                     &session,
                     [](const QString& message) {
                         std::printf("WARN: %s\n", qPrintable(message));
                     });

    int result = 1;
    QEventLoop loop;
    QObject::connect(&session,
                     &Platform::RecordingSession::finished,
                     &session,
                     [&](bool kept, const QString& output, const QString& error) {
                         loop.quit();
                         if (!kept || !QFileInfo(output).isFile()) {
                             std::printf("FAIL webcam recording was not kept: %s\n",
                                         qPrintable(error));
                             return;
                         }
                         const QString track = Platform::webcamTrackPathFor(output);
                         if (track.isEmpty() || !QFileInfo(track).isFile()
                             || QFileInfo(track).size() == 0) {
                             std::printf("FAIL webcam track was not written\n");
                             return;
                         }
                         QFile file(Platform::sidecarPathFor(output));
                         if (!file.open(QIODevice::ReadOnly)) {
                             std::printf("FAIL webcam sidecar was not written\n");
                             return;
                         }
                         const QJsonDocument document = QJsonDocument::fromJson(file.readAll());
                         const QJsonObject webcam =
                             document.object().value(QStringLiteral("webcam")).toObject();
                         if (webcam.isEmpty()) {
                             // The recorder keeps the footage when a camera will
                             // not open, which is a skip for this check.
                             std::puts("SKIP: the camera did not open");
                             result = 77;
                             return;
                         }
                         const auto closeEnough = [](double value, double expected) {
                             return std::abs(value - expected) < 0.01;
                         };
                         if (webcam.value(QStringLiteral("file")).toString().isEmpty()
                             || webcam.value(QStringLiteral("width")).toInt() <= 0
                             || webcam.value(QStringLiteral("height")).toInt() <= 0
                             || !closeEnough(webcam.value(QStringLiteral("x")).toDouble(), layout.x)
                             || !closeEnough(webcam.value(QStringLiteral("y")).toDouble(), layout.y)
                             || !closeEnough(webcam.value(QStringLiteral("w")).toDouble(), layout.w)
                             || !closeEnough(webcam.value(QStringLiteral("h")).toDouble(), layout.h)
                             || webcam.value(QStringLiteral("shape")).toInt() != 1
                             || !webcam.value(QStringLiteral("flip")).toBool()) {
                             std::printf("FAIL webcam sidecar block is wrong: %s\n",
                                         qPrintable(QString::fromUtf8(
                                             QJsonDocument(webcam).toJson(QJsonDocument::Compact))));
                             return;
                         }
                         if (frames <= 0 || frameSize.isEmpty()) {
                             std::printf("FAIL no live webcam frames reached the bubble\n");
                             return;
                         }
                         std::printf("WEBCAM_TRACK=%s\n", qPrintable(track));
                         std::printf("WEBCAM_FRAMES=%d preview=%dx%d track=%dx%d\n",
                                     frames,
                                     frameSize.width(),
                                     frameSize.height(),
                                     webcam.value(QStringLiteral("width")).toInt(),
                                     webcam.value(QStringLiteral("height")).toInt());
                         std::printf("WEBCAM_LAYOUT x=%.3f y=%.3f w=%.3f h=%.3f shape=%d flip=%d\n",
                                     layout.x,
                                     layout.y,
                                     layout.w,
                                     layout.h,
                                     layout.shape,
                                     layout.flip ? 1 : 0);
                         std::printf("WEBCAM_RECORDING %s %dx%d fps=%d crf=%d start=%.3f "
                                     "duration=%.3f\n",
                                     qPrintable(output),
                                     region.width(),
                                     region.height(),
                                     fps,
                                     crf,
                                     webcam.value(QStringLiteral("start")).toDouble(),
                                     webcam.value(QStringLiteral("duration")).toDouble());
                         result = 0;
                     });

    QString error;
    if (!session.start(request, error)) {
        std::printf("SKIP: the webcam recording did not start: %s\n", qPrintable(error));
        return 77;
    }
    // APEXSHOT_TEST_WEBCAM_PAUSE_AT pauses the recording that many seconds in,
    // and APEXSHOT_TEST_WEBCAM_PAUSE_FOR (default 2) resumes it after, so the
    // camera's start in a later segment is exercised. The pause is added to
    // the stop time.
    const int pauseAt = qEnvironmentVariableIntValue("APEXSHOT_TEST_WEBCAM_PAUSE_AT");
    int pauseFor = 2;
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_WEBCAM_PAUSE_FOR"))
        pauseFor = qEnvironmentVariableIntValue("APEXSHOT_TEST_WEBCAM_PAUSE_FOR");
    if (pauseAt > 0 && pauseFor > 0 && pauseAt * 1000 < milliseconds) {
        QTimer::singleShot(pauseAt * 1000, &session, [&session] {
            session.pause();
            std::puts("WEBCAM_PAUSED");
            std::fflush(stdout);
        });
        QTimer::singleShot((pauseAt + pauseFor) * 1000, &session, [&session] {
            session.resume();
            std::puts("WEBCAM_RESUMED");
            std::fflush(stdout);
        });
        milliseconds += pauseFor * 1000;
    }
    QTimer::singleShot(milliseconds, &session, [&session] { session.stop(); });
    loop.exec();
    return result;
}

namespace {

bool checkWebcamPlacement()
{
    const QRect region(100, 50, 1920, 1080);
    const double xs[] = {0.0, 0.37, 1.0};
    const double ys[] = {0.0, 0.62, 1.0};
    for (int size = 0; size < 4; ++size) {
        for (int shape = 0; shape < 4; ++shape) {
            for (double x : xs) {
                for (double y : ys) {
                    const QRect rect =
                        Platform::webcamBubbleRect(region, size, shape, x, y);
                    double relX = 0.0;
                    double relY = 0.0;
                    if (!Platform::webcamRelativePosition(region,
                                                          size,
                                                          shape,
                                                          rect,
                                                          &relX,
                                                          &relY)
                        || std::abs(relX - x) > 0.02 || std::abs(relY - y) > 0.02) {
                        std::printf("FAIL webcam placement size=%d shape=%d got=%.3f,%.3f "
                                    "want=%.3f,%.3f\n",
                                    size,
                                    shape,
                                    relX,
                                    relY,
                                    x,
                                    y);
                        return false;
                    }
                }
            }
        }
    }
    return true;
}

/// The low-disk guard: a start under the reserve is refused, and a recording
/// whose free space drops under it mid-session stops, keeps its file, and
/// says why.
int recordUntilDiskLow()
{
    const qint64 plenty = 64LL * 1024 * 1024 * 1024;
    const qint64 scarce = 100LL * 1024 * 1024;

    {
        Platform::setFreeDiskBytesProbeForTesting([scarce](const QString&) { return scarce; });
        Platform::RecordingSession session;
        Platform::RecordingRequest request;
        request.region = kRegion;
        request.outputPath =
            QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-record-diskfull-refused.mp4"));
        request.includeCursor = false;
        request.fps = 30;
        request.crf = 28;
        QString error;
        const bool started = session.start(request, error);
        Platform::setFreeDiskBytesProbeForTesting({});
        if (started) {
            session.discard();
            std::printf("FAIL: a start under the disk reserve was accepted\n");
            return 1;
        }
        if (!error.contains(QStringLiteral("free disk space"))) {
            std::printf("FAIL: unexpected low-disk start error: %s\n", qPrintable(error));
            return 1;
        }
    }

    QElapsedTimer clock;
    clock.start();
    Platform::setFreeDiskBytesProbeForTesting([&clock, plenty, scarce](const QString&) {
        return clock.elapsed() < 1500 ? plenty : scarce;
    });

    Platform::RecordingSession session;
    const QString path =
        QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-record-diskfull.mp4"));
    QFile::remove(path);
    Platform::RecordingRequest request;
    request.region = kRegion;
    request.outputPath = path;
    request.includeCursor = false;
    request.fps = 30;
    request.crf = 28;

    QString warningText;
    bool kept = false;
    QString output;
    QString finishError;
    bool finished = false;
    QEventLoop loop;
    QObject::connect(&session, &Platform::RecordingSession::warning, &session,
                     [&](const QString& message) { warningText += message; });
    QObject::connect(&session, &Platform::RecordingSession::finished, &session,
                     [&](bool k, const QString& p, const QString& e) {
                         kept = k;
                         output = p;
                         finishError = e;
                         finished = true;
                         loop.quit();
                     });
    QTimer::singleShot(20000, &loop, &QEventLoop::quit);

    QString error;
    if (!session.start(request, error)) {
        Platform::setFreeDiskBytesProbeForTesting({});
        std::printf("FAIL: low-disk recording did not start: %s\n", qPrintable(error));
        return 1;
    }
    loop.exec();
    Platform::setFreeDiskBytesProbeForTesting({});

    if (!finished) {
        session.discard();
        std::printf("FAIL: the recording did not stop when the disk ran low\n");
        return 1;
    }
    if (!kept || output.isEmpty() || QFileInfo(output).size() <= 0) {
        std::printf("FAIL: the low-disk stop did not keep the recording (%s)\n",
                    qPrintable(finishError));
        return 1;
    }
    if (!warningText.contains(QStringLiteral("disk is almost full"))) {
        std::printf("FAIL: the low-disk stop gave no reason (warning: %s)\n",
                    qPrintable(warningText));
        return 1;
    }
    std::printf("OK: low-disk stop kept %s after %lld ms\n", qPrintable(output),
                static_cast<long long>(clock.elapsed()));
    removeRecordingOutputs(output);
    return 0;
}

/// Crash recovery: a recording abandoned mid-session is listed as unfinished
/// and recovers into a playable video with its pointer sidecar and manifest.
int recordAndRecover()
{
    const QString path =
        QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-record-recover.mp4"));
    QFile::remove(path);

    QString workDir;
    {
        Platform::RecordingSession session;
        Platform::RecordingRequest request;
        request.region = kRegion;
        request.outputPath = path;
        request.includeCursor = false;
        request.fps = 30;
        request.crf = 28;
        QString error;
        if (!session.start(request, error)) {
            std::printf("FAIL recover start: %s\n", qPrintable(error));
            return 1;
        }
        QEventLoop wait;
        QTimer::singleShot(5500, &wait, &QEventLoop::quit);
        wait.exec();
        session.abandonForTesting();
    }

    const QVector<Platform::UnfinishedRecording> unfinished =
        Platform::RecordingSession::unfinishedRecordings(true);
    for (const Platform::UnfinishedRecording& item : unfinished) {
        if (QFileInfo(item.outputPath) == QFileInfo(path))
            workDir = item.workDir;
    }
    if (workDir.isEmpty()) {
        std::printf("FAIL recover: the abandoned recording was not listed (%d found)\n",
                    int(unfinished.size()));
        return 1;
    }

    Platform::RecordingSession recovery;
    bool kept = false;
    QString output;
    QString finishError;
    bool finished = false;
    QEventLoop loop;
    QObject::connect(&recovery, &Platform::RecordingSession::finished, &recovery,
                     [&](bool k, const QString& p, const QString& e) {
                         kept = k;
                         output = p;
                         finishError = e;
                         finished = true;
                         loop.quit();
                     });
    QTimer::singleShot(30000, &loop, &QEventLoop::quit);
    QString error;
    if (!recovery.recover(workDir, error)) {
        std::printf("FAIL recover: %s\n", qPrintable(error));
        Platform::RecordingSession::discardUnfinished(workDir);
        return 1;
    }
    loop.exec();
    if (!finished || !kept || output.isEmpty() || QFileInfo(output).size() <= 0) {
        std::printf("FAIL recover: nothing was saved (%s)\n", qPrintable(finishError));
        Platform::RecordingSession::discardUnfinished(workDir);
        return 1;
    }
    if (QFileInfo::exists(workDir)) {
        std::printf("FAIL recover: the work directory was left behind\n");
        return 1;
    }
    const QString sidecar = Platform::sidecarPathFor(output);
    QFile manifestFile(Platform::recordingManifestPathFor(output));
    const bool haveManifest = manifestFile.open(QIODevice::ReadOnly);
    const QJsonObject manifest =
        haveManifest ? QJsonDocument::fromJson(manifestFile.readAll()).object() : QJsonObject();
    manifestFile.close();
    if (sidecar.isEmpty() || !QFileInfo::exists(sidecar)) {
        std::printf("FAIL recover: no pointer sidecar for %s\n", qPrintable(output));
        return 1;
    }
    if (manifest.value(QStringLiteral("sessions")).toArray().size() != 1) {
        std::printf("FAIL recover: the manifest has no session\n");
        return 1;
    }
    std::printf("OK recover: %s (%lld bytes)\n", qPrintable(output),
                static_cast<long long>(QFileInfo(output).size()));
    removeRecordingOutputs(output);
    return 0;
}

} // namespace

/// A finished recording carries its ID inside the MP4, so the pointer data
/// follows the video through a rename or a move; an untagged video keeps the
/// path-hash location.
int testRecordingIdTag()
{
    const QString configured = qEnvironmentVariable("APEXSHOT_FFMPEG").trimmed();
    const QString ffmpeg = !configured.isEmpty() && QFileInfo::exists(configured)
        ? configured
        : QStandardPaths::findExecutable(QStringLiteral("ffmpeg"));
    if (ffmpeg.isEmpty()) {
        std::printf("SKIP: ffmpeg is unavailable\n");
        return 77;
    }

    QTemporaryDir directory;
    if (!directory.isValid()) {
        std::printf("FAIL recording id tag: no temporary directory\n");
        return 1;
    }
    const QString uuid = QUuid::createUuid().toString(QUuid::WithoutBraces);
    const QString tagged = directory.filePath(QStringLiteral("tagged.mp4"));
    const QString plain = directory.filePath(QStringLiteral("plain.mp4"));

    const auto encode = [&ffmpeg](const QString& path, const QString& description) {
        QStringList arguments{QStringLiteral("-y"),
                              QStringLiteral("-hide_banner"),
                              QStringLiteral("-loglevel"),
                              QStringLiteral("error"),
                              QStringLiteral("-f"),
                              QStringLiteral("lavfi"),
                              QStringLiteral("-i"),
                              QStringLiteral("testsrc=size=64x64:rate=10"),
                              QStringLiteral("-t"),
                              QStringLiteral("1"),
                              QStringLiteral("-c:v"),
                              QStringLiteral("libx264"),
                              QStringLiteral("-movflags"),
                              QStringLiteral("+faststart")};
        if (!description.isEmpty())
            arguments << QStringLiteral("-metadata")
                      << QStringLiteral("description=%1").arg(description);
        arguments << path;
        QProcess encoder;
        encoder.setProgram(ffmpeg);
        encoder.setProcessChannelMode(QProcess::MergedChannels);
        encoder.setArguments(arguments);
        encoder.start();
        if (!encoder.waitForStarted(15000))
            return false;
        if (!encoder.waitForFinished(60000)) {
            encoder.kill();
            encoder.waitForFinished(5000);
            return false;
        }
        return encoder.exitStatus() == QProcess::NormalExit && encoder.exitCode() == 0
            && QFileInfo(path).size() > 0;
    };

    if (!encode(tagged, Platform::recordingIdTagValue(uuid))) {
        std::printf("FAIL recording id tag: the tagged clip did not encode\n");
        return 1;
    }
    if (!encode(plain, QString())) {
        std::printf("FAIL recording id tag: the plain clip did not encode\n");
        return 1;
    }

    const QString taggedId = Platform::recordingIdOf(tagged);
    if (taggedId != uuid) {
        std::printf("FAIL recording id tag: the tag reads \"%s\"\n", qPrintable(taggedId));
        return 1;
    }
    const QString plainId = Platform::recordingIdOf(plain);
    if (!plainId.isEmpty()) {
        std::printf("FAIL recording id tag: an untagged clip carries \"%s\"\n",
                    qPrintable(plainId));
        return 1;
    }
    const QString taggedSidecar = Platform::sidecarPathFor(tagged);
    if (taggedSidecar.isEmpty() || taggedSidecar != Platform::recordingSidecarPathFor(uuid)) {
        std::printf("FAIL recording id tag: the sidecar path is %s\n",
                    qPrintable(taggedSidecar));
        return 1;
    }
    if (!Platform::sidecarPathFor(plain).contains(QStringLiteral("pointer-sidecars"))) {
        std::printf("FAIL recording id tag: the untagged sidecar path is %s\n",
                    qPrintable(Platform::sidecarPathFor(plain)));
        return 1;
    }

    const QString renamed = directory.filePath(QStringLiteral("renamed.mp4"));
    if (!QFile::rename(tagged, renamed)) {
        std::printf("FAIL recording id tag: the clip could not be renamed\n");
        return 1;
    }
    if (Platform::sidecarPathFor(renamed) != Platform::recordingSidecarPathFor(uuid)) {
        std::printf("FAIL recording id tag: the renamed sidecar path is %s\n",
                    qPrintable(Platform::sidecarPathFor(renamed)));
        return 1;
    }

    const QString garbage = directory.filePath(QStringLiteral("garbage.mp4"));
    QFile garbageFile(garbage);
    if (!garbageFile.open(QIODevice::WriteOnly)
        || garbageFile.write(QByteArrayLiteral("0123456789")) != 10) {
        std::printf("FAIL recording id tag: the garbage clip could not be written\n");
        return 1;
    }
    garbageFile.close();
    if (!Platform::recordingIdOf(garbage).isEmpty()) {
        std::printf("FAIL recording id tag: a garbage clip carries an id\n");
        return 1;
    }

    std::printf("PASS recording id tag\n");
    return 0;
}

int main(int argc, char** argv)
{
    // The recorder is per-monitor DPI aware. Without this the test sees
    // virtualized monitor and window rectangles and records a smaller crop.
    Platform::enableDpiAwareness();
    // The webcam sync check draws the live camera, which needs a GUI
    // application; every other check runs headless.
    std::unique_ptr<QCoreApplication> app;
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_WEBCAM_SHOW")
        || !qEnvironmentVariableIsEmpty("APEXSHOT_TEST_WEBCAM_FLASH"))
        app = std::make_unique<QGuiApplication>(argc, argv);
    else
        app = std::make_unique<QCoreApplication>(argc, argv);

    if (!checkCursorKinds())
        return 1;
    if (!checkWebcamPlacement())
        return 1;
    if (!checkRecordingManifestJson())
        return 1;
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_RECORDING_ID_TAG"))
        return testRecordingIdTag();

    QString error;
    if (!Platform::RecordingSession::available(&error)) {
        std::printf("SKIP: %s\n", qPrintable(error));
        return 77;
    }

    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_DEMO"))
        return recordDemo();
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_WINDOW_TITLE"))
        return recordWindow();
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_FULL_SCREEN_ONCE"))
        return recordOnce(false);
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_FULL_SCREEN"))
        return recordOnce(true);
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_DISK_LOW"))
        return recordUntilDiskLow();
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_RECOVER"))
        return recordAndRecover();

    // The cursor checks need a desktop that accepts input injection, which a
    // locked or game-focused session refuses; the camera path can still run.
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_WEBCAM_ONLY"))
        return recordWithWebcam();

    int result = recordOnce(false);
    if (result == 0)
        result = recordOnce(true);
    if (result == 0)
        result = recordUntilDiskLow();
    if (result == 0)
        result = recordAndRecover();
    if (result == 0)
        result = testRecordingIdTag();
    if (result == 0) {
        const int webcam = recordWithWebcam();
        // 77 is the camera-absent skip, which is not a failure of the recorder.
        if (webcam != 77)
            result = webcam;
    }
    return result;
}
