#pragma once

#include <QSize>
#include <QString>

#include <memory>

namespace Platform {

/// Captures one top-level window through Windows Graphics Capture and writes
/// BGRA frames to a named pipe for ffmpeg's rawvideo demuxer.
///
/// The item is captured without the pointer: the editor paints the cursor
/// from the pointer sidecar. Frames are paced at `fps`; the newest captured
/// frame is repeated when the window is static.
class WindowCaptureSource
{
public:
    WindowCaptureSource();
    ~WindowCaptureSource();

    WindowCaptureSource(const WindowCaptureSource&) = delete;
    WindowCaptureSource& operator=(const WindowCaptureSource&) = delete;

    /// Whether this Windows build exposes Window Graphics Capture.
    static bool supported(QString* error = nullptr);

    /// Create the pipe and validate the capture target before ffmpeg starts.
    bool prepare(quintptr windowId, const QSize& size, int fps, QString* error = nullptr);
    /// Start capturing and writing frames. Returns after the capture session is
    /// initialized; the writer keeps running until stop().
    bool start(QString* error = nullptr);
    void stop();

    bool isPrepared() const;
    QString pipePath() const;
    QSize size() const;
    QString lastError() const;

private:
    struct Impl;
    std::unique_ptr<Impl> d;
};

} // namespace Platform
