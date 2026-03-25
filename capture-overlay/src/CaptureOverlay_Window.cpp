#include "CaptureOverlay.h"
#include <QThread>
#include <QString>
#include <QList>
#include <QRect>
#include <X11/Xlib.h>
#include <X11/Xatom.h>
#undef None
#undef Bool

// ── Helpers ───────────────────────────────────────────────────────────────────

// ── Window enumeration (X11) ─────────────────────────────────────────────────

QList<CaptureOverlay::WindowInfo> CaptureOverlay::enumerateWindows() const
{
    QList<WindowInfo> result;
    Display* dpy = XOpenDisplay(nullptr);
    if (!dpy) return result;

    Window root = DefaultRootWindow(dpy);

    // Get _NET_CLIENT_LIST for proper window list (ordered, no hidden/desktop windows)
    Atom netClientList = XInternAtom(dpy, "_NET_CLIENT_LIST", true);
    if (netClientList == 0) {
        XCloseDisplay(dpy);
        return result;
    }

    Atom actualType;
    int actualFormat;
    unsigned long nItems, bytesAfter;
    unsigned char* data = nullptr;

    if (XGetWindowProperty(dpy, root, netClientList, 0, 1024, false,
                           XA_WINDOW, &actualType, &actualFormat,
                           &nItems, &bytesAfter, &data) == Success && data) {
        Window* windows = reinterpret_cast<Window*>(data);
        for (unsigned long i = 0; i < nItems; ++i) {
            Window win = windows[i];

            // Get window geometry in root coordinates
            Window child;
            int rx, ry;
            unsigned int rw, rh, bw, depth;
            if (!XGetGeometry(dpy, win, &root, &rx, &ry, &rw, &rh, &bw, &depth))
                continue;
            XTranslateCoordinates(dpy, win, DefaultRootWindow(dpy), 0, 0, &rx, &ry, &child);

            // Skip tiny or offscreen windows
            if ((int)rw < 50 || (int)rh < 50) continue;
            if (rx + (int)rw < 0 || ry + (int)rh < 0) continue;

            // Get window title via _NET_WM_NAME or WM_NAME
            QString title;
            Atom netWmName = XInternAtom(dpy, "_NET_WM_NAME", false);
            Atom utf8Str   = XInternAtom(dpy, "UTF8_STRING", false);
            unsigned char* nameProp = nullptr;
            Atom nameType; int nameFmt; unsigned long nameItems, nameAfter;
            if (XGetWindowProperty(dpy, win, netWmName, 0, 256, false, utf8Str,
                                   &nameType, &nameFmt, &nameItems, &nameAfter,
                                   &nameProp) == Success && nameProp) {
                title = QString::fromUtf8(reinterpret_cast<char*>(nameProp));
                XFree(nameProp);
            } else {
                char* wmName = nullptr;
                if (XFetchName(dpy, win, &wmName) && wmName) {
                    title = QString::fromLocal8Bit(wmName);
                    XFree(wmName);
                }
            }
            if (title.isEmpty()) title = "(Unnamed)";

            WindowInfo info;
            info.rect  = QRect(rx, ry, (int)rw, (int)rh);
            info.title = title;
            result.prepend(info); // prepend so topmost windows are first
        }
        XFree(data);
    }

    XCloseDisplay(dpy);
    return result;
}

void CaptureOverlay::enterWindowMode()
{
    exitScrollMode();
    m_windowMode    = true;
    m_fullscreenMode = false;
    m_captureIntent = CaptureIntent::Area;
    m_hasSelection  = false;
    m_hoveredWindow = -1;
    m_windows       = enumerateWindows();
    setCursor(Qt::CrossCursor);
    update();
}

void CaptureOverlay::exitWindowMode()
{
    m_windowMode    = false;
    m_hoveredWindow = -1;
    m_windows.clear();
    update();
}

