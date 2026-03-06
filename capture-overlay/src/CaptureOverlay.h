// SPDX-License-Identifier: GPL-3.0-or-later
// CleanShotX — Qt5 full-screen area selector overlay
//
// Full custom UI matching overlay.rs:
//   • Frosted-glass toolbar with 8 icons + hover states + size panel
//   • L-shaped resize markers at corners + edge midpoints
//   • Full drag / move / resize logic
//   • ESC to cancel, Enter/Space/double-click to confirm

#pragma once

#include <QWidget>
#include <QPixmap>
#include <QImage>
#include <QPoint>
#include <QRect>
#include <QSize>
#include <QList>
#include <QString>
#include <QPushButton>
#include <QLabel>
#include <QHBoxLayout>
#include <QVBoxLayout>

class QTimer;

// ── Small floating control panel shown during scroll capture ──────────────────
// Stays visible while the main overlay is hidden, giving the user Cancel/Done
// buttons and a frame counter.
class ScrollControlPanel : public QWidget
{
    Q_OBJECT

public:
    explicit ScrollControlPanel(QWidget* parent = nullptr);

    void setFrameCount(int count);
    void setStatus(const QString& text);
    void setCapturingDone();   // switch from "Capturing…" to "Capture complete"
    void positionNear(const QRect& captureArea, const QSize& screenSize);

protected:
    void paintEvent(QPaintEvent* event) override;

signals:
    void cancelClicked();
    void doneClicked();

private:
    QLabel*      m_statusLabel;
    QLabel*      m_frameLabel;
    QPushButton* m_cancelBtn;
    QPushButton* m_doneBtn;
};

class CaptureOverlay : public QWidget
{
    Q_OBJECT

public:
    enum class HandlePos {
        None,
        TopLeft, Top, TopRight,
        Right, BottomRight, Bottom,
        BottomLeft, Left,
        Inside
    };

    explicit CaptureOverlay(const QPixmap& background = QPixmap(),
                             QWidget* parent = nullptr);

    /// Returns the selected rectangle in screen (logical pixel) coordinates.
    /// Only valid when QApplication exits with code 0.
    QRect selection() const { return m_selection.normalized(); }
    bool ocrRequested() const { return m_captureIntent == CaptureIntent::Ocr; }
    bool scrollCaptureCompleted() const { return m_scrollCaptureReady; }
    QString scrollCapturePath() const { return m_scrollCapturePath; }
    QSize scrollCaptureSize() const { return m_scrollCaptureSize; }

protected:
    void paintEvent(QPaintEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;

private:
    enum class CaptureIntent {
        Area,
        Ocr,
        Scroll,
    };

    enum class ScrollStage {
        Inactive,
        Armed,
        Capturing,
    };

    // Drawing
    void drawToolbar(QPainter& p,
                     double selX, double selY,
                     double selW, double selH,
                     double screenW, double screenH);
    QRectF scrollPrimaryButtonRect() const;

    // Hit testing / cursor
    void updateCursor(const QPoint& pos);
    HandlePos hitTest(const QPoint& pos) const;
    QRect handleRect(const QPoint& center) const;
    QList<QPoint> handleCenters() const;

    void enterScrollMode();
    void exitScrollMode(bool keepIntent = false);
    bool handleScrollButtonClick(const QPoint& pos);

    // ── Auto-scroll capture methods ────────────────────────────────────────
    void startAutoScrollCapture();
    void onAutoScrollTick();
    bool captureScrollFrameSilent();        // capture without hiding/showing overlay
    void simulateScrollDown();
    void stopAutoScrollCapture(bool finalize);
    bool finalizeScrollCapture();
    bool stitchScrollFrames(QString& outPath, QSize& outSize, QString& outError) const;
    static bool imagesSimilar(const QImage& a, const QImage& b, double threshold);
    static double getImageDiff(const QImage& a, const QImage& b);
    static double computeTemplateMatchScore(const QImage& prev, int prevY, int templateHeight,
                                           const QImage& next, int nextY, int width);
    static int estimateOverlapRows(const QImage& prev, const QImage& next);
    static double overlapDiffScore(const QImage& prev, const QImage& next, int overlapRows);

    void confirmSelection();
    void cancelSelection();

    struct WindowInfo {
        QRect   rect;
        QString title;
    };

    void enterWindowMode();
    void exitWindowMode();
    QList<WindowInfo> enumerateWindows() const;

    // ── State ──────────────────────────────────────────────────────────────
    QPixmap   m_background;
    QImage    m_blurredBg;          // 1/4-res blurred bg for frosted glass
    QRect     m_selection;
    bool      m_hasSelection;
    bool      m_dragging;
    bool      m_moving;
    HandlePos m_resizing;
    QPoint    m_dragStart;
    QRect     m_selectionAtDragStart;
    bool      m_fullscreenMode;     // true when Fullscreen tool is active
    bool      m_windowMode;         // true when Window tool is active
    CaptureIntent m_captureIntent;   // current capture intent for confirmation
    ScrollStage m_scrollStage;       // inactive / ready / actively sampling scroll frames

    // Scroll capture state
    QList<QString> m_scrollFramePaths;
    QString        m_scrollCapturePath;
    QSize          m_scrollCaptureSize;
    bool           m_scrollCaptureReady;
    QTimer*        m_scrollCaptureTimer;
    ScrollControlPanel* m_scrollControlPanel;
    QRect          m_scrollCaptureArea;     // saved capture area in screen coords
    int            m_scrollSimilarCount;    // consecutive similar-frame count
    int            m_scrollFrameCount;      // total frames captured this session
    bool           m_manualScrollAssistMode;

    static constexpr int kMaxScrollFrames            = 100; // increased from 50
    static constexpr int kSimilarStopThreshold       = 4;   // auto-scroll stop threshold
    static constexpr int kManualSimilarStopThreshold = 20;  // manual-scroll inactivity stop threshold
    static constexpr int kScrollLinesPerTick         = 1;   // micro-step scrolling for stronger overlap
    static constexpr int kScrollSettleMs             = 180; // faster cadence for near-continuous capture

    // Window mode state
    QList<WindowInfo> m_windows;
    int               m_hoveredWindow; // index into m_windows, -1 = none

    // Toolbar hover state
    int  m_hoveredTool;             // -1 = none
    bool m_hoveredSizePanel;

    static constexpr int kHandleHitSize = 20;
    static constexpr int kMinSize       = 4;
};
