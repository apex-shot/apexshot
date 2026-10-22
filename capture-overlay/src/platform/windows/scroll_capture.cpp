#include "platform/windows/scroll_capture.h"

#include "platform/platform.h"
#include "platform/windows/win_util.h"

// objbase supplies the `interface` macro that the MIDL-generated UIA headers
// expect; WIN32_LEAN_AND_MEAN keeps it out of windows.h.
#include <objbase.h>
#include <UIAutomation.h>
#include <oleauto.h>

#include <QDebug>
#include <QDir>
#include <QFile>
#include <QPainter>
#include <QTimer>

#include <algorithm>
#include <cmath>
#include <cstring>
#include <limits>

namespace {

constexpr int kWheelNotches = 3;
constexpr int kFirstFrameDelayMs = 650;
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

void scrollLog(const QString& text)
{
    QFile log(QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-scroll.log")));
    if (log.open(QIODevice::Append | QIODevice::WriteOnly)) {
        log.write(text.toUtf8());
        log.write("\n");
    }
}

QImage toRgba(const QImage& image)
{
    return image.convertToFormat(QImage::Format_RGBA8888);
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
    };

    double bestScore = std::numeric_limits<double>::max();
    int bestOffset = 0;

    if (expectedOverlap >= 0) {
        // UIA tells us how far the view should have moved, so keep the search
        // near that offset. Repetitive text can otherwise match one line off.
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

QImage contentBand(const QImage& frame, int top, int bottom)
{
    const int height = frame.height() - top - bottom;
    if (height <= 0)
        return {};
    return frame.copy(0, top, frame.width(), height);
}

bool forceForeground(HWND hwnd)
{
    if (!hwnd || GetForegroundWindow() == hwnd)
        return true;

    const HWND foreground = GetForegroundWindow();
    const DWORD foregroundThread =
        foreground ? GetWindowThreadProcessId(foreground, nullptr) : 0;
    const DWORD targetThread = GetWindowThreadProcessId(hwnd, nullptr);

    bool ok = false;
    if (foregroundThread && targetThread && foregroundThread != targetThread) {
        AttachThreadInput(foregroundThread, targetThread, TRUE);
        ok = SetForegroundWindow(hwnd) != FALSE;
        SetFocus(hwnd);
        AttachThreadInput(foregroundThread, targetThread, FALSE);
    } else {
        ok = SetForegroundWindow(hwnd) != FALSE;
        SetFocus(hwnd);
    }
    return ok;
}

QString windowClass(HWND hwnd)
{
    wchar_t name[128]{};
    if (hwnd && GetClassNameW(hwnd, name, 128) > 0)
        return QString::fromWCharArray(name);
    return QStringLiteral("?");
}

HWND resolveTarget(quintptr windowId, const QPoint& point)
{
    if (windowId != 0) {
        HWND hwnd = reinterpret_cast<HWND>(windowId);
        if (IsWindow(hwnd))
            return hwnd;
    }

    POINT pt{point.x(), point.y()};
    HWND hwnd = WindowFromPoint(pt);
    if (hwnd)
        hwnd = GetAncestor(hwnd, GA_ROOT);
    return hwnd;
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
    void processFrame(const QImage& frame);

    bool initUia();
    bool findScrollPattern();
    void releaseUia();
    bool readUiaPercent(double& percent, double& viewSize) const;
    bool advanceUia();
    void finalize(const QString& reason);
    void fail(const QString& error);
    int totalHeight() const;

    ScrollCaptureSession* q = nullptr;
    QRect region;
    HWND target = nullptr;
    bool active = false;
    bool discardRequested = false;
    int frames = 0;
    int similar = 0;
    bool edgesDetected = false;
    int headerRows = 0;
    int footerRows = 0;
    QImage header;
    QImage content;
    QImage footer;
    QImage previousContent;
    QImage lastFrame;
    QImage pollFrame;
    int pollTries = 0;
    bool usePostedWheel = false;
    POINT originalCursor{};
    QSize frameSize;

    IUIAutomation* automation = nullptr;
    IUIAutomationScrollPattern* scrollPattern = nullptr;
    IUIAutomationElement* scrollElement = nullptr;
    bool useUia = false;
    bool comInitialized = false;
    double lastSettledPercent = -1;
    double pendingPercentDelta = -1;
    double pendingViewSize = -1;
};

int ScrollCaptureSession::Private::totalHeight() const
{
    return header.height() + content.height() + footer.height();
}

bool ScrollCaptureSession::Private::capture(QImage& out, QString& error)
{
    Platform::CaptureResult capture;
    QString captureError;
    if (!Platform::captureRegion(region, false, capture, captureError)) {
        error = captureError.isEmpty() ? QStringLiteral("Could not capture the scroll region.")
                                       : captureError;
        return false;
    }
    if (capture.image.isNull()) {
        error = QStringLiteral("The scroll capture returned an empty frame.");
        return false;
    }

    out = toRgba(capture.image);
    if (frameSize.isEmpty())
        frameSize = out.size();
    if (out.size() != frameSize)
        out = out.copy(0, 0, frameSize.width(), frameSize.height());
    return true;
}

void ScrollCaptureSession::Private::advanceScroll()
{
    if (useUia && advanceUia())
        return;
    if (useUia) {
        useUia = false;
        pendingPercentDelta = -1;
        pendingViewSize = -1;
        scrollLog(QStringLiteral("uia: falling back to wheel input"));
    }
    sendWheel();
}

void ScrollCaptureSession::Private::sendWheel()
{
    if (!target)
        return;

    const QPoint center = region.center();
    forceForeground(target);
    SetCursorPos(center.x(), center.y());

    if (usePostedWheel) {
        // Fallback for windows that ignore synthetic mouse input.
        POINT pt{center.x(), center.y()};
        HWND receiver = WindowFromPoint(pt);
        if (!receiver)
            receiver = target;

        const WPARAM wheel = MAKEWPARAM(0, static_cast<WORD>(-WHEEL_DELTA * kWheelNotches));
        const LPARAM position = MAKELPARAM(center.x(), center.y());
        PostMessageW(receiver, WM_MOUSEWHEEL, wheel, position);
        return;
    }

    INPUT input{};
    input.type = INPUT_MOUSE;
    input.mi.dwFlags = MOUSEEVENTF_WHEEL;
    input.mi.mouseData = static_cast<DWORD>(-WHEEL_DELTA * kWheelNotches);
    SendInput(1, &input, sizeof(INPUT));
}

bool ScrollCaptureSession::Private::initUia()
{
    releaseUia();

    const HRESULT init = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    comInitialized = SUCCEEDED(init);
    if (FAILED(init) && init != RPC_E_CHANGED_MODE) {
        scrollLog(QStringLiteral("uia: CoInitializeEx failed 0x%1")
                      .arg(static_cast<quintptr>(init), 0, 16));
        return false;
    }

    const HRESULT created = CoCreateInstance(CLSID_CUIAutomation,
                                             nullptr,
                                             CLSCTX_INPROC_SERVER,
                                             IID_PPV_ARGS(&automation));
    if (FAILED(created) || !automation) {
        automation = nullptr;
        scrollLog(QStringLiteral("uia: CoCreateInstance failed 0x%1")
                      .arg(static_cast<quintptr>(created), 0, 16));
        return false;
    }
    return true;
}

bool ScrollCaptureSession::Private::findScrollPattern()
{
    if (!automation)
        return false;

    IUIAutomationTreeWalker* walker = nullptr;
    automation->get_ControlViewWalker(&walker);

    IUIAutomationElement* element = nullptr;
    POINT point{region.center().x(), region.center().y()};
    if (FAILED(automation->ElementFromPoint(point, &element)) || !element) {
        scrollLog(QStringLiteral("uia: ElementFromPoint failed"));
        if (FAILED(automation->ElementFromHandle(target, &element)) || !element)
            element = nullptr;
    }

    IUIAutomationElement* current = element;
    for (int depth = 0; current && depth < 16; ++depth) {
        IUIAutomationScrollPattern* pattern = nullptr;
        if (SUCCEEDED(current->GetCurrentPatternAs(UIA_ScrollPatternId,
                                                  IID_PPV_ARGS(&pattern)))
            && pattern) {
            BOOL scrollable = FALSE;
            if (SUCCEEDED(pattern->get_CurrentVerticallyScrollable(&scrollable)) && scrollable) {
                scrollPattern = pattern;
                scrollElement = current;
                current = nullptr;
                break;
            }
            pattern->Release();
        }

        IUIAutomationElement* parent = nullptr;
        if (!walker || FAILED(walker->GetParentElement(current, &parent)) || !parent)
            break;
        current->Release();
        current = parent;
    }

    if (current)
        current->Release();
    if (walker)
        walker->Release();

    // Some apps expose the scrollable document below the element at the point
    // but not on its ancestor chain. Search the target's descendants when the
    // walk turns up nothing.
    if (!scrollPattern && target) {
        IUIAutomationElement* root = nullptr;
        if (SUCCEEDED(automation->ElementFromHandle(target, &root)) && root) {
            VARIANT value;
            VariantInit(&value);
            value.vt = VT_BOOL;
            value.boolVal = VARIANT_TRUE;
            IUIAutomationCondition* condition = nullptr;
            if (SUCCEEDED(automation->CreatePropertyCondition(
                    UIA_IsScrollPatternAvailablePropertyId, value, &condition))
                && condition) {
                IUIAutomationElement* found = nullptr;
                if (SUCCEEDED(root->FindFirst(TreeScope_Descendants, condition, &found))
                    && found) {
                    IUIAutomationScrollPattern* pattern = nullptr;
                    if (SUCCEEDED(found->GetCurrentPatternAs(UIA_ScrollPatternId,
                                                             IID_PPV_ARGS(&pattern)))
                        && pattern) {
                        BOOL scrollable = FALSE;
                        if (SUCCEEDED(pattern->get_CurrentVerticallyScrollable(&scrollable))
                            && scrollable) {
                            scrollPattern = pattern;
                            scrollElement = found;
                            found = nullptr;
                        } else {
                            pattern->Release();
                        }
                    }
                    if (found)
                        found->Release();
                }
                condition->Release();
            }
            root->Release();
        }
    }

    useUia = scrollPattern != nullptr;
    if (scrollElement) {
        BSTR name = nullptr;
        CONTROLTYPEID controlType = 0;
        scrollElement->get_CurrentName(&name);
        scrollElement->get_CurrentControlType(&controlType);
        scrollLog(QStringLiteral("uia: found type=%1 name='%2'")
                      .arg(controlType)
                      .arg(QString::fromWCharArray(name ? name : L"")));
        if (name)
            SysFreeString(name);
    } else {
        scrollLog(QStringLiteral("uia: no scroll pattern; using wheel input"));
    }
    return useUia;
}

void ScrollCaptureSession::Private::releaseUia()
{
    if (scrollPattern) {
        scrollPattern->Release();
        scrollPattern = nullptr;
    }
    if (scrollElement) {
        scrollElement->Release();
        scrollElement = nullptr;
    }
    if (automation) {
        automation->Release();
        automation = nullptr;
    }
    if (comInitialized) {
        CoUninitialize();
        comInitialized = false;
    }
    useUia = false;
    lastSettledPercent = -1;
    pendingPercentDelta = -1;
    pendingViewSize = -1;
}

bool ScrollCaptureSession::Private::readUiaPercent(double& percent, double& viewSize) const
{
    if (!scrollPattern)
        return false;

    double currentPercent = 0;
    double currentView = 0;
    if (FAILED(scrollPattern->get_CurrentVerticalScrollPercent(&currentPercent))
        || currentPercent < 0)
        return false;
    if (FAILED(scrollPattern->get_CurrentVerticalViewSize(&currentView)) || currentView <= 0)
        return false;

    percent = currentPercent;
    viewSize = currentView;
    return true;
}

bool ScrollCaptureSession::Private::advanceUia()
{
    double percent = 0;
    double viewSize = 0;
    if (!readUiaPercent(percent, viewSize))
        return false;

    if (percent >= 99.5)
        return true;

    // Half a viewport keeps roughly fifty percent overlap between frames, and
    // the twelve percent cap keeps very tall views from crawling.
    const double delta = std::min(12.0, std::max(0.5, viewSize * 0.5));
    const double target = std::min(100.0, percent + delta);

    const HRESULT scrolled =
        scrollPattern->SetScrollPercent(UIA_ScrollPatternNoScroll, target);
    scrollLog(QStringLiteral("uia: %1% -> %2% (view %3%) hr=0x%4")
                  .arg(percent, 0, 'f', 2)
                  .arg(target, 0, 'f', 2)
                  .arg(viewSize, 0, 'f', 2)
                  .arg(static_cast<quintptr>(scrolled), 0, 16));
    if (SUCCEEDED(scrolled)) {
        pendingPercentDelta = target - percent;
        pendingViewSize = viewSize;
    }
    return SUCCEEDED(scrolled);
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

    int expectedOverlap = -1;
    if (pendingPercentDelta > 0 && pendingViewSize > 0) {
        const double scrollPx =
            nextContent.height() * pendingPercentDelta / pendingViewSize;
        expectedOverlap = qRound(nextContent.height() - scrollPx);
        expectedOverlap =
            std::clamp(expectedOverlap, 16, nextContent.height() - 16);
        pendingPercentDelta = -1;
        pendingViewSize = -1;
    }

    int overlap = estimateOverlapRowsExact(content, nextContent);
    if (overlap < 0)
        overlap = estimateOverlapRows(previousContent, nextContent, expectedOverlap);
    overlap = std::max(0, std::min(overlap, nextContent.height() - 1));
    const int appendHeight = nextContent.height() - overlap;
    if (appendHeight > 2) {
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

    double percent = 0;
    double viewSize = 0;
    if (useUia && readUiaPercent(percent, viewSize))
        lastSettledPercent = percent;

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
    if (useUia) {
        double percent = 0;
        double viewSize = 0;
        if (!readUiaPercent(percent, viewSize)) {
            useUia = false;
            scrollLog(QStringLiteral("uia: scroll pattern became unavailable; wheel fallback"));
        } else {
            const bool moved =
                lastSettledPercent < 0 || percent > lastSettledPercent + 0.01;
            lastSettledPercent = percent;

            if (moved) {
                similar = 0;
                if (!lastFrame.isNull() && !imagesSimilar(lastFrame, frame)) {
                    processFrame(frame);
                    lastFrame = frame;
                }
            } else {
                ++similar;
                if (similar >= 2) {
                    // Some providers expose a pattern but ignore percent
                    // changes. Give real wheel input one chance before ending.
                    useUia = false;
                    pendingPercentDelta = -1;
                    pendingViewSize = -1;
                    similar = 0;
                    scrollLog(QStringLiteral(
                        "uia: no movement; trying wheel input"));
                    advanceScroll();
                    scheduleSettle();
                    return;
                }
            }

            if (percent >= 99.5) {
                if (!lastFrame.isNull() && !imagesSimilar(lastFrame, frame))
                    processFrame(frame);
                finalize(QStringLiteral("Reached the end of the content."));
                return;
            }

            if (content.height() >= kMaxStitchedHeight || frames >= kMaxFrames) {
                finalize(QStringLiteral("Reached the capture limit."));
                return;
            }

            advanceScroll();
            scheduleSettle();
            return;
        }
    }

    if (!lastFrame.isNull() && imagesSimilar(lastFrame, frame)) {
        ++similar;
        if (!usePostedWheel && similar >= 2) {
            // Real input stopped moving the window. Some windows only accept
            // a posted wheel message (or vice versa); try the other route
            // before declaring the end.
            usePostedWheel = true;
            similar = 0;
            scrollLog(QStringLiteral("switching to posted wheel messages"));
        } else if (similar >= 3) {
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
    releaseUia();

    if (content.isNull()) {
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

    SetCursorPos(originalCursor.x, originalCursor.y);
    qInfo().noquote() << "[scroll]" << reason << "frames=" << frames
                      << "size=" << result.width() << "x" << result.height();
    emit q->statusChanged(reason);
    emit q->finished(true, result, QString());
}

void ScrollCaptureSession::Private::fail(const QString& error)
{
    if (!active)
        return;
    active = false;
    releaseUia();
    SetCursorPos(originalCursor.x, originalCursor.y);
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
    d->active = false;
    d->releaseUia();
    delete d;
}

bool ScrollCaptureSession::available(QString* error)
{
    if (error)
        error->clear();
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

    d->region = region;
    d->target = resolveTarget(windowId, region.center());
    if (!d->target) {
        error = QStringLiteral("Could not find a window to scroll.");
        return false;
    }

    if (IsIconic(d->target))
        ShowWindow(d->target, SW_RESTORE);
    GetCursorPos(&d->originalCursor);
    forceForeground(d->target);
    SetCursorPos(region.center().x(), region.center().y());

    // Resolve the scrollable element before any ApexShot surface is shown, so
    // ElementFromPoint reaches the target window rather than our mask.
    if (d->initUia())
        d->findScrollPattern();

    d->active = true;
    d->discardRequested = false;
    d->usePostedWheel = false;
    d->frames = 0;
    d->similar = 0;
    d->edgesDetected = false;
    d->header = QImage();
    d->footer = QImage();
    d->content = QImage();
    d->previousContent = QImage();
    d->lastFrame = QImage();
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
        d->discardRequested = true;
        d->releaseUia();
        SetCursorPos(d->originalCursor.x, d->originalCursor.y);
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

} // namespace Platform
