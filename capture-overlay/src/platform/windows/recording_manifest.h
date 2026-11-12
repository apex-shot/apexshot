#pragma once

#include <QJsonObject>
#include <QPoint>
#include <QRect>
#include <QSize>
#include <QString>
#include <QVector>

namespace Platform {

/// What a kept recording was, written beside its pointer sidecar. The studied
/// recorder keeps the same kind of document per recording: one entry per
/// session between pauses, plus the capture and source details the samples
/// alone do not carry.
struct RecordingManifest {
    struct Session {
        qint64 startedAtUnixMs = 0;
        qint64 endedAtUnixMs = 0;
        /// Media time, in milliseconds of the joined video, the session covers.
        qint64 startMs = 0;
        qint64 endMs = 0;
        /// The window's frame when this session began; empty for screen and
        /// area recordings.
        QRect windowBounds;
    };

    qint64 startedAtUnixMs = 0;
    qint64 finishedAtUnixMs = 0;
    qint64 durationMs = 0;
    /// "screen", "area" or "window".
    QString target;
    QRect region;
    int fps = 0;
    double displayScale = 1.0;
    /// Hz of the display the region sits on; 0 when unknown.
    int displayRefreshRate = 0;
    /// Device pixel ratio of the display the region sits on, e.g. 1.5 at 150%.
    double devicePixelRatio = 1.0;
    QSize outputSize;
    /// "h264_qsv", "h264_nvenc" or "libx264".
    QString encoder;
    int crf = 0;
    /// "window-graphics", "ddagrab" or "gdigrab".
    QString captureBackend;

    bool hasWindow = false;
    quintptr windowId = 0;
    QString windowTitle;
    /// The owning process's executable file name, e.g. "notepad.exe".
    QString windowApp;
    QRect windowBounds;

    bool microphone = false;
    /// The microphone's DirectShow name; empty for the default device.
    QString microphoneDevice;
    /// "off", "all" or "app".
    QString systemAudio = QStringLiteral("off");
    QString systemAudioApplication;
    bool monoAudio = false;

    bool hasCamera = false;
    QString cameraDevice;
    int cameraWidth = 0;
    int cameraHeight = 0;
    int cameraFps = 0;
    /// The track's `start` from the sidecar, in milliseconds.
    qint64 cameraStartMs = 0;
    qint64 cameraDurationMs = 0;
    /// Wall-clock gap between the screen and camera processes starting for
    /// the first segment, or -1 when not measured.
    qint64 cameraProcessDelayMs = -1;

    bool keys = false;
    QVector<Session> sessions;
    /// Moments marked while recording, in milliseconds of the joined video.
    QVector<qint64> markers;
};

/// The manifest as JSON (format version 1).
QJsonObject recordingManifestJson(const RecordingManifest& manifest);

/// `<pointer sidecar path>.recording.json`, or empty when the video has no
/// sidecar location.
QString recordingManifestPathFor(const QString& videoPath);

/// Write the manifest for `videoPath`. Returns false and fills `error` on failure.
bool writeRecordingManifest(const QString& videoPath,
                            const RecordingManifest& manifest,
                            QString* error = nullptr);

struct WindowIdentity {
    QString title;
    QString app;
    QRect bounds;
};

/// Title, owning executable name and visible frame of a native window.
/// Fields are empty when the window is gone.
WindowIdentity windowIdentity(quintptr windowId);

/// The refresh rate, in Hz, of the display nearest `point`; 0 when unknown.
int displayRefreshRateAt(const QPoint& point);

} // namespace Platform
