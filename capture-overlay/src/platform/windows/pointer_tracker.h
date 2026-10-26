#pragma once

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif

#include <windows.h>

#include <QObject>
#include <QRect>
#include <QString>

#include <functional>
#include <memory>

namespace Platform {

/// The cursor shapes the sidecar records. The editor parses the recorded name
/// with an enum that maps anything unknown to default, so these names are a
/// contract and a wrong one degrades silently.
enum class CursorKind {
    Default,
    Hand,
    Text,
    Crosshair,
    Wait,
    Resize,
};

/// The webcam track recorded beside a video.
///
/// The recorder writes the camera into its own file so the editor can composite
/// it as a layer instead of a burn-in, and records the bubble's position in
/// fractions of the captured region. Fractions keep the layer at the same place
/// and size whatever resolution the recording was made at, and let the editor
/// re-render it at any output size.
struct WebcamTrack {
    /// File name inside the pointer sidecar's directory.
    QString file;
    int width = 0;
    int height = 0;
    /// The track's own length. The camera's clock is not the desktop's, so the
    /// editor maps a video time onto the track by scaling with it.
    double duration = 0.0;
    double x = 0.0;
    double y = 0.0;
    double w = 0.0;
    double h = 0.0;
    /// The shared shape table: 0 circle, 1 square, 2 rectangle, 3 vertical.
    int shape = 0;
    bool flip = false;

    bool is_valid() const { return !file.isEmpty() && width > 0 && height > 0; }
};

class PointerTracker final : public QObject
{
public:
    using MediaClock = std::function<qint64()>;

    explicit PointerTracker(QObject* parent = nullptr);
    ~PointerTracker() override;

    void start(const QRect& region, int fps, MediaClock mediaClock);
    void pause(qint64 mediaTimeMs);
    void resume();
    void stop(qint64 mediaTimeMs);
    void discard();
    /// Write the sidecar. `webcam` is written as the `webcam` block when it is
    /// valid, so a recording without a camera carries no camera keys at all.
    bool writeSidecar(const QString& videoPath,
                      const WebcamTrack& webcam,
                      QString* error = nullptr) const;

private:
    struct Press {
        bool active = false;
        double down = 0.0;
        double x = 0.0;
        double y = 0.0;
        bool dragged = false;
    };

    void sample();
    void closePresses(qint64 mediaTimeMs);
    void reset();

    QRect region_;
    MediaClock mediaClock_;
    class Data;
    std::unique_ptr<Data> data_;
};

/// Returns the Windows verbatim canonical path for an existing video file.
QString canonicalVideoPathFor(const QString& videoPath);

/// Returns the app-data sidecar path for an existing video file.
QString sidecarPathFor(const QString& videoPath);

/// Returns the app-data path of the webcam track recorded beside a video. The
/// track lives next to the pointer sidecar, so both are found from the video
/// path alone and neither can be mistaken for user footage.
QString webcamTrackPathFor(const QString& videoPath);

/// Returns the sidecar name of a cursor kind: "default", "hand", "text",
/// "crosshair", "wait", or "resize".
QString cursorKindName(CursorKind kind);

/// Maps a cursor handle to the shared kind by comparing it against the
/// standard system cursors. Anything that is not one of those, and a null
/// handle, records as default: see the note in the implementation about
/// application-supplied cursors.
CursorKind cursorKindFor(HCURSOR cursor);

} // namespace Platform
