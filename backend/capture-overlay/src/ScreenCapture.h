#pragma once

#include <QRect>
#include <QSize>
#include <QString>

namespace ScreenCapture {

bool captureFullscreenToTempPng(QString& outPath, QSize& outSize, QString& outError);
bool captureFullscreenToTempPngViaPortal(QString& outPath,
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
                                          const QSize& overlaySize,
                                          QString& outPath,
                                          QSize& outSize,
                                          QString& outError);

}
