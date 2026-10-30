#include "platform/windows/webcam_preview.h"

#include "platform/recording.h"

#include <QBuffer>
#include <QHostAddress>
#include <QImageReader>
#include <QProcess>
#include <QRegularExpression>
#include <QTcpServer>
#include <QTcpSocket>
#include <QTimer>
#include <QUuid>
#include <QVector>

#include <algorithm>

namespace Platform {

WebcamCameraFormat webcamPreferredFormat(const QString& options, int fps, qint64 maxArea)
{
    const QRegularExpression option(QStringLiteral(
        "min s=(\\d+)x(\\d+) fps=([\\d.]+) max s=(\\d+)x(\\d+) fps=([\\d.]+)"));
    const qint64 mjpegCeiling = qMin(maxArea, qint64(1920) * 1080);
    // Uncompressed frames past 720p need more USB bandwidth than an integrated
    // camera has: asking for one trades the frame rate for pixels, and the
    // staggered delivery is what a recording then carries. MJPEG modes are
    // compressed on the camera and hold their rate at larger sizes.
    const qint64 rawCeiling = qMin(maxArea, qint64(1280) * 720);
    QVector<WebcamCameraFormat> sustained;
    QVector<WebcamCameraFormat> candidates;
    for (const QString& line : options.split(QLatin1Char('\n'))) {
        const auto match = option.match(line);
        if (!match.hasMatch())
            continue;
        const bool mjpeg = line.contains(QStringLiteral("vcodec=mjpeg"), Qt::CaseInsensitive);
        const int width = match.captured(4).toInt();
        const int height = match.captured(5).toInt();
        const double maxFps = match.captured(6).toDouble();
        const qint64 area = qint64(width) * height;
        if (width <= 0 || height <= 0 || maxFps < 15 || area > (mjpeg ? mjpegCeiling : rawCeiling))
            continue;
        const WebcamCameraFormat candidate{QSize(width, height), mjpeg};
        candidates.append(candidate);
        if (fps <= 0 || maxFps >= fps)
            sustained.append(candidate);
    }
    const QVector<WebcamCameraFormat>& pool = sustained.isEmpty() ? candidates : sustained;
    WebcamCameraFormat best;
    qint64 bestArea = 0;
    for (const WebcamCameraFormat& candidate : pool) {
        const qint64 area = qint64(candidate.size.width()) * candidate.size.height();
        if (area > bestArea || (area == bestArea && candidate.mjpeg && !best.mjpeg)) {
            best = candidate;
            bestArea = area;
        }
    }
    return best;
}

WebcamPreview::WebcamPreview(const QString& device, int fps, QObject* parent)
    : QObject(parent)
    , m_device(device)
    , m_fps(std::max(1, fps))
    , m_token(QUuid::createUuid().toString(QUuid::WithoutBraces))
    , m_server(new QTcpServer(this))
{
    connect(m_server, &QTcpServer::newConnection, this, &WebcamPreview::acceptClients);
}

WebcamPreview::~WebcamPreview()
{
    for (QProcess* process : {m_probe, m_camera}) {
        if (process && process->state() != QProcess::NotRunning) {
            disconnect(process, nullptr, this, nullptr);
            process->kill();
            process->waitForFinished(2000);
        }
    }
}

void WebcamPreview::start()
{
    if (ffmpegPath().isEmpty() || !m_server->listen(QHostAddress::LocalHost, 0)) {
        emit failed(QStringLiteral("Camera preview is unavailable"));
        return;
    }
    m_probe = new QProcess(this);
    m_probe->setProcessChannelMode(QProcess::MergedChannels);
    m_probe->setProgram(ffmpegPath());
    m_probe->setArguments({QStringLiteral("-hide_banner"), QStringLiteral("-f"),
                           QStringLiteral("dshow"), QStringLiteral("-list_options"),
                           QStringLiteral("true"), QStringLiteral("-i"),
                           QStringLiteral("video=%1").arg(m_device)});
    connect(m_probe, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished), this,
            [this](int, QProcess::ExitStatus) {
                m_options = QString::fromUtf8(m_probe->readAll());
                startCamera(webcamPreferredFormat(m_options, m_fps));
            });
    connect(m_probe, &QProcess::errorOccurred, this, [this](QProcess::ProcessError error) {
        if (error == QProcess::FailedToStart)
            emit failed(QStringLiteral("Camera preview could not start"));
    });
    m_probe->start();
    QTimer::singleShot(15000, this, [this] {
        if (m_probe && m_probe->state() != QProcess::NotRunning)
            m_probe->kill();
    });
}

void WebcamPreview::startCamera(const WebcamCameraFormat& format)
{
    ++m_openAttempts;
    m_framesSinceOpen = 0;
    m_camera = new QProcess(this);
    m_camera->setProgram(ffmpegPath());
    QStringList args{QStringLiteral("-hide_banner"), QStringLiteral("-loglevel"),
                     QStringLiteral("error"), QStringLiteral("-f"), QStringLiteral("dshow")};
    if (format.mjpeg)
        args << QStringLiteral("-vcodec") << QStringLiteral("mjpeg");
    if (!format.size.isEmpty())
        args << QStringLiteral("-video_size")
             << QStringLiteral("%1x%2").arg(format.size.width()).arg(format.size.height());
    if (m_openAttempts == 1)
        args << QStringLiteral("-framerate") << QString::number(m_fps);
    args << QStringLiteral("-i") << QStringLiteral("video=%1").arg(m_device)
         << QStringLiteral("-an") << QStringLiteral("-c:v")
         << (format.mjpeg ? QStringLiteral("copy") : QStringLiteral("mjpeg"));
    if (!format.mjpeg)
        args << QStringLiteral("-q:v") << QStringLiteral("2");
    args << QStringLiteral("-f") << QStringLiteral("mjpeg") << QStringLiteral("pipe:1");
    m_camera->setArguments(args);
    if (m_openAttempts == 1) {
        // A camera that cannot keep up at the chosen format is bandwidth- or
        // driver-limited: it sends a fraction of the frames the recording
        // assumes, and the forced frame rate turns the gaps into staggering.
        // Reopen once at a mode that fits in 720p - MJPEG where the camera
        // offers it - and accept whatever rate the fallback delivers.
        QTimer::singleShot(2500, this, [this, format] {
            const double expected = qMax(15.0, m_fps * 0.7);
            if (!m_camera || m_camera->state() != QProcess::Running
                || m_framesSinceOpen >= int(expected * 2.5)) {
                return;
            }
            WebcamCameraFormat fallback =
                webcamPreferredFormat(m_options, m_fps, qint64(1280) * 720);
            if (fallback.size == format.size && fallback.mjpeg == format.mjpeg)
                return;
            if (fallback.size.isEmpty())
                fallback = {QSize(1280, 720), false};
            disconnect(m_camera, nullptr, this, nullptr);
            m_camera->kill();
            m_camera->deleteLater();
            m_camera = nullptr;
            startCamera(fallback);
        });
    }
    connect(m_camera, &QProcess::readyReadStandardOutput, this, &WebcamPreview::consumeFrames);
    connect(m_camera, &QProcess::readyReadStandardError, this, [this] {
        m_camera->readAllStandardError();
    });
    connect(m_camera, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished), this,
            [this](int, QProcess::ExitStatus) {
                if (!m_hasFrame && m_openAttempts == 1) {
                    m_camera->deleteLater();
                    m_camera = nullptr;
                    QTimer::singleShot(350, this, [this] { startCamera({}); });
                    return;
                }
                if (m_client)
                    m_client->disconnectFromHost();
                emit failed(QStringLiteral("Camera preview stopped"));
            });
    connect(m_camera, &QProcess::errorOccurred, this, [this](QProcess::ProcessError error) {
        if (error == QProcess::FailedToStart)
            emit failed(QStringLiteral("Camera preview could not start"));
    });
    m_camera->start();
}

QString WebcamPreview::streamUrl() const
{
    if (!m_server->isListening() || !m_hasFrame || !m_camera
        || m_camera->state() != QProcess::Running)
        return {};
    return QStringLiteral("http://127.0.0.1:%1/%2").arg(m_server->serverPort()).arg(m_token);
}

void WebcamPreview::consumeFrames()
{
    static const QByteArray start("\xff\xd8", 2);
    static const QByteArray end("\xff\xd9", 2);
    m_buffer.append(m_camera->readAllStandardOutput());
    if (m_buffer.size() > 8 * 1024 * 1024) {
        const int last = m_buffer.lastIndexOf(start);
        m_buffer = last >= 0 ? m_buffer.mid(last) : QByteArray();
    }
    QByteArray newest;
    int index = m_buffer.indexOf(start);
    while (index >= 0) {
        if (index > 0)
            m_buffer.remove(0, index);
        const int finish = m_buffer.indexOf(end, 2);
        if (finish < 0)
            break;
        const QByteArray frame = m_buffer.left(finish + 2);
        m_buffer.remove(0, finish + 2);
        if (m_client && m_client->bytesToWrite() < 2 * 1024 * 1024)
            m_client->write(frame);
        newest = frame;
        ++m_framesSinceOpen;
        index = m_buffer.indexOf(start);
    }
    if (newest.isEmpty())
        return;
    m_latestFrame = newest;
    if (m_lastPreview.isValid() && m_lastPreview.elapsed() < 66)
        return;
    QBuffer buffer(&newest);
    buffer.open(QIODevice::ReadOnly);
    QImageReader reader(&buffer, "JPEG");
    const QSize source = reader.size();
    if (source.width() > 640 || source.height() > 640)
        reader.setScaledSize(source.scaled(640, 640, Qt::KeepAspectRatio));
    const QImage image = reader.read();
    if (!image.isNull()) {
        m_lastPreview.restart();
        m_hasFrame = true;
        emit frameReady(image);
    }
}

void WebcamPreview::acceptClients()
{
    while (QTcpSocket* socket = m_server->nextPendingConnection()) {
        // Each frame is written whole, and a sub-MSS tail left to Nagle waits
        // on a delayed ACK: the reader then stamps a late arrival wallclock on
        // it and the forced frame rate turns the jitter into dup/drop judder.
        socket->setSocketOption(QAbstractSocket::LowDelayOption, 1);
        connect(socket, &QTcpSocket::disconnected, socket, &QObject::deleteLater);
        QTimer::singleShot(5000, socket, [this, socket] {
            if (m_client != socket)
                socket->disconnectFromHost();
        });
        connect(socket, &QTcpSocket::readyRead, this, [this, socket] {
            const QByteArray request = socket->peek(8192);
            if (!request.contains("\r\n\r\n")) {
                if (request.size() >= 8192)
                    socket->disconnectFromHost();
                return;
            }
            socket->readAll();
            if (m_client && m_client->state() != QAbstractSocket::ConnectedState)
                m_client = nullptr;
            if (!request.startsWith("GET /" + m_token.toUtf8() + " ") || m_client) {
                socket->write("HTTP/1.0 403 Forbidden\r\n\r\n");
                socket->disconnectFromHost();
                return;
            }
            m_client = socket;
            connect(socket, &QTcpSocket::disconnected, this, [this, socket] {
                if (m_client == socket)
                    m_client = nullptr;
            });
            socket->write("HTTP/1.0 200 OK\r\nContent-Type: image/jpeg\r\n"
                          "Cache-Control: no-store\r\n\r\n");
            socket->write(m_latestFrame);
        });
    }
}

} // namespace Platform
