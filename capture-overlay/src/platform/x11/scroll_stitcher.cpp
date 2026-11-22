#include "platform/x11/scroll_stitcher.h"

#include <QDebug>
#include <QPainter>
#include <QSize>
#include <QString>

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <limits>

namespace {

constexpr int kTemplateHeight = 200;
constexpr double kSimilarThreshold = 2.0;
constexpr double kFuzzyAcceptScore = 15.0;
// The right edge holds the scrollbar overlay and any window border. It does
// not scroll with the text, so the comparisons keep it out.
constexpr int kEdgeMargin = 24;
// Fixed bands are re-checked over this many frame pairs.
constexpr int kStablePairCount = 3;
constexpr int kAdvanceHistory = 3;
constexpr int kMinAppendRows = 2;
// Scroll views paint edge shadows over the first and last rows of their
// content, and those rows differ between frames. Matching keeps them out and
// each seam takes them from the newer frame.
constexpr int kEdgeTrimRows = 4;

// A short content band cannot spare the edge rows.
int contentEdgeTrim(int height)
{
    return height >= kEdgeTrimRows * 4 ? kEdgeTrimRows : 0;
}

struct Band {
    int left = 0;
    int span = 1;
};

Band comparisonBand(int width, int scrollbarColumns)
{
    Band band;
    band.left = std::max(0, width / 60);
    const int margin =
        std::max(width / 20, kEdgeMargin + std::max(0, scrollbarColumns));
    band.span = std::max(1, width - band.left - margin);
    return band;
}

QImage toRgba(const QImage& image)
{
    return image.format() == QImage::Format_RGBA8888
        ? image
        : image.convertToFormat(QImage::Format_RGBA8888);
}

bool rowsEqual(const QImage& a, int aRow, const QImage& b, int bRow, int scrollbarColumns)
{
    const int width = std::min(a.width(), b.width());
    if (width <= 0 || aRow < 0 || bRow < 0 || aRow >= a.height() || bRow >= b.height())
        return false;

    const Band band = comparisonBand(width, scrollbarColumns);
    if (band.left + band.span > width)
        return false;
    return std::memcmp(a.constScanLine(aRow) + band.left * 4,
                       b.constScanLine(bRow) + band.left * 4,
                       static_cast<size_t>(band.span) * 4)
        == 0;
}

double templateMatchScore(const QImage& previous,
                          int previousY,
                          int templateHeight,
                          const QImage& next,
                          int nextY,
                          int scrollbarColumns)
{
    const int width = std::min(previous.width(), next.width());
    if (width <= 0 || templateHeight <= 0)
        return std::numeric_limits<double>::max();
    if (previousY < 0 || nextY < 0 || previousY + templateHeight > previous.height()
        || nextY + templateHeight > next.height())
        return std::numeric_limits<double>::max();

    const Band band = comparisonBand(width, scrollbarColumns);
    const int stepX = std::max(1, band.span / 64);
    const int stepY = std::max(1, templateHeight / 8);

    double diff = 0.0;
    int samples = 0;
    for (int y = 0; y < templateHeight; y += stepY) {
        const uchar* lineA = previous.constScanLine(previousY + y);
        const uchar* lineB = next.constScanLine(nextY + y);
        for (int x = band.left; x < band.left + band.span; x += stepX) {
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

double overlapDiffScore(const QImage& previous, const QImage& next, int rows)
{
    const int width = std::min(previous.width(), next.width());
    const int height = std::min(previous.height(), next.height());
    rows = std::min(rows, height);
    if (width <= 0 || rows <= 0)
        return std::numeric_limits<double>::max();

    const int previousStart = previous.height() - rows;
    const Band band = comparisonBand(width, 0);
    const int stepX = std::max(1, band.span / 64);
    const int stepY = std::max(1, rows / 42);

    double diff = 0.0;
    int samples = 0;
    for (int y = 0; y < rows; y += stepY) {
        const uchar* lineA = previous.constScanLine(previousStart + y);
        const uchar* lineB = next.constScanLine(y);
        for (int x = band.left; x < band.left + band.span; x += stepX) {
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

bool framesAreSimilar(const QImage& a, const QImage& b)
{
    if (a.isNull() || b.isNull() || a.size() != b.size())
        return false;
    const QImage left = toRgba(a);
    const QImage right = toRgba(b);
    return overlapDiffScore(left, right, std::min(left.height(), right.height()))
        <= kSimilarThreshold;
}

/// Row-exact stitch used for text and other pixel-stable content. Finds the
/// current frame row that lines up with the bottom of the stitched content and
/// returns the overlap. The right margin is left out of the comparison.
int estimateOverlapRowsExact(const QImage& stitched, const QImage& next, int scrollbarColumns)
{
    if (stitched.isNull() || next.isNull())
        return -1;

    const int trim = contentEdgeTrim(std::min(stitched.height(), next.height()));
    const int limit = next.height() / 2;
    const int minimum = std::max(8, next.height() / 40);
    if (limit <= minimum)
        return -1;

    const int firstRow = trim;
    const int lastNextRow = next.height() - 1 - trim;
    const int lastStitchedRow = stitched.height() - 1 - trim;
    if (lastNextRow < firstRow || lastStitchedRow < firstRow)
        return -1;

    int bestCount = 0;
    int bestIndex = -1;

    // currentY is the full-band row of `next` that lines up with the last row
    // of `stitched`. The trim only keeps the unstable edge rows out of the
    // comparison, so the returned overlap stays in full-band coordinates.
    for (int currentY = lastNextRow; currentY >= firstRow && bestCount < limit; --currentY) {
        int count = 0;
        int stitchedY = lastStitchedRow;
        int nextY = currentY - trim;
        while (count < limit && stitchedY >= firstRow && nextY >= firstRow) {
            if (!rowsEqual(stitched, stitchedY, next, nextY, scrollbarColumns))
                break;
            ++count;
            --stitchedY;
            --nextY;
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

struct FuzzyOverlap {
    int overlap = 0;
    double score = std::numeric_limits<double>::max();
    double expectedScore = std::numeric_limits<double>::max();
    bool forced = false;
};

FuzzyOverlap estimateOverlapRowsFuzzy(const QImage& previous,
                                      const QImage& next,
                                      int expectedOverlap,
                                      int scrollbarColumns,
                                      bool allowNearEnd)
{
    FuzzyOverlap result;
    const int maxOverlap = std::min(previous.height(), next.height());
    if (maxOverlap <= 8) {
        result.overlap = 0;
        result.score = 0.0;
        return result;
    }

    const int trim = contentEdgeTrim(maxOverlap);
    const int templateHeight =
        std::max(8, std::min(kTemplateHeight, std::max(8, maxOverlap / 2 - trim)));
    // The template sits above the previous frame's bottom edge shadow, and its
    // offsets stay inside the next frame's usable rows.
    const int previousY = previous.height() - trim - templateHeight;
    const int firstOffset = trim;
    const int fullEnd =
        std::min(maxOverlap - templateHeight - trim, next.height() - trim - templateHeight);
    const int fallback = expectedOverlap >= 0 ? expectedOverlap : std::max(50, maxOverlap / 3);
    if (previousY < firstOffset || fullEnd < firstOffset) {
        result.overlap = fallback;
        result.forced = expectedOverlap >= 0;
        return result;
    }

    const auto scoreAt = [&](int offset) {
        return templateMatchScore(previous,
                                  previousY,
                                  templateHeight,
                                  next,
                                  offset,
                                  scrollbarColumns);
    };

    double bestScore = std::numeric_limits<double>::max();
    int bestOffset = firstOffset;
    const auto search = [&](int start, int end) {
        for (int offset = start; offset <= end; offset += 2) {
            const double score = scoreAt(offset);
            if (score < bestScore) {
                bestScore = score;
                bestOffset = offset;
            }
        }
        // The coarse pass steps by two rows; settle on the exact row.
        for (const int neighbour : {bestOffset - 1, bestOffset + 1}) {
            if (neighbour < start || neighbour > end)
                continue;
            const double score = scoreAt(neighbour);
            if (score < bestScore) {
                bestScore = score;
                bestOffset = neighbour;
            }
        }
    };

    if (expectedOverlap >= 0) {
        const int expectedOffset =
            std::clamp(expectedOverlap - templateHeight - trim, firstOffset, fullEnd);
        result.expectedScore = scoreAt(expectedOffset);
        // Start at the expected offset so ties keep the known step instead of
        // drifting to the first equal offset in the window.
        bestScore = result.expectedScore;
        bestOffset = expectedOffset;
        const int window = std::max(40, next.height() / 5);
        search(std::max(firstOffset, expectedOffset - window),
               std::min(fullEnd, expectedOffset + window));
        if (bestScore > kFuzzyAcceptScore) {
            bestScore = std::numeric_limits<double>::max();
            search(firstOffset, fullEnd);
        }
    } else {
        search(firstOffset, fullEnd);
    }

    if (bestScore > kFuzzyAcceptScore) {
        result.overlap = fallback;
        result.score = bestScore;
        result.forced = expectedOverlap >= 0;
        return result;
    }

    int detected = bestOffset + templateHeight + trim;
    // A short final step can leave a near-full overlap; that is valid only for
    // the relaxed search the end of the content performs.
    if (detected < 20 || (!allowNearEnd && detected > maxOverlap - 20)) {
        result.overlap = fallback;
        result.score = bestScore;
        result.forced = expectedOverlap >= 0;
        return result;
    }

    if (expectedOverlap < 0) {
        const int guess = maxOverlap / 2;
        if (std::abs(detected - guess) > maxOverlap / 3)
            detected = (detected + guess) / 2;
    }

    result.overlap = detected;
    result.score = bestScore;
    return result;
}

/// Columns at the right edge that did not scroll with the content, such as a
/// scrollbar thumb, after the central columns matched row for row.
int nonScrollingRightColumns(const QImage& stitched,
                             const QImage& next,
                             int overlap,
                             int knownColumns)
{
    const int width = std::min(stitched.width(), next.width());
    const int margin =
        std::min(width, std::max(width / 20, kEdgeMargin + std::max(0, knownColumns)));
    const int rows = std::min({overlap, stitched.height(), next.height()});
    if (width <= 0 || margin <= 0 || rows <= 0)
        return 0;

    const int trim = contentEdgeTrim(std::min(stitched.height(), next.height()));
    const int firstRow = trim;
    const int lastRow = rows - 1 - trim;
    if (lastRow < firstRow)
        return 0;

    for (int x = width - margin; x < width; ++x) {
        int differing = 0;
        for (int y = firstRow; y <= lastRow; ++y) {
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

int detectTopBand(const QVector<QImage>& frames, int height)
{
    if (frames.size() < 2)
        return 0;
    const int limit = height * 28 / 100;
    for (int y = 0; y < limit; ++y) {
        for (int i = 1; i < frames.size(); ++i) {
            if (!rowsEqual(frames.first(), y, frames.at(i), y, 0))
                return y;
        }
    }
    return limit;
}

int detectBottomBand(const QVector<QImage>& frames, int height)
{
    if (frames.size() < 2)
        return 0;
    const int limit = height * 22 / 100;
    for (int offset = 1; offset <= limit; ++offset) {
        for (int i = 1; i < frames.size(); ++i) {
            if (!rowsEqual(frames.first(),
                           frames.first().height() - offset,
                           frames.at(i),
                           frames.at(i).height() - offset,
                           0))
                return offset - 1;
        }
    }
    return limit;
}

QImage appendFrame(const QImage& base, const QImage& next, int overlap)
{
    // The last rows of `base` hold the previous frame's edge shadow. The newer
    // frame's rows replace them, so the seam keeps no stale overlay, every
    // content row stays once, and the stitched height is unchanged.
    const int trim = contentEdgeTrim(std::min(base.height(), next.height()));
    const int drop = std::min({trim, overlap, base.height()});
    const int sourceStart = overlap - drop;
    const int keptHeight = base.height() - drop;
    const int appendHeight = next.height() - sourceStart;

    QImage combined(base.width(), keptHeight + appendHeight, QImage::Format_RGBA8888);
    combined.fill(Qt::transparent);
    {
        QPainter painter(&combined);
        painter.drawImage(0, 0, base, 0, 0, base.width(), keptHeight);
        painter.drawImage(0,
                          keptHeight,
                          next,
                          0,
                          sourceStart,
                          next.width(),
                          appendHeight);
    }
    return combined;
}

} // namespace

namespace Platform {

class ScrollStitcher::Private
{
public:
    void start(const QImage& frame);
    void addMovedFrame(const QImage& frame);
    void joinFrame(const QImage& frame, bool logJoin);
    void rebuild(bool logJoins);
    void detectBands();
    void finish();
    void recheckLastJoin();
    int medianAdvance() const;
    int totalHeight() const;

    QSize frameSize;
    QVector<QImage> rawFrames;
    bool bandsFrozen = false;
    int headerRows = 0;
    int footerRows = 0;
    QImage header;
    QImage footer;
    QImage content;
    QImage previousContent;
    QImage lastFrame;
    int similar = 0;
    int frames = 0;
    bool ended = false;
    int scrollbarColumns = 0;
    QVector<int> advances;
    QVector<ScrollStitcher::Join> joins;
    QImage lastJoinBase;
    QImage lastJoinFrame;
    QImage lastJoinPrevious;
    bool lastJoinForced = false;
};

int ScrollStitcher::Private::totalHeight() const
{
    return header.height() + content.height() + footer.height();
}

int ScrollStitcher::Private::medianAdvance() const
{
    if (advances.isEmpty())
        return 0;
    QVector<int> sorted = advances;
    std::sort(sorted.begin(), sorted.end());
    return sorted.at(sorted.size() / 2);
}

void ScrollStitcher::Private::start(const QImage& frame)
{
    frameSize = frame.size();
    rawFrames = {frame};
    content = frame;
    previousContent = frame;
    lastFrame = frame;
    frames = 1;
}

void ScrollStitcher::Private::detectBands()
{
    if (rawFrames.isEmpty())
        return;

    const int height = rawFrames.first().height();
    const int top = detectTopBand(rawFrames, height);
    const int bottom = detectBottomBand(rawFrames, height);
    if (top + bottom > height - 60) {
        headerRows = 0;
        footerRows = 0;
    } else {
        headerRows = top;
        footerRows = bottom;
    }

    header = headerRows > 0
        ? rawFrames.first().copy(0, 0, rawFrames.first().width(), headerRows)
        : QImage();
    footer = footerRows > 0
        ? rawFrames.first().copy(0,
                                 rawFrames.first().height() - footerRows,
                                 rawFrames.first().width(),
                                 footerRows)
        : QImage();
}

void ScrollStitcher::Private::rebuild(bool logJoins)
{
    if (rawFrames.isEmpty())
        return;

    content = contentBand(rawFrames.first(), headerRows, footerRows);
    previousContent = content;
    scrollbarColumns = 0;
    frames = content.isNull() ? 0 : 1;
    advances.clear();
    joins.clear();
    lastJoinBase = QImage();
    lastJoinFrame = QImage();
    lastJoinPrevious = QImage();
    lastJoinForced = false;

    for (int i = 1; i < rawFrames.size(); ++i)
        joinFrame(rawFrames.at(i), logJoins);
}

void ScrollStitcher::Private::addMovedFrame(const QImage& frame)
{
    if (!bandsFrozen) {
        rawFrames.append(frame);
        detectBands();
        const bool lastPair = rawFrames.size() >= kStablePairCount + 1;
        rebuild(lastPair);
        if (lastPair)
            bandsFrozen = true;
        return;
    }
    joinFrame(frame, true);
}

void ScrollStitcher::Private::joinFrame(const QImage& frame, bool logJoin)
{
    const QImage next = contentBand(frame, headerRows, footerRows);
    if (next.isNull() || content.isNull())
        return;

    int expectedOverlap = -1;
    const int median = medianAdvance();
    if (median > 0 && next.height() > 1)
        expectedOverlap = std::clamp(next.height() - median, 0, next.height() - 1);

    Join join;
    join.index = joins.size() + 1;
    join.expectedOverlap = expectedOverlap;
    join.trimmedRows = contentEdgeTrim(next.height());

    int overlap = estimateOverlapRowsExact(content, next, scrollbarColumns);
    const bool exact = overlap >= 0;
    double score = 0.0;
    bool forced = false;

    if (!exact) {
        const QImage& previous = previousContent.isNull() ? content : previousContent;
        const FuzzyOverlap fuzzy =
            estimateOverlapRowsFuzzy(previous, next, expectedOverlap, scrollbarColumns, false);
        overlap = fuzzy.overlap;
        score = fuzzy.score;
        forced = fuzzy.forced;

        if (expectedOverlap >= 0 && !forced) {
            // Ambiguous content can match one line off. Keep the known step
            // unless the deviating match is clearly better.
            const int expectedAdvance = next.height() - expectedOverlap;
            const int actualAdvance = next.height() - overlap;
            const int tolerance = std::max(4, expectedAdvance / 4);
            if (std::abs(actualAdvance - expectedAdvance) > tolerance
                && fuzzy.score > fuzzy.expectedScore * 0.7 + 0.01) {
                overlap = expectedOverlap;
                forced = true;
            }
        }
    }

    overlap = std::max(0, std::min(overlap, next.height() - 1));
    const int appendHeight = next.height() - overlap;

    join.method = exact ? Method::Exact : Method::Fuzzy;
    join.overlap = overlap;
    join.advance = appendHeight;
    join.score = score;
    join.forcedExpected = forced;
    join.headerRows = headerRows;
    join.footerRows = footerRows;

    if (appendHeight > kMinAppendRows) {
        if (exact) {
            scrollbarColumns =
                std::max(scrollbarColumns,
                         nonScrollingRightColumns(content, next, overlap, scrollbarColumns));
        }
        lastJoinBase = content;
        lastJoinFrame = next;
        lastJoinPrevious = previousContent;
        lastJoinForced = forced;

        content = appendFrame(content, next, overlap);
        ++frames;
        advances.append(appendHeight);
        while (advances.size() > kAdvanceHistory)
            advances.removeFirst();
    } else {
        join.method = Method::Skipped;
        lastJoinForced = false;
    }

    join.scrollbarColumns = scrollbarColumns;
    joins.append(join);

    if (logJoin) {
        qInfo().noquote() << "[stitch] join" << join.index
                          << "method"
                          << (join.method == Method::Exact
                                  ? "exact"
                                  : (join.method == Method::Skipped ? "skipped" : "fuzzy"))
                          << "overlap" << overlap
                          << "advance" << appendHeight
                          << "expected" << expectedOverlap
                          << "score" << QString::number(score, 'f', 2)
                          << (forced ? "forced" : "")
                          << "header" << headerRows
                          << "footer" << footerRows
                          << "trimmed" << join.trimmedRows
                          << "scrollbar" << scrollbarColumns;
    }

    previousContent = next;
}

void ScrollStitcher::Private::recheckLastJoin()
{
    if (!lastJoinForced || lastJoinFrame.isNull() || lastJoinBase.isNull())
        return;

    const int exact = estimateOverlapRowsExact(lastJoinBase, lastJoinFrame, scrollbarColumns);
    int overlap = -1;
    double score = 0.0;
    Method method = Method::Fuzzy;
    bool used = false;

    if (exact >= 0) {
        overlap = exact;
        method = Method::Exact;
        used = true;
    } else {
        const QImage& previous = lastJoinPrevious.isNull() ? lastJoinBase : lastJoinPrevious;
        // Use the previous choice as a search hint rather than a forced value,
        // so a short final step can win on its own score.
        const int hint = joins.isEmpty() ? -1 : joins.last().overlap;
        const FuzzyOverlap fuzzy =
            estimateOverlapRowsFuzzy(previous, lastJoinFrame, hint, scrollbarColumns, true);
        if (fuzzy.score <= kFuzzyAcceptScore && fuzzy.overlap > 20) {
            overlap = fuzzy.overlap;
            score = fuzzy.score;
            method = Method::Fuzzy;
            used = true;
        }
    }

    if (!used)
        return;

    const int advance = lastJoinFrame.height() - overlap;
    if (advance <= kMinAppendRows)
        return;

    content = appendFrame(lastJoinBase, lastJoinFrame, overlap);
    if (!advances.isEmpty())
        advances.last() = advance;
    if (!joins.isEmpty()) {
        Join& join = joins.last();
        join.overlap = overlap;
        join.advance = advance;
        join.method = method;
        join.score = score;
        join.forcedExpected = false;
        join.scrollbarColumns = scrollbarColumns;
        join.trimmedRows = contentEdgeTrim(lastJoinFrame.height());
    }
    qInfo().noquote() << "[stitch] last join rechecked overlap" << overlap
                      << "advance" << advance << "method"
                      << (method == Method::Exact ? "exact" : "fuzzy")
                      << "trimmed" << contentEdgeTrim(lastJoinFrame.height());
}

void ScrollStitcher::Private::finish()
{
    if (ended)
        return;
    ended = true;
    recheckLastJoin();
}

ScrollStitcher::ScrollStitcher()
    : d(new Private())
{
}

ScrollStitcher::~ScrollStitcher()
{
    delete d;
}

void ScrollStitcher::reset()
{
    d->frameSize = QSize();
    d->rawFrames.clear();
    d->bandsFrozen = false;
    d->headerRows = 0;
    d->footerRows = 0;
    d->header = QImage();
    d->footer = QImage();
    d->content = QImage();
    d->previousContent = QImage();
    d->lastFrame = QImage();
    d->similar = 0;
    d->frames = 0;
    d->ended = false;
    d->scrollbarColumns = 0;
    d->advances.clear();
    d->joins.clear();
    d->lastJoinBase = QImage();
    d->lastJoinFrame = QImage();
    d->lastJoinPrevious = QImage();
    d->lastJoinForced = false;
}

bool ScrollStitcher::addFrame(const QImage& input)
{
    if (input.isNull())
        return false;

    const QImage frame = toRgba(input);
    if (d->content.isNull()) {
        d->start(frame);
        return true;
    }
    if (frame.size() != d->frameSize) {
        qWarning().noquote() << "[stitch] ignoring frame" << frame.size() << "expected"
                             << d->frameSize;
        return false;
    }
    if (d->ended)
        return false;

    if (framesAreSimilar(d->lastFrame, frame)) {
        ++d->similar;
        if (d->similar >= 2)
            d->finish();
        return false;
    }

    d->similar = 0;
    d->lastFrame = frame;
    d->addMovedFrame(frame);
    return true;
}

bool ScrollStitcher::reachedEnd() const
{
    return d->ended;
}

bool ScrollStitcher::hasContent() const
{
    return !d->content.isNull();
}

int ScrollStitcher::frames() const
{
    return d->frames;
}

int ScrollStitcher::stitchedHeight() const
{
    return d->totalHeight();
}

int ScrollStitcher::width() const
{
    return d->content.width();
}

QImage ScrollStitcher::result() const
{
    if (d->content.isNull())
        return {};

    QImage output(d->content.width(), d->totalHeight(), QImage::Format_RGBA8888);
    output.fill(Qt::transparent);
    {
        QPainter painter(&output);
        int y = 0;
        if (!d->header.isNull()) {
            painter.drawImage(0, y, d->header);
            y += d->header.height();
        }
        painter.drawImage(0, y, d->content);
        y += d->content.height();
        if (!d->footer.isNull())
            painter.drawImage(0, y, d->footer);
    }

    if (d->scrollbarColumns > 0 && d->scrollbarColumns < output.width())
        output = output.copy(0, 0, output.width() - d->scrollbarColumns, output.height());
    // Every pixel is painted, so the alpha channel carries nothing.
    return output.convertToFormat(QImage::Format_RGB32);
}

int ScrollStitcher::headerRows() const
{
    return d->headerRows;
}

int ScrollStitcher::footerRows() const
{
    return d->footerRows;
}

int ScrollStitcher::scrollbarColumns() const
{
    return d->scrollbarColumns;
}

const QVector<ScrollStitcher::Join>& ScrollStitcher::joins() const
{
    return d->joins;
}

bool ScrollStitcher::imagesSimilar(const QImage& a, const QImage& b)
{
    return framesAreSimilar(a, b);
}

} // namespace Platform
