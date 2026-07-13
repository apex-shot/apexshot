#pragma once

#include <QImage>
#include <QPixmap>
#include <QRect>
#include <QSize>
#include <QString>

namespace ScreenCapture {

bool captureFullscreenToTempPng(QString& outPath, QSize& outSize, QString& outError);
bool captureFullscreenToTempPngViaPortal(QString& outPath,
                                         QSize& outSize,
                                         QString& outError);
/// Flameshot-style freeze: silent full-desktop capture into a QImage.
bool captureFullscreenToImage(QImage& outImage, QString& outError);
/// Crop a logical desktop rect from a freeze image and scale to logical size
/// so CaptureOverlay can paint 1:1 in widget coordinates.
QPixmap freezeBackgroundForLogicalRect(const QImage& fullDesktopImage,
                                       const QRect& logicalRect);
/// Crop selection from an existing freeze (no second portal call).
bool cropFromDesktopImageToTempPng(const QImage& fullDesktopImage,
                                   const QRect& logicalSelection,
                                   QString& outPath,
                                   QSize& outSize,
                                   QString& outError);
bool captureAreaToTempPng(const QRect& logicalSelection,
                          QString& outPath,
                          QSize& outSize,
                          QString& outError);
bool captureAreaToTempPngViaPortal(const QRect& logicalSelection,
                                   QString& outPath,
                                   QSize& outSize,
                                   QString& outError);
bool captureAreaToTempPngFromOverlayLocal(const QRect& localSelection,
                                          const QRect& overlayGeometry,
                                          QString& outPath,
                                          QSize& outSize,
                                          QString& outError);

}
