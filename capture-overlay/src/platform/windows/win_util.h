#pragma once

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif

#include <windows.h>

#include <dwmapi.h>

#include <QRect>

/// Small helpers shared by the Windows platform sources. Not part of the
/// public seam.
namespace Platform::Win {

/// Paint a cursor in desktop coordinates onto a captured frame.
void drawCaptureCursor(HDC target, const QRect& frame, HCURSOR cursor, const QPoint& position);

/// Window frame as the user sees it. GetWindowRect includes the invisible
/// resize border, which makes window screenshots a few pixels too large.
inline QRect visibleFrame(HWND hwnd)
{
    RECT rect{};
    if (SUCCEEDED(DwmGetWindowAttribute(
            hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &rect, sizeof(rect)))) {
        return QRect(QPoint(rect.left, rect.top),
                     QSize(rect.right - rect.left, rect.bottom - rect.top));
    }
    if (GetWindowRect(hwnd, &rect)) {
        return QRect(QPoint(rect.left, rect.top),
                     QSize(rect.right - rect.left, rect.bottom - rect.top));
    }
    return {};
}

/// Cloaked windows are alive but not drawn: other virtual desktops, and the
/// hidden shells UWP apps keep around.
inline bool isCloaked(HWND hwnd)
{
    DWORD cloaked = 0;
    if (FAILED(DwmGetWindowAttribute(
            hwnd, DWMWA_CLOAKED, &cloaked, sizeof(cloaked))))
        return false;
    return cloaked != 0;
}

} // namespace Platform::Win
