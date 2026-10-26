#include "platform/windows/webcam_layout.h"

#include <algorithm>
#include <cmath>

namespace Platform {

namespace {

double clamped(double value, double low, double high)
{
    return std::min(high, std::max(low, value));
}

} // namespace

QRect webcamBubbleRect(const QRect& region,
                       int sizeIndex,
                       int shapeIndex,
                       double relX,
                       double relY)
{
    if (region.isEmpty())
        return QRect();

    const int size = std::clamp(sizeIndex, 0, 3);
    const int shape = std::clamp(shapeIndex, 0, 3);
    const double aspect = kWebcamShapeAspects[shape];
    // The margin scales with the capture so a small area capture does not lose
    // most of its width to a corner inset.
    const double margin = clamped(region.height() * 0.01, 8.0, 40.0);

    int width = std::max(1, int(std::lround(region.height() * kWebcamSizeFractions[size])));
    int height = std::max(1, int(std::lround(width * aspect)));

    const int maxWidth = std::max(1, int(std::lround(region.width() - (2.0 * margin))));
    const int maxHeight = std::max(1, int(std::lround(region.height() - (2.0 * margin))));
    if (width > maxWidth) {
        width = maxWidth;
        height = std::max(1, int(std::lround(width * aspect)));
    }
    if (height > maxHeight) {
        height = maxHeight;
        width = std::max(1, int(std::lround(height / aspect)));
    }

    const int minX = int(std::lround(region.x() + margin));
    const int minY = int(std::lround(region.y() + margin));
    const int maxX = std::max(minX, int(std::lround(region.x() + region.width() - margin - width)));
    const int maxY = std::max(minY, int(std::lround(region.y() + region.height() - margin - height)));
    const int x = minX + int(std::lround((maxX - minX) * clamped(relX, 0.0, 1.0)));
    // A stored relY of 0 is the bottom edge, as the shared config defines it.
    const int y = minY + int(std::lround((maxY - minY) * (1.0 - clamped(relY, 0.0, 1.0))));
    return QRect(x, y, width, height);
}

WebcamLayout webcamLayoutFor(const QRect& region,
                             int sizeIndex,
                             int shapeIndex,
                             double relX,
                             double relY,
                             bool flip)
{
    WebcamLayout layout;
    const QRect rect = webcamBubbleRect(region, sizeIndex, shapeIndex, relX, relY);
    if (region.isEmpty() || rect.isEmpty())
        return layout;

    layout.x = double(rect.x() - region.x()) / region.width();
    layout.y = double(rect.y() - region.y()) / region.height();
    layout.w = double(rect.width()) / region.width();
    layout.h = double(rect.height()) / region.height();
    layout.shape = std::clamp(shapeIndex, 0, 3);
    layout.flip = flip;
    return layout;
}

} // namespace Platform
