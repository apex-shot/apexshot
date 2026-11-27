#pragma once

#include <QImage>
#include <QRect>
#include <QString>
#include <QVector>

/// Text recognition through the platform engine.
namespace Platform {

/// Extracts text from the image using Windows.Media.Ocr.
/// Returns false and sets outError when the engine is unavailable (for
/// example, when no OCR language pack is installed).
bool recognizeText(const QImage& image, const QString& language, bool keepLineBreaks,
                   QString& outText, QString& outError);

/// One recognised line and its bounding box in image pixels.
struct OcrTextLine {
    QString text;
    QRect rect;
};

/// Recognises text line by line with bounding boxes. Returns false and sets
/// outError when the engine is unavailable.
bool recognizeLines(const QImage& image, const QString& language,
                    QVector<OcrTextLine>& outLines, QString& outError);

} // namespace Platform
