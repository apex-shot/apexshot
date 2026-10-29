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
    return webcamBubbleRectForWidth(region,
                                    region.height() * kWebcamSizeFractions[size],
                                    shapeIndex,
                                    relX,
                                    relY);
}

QRect webcamBubbleRectForWidth(const QRect& region,
                               double requestedWidth,
                               int shapeIndex,
                               double relX,
                               double relY)
{
    if (region.isEmpty())
        return QRect();

    const int shape = std::clamp(shapeIndex, 0, 3);
    const double aspect = kWebcamShapeAspects[shape];
    // The margin scales with the capture so a small area capture does not lose
    // most of its width to a corner inset.
    const double margin = clamped(region.height() * 0.01, 8.0, 40.0);

    int width = std::max(1, int(std::lround(requestedWidth)));
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

QRect webcamFitRect(const QRect& region, const QRect& rect)
{
    if (region.isEmpty() || rect.isEmpty())
        return QRect();

    const double scale = std::min({1.0, double(region.width()) / rect.width(),
                                   double(region.height()) / rect.height()});
    const int width = std::max(1, int(std::floor(rect.width() * scale)));
    const int height = std::max(1, int(std::floor(rect.height() * scale)));
    const int x = std::clamp(rect.x(), region.x(), region.x() + region.width() - width);
    const int y = std::clamp(rect.y(), region.y(), region.y() + region.height() - height);
    return QRect(x, y, width, height);
}

WebcamLayout webcamLayoutForRect(const QRect& region, const QRect& rect, int shape, bool flip)
{
    WebcamLayout layout;
    const QRect fitted = webcamFitRect(region, rect);
    if (fitted.isEmpty())
        return layout;
    layout.x = double(fitted.x() - region.x()) / region.width();
    layout.y = double(fitted.y() - region.y()) / region.height();
    layout.w = double(fitted.width()) / region.width();
    layout.h = double(fitted.height()) / region.height();
    layout.shape = std::clamp(shape, 0, 3);
    layout.flip = flip;
    return layout;
}

QRect webcamProjectRect(const QRect& screen, const QRect& region, const QRect& rect)
{
    if (screen.isEmpty() || region.isEmpty() || rect.isEmpty())
        return {};
    // The camera keeps the pixel size it had on screen: a window is a smaller
    // frame than the display, and scaling the bubble down with it would show
    // the person a camera smaller than the one they arranged.
    const QRect size = webcamFitRect(region, QRect(region.topLeft(), rect.size()));
    const int xSpan = std::max(0, screen.width() - rect.width());
    const int ySpan = std::max(0, screen.height() - rect.height());
    const double x = xSpan > 0 ? clamped(double(rect.x() - screen.x()) / xSpan, 0.0, 1.0) : 0.0;
    const double y = ySpan > 0 ? clamped(double(rect.y() - screen.y()) / ySpan, 0.0, 1.0) : 0.0;
    return QRect(region.x() + int(std::lround(x * (region.width() - size.width()))),
                 region.y() + int(std::lround(y * (region.height() - size.height()))),
                 size.width(), size.height());
}

bool webcamRelativePosition(const QRect& region,
                            int sizeIndex,
                            int shapeIndex,
                            const QRect& rect,
                            double* relX,
                            double* relY)
{
    if (region.isEmpty())
        return false;
    const int size = std::clamp(sizeIndex, 0, 3);
    return webcamRelativePositionForWidth(
        region, region.height() * kWebcamSizeFractions[size], shapeIndex, rect, relX, relY);
}

bool webcamRelativePositionForWidth(const QRect& region,
                                    double width,
                                    int shapeIndex,
                                    const QRect& rect,
                                    double* relX,
                                    double* relY)
{
    if (region.isEmpty() || rect.isEmpty() || !relX || !relY)
        return false;

    // The two extreme placements give the range the stored position spans;
    // recovering it from those keeps the drag and the config's convention in
    // one place instead of duplicating the margin and size maths here.
    const QRect low = webcamBubbleRectForWidth(region, width, shapeIndex, 0.0, 0.0);
    const QRect high = webcamBubbleRectForWidth(region, width, shapeIndex, 1.0, 1.0);
    const int spanX = high.x() - low.x();
    const int spanY = high.y() - low.y();
    *relX = spanX > 0 ? clamped(double(rect.x() - low.x()) / spanX, 0.0, 1.0) : 0.0;
    *relY = spanY != 0 ? clamped(double(rect.y() - low.y()) / spanY, 0.0, 1.0) : 0.0;
    return true;
}

} // namespace Platform
