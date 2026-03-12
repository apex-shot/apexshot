#include "ScreenCapture.h"

#include "request.h"

#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusInterface>
#include <QDBusMessage>
#include <QDBusReply>
#include <QDir>
#include <QEventLoop>
#include <QFile>
#include <QGuiApplication>
#include <QImage>
#include <QIODevice>
#include <QMap>
#include <QPixmap>
#include <QScreen>
#include <QTimer>
#include <QUuid>
#include <QUrl>
#include <QtMath>

namespace {

QString makeTempPngPath()
{
    const auto token = QUuid::createUuid().toString(QUuid::Id128);
    return QDir(QDir::tempPath())
      .filePath(QStringLiteral("apexshot_cpp_%1.png").arg(token));
}

#if defined(Q_OS_LINUX)
bool isGnomeWaylandSession()
{
    const bool wayland = qEnvironmentVariableIsSet("WAYLAND_DISPLAY");
    const QString desktop = qEnvironmentVariable("XDG_CURRENT_DESKTOP");
    const bool gnomeDesktop = desktop.contains("GNOME", Qt::CaseInsensitive) ||
                              qEnvironmentVariableIsSet("GNOME_SETUP_DISPLAY");
    return wayland && gnomeDesktop;
}

bool extractShellScreenshotPath(const QDBusMessage& message,
                               const QString& requestedPath,
                               QString& outPath,
                               QString& outError)
{
    if (message.type() == QDBusMessage::ErrorMessage) {
        outError = QStringLiteral("GNOME Shell screenshot call failed: %1")
                     .arg(message.errorMessage());
        return false;
    }

    const auto args = message.arguments();
    if (args.size() < 2) {
        outError = QStringLiteral("GNOME Shell screenshot returned an invalid response");
        return false;
    }

    const bool success = args.at(0).toBool();
    const QString filenameUsedRaw = args.at(1).toString();
    if (!success) {
        outError = QStringLiteral("GNOME Shell screenshot returned success=false");
        return false;
    }

    QString filenameUsed = filenameUsedRaw;
    const QUrl maybeUri(filenameUsedRaw);
    if (maybeUri.isValid() && maybeUri.isLocalFile()) {
        filenameUsed = maybeUri.toLocalFile();
    }

    outPath = filenameUsed.isEmpty() ? requestedPath : filenameUsed;
    if (outPath.isEmpty()) {
        outError = QStringLiteral("GNOME Shell screenshot returned an empty output path");
        return false;
    }

    if (!QFile::exists(outPath)) {
        outError = QStringLiteral("GNOME Shell screenshot output file does not exist: %1")
                     .arg(outPath);
        return false;
    }

    return true;
}

bool captureViaGnomeShellFullscreen(QString& outPath,
                                    QSize& outSize,
                                    QString& outError)
{
    QDBusInterface wrapperInterface(QStringLiteral("org.apexshot.Daemon"),
                                  QStringLiteral("/org/apexshot/Daemon"),
                                  QStringLiteral("org.apexshot.Daemon"),
                                  QDBusConnection::sessionBus());

    if (!wrapperInterface.isValid()) {
        outError = QStringLiteral("ApexShot daemon interface unavailable: %1")
                     .arg(wrapperInterface.lastError().message());
        return false;
    }

    const QDBusMessage reply = wrapperInterface.call(QStringLiteral("CaptureFullscreenGnome"));

    if (reply.type() == QDBusMessage::ErrorMessage) {
        outError = QStringLiteral("Daemon screenshot call failed: %1").arg(reply.errorMessage());
        return false;
    }

    const auto args = reply.arguments();
    if (args.isEmpty()) {
        outError = QStringLiteral("Daemon screenshot returned invalid response");
        return false;
    }

    QString outputPath = args.at(0).toString();
    if (outputPath.isEmpty() || !QFile::exists(outputPath)) {
        outError = QStringLiteral("Daemon screenshot output missing: %1").arg(outputPath);
        return false;
    }

    QImage image(outputPath);
    if (image.isNull()) {
        outError = QStringLiteral("Failed to load GNOME Shell screenshot from daemon: %1")
                     .arg(outputPath);
        return false;
    }

    outPath = outputPath;
    outSize = image.size();
    return true;
}

bool captureViaGnomeShellArea(const QRect& logicalSelection,
                              QString& outPath,
                              QSize& outSize,
                              QString& outError)
{
    const QRect selected = logicalSelection.normalized();
    if (selected.width() <= 0 || selected.height() <= 0) {
        outError = QStringLiteral("Selection is empty");
        return false;
    }

    QDBusInterface wrapperInterface(QStringLiteral("org.apexshot.Daemon"),
                                  QStringLiteral("/org/apexshot/Daemon"),
                                  QStringLiteral("org.apexshot.Daemon"),
                                  QDBusConnection::sessionBus());

    if (!wrapperInterface.isValid()) {
        outError = QStringLiteral("ApexShot daemon interface unavailable: %1")
                     .arg(wrapperInterface.lastError().message());
        return false;
    }

    const QDBusMessage reply = wrapperInterface.call(QStringLiteral("CaptureAreaGnome"),
                                                   selected.x(),
                                                   selected.y(),
                                                   selected.width(),
                                                   selected.height());

    if (reply.type() == QDBusMessage::ErrorMessage) {
        outError = QStringLiteral("Daemon screenshot call failed: %1").arg(reply.errorMessage());
        return false;
    }

    const auto args = reply.arguments();
    if (args.isEmpty()) {
        outError = QStringLiteral("Daemon screenshot returned invalid response");
        return false;
    }

    QString outputPath = args.at(0).toString();
    if (outputPath.isEmpty() || !QFile::exists(outputPath)) {
        outError = QStringLiteral("Daemon screenshot output missing: %1").arg(outputPath);
        return false;
    }

    QImage image(outputPath);
    if (image.isNull()) {
        outError = QStringLiteral("Failed to load GNOME Shell area screenshot from daemon: %1")
                     .arg(outputPath);
        return false;
    }

    outPath = outputPath;
    outSize = image.size();
    return true;
}

bool captureViaPortal(QString& outPortalPath, QString& outError, bool interactive)
{
    auto* connectionInterface = QDBusConnection::sessionBus().interface();
    const auto service = QStringLiteral("org.freedesktop.portal.Desktop");

    if (!connectionInterface || !connectionInterface->isServiceRegistered(service)) {
        outError = QStringLiteral(
          "Could not locate `org.freedesktop.portal.Desktop`");
        return false;
    }

    QDBusInterface screenshotInterface(service,
                                       QStringLiteral("/org/freedesktop/portal/desktop"),
                                       QStringLiteral(
                                         "org.freedesktop.portal.Screenshot"));

    if (!screenshotInterface.isValid()) {
        outError = QStringLiteral("Portal Screenshot interface is invalid: %1")
                     .arg(screenshotInterface.lastError().message());
        return false;
    }

    const QString token = QUuid::createUuid().toString(QUuid::Id128);
    QString sender = QDBusConnection::sessionBus().baseService();
    sender.remove(QLatin1Char(':'));
    sender.replace(QLatin1Char('.'), QLatin1Char('_'));

    const auto requestPath = QStringLiteral(
                               "/org/freedesktop/portal/desktop/request/%1/%2")
                               .arg(sender, token);

    auto* request = new OrgFreedesktopPortalRequestInterface(
      service, requestPath, QDBusConnection::sessionBus(), nullptr);

    QEventLoop loop;
    QTimer timeout;
    timeout.setSingleShot(true);
    timeout.setInterval(30000);

    bool finished = false;
    bool success = false;
    QString portalUri;

    QObject::connect(request,
                     &org::freedesktop::portal::Request::Response,
                     &loop,
                     [&](uint status, const QVariantMap& map) {
                         finished = true;
                         if (status == 0) {
                             portalUri = map.value(QStringLiteral("uri")).toString();
                             success = !portalUri.isEmpty();
                             if (!success) {
                                 outError =
                                   QStringLiteral("Portal response missing URI");
                             }
                         } else {
                             outError =
                               QStringLiteral("Portal screenshot rejected: status=%1")
                                 .arg(status);
                         }
                         loop.quit();
                     });

    QObject::connect(&timeout, &QTimer::timeout, &loop, [&]() {
        outError = QStringLiteral("Portal screenshot timed out after 30 seconds");
        loop.quit();
    });

    timeout.start();

    const auto callReply = screenshotInterface.call(
      QStringLiteral("Screenshot"),
      QStringLiteral(""),
      QMap<QString, QVariant>({ { QStringLiteral("handle_token"), token },
                                { QStringLiteral("interactive"), interactive } }));

    if (callReply.type() == QDBusMessage::ErrorMessage) {
        timeout.stop();
        outError = QStringLiteral("Portal Screenshot call failed: %1")
                     .arg(callReply.errorMessage());
        request->deleteLater();
        return false;
    }

    loop.exec();
    timeout.stop();

    request->Close().waitForFinished();
    request->deleteLater();

    if (!finished || !success) {
        if (outError.isEmpty()) {
            outError = QStringLiteral("Portal screenshot failed");
        }
        return false;
    }

    const auto localFilePath = QUrl(portalUri).toLocalFile();
    if (localFilePath.isEmpty()) {
        outError = QStringLiteral("Portal returned non-file URI: %1").arg(portalUri);
        return false;
    }

    if (!QFile::exists(localFilePath)) {
        outError = QStringLiteral("Portal screenshot file does not exist: %1")
                     .arg(localFilePath);
        return false;
    }

    outPortalPath = localFilePath;
    return true;
}
#endif

#if defined(Q_OS_LINUX)
bool captureViaScreenCastPortal(QString& outPath, QSize& outSize, QString& outError);
#endif

bool captureViaQtGrab(QString& outTempPath, QSize& outSize, QString& outError)
{
    QScreen* primary = QGuiApplication::primaryScreen();
    if (!primary) {
        outError = QStringLiteral("No primary screen available for fallback capture");
        return false;
    }

    QRect desktop;
    for (QScreen* screen : QGuiApplication::screens()) {
        desktop = desktop.united(screen->geometry());
    }

    QPixmap pixmap =
      primary->grabWindow(0, desktop.x(), desktop.y(), desktop.width(), desktop.height());
    if (pixmap.isNull()) {
        pixmap = primary->grabWindow(0);
    }

    if (pixmap.isNull()) {
        outError = QStringLiteral("Qt screen grab fallback returned an empty image");
        return false;
    }

    const auto tmpPath = makeTempPngPath();
    if (!pixmap.save(tmpPath, "PNG")) {
        outError = QStringLiteral("Failed to save Qt fallback screenshot to %1")
                     .arg(tmpPath);
        return false;
    }

    outTempPath = tmpPath;
    outSize = pixmap.size();
    return true;
}

bool logicalDesktopBounds(QRect& outBounds)
{
    const auto screens = QGuiApplication::screens();
    if (screens.isEmpty()) {
        return false;
    }

    int minX = screens.first()->geometry().x();
    int minY = screens.first()->geometry().y();
    int maxX = screens.first()->geometry().x() + screens.first()->geometry().width();
    int maxY = screens.first()->geometry().y() + screens.first()->geometry().height();

    for (QScreen* screen : screens) {
        const QRect geo = screen->geometry();
        minX = qMin(minX, geo.x());
        minY = qMin(minY, geo.y());
        maxX = qMax(maxX, geo.x() + geo.width());
        maxY = qMax(maxY, geo.y() + geo.height());
    }

    outBounds = QRect(minX, minY, maxX - minX, maxY - minY);
    return outBounds.width() > 0 && outBounds.height() > 0;
}

bool saveCroppedToTemp(const QImage& fullImage,
                       const QRect& cropRect,
                       QString& outPath,
                       QSize& outSize,
                       QString& outError)
{
    const QRect bounded = cropRect.intersected(
      QRect(0, 0, fullImage.width(), fullImage.height()));
    if (bounded.width() <= 0 || bounded.height() <= 0) {
        outError =
          QStringLiteral("Mapped crop rectangle is outside captured image bounds");
        return false;
    }

    const QImage cropped = fullImage.copy(bounded);
    const auto tmpPath = makeTempPngPath();
    if (!cropped.save(tmpPath, "PNG")) {
        outError = QStringLiteral("Failed to save cropped image to %1").arg(tmpPath);
        return false;
    }

    outPath = tmpPath;
    outSize = cropped.size();
    return true;
}

#if defined(Q_OS_LINUX)
#include <QDBusMetaType>

bool captureViaScreenCastPortal(QString& outPath, QSize& outSize, QString& outError)
{
    auto* connectionInterface = QDBusConnection::sessionBus().interface();
    if (!connectionInterface || !connectionInterface->isServiceRegistered(
                                   "org.freedesktop.portal.Desktop")) {
        outError = QStringLiteral("Could not locate org.freedesktop.portal.Desktop");
        return false;
    }

    QDBusInterface screenCastInterface(
      "org.freedesktop.portal.Desktop",
      "/org/freedesktop/portal/desktop",
      "org.freedesktop.portal.ScreenCast",
      QDBusConnection::sessionBus());

    if (!screenCastInterface.isValid()) {
        outError = QStringLiteral("ScreenCast interface invalid: %1")
                     .arg(screenCastInterface.lastError().message());
        return false;
    }

    const QString token = QUuid::createUuid().toString(QUuid::Id128).remove('-');
    QString sender = QDBusConnection::sessionBus().baseService();
    sender.remove(':');
    sender.replace('.', '_');

    const QString sessionRequestPath =
      QStringLiteral("/org/freedesktop/portal/desktop/request/%1/session_%2").arg(sender, token);

    OrgFreedesktopPortalRequestInterface sessionRequest(
      "org.freedesktop.portal.Desktop",
      sessionRequestPath,
      QDBusConnection::sessionBus(),
      nullptr);

    QEventLoop sessionLoop;
    QString sessionHandle;
    bool sessionSuccess = false;

    QObject::connect(&sessionRequest,
                     &org::freedesktop::portal::Request::Response,
                     &sessionLoop,
                     [&](uint response, const QVariantMap& results) {
                         if (response == 0) {
                             sessionHandle = results.value("session_handle").toString();
                             sessionSuccess = !sessionHandle.isEmpty();
                         }
                         sessionLoop.quit();
                     });

    QTimer sessionTimeout;
    sessionTimeout.setSingleShot(true);
    sessionTimeout.setInterval(30000);
    QObject::connect(&sessionTimeout, &QTimer::timeout, &sessionLoop, [&]() {
        outError = QStringLiteral("ScreenCast CreateSession timed out");
        sessionLoop.quit();
    });
    sessionTimeout.start();

    QVariantMap sessionOptions;
    sessionOptions["persist_mode"] = QStringLiteral("once");

    auto sessionReply =
      screenCastInterface.call(QStringLiteral("CreateSession"), sessionOptions);
    if (sessionReply.type() == QDBusMessage::ErrorMessage) {
        outError =
          QStringLiteral("ScreenCast CreateSession failed: %1").arg(sessionReply.errorMessage());
        sessionRequest.deleteLater();
        return false;
    }

    sessionLoop.exec();
    sessionTimeout.stop();

    if (!sessionSuccess || sessionHandle.isEmpty()) {
        outError = QStringLiteral("ScreenCast session creation failed");
        sessionRequest.deleteLater();
        return false;
    }

    sessionRequest.Close().waitForFinished();
    sessionRequest.deleteLater();

    const QString sourceRequestPath =
      QStringLiteral("/org/freedesktop/portal/desktop/request/%1/source_%2").arg(sender, token);

    OrgFreedesktopPortalRequestInterface sourceRequest(
      "org.freedesktop.portal.Desktop",
      sourceRequestPath,
      QDBusConnection::sessionBus(),
      nullptr);

    QEventLoop sourceLoop;
    bool sourceSuccess = false;

    QObject::connect(&sourceRequest,
                     &org::freedesktop::portal::Request::Response,
                     &sourceLoop,
                     [&](uint response, const QVariantMap& results) {
                         if (response == 0) {
                             sourceSuccess = true;
                         }
                         sourceLoop.quit();
                     });

    QTimer sourceTimeout;
    sourceTimeout.setSingleShot(true);
    sourceTimeout.setInterval(30000);
    QObject::connect(&sourceTimeout, &QTimer::timeout, &sourceLoop, [&]() {
        outError = QStringLiteral("ScreenCast SelectSources timed out");
        sourceLoop.quit();
    });
    sourceTimeout.start();

    QVariantMap sourceOptions;
    sourceOptions["types"] = QVariant::fromValue(QVariantList() << "screen" << "window");
    sourceOptions["multiple"] = false;
    sourceOptions["cursor_mode"] = QStringLiteral("embedded");

    auto sourceReply = screenCastInterface.call(
      QStringLiteral("SelectSources"), QVariant::fromValue(sessionHandle), sourceOptions);
    if (sourceReply.type() == QDBusMessage::ErrorMessage) {
        outError =
          QStringLiteral("ScreenCast SelectSources failed: %1").arg(sourceReply.errorMessage());
        sourceRequest.deleteLater();
        return false;
    }

    sourceLoop.exec();
    sourceTimeout.stop();

    if (!sourceSuccess) {
        outError = QStringLiteral("ScreenCast source selection failed");
        sourceRequest.deleteLater();
        return false;
    }

    sourceRequest.Close().waitForFinished();
    sourceRequest.deleteLater();

    const QString startRequestPath =
      QStringLiteral("/org/freedesktop/portal/desktop/request/%1/start_%2").arg(sender, token);

    OrgFreedesktopPortalRequestInterface startRequest(
      "org.freedesktop.portal.Desktop",
      startRequestPath,
      QDBusConnection::sessionBus(),
      nullptr);

    QEventLoop startLoop;
    QString streamPath;
    bool startSuccess = false;

    QObject::connect(&startRequest,
                     &org::freedesktop::portal::Request::Response,
                     &startLoop,
                     [&](uint response, const QVariantMap& results) {
                         if (response == 0) {
                             streamPath = results.value("stream_path").toString();
                             startSuccess = !streamPath.isEmpty();
                         }
                         startLoop.quit();
                     });

    QTimer startTimeout;
    startTimeout.setSingleShot(true);
    startTimeout.setInterval(30000);
    QObject::connect(&startTimeout, &QTimer::timeout, &startLoop, [&]() {
        outError = QStringLiteral("ScreenCast Start timed out");
        startLoop.quit();
    });
    startTimeout.start();

    QVariantMap startOptions;
    startOptions["cursor_mode"] = QStringLiteral("embedded");

    auto startReply = screenCastInterface.call(
      QStringLiteral("Start"), QVariant::fromValue(sessionHandle), "", startOptions);
    if (startReply.type() == QDBusMessage::ErrorMessage) {
        outError =
          QStringLiteral("ScreenCast Start failed: %1").arg(startReply.errorMessage());
        startRequest.deleteLater();
        return false;
    }

    startLoop.exec();
    startTimeout.stop();

    if (!startSuccess || streamPath.isEmpty()) {
        outError = QStringLiteral("ScreenCast start failed");
        startRequest.deleteLater();
        return false;
    }

    startRequest.Close().waitForFinished();
    startRequest.deleteLater();

    QDBusInterface streamInterface("org.freedesktop.portal.Desktop",
                                    streamPath,
                                    "org.freedesktop.portal.ScreenCastStream",
                                    QDBusConnection::sessionBus());

    if (!streamInterface.isValid()) {
        outError = QStringLiteral("ScreenCast stream interface invalid");
        return false;
    }

    QEventLoop pipeLoop;
    int pipeFd = -1;
    bool pipeSuccess = false;

    auto pipeReply = streamInterface.call(QStringLiteral("OpenPipeWireRemote"), QVariantMap());
    if (pipeReply.type() == QDBusMessage::ErrorMessage) {
        outError =
          QStringLiteral("ScreenCast OpenPipeWireRemote failed: %1").arg(pipeReply.errorMessage());
        return false;
    }

    pipeFd = pipeReply.arguments().at(0).toInt();
    if (pipeFd < 0) {
        outError = QStringLiteral("ScreenCast got invalid PipeWire fd");
        return false;
    }

    QFile pipeFile;
    if (!pipeFile.open(pipeFd, QIODevice::ReadOnly)) {
        outError = QStringLiteral("Could not open PipeWire fd for reading");
        return false;
    }

    QByteArray frameData = pipeFile.readAll();
    pipeFile.close();

    if (frameData.isEmpty()) {
        outError = QStringLiteral("ScreenCast returned empty frame");
        return false;
    }

    QImage image;
    if (!image.loadFromData(frameData)) {
        outError = QStringLiteral("ScreenCast frame is not a valid image");
        return false;
    }

    const auto tmpPath = makeTempPngPath();
    if (!image.save(tmpPath, "PNG")) {
        outError = QStringLiteral("Failed to save screencast frame to %1").arg(tmpPath);
        return false;
    }

    outPath = tmpPath;
    outSize = image.size();
    return true;
}
#endif

} // namespace

namespace ScreenCapture {

bool captureFullscreenToTempPng(QString& outPath, QSize& outSize, QString& outError)
{
#if defined(Q_OS_LINUX)
    QString shellError;
    if (isGnomeWaylandSession()) {
        if (captureViaGnomeShellFullscreen(outPath, outSize, shellError)) {
            return true;
        }
    }

    QString portalPath;
    QString portalError;
    if (captureViaPortal(portalPath, portalError, false)) {
        QImage image(portalPath);
        if (image.isNull()) {
            outError = QStringLiteral("Failed to load portal screenshot file: %1")
                         .arg(portalPath);
            return false;
        }

        const auto tmpPath = makeTempPngPath();
        if (!image.save(tmpPath, "PNG")) {
            outError = QStringLiteral("Failed to save portal screenshot copy to %1")
                         .arg(tmpPath);
            return false;
        }

        QFile::remove(portalPath);
        outPath = tmpPath;
        outSize = image.size();
        return true;
    }

    QString screencastError;
    if (isGnomeWaylandSession()) {
        if (captureViaScreenCastPortal(outPath, outSize, screencastError)) {
            return true;
        }
    }

    QString fallbackError;
    if (captureViaQtGrab(outPath, outSize, fallbackError)) {
        return true;
    }

    if (shellError.isEmpty()) {
        outError =
          QStringLiteral("Portal capture failed (%1); Screencast fallback failed (%2); Qt fallback failed (%3)")
            .arg(portalError, screencastError, fallbackError);
    } else {
        outError =
          QStringLiteral("GNOME Shell capture failed (%1); Portal capture failed (%2); Screencast fallback failed (%3); Qt fallback failed (%4)")
            .arg(shellError, portalError, screencastError, fallbackError);
    }
    return false;
#else
    return captureViaQtGrab(outPath, outSize, outError);
#endif
}

bool captureFullscreenToTempPngViaPortal(QString& outPath,
                                         QSize& outSize,
                                         QString& outError)
{
#if defined(Q_OS_LINUX)
    QString portalPath;
    QString portalError;
    if (!captureViaPortal(portalPath, portalError, false)) {
        outError = QStringLiteral("Portal fullscreen capture failed (%1)").arg(portalError);
        return false;
    }

    QImage image(portalPath);
    if (image.isNull()) {
        outError = QStringLiteral("Failed to load portal screenshot file: %1")
                     .arg(portalPath);
        return false;
    }

    const auto tmpPath = makeTempPngPath();
    if (!image.save(tmpPath, "PNG")) {
        outError = QStringLiteral("Failed to save portal screenshot copy to %1")
                     .arg(tmpPath);
        return false;
    }

    QFile::remove(portalPath);
    outPath = tmpPath;
    outSize = image.size();
    return true;
#else
    return captureFullscreenToTempPng(outPath, outSize, outError);
#endif
}

bool captureAreaToTempPng(const QRect& logicalSelection,
                          QString& outPath,
                          QSize& outSize,
                          QString& outError)
{
    QString shellError;
    if (isGnomeWaylandSession()) {
        if (captureViaGnomeShellArea(logicalSelection, outPath, outSize, shellError)) {
            return true;
        }
    }

    QRect desktopBounds;
    if (!logicalDesktopBounds(desktopBounds)) {
        outError = QStringLiteral("Unable to determine logical desktop bounds");
        return false;
    }

    QRect selected = logicalSelection.normalized().intersected(desktopBounds);
    if (selected.width() <= 0 || selected.height() <= 0) {
        outError = QStringLiteral("Selection is outside desktop bounds");
        return false;
    }

    QString fullPath;
    QSize capturedSize;
    if (!captureFullscreenToTempPng(fullPath, capturedSize, outError)) {
        if (!shellError.isEmpty()) {
            outError = QStringLiteral("GNOME Shell area capture failed (%1); fallback failed (%2)")
                         .arg(shellError, outError);
        }
        return false;
    }
    if (capturedSize.width() <= 0 || capturedSize.height() <= 0) {
        outError = QStringLiteral("Captured fullscreen image has invalid size");
        QFile::remove(fullPath);
        return false;
    }

    QImage fullImage(fullPath);
    QFile::remove(fullPath);
    if (fullImage.isNull()) {
        outError = QStringLiteral("Failed to load captured fullscreen image");
        return false;
    }

    const double scaleX = static_cast<double>(fullImage.width()) /
                          static_cast<double>(desktopBounds.width());
    const double scaleY = static_cast<double>(fullImage.height()) /
                          static_cast<double>(desktopBounds.height());

    int cropX = qRound((selected.x() - desktopBounds.x()) * scaleX);
    int cropY = qRound((selected.y() - desktopBounds.y()) * scaleY);
    int cropW = qMax(1, qRound(selected.width() * scaleX));
    int cropH = qMax(1, qRound(selected.height() * scaleY));

    return saveCroppedToTemp(
      fullImage, QRect(cropX, cropY, cropW, cropH), outPath, outSize, outError);
}

bool captureAreaToTempPngViaPortal(const QRect& logicalSelection,
                                   QString& outPath,
                                   QSize& outSize,
                                   QString& outError)
{
#if defined(Q_OS_LINUX)
    QRect desktopBounds;
    if (!logicalDesktopBounds(desktopBounds)) {
        outError = QStringLiteral("Unable to determine logical desktop bounds");
        return false;
    }

    QRect selected = logicalSelection.normalized().intersected(desktopBounds);
    if (selected.width() <= 0 || selected.height() <= 0) {
        outError = QStringLiteral("Selection is outside desktop bounds");
        return false;
    }

    QString portalPath;
    QString portalError;
    if (!captureViaPortal(portalPath, portalError, false)) {
        outError = QStringLiteral("Portal permission capture failed (%1)").arg(portalError);
        return false;
    }

    QImage fullImage(portalPath);
    QFile::remove(portalPath);
    if (fullImage.isNull()) {
        outError = QStringLiteral("Failed to load portal screenshot file: %1")
                     .arg(portalPath);
        return false;
    }

    const double scaleX = static_cast<double>(fullImage.width()) /
                          static_cast<double>(desktopBounds.width());
    const double scaleY = static_cast<double>(fullImage.height()) /
                          static_cast<double>(desktopBounds.height());

    int cropX = qRound((selected.x() - desktopBounds.x()) * scaleX);
    int cropY = qRound((selected.y() - desktopBounds.y()) * scaleY);
    int cropW = qMax(1, qRound(selected.width() * scaleX));
    int cropH = qMax(1, qRound(selected.height() * scaleY));

    return saveCroppedToTemp(
      fullImage, QRect(cropX, cropY, cropW, cropH), outPath, outSize, outError);
#else
    return captureAreaToTempPng(logicalSelection, outPath, outSize, outError);
#endif
}

bool captureAreaToTempPngFromOverlayLocal(const QRect& localSelection,
                                          const QRect& overlayGeometry,
                                          QString& outPath,
                                          QSize& outSize,
                                          QString& outError)
{
    QRect selected = localSelection.normalized();
    if (selected.width() <= 0 || selected.height() <= 0) {
        outError = QStringLiteral("Selection is empty");
        return false;
    }

    if (overlayGeometry.width() <= 0 || overlayGeometry.height() <= 0) {
        outError = QStringLiteral("Overlay geometry is invalid");
        return false;
    }

    QRect desktopBounds;
    if (!logicalDesktopBounds(desktopBounds)) {
        outError = QStringLiteral("Unable to determine logical desktop bounds");
        return false;
    }

    QString fullPath;
    QSize capturedSize;
    if (!captureFullscreenToTempPng(fullPath, capturedSize, outError)) {
        return false;
    }
    if (capturedSize.width() <= 0 || capturedSize.height() <= 0) {
        outError = QStringLiteral("Captured fullscreen image has invalid size");
        QFile::remove(fullPath);
        return false;
    }

    QImage fullImage(fullPath);
    QFile::remove(fullPath);
    if (fullImage.isNull()) {
        outError = QStringLiteral("Failed to load captured fullscreen image");
        return false;
    }

    const QRect selectedGlobal = selected.translated(overlayGeometry.topLeft());
    const QRect bounded = selectedGlobal.intersected(overlayGeometry).intersected(desktopBounds);
    if (bounded.width() <= 0 || bounded.height() <= 0) {
        outError = QStringLiteral("Selection is outside desktop bounds");
        return false;
    }

    const double scaleX = static_cast<double>(fullImage.width()) /
                          static_cast<double>(desktopBounds.width());
    const double scaleY = static_cast<double>(fullImage.height()) /
                          static_cast<double>(desktopBounds.height());

    const int cropX = qRound((bounded.x() - desktopBounds.x()) * scaleX);
    const int cropY = qRound((bounded.y() - desktopBounds.y()) * scaleY);
    const int cropW = qMax(1, qRound(bounded.width() * scaleX));
    const int cropH = qMax(1, qRound(bounded.height() * scaleY));

    return saveCroppedToTemp(
      fullImage, QRect(cropX, cropY, cropW, cropH), outPath, outSize, outError);
}

} // namespace ScreenCapture
