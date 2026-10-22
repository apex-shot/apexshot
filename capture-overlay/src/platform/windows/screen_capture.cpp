#include "platform/windows/win_util.h"

#include "platform/platform.h"

#include <QVector>

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

    ICONINFO icon{};
    if (!GetIconInfo(cursor.hCursor, &icon))
        return;

    const int x = cursor.ptScreenPos.x - rect.x() - static_cast<int>(icon.xHotspot);
    const int y = cursor.ptScreenPos.y - rect.y() - static_cast<int>(icon.yHotspot);
    DrawIconEx(target, x, y, cursor.hCursor, 0, 0, 0, nullptr, DI_NORMAL);

    if (icon.hbmMask)
        DeleteObject(icon.hbmMask);
    if (icon.hbmColor)
        DeleteObject(icon.hbmColor);
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

bool grabScreenRect(const QRect& rect,
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

    // CAPTUREBLT includes layered windows, which is what modern apps use.
    if (!BitBlt(memoryDc,
                0,
                0,
                rect.width(),
                rect.height(),
                screenDc,
                rect.x(),
                rect.y(),
                SRCCOPY | CAPTUREBLT)) {
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
    // window still captures correctly.
    HDC screenDc = GetDC(nullptr);
    if (screenDc) {
        HDC memoryDc = CreateCompatibleDC(screenDc);
        HBITMAP bitmap = CreateCompatibleBitmap(screenDc, frame.width(), frame.height());
        if (memoryDc && bitmap) {
            HGDIOBJ previous = SelectObject(memoryDc, bitmap);
            if (PrintWindow(hwnd, memoryDc, PW_RENDERFULLCONTENT)) {
                QImage image;
                QString ignored;
                if (imageFromBitmap(memoryDc, bitmap, frame.size(), image, ignored)
                    && !isMostlyBlack(image)) {
                    out.image = image;
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
{    const QVector<MonitorInfo> all = monitors();
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
