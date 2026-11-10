#include "platform/windows/win_util.h"

#include "platform/platform.h"
#include "platform/windows/hdr_capture.h"

#include <QDebug>
#include <QVector>

#include <cstring>

void Platform::Win::drawCaptureCursor(HDC target,
                                      const QRect& frame,
                                      HCURSOR cursor,
                                      const QPoint& position)
{
    if (!target || !cursor)
        return;
    ICONINFO icon{};
    if (!GetIconInfo(cursor, &icon))
        return;
    const int x = position.x() - frame.x() - static_cast<int>(icon.xHotspot);
    const int y = position.y() - frame.y() - static_cast<int>(icon.yHotspot);
    DrawIconEx(target, x, y, cursor, 0, 0, 0, nullptr, DI_NORMAL);
    if (icon.hbmMask)
        DeleteObject(icon.hbmMask);
    if (icon.hbmColor)
        DeleteObject(icon.hbmColor);
}

namespace {

#ifndef PW_RENDERFULLCONTENT
#define PW_RENDERFULLCONTENT 0x00000002
#endif

using GetDpiForMonitorFn = HRESULT(WINAPI*)(HMONITOR, int, UINT*, UINT*);
using SetDpiAwarenessContextFn = BOOL(WINAPI*)(DPI_AWARENESS_CONTEXT);

constexpr int kEffectiveDpi = 0;

QString lastErrorText(const char* what)
{
    const DWORD code = GetLastError();
    return QStringLiteral("%1 failed (Win32 error %2)").arg(QString::fromLatin1(what)).arg(code);
}

SetDpiAwarenessContextFn resolveSetDpiAwarenessContext()
{
    HMODULE user32 = GetModuleHandleW(L"user32.dll");
    if (!user32)
        return nullptr;
    return reinterpret_cast<SetDpiAwarenessContextFn>(
        GetProcAddress(user32, "SetProcessDpiAwarenessContext"));
}

GetDpiForMonitorFn resolveGetDpiForMonitor()
{
    HMODULE shcore = LoadLibraryW(L"shcore.dll");
    if (!shcore)
        return nullptr;
    return reinterpret_cast<GetDpiForMonitorFn>(GetProcAddress(shcore, "GetDpiForMonitor"));
}

qreal devicePixelRatioFor(HMONITOR handle)
{
    static const GetDpiForMonitorFn getDpi = resolveGetDpiForMonitor();
    if (!getDpi)
        return 1.0;

    UINT dpiX = 96;
    UINT dpiY = 96;
    if (FAILED(getDpi(handle, kEffectiveDpi, &dpiX, &dpiY)) || dpiX == 0)
        return 1.0;
    return static_cast<qreal>(dpiX) / 96.0;
}

struct MonitorEnumState {
    QVector<Platform::MonitorInfo>* monitors = nullptr;
};

BOOL CALLBACK monitorEnumProc(HMONITOR handle, HDC, LPRECT, LPARAM userData)
{
    auto* state = reinterpret_cast<MonitorEnumState*>(userData);
    if (!state || !state->monitors)
        return FALSE;

    MONITORINFOEXW info{};
    info.cbSize = sizeof(info);
    if (!GetMonitorInfoW(handle, &info))
        return TRUE;

    Platform::MonitorInfo entry;
    entry.id = reinterpret_cast<quintptr>(handle);
    entry.name = QString::fromWCharArray(info.szDevice);
    entry.geometry = QRect(QPoint(info.rcMonitor.left, info.rcMonitor.top),
                           QSize(info.rcMonitor.right - info.rcMonitor.left,
                                 info.rcMonitor.bottom - info.rcMonitor.top));
    entry.workArea = QRect(QPoint(info.rcWork.left, info.rcWork.top),
                           QSize(info.rcWork.right - info.rcWork.left,
                                 info.rcWork.bottom - info.rcWork.top));
    entry.dpr = devicePixelRatioFor(handle);
    entry.primary = (info.dwFlags & MONITORINFOF_PRIMARY) != 0;

    // Primary display first, so index 0 is always the one the user thinks of
    // as "the screen".
    if (entry.primary)
        state->monitors->prepend(entry);
    else
        state->monitors->append(entry);
    return TRUE;
}

void drawCursor(HDC target, const QRect& rect)
{
    CURSORINFO cursor{};
    cursor.cbSize = sizeof(cursor);
    if (!GetCursorInfo(&cursor) || !(cursor.flags & CURSOR_SHOWING) || !cursor.hCursor)
        return;

    Platform::Win::drawCaptureCursor(target, rect, cursor.hCursor,
                                     QPoint(cursor.ptScreenPos.x, cursor.ptScreenPos.y));
}

bool imageFromBitmap(HDC device, HBITMAP bitmap, const QSize& size, QImage& out, QString& outError)
{
    BITMAPINFO info{};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = size.width();
    info.bmiHeader.biHeight = -size.height(); // top-down rows
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;

    QImage image(size, QImage::Format_RGB32);
    const int rows = GetDIBits(device,
                               bitmap,
                               0,
                               static_cast<UINT>(size.height()),
                               image.bits(),
                               &info,
                               DIB_RGB_COLORS);
    if (rows != size.height()) {
        outError = lastErrorText("GetDIBits");
        return false;
    }

    out = image;
    return true;
}

/// GPU-composited windows occasionally hand back an all-black surface instead
/// of failing outright, so the caller has to look at the pixels.
bool isMostlyBlack(const QImage& image)
{
    for (int y = 0; y < image.height(); y += 7) {
        const auto* row = reinterpret_cast<const QRgb*>(image.constScanLine(y));
        for (int x = 0; x < image.width(); x += 7) {
            if (row[x] != 0)
                return false;
        }
    }
    return true;
}

bool grabGdiRect(const QRect& rect,
                 bool includeCursor,
                 Platform::CaptureResult& out,
                 QString& outError)
{
    if (rect.width() <= 0 || rect.height() <= 0) {
        outError = QStringLiteral("empty capture rectangle");
        return false;
    }

    HDC screenDc = GetDC(nullptr);
    if (!screenDc) {
        outError = lastErrorText("GetDC");
        return false;
    }

    HDC memoryDc = CreateCompatibleDC(screenDc);
    HBITMAP bitmap = CreateCompatibleBitmap(screenDc, rect.width(), rect.height());
    if (!memoryDc || !bitmap) {
        if (bitmap)
            DeleteObject(bitmap);
        if (memoryDc)
            DeleteDC(memoryDc);
        ReleaseDC(nullptr, screenDc);
        outError = lastErrorText("CreateCompatibleDC/CreateCompatibleBitmap");
        return false;
    }

    HGDIOBJ previous = SelectObject(memoryDc, bitmap);
    bool ok = false;

    // Desktop composition is always on, so the screen DC already includes
    // layered windows. CAPTUREBLT would only make the cursor flicker.
    if (!BitBlt(memoryDc,
                0,
                0,
                rect.width(),
                rect.height(),
                screenDc,
                rect.x(),
                rect.y(),
                SRCCOPY)) {
        outError = lastErrorText("BitBlt");
    } else {
        if (includeCursor)
            drawCursor(memoryDc, rect);

        QImage image;
        if (imageFromBitmap(memoryDc, bitmap, rect.size(), image, outError)) {
            out.image = image;
            out.geometry = rect;
            ok = true;
        }
    }

    SelectObject(memoryDc, previous);
    DeleteObject(bitmap);
    DeleteDC(memoryDc);
    ReleaseDC(nullptr, screenDc);
    return ok;
}

/// Paint the cursor onto an image that is no longer in a GDI bitmap.
void drawCursorOnImage(QImage& image, const QRect& rect)
{
    if (image.format() != QImage::Format_RGB32)
        image = image.convertToFormat(QImage::Format_RGB32);

    BITMAPINFO info{};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = image.width();
    info.bmiHeader.biHeight = -image.height(); // top-down rows
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;

    void* bits = nullptr;
    HDC dc = CreateCompatibleDC(nullptr);
    HBITMAP dib = dc ? CreateDIBSection(dc, &info, DIB_RGB_COLORS, &bits, nullptr, 0) : nullptr;
    if (!dc || !dib || !bits) {
        if (dib)
            DeleteObject(dib);
        if (dc)
            DeleteDC(dc);
        return;
    }

    HGDIOBJ previous = SelectObject(dc, dib);
    const size_t rowBytes = static_cast<size_t>(image.width()) * 4;
    auto* rows = static_cast<uchar*>(bits);
    for (int y = 0; y < image.height(); ++y)
        std::memcpy(rows + y * rowBytes, image.constScanLine(y), rowBytes);
    drawCursor(dc, rect);
    GdiFlush();
    for (int y = 0; y < image.height(); ++y)
        std::memcpy(image.scanLine(y), rows + y * rowBytes, rowBytes);

    SelectObject(dc, previous);
    DeleteObject(dib);
    DeleteDC(dc);
}

/// Screen pixels for a rectangle. GDI hands back the desktop already
/// converted to SDR, which clips HDR content, so displays in HDR mode are
/// grabbed through desktop duplication and painted over the GDI frame.
bool grabScreenRect(const QRect& rect,
                    bool includeCursor,
                    Platform::CaptureResult& out,
                    QString& outError)
{
    QVector<Platform::MonitorInfo> hdrDisplays;
    for (const Platform::MonitorInfo& display : Platform::monitors()) {
        if (display.geometry.intersects(rect)
            && Platform::Win::hdrSdrWhiteNits(display.name) > 0.0f)
            hdrDisplays.append(display);
    }
    if (hdrDisplays.isEmpty())
        return grabGdiRect(rect, includeCursor, out, outError);

    if (!grabGdiRect(rect, false, out, outError))
        return false;

    QImage& image = out.image;
    for (const Platform::MonitorInfo& display : hdrDisplays) {
        QImage hdr;
        QString hdrError;
        if (!Platform::Win::captureHdrDisplay(display.name, display.geometry.size(), hdr,
                                              hdrError)) {
            qWarning().noquote() << "[capture] HDR grab failed, keeping GDI pixels:" << hdrError;
            continue;
        }
        const QRect part = display.geometry.intersected(rect);
        const QPoint source = part.topLeft() - display.geometry.topLeft();
        const QPoint target = part.topLeft() - rect.topLeft();
        for (int y = 0; y < part.height(); ++y) {
            std::memcpy(image.scanLine(target.y() + y) + static_cast<size_t>(target.x()) * 4,
                        hdr.constScanLine(source.y() + y) + static_cast<size_t>(source.x()) * 4,
                        static_cast<size_t>(part.width()) * 4);
        }
    }
    if (includeCursor)
        drawCursorOnImage(image, rect);
    return true;
}

} // namespace

namespace Platform {

void enableDpiAwareness()
{
    static const SetDpiAwarenessContextFn setContext = resolveSetDpiAwarenessContext();
    if (setContext) {
        // Per-monitor v2 keeps the captured pixels and the overlay in the same
        // coordinate space on mixed-DPI setups.
        if (setContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2))
            return;
    }
    SetProcessDPIAware();
}

QVector<MonitorInfo> monitors()
{
    QVector<MonitorInfo> result;
    MonitorEnumState state{&result};
    if (!EnumDisplayMonitors(nullptr, nullptr, monitorEnumProc, reinterpret_cast<LPARAM>(&state)))
        return {};
    return result;
}

MonitorInfo monitorAt(const QPoint& point)
{
    const QVector<MonitorInfo> all = monitors();
    for (const MonitorInfo& monitor : all) {
        if (monitor.geometry.contains(point))
            return monitor;
    }
    if (all.isEmpty())
        return {};
    // monitors() puts the primary display first.
    return all.first();
}

bool captureMonitor(int index, bool includeCursor, CaptureResult& out, QString& outError)
{
    const QVector<MonitorInfo> all = monitors();
    if (index < 0 || index >= all.size()) {
        outError = QStringLiteral("monitor index %1 out of range (%2 attached)")
                       .arg(index)
                       .arg(all.size());
        return false;
    }
    return grabScreenRect(all.at(index).geometry, includeCursor, out, outError);
}

bool captureRegion(const QRect& region, bool includeCursor, CaptureResult& out, QString& outError)
{
    return grabScreenRect(region, includeCursor, out, outError);
}

namespace {

/// Frame a minimized window would occupy when restored, which is what the
/// picker shows and what the capture has to use.
QRect restoredFrame(HWND hwnd)
{
    WINDOWPLACEMENT placement{};
    placement.length = sizeof(placement);
    if (!GetWindowPlacement(hwnd, &placement))
        return Win::visibleFrame(hwnd);
    const RECT& rect = placement.rcNormalPosition;
    return QRect(QPoint(rect.left, rect.top),
                 QSize(rect.right - rect.left, rect.bottom - rect.top));
}

bool captureWindowPixels(HWND hwnd,
                         const QRect& frame,
                         bool includeCursor,
                         Platform::CaptureResult& out,
                         QString& outError)
{
    // PrintWindow renders the window itself, so a covered or partly off-screen
    // window still captures correctly. It paints the full window rectangle,
    // invisible resize border included, from (0,0), so the visible frame is
    // cropped out of that afterwards.
    RECT windowRect{};
    QRect painted = frame;
    if (GetWindowRect(hwnd, &windowRect)) {
        const QRect outer(QPoint(windowRect.left, windowRect.top),
                          QSize(windowRect.right - windowRect.left,
                                windowRect.bottom - windowRect.top));
        if (outer.contains(frame))
            painted = outer;
    }
    const QRect crop(frame.topLeft() - painted.topLeft(), frame.size());

    HDC screenDc = GetDC(nullptr);
    if (screenDc) {
        HDC memoryDc = CreateCompatibleDC(screenDc);
        HBITMAP bitmap = CreateCompatibleBitmap(screenDc, painted.width(), painted.height());
        if (memoryDc && bitmap) {
            HGDIOBJ previous = SelectObject(memoryDc, bitmap);
            if (PrintWindow(hwnd, memoryDc, PW_RENDERFULLCONTENT)) {
                QImage image;
                QString ignored;
                if (imageFromBitmap(memoryDc, bitmap, painted.size(), image, ignored)
                    && !isMostlyBlack(image.copy(crop))) {
                    if (includeCursor) {
                        drawCursor(memoryDc, painted);
                        imageFromBitmap(memoryDc, bitmap, painted.size(), image, ignored);
                    }
                    out.image = image.copy(crop);
                    out.geometry = frame;
                    SelectObject(memoryDc, previous);
                    DeleteObject(bitmap);
                    DeleteDC(memoryDc);
                    ReleaseDC(nullptr, screenDc);
                    return true;
                }
            }
            SelectObject(memoryDc, previous);
        }
        if (bitmap)
            DeleteObject(bitmap);
        if (memoryDc)
            DeleteDC(memoryDc);
        ReleaseDC(nullptr, screenDc);
    }

    // Fall back to the pixels that are actually on screen.
    return grabScreenRect(frame, includeCursor, out, outError);
}

} // namespace

QRect windowFrame(quintptr id)
{
    const HWND hwnd = reinterpret_cast<HWND>(id);
    if (!hwnd || !IsWindow(hwnd) || !IsWindowVisible(hwnd))
        return {};
    return Win::visibleFrame(hwnd);
}

void activateWindow(quintptr id)
{
    const HWND hwnd = reinterpret_cast<HWND>(id);
    if (!hwnd || !IsWindow(hwnd))
        return;
    if (IsIconic(hwnd))
        ShowWindow(hwnd, SW_RESTORE);
    SetForegroundWindow(hwnd);
    BringWindowToTop(hwnd);
}

void raiseWindow(quintptr id)
{
    const HWND hwnd = reinterpret_cast<HWND>(id);
    if (!hwnd || !IsWindow(hwnd))
        return;
    SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
}

void resizeWindow(quintptr id, const QRect& frame)
{
    const HWND hwnd = reinterpret_cast<HWND>(id);
    if (!hwnd || !IsWindow(hwnd) || frame.isEmpty())
        return;

    // SetWindowPos takes the window rectangle, which on Windows 11 includes
    // an invisible resize border. Convert the visible frame the caller wants
    // into that rectangle using the window's current insets.
    RECT windowRect{};
    if (!GetWindowRect(hwnd, &windowRect))
        return;
    const QRect visible = Win::visibleFrame(hwnd);
    if (visible.isEmpty())
        return;
    const int leftInset = visible.left() - windowRect.left;
    const int topInset = visible.top() - windowRect.top;
    const int rightInset = windowRect.right - (visible.left() + visible.width());
    const int bottomInset = windowRect.bottom - (visible.top() + visible.height());

    SetWindowPos(hwnd,
                 nullptr,
                 frame.left() - leftInset,
                 frame.top() - topInset,
                 frame.width() + leftInset + rightInset,
                 frame.height() + topInset + bottomInset,
                 SWP_NOZORDER | SWP_NOACTIVATE);
}

QRect clientRect(quintptr id)
{
    const HWND hwnd = reinterpret_cast<HWND>(id);
    if (!hwnd || !IsWindow(hwnd))
        return {};

    RECT client{};
    if (!GetClientRect(hwnd, &client))
        return {};
    POINT origin{0, 0};
    if (!ClientToScreen(hwnd, &origin))
        return {};

    return QRect(QPoint(origin.x, origin.y),
                 QSize(client.right - client.left, client.bottom - client.top));
}

bool captureWindow(quintptr id, bool includeCursor, CaptureResult& out, QString& outError)
{
    const HWND hwnd = reinterpret_cast<HWND>(id);
    if (!hwnd || !IsWindow(hwnd)) {
        outError = QStringLiteral("invalid window handle");
        return false;
    }

    const bool minimized = IsIconic(hwnd) != 0;
    const QRect frame = minimized ? restoredFrame(hwnd) : Win::visibleFrame(hwnd);
    if (frame.width() <= 0 || frame.height() <= 0) {
        outError = QStringLiteral("window has no usable frame");
        return false;
    }

    // A minimized window has nothing to render, so it is restored without
    // activation for the grab and put back exactly where it was afterwards.
    WINDOWPLACEMENT saved{};
    bool restoredForCapture = false;
    if (minimized) {
        saved.length = sizeof(saved);
        if (GetWindowPlacement(hwnd, &saved)) {
            WINDOWPLACEMENT shown = saved;
            shown.showCmd = SW_SHOWNOACTIVATE;
            if (SetWindowPlacement(hwnd, &shown)) {
                restoredForCapture = true;
                // Paint once, then put it on top so the screen-grab fallback
                // sees the window instead of whatever was covering it.
                ::Sleep(120);
                SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0,
                             SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                ::Sleep(120);
            }
        }
    }

    // The restored window can sit somewhere else than its placement record
    // claimed, so re-read the real frame before grabbing. Capturing the stale
    // rect is what produces a strip of unrelated pixels.
    QRect captureFrame = frame;
    if (restoredForCapture) {
        const QRect live = Win::visibleFrame(hwnd);
        if (live.width() > 0 && live.height() > 0)
            captureFrame = live;
    }

    const bool ok = captureWindowPixels(hwnd, captureFrame, includeCursor, out, outError);

    if (restoredForCapture)
        SetWindowPlacement(hwnd, &saved);

    if (!ok && outError.isEmpty())
        outError = QStringLiteral("window did not render");
    return ok;
}

bool captureVirtualDesktop(bool includeCursor, CaptureResult& out, QString& outError)
{
    const QVector<MonitorInfo> all = monitors();
    if (all.isEmpty()) {
        outError = QStringLiteral("no displays reported by EnumDisplayMonitors");
        return false;
    }

    QRect bounds = all.first().geometry;
    for (const MonitorInfo& monitor : all)
        bounds = bounds.united(monitor.geometry);

    return grabScreenRect(bounds, includeCursor, out, outError);
}

void excludeFromCapture(quintptr nativeId)
{
    const HWND hwnd = reinterpret_cast<HWND>(nativeId);
    if (!hwnd)
        return;

    // WDA_EXCLUDEFROMCAPTURE, Windows 10 version 2004 and later. Older builds
    // return FALSE and the controls stay visible in the recording.
    constexpr DWORD kExcludeFromCapture = 0x00000011;
    SetWindowDisplayAffinity(hwnd, kExcludeFromCapture);
}

void keepTopmost(quintptr nativeId)
{
    if (!nativeId)
        return;
    SetWindowPos(reinterpret_cast<HWND>(nativeId),
                 HWND_TOPMOST,
                 0,
                 0,
                 0,
                 0,
                 SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
}

} // namespace Platform
