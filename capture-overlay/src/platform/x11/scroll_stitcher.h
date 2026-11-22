#pragma once

#include <QImage>
#include <QVector>

namespace Platform {

/// Pure scroll stitching: frame comparison, fixed band detection, scroll-step
/// tracking, and the final composite. No platform input, timers, or signals, so
/// the same logic runs in the capture session and in the offline tool.
class ScrollStitcher
{
public:
    enum class Method {
        None,
        Exact,
        Fuzzy,
        Skipped,
    };

    struct Join {
        int index = 0;
        Method method = Method::None;
        int overlap = 0;
        int advance = 0;
        int expectedOverlap = -1;
        double score = 0.0;
        bool forcedExpected = false;
        int headerRows = 0;
        int footerRows = 0;
        int scrollbarColumns = 0;
        int trimmedRows = 0;
    };

    ScrollStitcher();
    ~ScrollStitcher();
    ScrollStitcher(const ScrollStitcher&) = delete;
    ScrollStitcher& operator=(const ScrollStitcher&) = delete;

    void reset();

    /// Feed one settled frame. Returns true when the frame was added: the
    /// first frame, or a frame at a new scroll position.
    bool addFrame(const QImage& frame);

    /// True after two consecutive settled frames matched, i.e. scrolling
    /// stopped. At that point the last join is re-checked without forcing the
    /// previous scroll step.
    bool reachedEnd() const;

    bool hasContent() const;
    int frames() const;
    int stitchedHeight() const;
    int width() const;

    /// Header, stitched content, and footer, with the scrollbar margin removed.
    QImage result() const;

    int headerRows() const;
    int footerRows() const;
    int scrollbarColumns() const;
    const QVector<Join>& joins() const;

    /// True when two frames show the same scroll position. The comparison
    /// ignores the right margin where the scrollbar lives.
    static bool imagesSimilar(const QImage& a, const QImage& b);

private:
    class Private;
    Private* d;
};

} // namespace Platform
