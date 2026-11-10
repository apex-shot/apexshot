#include "platform/windows/win_util.h"
#include "platform/windows/hdr_capture.h"
#include "platform/windows/screenshot_scale.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <iterator>
#include <vector>

namespace {

std::uint16_t toHalf(float value)
{
    if (value == 0.0f)
        return 0;
    std::uint32_t bits = 0;
    std::memcpy(&bits, &value, sizeof(bits));
    const std::uint32_t sign = (bits >> 16) & 0x8000u;
    const int exponent = static_cast<int>((bits >> 23) & 0xffu) - 127 + 15;
    const std::uint32_t mantissa = (bits >> 13) & 0x3ffu;
    return static_cast<std::uint16_t>(sign | (static_cast<std::uint32_t>(exponent) << 10) | mantissa);
}

float srgbToLinear(int value)
{
    const float v = value / 255.0f;
    return v <= 0.04045f ? v / 12.92f : std::pow((v + 0.055f) / 1.055f, 2.4f);
}

QImage convert(const std::vector<float>& rgb, float whiteNits)
{
    const int width = static_cast<int>(rgb.size() / 3);
    std::vector<std::uint16_t> pixels;
    for (int i = 0; i < width; ++i) {
        pixels.push_back(toHalf(rgb[i * 3]));
        pixels.push_back(toHalf(rgb[i * 3 + 1]));
        pixels.push_back(toHalf(rgb[i * 3 + 2]));
        pixels.push_back(toHalf(1.0f));
    }
    return Platform::Win::scRgbToSdr(pixels.data(), width, 1, width * 8, whiteNits);
}

bool withinOne(int actual, int expected)
{
    return std::abs(actual - expected) <= 1;
}

int checkHdrConversion()
{
    if (Platform::Win::hdrSdrWhiteNits(QStringLiteral("\\\\.\\NOT-A-DISPLAY")) != 0.0f) {
        std::fprintf(stderr, "An unknown display was reported as HDR\n");
        return 1;
    }

    // 240-nit SDR white: scRGB 3.0 is SDR white, and SDR values round-trip.
    const float gain = 240.0f / 80.0f;
    const QImage sdr = convert({srgbToLinear(128) * gain, srgbToLinear(128) * gain,
                                srgbToLinear(128) * gain, gain, gain, gain, 0.0f, 0.0f, 0.0f,
                                -0.5f, srgbToLinear(64) * gain, 0.0f},
                               240.0f);
    if (sdr.width() != 4 || !withinOne(qRed(sdr.pixel(0, 0)), 128) || !withinOne(qGreen(sdr.pixel(0, 0)), 128)
        || !withinOne(qBlue(sdr.pixel(0, 0)), 128) || qRed(sdr.pixel(1, 0)) != 255
        || qGreen(sdr.pixel(1, 0)) != 255 || qBlue(sdr.pixel(1, 0)) != 255
        || qRed(sdr.pixel(2, 0)) != 0 || qRed(sdr.pixel(3, 0)) != 0
        || !withinOne(qGreen(sdr.pixel(3, 0)), 64) || qBlue(sdr.pixel(3, 0)) != 0) {
        std::fprintf(stderr, "SDR content did not survive the HDR conversion\n");
        return 1;
    }

    // A frame with a 10x highlight rolls off instead of clipping, keeps the
    // highlight's hue, and leaves SDR white only slightly below full.
    const QImage hdr = convert({gain, gain, gain, 10.0f * gain, 5.0f * gain, 0.0f}, 240.0f);
    const QRgb white = hdr.pixel(0, 0);
    const QRgb highlight = hdr.pixel(1, 0);
    if (qRed(white) < 245 || qRed(white) > 254 || qRed(highlight) < 250
        || qGreen(highlight) < 150 || qGreen(highlight) > 220 || qBlue(highlight) != 0) {
        std::fprintf(stderr, "HDR highlights were not rolled off\n");
        return 1;
    }
    return 0;
}

} // namespace

int main()
{
    Platform::MonitorInfo low;
    low.geometry = QRect(-1920, 0, 1920, 1080);
    low.dpr = 1.0;
    Platform::MonitorInfo high;
    high.geometry = QRect(0, 0, 2560, 1440);
    high.dpr = 1.5;
    const QVector<Platform::MonitorInfo> displays{low, high};
    if (Platform::screenshotScale(QRect(-500, 100, 200, 200), displays) != 1.0
        || Platform::screenshotScale(QRect(500, 100, 200, 200), displays) != 1.5
        || Platform::screenshotScale(QRect(-50, 100, 100, 100), displays) != 1.5
        || Platform::screenshotScale(QRect(5000, 0, 100, 100), displays) != 1.0) {
        std::fprintf(stderr, "Screenshot scale did not match its source displays\n");
        return 1;
    }

    constexpr int size = 128;
    BYTE andMask[32 * 32 / 8]{};
    BYTE xorMask[32 * 32 / 8];
    std::fill(std::begin(xorMask), std::end(xorMask), BYTE{0xff});
    HCURSOR cursor = CreateCursor(nullptr, 4, 6, 32, 32, andMask, xorMask);
    HDC dc = CreateCompatibleDC(nullptr);
    BITMAPINFO info{};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = size;
    info.bmiHeader.biHeight = -size;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    void* data = nullptr;
    HBITMAP bitmap = dc ? CreateDIBSection(dc, &info, DIB_RGB_COLORS, &data, nullptr, 0) : nullptr;
    if (!cursor || !dc || !bitmap || !data) {
        std::fprintf(stderr, "Could not create the synthetic cursor or bitmap\n");
        if (bitmap)
            DeleteObject(bitmap);
        if (dc)
            DeleteDC(dc);
        if (cursor)
            DestroyCursor(cursor);
        return 1;
    }

    HGDIOBJ previous = SelectObject(dc, bitmap);
    auto* pixels = static_cast<quint32*>(data);
    std::fill(pixels, pixels + size * size, 0u);
    Platform::Win::drawCaptureCursor(dc, QRect(-110, -200, size, size), cursor, QPoint(-95, -180));
    GdiFlush();
    const bool inside = (pixels[14 * size + 11] & 0x00ffffffu) == 0x00ffffffu;
    const bool outside = (pixels[14 * size + 10] & 0x00ffffffu) == 0u
        && (pixels[13 * size + 11] & 0x00ffffffu) == 0u;

    SelectObject(dc, previous);
    DeleteObject(bitmap);
    DeleteDC(dc);
    DestroyCursor(cursor);
    if (!inside || !outside) {
        std::fprintf(stderr, "Cursor position or hotspot did not match the capture frame\n");
        return 1;
    }
    return checkHdrConversion();
}
