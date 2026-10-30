#include "platform/windows/webcam_format.h"

#include <QRegularExpression>
#include <QVector>

namespace Platform {

/// Bandwidth tier of a raw pixel format name: 12-bit layouts move a quarter
/// less data than 16-bit ones, and RGB the most. A larger mode in a heavier
/// format starves the bus before it sharpens the picture, so tiers rank
/// ahead of size.
static int pixelFormatTier(const QString& pixelFormat)
{
    if (pixelFormat == QLatin1String("nv12") || pixelFormat == QLatin1String("nv21")
        || pixelFormat == QLatin1String("yuv420p") || pixelFormat == QLatin1String("i420")
        || pixelFormat == QLatin1String("yv12")) {
        return 1;
    }
    if (pixelFormat.startsWith(QLatin1String("rgb"))
        || pixelFormat.startsWith(QLatin1String("bgr"))) {
        return 3;
    }
    return 2;
}

static QString modePixelFormat(const QString& line)
{
    static const QRegularExpression name(QStringLiteral("pixel_format=(\\w+)"));
    return name.match(line).captured(1).toLower();
}

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
        if (width <= 0 || height <= 0 || maxFps < 15
            || area > (mjpeg ? mjpegCeiling : rawCeiling)) {
            continue;
        }
        candidates.append({QSize(width, height), mjpeg,
                           mjpeg ? QString() : modePixelFormat(line)});
        if (fps <= 0 || maxFps >= fps)
            sustained.append(candidates.last());
    }
    const QVector<WebcamCameraFormat>& pool = sustained.isEmpty() ? candidates : sustained;
    WebcamCameraFormat best;
    int bestTier = 4;
    qint64 bestArea = 0;
    for (const WebcamCameraFormat& candidate : pool) {
        const int tier = candidate.mjpeg ? 0 : pixelFormatTier(candidate.pixelFormat);
        const qint64 area = qint64(candidate.size.width()) * candidate.size.height();
        if (tier < bestTier || (tier == bestTier && area > bestArea)) {
            best = candidate;
            bestTier = tier;
            bestArea = area;
        }
    }
    return best;
}

}  // namespace Platform
