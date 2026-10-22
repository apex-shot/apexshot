#pragma once

#include <QImage>
#include <QObject>
#include <QRect>
#include <QString>

/// Scrolling capture for Windows windows and desktop regions.
///
/// Drives the target window with synthetic wheel input, captures the scroll
/// region between steps, and stitches the frames into one tall image. Fixed
/// top and bottom bands (toolbars, status bars) are detected once and appended
/// only once, so they neither break the overlap search nor repeat.
namespace Platform {

class ScrollCaptureSession : public QObject
{
    Q_OBJECT

public:
    explicit ScrollCaptureSession(QObject* parent = nullptr);
    ~ScrollCaptureSession() override;

    /// True when a capture region can be grabbed. Synthetic wheel input is
    /// always available on Windows, so this only reports a missing desktop.
    static bool available(QString* error = nullptr);

    /// Start scrolling `region` (device pixels). `windowId` is the window to
    /// foreground; 0 uses the window under the region's centre. The caller
    /// passes the client rectangle for window captures.
    bool start(const QRect& region, quintptr windowId, QString& error);

    /// Finish now: stop scrolling, stitch what has been captured, and emit
    /// finished(true, image, {}).
    void stop();
    /// Stop without stitching and emit finished(false, {}, {}).
    void discard();

    bool isActive() const;
    int frameCount() const;
    QSize captureSize() const;

signals:
    void statusChanged(const QString& status);
    void frameCaptured(int frames, int stitchedHeight);
    void finished(bool ok, const QImage& image, const QString& error);

private:
    class Private;
    Private* d;
};

} // namespace Platform
