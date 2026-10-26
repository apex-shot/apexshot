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
    bool writeSidecar(const QString& videoPath, QString* error = nullptr) const;

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

/// Returns the sidecar name of a cursor kind: "default", "hand", "text",
/// "crosshair", "wait", or "resize".
QString cursorKindName(CursorKind kind);

/// Maps a cursor handle to the shared kind by comparing it against the
/// standard system cursors. Anything that is not one of those, and a null
/// handle, records as default: see the note in the implementation about
/// application-supplied cursors.
CursorKind cursorKindFor(HCURSOR cursor);

} // namespace Platform
