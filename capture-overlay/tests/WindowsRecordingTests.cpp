// Windows-only smoke test for the ffmpeg-backed recorder.
//
// Records a small desktop region twice: once straight through, once with a
// pause and a resume. Each run must produce a non-empty MP4. Intentional
// console output; this is not part of the shipped binary.

#include "platform/recording.h"
#include "platform/windows/pointer_tracker.h"
#include "platform/windows/webcam_layout.h"

#include <QCoreApplication>
#include <QDir>
#include <QElapsedTimer>
#include <QEventLoop>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QRegularExpression>
#include <QStringList>
#include <QTimer>

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <cstdio>

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
    static const QRegularExpression hashName(QStringLiteral("^[0-9a-f]{64}$"));
    QDir directory = sidecarInfo.dir();
    if (!hashName.match(sidecarInfo.fileName()).hasMatch()
        || directory.dirName() != QStringLiteral("pointer-sidecars")) {
        std::printf("FAIL sidecar hash path is malformed: %s\n", qPrintable(path));
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
    if (object.value(QStringLiteral("version")).toInt() != 2 || pointer.size() < 10
        || clicks.isEmpty() || presses.isEmpty()
        || !object.value(QStringLiteral("t0_monotonic_us")).isDouble()
        || object.value(QStringLiteral("t0_monotonic_us")).toDouble() <= 0.0
        || region.value(QStringLiteral("x")).toInt() != kRegion.x()
        || region.value(QStringLiteral("y")).toInt() != kRegion.y()
        || region.value(QStringLiteral("w")).toInt() != kRegion.width()
        || region.value(QStringLiteral("h")).toInt() != kRegion.height()
        || !object.value(QStringLiteral("visibility")).toArray().isEmpty()
        || object.value(QStringLiteral("source")).toString() != QStringLiteral("recorded")) {
        std::printf("FAIL sidecar version or sample counts are invalid\n");
        return false;
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
        if (kind.toString() == QStringLiteral("default"))
            ++defaultKinds;
    }
    for (const QJsonValue& value : clicks) {
        const QJsonObject sample = value.toObject();
        if (!validTime(sample.value(QStringLiteral("t")).toDouble()) || !validPosition(sample)) {
            std::printf("FAIL click sample has an invalid time or position\n");
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

int recordOnce(bool pauseAndResume)
{
    Platform::RecordingSession session;
    const QString path = QDir(QDir::tempPath())
                             .filePath(pauseAndResume
                                           ? QStringLiteral("apexshot-record-pause.mp4")
                                           : QStringLiteral("apexshot-record-basic.mp4"));
    const QString oldSidecarPath = Platform::sidecarPathFor(path);
    if (!oldSidecarPath.isEmpty())
        QFile::remove(oldSidecarPath);
    QFile::remove(path);

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

    Platform::RecordingRequest request;
    request.region = kRegion;
    request.outputPath = path;
    // The editor draws the pointer from the sidecar, so the frames carry none.
    request.includeCursor = false;
    request.includeMicrophone = !qEnvironmentVariableIsEmpty("APEXSHOT_TEST_MIC");
    request.fps = 30;
    request.crf = 28;

    int result = 1;
    bool inputInjectionRefused = false;
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
                         if (inputInjectionRefused) {
                             std::printf("SKIP: desktop session refused %s input injection\n",
                                         qPrintable(inputRefusalReason));
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
                                 && !validateSidecar(output, recordedSeconds))
                                 result = 1;
                             if (qEnvironmentVariableIsEmpty("APEXSHOT_KEEP_TEST_OUTPUT"))
                                 QFile::remove(output);
                             if (qEnvironmentVariableIsEmpty("APEXSHOT_KEEP_TEST_OUTPUT")) {
                                 const QString sidecar = Platform::sidecarPathFor(output);
                                 if (!sidecar.isEmpty())
                                     QFile::remove(sidecar);
                             }
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
        QTimer::singleShot(800, &session, [&session] { session.pause(); });
        QTimer::singleShot(1800, &session, [&session] { session.resume(); });
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
        QTimer::singleShot(2400,
                           &session,
                           [&session, &recordingClock, &recordedSeconds, &cursorGuard] {
            recordedSeconds = recordingClock.elapsed() / 1000.0;
            cursorGuard.stop();
            session.stop();
        });
    }

    QTimer::singleShot(20000, &session, [&loop] {
        std::printf("FAIL timeout\n");
        loop.quit();
    });

    loop.exec();
    restoreCursor();
    return result;
}

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
    const QString path =
        QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-record-webcam.mp4"));
    const QString existingSidecar = Platform::sidecarPathFor(path);
    if (!existingSidecar.isEmpty())
        QFile::remove(existingSidecar);
    QFile::remove(path);

    const Platform::WebcamLayout layout =
        Platform::webcamLayoutFor(kRegion, 2, 1, 0.0, 1.0, true);

    Platform::RecordingRequest request;
    request.region = kRegion;
    request.outputPath = path;
    request.includeCursor = false;
    request.fps = 30;
    request.crf = 28;
    request.webcamEnabled = true;
    request.webcamDevice = camera;
    request.webcamLayout = layout;

    int frames = 0;
    QSize frameSize;
    QObject::connect(&session,
                     &Platform::RecordingSession::webcamFrame,
                     &session,
                     [&](const QImage& frame) {
                         ++frames;
                         frameSize = frame.size();
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
                         result = 0;
                     });

    QString error;
    if (!session.start(request, error)) {
        std::printf("SKIP: the webcam recording did not start: %s\n", qPrintable(error));
        return 77;
    }
    QTimer::singleShot(2500, &session, [&session] { session.stop(); });
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

} // namespace

int main(int argc, char** argv)
{
    QCoreApplication app(argc, argv);

    if (!checkCursorKinds())
        return 1;
    if (!checkWebcamPlacement())
        return 1;

    QString error;
    if (!Platform::RecordingSession::available(&error)) {
        std::printf("SKIP: %s\n", qPrintable(error));
        return 77;
    }

    // The cursor checks need a desktop that accepts input injection, which a
    // locked or game-focused session refuses; the camera path can still run.
    if (!qEnvironmentVariableIsEmpty("APEXSHOT_TEST_WEBCAM_ONLY"))
        return recordWithWebcam();

    int result = recordOnce(false);
    if (result == 0)
        result = recordOnce(true);
    if (result == 0) {
        const int webcam = recordWithWebcam();
        // 77 is the camera-absent skip, which is not a failure of the recorder.
        if (webcam != 77)
            result = webcam;
    }
    return result;
}
