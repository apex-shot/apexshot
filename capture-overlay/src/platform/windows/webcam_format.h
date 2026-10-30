#pragma once

#include <QSize>
#include <QString>

namespace Platform {

struct WebcamCameraFormat {
    QSize size;
    bool mjpeg = false;
    /// The dshow pixel_format to open, when a raw mode was chosen. Empty for
    /// MJPEG or the driver's default.
    QString pixelFormat;
};

/// The camera mode to open, chosen from a dshow `-list_options` listing.
///
/// Modes that can sustain `fps` win over larger ones that cannot: a smooth
/// picture beats a sharp one. Between modes at the same size the lighter
/// pixel format wins, because uncompressed video is what starves a camera's
/// USB bandwidth. Raw modes are trusted only to 720p, where MJPEG modes may
/// reach `maxArea` (1080p unless a fallback asks for less). An empty size
/// opens the camera's default.
WebcamCameraFormat webcamPreferredFormat(const QString& options, int fps,
                                         qint64 maxArea = qint64(1920) * 1080);

}  // namespace Platform
