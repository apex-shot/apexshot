#pragma once

#include <QObject>
#include <QRect>
#include <QString>

class QElapsedTimer;

/// Platform seam for screen recording.
///
/// The Windows build implements this over a bundled ffmpeg. The Linux build
/// keeps its existing recorder in the Rust binary and never links this seam.
namespace Platform {

struct RecordingRequest {
    enum class Target {
        /// A whole display, identified by the region of its geometry.
        Screen,
        /// An arbitrary desktop rectangle, in device pixels.
        Area,
        /// A window's visible frame, in device pixels. The recorder captures
        /// the rectangle; it cannot exclude occluding windows.
        Window,
    };

    Target target = Target::Screen;
    /// Desktop rectangle in device pixels. Required for Screen and Area.
    QRect region;
    /// Native window handle for Window, opaque to the recorder.
    quintptr windowId = 0;
    /// Final MP4 path. The parent directory is created when missing.
    QString outputPath;
    bool includeCursor = true;
    bool includeMicrophone = false;
    bool includeSpeaker = false;
    /// Frames per second. 0 falls back to 30.
    int fps = 30;
    /// Maximum output size. Zero means native resolution.
    int maxWidth = 0;
    int maxHeight = 0;
    /// x264 CRF. 0 falls back to 20.
    int crf = 20;
};

class RecordingSession : public QObject
{
    Q_OBJECT

public:
    explicit RecordingSession(QObject* parent = nullptr);
    ~RecordingSession() override;

    /// Whether this machine can record: an ffmpeg binary with `gdigrab` is
    /// available. `error` receives the reason when it can't.
    static bool available(QString* error = nullptr);

    /// Begin recording. On failure, `error` explains why and the session
    /// stays idle. The session does not own the output path.
    bool start(const RecordingRequest& request, QString& error);

    /// Finalise the current segment. Resuming starts a new one.
    void pause();
    void resume();
    /// Finalise, concatenate, and keep the output file.
    void stop();
    /// Stop and remove every segment and the output file.
    void discard();

    bool isActive() const;
    bool isPaused() const;
    QString outputPath() const;

signals:
    void elapsedChanged(qint64 elapsedMs);
    void stateChanged();
    /// A non-fatal problem, such as a requested audio device that does not
    /// exist. Recording continues without that track.
    void warning(const QString& message);
    /// `kept` is false for a discard. `path` is empty unless `kept`.
    void finished(bool kept, const QString& path, const QString& error);

private:
    class Private;
    Private* d;
};

} // namespace Platform
