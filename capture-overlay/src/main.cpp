#include "CaptureOverlay.h"
#include "RecordingControlsWindow.h"
#include "WindowPickerOverlay.h"
#include "ScreenCapture.h"

#include <QApplication>
#include <QIcon>
#include <QPixmap>
#include <QSize>
#include <QScreen>
#include <QDBusConnection>
#include <QDBusInterface>
#include <QDBusReply>
#include <QTemporaryFile>
#include <QFileInfo>
#include <QDir>
#include <QLocalServer>
#include <QLocalSocket>
#include <QStandardPaths>
#include <QUrl>
#include <QEventLoop>
#include <QThread>

#include <QCoreApplication>
#include <cstdio>
#include <cstring>

// Helper QObject to receive the async XDG portal Screenshot response
class PortalResponse : public QObject
{
    Q_OBJECT
public:
    QString uri;
    bool    got = false;
    QEventLoop loop;

public slots:
    void onResponse(uint code, const QVariantMap& results)
    {
        if (code == 0)
            uri = results.value(QStringLiteral("uri")).toString();
        got = true;
        loop.quit();
    }
};

namespace {

constexpr int kExitRecordConfigUpdated = 6;
constexpr int kExitForwardedToExistingOverlay = 10;
constexpr char kOverlayFocusRequest[] = "focus";
constexpr char kOverlayCancelRequest[] = "cancel";

QString overlaySocketPath()
{
    const QString runtimeDir = qEnvironmentVariable("XDG_RUNTIME_DIR");
    const QString baseDir = runtimeDir.isEmpty() ? QDir::tempPath() : runtimeDir;
    return QDir(baseDir).filePath(QStringLiteral("apexshot-capture-overlay.sock"));
}

bool forwardOverlayRequestToExistingOverlay(const QString& socketPath, const char* request)
{
    QLocalSocket socket;
    socket.connectToServer(socketPath);
    if (!socket.waitForConnected(150)) {
        return false;
    }

    socket.write(request);
    socket.write("\n");
    socket.flush();
    socket.waitForBytesWritten(150);
    socket.disconnectFromServer();
    return true;
}

template <typename FocusFn, typename CancelFn>
void handleOverlayControlPayload(const QByteArray& payload, FocusFn&& focusFn, CancelFn&& cancelFn)
{
    if (payload == kOverlayFocusRequest) {
        focusFn();
    } else if (payload == kOverlayCancelRequest) {
        cancelFn();
    }
}

QByteArray jsonEscape(const QString& value)
{
    const auto utf8 = value.toUtf8();
    QByteArray escaped;
    escaped.reserve(utf8.size() + 8);

    for (const char c : utf8) {
        switch (c) {
        case '"':
            escaped += "\\\"";
            break;
        case '\\':
            escaped += "\\\\";
            break;
        case '\n':
            escaped += "\\n";
            break;
        case '\r':
            escaped += "\\r";
            break;
        case '\t':
            escaped += "\\t";
            break;
        default:
            escaped += c;
            break;
        }
    }
    return escaped;
}

void printCaptureScreenJson(const QString& path, const QSize& size, const char* mode = nullptr)
{
    const auto escapedPath = jsonEscape(path);
    if (mode && *mode) {
        std::printf("{\"path\":\"%s\",\"width\":%d,\"height\":%d,\"mode\":\"%s\"}\n",
                    escapedPath.constData(),
                    size.width(),
                    size.height(),
                    mode);
    } else {
        std::printf("{\"path\":\"%s\",\"width\":%d,\"height\":%d}\n",
                    escapedPath.constData(),
                    size.width(),
                    size.height());
    }
    std::fflush(stdout);
}

void printRecordingJson(const QRect& sel, const char* mode, const char* recordType,
                         bool controls, bool mic, bool speaker,
                         bool clicks, bool keystrokes,
                         bool webcam, double clickSize, int clickColor, int clickStyle,
                         bool clickAnimate, double keySize, int keyPosition,
                         int keyAppearance, bool keyBlurBg, int keyFilter,
                         int webcamSize, int webcamShape, bool webcamFlip,
                         int webcamDevice, double webcamRelX, double webcamRelY,
                         bool displayRecTime, bool hidpi, bool doNotDisturb,
                         bool showCursor, bool rememberSelection,
                         bool dimScreen, bool countdown,
                         int videoFormat, int videoMaxRes, int videoFps, bool recordMono, bool openEditor,
                         int gifFps, double gifQuality, int gifSizeIdx, bool optimizeGif,
                         bool fullscreen)
{
    std::printf("{\"x\":%d,\"y\":%d,\"width\":%d,\"height\":%d,"
                "\"mode\":\"%s\",\"record_type\":\"%s\","
                "\"controls\":%s,\"mic\":%s,\"speaker\":%s,"
                "\"clicks\":%s,\"keystrokes\":%s,"
                "\"webcam\":%s,\"click_size\":%.4f,\"click_color\":%d,"
                "\"click_style\":%d,\"click_animate\":%s,"
                "\"key_size\":%.4f,\"key_position\":%d,\"key_appearance\":%d,"
                "\"key_blur_bg\":%s,\"key_filter\":%d,"
                "\"webcam_size\":%d,\"webcam_shape\":%d,\"webcam_flip\":%s,"
                "\"webcam_device\":%d,\"webcam_rel_x\":%.4f,\"webcam_rel_y\":%.4f,"
                "\"display_rec_time\":%s,\"hidpi\":%s,"
                "\"notifications\":%s,\"cursor\":%s,"
                "\"remember_selection\":%s,\"dim_screen\":%s,"
                "\"countdown\":%s,"
                "\"video_format\":%d,\"video_max_res\":%d,\"video_fps\":%d,"
                "\"record_mono\":%s,\"open_editor\":%s,"
                "\"gif_fps\":%d,\"gif_quality\":%.4f,"
                "\"gif_size_idx\":%d,\"optimize_gif\":%s,\"fullscreen\":%s}\n",
                sel.x(), sel.y(), sel.width(), sel.height(),
                mode,
                recordType,
                controls ? "true" : "false",
                mic ? "true" : "false",
                speaker ? "true" : "false",
                clicks ? "true" : "false",
                keystrokes ? "true" : "false",
                webcam ? "true" : "false",
                clickSize,
                clickColor,
                clickStyle,
                clickAnimate ? "true" : "false",
                keySize,
                keyPosition,
                keyAppearance,
                keyBlurBg ? "true" : "false",
                keyFilter,
                webcamSize,
                webcamShape,
                webcamFlip ? "true" : "false",
                webcamDevice,
                webcamRelX,
                webcamRelY,
                displayRecTime ? "true" : "false",
                hidpi ? "true" : "false",
                doNotDisturb ? "true" : "false",
                showCursor ? "true" : "false",
                rememberSelection ? "true" : "false",
                dimScreen ? "true" : "false",
                countdown ? "true" : "false",
                0,
                videoMaxRes,
                videoFps,
                recordMono ? "true" : "false",
                openEditor ? "true" : "false",
                gifFps,
                gifQuality,
                gifSizeIdx,
                optimizeGif ? "true" : "false",
                fullscreen ? "true" : "false");
    std::fflush(stdout);
}

} // namespace

int main(int argc, char* argv[])
{
    qputenv("QT_QPA_PLATFORM", "");
    qputenv("QT_IM_MODULE", "compose");

    QApplication app(argc, argv);
    app.setApplicationName("ApexShot Capture");
    app.setDesktopFileName("io.github.codegoddy.apexshot");
    app.setWindowIcon(QIcon::fromTheme("io.github.codegoddy.apexshot"));

    if (!QDBusConnection::sessionBus().isConnected()) {
        std::fprintf(stderr, "apexshot-capture: session bus not connected: %s\n", 
            QDBusConnection::sessionBus().lastError().message().toLocal8Bit().constData());
    }

    bool captureScreenMode = false;
    bool areaInitMode = false;
    bool crosshairCaptureMode = false;
    bool windowCaptureMode = false;
    bool recordControlsMode = false;
    bool timerCaptureEnabled = false;
    QString backgroundPath;
    QString controlDbusDest;
    QString controlSessionId;
    int controlCaptureX = 0;
    int controlCaptureY = 0;
    int controlCaptureW = 0;
    int controlCaptureH = 0;
    bool controlFullscreen = false;
    bool controlShowTimer = true;
    QRect restoreSel;
    bool initialMic = false;
    bool initialSpeaker = false;
    bool initialRecControls = true;
    bool initialDisplayRecTime = false;
    bool initialHidpi = true;
    bool initialDoNotDisturb = true;
    bool initialShowCursor = true;
    bool initialRecClicks = false;
    bool initialRecKeystrokes = false;
    bool initialRecWebcam = false;
    double initialClickSize = 0.3;
    int initialClickColor = 0;
    int initialClickStyle = 0;
    bool initialClickAnimate = true;
    double initialKeySize = 0.32;
    int initialKeyPosition = 0;
    int initialKeyAppearance = 0;
    bool initialKeyBlurBg = true;
    int initialKeyFilter = 0;
    int initialWebcamSize = 1;
    int initialWebcamShape = 3;
    bool initialWebcamFlip = false;
    int initialWebcamDevice = -1;
    double initialWebcamRelX = 0.0;
    double initialWebcamRelY = 0.0;
    bool initialRememberSelection = false;
    bool initialDimScreen = true;
    bool initialShowCountdown = true;
    int initialCaptureDelaySeconds = 5;
    QString selectionCursor = QStringLiteral("Disabled");
    bool showZoomPreview = false;
    bool freezeSelectionBackground = true;
    int initialVideoFormat = 0;
    int initialVideoMaxRes = 0;
    int initialVideoFps = 2;
    bool initialRecordMono = false;
    bool initialOpenEditor = true;
    int initialGifFps = 50;
    double initialGifQuality = 0.75;
    int initialGifSizeIdx = 0;
    bool initialGifOptimize = true;
    bool openRecordingUiMode = false;
    const QString sessionSocketPath = overlaySocketPath();
    QLocalServer sessionServer;

    for (int i = 1; i < argc; ++i) {
        if (std::strcmp(argv[i], "--background") == 0 && i + 1 < argc) {
            backgroundPath = QString::fromLocal8Bit(argv[i + 1]);
            ++i;
        } else if (std::strcmp(argv[i], "--capture-screen") == 0) {
            captureScreenMode = true;
        } else if (std::strcmp(argv[i], "--area-init") == 0) {
            areaInitMode = true;
        } else if (std::strcmp(argv[i], "--crosshair-capture") == 0) {
            crosshairCaptureMode = true;
        } else if (std::strcmp(argv[i], "--open-recording-ui") == 0) {
            openRecordingUiMode = true;
        } else if (std::strcmp(argv[i], "--window-capture") == 0) {
            windowCaptureMode = true;
        } else if (std::strcmp(argv[i], "--record-controls") == 0) {
            recordControlsMode = true;
        } else if (QString(argv[i]).startsWith("--dbus-dest=")) {
            controlDbusDest = QString(argv[i]).mid(12);
        } else if (QString(argv[i]).startsWith("--session-id=")) {
            controlSessionId = QString(argv[i]).mid(13);
        } else if (QString(argv[i]).startsWith("--capture-x=")) {
            controlCaptureX = QString(argv[i]).mid(12).toInt();
        } else if (QString(argv[i]).startsWith("--capture-y=")) {
            controlCaptureY = QString(argv[i]).mid(12).toInt();
        } else if (QString(argv[i]).startsWith("--capture-w=")) {
            controlCaptureW = QString(argv[i]).mid(12).toInt();
        } else if (QString(argv[i]).startsWith("--capture-h=")) {
            controlCaptureH = QString(argv[i]).mid(12).toInt();
        } else if (std::strcmp(argv[i], "--fullscreen") == 0) {
            controlFullscreen = true;
        } else if (std::strcmp(argv[i], "--show-timer") == 0) {
            controlShowTimer = true;
        } else if (std::strcmp(argv[i], "--hide-timer") == 0) {
            controlShowTimer = false;
        } else if (QString(argv[i]).startsWith("--timer-seconds=")) {
            initialCaptureDelaySeconds = std::max(0, QString(argv[i]).mid(16).toInt());
        } else if (QString(argv[i]).startsWith("--restore-selection=")) {
            // Format: --restore-selection=x,y,w,h
            QString val = QString(argv[i]).mid(20);
            QStringList parts = val.split(',');
            if (parts.size() == 4) {
                restoreSel = QRect(parts[0].toInt(), parts[1].toInt(),
                                   parts[2].toInt(), parts[3].toInt());
            }
        } else if (QString(argv[i]).startsWith("--selection-cursor=")) {
            selectionCursor = QString(argv[i]).mid(19);
        } else if (QString(argv[i]).startsWith("--show-zoom-preview=")) {
            showZoomPreview = QString(argv[i]).mid(20) == QStringLiteral("1");
        } else if (QString(argv[i]).startsWith("--freeze-selection-bg=")) {
            freezeSelectionBackground = QString(argv[i]).mid(22) == QStringLiteral("1");
        } else if (std::strcmp(argv[i], "--rec-mic") == 0) {
            initialMic = true;
        } else if (std::strcmp(argv[i], "--rec-speaker") == 0) {
            initialSpeaker = true;
        } else if (std::strcmp(argv[i], "--rec-controls") == 0) {
            initialRecControls = true;
        } else if (std::strcmp(argv[i], "--no-rec-controls") == 0) {
            initialRecControls = false;
        } else if (std::strcmp(argv[i], "--display-rec-time") == 0) {
            initialDisplayRecTime = true;
        } else if (std::strcmp(argv[i], "--no-display-rec-time") == 0) {
            initialDisplayRecTime = false;
        } else if (std::strcmp(argv[i], "--hidpi") == 0) {
            initialHidpi = true;
        } else if (std::strcmp(argv[i], "--no-hidpi") == 0) {
            initialHidpi = false;
        } else if (std::strcmp(argv[i], "--do-not-disturb") == 0) {
            initialDoNotDisturb = true;
        } else if (std::strcmp(argv[i], "--no-do-not-disturb") == 0) {
            initialDoNotDisturb = false;
        } else if (std::strcmp(argv[i], "--show-cursor") == 0) {
            initialShowCursor = true;
        } else if (std::strcmp(argv[i], "--no-show-cursor") == 0) {
            initialShowCursor = false;
        } else if (std::strcmp(argv[i], "--rec-clicks") == 0) {
            initialRecClicks = true;
        } else if (std::strcmp(argv[i], "--no-rec-clicks") == 0) {
            initialRecClicks = false;
        } else if (std::strcmp(argv[i], "--rec-keystrokes") == 0) {
            initialRecKeystrokes = true;
        } else if (std::strcmp(argv[i], "--no-rec-keystrokes") == 0) {
            initialRecKeystrokes = false;
        } else if (QString(argv[i]).startsWith("--rec-click-size=")) {
            bool ok = false;
            double v = QString(argv[i]).mid(17).toDouble(&ok);
            if (ok) initialClickSize = std::clamp(v, 0.0, 1.0);
        } else if (QString(argv[i]).startsWith("--rec-click-color=")) {
            bool ok = false;
            int v = QString(argv[i]).mid(18).toInt(&ok);
            if (ok) initialClickColor = std::clamp(v, 0, 8);
        } else if (QString(argv[i]).startsWith("--rec-click-style=")) {
            bool ok = false;
            int v = QString(argv[i]).mid(18).toInt(&ok);
            if (ok) initialClickStyle = std::clamp(v, 0, 1);
        } else if (std::strcmp(argv[i], "--rec-click-animate") == 0) {
            initialClickAnimate = true;
        } else if (std::strcmp(argv[i], "--no-rec-click-animate") == 0) {
            initialClickAnimate = false;
        } else if (QString(argv[i]).startsWith("--rec-key-size=")) {
            bool ok = false;
            double v = QString(argv[i]).mid(15).toDouble(&ok);
            if (ok) initialKeySize = std::clamp(v, 0.0, 1.0);
        } else if (QString(argv[i]).startsWith("--rec-key-position=")) {
            bool ok = false;
            int v = QString(argv[i]).mid(19).toInt(&ok);
            if (ok) initialKeyPosition = std::clamp(v, 0, 5);
        } else if (QString(argv[i]).startsWith("--rec-key-appearance=")) {
            bool ok = false;
            int v = QString(argv[i]).mid(21).toInt(&ok);
            if (ok) initialKeyAppearance = std::clamp(v, 0, 1);
        } else if (std::strcmp(argv[i], "--rec-key-blur-bg") == 0) {
            initialKeyBlurBg = true;
        } else if (std::strcmp(argv[i], "--no-rec-key-blur-bg") == 0) {
            initialKeyBlurBg = false;
        } else if (QString(argv[i]).startsWith("--rec-key-filter=")) {
            bool ok = false;
            int v = QString(argv[i]).mid(17).toInt(&ok);
            if (ok) initialKeyFilter = std::clamp(v, 0, 1);
        } else if (std::strcmp(argv[i], "--rec-webcam") == 0) {
            initialRecWebcam = true;
        } else if (std::strcmp(argv[i], "--no-rec-webcam") == 0) {
            initialRecWebcam = false;
        } else if (QString(argv[i]).startsWith("--rec-webcam-size=")) {
            bool ok = false;
            int v = QString(argv[i]).mid(18).toInt(&ok);
            if (ok) initialWebcamSize = std::clamp(v, 0, 4);
        } else if (QString(argv[i]).startsWith("--rec-webcam-shape=")) {
            bool ok = false;
            int v = QString(argv[i]).mid(19).toInt(&ok);
            if (ok) initialWebcamShape = std::clamp(v, 0, 3);
        } else if (std::strcmp(argv[i], "--rec-webcam-flip") == 0) {
            initialWebcamFlip = true;
        } else if (std::strcmp(argv[i], "--no-rec-webcam-flip") == 0) {
            initialWebcamFlip = false;
        } else if (QString(argv[i]).startsWith("--rec-webcam-device=")) {
            bool ok = false;
            int v = QString(argv[i]).mid(20).toInt(&ok);
            if (ok) initialWebcamDevice = v;
        } else if (QString(argv[i]).startsWith("--rec-webcam-rel-x=")) {
            bool ok = false;
            double v = QString(argv[i]).mid(19).toDouble(&ok);
            if (ok) initialWebcamRelX = std::clamp(v, 0.0, 1.0);
        } else if (QString(argv[i]).startsWith("--rec-webcam-rel-y=")) {
            bool ok = false;
            double v = QString(argv[i]).mid(19).toDouble(&ok);
            if (ok) initialWebcamRelY = std::clamp(v, 0.0, 1.0);
        } else if (std::strcmp(argv[i], "--remember-selection") == 0) {
            initialRememberSelection = true;
        } else if (std::strcmp(argv[i], "--no-remember-selection") == 0) {
            initialRememberSelection = false;
        } else if (std::strcmp(argv[i], "--dim-screen") == 0) {
            initialDimScreen = true;
        } else if (std::strcmp(argv[i], "--no-dim-screen") == 0) {
            initialDimScreen = false;
        } else if (std::strcmp(argv[i], "--show-countdown") == 0) {
            initialShowCountdown = true;
        } else if (std::strcmp(argv[i], "--no-show-countdown") == 0) {
            initialShowCountdown = false;
        } else if (QString(argv[i]).startsWith("--video-format=")) {
            bool ok = false;
            int v = QString(argv[i]).mid(15).toInt(&ok);
            if (ok && v >= 0 && v <= 1) initialVideoFormat = v;
        } else if (QString(argv[i]).startsWith("--video-max-res=")) {
            bool ok = false;
            int v = QString(argv[i]).mid(16).toInt(&ok);
            if (ok && v >= 0 && v <= 2) initialVideoMaxRes = v;
        } else if (QString(argv[i]).startsWith("--video-fps=")) {
            bool ok = false;
            int v = QString(argv[i]).mid(12).toInt(&ok);
            if (ok && v >= 0 && v <= 3) initialVideoFps = v;
        } else if (std::strcmp(argv[i], "--record-mono") == 0) {
            initialRecordMono = true;
        } else if (std::strcmp(argv[i], "--no-record-mono") == 0) {
            initialRecordMono = false;
        } else if (std::strcmp(argv[i], "--open-editor") == 0) {
            initialOpenEditor = true;
        } else if (std::strcmp(argv[i], "--no-open-editor") == 0) {
            initialOpenEditor = false;
        } else if (QString(argv[i]).startsWith("--gif-fps=")) {
            bool ok = false;
            int v = QString(argv[i]).mid(10).toInt(&ok);
            if (ok && v >= 5 && v <= 60) initialGifFps = v;
        } else if (QString(argv[i]).startsWith("--gif-quality=")) {
            bool ok = false;
            double v = QString(argv[i]).mid(14).toDouble(&ok);
            if (ok && v >= 0.0 && v <= 1.0) initialGifQuality = v;
        } else if (QString(argv[i]).startsWith("--gif-size=")) {
            bool ok = false;
            int v = QString(argv[i]).mid(11).toInt(&ok);
            if (ok && v >= 0 && v <= 3) initialGifSizeIdx = v;
        } else if (std::strcmp(argv[i], "--gif-optimize") == 0) {
            initialGifOptimize = true;
        } else if (std::strcmp(argv[i], "--no-gif-optimize") == 0) {
            initialGifOptimize = false;
        }
    }

    if ((captureScreenMode && areaInitMode) ||
        (captureScreenMode && crosshairCaptureMode) ||
        (areaInitMode && crosshairCaptureMode)) {
        std::fprintf(stderr,
                     "apexshot-capture: interactive capture modes are mutually exclusive\n");
        return 2;
    }

    const bool interactiveOverlayMode =
      !captureScreenMode && !recordControlsMode;
    if (interactiveOverlayMode) {
        if (forwardOverlayRequestToExistingOverlay(sessionSocketPath, kOverlayFocusRequest)) {
            return kExitForwardedToExistingOverlay;
        }

        QLocalServer::removeServer(sessionSocketPath);
        if (!sessionServer.listen(sessionSocketPath)) {
            if (forwardOverlayRequestToExistingOverlay(sessionSocketPath, kOverlayFocusRequest)) {
                return kExitForwardedToExistingOverlay;
            }

            std::fprintf(stderr,
                         "apexshot-capture: failed to listen on overlay socket %s: %s\n",
                         sessionSocketPath.toLocal8Bit().constData(),
                         sessionServer.errorString().toLocal8Bit().constData());
            return 2;
        }
    }

    if (recordControlsMode) {
        if (controlDbusDest.isEmpty() || controlSessionId.isEmpty()) {
            std::fprintf(stderr, "apexshot-capture: --record-controls requires --dbus-dest and --session-id\n");
            return 2;
        }

        RecordingControlsWindow controls(
          controlDbusDest,
          controlSessionId,
          QRect(controlCaptureX, controlCaptureY, controlCaptureW, controlCaptureH),
          controlFullscreen,
          controlShowTimer);
        controls.show();
        return app.exec();
    }

    if (captureScreenMode) {
        QString imagePath;
        QSize imageSize;
        QString error;
        if (!ScreenCapture::captureFullscreenToTempPng(imagePath, imageSize, error)) {
            std::fprintf(stderr,
                         "apexshot-capture: fullscreen capture failed: %s\n",
                         error.toLocal8Bit().constData());
            return 2;
        }
        printCaptureScreenJson(imagePath, imageSize);
        return 0;
    }

    if (windowCaptureMode) {
        // Show our custom window picker overlay UI
        WindowPickerOverlay picker;
        QObject::connect(&sessionServer, &QLocalServer::newConnection, [&sessionServer, &picker, &app]() {
            while (QLocalSocket* socket = sessionServer.nextPendingConnection()) {
                QObject::connect(socket, &QLocalSocket::readyRead, [socket, &picker, &app]() {
                    const QByteArray payload = socket->readAll().trimmed();
                    handleOverlayControlPayload(
                        payload,
                        [&picker]() { picker.focusAndRaiseOverlay(); },
                        [&picker, &app]() {
                            picker.hide();
                            app.exit(1);
                        });
                });
                QObject::connect(socket, &QLocalSocket::disconnected, socket, &QObject::deleteLater);
                if (socket->bytesAvailable() > 0) {
                    const QByteArray payload = socket->readAll().trimmed();
                    handleOverlayControlPayload(
                        payload,
                        [&picker]() { picker.focusAndRaiseOverlay(); },
                        [&picker, &app]() {
                            picker.hide();
                            app.exit(1);
                        });
                }
            }
        });
        const int ret = app.exec();

        if (ret != 3 || !picker.wasSelected()) {
            // User cancelled or no selection
            return 1;
        }

        // User selected a window — capture it via GNOME Shell DBus using XID
        AppWindowInfo selected = picker.selectedWindow();
        std::fprintf(stderr, "apexshot-capture: capturing window '%s' (xid=%llu)\n",
            selected.title.toLocal8Bit().constData(),
            (unsigned long long)selected.xid);

        // Activate the selected window before capture so GNOME Shell captures
        // that real window surface instead of composited pixels underneath the
        // picker overlay.
        {
            QDBusInterface windowListIface(
                QStringLiteral("org.apexshot.WindowList"),
                QStringLiteral("/org/apexshot/WindowList"),
                QStringLiteral("org.apexshot.WindowList"),
                QDBusConnection::sessionBus());

            if (windowListIface.isValid()) {
                QDBusReply<bool> activateReply = windowListIface.call(
                    QStringLiteral("ActivateWindowById"),
                    static_cast<quint32>(selected.xid));
                if (!activateReply.isValid() || !activateReply.value()) {
                    std::fprintf(stderr,
                                 "apexshot-capture: failed to activate selected window before capture\n");
                }
            }
        }

        QApplication::processEvents(QEventLoop::AllEvents, 50);
        QThread::msleep(180);
        QApplication::processEvents(QEventLoop::AllEvents, 50);

        // Use GNOME Shell ScreenshotWindow to capture the active selected window.
        const QString tmpPath = QDir::tempPath() + QStringLiteral("/apexshot-window-%1.png")
                                    .arg(QCoreApplication::applicationPid());

        QDBusInterface gnomeShot(
            QStringLiteral("org.gnome.Shell.Screenshot"),
            QStringLiteral("/org/gnome/Shell/Screenshot"),
            QStringLiteral("org.gnome.Shell.Screenshot"),
            QDBusConnection::sessionBus());

        if (!gnomeShot.isValid()) {
            std::fprintf(stderr, "apexshot-capture: GNOME Shell Screenshot DBus not available\n");
            return 2;
        }

        QDBusReply<bool> reply = gnomeShot.call(
            QStringLiteral("ScreenshotWindow"),
            true,   // include_frame
            false,  // include_cursor
            false,  // flash
            tmpPath);

        if (!reply.isValid() || !reply.value()) {
            // Fallback: capture the rect of the selected window from a fullscreen shot
            std::fprintf(stderr, "apexshot-capture: ScreenshotWindow failed, using rect fallback\n");
            QString imagePath;
            QSize imageSize;
            QString error;
            if (ScreenCapture::captureAreaToTempPng(selected.rect, imagePath, imageSize, error)) {
                printCaptureScreenJson(imagePath, imageSize);
                return 0;
            }
            std::fprintf(stderr, "apexshot-capture: rect fallback also failed: %s\n",
                error.toLocal8Bit().constData());
            return 2;
        }

        QString actualPath = tmpPath;
        if (!QFileInfo::exists(actualPath)) {
            for (const QString& suffix : {".png", "-1.png", "-0.png"}) {
                QString candidate = tmpPath + suffix;
                if (QFileInfo::exists(candidate)) { actualPath = candidate; break; }
            }
        }

        QPixmap pm(actualPath);
        if (pm.isNull()) {
            std::fprintf(stderr, "apexshot-capture: could not load window screenshot: %s\n",
                         actualPath.toLocal8Bit().constData());
            return 2;
        }

        printCaptureScreenJson(actualPath, pm.size());
        return 0;
    }

    QPixmap background;
    if (!backgroundPath.isEmpty()) {
        if (!background.load(backgroundPath)) {
            std::fprintf(stderr,
                         "apexshot-capture: failed to load background image: %s\n",
                         backgroundPath.toLocal8Bit().constData());
            return 2;
        }
    }

    const CaptureOverlay::OverlayMode overlayMode =
      crosshairCaptureMode ? CaptureOverlay::OverlayMode::CrosshairCapture
                           : CaptureOverlay::OverlayMode::StandardArea;
    CaptureOverlay overlay(background, nullptr, timerCaptureEnabled, initialMic, initialSpeaker, overlayMode);
    QObject::connect(&sessionServer, &QLocalServer::newConnection, [&sessionServer, &overlay, &app]() {
        while (QLocalSocket* socket = sessionServer.nextPendingConnection()) {
            QObject::connect(socket, &QLocalSocket::readyRead, [socket, &overlay, &app]() {
                const QByteArray payload = socket->readAll().trimmed();
                handleOverlayControlPayload(
                    payload,
                    [&overlay]() { overlay.focusAndRaiseOverlay(); },
                    [&overlay, &app]() {
                        overlay.hide();
                        app.exit(1);
                    });
            });
            QObject::connect(socket, &QLocalSocket::disconnected, socket, &QObject::deleteLater);
            if (socket->bytesAvailable() > 0) {
                const QByteArray payload = socket->readAll().trimmed();
                handleOverlayControlPayload(
                    payload,
                    [&overlay]() { overlay.focusAndRaiseOverlay(); },
                    [&overlay, &app]() {
                        overlay.hide();
                        app.exit(1);
                    });
            }
        }
    });
    overlay.setSelectionCursorMode(selectionCursor);
    overlay.setShowZoomPreview(showZoomPreview);
    overlay.setFreezeSelectionBackground(freezeSelectionBackground);
    overlay.setInitialCaptureDelaySeconds(initialCaptureDelaySeconds);
    if (!restoreSel.isNull() && restoreSel.isValid()) {
        overlay.setInitialSelection(restoreSel);
    }
    overlay.setInitialGifFps(initialGifFps);
    overlay.setInitialGifQuality(initialGifQuality);
    overlay.setInitialGifSizeIdx(initialGifSizeIdx);
    overlay.setInitialGifOptimize(initialGifOptimize);
    overlay.setInitialRecControls(initialRecControls);
    overlay.setInitialDisplayRecTime(initialDisplayRecTime);
    overlay.setInitialHidpi(initialHidpi);
    overlay.setInitialDoNotDisturb(initialDoNotDisturb);
    overlay.setInitialShowCursor(initialShowCursor);
    overlay.setInitialRecClicks(initialRecClicks);
    overlay.setInitialRecKeystrokes(initialRecKeystrokes);
    overlay.setInitialRecWebcam(initialRecWebcam);
    overlay.setInitialClickSize(initialClickSize);
    overlay.setInitialClickColor(initialClickColor);
    overlay.setInitialClickStyle(initialClickStyle);
    overlay.setInitialClickAnimate(initialClickAnimate);
    overlay.setInitialKeySize(initialKeySize);
    overlay.setInitialKeyPosition(initialKeyPosition);
    overlay.setInitialKeyAppearance(initialKeyAppearance);
    overlay.setInitialKeyBlurBg(initialKeyBlurBg);
    overlay.setInitialKeyFilter(initialKeyFilter);
    overlay.setInitialWebcamSize(initialWebcamSize);
    overlay.setInitialWebcamShape(initialWebcamShape);
    overlay.setInitialWebcamFlip(initialWebcamFlip);
    overlay.setInitialWebcamDevice(initialWebcamDevice);
    overlay.setInitialWebcamRelX(initialWebcamRelX);
    overlay.setInitialWebcamRelY(initialWebcamRelY);
    overlay.setInitialRememberSelection(initialRememberSelection);
    overlay.setInitialDimScreen(initialDimScreen);
    overlay.setInitialShowCountdown(initialShowCountdown);
    overlay.setInitialVideoFormat(initialVideoFormat);
    overlay.setInitialVideoMaxRes(initialVideoMaxRes);
    overlay.setInitialVideoFps(initialVideoFps);
    overlay.setInitialRecordMono(initialRecordMono);
    overlay.setInitialOpenEditor(initialOpenEditor);
    if (openRecordingUiMode) {
        overlay.openRecordingPanelForShortcut();
    }
    overlay.show();

    const int ret = app.exec();
    if (interactiveOverlayMode) {
        sessionServer.close();
        QLocalServer::removeServer(sessionSocketPath);
    }

    if (ret == 3) {
        // Window capture requested via toolbar button
        return 3;
    }
    if (ret == kExitRecordConfigUpdated) {
        if (areaInitMode && overlay.recordConfigRequested()) {
            const QRect sel = overlay.selection();
            int screenHeight = 0;
            for (QScreen* screen : QGuiApplication::screens()) {
                screenHeight = std::max(screenHeight, screen->geometry().height());
            }
            const int yOffset = screenHeight - overlay.height();
            const QRect selGlobal = sel.translated(overlay.geometry().x(), yOffset);
            const char* recordType = "video";
            if (overlay.recordType() == CaptureOverlay::RecordType::Gif) {
                recordType = "gif";
            }
            printRecordingJson(selGlobal, "record-config", recordType,
                               overlay.recordControlsEnabled(),
                               overlay.recordMicEnabled(),
                               overlay.recordSpeakerEnabled(),
                               overlay.recordClicksEnabled(),
                               overlay.recordKeystrokesEnabled(),
                               overlay.recordWebcamEnabled(),
                               overlay.recordClickSize(),
                               overlay.recordClickColor(),
                               overlay.recordClickStyle(),
                               overlay.recordClickAnimate(),
                               overlay.recordKeySize(),
                               overlay.recordKeyPosition(),
                               overlay.recordKeyAppearance(),
                               overlay.recordKeyBlurBg(),
                               overlay.recordKeyFilter(),
                               overlay.recordWebcamSize(),
                               overlay.recordWebcamShape(),
                               overlay.recordWebcamFlip(),
                               overlay.recordWebcamDevice(),
                               overlay.recordWebcamRelX(),
                               overlay.recordWebcamRelY(),
                               overlay.recordDisplayRecTime(),
                               overlay.recordHidpiEnabled(),
                               overlay.recordDoNotDisturb(),
                               overlay.recordShowCursor(),
                               overlay.recordRememberSelection(),
                               overlay.recordDimScreen(),
                               overlay.recordShowCountdown(),
                               overlay.recordVideoFormat(),
                               overlay.recordVideoMaxRes(),
                               overlay.recordVideoFps(),
                               overlay.recordMono(),
                               overlay.recordOpenEditor(),
                               overlay.recordGifFps(),
                               overlay.recordGifQuality(),
                               overlay.recordGifSizeIdx(),
                               overlay.recordOptimizeGif(),
                               overlay.recordFullscreen());
            return kExitRecordConfigUpdated;
        }
        std::fprintf(stderr,
                     "apexshot-capture: record-config exit requested without explicit record-config state\n");
        return 2;
    }
    if (ret != 0) {
        std::fprintf(stderr, "apexshot-capture: event loop exited with code %d\n", ret);
        return 1;
    }

    if (areaInitMode && overlay.scrollCaptureCompleted()) {
        const QString scrollPath = overlay.scrollCapturePath();
        const QSize scrollSize = overlay.scrollCaptureSize();
        if (scrollPath.isEmpty() || scrollSize.isEmpty() || !QFileInfo::exists(scrollPath)) {
            std::fprintf(stderr,
                         "apexshot-capture: scroll capture completed but output is missing\n");
            return 2;
        }
        printCaptureScreenJson(scrollPath, scrollSize, "scroll");
        return 0;
    }

    const QRect sel = overlay.selection();
    if (sel.isEmpty()) {
        std::fprintf(stderr, "apexshot-capture: empty selection\n");
        return 2;
    }

    // Calculate Y offset: the overlay may not cover the full screen height
    // (e.g., GNOME top bar is not covered on Wayland without Layer Shell)
    int screenHeight = 0;
    for (QScreen* screen : QGuiApplication::screens()) {
        screenHeight = std::max(screenHeight, screen->geometry().height());
    }
    const int yOffset = screenHeight - overlay.height();

    // Handle recording request
    if (overlay.recordRequested()) {
        const char* recordType = "video";
        if (overlay.recordType() == CaptureOverlay::RecordType::Gif) {
            recordType = "gif";
        }
        // Translate from local overlay coords to global screen coords
        // Include yOffset because overlay doesn't cover the top bar
        const QRect selGlobal = sel.translated(overlay.geometry().x(), yOffset);
        printRecordingJson(selGlobal, "record", recordType,
                           overlay.recordControlsEnabled(),
                           overlay.recordMicEnabled(),
                           overlay.recordSpeakerEnabled(),
                           overlay.recordClicksEnabled(),
                           overlay.recordKeystrokesEnabled(),
                           overlay.recordWebcamEnabled(),
                           overlay.recordClickSize(),
                           overlay.recordClickColor(),
                           overlay.recordClickStyle(),
                           overlay.recordClickAnimate(),
                           overlay.recordKeySize(),
                           overlay.recordKeyPosition(),
                           overlay.recordKeyAppearance(),
                           overlay.recordKeyBlurBg(),
                           overlay.recordKeyFilter(),
                           overlay.recordWebcamSize(),
                           overlay.recordWebcamShape(),
                           overlay.recordWebcamFlip(),
                           overlay.recordWebcamDevice(),
                           overlay.recordWebcamRelX(),
                           overlay.recordWebcamRelY(),
                           overlay.recordDisplayRecTime(),
                           overlay.recordHidpiEnabled(),
                           overlay.recordDoNotDisturb(),
                           overlay.recordShowCursor(),
                           overlay.recordRememberSelection(),
                           overlay.recordDimScreen(),
                           overlay.recordShowCountdown(),
                           overlay.recordVideoFormat(),
                           overlay.recordVideoMaxRes(),
                           overlay.recordVideoFps(),
                           overlay.recordMono(),
                           overlay.recordOpenEditor(),
                           overlay.recordGifFps(),
                           overlay.recordGifQuality(),
                           overlay.recordGifSizeIdx(),
                           overlay.recordOptimizeGif(),
                           overlay.recordFullscreen());
        return 0;
    }

    if (areaInitMode || crosshairCaptureMode) {
        const bool ocrRequested = overlay.ocrRequested();
        const bool fullscreenRequested = overlay.recordFullscreen();
        // Translate from local overlay coords to global screen coords
        // Include yOffset because overlay doesn't cover the top bar
        const QRect selGlobal = sel.translated(overlay.geometry().x(), yOffset);
        const bool isWayland = qEnvironmentVariableIsSet("WAYLAND_DISPLAY");
        const QString desktop = qEnvironmentVariable("XDG_CURRENT_DESKTOP");
        const bool isGnomeWayland =
          isWayland &&
          (desktop.contains("GNOME", Qt::CaseInsensitive) ||
           qEnvironmentVariableIsSet("GNOME_SETUP_DISPLAY"));

        QString imagePath;
        QSize imageSize;
        QString error;
        bool ok = false;

        if (crosshairCaptureMode) {
            if (isGnomeWayland) {
                ok = ScreenCapture::captureAreaToTempPngFromOverlayLocal(
                  sel, overlay.geometry(), imagePath, imageSize, error);
            } else {
                ok =
                  ScreenCapture::captureAreaToTempPng(selGlobal, imagePath, imageSize, error);
            }
        } else if (fullscreenRequested) {
            ok = ScreenCapture::captureFullscreenToTempPng(imagePath, imageSize, error);
        } else if (isGnomeWayland) {
            ok = ScreenCapture::captureAreaToTempPngFromOverlayLocal(
              sel, overlay.geometry(), imagePath, imageSize, error);
        } else {
            ok =
              ScreenCapture::captureAreaToTempPng(selGlobal, imagePath, imageSize, error);
        }

        if (!ok && isWayland && !isGnomeWayland) {
            QString fallbackError;
            ok = ScreenCapture::captureAreaToTempPngFromOverlayLocal(
              sel, overlay.geometry(), imagePath, imageSize, fallbackError);
            if (!ok) {
                error = QStringLiteral("%1; overlay-local fallback failed (%2)")
                          .arg(error, fallbackError);
            }
        }
        if (!ok) {
            std::fprintf(stderr,
                         "apexshot-capture: area capture failed: %s\n",
                         error.toLocal8Bit().constData());
            return 2;
        }
        printCaptureScreenJson(
          imagePath,
          imageSize,
          crosshairCaptureMode ? "area" : (ocrRequested ? "ocr" : "area"));
    } else {
        std::printf("{\"x\":%d,\"y\":%d,\"width\":%d,\"height\":%d}\n",
                    sel.x(),
                    sel.y(),
                    sel.width(),
                    sel.height());
        std::fflush(stdout);
    }

    return 0;
}

#include "main.moc"
