// SPDX-License-Identifier: GPL-3.0-or-later
// ApexShot — Qt5 full-screen area selector overlay
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
#include <QMutex>
#include <QPixmap>
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

    enum class RecordType {
        None,
        Video,
        Gif
    };

    explicit CaptureOverlay(const QPixmap& background = QPixmap(),
                             QWidget* parent = nullptr,
                             bool timerCaptureEnabled = false);

    /// Returns the selected rectangle in screen (logical pixel) coordinates.
    /// Only valid when QApplication exits with code 0.
    QRect selection() const { return m_selection.normalized(); }
    void setInitialSelection(const QRect& rect) { m_selection = rect; }
    bool ocrRequested() const { return m_captureIntent == CaptureIntent::Ocr; }
    bool scrollCaptureCompleted() const { return m_scrollCaptureReady; }
    QString scrollCapturePath() const { return m_scrollCapturePath; }
    QSize scrollCaptureSize() const { return m_scrollCaptureSize; }
    int captureDelaySeconds() const { return m_captureDelaySeconds; }
    bool countdownHandledInOverlay() const { return true; }

    // Recording accessors
    bool recordRequested() const { return m_captureIntent == CaptureIntent::Record; }
    RecordType recordType() const { return m_recordType; }
    bool recordControlsEnabled() const { return m_recControls; }
    bool recordMicEnabled() const { return m_recMic; }
    bool recordSpeakerEnabled() const { return m_recSpeaker; }
    bool recordClicksEnabled() const { return m_recClicks; }
    bool recordKeystrokesEnabled() const { return m_recKeystrokes; }
    bool recordDisplayRecTime() const { return m_displayRecTime; }
    bool recordHidpiEnabled() const { return m_hidpi; }
    bool recordDoNotDisturb() const { return m_doNotDisturb; }
    bool recordShowCursor() const { return m_showCursor; }
    bool recordRememberSelection() const { return m_rememberSelection; }
    bool recordDimScreen() const { return m_dimScreen; }
    bool recordShowCountdown() const { return m_showCountdown; }

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
        Record,
    };

    enum class ScrollStage {
        Inactive,
        Armed,
        Capturing,
    };

    enum class RecordPanelTile {
        None,
        Controls, Size, Crop,
        Mic, Speaker, Webcam, Click, Keystrokes,
        RecordVideo, RecordGif
    };

private slots:
    void onMicLevelUpdated(double level);

private:
    // Drawing
    void drawToolbar(QPainter& p,
                     double selX, double selY,
                     double selW, double selH,
                     double screenW, double screenH);
    void drawRecordingPanel(QPainter& p,
                            double selX, double selY,
                            double selW, double selH);
    void drawSettingsMenu(QPainter& p,
                          double panelX, double startY);
    void drawClickOptions(QPainter& p, const QRectF& parentRect);
    void drawKeystrokeOptions(QPainter& p, const QRectF& parentRect);
    void drawDropdownPopup(QPainter& p, const QRectF& anchorRect,
                           const QStringList& options, int selectedIndex);
    void startClickAnimTimer();
    void stopClickAnimTimer();
    QRectF scrollPrimaryButtonRect() const;

    // Webcam
    void showWebcamContextMenu(const QPoint& globalPos);
    void enumerateWebcamDevices();
    void startWebcamCapture();
    void stopWebcamCapture();
    void* m_webcamPipeline = nullptr; // GstElement*
    QPixmap m_webcamFrame;
    QMutex m_webcamMutex;

    // Hit testing / cursor
    void updateCursor(const QPoint& pos);
    HandlePos hitTest(const QPoint& pos) const;
    RecordPanelTile hitTestRecordingPanel(const QPoint& pos) const;
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

    void cycleCaptureDelay();
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
    bool      m_timerCaptureEnabled;
    bool      m_timerDelayActive;
    int       m_captureDelaySeconds;
    bool      m_countdownActive;
    int       m_countdownValue;
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

    // Recording panel state
    bool m_recordingPanelOpen;
    bool m_settingsOpen; // new: true when Settings/Sliders icon clicked
    int  m_settingsTab;  // new: 0=General, 1=Video, 2=GIF
    // Dropdown popup state
    int m_dropdownOpen;      // -1 = none, else index in m_settingsClickableRects
    QRectF m_dropdownAnchor; // rect of the button that opened the dropdown
    QStringList m_dropdownOptions;
    QList<QColor> m_dropdownColors; // optional: if non-empty, draw color circles
    int* m_dropdownValuePtr; // pointer to the int being edited
    int  m_hoveredDropdownItem; // index into current dropdown options
    QList<QRectF> m_dropdownItemRects;
    bool m_recordingToolsHidden; // true when user clicks Record tile
    RecordType m_recordType;
    RecordPanelTile m_hoveredRecordTile;
    
    // Recording & Settings state (matching screenshot)
    bool m_recControls;        // "Show controls while recording"
    bool m_displayRecTime;     // "Display recording time"
    bool m_hidpi;              // "HiDPI Scaling — record at display scale resolution"
    bool m_doNotDisturb;       // ""Do Not Disturb" while recording"
    bool m_showCursor;         // "Show cursor"
    bool m_recClicks;          // "Highlight clicks"
    bool m_recKeystrokes;      // "Show keystrokes"
    bool m_rememberSelection;  // "Remember last selection"
    bool m_dimScreen;          // "Dim screen while recording"
    bool m_showCountdown;      // "Show countdown"

    // Click highlight options
    bool   m_clickOptionsOpen;
    double m_clickSize;        // 0.0 to 1.0
    int    m_clickColor;       // index
    int    m_clickStyle;       // index
    bool   m_clickAnimate;
    QList<QPointF> m_clickPreviews; // for preview animation state if needed
    bool   m_sliderDragging;   // true while dragging size slider
    QRectF m_sliderTrackRect;  // cached slider track rect for drag calc
    QTimer* m_clickAnimTimer;  // timer for preview animation ticks
    double m_clickAnimPhase;   // 0.0 to 1.0 cycling phase for animation

    // Keystroke options
    bool   m_keystrokeOptionsOpen;
    double m_keySize;        // 0.0 to 1.0
    int    m_keyPosition;    // index
    int    m_keyAppearance;  // index
    bool   m_keyBlurBg;
    int    m_keyFilter;      // 0=All, 1=Command
    
    // Video settings
    int  m_videoMaxRes;      // index
    int  m_videoFps;         // index
    bool m_recordMono;
    bool m_openEditor;

    // GIF settings
    int    m_gifFps;         // value (typically 5-60)
    double m_gifQuality;     // 0.0 to 1.0
    bool   m_optimizeGif;
    int    m_gifSizeIdx;     // index

    bool m_recMic;
    bool m_recSpeaker;
    bool m_recWebcam;
    enum class WebcamSize { Small, Medium, Large, Huge, Fullscreen };
    enum class WebcamShape { Circle, Square, Rectangle, Vertical };
    WebcamSize m_webcamSize = WebcamSize::Medium;
    WebcamShape m_webcamShape = WebcamShape::Vertical;
    bool m_webcamFlip = false;
    int m_webcamDevice = -1; // -1 = None, 0+ = /dev/videoN
    QStringList m_webcamDevices; // cached device names
    double m_micLevel; // Normalized level for animation
    double m_speakerLevel; // Normalized level for speaker animation
    QTimer* m_micTimer;
    
    // Recording panel layout caches (for hit testing)
    QRectF m_recPanelRect;
    QRectF m_settingsPanelRect; // for hit testing settings menu
    QRectF m_clickOptionsPanelRect;
    QRectF m_keystrokeOptionsPanelRect;
    QList<QRectF> m_recTileRects; // Matches RecordPanelTile order (skip None)
    QList<QRectF> m_settingsClickableRects; // checkbox & tab rects for hit testing
    QList<QRectF> m_clickOptionsClickableRects;
    QList<QRectF> m_keystrokeOptionsClickableRects;

    // Toolbar hover state
    int  m_hoveredTool;             // -1 = none
    bool m_hoveredSizePanel;
    int  m_hoveredSettingsItem;     // new: index into m_settingsClickableRects, -1 = none

    static constexpr int kHandleHitSize = 20;
    static constexpr int kMinSize       = 4;
};
