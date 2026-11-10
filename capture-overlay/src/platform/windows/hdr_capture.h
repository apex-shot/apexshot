#pragma once

#include <QImage>
#include <QSize>
#include <QString>

#include <cstdint>

/// HDR-aware display grabs. GDI returns the desktop already converted to SDR,
/// which clips HDR video, games, and photos; desktop duplication in FP16
/// keeps the full range so it can be rolled off instead.
namespace Platform::Win {

/// SDR white level in nits for a display in HDR mode, or 0 when the display,
/// named by its GDI device name such as "\\.\DISPLAY1", is not in HDR mode.
float hdrSdrWhiteNits(const QString& gdiDeviceName);

/// Convert scRGB half-float RGBA pixels (linear, 1.0 = 80 nits) to an 8-bit
/// sRGB image. SDR content maps back to its original values; only a frame
/// that holds highlights above SDR white rolls them off instead of clipping.
QImage scRgbToSdr(const std::uint16_t* pixels,
                  int width,
                  int height,
                  int strideBytes,
                  float sdrWhiteNits);

/// Grab one HDR display through desktop duplication and convert it. Fails,
/// so the caller keeps the GDI pixels, when the display is rotated or the
/// frame does not match `expectedSize`. The cursor is not included.
bool captureHdrDisplay(const QString& gdiDeviceName,
                       const QSize& expectedSize,
                       QImage& out,
                       QString& outError);

} // namespace Platform::Win
