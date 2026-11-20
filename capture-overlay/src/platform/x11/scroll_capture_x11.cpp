#include "platform/x11/scroll_capture_x11.h"

#include "platform/platform.h"

#include <QDebug>
#include <QPainter>
#include <QTimer>

#include <algorithm>
#include <cmath>
#include <cstring>
#include <limits>

#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/extensions/XTest.h>

#undef None
#undef Bool

// The stitching below mirrors the Windows session, so the same text, images,
// and documents produce the same output on every desktop. Only the input and
// frame-grab seams differ: XTest drives the wheel and XGetImage reads the root.
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
// A taller template makes repetitive text (same sentence, different line
// number) less likely to match one line off.
constexpr int kTemplateHeight = 200;
constexpr double kSimilarThreshold = 2.0;
constexpr double kStaticRowThreshold = 3.0;

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

// Keep the left text (line numbers and labels help disambiguate repeated
// content) and drop only the right margin, where scrollbars live.
int centralLeft(const QImage& image)
{
    return std::max(0, image.width() / 60);
}

int centralWidth(const QImage& image)
{
    const int left = centralLeft(image);
    return std::max(1, image.width() - left - image.width() / 20);
}

double rowDiff(const QImage& a, int ay, const QImage& b, int by)
{
    const int width = std::min(a.width(), b.width());
    if (width <= 0 || ay < 0 || by < 0 || ay >= a.height() || by >= b.height())
        return std::numeric_limits<double>::max();

    const int left = width / 60;
    const int span = std::max(1, width - left - width / 20);
    const int stepX = std::max(1, span / 96);

    const uchar* lineA = a.constScanLine(ay);
    const uchar* lineB = b.constScanLine(by);
    double diff = 0.0;
    int samples = 0;
    for (int x = left; x < left + span; x += stepX) {
        const uchar* pa = lineA + x * 4;
        const uchar* pb = lineB + x * 4;
        diff += std::abs(int(pa[0]) - int(pb[0]));
        diff += std::abs(int(pa[1]) - int(pb[1]));
        diff += std::abs(int(pa[2]) - int(pb[2]));
        samples += 3;
    }
    return samples > 0 ? diff / samples : std::numeric_limits<double>::max();
}

int detectTopSticky(const QImage& previous, const QImage& next)
{
    const int limit = std::min(previous.height(), next.height()) * 28 / 100;
    int rows = 0;
    for (int y = 0; y < limit; ++y) {
        if (rowDiff(previous, y, next, y) > kStaticRowThreshold)
            break;
        ++rows;
    }
    return rows;
}

int detectBottomSticky(const QImage& previous, const QImage& next)
{
    const int limit = std::min(previous.height(), next.height()) * 22 / 100;
    int rows = 0;
    for (int y = 1; y <= limit; ++y) {
        const int py = previous.height() - y;
        const int ny = next.height() - y;
        if (py < 0 || ny < 0 || rowDiff(previous, py, next, ny) > kStaticRowThreshold)
            break;
        ++rows;
    }
    return rows;
}

double templateMatchScore(const QImage& previous,
                          int previousY,
                          int templateHeight,
                          const QImage& next,
                          int nextY)
{
    const int width = std::min(previous.width(), next.width());
    const int left = width / 60;
    const int span = std::max(1, width - left - width / 20);
    const int stepX = std::max(1, span / 64);
    const int stepY = std::max(1, templateHeight / 8);

    double diff = 0.0;
    int samples = 0;
    for (int y = 0; y < templateHeight; y += stepY) {
        const uchar* lineA = previous.constScanLine(previousY + y);
        const uchar* lineB = next.constScanLine(nextY + y);
        for (int x = left; x < left + span; x += stepX) {
            const uchar* pa = lineA + x * 4;
            const uchar* pb = lineB + x * 4;
            diff += std::abs(int(pa[0]) - int(pb[0]));
            diff += std::abs(int(pa[1]) - int(pb[1]));
            diff += std::abs(int(pa[2]) - int(pb[2]));
            samples += 3;
        }
    }
    return samples > 0 ? diff / samples : std::numeric_limits<double>::max();
}

int estimateOverlapRows(const QImage& previous, const QImage& next, int expectedOverlap = -1)
{
    const int maxOverlap = std::min(previous.height(), next.height());
    if (maxOverlap <= 8)
        return 0;

    const int templateHeight = std::max(8, std::min(kTemplateHeight, maxOverlap / 2));
    const int fullEnd = std::min(maxOverlap - templateHeight, next.height() - templateHeight);
    if (fullEnd <= 0)
        return expectedOverlap >= 0 ? expectedOverlap : std::max(50, maxOverlap / 3);

    auto search = [&](int start, int end, double& bestScore, int& bestOffset) {
        bestScore = std::numeric_limits<double>::max();
        bestOffset = start;
        for (int offset = start; offset <= end; offset += 2) {
            const double score = templateMatchScore(previous,
                                                    previous.height() - templateHeight,
                                                    templateHeight,
                                                    next,
                                                    offset);
            if (score < bestScore) {
                bestScore = score;
                bestOffset = offset;
            }
        }
        // The coarse pass steps by two rows; settle on the exact row.
        for (const int neighbour : {bestOffset - 1, bestOffset + 1}) {
            if (neighbour < start || neighbour > end)
                continue;
            const double score = templateMatchScore(previous,
                                                    previous.height() - templateHeight,
                                                    templateHeight,
                                                    next,
                                                    neighbour);
            if (score < bestScore) {
                bestScore = score;
                bestOffset = neighbour;
            }
        }
    };

    double bestScore = std::numeric_limits<double>::max();
    int bestOffset = 0;

    if (expectedOverlap >= 0) {
        // A known view movement keeps the search near that offset. Repetitive
        // text can otherwise match one line off.
        const int expectedOffset =
            std::clamp(expectedOverlap - templateHeight, 0, fullEnd);
        const int window = std::max(40, next.height() / 5);
        search(std::max(0, expectedOffset - window),
               std::min(fullEnd, expectedOffset + window),
               bestScore,
               bestOffset);
        if (bestScore > 15.0)
            search(0, fullEnd, bestScore, bestOffset);
    } else {
        search(0, fullEnd, bestScore, bestOffset);
    }

    if (bestScore > 15.0) {
        return expectedOverlap >= 0 ? expectedOverlap : std::max(50, maxOverlap / 3);
    }

    const int detected = bestOffset + templateHeight;
    if (detected < 20 || detected > maxOverlap - 20) {
        return expectedOverlap >= 0 ? expectedOverlap : std::max(50, maxOverlap / 3);
    }

    if (expectedOverlap < 0) {
        const int guess = maxOverlap / 2;
        if (std::abs(detected - guess) > maxOverlap / 3)
            return (detected + guess) / 2;
    }
    return detected;
}

double overlapDiffScore(const QImage& previous, const QImage& next, int rows)
{
    const int width = std::min(previous.width(), next.width());
    const int height = std::min(previous.height(), next.height());
    rows = std::min(rows, height);
    if (width <= 0 || rows <= 0)
        return std::numeric_limits<double>::max();

    const int previousStart = previous.height() - rows;
    const int left = width / 60;
    const int span = std::max(1, width - left - width / 20);
    const int stepX = std::max(1, span / 64);
    const int stepY = std::max(1, rows / 42);

    double diff = 0.0;
    int samples = 0;
    for (int y = 0; y < rows; y += stepY) {
        const uchar* lineA = previous.constScanLine(previousStart + y);
        const uchar* lineB = next.constScanLine(y);
        for (int x = left; x < left + span; x += stepX) {
            const uchar* pa = lineA + x * 4;
            const uchar* pb = lineB + x * 4;
            diff += std::abs(int(pa[0]) - int(pb[0]));
            diff += std::abs(int(pa[1]) - int(pb[1]));
            diff += std::abs(int(pa[2]) - int(pb[2]));
            samples += 3;
        }
    }
    return samples > 0 ? diff / samples : std::numeric_limits<double>::max();
}

bool imagesSimilar(const QImage& a, const QImage& b)
{
    if (a.isNull() || b.isNull() || a.size() != b.size())
        return false;
    return overlapDiffScore(a, b, std::min(a.height(), b.height())) <= kSimilarThreshold;
}

bool rowsEqual(const QImage& a, int aRow, const QImage& b, int bRow)
{
    const int width = std::min(a.width(), b.width());
    const int left = width / 60;
    const int span = width - left - width / 20;
    if (span <= 0 || aRow < 0 || bRow < 0 || aRow >= a.height() || bRow >= b.height())
        return false;
    return std::memcmp(a.constScanLine(aRow) + left * 4,
                       b.constScanLine(bRow) + left * 4,
                       static_cast<size_t>(span) * 4)
        == 0;
}

/// Row-exact stitch used for text and other pixel-stable content. Finds the
/// current frame row that lines up with the bottom of the stitched content and
/// returns the overlap. Repetitive lines cannot match one row off because the
/// full row, including line numbers, has to be identical.
int estimateOverlapRowsExact(const QImage& stitched, const QImage& next)
{
    if (stitched.isNull() || next.isNull())
        return -1;

    const int limit = next.height() / 2;
    const int minimum = std::max(8, next.height() / 40);
    if (limit <= minimum)
        return -1;

    int bestCount = 0;
    int bestIndex = -1;

    for (int currentY = next.height() - 1; currentY >= 0 && bestCount < limit; --currentY) {
        int count = 0;
        while (count < limit && count < stitched.height() && currentY - count >= 0) {
            if (!rowsEqual(stitched, stitched.height() - 1 - count, next, currentY - count))
                break;
            ++count;
        }
        if (count > bestCount) {
            bestCount = count;
            bestIndex = currentY;
        }
    }

    if (bestIndex < 0 || bestCount < minimum)
        return -1;

    return bestIndex + 1;
}

/// Columns at the right edge that did not scroll with the content, such as a
/// scrollbar thumb, after the central columns matched row for row. Each
/// appended strip would otherwise repeat whatever that column showed.
int nonScrollingRightColumns(const QImage& stitched, const QImage& next, int overlap)
{
    const int width = std::min(stitched.width(), next.width());
    const int margin = width / 20;
    const int rows = std::min({overlap, stitched.height(), next.height()});
    if (margin <= 0 || rows <= 0)
        return 0;

    for (int x = width - margin; x < width; ++x) {
        int differing = 0;
        for (int y = 0; y < rows; ++y) {
            const uchar* a = stitched.constScanLine(stitched.height() - rows + y) + x * 4;
            const uchar* b = next.constScanLine(y) + x * 4;
            const int diff = std::abs(int(a[0]) - int(b[0])) + std::abs(int(a[1]) - int(b[1]))
                + std::abs(int(a[2]) - int(b[2]));
            if (diff > 24 && ++differing >= 2)
                return width - x;
        }
    }
    return 0;
}

QImage contentBand(const QImage& frame, int top, int bottom)
{
    const int height = frame.height() - top - bottom;
    if (height <= 0)
        return {};
    return frame.copy(0, top, frame.width(), height);
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
    void processFrame(const QImage& frame);
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
    int frames = 0;
    int similar = 0;
    bool edgesDetected = false;
    int headerRows = 0;
    int footerRows = 0;
    int scrollbarColumns = 0;
    QImage header;
    QImage content;
    QImage footer;
    QImage previousContent;
    QImage lastFrame;
    QImage pollFrame;
    int pollTries = 0;
    QPoint originalCursor;
    bool hasOriginalCursor = false;
    QSize frameSize;
};

int ScrollCaptureSession::Private::totalHeight() const
{
    return header.height() + content.height() + footer.height();
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

void ScrollCaptureSession::Private::processFrame(const QImage& frame)
{
    if (content.isNull()) {
        content = frame;
        lastFrame = frame;
        frames = 1;
        emit q->frameCaptured(frames, totalHeight());
        return;
    }

    if (!edgesDetected) {
        headerRows = detectTopSticky(lastFrame, frame);
        footerRows = detectBottomSticky(lastFrame, frame);
        if (headerRows + footerRows > frame.height() - 60) {
            headerRows = 0;
            footerRows = 0;
        }

        const int contentHeight = frame.height() - headerRows - footerRows;
        if (contentHeight <= 0) {
            content = frame;
            lastFrame = frame;
            return;
        }

        header = headerRows > 0 ? content.copy(0, 0, content.width(), headerRows) : QImage();
        footer = footerRows > 0
            ? content.copy(0, content.height() - footerRows, content.width(), footerRows)
            : QImage();
        content = content.copy(0, headerRows, content.width(), contentHeight);
        previousContent = contentBand(lastFrame, headerRows, footerRows);
        edgesDetected = true;
    }

    const QImage nextContent = contentBand(frame, headerRows, footerRows);
    if (nextContent.isNull()) {
        lastFrame = frame;
        return;
    }

    const int exactOverlap = estimateOverlapRowsExact(content, nextContent);
    int overlap = exactOverlap;
    if (overlap < 0)
        overlap = estimateOverlapRows(previousContent, nextContent);
    overlap = std::max(0, std::min(overlap, nextContent.height() - 1));
    const int appendHeight = nextContent.height() - overlap;
    if (appendHeight > 2) {
        // Only a row-exact match proves the margin difference is not content.
        if (exactOverlap > 0)
            scrollbarColumns =
                std::max(scrollbarColumns, nonScrollingRightColumns(content, nextContent, overlap));
        QImage combined(content.width(), content.height() + appendHeight,
                        QImage::Format_RGBA8888);
        combined.fill(Qt::transparent);
        {
            QPainter painter(&combined);
            painter.drawImage(0, 0, content);
            painter.drawImage(0,
                              content.height(),
                              nextContent,
                              0,
                              overlap,
                              nextContent.width(),
                              appendHeight);
        }
        content = combined;
        ++frames;
        emit q->frameCaptured(frames, totalHeight());
    }

    previousContent = nextContent;
    lastFrame = frame;
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

    content = frame;
    lastFrame = frame;
    frames = 1;

    emit q->frameCaptured(frames, totalHeight());
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
    if (!pollFrame.isNull() && imagesSimilar(pollFrame, frame)) {
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
    if (!lastFrame.isNull() && imagesSimilar(lastFrame, frame)) {
        ++similar;
        // Wheel input is the only route on X11, so two settled frames that
        // match the last captured frame mean the content has stopped moving.
        if (similar >= 2) {
            finalize(QStringLiteral("Reached the end of the content."));
            return;
        }
    } else {
        similar = 0;
        processFrame(frame);
        lastFrame = frame;
    }

    if (content.height() >= kMaxStitchedHeight || frames >= kMaxFrames) {
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

    if (content.isNull()) {
        restoreCursor();
        emit q->finished(false, QImage(), QStringLiteral("No frames were captured."));
        return;
    }

    QImage result(content.width(), totalHeight(), QImage::Format_RGBA8888);
    result.fill(Qt::transparent);
    {
        QPainter painter(&result);
        int y = 0;
        if (!header.isNull()) {
            painter.drawImage(0, y, header);
            y += header.height();
        }
        painter.drawImage(0, y, content);
        y += content.height();
        if (!footer.isNull())
            painter.drawImage(0, y, footer);
    }

    if (scrollbarColumns > 0 && scrollbarColumns < result.width())
        result = result.copy(0, 0, result.width() - scrollbarColumns, result.height());
    // Every pixel is painted, so the alpha channel carries nothing.
    result = result.convertToFormat(QImage::Format_RGB32);

    restoreCursor();
    qInfo().noquote() << "[scroll]" << reason << "frames=" << frames
                      << "scrollbar=" << scrollbarColumns
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
    d->frames = 0;
    d->similar = 0;
    d->edgesDetected = false;
    d->scrollbarColumns = 0;
    d->header = QImage();
    d->footer = QImage();
    d->content = QImage();
    d->previousContent = QImage();
    d->lastFrame = QImage();
    d->pollFrame = QImage();
    d->frameSize = QSize();

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
    return d->frames;
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

} // namespace Platform
