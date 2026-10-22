#include "platform/windows/qr.h"

#include <cstring>

extern "C" {
#include "third_party/quirc/quirc.h"
}

namespace Platform {

bool decodeQr(const QImage& image, QByteArray& outPayload, QString& outError)
{
    outPayload.clear();
    outError.clear();

    if (image.isNull()) {
        outError = QStringLiteral("empty image");
        return false;
    }

    const QImage gray = image.convertToFormat(QImage::Format_Grayscale8);
    const int width = gray.width();
    const int height = gray.height();

    quirc* decoder = quirc_new();
    if (!decoder) {
        outError = QStringLiteral("could not allocate the decoder");
        return false;
    }

    if (quirc_resize(decoder, width, height) < 0) {
        quirc_destroy(decoder);
        outError = QStringLiteral("could not allocate the frame buffer");
        return false;
    }

    int bufferWidth = 0;
    int bufferHeight = 0;
    uint8_t* buffer = quirc_begin(decoder, &bufferWidth, &bufferHeight);
    for (int y = 0; y < height; ++y) {
        std::memcpy(buffer + static_cast<size_t>(y) * bufferWidth,
                    gray.constScanLine(y),
                    static_cast<size_t>(width));
    }
    quirc_end(decoder);

    const int symbols = quirc_count(decoder);
    bool decoded = false;
    for (int i = 0; i < symbols; ++i) {
        quirc_code code{};
        quirc_data data{};
        quirc_extract(decoder, i, &code);
        if (quirc_decode(&code, &data) == QUIRC_SUCCESS) {
            outPayload = QByteArray(reinterpret_cast<const char*>(data.payload),
                                    static_cast<int>(data.payload_len));
            decoded = true;
            break;
        }
    }

    quirc_destroy(decoder);

    if (!decoded) {
        outError = symbols > 0 ? QStringLiteral("QR symbol found but not readable")
                               : QStringLiteral("no QR code found");
        return false;
    }
    return true;
}

} // namespace Platform
