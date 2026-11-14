#include "platform/recording.h"
#include "platform/windows/pointer_tracker.h"
#include "platform/windows/settings_store.h"
#include "platform/windows/webcam_bubble.h"
#include "platform/windows/webcam_layout.h"
#include "platform/windows/webcam_preview.h"

#include <QApplication>
#include <QDir>
#include <QEventLoop>
#include <QFile>
#include <QFileInfo>
#include <QHostAddress>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMouseEvent>
#include <QProcess>
#include <QStandardPaths>
#include <QTemporaryDir>
#include <QTcpSocket>
#include <QTimer>
#include <QUrl>

#include <cstdio>

namespace {

bool waitUntil(const std::function<bool()>& condition, int milliseconds)
{
    QEventLoop loop;
    QTimer check;
    QTimer deadline;
    check.setInterval(20);
    deadline.setSingleShot(true);
    QObject::connect(&check, &QTimer::timeout, &loop, [&] {
        if (condition())
            loop.quit();
    });
    QObject::connect(&deadline, &QTimer::timeout, &loop, &QEventLoop::quit);
    check.start();
    deadline.start(milliseconds);
    if (!condition())
        loop.exec();
    return condition();
}

void mouse(QWidget& widget, QEvent::Type type, const QPoint& local, const QPoint& global,
           Qt::MouseButton button, Qt::MouseButtons buttons)
{
    QMouseEvent event(type, local, global, button, buttons, Qt::NoModifier);
    QApplication::sendEvent(&widget, &event);
}

bool checkBubble()
{
    Platform::WebcamBubble bubble(0, false);
    bubble.setGeometry(100, 100, 200, 200);
    bubble.setMoveBounds(QRect(0, 0, 800, 600));
    bubble.setSetupMode(true);
    int updates = 0;
    bubble.setMovedHandler([&updates](const QRect&) { ++updates; });
    mouse(bubble, QEvent::MouseButtonPress, QPoint(78, 30), QPoint(178, 130),
          Qt::LeftButton, Qt::LeftButton);
    if (!bubble.flipped() || updates != 1)
        return false;
    mouse(bubble, QEvent::MouseButtonPress, QPoint(122, 30), QPoint(222, 130),
          Qt::LeftButton, Qt::LeftButton);
    if (bubble.shape() != 1 || updates != 2)
        return false;
    for (int expected : {2, 3, 0}) {
        mouse(bubble, QEvent::MouseButtonPress, QPoint(122, 30), QPoint(222, 130),
              Qt::LeftButton, Qt::LeftButton);
        if (bubble.shape() != expected)
            return false;
    }
    mouse(bubble, QEvent::MouseButtonPress, QPoint(191, 191), QPoint(291, 291),
          Qt::LeftButton, Qt::LeftButton);
    mouse(bubble, QEvent::MouseMove, QPoint(241, 241), QPoint(341, 341),
          Qt::NoButton, Qt::LeftButton);
    mouse(bubble, QEvent::MouseButtonRelease, QPoint(241, 241), QPoint(341, 341),
          Qt::LeftButton, Qt::NoButton);
    if (bubble.width() != 250 || bubble.height() != 250 || updates != 6)
        return false;
    const QRect fitted = Platform::webcamFitRect(QRect(20, 30, 100, 80), QRect(0, 0, 200, 200));
    const Platform::WebcamLayout layout =
        Platform::webcamLayoutForRect(QRect(20, 30, 100, 80), fitted, 2, true);
    const QRect projected = Platform::webcamProjectRect(
        QRect(0, 0, 1920, 1080), QRect(100, 100, 800, 450), QRect(1720, 880, 200, 200));
    return fitted == QRect(20, 30, 80, 80) && layout.w == 0.8 && layout.h == 1.0
        && layout.shape == 2 && layout.flip
        && projected == QRect(700, 350, 200, 200)
        && Platform::webcamProjectRect({}, QRect(0, 0, 100, 100), fitted).isEmpty()
        && Platform::webcamFitRect(QRect(), QRect(0, 0, 10, 10)).isEmpty();
}

bool checkFormats()
{
    const QString modes = QStringLiteral(
        "pixel_format=yuyv422 min s=1920x1080 fps=5 max s=1920x1080 fps=30\n"
        "pixel_format=yuyv422 min s=1280x720 fps=5 max s=1280x720 fps=30\n"
        "vcodec=mjpeg min s=1920x1080 fps=5 max s=1920x1080 fps=30.00\n"
        "vcodec=mjpeg min s=3840x2160 fps=5 max s=3840x2160 fps=30\n");
    const auto preferred = Platform::webcamPreferredFormat(modes, 30);
    const auto fallback = Platform::webcamPreferredFormat(
        QStringLiteral("pixel_format=yuyv422 min s=1280x720 fps=5 max s=1280x720 fps=30\n"), 30);
    // Nothing listed sustains 60, so the best deliverable mode wins: MJPEG can
    // use its whole size where a raw mode that large would starve the USB bus.
    const auto fast = Platform::webcamPreferredFormat(modes, 60);
    // A raw 1080p mode is never trusted on its own, and the recording-time
    // fallback may bound the pick to 720p.
    const QString rawOnly =
        QStringLiteral("pixel_format=yuyv422 min s=1920x1080 fps=5 max s=1920x1080 fps=30\n");
    const auto bounded = Platform::webcamPreferredFormat(modes, 60, qint64(1280) * 720);
    // Between two raw modes at the same size, the 12-bit layout wins: it
    // moves a quarter less data over the same bus.
    const auto lighter = Platform::webcamPreferredFormat(
        QStringLiteral("pixel_format=yuyv422 min s=1280x720 fps=30 max s=1280x720 fps=30\n"
                       "pixel_format=nv12 min s=1280x720 fps=30 max s=1280x720 fps=30\n"),
        30);
    return preferred.mjpeg && preferred.size == QSize(1920, 1080)
        && !fallback.mjpeg && fallback.size == QSize(1280, 720)
        && fast.mjpeg && fast.size == QSize(1920, 1080)
        && Platform::webcamPreferredFormat(rawOnly, 30).size.isEmpty()
        && !bounded.mjpeg && bounded.size == QSize(1280, 720)
        && !lighter.mjpeg && lighter.size == QSize(1280, 720)
        && lighter.pixelFormat == QLatin1String("nv12");
}

bool checkPlacementPersistence()
{
    QTemporaryDir directory;
    if (!directory.isValid())
        return false;
    const QByteArray previous = qgetenv("APPDATA");
    qputenv("APPDATA", directory.path().toLocal8Bit());

    Platform::WebcamPlacement saved;
    saved.present = true;
    saved.relX = 0.25;
    saved.relY = 0.75;
    saved.sizeFraction = 0.3;
    saved.shape = 3;
    saved.flip = true;
    Platform::saveWebcamPlacement(saved);

    bool ok = false;
    const Platform::WebcamPlacement loaded = Platform::loadWebcamPlacement();
    if (loaded.present && loaded.shape == 3 && loaded.flip
        && std::abs(loaded.sizeFraction - 0.3) < 1e-9
        && std::abs(loaded.relX - 0.25) < 1e-9
        && std::abs(loaded.relY - 0.75) < 1e-9) {
        // A file written before the look was remembered keeps its position and
        // takes the defaults for everything else.
        QFile legacy(directory.filePath(QStringLiteral("apexshot/webcam-placement.json")));
        if (legacy.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
            legacy.write(R"({"rel_x": 0.1, "rel_y": 0.2})");
            legacy.close();
            const Platform::WebcamPlacement old = Platform::loadWebcamPlacement();
            ok = old.present && old.shape == 0 && !old.flip && old.sizeFraction == 0.0
                && std::abs(old.relX - 0.1) < 1e-9;
        }
    }

    if (previous.isEmpty())
        qunsetenv("APPDATA");
    else
        qputenv("APPDATA", previous);
    return ok;
}

bool checkRecording(const QString& fakeCamera, bool bake)
{
    const QByteArray previousFfmpeg = qgetenv("APEXSHOT_FFMPEG");
    const QString realFfmpeg = previousFfmpeg.isEmpty()
        ? QStandardPaths::findExecutable(QStringLiteral("ffmpeg"))
        : QString::fromLocal8Bit(previousFfmpeg);
    if (realFfmpeg.isEmpty()) {
        std::fprintf(stderr, "ffmpeg is unavailable\n");
        return false;
    }
    qputenv("APEXSHOT_FFMPEG", fakeCamera.toLocal8Bit());
    Platform::WebcamPreview preview(QStringLiteral("Synthetic camera"), 30);
    int frames = 0;
    QString previewError;
    QObject::connect(&preview, &Platform::WebcamPreview::frameReady, &preview,
                     [&frames](const QImage&) { ++frames; });
    QObject::connect(&preview, &Platform::WebcamPreview::failed, &preview,
                     [&previewError](const QString& error) { previewError = error; });
    preview.start();
    if (!waitUntil([&] { return frames > 2 || !previewError.isEmpty(); }, 10000)) {
        std::fprintf(stderr, "preview did not start\n");
        return false;
    }
    if (!previewError.isEmpty() || preview.streamUrl().isEmpty())
        return false;

    const QUrl stream(preview.streamUrl());
    QTcpSocket unauthorized;
    unauthorized.connectToHost(QHostAddress::LocalHost, quint16(stream.port()));
    if (!waitUntil([&] { return unauthorized.state() == QAbstractSocket::ConnectedState; }, 5000))
        return false;
    unauthorized.write("GET /wrong-token HTTP/1.0\r\n\r\n");
    if (!waitUntil([&] { return unauthorized.bytesAvailable() > 0; }, 5000)
        || !unauthorized.readAll().startsWith("HTTP/1.0 403"))
        return false;

    Platform::WebcamPreview unavailable(QStringLiteral("Unavailable camera"), 30);
    bool reportedFailure = false;
    QObject::connect(&unavailable, &Platform::WebcamPreview::failed, &unavailable,
                     [&](const QString&) { reportedFailure = true; });
    unavailable.start();
    if (!waitUntil([&] { return reportedFailure; }, 10000) || !unavailable.streamUrl().isEmpty())
        return false;
    qputenv("APEXSHOT_FFMPEG", realFfmpeg.toLocal8Bit());

    QTemporaryDir output;
    if (!output.isValid())
        return false;
    Platform::RecordingRequest request;
    request.target = Platform::RecordingRequest::Target::Area;
    request.region = QRect(0, 0, 320, 240);
    request.outputPath = output.filePath(QStringLiteral("preview-test.mp4"));
    struct Cleanup {
        QString video;
        ~Cleanup()
        {
            QFile::remove(Platform::webcamTrackPathFor(video));
            const QString sidecar = Platform::sidecarPathFor(video);
            QFile::remove(sidecar);
            if (QFileInfo(sidecar).fileName() == QStringLiteral("pointer.json"))
                QDir(QFileInfo(sidecar).absolutePath()).removeRecursively();
        }
    } cleanup{request.outputPath};
    request.webcamEnabled = true;
    request.webcamDevice = QStringLiteral("Synthetic camera");
    request.webcamStreamUrl = preview.streamUrl();
    request.webcamLayout = Platform::webcamLayoutForRect(
        request.region, QRect(160, 80, 120, 120), 1, true);
    request.bakeWebcam = bake;
    Platform::RecordingSession session;
    bool finished = false;
    bool kept = false;
    bool paused = false;
    QString failure;
    QObject::connect(&session, &Platform::RecordingSession::finished, &session,
                     [&](bool success, const QString&, const QString& error) {
                         finished = true;
                         kept = success;
                         failure = error;
                     });
    QObject::connect(&session, &Platform::RecordingSession::stateChanged, &session, [&] {
        if (session.isPaused() && !paused) {
            paused = true;
            QTimer::singleShot(500, &session, &Platform::RecordingSession::resume);
            QTimer::singleShot(4000, &session, &Platform::RecordingSession::stop);
        }
    });
    QString error;
    if (!session.start(request, error)) {
        std::fprintf(stderr, "recording did not start: %s\n", qPrintable(error));
        return false;
    }
    const int before = frames;
    QTimer::singleShot(3500, &session, &Platform::RecordingSession::pause);
    if (!waitUntil([&] { return finished; }, 60000)) {
        session.discard();
        std::fprintf(stderr, "recording did not finish\n");
        return false;
    }
    if (!kept || !paused || frames <= before + 5 || !QFileInfo::exists(request.outputPath)) {
        std::fprintf(stderr, "recording failed: %s; frames=%d\n", qPrintable(failure), frames);
        return false;
    }
    QFile sidecar(Platform::sidecarPathFor(request.outputPath));
    if (!sidecar.open(QIODevice::ReadOnly))
        return false;
    const QJsonObject metadata = QJsonDocument::fromJson(sidecar.readAll()).object();
    const QJsonObject camera = metadata.value(QStringLiteral("webcam")).toObject();
    const QString cleanName = metadata.value(QStringLiteral("screen_file")).toString();
    const QString cameraName = camera.value(QStringLiteral("file")).toString();
    if (!bake) {
        // Without the bake the saved video is the screen footage itself: the
        // camera exists only as its own timed track, and no clean copy is kept.
        const QStringList parked = QDir(output.path())
                                       .entryList({QStringLiteral(".apexshot-screen-*")},
                                                  QDir::Files | QDir::Hidden);
        if (!cleanName.isEmpty() || !parked.isEmpty() || cameraName.isEmpty()
            || !QFileInfo::exists(Platform::webcamTrackPathFor(request.outputPath))
            || camera.value(QStringLiteral("duration")).toDouble() <= 0.0
            || camera.value(QStringLiteral("start")).toDouble() < 0.0) {
            std::fprintf(stderr, "unbaked recording lost its camera track or kept a clean copy\n");
            return false;
        }
        std::printf("unbaked camera track start=%.3fs duration=%.3fs\n",
                    camera.value(QStringLiteral("start")).toDouble(),
                    camera.value(QStringLiteral("duration")).toDouble());
        if (previousFfmpeg.isEmpty())
            qunsetenv("APEXSHOT_FFMPEG");
        else
            qputenv("APEXSHOT_FFMPEG", previousFfmpeg);
        return true;
    }
    if (cleanName.isEmpty() || cameraName.isEmpty()
        || !QFileInfo::exists(output.filePath(cleanName))
        || !QFileInfo::exists(Platform::webcamTrackPathFor(request.outputPath))
        || camera.value(QStringLiteral("duration")).toDouble() <= 0.0
        || camera.value(QStringLiteral("start")).toDouble() < 0.0) {
        std::fprintf(stderr, "editable area footage is missing\n");
        return false;
    }
    std::printf("camera track start=%.3fs duration=%.3fs clean=%s\n",
                camera.value(QStringLiteral("start")).toDouble(),
                camera.value(QStringLiteral("duration")).toDouble(),
                qPrintable(cleanName));
    QProcess extract;
    extract.setProgram(realFfmpeg);
    extract.setArguments({QStringLiteral("-v"), QStringLiteral("error"), QStringLiteral("-ss"),
                          QStringLiteral("1"), QStringLiteral("-i"), request.outputPath,
                          QStringLiteral("-frames:v"), QStringLiteral("1"), QStringLiteral("-f"),
                          QStringLiteral("image2pipe"), QStringLiteral("-c:v"),
                          QStringLiteral("png"), QStringLiteral("pipe:1")});
    extract.start();
    if (!extract.waitForFinished(20000) || extract.exitCode() != 0)
        return false;
    const QImage image = QImage::fromData(extract.readAllStandardOutput());
    if (image.isNull() || image.width() <= 220 || image.height() <= 140)
        return false;
    const QColor left = image.pixelColor(190, 140);
    const QColor right = image.pixelColor(250, 140);
    if (left.blue() < 150 || left.red() > 100 || right.red() < 150 || right.blue() > 100) {
        std::fprintf(stderr, "recorded webcam is missing or not mirrored\n");
        return false;
    }
    // The square's rounded corner is cut away, so the bubble's own corner
    // should match the desktop nearby rather than the camera. Compare against
    // a desktop pixel instead of assuming the wallpaper is not red or blue.
    const QColor edge = image.pixelColor(180, 83);
    const QColor corner = image.pixelColor(160, 80);
    const QColor outside = image.pixelColor(150, 70);
    const auto colorDistance = [](const QColor& first, const QColor& second) {
        return std::abs(first.red() - second.red())
            + std::abs(first.green() - second.green())
            + std::abs(first.blue() - second.blue());
    };
    if (edge.blue() < 150 || edge.red() > 100) {
        std::fprintf(stderr, "recorded webcam does not reach its top edge\n");
        return false;
    }
    if (colorDistance(corner, outside) > 60) {
        std::fprintf(stderr, "recorded webcam kept its square corner: corner=%s outside=%s\n",
                     qPrintable(corner.name()),
                     qPrintable(outside.name()));
        return false;
    }
    if (previousFfmpeg.isEmpty())
        qunsetenv("APEXSHOT_FFMPEG");
    else
        qputenv("APEXSHOT_FFMPEG", previousFfmpeg);
    return true;
}

} // namespace

int main(int argc, char** argv)
{
    QApplication app(argc, argv);
    if (!checkBubble() || !checkFormats() || !checkPlacementPersistence()) {
        std::fprintf(stderr, "webcam controls failed\n");
        return 1;
    }
    const QString fake = QCoreApplication::applicationDirPath()
                         + QStringLiteral("/apexshot-fake-webcam.exe");
    if (!checkRecording(fake, true) || !checkRecording(fake, false))
        return 1;
    std::puts("webcam setup and continuous recording passed");
    return 0;
}
