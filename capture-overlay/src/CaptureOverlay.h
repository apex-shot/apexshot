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
#include <algorithm>

class QTimer;

#include "ScrollControlPanel.h"

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
                             bool timerCaptureEnabled = false,
                             bool initialMic = false,
                             bool initialSpeaker = false);

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
    void focusAndRaiseOverlay();

    // Recording accessors
    bool recordRequested() const { return m_captureIntent == CaptureIntent::Record; }
    bool recordConfigRequested() const { return m_recordConfigRequested; }
    RecordType recordType() const { return m_recordType; }
    bool recordControlsEnabled() const { return m_recControls; }
    bool recordFullscreen() const { return m_fullscreenMode; }
    bool recordMicEnabled() const { return m_recMic; }
    bool recordSpeakerEnabled() const { return m_recSpeaker; }
    bool recordClicksEnabled() const { return m_recClicks; }
    bool recordKeystrokesEnabled() const { return m_recKeystrokes; }
    bool recordWebcamEnabled() const { return m_recWebcam; }
    double recordClickSize() const { return m_clickSize; }
    int recordClickColor() const { return m_clickColor; }
    int recordClickStyle() const { return m_clickStyle; }
    bool recordClickAnimate() const { return m_clickAnimate; }
    double recordKeySize() const { return m_keySize; }
    int recordKeyPosition() const { return m_keyPosition; }
    int recordKeyAppearance() const { return m_keyAppearance; }
    bool recordKeyBlurBg() const { return m_keyBlurBg; }
    int recordKeyFilter() const { return m_keyFilter; }
    int recordWebcamSize() const { return static_cast<int>(m_webcamSize); }
    int recordWebcamShape() const { return static_cast<int>(m_webcamShape); }
    bool recordWebcamFlip() const { return m_webcamFlip; }
    int recordWebcamDevice() const { return m_webcamDevice; }
    double recordWebcamRelX() const { return m_webcamRelX; }
    double recordWebcamRelY() const { return m_webcamRelY; }
    bool recordDisplayRecTime() const { return m_displayRecTime; }
    bool recordHidpiEnabled() const { return m_hidpi; }
    bool recordDoNotDisturb() const { return m_doNotDisturb; }
    bool recordShowCursor() const { return m_showCursor; }
    bool recordRememberSelection() const { return m_rememberSelection; }
    bool recordDimScreen() const { return m_dimScreen; }
    bool recordShowCountdown() const { return m_showCountdown; }

    // Video tab settings
    int recordVideoMaxRes() const { return m_videoMaxRes; }
    int recordVideoFps() const { return m_videoFps; }
    bool recordMono() const { return m_recordMono; }
    bool recordOpenEditor() const { return m_openEditor; }

    // GIF tab settings — accessors
    int recordGifFps() const { return m_gifFps; }
    double recordGifQuality() const { return m_gifQuality; }
    int recordGifSizeIdx() const { return m_gifSizeIdx; }
    bool recordOptimizeGif() const { return m_optimizeGif; }

    // GIF tab settings — setters for initial config load
    void setInitialRecControls(bool v) { m_recControls = v; }
    void setInitialDisplayRecTime(bool v) { m_displayRecTime = v; }
    void setInitialHidpi(bool v) { m_hidpi = v; }
    void setInitialDoNotDisturb(bool v) { m_doNotDisturb = v; }
    void setInitialShowCursor(bool v) { m_showCursor = v; }
    void setInitialRecClicks(bool v) { m_recClicks = v; }
    void setInitialRecKeystrokes(bool v) { m_recKeystrokes = v; }
    void setInitialRecWebcam(bool v)
    {
        m_recWebcam = v;
        if (!m_recWebcam) {
            stopWebcamCapture();
        } else if (m_recordingPanelOpen && m_webcamDevice >= 0) {
            startWebcamCapture();
        }
    }
    void setInitialClickSize(double v) { m_clickSize = v; }
    void setInitialClickColor(int v) { m_clickColor = v; }
    void setInitialClickStyle(int v) { m_clickStyle = v; }
    void setInitialClickAnimate(bool v) { m_clickAnimate = v; }
    void setInitialKeySize(double v) { m_keySize = v; }
    void setInitialKeyPosition(int v) { m_keyPosition = v; }
    void setInitialKeyAppearance(int v) { m_keyAppearance = v; }
    void setInitialKeyBlurBg(bool v) { m_keyBlurBg = v; }
    void setInitialKeyFilter(int v) { m_keyFilter = v; }
    void setInitialWebcamSize(int v) { m_webcamSize = static_cast<WebcamSize>(v); }
    void setInitialWebcamShape(int v) { m_webcamShape = static_cast<WebcamShape>(v); }
    void setInitialWebcamFlip(bool v) { m_webcamFlip = v; }
    void setInitialWebcamDevice(int v)
    {
        m_webcamDevice = v;
        if (m_webcamDevice < 0) {
            stopWebcamCapture();
        } else if (m_recordingPanelOpen && m_recWebcam) {
            startWebcamCapture();
        }
    }
    void setInitialWebcamRelX(double v) { m_webcamRelX = std::clamp(v, 0.0, 1.0); }
    void setInitialWebcamRelY(double v) { m_webcamRelY = std::clamp(v, 0.0, 1.0); }
    void setInitialRememberSelection(bool v) { m_rememberSelection = v; }
    void setInitialDimScreen(bool v) { m_dimScreen = v; }
    void setInitialShowCountdown(bool v) { m_showCountdown = v; }
    void setInitialVideoMaxRes(int v) { m_videoMaxRes = v; }
    void setInitialVideoFps(int v) { m_videoFps = v; }
    void setInitialRecordMono(bool v) { m_recordMono = v; }
    void setInitialOpenEditor(bool v) { m_openEditor = v; }
    void setInitialGifFps(int v) { m_gifFps = v; }
    void setInitialGifQuality(double v) { m_gifQuality = v; }
    void setInitialGifSizeIdx(int v) { m_gifSizeIdx = v; }
    void setInitialGifOptimize(bool v) { m_optimizeGif = v; }

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

    enum class ToolbarActionCard {
        None,
        Confirm,
        Cancel
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
    void drawKeystrokePreview(QPainter& p, double sx, double sy, double selW, double selH);
    QRectF scrollPrimaryButtonRect() const;
    QSizeF webcamPreviewSize(double selW, double selH) const;
    QRectF webcamPreviewRect(double selX, double selY, double selW, double selH) const;
    void setWebcamPreviewTopLeft(const QPointF& topLeft,
                                 double selX, double selY,
                                 double selW, double selH);

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
    void confirmRecordingSelection();
    void cancelSelection();
    void resetRecordingPanelToAreaMode(bool keepSelection = true);

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
    bool      m_countdownCancelRequested;
    bool      m_hoveredCountdownCancel;
    QRectF    m_countdownBubbleRect;
    bool      m_captureCropMenuOpen;
    int       m_captureAspectRatioIndex;
    int       m_hoveredCaptureCropMenuItem;
    QRectF    m_captureCropMenuPanelRect;
    QList<QRectF> m_captureCropMenuItemRects;
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
    bool m_recordConfigRequested;
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
    int  m_recordAspectRatioIndex; // 0 = Freeform
    bool m_rememberSelection;  // "Remember last selection"
    bool m_dimScreen;          // "Dim screen while recording"
    bool m_showCountdown;      // "Show countdown"

    // Click highlight options
    bool   m_clickOptionsOpen;
    double m_clickSize;        // 0.0 to 1.0
    int    m_clickColor;       // index
    int    m_clickStyle;       // index
    bool   m_clickAnimate;
    struct ClickPreview {
        QPointF pos;
        qint64  birthMs;       // QDateTime::currentMSecsSinceEpoch() at creation
    };
    QList<ClickPreview> m_clickPreviews;
    bool   m_sliderDragging;   // true while dragging click size slider
    QRectF m_sliderTrackRect;  // cached click slider track rect for drag calc
    bool   m_keySliderDragging; // true while dragging keystroke size slider
    QRectF m_keySliderTrackRect; // cached keystroke slider track rect
    bool   m_gifFpsDragging;       // true while dragging GIF FPS slider
    bool   m_gifQualityDragging;   // true while dragging GIF quality slider
    QRectF m_gifFpsTrackRect;      // cached GIF FPS slider track rect for drag calc
    QRectF m_gifQualityTrackRect;  // cached GIF quality slider track rect for drag calc
    QTimer* m_clickAnimTimer;  // timer for preview animation ticks
    double m_clickAnimPhase;   // 0.0 to 1.0 cycling phase for animation

    // Keystroke options
    bool   m_keystrokeOptionsOpen;
    bool   m_showKeystrokePreview; // toggle for previewing position/style
    double m_keySize;        // 0.0 to 1.0
    int    m_keyPosition;    // index
    int    m_keyAppearance;  // index
    bool   m_keyBlurBg;
    int    m_keyFilter;      // 0=All, 1=Command
    struct KeyPreview {
        QString text;
        qint64  birthMs;
    };
    QList<KeyPreview> m_keyPreviews; // recent key presses for live preview
    
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
    double m_webcamRelX = 0.0;
    double m_webcamRelY = 0.0;
    bool m_draggingWebcam = false;
    QPointF m_webcamDragOffset;
    QStringList m_webcamDevices; // cached device names
    double m_micLevel; // Normalized level for animation
    double m_speakerLevel; // Normalized level for speaker animation
    QTimer* m_micTimer;
    
    // Recording panel layout caches (for hit testing)
    QRectF m_recPanelRect;
    QRectF m_recordingToggleRailRect;
    QRectF m_recordingTopClusterRect;
    QRectF m_recordingBottomBarRect;
    QRectF m_settingsPanelRect; // for hit testing settings menu
    QRectF m_clickOptionsPanelRect;
    QRectF m_keystrokeOptionsPanelRect;
    QRectF m_cropMenuPanelRect;
    QList<QRectF> m_recTileRects; // Matches RecordPanelTile order (skip None)
    QList<QRectF> m_settingsClickableRects; // checkbox & tab rects for hit testing
    QList<QRectF> m_clickOptionsClickableRects;
    QList<QRectF> m_keystrokeOptionsClickableRects;
    QList<QRectF> m_cropMenuItemRects;

    // Toolbar hover state
    int  m_hoveredTool;             // -1 = none
    bool m_hoveredSizeCard;
    bool m_hoveredCaptureCropCard;
    ToolbarActionCard m_hoveredActionCard;
    int  m_hoveredSettingsItem;     // new: index into m_settingsClickableRects, -1 = none
    int  m_hoveredCropMenuItem;
    bool m_cropMenuOpen;

    static constexpr int kHandleHitSize = 20;
    static constexpr int kMinSize       = 4;
};
