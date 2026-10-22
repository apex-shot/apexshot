#pragma once

#include <QByteArray>
#include <QImage>
#include <QString>

/// QR code decoding through the vendored quirc decoder.
namespace Platform {

/// Decodes the first readable QR symbol in the image.
/// outPayload holds the raw symbol bytes on success.
bool decodeQr(const QImage& image, QByteArray& outPayload, QString& outError);

} // namespace Platform
