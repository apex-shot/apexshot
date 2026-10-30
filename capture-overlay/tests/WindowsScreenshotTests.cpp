#include "platform/windows/win_util.h"
#include "platform/windows/screenshot_scale.h"

#include <algorithm>
#include <cstdio>
#include <iterator>

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
    return 0;
}
