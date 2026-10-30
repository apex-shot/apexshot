#pragma once

#include <QImage>
#include <QString>

/// Text recognition through the platform engine.
namespace Platform {

/// Extracts text from the image using Windows.Media.Ocr.
/// Returns false and sets outError when the engine is unavailable (for
/// example, when no OCR language pack is installed).
bool recognizeText(const QImage& image, const QString& language, bool keepLineBreaks,
                   QString& outText, QString& outError);

} // namespace Platform
