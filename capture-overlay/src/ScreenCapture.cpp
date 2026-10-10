#include "ScreenCapture.h"

#include "Sandbox.h"

#include <QCoreApplication>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QImageWriter>
#include <QJsonArray>
#include <QJsonParseError>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonValue>
#include <QProcess>
#include <QScreen>
#include <QStandardPaths>
#include <QStringList>
#include <QTemporaryFile>
#include <QtMath>

namespace {

constexpr int kStartTimeoutMs = 5000;
constexpr int kReadyTimeoutMs = 180000;
constexpr int kCaptureTimeoutMs = 30000;
constexpr int kShutdownTimeoutMs = 2000;
constexpr double kAspectTolerance = 0.01;

bool isWaylandSession()
{
    const QString sessionType = qEnvironmentVariable("XDG_SESSION_TYPE");
    if (sessionType.compare(QStringLiteral("wayland"), Qt::CaseInsensitive) == 0) {
        return true;
    }
    if (sessionType.compare(QStringLiteral("x11"), Qt::CaseInsensitive) == 0) {
        return false;
    }
    if (!qEnvironmentVariable("WAYLAND_DISPLAY").isEmpty()) {
        return true;
    }
    return QGuiApplication::platformName().contains(QStringLiteral("wayland"),
                                                    Qt::CaseInsensitive);
}

QString screenGeometryToken(const QScreen* screen)
{
    const QRect geometry = screen->geometry();
    return QStringLiteral("%1@%2,%3,%4x%5")
      .arg(screen->name())
      .arg(geometry.x())
      .arg(geometry.y())
      .arg(geometry.width())
      .arg(geometry.height());
}

QString sessionKey(const QScreen* chosen)
{
    QStringList topology;
    for (const QScreen* screen : QGuiApplication::screens()) {
        topology << screenGeometryToken(screen);
    }
    return screenGeometryToken(chosen) + QLatin1Char('|')
           + topology.join(QLatin1Char(';'));
}

QString portalHelperPath(QString& outError)
{
    const QString overridden = qEnvironmentVariable("APEXSHOT_PORTAL_HELPER");
    if (!overridden.isEmpty()) {
        if (QFileInfo(overridden).isExecutable()) {
            return overridden;
        }
        outError = QStringLiteral("APEXSHOT_PORTAL_HELPER is not executable: %1")
                     .arg(overridden);
        return QString();
    }

    const QString sibling = QDir(QCoreApplication::applicationDirPath())
                              .filePath(QStringLiteral("apexshot"));
    if (QFileInfo(sibling).isExecutable()) {
        return sibling;
    }

    const QString onPath = QStandardPaths::findExecutable(QStringLiteral("apexshot"));
    if (!onPath.isEmpty()) {
        return onPath;
    }

    outError = QStringLiteral("apexshot portal helper not found; set APEXSHOT_PORTAL_HELPER");
    return QString();
}

bool readHelperLine(QProcess& helper, int timeoutMs, QByteArray& outLine)
{
    QElapsedTimer timer;
    timer.start();
    while (!helper.canReadLine()) {
        if (helper.state() == QProcess::NotRunning) {
            return false;
        }
        const qint64 remaining = timeoutMs - timer.elapsed();
        if (remaining <= 0) {
            return false;
        }
        helper.waitForReadyRead(static_cast<int>(remaining));
    }
    outLine = helper.readLine();
    return true;
}

bool readIntPair(const QJsonValue& value, int& outFirst, int& outSecond)
{
    if (!value.isArray()) {
        return false;
    }
    const QJsonArray pair = value.toArray();
    if (pair.size() != 2 || !pair.at(0).isDouble() || !pair.at(1).isDouble()) {
        return false;
    }
    outFirst = pair.at(0).toInt();
    outSecond = pair.at(1).toInt();
    return true;
}

}

namespace ScreenCapture {

namespace {

CaptureStatus statusForFailure(const HelperMessage& message)
{
    return message.cancelled ? CaptureStatus::Cancelled : CaptureStatus::Failed;
}

}

HelperMessage parseHelperMessage(const QByteArray& line)
{
    HelperMessage message;
    QJsonParseError parseError;
    const QJsonDocument document = QJsonDocument::fromJson(line.trimmed(), &parseError);
    if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
        return message;
    }

    const QJsonObject object = document.object();
    if (object.contains(QStringLiteral("error"))) {
        message.kind = HelperMessage::Kind::Error;
        message.error = object.value(QStringLiteral("error")).toString();
        message.cancelled = object.value(QStringLiteral("cancelled")).toBool();
        return message;
    }

    if (object.value(QStringLiteral("ready")).toBool()) {
        message.kind = HelperMessage::Kind::Ready;
        int x = 0;
        int y = 0;
        if (readIntPair(object.value(QStringLiteral("position")), x, y)) {
            message.hasPosition = true;
            message.position = QPoint(x, y);
        }
        int width = 0;
        int height = 0;
        if (readIntPair(object.value(QStringLiteral("size")), width, height)) {
            message.hasSize = true;
            message.size = QSize(width, height);
        }
        return message;
    }

    if (object.contains(QStringLiteral("path"))) {
        message.kind = HelperMessage::Kind::Frame;
        message.path = object.value(QStringLiteral("path")).toString();
    }
    return message;
}

bool readyMetadataMatches(const HelperMessage& ready, const QRect& captureRect)
{
    if (captureRect.isEmpty()) {
        return false;
    }
    if (ready.hasPosition && ready.position != captureRect.topLeft()) {
        return false;
    }
    if (ready.hasSize && !ready.size.isEmpty()) {
        const double streamRatio = static_cast<double>(ready.size.width())
                                   / ready.size.height();
        const double rectRatio = static_cast<double>(captureRect.width())
                                 / captureRect.height();
        return qAbs(streamRatio - rectRatio) <= kAspectTolerance * rectRatio;
    }
    return true;
}

QRect mapLogicalRectToImage(const QRect& logicalRect,
                            const QRect& captureRect,
                            const QSize& imageSize)
{
    const QRect selected = logicalRect.normalized().intersected(captureRect);
    if (selected.isEmpty() || captureRect.isEmpty() || imageSize.isEmpty()) {
        return QRect();
    }

    const double scaleX = static_cast<double>(imageSize.width()) / captureRect.width();
    const double scaleY = static_cast<double>(imageSize.height()) / captureRect.height();
    const int left = qRound((selected.left() - captureRect.left()) * scaleX);
    const int top = qRound((selected.top() - captureRect.top()) * scaleY);
    const int right = qRound((selected.right() + 1 - captureRect.left()) * scaleX);
    const int bottom = qRound((selected.bottom() + 1 - captureRect.top()) * scaleY);

    return QRect(left, top, qMax(1, right - left), qMax(1, bottom - top))
      .intersected(QRect(QPoint(0, 0), imageSize));
}

CaptureSession::CaptureSession() = default;

CaptureSession::~CaptureSession()
{
    close();
}

CaptureStatus CaptureSession::prepare(QScreen* screen, bool includeCursor, QString& outError)
{
    close();
    if (!screen) {
        outError = QStringLiteral("No screen selected for capture");
        return CaptureStatus::Failed;
    }

    m_screen = screen;
    m_captureRect = screen->geometry();

    if (!isWaylandSession() && !isFlatpakSandboxed()) {
        m_prepared = true;
        return CaptureStatus::Ok;
    }

    const QString program = portalHelperPath(outError);
    if (program.isEmpty()) {
        return CaptureStatus::Failed;
    }

    const QStringList arguments = {
        QStringLiteral("portal-still-internal"),
        sessionKey(screen),
        QString::number(m_captureRect.x()),
        QString::number(m_captureRect.y()),
        QString::number(m_captureRect.width()),
        QString::number(m_captureRect.height()),
        includeCursor ? QStringLiteral("1") : QStringLiteral("0"),
    };

    m_helper = std::make_unique<QProcess>();
    m_helper->setProcessChannelMode(QProcess::ForwardedErrorChannel);
    m_helper->start(program, arguments);
    if (!m_helper->waitForStarted(kStartTimeoutMs)) {
        outError = QStringLiteral("Failed to start portal helper %1: %2")
                     .arg(program, m_helper->errorString());
        close();
        return CaptureStatus::Failed;
    }

    QByteArray line;
    if (!readHelperLine(*m_helper, kReadyTimeoutMs, line)) {
        outError = QStringLiteral("Portal helper did not report ready");
        close();
        return CaptureStatus::Failed;
    }

    const HelperMessage message = parseHelperMessage(line);
    if (message.kind == HelperMessage::Kind::Error) {
        outError = message.error;
        const CaptureStatus status = statusForFailure(message);
        close();
        return status;
    }
    if (message.kind != HelperMessage::Kind::Ready) {
        outError = QStringLiteral("Portal helper sent an unexpected ready reply");
        close();
        return CaptureStatus::Failed;
    }
    if (!readyMetadataMatches(message, m_captureRect)) {
        outError = QStringLiteral("Portal stream does not match the selected monitor");
        close();
        return CaptureStatus::Failed;
    }

    m_prepared = true;
    return CaptureStatus::Ok;
}

CaptureStatus CaptureSession::captureFresh(QImage& outImage, QString& outError)
{
    if (!m_prepared) {
        outError = QStringLiteral("Capture session is not prepared");
        return CaptureStatus::Failed;
    }

    if (!m_helper) {
        if (m_screen.isNull()) {
            outError = QStringLiteral("The selected display is no longer available");
            return CaptureStatus::Failed;
        }
        const QPixmap pixmap = m_screen->grabWindow(0);
        if (pixmap.isNull()) {
            outError = QStringLiteral("Qt screen grab returned an empty image");
            return CaptureStatus::Failed;
        }
        outImage = pixmap.toImage();
        return CaptureStatus::Ok;
    }

    m_helper->write("capture\n");
    if (!m_helper->waitForBytesWritten(kCaptureTimeoutMs)) {
        outError = QStringLiteral("Portal helper did not accept capture request");
        close();
        return CaptureStatus::Failed;
    }

    QByteArray line;
    if (!readHelperLine(*m_helper, kCaptureTimeoutMs, line)) {
        outError = QStringLiteral("Portal helper did not return a frame");
        close();
        return CaptureStatus::Failed;
    }

    const HelperMessage message = parseHelperMessage(line);
    if (message.kind == HelperMessage::Kind::Error) {
        outError = message.error;
        const CaptureStatus status = statusForFailure(message);
        close();
        return status;
    }
    if (message.kind != HelperMessage::Kind::Frame || message.path.isEmpty()) {
        outError = QStringLiteral("Portal helper sent an unexpected frame reply");
        close();
        return CaptureStatus::Failed;
    }

    const QImage frame(message.path);
    QFile::remove(message.path);
    if (frame.isNull()) {
        outError = QStringLiteral("Failed to load portal frame %1").arg(message.path);
        return CaptureStatus::Failed;
    }
    outImage = frame;
    return CaptureStatus::Ok;
}

bool CaptureSession::isPrepared() const
{
    return m_prepared;
}

QRect CaptureSession::captureRect() const
{
    return m_captureRect;
}

void CaptureSession::close()
{
    m_prepared = false;
    if (!m_helper) {
        return;
    }
    m_helper->closeWriteChannel();
    if (!m_helper->waitForFinished(kShutdownTimeoutMs)) {
        m_helper->kill();
        m_helper->waitForFinished(kShutdownTimeoutMs);
    }
    m_helper.reset();
}

bool saveImageToTempPng(const QImage& image,
                        QString& outPath,
                        QSize& outSize,
                        QString& outError)
{
    if (image.isNull()) {
        outError = QStringLiteral("Captured image is empty");
        return false;
    }
    QTemporaryFile file(QDir::temp().filePath(QStringLiteral("apexshot_cpp_XXXXXX.png")));
    if (!file.open()) {
        outError = QStringLiteral("Failed to create screenshot temporary file: %1")
                     .arg(file.errorString());
        return false;
    }
    QImageWriter writer(&file, "PNG");
    writer.setCompression(1);
    if (!writer.write(image)) {
        outError = QStringLiteral("Failed to save screenshot: %1").arg(writer.errorString());
        return false;
    }
    file.setAutoRemove(false);
    outPath = file.fileName();
    outSize = image.size();
    return true;
}

bool cropLogicalSelectionToTempPng(const QImage& captureImage,
                                   const QRect& captureRect,
                                   const QRect& logicalSelection,
                                   QString& outPath,
                                   QSize& outSize,
                                   QString& outError)
{
    const QRect crop = mapLogicalRectToImage(logicalSelection, captureRect, captureImage.size());
    if (crop.isEmpty()) {
        outError = QStringLiteral("Selection is outside the captured display");
        return false;
    }
    return saveImageToTempPng(captureImage.copy(crop), outPath, outSize, outError);
}

QPixmap freezeBackgroundForLogicalRect(const QImage& captureImage,
                                       const QRect& captureRect,
                                       const QRect& logicalRect)
{
    const QRect selected = logicalRect.normalized().intersected(captureRect);
    if (captureImage.isNull() || selected.isEmpty()) {
        return QPixmap();
    }

    QImage cropped = captureImage.copy(
      mapLogicalRectToImage(selected, captureRect, captureImage.size()));
    if (cropped.isNull()) {
        return QPixmap();
    }
    if (cropped.size() != selected.size()) {
        cropped = cropped.scaled(selected.size(),
                                 Qt::IgnoreAspectRatio,
                                 Qt::SmoothTransformation);
    }
    return QPixmap::fromImage(cropped);
}

}
