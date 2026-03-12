#include "CaptureOverlay.h"
#include "WindowPickerOverlay.h"
#include "ScreenCapture.h"

#include <QApplication>
#include <QPixmap>
#include <QSize>
#include <QDBusConnection>
#include <QDBusInterface>
#include <QDBusReply>
#include <QTemporaryFile>
#include <QFileInfo>
#include <QDir>
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

} // namespace

int main(int argc, char* argv[])
{
    qputenv("QT_QPA_PLATFORM", "");
    qputenv("QT_IM_MODULE", "compose");

    QApplication app(argc, argv);
    app.setApplicationName("ApexShot Capture");

    bool captureScreenMode = false;
    bool areaInitMode = false;
    bool windowCaptureMode = false;
    QString backgroundPath;

    for (int i = 1; i < argc; ++i) {
        if (std::strcmp(argv[i], "--background") == 0 && i + 1 < argc) {
            backgroundPath = QString::fromLocal8Bit(argv[i + 1]);
            ++i;
        } else if (std::strcmp(argv[i], "--capture-screen") == 0) {
            captureScreenMode = true;
        } else if (std::strcmp(argv[i], "--area-init") == 0) {
            areaInitMode = true;
        } else if (std::strcmp(argv[i], "--window-capture") == 0) {
            windowCaptureMode = true;
        }
    }

    if (captureScreenMode && areaInitMode) {
        std::fprintf(stderr,
                     "apexshot-capture: --capture-screen and --area-init are mutually exclusive\n");
        return 2;
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

        // Prefer portal-permission route on Wayland/desktop portals.
        // This keeps window capture aligned with compositor security constraints.
        {
            QString imagePath;
            QSize imageSize;
            QString error;
            if (ScreenCapture::captureAreaToTempPngViaPortal(
                  selected.rect, imagePath, imageSize, error)) {
                printCaptureScreenJson(imagePath, imageSize);
                return 0;
            }
            std::fprintf(stderr,
                         "apexshot-capture: portal window capture failed, falling back to GNOME Shell API: %s\n",
                         error.toLocal8Bit().constData());
        }

        // Use GNOME Shell ScreenshotWindow to capture the selected window
        // First focus the window, then capture
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

    CaptureOverlay overlay(background, nullptr, areaInitMode);
    overlay.show();

    const int ret = app.exec();

    if (ret == 3) {
        // Window capture requested via toolbar button
        return 3;
    }
    if (ret != 0) {
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

    if (areaInitMode) {
        const bool ocrRequested = overlay.ocrRequested();
        const QRect selGlobal =
          sel.translated(overlay.geometry().x(), overlay.geometry().y());
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

        if (isGnomeWayland) {
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
        printCaptureScreenJson(imagePath, imageSize, ocrRequested ? "ocr" : "area");
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
