#pragma once

#include <QImage>
#include <QRect>
#include <QString>
#include <QVector>

/// Platform seam for the capture overlay.
///
/// Every function here has one implementation per platform, with identical
/// file names under platform/<os>/. A missing implementation is a link error
/// rather than a silent fallback.
namespace Platform {

struct MonitorInfo {
    /// Native display handle, for matching a point to a display.
    quintptr id = 0;
    /// Stable identifier for the display, as reported by the platform.
    QString name;
    /// Full display rectangle in device pixels.
    QRect geometry;
    /// Usable rectangle in device pixels, excluding taskbars and docks.
    QRect workArea;
    /// Device pixel ratio, e.g. 1.5 for a 150% scaled display.
    qreal dpr = 1.0;
    bool primary = false;
};

struct WindowInfo {
    /// Native window handle, opaque to callers.
    quintptr id = 0;
    QString title;
    QString className;
    quint32 processId = 0;
    /// Visible frame in device pixels, excluding the invisible resize border.
    QRect geometry;
    bool minimized = false;
};

/// Must run before any window is created, so the process reports true
/// device pixels and the overlay can line up with the captured image.
void enableDpiAwareness();

/// Every display attached to the session, primary first.
QVector<MonitorInfo> monitors();

/// Display containing the point, or the primary display when the point falls
/// outside every display.
MonitorInfo monitorAt(const QPoint& point);

/// Every capturable top-level window, topmost first.
QVector<WindowInfo> windows();

struct CaptureResult {
    QImage image;
    /// Rectangle the image was captured from, in device pixels.
    QRect geometry;
};

/// Grab one display by index into monitors().
bool captureMonitor(int index, bool includeCursor, CaptureResult& out, QString& outError);

/// Grab an arbitrary rectangle in device pixels, for area and crosshair
/// selections and for re-cropping a frozen background.
bool captureRegion(const QRect& region, bool includeCursor, CaptureResult& out, QString& outError);

/// Grab one window by its WindowInfo::id.
bool captureWindow(quintptr id, bool includeCursor, CaptureResult& out, QString& outError);

/// Client area of a window in device pixels, excluding its frame, menu bar,
/// and title bar. Empty when the window or its client rect is unavailable.
QRect clientRect(quintptr id);

/// Visible frame of one window in device pixels, empty when the window is
/// gone. The picker calls this while it follows a selected window.
QRect windowFrame(quintptr id);

/// Bring a window to the front before recording it, since a window recording
/// captures the screen region it occupies.
void activateWindow(quintptr id);

/// Raise a window above its neighbours without taking focus, so a covered
/// window can be previewed while a selection overlay keeps the keyboard.
void raiseWindow(quintptr id);

/// Resize a window's visible frame, keeping its top-left corner in place. The
/// caller passes the frame it wants to see, not the extended window bounds,
/// so a handle that follows visible pixels does not drift on every drag.
void resizeWindow(quintptr id, const QRect& frame);

/// Grab the bounding box of every display, for selections that span monitors.
bool captureVirtualDesktop(bool includeCursor, CaptureResult& out, QString& outError);

/// Re-assert topmost z-order for a native window handle (Qt's winId()), so a
/// floating surface stays above windows opened after it was shown.
void keepTopmost(quintptr nativeId);

/// Best-effort exclusion of a native window from screen capture, used so the
/// recorder's own controls do not appear in the video. No-op where the
/// platform cannot do it.
void excludeFromCapture(quintptr nativeId);

} // namespace Platform
