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

/// The same placement as fractions of `region`, which is what the sidecar
/// records and the editor re-renders at any output size.
WebcamLayout webcamLayoutFor(const QRect& region,
                             int sizeIndex,
                             int shapeIndex,
                             double relX,
                             double relY,
                             bool flip);

} // namespace Platform
