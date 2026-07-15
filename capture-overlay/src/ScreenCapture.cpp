#include "ScreenCapture.h"

#include "request.h"

#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusInterface>
#include <QDBusMessage>
#include <QDBusReply>
#include <QDir>
#include <QElapsedTimer>
#include <QEventLoop>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QImage>
#include <QImageReader>
#include <QImageWriter>
#include <QIODevice>
#include <QMap>
#include <QPixmap>
#include <QScreen>
#include <QStandardPaths>
#include <QThread>
#include <QTimer>
#include <QUuid>
#include <QUrl>
#include <QtMath>

#include <cstdio>

namespace {

QString makeTempPngPath()
{
    const auto token = QUuid::createUuid().toString(QUuid::Id128);
    return QDir(QDir::tempPath())
      .filePath(QStringLiteral("apexshot_cpp_%1.png").arg(token));
}

/// Fast PNG write for temp IPC. Default zlib level is slow on multi-monitor
/// freezes; level 1 keeps lossless quality with much lower encode cost.
bool savePngFast(const QImage& image, const QString& path, QString& outError)
{
    QImageWriter writer(path, "PNG");
    writer.setCompression(1);
    if (!writer.write(image)) {
        // Fallback to QImage::save if the writer rejects the image.
        if (!image.save(path, "PNG")) {
            outError = QStringLiteral("Failed to save PNG to %1: %2")
                         .arg(path, writer.errorString());
            return false;
        }
    }
    return true;
}

bool grabDesktopPixmap(QPixmap& outPixmap, QString& outError)
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

    outPixmap =
      primary->grabWindow(0, desktop.x(), desktop.y(), desktop.width(), desktop.height());
    if (outPixmap.isNull()) {
        outPixmap = primary->grabWindow(0);
    }

    if (outPixmap.isNull()) {
        outError = QStringLiteral("Qt screen grab fallback returned an empty image");
        return false;
    }
    return true;
}

#if defined(Q_OS_LINUX)
constexpr unsigned long kPortalDialogDismissalDelayMs = 650;

// Large multi-monitor freezes can exceed Qt's default 128MB image allocation
// cap. allocationLimit APIs exist only on Qt 6+; Qt 5 has no hard limit here.
struct PortalImageAllocationGuard {
    PortalImageAllocationGuard()
    {
#if QT_VERSION >= QT_VERSION_CHECK(6, 0, 0)
        previous = QImageReader::allocationLimit();
        if (previous > 0 && previous < 1024) {
            QImageReader::setAllocationLimit(1024);
        }
#endif
    }
    ~PortalImageAllocationGuard()
    {
#if QT_VERSION >= QT_VERSION_CHECK(6, 0, 0)
        QImageReader::setAllocationLimit(previous);
#endif
    }
#if QT_VERSION >= QT_VERSION_CHECK(6, 0, 0)
    int previous = 0;
#endif
};

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

/// Flameshot-style: portal URI → QImage in memory. No re-encode to a second PNG.
bool captureViaPortalToImage(QImage& outImage, QString& outError, bool interactive)
{
    QString portalPath;
    if (!captureViaPortal(portalPath, outError, interactive)) {
        return false;
    }

    PortalImageAllocationGuard allocationGuard;
    Q_UNUSED(allocationGuard);

    outImage = QImage(portalPath);
    QFile::remove(portalPath);
    if (outImage.isNull()) {
        outError = QStringLiteral("Failed to load portal screenshot file: %1")
                     .arg(portalPath);
        return false;
    }
    return true;
}
#endif

bool captureViaQtGrabToImage(QImage& outImage, QString& outError)
{
    QPixmap pixmap;
    if (!grabDesktopPixmap(pixmap, outError)) {
        return false;
    }
    outImage = pixmap.toImage();
    if (outImage.isNull()) {
        outError = QStringLiteral("Qt screen grab produced an empty image");
        return false;
    }
    return true;
}

bool captureViaQtGrab(QString& outTempPath, QSize& outSize, QString& outError)
{
    QImage image;
    if (!captureViaQtGrabToImage(image, outError)) {
        return false;
    }

    const auto tmpPath = makeTempPngPath();
    if (!savePngFast(image, tmpPath, outError)) {
        return false;
    }

    outTempPath = tmpPath;
    outSize = image.size();
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

bool logicalAvailableDesktopBounds(QRect& outBounds)
{
    const auto screens = QGuiApplication::screens();
    if (screens.isEmpty()) {
        return false;
    }

    outBounds = screens.first()->availableGeometry();
    for (QScreen* screen : screens) {
        outBounds = outBounds.united(screen->availableGeometry());
    }
    return outBounds.width() > 0 && outBounds.height() > 0;
}

QPoint overlayLocalOriginForDesktopCapture(const QRect& overlayGeometry,
                                           const QRect& desktopBounds)
{
    QRect availableBounds;
    if (!logicalAvailableDesktopBounds(availableBounds)) {
        return overlayGeometry.topLeft();
    }

    QPoint origin = overlayGeometry.topLeft();
    constexpr int tolerance = 2;

    const bool widthMatchesAvailable =
        availableBounds.width() > 0 &&
        qAbs(overlayGeometry.width() - availableBounds.width()) <= tolerance &&
        availableBounds.width() < desktopBounds.width();
    if (widthMatchesAvailable) {
        origin.setX(availableBounds.x());
    }

    return origin;
}

QRect mapLogicalSelectionToCapturedImage(const QRect& logicalSelection,
                                         const QImage& fullImage,
                                         const QRect& desktopBounds)
{
    const QRect selected = logicalSelection.normalized().intersected(desktopBounds);

    // Compute effective scale from actual captured image dimensions rather than
    // relying on QScreen::devicePixelRatio(), which can be inaccurate with
    // fractional display scaling (e.g. 133%) where the compositor renders at a
    // scaled framebuffer that already matches the logical desktop size.
    // The compositor's screenshot / screencast always produces a uniformly-scaled
    // image of the whole logical desktop, so a simple ratio is correct for all
    // monitor layouts — single, multi, and mixed-DPI.
    const double scaleX = static_cast<double>(fullImage.width()) /
                          static_cast<double>(desktopBounds.width());
    const double scaleY = static_cast<double>(fullImage.height()) /
                          static_cast<double>(desktopBounds.height());
    return QRect(qRound((selected.x() - desktopBounds.x()) * scaleX),
                 qRound((selected.y() - desktopBounds.y()) * scaleY),
                 qMax(1, qRound(selected.width() * scaleX)),
                 qMax(1, qRound(selected.height() * scaleY)));
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
    if (!savePngFast(cropped, tmpPath, outError)) {
        return false;
    }

    outPath = tmpPath;
    outSize = cropped.size();
    return true;
}

#if defined(Q_OS_LINUX)
#include <QDBusMetaType>

namespace {

// Path used to persist the ScreenCast portal `restore_token` between
// capture invocations *and* across reboots. Mirrors the location used by
// the Rust capture path (~/.cache/apexshot/...) but uses a distinct file
// name so the two flows don't accidentally share / clobber each other's
// token grants. See `src/backend/wayland.rs::restore_token_path`.
QString screenCastRestoreTokenPath()
{
    QString cacheDir =
        QStandardPaths::writableLocation(QStandardPaths::CacheLocation);
    if (cacheDir.isEmpty()) {
        cacheDir = QDir::homePath() + QLatin1String("/.cache");
    }
    // QStandardPaths::CacheLocation already includes the application name
    // when QCoreApplication::applicationName() is set, but we don't rely
    // on that here — pin it to the same `apexshot/` directory the Rust
    // side uses so both restore-token caches live under one folder.
    QDir dir(cacheDir);
    if (dir.dirName() != QLatin1String("apexshot")) {
        dir = QDir(cacheDir + QLatin1String("/apexshot"));
    }
    return dir.filePath(QStringLiteral("cpp-screencast.token"));
}

QString loadScreenCastRestoreToken()
{
    QFile file(screenCastRestoreTokenPath());
    if (!file.exists() || !file.open(QIODevice::ReadOnly | QIODevice::Text)) {
        return QString();
    }
    const QString token = QString::fromUtf8(file.readAll()).trimmed();
    file.close();
    return token;
}

void saveScreenCastRestoreToken(const QString& token)
{
    if (token.trimmed().isEmpty()) {
        return;
    }
    const QString path = screenCastRestoreTokenPath();
    QDir().mkpath(QFileInfo(path).absolutePath());
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate | QIODevice::Text)) {
        return;
    }
    file.write(token.toUtf8());
    file.close();
}

void clearScreenCastRestoreToken()
{
    QFile::remove(screenCastRestoreTokenPath());
}

void waitForPortalDialogDismissal()
{
    QThread::msleep(kPortalDialogDismissalDelayMs);
}

} // namespace

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

    // NOTE: per the xdg-desktop-portal ScreenCast spec, `persist_mode` and
    // `restore_token` belong on `SelectSources`, NOT on `CreateSession`.
    // The previous code placed a malformed `"once"` string here, which the
    // portal silently ignored — that's why the user was re-prompted on
    // every reboot. Leave CreateSession's options empty (apart from the
    // mandatory request handle, which Qt/libdbus fills in automatically).
    QVariantMap sessionOptions;

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
    // ScreenCast spec v4+ (GNOME 43, KDE Plasma 5.27+): ask the portal to
    // persist the access grant until the user explicitly revokes it. The
    // value MUST be a uint32 — passing a string here is what caused the
    // permission to silently fall back to "do not persist" before.
    sourceOptions["persist_mode"] = QVariant::fromValue<quint32>(2);
    // If we have a token from a prior accepted session, hand it back so
    // the portal skips the "Allow…?" dialog entirely. The token is opaque;
    // we never inspect it.
    const QString restoreToken = loadScreenCastRestoreToken();
    const bool usedRestoreToken = !restoreToken.isEmpty();
    if (!restoreToken.isEmpty()) {
        sourceOptions["restore_token"] = restoreToken;
    }

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
    QString returnedRestoreToken;
    bool startSuccess = false;

    QObject::connect(&startRequest,
                     &org::freedesktop::portal::Request::Response,
                     &startLoop,
                     [&](uint response, const QVariantMap& results) {
                         if (response == 0) {
                             streamPath = results.value("stream_path").toString();
                             startSuccess = !streamPath.isEmpty();
                             // The portal includes a `restore_token` here
                             // when persist_mode != 0 and the user
                             // approved the request. Cache it so future
                             // captures (including across reboots) skip
                             // the dialog. If the user denied the
                             // request, this key is simply absent and we
                             // do nothing.
                             returnedRestoreToken =
                                 results.value(QStringLiteral("restore_token")).toString();
                         } else {
                             // Response code 1 == user cancelled, 2 ==
                             // failed. In either case the previously
                             // cached token is no longer valid (revoked
                             // or rejected), so drop it to avoid getting
                             // stuck repeatedly retrying with a stale
                             // token.
                             clearScreenCastRestoreToken();
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

    // Save the freshly-issued token now (before we touch PipeWire) — even
    // if the actual frame grab below fails for some unrelated reason, the
    // grant itself is already valid and we want the next run to reuse it.
    if (!returnedRestoreToken.isEmpty()) {
        saveScreenCastRestoreToken(returnedRestoreToken);
    }

    startRequest.Close().waitForFinished();
    startRequest.deleteLater();

    if (!usedRestoreToken) {
        // GNOME can keep the portal "Share screen" dialog composited briefly
        // after Start succeeds. Wait before opening PipeWire so the first
        // captured frame does not include the dismissed dialog.
        waitForPortalDialogDismissal();
    }

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
    return captureFullscreenToTempPngViaPortal(outPath, outSize, outError);
#else
    return captureViaQtGrab(outPath, outSize, outError);
#endif
}

bool captureFullscreenToImage(QImage& outImage, QString& outError)
{
    QElapsedTimer timer;
    timer.start();

#if defined(Q_OS_LINUX)
    // Preferred still path: Screenshot portal → QImage (no intermediate PNG write).
    QString portalError;
    if (captureViaPortalToImage(outImage, portalError, false)) {
        fprintf(stderr,
                "apexshot-capture: freeze via Screenshot portal in %lldms (%dx%d)\n",
                static_cast<long long>(timer.elapsed()),
                outImage.width(),
                outImage.height());
        return true;
    }
    fprintf(stderr,
            "apexshot-capture: Screenshot portal freeze failed (%s); trying Qt grab\n",
            portalError.toLocal8Bit().constData());
#endif

    if (!captureViaQtGrabToImage(outImage, outError)) {
        return false;
    }
    fprintf(stderr,
            "apexshot-capture: freeze via Qt grab in %lldms (%dx%d)\n",
            static_cast<long long>(timer.elapsed()),
            outImage.width(),
            outImage.height());
    return true;
}

QPixmap freezeBackgroundForLogicalRect(const QImage& fullDesktopImage,
                                       const QRect& logicalRect)
{
    if (fullDesktopImage.isNull()) {
        return QPixmap();
    }

    QRect desktopBounds;
    if (!logicalDesktopBounds(desktopBounds)) {
        return QPixmap();
    }

    const QRect selected = logicalRect.normalized().intersected(desktopBounds);
    if (selected.width() <= 0 || selected.height() <= 0) {
        return QPixmap();
    }

    const QRect crop =
      mapLogicalSelectionToCapturedImage(selected, fullDesktopImage, desktopBounds);
    QImage cropped = fullDesktopImage.copy(
      crop.intersected(QRect(0, 0, fullDesktopImage.width(), fullDesktopImage.height())));
    if (cropped.isNull()) {
        return QPixmap();
    }

    // CaptureOverlay paints in logical widget coords; match that space 1:1.
    const QSize logicalSize = selected.size();
    if (cropped.size() != logicalSize && logicalSize.isValid()) {
        cropped = cropped.scaled(logicalSize,
                                 Qt::IgnoreAspectRatio,
                                 Qt::SmoothTransformation);
    }
    return QPixmap::fromImage(cropped);
}

bool cropFromDesktopImageToTempPng(const QImage& fullDesktopImage,
                                   const QRect& logicalSelection,
                                   QString& outPath,
                                   QSize& outSize,
                                   QString& outError)
{
    if (fullDesktopImage.isNull()) {
        outError = QStringLiteral("Freeze image is empty");
        return false;
    }

    QRect desktopBounds;
    if (!logicalDesktopBounds(desktopBounds)) {
        outError = QStringLiteral("Unable to determine logical desktop bounds");
        return false;
    }

    const QRect selected = logicalSelection.normalized().intersected(desktopBounds);
    if (selected.width() <= 0 || selected.height() <= 0) {
        outError = QStringLiteral("Selection is outside desktop bounds");
        return false;
    }

    return saveCroppedToTemp(
      fullDesktopImage,
      mapLogicalSelectionToCapturedImage(selected, fullDesktopImage, desktopBounds),
      outPath,
      outSize,
      outError);
}

bool claimPortalFileToTemp(const QString& portalPath,
                           QString& outPath,
                           QSize& outSize,
                           QString& outError)
{
    PortalImageAllocationGuard allocationGuard;
    Q_UNUSED(allocationGuard);

    QImageReader reader(portalPath);
    QSize size = reader.size();
    if (!size.isValid()) {
        const QImage image(portalPath);
        if (image.isNull()) {
            outError = QStringLiteral("Failed to load portal screenshot file: %1")
                         .arg(portalPath);
            QFile::remove(portalPath);
            return false;
        }
        size = image.size();
    }

    // Own a temp path under /tmp so Rust can safely delete after load.
    // Prefer rename (same FS, free) then raw file copy — never decode+re-encode.
    const QString tmpPath = makeTempPngPath();
    if (QFile::rename(portalPath, tmpPath)) {
        outPath = tmpPath;
        outSize = size;
        return true;
    }

    if (QFile::copy(portalPath, tmpPath)) {
        QFile::remove(portalPath);
        outPath = tmpPath;
        outSize = size;
        return true;
    }

    // Last resort: return portal path as-is (caller must delete).
    fprintf(stderr,
            "apexshot-capture: could not claim portal file into temp; using portal path\n");
    outPath = portalPath;
    outSize = size;
    return true;
}

bool captureFullscreenToTempPngViaPortal(QString& outPath,
                                         QSize& outSize,
                                         QString& outError)
{
#if defined(Q_OS_LINUX)
    QString portalPath;
    QString portalError;
    if (!captureViaPortal(portalPath, portalError, false)) {
        // Fall back to in-process grab rather than failing hard on portal glitches.
        fprintf(stderr,
                "apexshot-capture: Screenshot portal failed (%s); Qt grab fallback\n",
                portalError.toLocal8Bit().constData());
        return captureViaQtGrab(outPath, outSize, outError);
    }

    return claimPortalFileToTemp(portalPath, outPath, outSize, outError);
#else
    return captureViaQtGrab(outPath, outSize, outError);
#endif
}

bool captureAreaToTempPng(const QRect& logicalSelection,
                          QString& outPath,
                          QSize& outSize,
                          QString& outError)
{
    return captureAreaToTempPngViaPortal(logicalSelection, outPath, outSize, outError);
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

    return saveCroppedToTemp(
      fullImage,
      mapLogicalSelectionToCapturedImage(selected, fullImage, desktopBounds),
      outPath,
      outSize,
      outError);
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

    QImage fullImage;
    if (!captureFullscreenToImage(fullImage, outError)) {
        return false;
    }
    if (fullImage.width() <= 0 || fullImage.height() <= 0) {
        outError = QStringLiteral("Captured fullscreen image has invalid size");
        return false;
    }

    const QPoint overlayOrigin =
      overlayLocalOriginForDesktopCapture(overlayGeometry, desktopBounds);
    const QRect selectedGlobal = selected.translated(overlayOrigin);
    const QRect bounded = selectedGlobal.intersected(desktopBounds);
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
