#pragma once

#include "platform/recording.h"

#include <QRect>

namespace Platform {

/// Size presets, as the bubble's width in fractions of the captured region's
/// height. Sizing against the region rather than against the display keeps a
/// 4K recording's bubble the same size in proportion to a 1080p one.
inline constexpr double kWebcamSizeFractions[4] = {0.12, 0.19, 0.26, 0.34};

/// Height over width for each shape: circle, square, rectangle, vertical.
inline constexpr double kWebcamShapeAspects[4] = {1.0, 1.0, 0.75, 1.3};

/// The bubble's rectangle inside `region`, in device pixels.
///
/// `relX` runs left to right and `relY` bottom to top, the way the shared config
/// stores a position, so 1/0 is the bottom-right corner and 0/0 the bottom-left.
QRect webcamBubbleRect(const QRect& region,
                       int sizeIndex,
                       int shapeIndex,
                       double relX,
                       double relY);

/// The bubble's rectangle for an explicit width, which is how a bubble the
/// person resized comes back at the size they left it.
QRect webcamBubbleRectForWidth(const QRect& region,
                               double width,
                               int shapeIndex,
                               double relX,
                               double relY);

/// Recover the stored position for a bubble of `width`.
bool webcamRelativePositionForWidth(const QRect& region,
                                    double width,
                                    int shapeIndex,
                                    const QRect& rect,
                                    double* relX,
                                    double* relY);

/// The same placement as fractions of `region`, which is what the sidecar
/// records and the editor re-renders at any output size.
WebcamLayout webcamLayoutFor(const QRect& region,
                             int sizeIndex,
                             int shapeIndex,
                             double relX,
                             double relY,
                             bool flip);

/// Fit an existing bubble into a capture region without changing its aspect ratio.
QRect webcamFitRect(const QRect& region, const QRect& rect);

/// Express an existing bubble's placement in capture-relative coordinates.
WebcamLayout webcamLayoutForRect(const QRect& region, const QRect& rect, int shape, bool flip);

/// Project a screen preview into a recording without moving the preview. The
/// bubble keeps its on-screen pixel size and only its position is mapped.
QRect webcamProjectRect(const QRect& screen, const QRect& region, const QRect& rect);

/// Recover the stored position for a bubble the person dragged to `rect`.
/// Returns false when the region is empty or the position cannot be expressed.
bool webcamRelativePosition(const QRect& region,
                            int sizeIndex,
                            int shapeIndex,
                            const QRect& rect,
                            double* relX,
                            double* relY);

} // namespace Platform
