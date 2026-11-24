#include "platform/x11/scroll_capture_x11.h"

#include "platform/platform.h"
#include "platform/x11/scroll_stitcher.h"

#include <QDebug>
#include <QDir>
#include <QStringList>
#include <QTimer>

#include <algorithm>
#include <cstring>

#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/extensions/XTest.h>

#undef None
#undef Bool

// The X11 seams differ from the Windows session: XTest drives the wheel and
// XGetImage reads the root. The image comparison and stitching live in
// ScrollStitcher, which also runs offline through the stitch tool.
namespace {

/// Xlib reports protocol errors through one process-wide handler whose
/// default exits the process. A window that disappears mid-capture must end
/// the scroll, not the helper, so the session's own calls run under a handler
/// that only records the error.
bool g_xErrorSeen = false;

int recordXError(Display*, XErrorEvent*)
{
    g_xErrorSeen = true;
    return 0;
}

class XErrorTrap
{
public:
    explicit XErrorTrap(Display* display)
        : m_display(display)
    {
        g_xErrorSeen = false;
        m_previous = XSetErrorHandler(recordXError);
    }
    ~XErrorTrap()
    {
        if (m_display)
            XSync(m_display, False);
        XSetErrorHandler(m_previous);
    }
    XErrorTrap(const XErrorTrap&) = delete;
    XErrorTrap& operator=(const XErrorTrap&) = delete;

    /// Flush pending requests and report whether any of them failed.
    bool failed()
    {
        if (m_display)
            XSync(m_display, False);
        return g_xErrorSeen;
    }

private:
    Display* m_display = nullptr;
    int (*m_previous)(Display*, XErrorEvent*) = nullptr;
};


constexpr int kWheelNotches = 3;
constexpr int kFirstFrameDelayMs = 650;
// The wheel needs the cursor over the target, but a cursor left there paints
// hover highlights and tooltips into the frames. Park it once the wheel has
// been delivered and before the first settle poll.
constexpr int kParkDelayMs = 60;
// Smooth scrolling can still be animating after a wheel step. Poll until two
// captures agree instead of assuming a fixed settle time.
constexpr int kPollMs = 180;
constexpr int kMaxPollTries = 12;
constexpr int kMaxFrames = 120;
constexpr int kMaxStitchedHeight = 24000;

QImage toRgba(const QImage& image)
{
    return image.convertToFormat(QImage::Format_RGBA8888);
}

int colorChannel(unsigned long pixel, unsigned long mask)
{
    if (mask == 0)
        return 0;
    int shift = 0;
    unsigned long shifted = mask;
    while ((shifted & 1UL) == 0UL) {
        shifted >>= 1;
        ++shift;
    }
    const unsigned long maximum = mask >> shift;
    if (maximum == 0)
        return 0;
    const unsigned long value = (pixel & mask) >> shift;
    return static_cast<int>((value * 255UL + maximum / 2) / maximum);
}

/// Convert the root-window pixels to the format the stitcher reads.
QImage imageFromXImage(XImage* image)
{
    if (!image || image->width <= 0 || image->height <= 0)
        return {};

    const int width = image->width;
    const int height = image->height;
    if (image->bits_per_pixel == 32 && image->byte_order == LSBFirst
        && image->red_mask == 0xff0000UL && image->green_mask == 0xff00UL
        && image->blue_mask == 0xffUL) {
        QImage out(width, height, QImage::Format_RGB32);
        for (int y = 0; y < height; ++y) {
            std::memcpy(out.scanLine(y),
                        image->data + y * image->bytes_per_line,
                        static_cast<size_t>(width) * 4);
        }
        return out;
    }

    QImage out(width, height, QImage::Format_RGB32);
    for (int y = 0; y < height; ++y) {
        auto* line = reinterpret_cast<QRgb*>(out.scanLine(y));
        for (int x = 0; x < width; ++x) {
            const unsigned long pixel = XGetPixel(image, x, y);
            line[x] = qRgb(colorChannel(pixel, image->red_mask),
                           colorChannel(pixel, image->green_mask),
                           colorChannel(pixel, image->blue_mask));
        }
    }
    return out;
}

QPoint pointerPosition(Display* display)
{
    Window rootReturn = 0;
    Window childReturn = 0;
    int rootX = 0;
    int rootY = 0;
    int winX = 0;
    int winY = 0;
    unsigned int mask = 0;
    if (!XQueryPointer(display,
                       DefaultRootWindow(display),
                       &rootReturn,
                       &childReturn,
                       &rootX,
                       &rootY,
                       &winX,
                       &winY,
                       &mask)) {
        return QPoint();
    }
    return QPoint(rootX, rootY);
}

/// Ask the window manager to activate the target without waiting for a reply.
void requestActivation(Display* display, Window window)
{
    const Atom activeWindow = XInternAtom(display, "_NET_ACTIVE_WINDOW", False);
    if (activeWindow == 0)
        return;

    XEvent event{};
    event.xclient.type = ClientMessage;
    event.xclient.send_event = True;
    event.xclient.display = display;
    event.xclient.window = window;
    event.xclient.message_type = activeWindow;
    event.xclient.format = 32;
    // Source indication 2 means a pager or other application, not a direct
    // user action.
    event.xclient.data.l[0] = 2;
    event.xclient.data.l[1] = CurrentTime;
    event.xclient.data.l[2] = 0;
    XSendEvent(display,
               DefaultRootWindow(display),
               False,
               SubstructureRedirectMask | SubstructureNotifyMask,
               &event);
    XFlush(display);
}

/// A point the cursor can rest on between captures. The person's own cursor
/// position wins when it is already outside the region; otherwise the first
/// spot beside the region that is on the desktop.
QPoint parkingPoint(const QRect& region, const QPoint& original, const QSize& desktop)
{
    if (!region.contains(original))
        return original;
    const QPoint candidates[] = {
        QPoint(region.right() + 24, region.center().y()),
        QPoint(region.left() - 24, region.center().y()),
        QPoint(region.center().x(), region.bottom() + 24),
        QPoint(region.center().x(), region.top() - 24),
    };
    const QRect bounds(QPoint(0, 0), desktop);
    for (const QPoint& candidate : candidates) {
        if (bounds.contains(candidate))
            return candidate;
    }
    // The region covers the desktop: the bottom-right corner sits in the
    // scrollbar margin the overlap search already ignores.
    return QPoint(region.right(), region.bottom());
}

} // namespace

namespace Platform {

class ScrollCaptureSession::Private
{
public:
    explicit Private(ScrollCaptureSession* owner)
        : q(owner)
    {
    }

    void captureFirst();
    void scheduleSettle();
    void pollSettle();
    void handleSettledFrame(const QImage& frame);
    bool capture(QImage& out, QString& error);
    void advanceScroll();
    void sendWheel();
    void parkCursor();
    void dumpFrame(const QImage& frame);
    void dumpResult(const QImage& image);
    void finalize(const QString& reason);
    void fail(const QString& error);
    int totalHeight() const;
    bool openDisplay(QString& error);
    void closeDisplay();
    void restoreCursor();
    void movePointer(const QPoint& point);

    ScrollCaptureSession* q = nullptr;
    QRect region;
    Window target = 0;
    Display* display = nullptr;
    bool active = false;
    ScrollStitcher stitcher;
    QImage pollFrame;
    int pollTries = 0;
    QPoint originalCursor;
    bool hasOriginalCursor = false;
    QSize frameSize;
    QString dumpDir;
    bool dumpChecked = false;
    int dumpFrames = 0;
};

int ScrollCaptureSession::Private::totalHeight() const
{
    return stitcher.stitchedHeight();
}

bool ScrollCaptureSession::Private::openDisplay(QString& error)
{
    if (display)
        return true;

    display = XOpenDisplay(nullptr);
    if (!display) {
        error = QStringLiteral("The X11 display is not available.");
        return false;
    }

    int eventBase = 0;
    int errorBase = 0;
    int major = 0;
    int minor = 0;
    if (!XTestQueryExtension(display, &eventBase, &errorBase, &major, &minor)) {
        XCloseDisplay(display);
        display = nullptr;
        error = QStringLiteral("Synthetic input (XTest) is not available.");
        return false;
    }
    return true;
}

void ScrollCaptureSession::Private::closeDisplay()
{
    if (display) {
        XCloseDisplay(display);
        display = nullptr;
    }
}

void ScrollCaptureSession::Private::movePointer(const QPoint& point)
{
    if (!display)
        return;
    XTestFakeMotionEvent(display, DefaultScreen(display), point.x(), point.y(), CurrentTime);
    XFlush(display);
}

void ScrollCaptureSession::Private::restoreCursor()
{
    if (display && hasOriginalCursor)
        movePointer(originalCursor);
}

bool ScrollCaptureSession::Private::capture(QImage& out, QString& error)
{
    if (!display) {
        error = QStringLiteral("The X11 display is not available.");
        return false;
    }

    XErrorTrap trap(display);
    const int screen = DefaultScreen(display);
    XImage* image = XGetImage(display,
                              RootWindow(display, screen),
                              region.x(),
                              region.y(),
                              static_cast<unsigned int>(region.width()),
                              static_cast<unsigned int>(region.height()),
                              AllPlanes,
                              ZPixmap);
    if (!image || trap.failed()) {
        if (image)
            XDestroyImage(image);
        error = QStringLiteral("Could not capture the scroll region.");
        return false;
    }

    const QImage frame = imageFromXImage(image);
    XDestroyImage(image);
    if (frame.isNull()) {
        error = QStringLiteral("The scroll capture returned an empty frame.");
        return false;
    }

    out = toRgba(frame);
    if (frameSize.isEmpty())
        frameSize = out.size();
    if (out.size() != frameSize)
        out = out.copy(0, 0, frameSize.width(), frameSize.height());
    return true;
}

void ScrollCaptureSession::Private::advanceScroll()
{
    sendWheel();
}

void ScrollCaptureSession::Private::sendWheel()
{
    if (!display)
        return;

    XErrorTrap trap(display);
    const QPoint center = region.center();
    if (target != 0)
        requestActivation(display, target);
    movePointer(center);

    for (int notch = 0; notch < kWheelNotches; ++notch) {
        XTestFakeButtonEvent(display, 5, True, CurrentTime);
        XTestFakeButtonEvent(display, 5, False, CurrentTime);
    }
    XFlush(display);

    QTimer::singleShot(kParkDelayMs, q, [this] {
        if (active)
            parkCursor();
    });
}

void ScrollCaptureSession::Private::parkCursor()
{
    if (!display)
        return;
    const int screen = DefaultScreen(display);
    const QPoint point = parkingPoint(
        region,
        originalCursor,
        QSize(DisplayWidth(display, screen), DisplayHeight(display, screen)));
    movePointer(point);
}

void ScrollCaptureSession::Private::dumpFrame(const QImage& frame)
{
    if (!dumpChecked) {
        dumpChecked = true;
        dumpDir = qEnvironmentVariable("APEXSHOT_SCROLL_DUMP_DIR");
        if (!dumpDir.isEmpty()) {
            QDir dir(dumpDir);
            if (!dir.exists() && !QDir().mkpath(dumpDir)) {
                qWarning().noquote() << "[scroll] could not create dump directory" << dumpDir;
                dumpDir.clear();
            } else {
                // A dump directory belongs to one capture, so an earlier run's
                // frames must not leak into this one.
                const QStringList stale =
                    dir.entryList(QStringList{QStringLiteral("frame_*.png"),
                                              QStringLiteral("result.png")},
                                  QDir::Files);
                for (const QString& name : stale)
                    dir.remove(name);
            }
        }
    }
    if (dumpDir.isEmpty())
        return;

    const QString path = QDir(dumpDir).filePath(
        QStringLiteral("frame_%1.png").arg(dumpFrames++, 4, 10, QLatin1Char('0')));
    if (!frame.save(path, "PNG"))
        qWarning().noquote() << "[scroll] could not write" << path;
}

void ScrollCaptureSession::Private::dumpResult(const QImage& image)
{
    if (dumpDir.isEmpty())
        return;
    const QString path = QDir(dumpDir).filePath(QStringLiteral("result.png"));
    if (!image.save(path, "PNG"))
        qWarning().noquote() << "[scroll] could not write" << path;
}

void ScrollCaptureSession::Private::captureFirst()
{
    if (!active)
        return;

    QImage frame;
    QString error;
    if (!capture(frame, error)) {
        fail(error);
        return;
    }

    dumpFrame(frame);
    stitcher.addFrame(frame);

    emit q->frameCaptured(stitcher.frames(), totalHeight());
    emit q->statusChanged(QStringLiteral("Scrolling..."));

    advanceScroll();
    scheduleSettle();
}

void ScrollCaptureSession::Private::scheduleSettle()
{
    pollFrame = QImage();
    pollTries = 0;
    QTimer::singleShot(kPollMs, q, [this] { pollSettle(); });
}

void ScrollCaptureSession::Private::pollSettle()
{
    if (!active)
        return;

    QImage frame;
    QString error;
    if (!capture(frame, error)) {
        fail(error);
        return;
    }

    // Two matching captures mean the smooth-scroll animation has come to rest.
    if (!pollFrame.isNull() && ScrollStitcher::imagesSimilar(pollFrame, frame)) {
        pollFrame = QImage();
        handleSettledFrame(frame);
        return;
    }

    pollFrame = frame;
    if (++pollTries >= kMaxPollTries) {
        pollFrame = QImage();
        handleSettledFrame(frame);
        return;
    }

    QTimer::singleShot(kPollMs, q, [this] { pollSettle(); });
}

void ScrollCaptureSession::Private::handleSettledFrame(const QImage& frame)
{
    dumpFrame(frame);
    const bool changed = stitcher.addFrame(frame);
    if (changed)
        emit q->frameCaptured(stitcher.frames(), totalHeight());

    // Two settled frames that match the last processed frame mean the
    // content has stopped moving.
    if (stitcher.reachedEnd()) {
        finalize(QStringLiteral("Reached the end of the content."));
        return;
    }

    if (stitcher.stitchedHeight() >= kMaxStitchedHeight || stitcher.frames() >= kMaxFrames) {
        finalize(QStringLiteral("Reached the capture limit."));
        return;
    }

    advanceScroll();
    scheduleSettle();
}

void ScrollCaptureSession::Private::finalize(const QString& reason)
{
    if (!active)
        return;
    active = false;

    if (!stitcher.hasContent()) {
        restoreCursor();
        emit q->finished(false, QImage(), QStringLiteral("No frames were captured."));
        return;
    }

    const QImage result = stitcher.result();
    if (result.isNull()) {
        restoreCursor();
        emit q->finished(false, QImage(), QStringLiteral("No frames were captured."));
        return;
    }

    dumpResult(result);
    restoreCursor();
    qInfo().noquote() << "[scroll]" << reason << "frames=" << stitcher.frames()
                      << "scrollbar=" << stitcher.scrollbarColumns()
                      << "size=" << result.width() << "x" << result.height();
    emit q->statusChanged(reason);
    emit q->finished(true, result, QString());
}

void ScrollCaptureSession::Private::fail(const QString& error)
{
    if (!active)
        return;
    active = false;
    restoreCursor();
    qWarning().noquote() << "[scroll]" << error;
    emit q->finished(false, QImage(), error);
}

ScrollCaptureSession::ScrollCaptureSession(QObject* parent)
    : QObject(parent)
    , d(new Private(this))
{
}

ScrollCaptureSession::~ScrollCaptureSession()
{
    if (d->active)
        d->restoreCursor();
    d->active = false;
    d->closeDisplay();
    delete d;
}

bool ScrollCaptureSession::available(QString* error)
{
    if (error)
        error->clear();

    Display* display = XOpenDisplay(nullptr);
    if (!display) {
        if (error)
            *error = QStringLiteral("The X11 display is not available.");
        return false;
    }

    int eventBase = 0;
    int errorBase = 0;
    int major = 0;
    int minor = 0;
    const bool hasXTest =
        XTestQueryExtension(display, &eventBase, &errorBase, &major, &minor) != 0;
    XCloseDisplay(display);
    if (!hasXTest) {
        if (error)
            *error = QStringLiteral("Synthetic input (XTest) is not available.");
        return false;
    }
    return true;
}

bool ScrollCaptureSession::start(const QRect& region, quintptr windowId, QString& error)
{
    if (d->active) {
        error = QStringLiteral("A scroll capture is already running.");
        return false;
    }
    if (region.width() < 32 || region.height() < 32) {
        error = QStringLiteral("The scroll capture region is too small.");
        return false;
    }
    if (!d->openDisplay(error))
        return false;

    d->region = region;
    d->target = static_cast<Window>(windowId);
    d->originalCursor = pointerPosition(d->display);
    d->hasOriginalCursor = true;

    if (d->target != 0)
        requestActivation(d->display, d->target);

    // Move to the scroll point so the first wheel reaches the target, then
    // park before the first frame is grabbed.
    d->movePointer(region.center());
    d->parkCursor();

    d->active = true;
    d->stitcher.reset();
    d->pollFrame = QImage();
    d->pollTries = 0;
    d->frameSize = QSize();
    d->dumpDir.clear();
    d->dumpChecked = false;
    d->dumpFrames = 0;

    emit statusChanged(QStringLiteral("Starting..."));
    QTimer::singleShot(kFirstFrameDelayMs, this, [this] {
        d->captureFirst();
    });
    return true;
}

void ScrollCaptureSession::stop()
{
    if (d->active)
        d->finalize(QStringLiteral("Finishing..."));
}

void ScrollCaptureSession::discard()
{
    if (d->active) {
        d->active = false;
        d->restoreCursor();
        emit finished(false, QImage(), QString());
    }
}

bool ScrollCaptureSession::isActive() const
{
    return d->active;
}

int ScrollCaptureSession::frameCount() const
{
    return d->stitcher.frames();
}

QSize ScrollCaptureSession::captureSize() const
{
    return d->region.size();
}

// The control bar asks the platform seam to keep it above other windows and
// out of captures. Qt's window flags already keep it above, and the X11 root
// grab includes every surface, so there is nothing extra to do here.
void keepTopmost(quintptr nativeId)
{
    Q_UNUSED(nativeId);
}

void excludeFromCapture(quintptr nativeId)
{
    Q_UNUSED(nativeId);
}

void includeInCapture(quintptr nativeId)
{
    Q_UNUSED(nativeId);
}

} // namespace Platform
