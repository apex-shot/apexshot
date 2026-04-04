#pragma once

#include <QRect>
#include <QSize>
#include <QString>

namespace ScreenCapture {

bool captureFullscreenToTempPng(QString& outPath,
                                QSize& outSize,
                                QString& outError,
                                bool includeCursor = true);
bool captureFullscreenToTempPngViaPortal(QString& outPath,
                                         QSize& outSize,
                                         QString& outError,
                                         bool includeCursor = true);
bool captureAreaToTempPng(const QRect& logicalSelection,
                          QString& outPath,
                          QSize& outSize,
                          QString& outError,
                          bool includeCursor = true);
bool captureAreaToTempPngViaPortal(const QRect& logicalSelection,
                                   QString& outPath,
                                   QSize& outSize,
                                   QString& outError,
                                   bool includeCursor = true);
bool captureAreaToTempPngFromOverlayLocal(const QRect& localSelection,
                                          const QRect& overlayGeometry,
                                          QString& outPath,
                                          QSize& outSize,
                                          QString& outError,
                                          bool includeCursor = true);

}
