#pragma once

#include <QByteArray>
#include <QElapsedTimer>
#include <QImage>
#include <QObject>
#include <QPointer>
#include <QString>

class QProcess;
class QTcpServer;
class QTcpSocket;

namespace Platform {

struct WebcamCameraFormat {
    QSize size;
    bool mjpeg = false;
};

/// The camera mode to open, chosen from a dshow `-list_options` listing.
///
/// Modes that can sustain `fps` win over larger ones that cannot: a smooth
/// picture beats a sharp one. Raw modes are trusted only to 720p, since
/// uncompressed video past that needs more USB bandwidth than a laptop camera
/// has; MJPEG modes may reach `maxArea` (1080p unless a fallback asks for
/// less). An empty size opens the camera's default.
WebcamCameraFormat webcamPreferredFormat(const QString& options, int fps,
                                         qint64 maxArea = qint64(1920) * 1080);

class WebcamPreview : public QObject
{
    Q_OBJECT

public:
    WebcamPreview(const QString& device, int fps, QObject* parent = nullptr);
    ~WebcamPreview() override;
    void start();
    QString streamUrl() const;
    bool hasFrame() const { return m_hasFrame; }

signals:
    void frameReady(const QImage& frame);
    void failed(const QString& message);

private:
    void startCamera(const WebcamCameraFormat& format);
    void consumeFrames();
    void acceptClients();

    QString m_device;
    int m_fps;
    QString m_options;
    QString m_token;
    QProcess* m_probe = nullptr;
    QProcess* m_camera = nullptr;
    QTcpServer* m_server = nullptr;
    QPointer<QTcpSocket> m_client;
    QByteArray m_buffer;
    QByteArray m_latestFrame;
    QElapsedTimer m_lastPreview;
    bool m_hasFrame = false;
    int m_openAttempts = 0;
    int m_framesSinceOpen = 0;
};

} // namespace Platform
