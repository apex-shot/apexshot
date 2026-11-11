#pragma once

#include <QImage>
#include <QObject>
#include <QRect>
#include <QString>
#include <QVector>

class QElapsedTimer;

/// Platform seam for screen recording.
///
/// The Windows build implements this over a bundled ffmpeg. The Linux build
/// keeps its existing recorder in the Rust binary and never links this seam.
namespace Platform {

/// Where the webcam bubble sat inside the captured region.
///
/// The recorder stores these fractions in the sidecar instead of the bubble's
/// pixel size, so the editor can rebuild the layer at any output resolution and
/// a recording made on one display looks the same on another.
struct WebcamLayout {
    double x = 0.0;
    double y = 0.0;
    double w = 0.0;
    double h = 0.0;
    /// The shared shape table: 0 circle, 1 square, 2 rectangle, 3 vertical.
    int shape = 0;
    bool flip = false;
};

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

    /// Where system audio comes from. Windows records it natively, because
    /// ffmpeg's dshow input cannot reach loopback audio.
    enum class SystemAudio {
        Off,
        /// Every app on the default output.
        AllApps,
        /// One app's process tree, e.g. every tab of one browser.
        Application,
    };

    Target target = Target::Screen;
    /// Desktop rectangle in device pixels. Required for Screen and Area.
    QRect region;
    /// Native window handle for Window, opaque to the recorder.
    quintptr windowId = 0;
    /// Final MP4 path. The parent directory is created when missing.
    QString outputPath;
    bool includeCursor = true;
    /// Capture key down/up events. Off by default: a recording should not
    /// collect keystrokes unless the caller explicitly asks for them.
    bool captureKeystrokes = false;
    bool includeMicrophone = false;
    /// The microphone's DirectShow friendly name. Empty uses the default.
    QString microphoneDevice;
    SystemAudio systemAudio = SystemAudio::Off;
    /// The process tree to capture when `systemAudio` is Application.
    quint32 systemAudioProcessId = 0;
    /// The app's display name, for logs and warnings.
    QString systemAudioApplication;
    /// Mix the recorded audio down to a single channel.
    bool monoAudio = false;
    /// Frames per second. 0 falls back to 30.
    int fps = 30;
    /// Maximum output size. Zero means native resolution.
    int maxWidth = 0;
    int maxHeight = 0;
    /// Output scale relative to device pixels; 1 keeps native resolution.
    double displayScale = 1.0;
    /// x264 CRF. 0 falls back to 20.
    int crf = 20;
    /// Encode the screen in HEVC instead of H.264. The camera track stays
    /// H.264.
    bool hevc = false;
    /// Record a camera track beside the video. A camera that cannot be opened
    /// is reported through `warning` and leaves the recording without a track
    /// rather than failing it.
    bool webcamEnabled = false;
    /// The dshow camera to record. Empty means the machine's first camera.
    QString webcamDevice;
    /// A running local webcam preview stream, when setup already owns the camera.
    QString webcamStreamUrl;
    /// Where the bubble sat in `region`, for the editor's layer.
    WebcamLayout webcamLayout;
};

/// The ffmpeg this build records with, resolved from APEXSHOT_FFMPEG, the
/// application folder, or PATH. Empty when none is available.
QString ffmpegPath();

/// A recording whose recorder stopped without finishing, found in the temp
/// folder by `RecordingSession::unfinishedRecordings`.
struct UnfinishedRecording {
    QString workDir;
    /// Where the recording was going to be saved.
    QString outputPath;
    qint64 startedAtUnixMs = 0;
    /// Recorded length when the state was last written, in milliseconds.
    qint64 durationMs = 0;
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

    /// Mark the current moment of the recording, for the editor's timeline.
    /// While paused the mark lands where the recording will resume.
    void addMarker();

    bool isActive() const;
    bool isPaused() const;
    QString outputPath() const;

    /// Move the camera layer while the recording runs. The camera is baked
    /// into the file when the recording ends, so the final position is the
    /// one the person left the bubble at.
    void setWebcamLayout(const WebcamLayout& layout);

    /// Whether the current recording successfully opened a webcam.
    bool webcamActive() const;

    /// Recordings whose recorder stopped without finishing. A work directory
    /// whose recorder process is still running is skipped, and so is this
    /// process's own unless `includeOwnProcess` is set (for tests). Work
    /// directories that hold no captured video are deleted.
    static QVector<UnfinishedRecording> unfinishedRecordings(bool includeOwnProcess = false);

    /// Join what an unfinished recording captured and save it the way a
    /// stopped recording is saved; `finished` reports the result. The video
    /// goes to the recorded output path, or beside it with " (recovered)"
    /// when that name is taken. Returns false with `error` when it cannot
    /// start.
    bool recover(const QString& workDir, QString& error);

    /// Delete an unfinished recording's captured data.
    static void discardUnfinished(const QString& workDir);

    /// Test seam: stop every capture process and timer the way a crash would,
    /// leaving the work directory and its state file behind.
    void abandonForTesting();

signals:
    void elapsedChanged(qint64 elapsedMs);
    /// A marker was added at `mediaMs` of the recording.
    void markerAdded(qint64 mediaMs);
    void stateChanged();
    /// The capture process has stopped for finalization or discard.
    void captureStopped();
    /// One live frame from the webcam track, for the bubble window. The frames
    /// are small (the preview is scaled down) and arrive at the recording's
    /// frame rate; they are not the frames that reach the track file.
    void webcamFrame(const QImage& frame);
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
