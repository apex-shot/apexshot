#include "CaptureOverlay.h"
#include "CaptureOverlay_p.h"
#include <QApplication>
#include <QGuiApplication>
#include <QScreen>
#include <QWindow>
#include <QImage>
#include <QMouseEvent>
#include <QRect>
#include <QTimer>
#include <QCursor>
#include <QVector>
#include <QDBusConnection>
#include <QDBusInterface>
#include <QDBusReply>
#include <QtMath>
#if defined(Q_OS_LINUX)
#include <X11/Xatom.h>
#include <X11/Xlib.h>
#undef None
#endif

void CaptureOverlay::onMicLevelUpdated(double) { /* unused — using polling */ }

namespace {

bool desktopBounds(bool available, QRect& outBounds)
{
    const auto screens = QGuiApplication::screens();
    if (screens.isEmpty()) {
        return false;
    }

    outBounds = available ? screens.first()->availableGeometry()
                          : screens.first()->geometry();
    for (QScreen* screen : screens) {
        outBounds = outBounds.united(available ? screen->availableGeometry()
                                               : screen->geometry());
    }
    return outBounds.width() > 0 && outBounds.height() > 0;
}

bool x11RootCardinalProperty(const char* name, QVector<unsigned long>& values)
{
#if defined(Q_OS_LINUX)
    Display* display = XOpenDisplay(nullptr);
    if (!display) {
        return false;
    }

    const Atom property = XInternAtom(display, name, True);
    if (property == 0) {
        XCloseDisplay(display);
        return false;
    }

    Atom actualType = 0;
    int actualFormat = 0;
    unsigned long itemCount = 0;
    unsigned long bytesAfter = 0;
    unsigned char* data = nullptr;
    const int status = XGetWindowProperty(display,
                                          DefaultRootWindow(display),
                                          property,
                                          0,
                                          1024,
                                          False,
                                          XA_CARDINAL,
                                          &actualType,
                                          &actualFormat,
                                          &itemCount,
                                          &bytesAfter,
                                          &data);
    if (status != Success || !data || actualType != XA_CARDINAL || actualFormat != 32) {
        if (data) {
            XFree(data);
        }
        XCloseDisplay(display);
        return false;
    }

    const auto* raw = reinterpret_cast<unsigned long*>(data);
    values.clear();
    values.reserve(static_cast<int>(itemCount));
    for (unsigned long i = 0; i < itemCount; ++i) {
        values.push_back(raw[i]);
    }

    XFree(data);
    XCloseDisplay(display);
    return !values.isEmpty();
#else
    Q_UNUSED(name);
    Q_UNUSED(values);
    return false;
#endif
}

bool x11CurrentDesktop(unsigned long& desktop)
{
    QVector<unsigned long> values;
    if (!x11RootCardinalProperty("_NET_CURRENT_DESKTOP", values) || values.isEmpty()) {
        return false;
    }
    desktop = values.first();
    return true;
}

bool x11NetWorkArea(QRect& outWorkArea)
{
    QVector<unsigned long> values;
    if (!x11RootCardinalProperty("_NET_WORKAREA", values) || values.size() < 4) {
        return false;
    }

    unsigned long desktop = 0;
    if (x11CurrentDesktop(desktop)) {
        const int offset = static_cast<int>(desktop) * 4;
        if (offset >= 0 && offset + 3 < values.size()) {
            outWorkArea = QRect(static_cast<int>(values[offset]),
                                static_cast<int>(values[offset + 1]),
                                static_cast<int>(values[offset + 2]),
                                static_cast<int>(values[offset + 3]));
            return outWorkArea.width() > 0 && outWorkArea.height() > 0;
        }
    }

    outWorkArea = QRect(static_cast<int>(values[0]),
                        static_cast<int>(values[1]),
                        static_cast<int>(values[2]),
                        static_cast<int>(values[3]));
    return outWorkArea.width() > 0 && outWorkArea.height() > 0;
}

QPoint overlayLocalOriginForDesktop(const QRect& overlayRect)
{
    QRect desktop;
    QRect available;
    if (!desktopBounds(false, desktop)) {
        return overlayRect.topLeft();
    }
    if (!x11NetWorkArea(available) && !desktopBounds(true, available)) {
        return overlayRect.topLeft();
    }

    QPoint origin = overlayRect.topLeft();
    constexpr int tolerance = 2;

    const bool widthMatchesAvailable =
        available.width() > 0 &&
        qAbs(overlayRect.width() - available.width()) <= tolerance &&
        available.width() < desktop.width();
    if (widthMatchesAvailable) {
        origin.setX(available.x());
    }

    const bool heightMatchesAvailable =
        available.height() > 0 &&
        qAbs(overlayRect.height() - available.height()) <= tolerance &&
        available.height() < desktop.height();
    if (heightMatchesAvailable) {
        origin.setY(available.y());
    }

    return origin;
}

} // namespace

QPoint CaptureOverlay::desktopOriginForLocalCoordinates() const
{
    if (!qEnvironmentVariableIsSet("WAYLAND_DISPLAY") && m_hasEventDesktopOrigin) {
        return m_eventDesktopOrigin;
    }
    return overlayLocalOriginForDesktop(QRect(mapToGlobal(QPoint(0, 0)), size()));
}

QRect CaptureOverlay::desktopSelection() const
{
    return m_selection.normalized().translated(desktopOriginForLocalCoordinates());
}

void CaptureOverlay::updateDesktopOriginFromMouseEvent(QMouseEvent* event)
{
    if (!event) {
        return;
    }
    m_eventDesktopOrigin = event->globalPos() - event->pos();
    m_hasEventDesktopOrigin = true;
}

QSizeF CaptureOverlay::webcamPreviewSize(double selW, double selH) const
{
    constexpr double kMargin = 10.0;

    double previewW = 200.0;
    double previewH = 260.0;
    switch (m_webcamSize) {
    case WebcamSize::Small:
        previewW = 120.0;
        previewH = 160.0;
        break;
    case WebcamSize::Medium:
        previewW = 200.0;
        previewH = 260.0;
        break;
    case WebcamSize::Large:
        previewW = 280.0;
        previewH = 370.0;
        break;
    case WebcamSize::Huge:
        previewW = 360.0;
        previewH = 480.0;
        break;
    case WebcamSize::Fullscreen:
        previewW = std::max(1.0, selW - (2.0 * kMargin));
        previewH = std::max(1.0, selH - (2.0 * kMargin));
        break;
    }

    switch (m_webcamShape) {
    case WebcamShape::Circle:
    case WebcamShape::Square:
        previewH = previewW;
        break;
    case WebcamShape::Rectangle:
        previewH = previewW * 0.75;
        break;
    case WebcamShape::Vertical:
        break;
    }

    previewW = std::min(previewW, std::max(1.0, selW - (2.0 * kMargin)));
    previewH = std::min(previewH, std::max(1.0, selH - (2.0 * kMargin)));
    return QSizeF(previewW, previewH);
}

QRectF CaptureOverlay::webcamPreviewRect(double selX, double selY, double selW, double selH) const
{
    constexpr double kMargin = 10.0;
    const QSizeF previewSize = webcamPreviewSize(selW, selH);

    const double minX = selX + kMargin;
    const double maxX = std::max(minX, selX + selW - previewSize.width() - kMargin);
    const double minY = selY + kMargin;
    const double maxY = std::max(minY, selY + selH - previewSize.height() - kMargin);

    const double px = minX + ((maxX - minX) * std::clamp(m_webcamRelX, 0.0, 1.0));
    // Preserve the existing bottom-left default when rel_y is 0.0.
    const double py = minY + ((maxY - minY) * (1.0 - std::clamp(m_webcamRelY, 0.0, 1.0)));
    return QRectF(px, py, previewSize.width(), previewSize.height());
}

void CaptureOverlay::setWebcamPreviewTopLeft(const QPointF& topLeft,
                                             double selX, double selY,
                                             double selW, double selH)
{
    constexpr double kMargin = 10.0;
    const QSizeF previewSize = webcamPreviewSize(selW, selH);

    const double minX = selX + kMargin;
    const double maxX = std::max(minX, selX + selW - previewSize.width() - kMargin);
    const double minY = selY + kMargin;
    const double maxY = std::max(minY, selY + selH - previewSize.height() - kMargin);

    const double clampedX = std::clamp(topLeft.x(), minX, maxX);
    const double clampedY = std::clamp(topLeft.y(), minY, maxY);

    m_webcamRelX = (maxX > minX) ? (clampedX - minX) / (maxX - minX) : 0.0;
    m_webcamRelY = (maxY > minY) ? 1.0 - ((clampedY - minY) / (maxY - minY)) : 0.0;
    m_webcamRelX = std::clamp(m_webcamRelX, 0.0, 1.0);
    m_webcamRelY = std::clamp(m_webcamRelY, 0.0, 1.0);
}

void CaptureOverlay::focusAndRaiseOverlay()
{
    show();
    raise();
    activateWindow();
    if (windowHandle()) {
        windowHandle()->requestActivate();
    }
    setFocus(Qt::ActiveWindowFocusReason);
    if (QWidget::keyboardGrabber() != this) {
        grabKeyboard();
    }
    if (!qEnvironmentVariableIsSet("WAYLAND_DISPLAY")
        && QWidget::mouseGrabber() != this) {
        grabMouse();
    }
}

static Qt::WindowFlags captureOverlayWindowFlags()
{
    if (qEnvironmentVariableIsSet("WAYLAND_DISPLAY")) {
        return Qt::BypassWindowManagerHint
               | Qt::WindowStaysOnTopHint
               | Qt::FramelessWindowHint
               | Qt::Tool;
    }

    return Qt::FramelessWindowHint
           | Qt::WindowStaysOnTopHint
           | Qt::Tool;
}
// ── Constructor ───────────────────────────────────────────────────────────────

void CaptureOverlay::openRecordingPanelForShortcut()
{
    m_recordingPanelOpen = true;
    m_recordConfigRequested = true;
    m_micTimer->start();
    m_recordingToolsHidden = false;
    m_settingsOpen = false;
    m_captureIntent = CaptureIntent::Area;
    if (m_recordType == RecordType::None) {
        m_recordType = RecordType::Video;
    }
    if (m_recWebcam && m_webcamDevice >= 0) {
        startWebcamCapture();
    }
    update();
}

CaptureOverlay::CaptureOverlay(const QPixmap& background, QWidget* parent,
                               bool timerCaptureEnabled,
                               bool initialMic, bool initialSpeaker,
                               OverlayMode overlayMode)
    : QWidget(parent)
    , m_background(background)
    , m_overlayMode(overlayMode)
    , m_eventDesktopOrigin(0, 0)
    , m_hasEventDesktopOrigin(false)
    , m_hasSelection(false)
    , m_dragging(false)
    , m_moving(false)
    , m_resizing(HandlePos::None)
    , m_dragStart(0, 0)
    , m_pointerPos(0, 0)
    , m_lastCursorShape(Qt::ArrowCursor)
    , m_lastCrosshairPaintPoint(0, 0)
    , m_lastCrosshairHadSelection(false)
    , m_fullscreenMode(false)
    , m_windowMode(false)
    , m_timerCaptureEnabled(timerCaptureEnabled)
    , m_selectionCursorMode(QStringLiteral("Disabled"))
    , m_showZoomPreview(false)
    , m_freezeSelectionBackground(true)
    , m_timerDelayActive(timerCaptureEnabled)
    , m_captureDelaySeconds(5)
    , m_countdownActive(false)
    , m_countdownValue(0)
    , m_countdownCancelRequested(false)
    , m_hoveredCountdownCancel(false)
    , m_countdownTimer(new QTimer(this))
    , m_countdownForRecording(false)
    , m_captureCropMenuOpen(false)
    , m_captureAspectRatioIndex(0)
    , m_hoveredCaptureCropMenuItem(-1)
    , m_captureIntent(CaptureIntent::Area)
    , m_scrollStage(ScrollStage::Inactive)
    , m_scrollCaptureReady(false)
    , m_scrollCaptureTimer(new QTimer(this))
    , m_scrollControlPanel(new ScrollControlPanel())
    , m_scrollSimilarCount(0)
    , m_scrollFrameCount(0)
    , m_manualScrollAssistMode(false)
    , m_hoveredWindow(-1)
    , m_recordingPanelOpen(false)
    , m_recordingToolsHidden(false)
    , m_recordType(RecordType::None)
    , m_hoveredRecordTile(RecordPanelTile::None)
    , m_settingsOpen(false)
    , m_settingsTab(0)
    , m_dropdownOpen(-1)
    , m_dropdownValuePtr(nullptr)
    , m_hoveredDropdownItem(-1)
    , m_recControls(true)
    , m_displayRecTime(false)
    , m_hidpi(true)
    , m_doNotDisturb(true)
    , m_showCursor(true)
    , m_recordAspectRatioIndex(0)
    , m_rememberSelection(false)
    , m_dimScreen(true)
    , m_showCountdown(true)
    , m_gifFpsDragging(false)
    , m_gifQualityDragging(false)
    , m_videoFormat(0) // MP4
    , m_videoMaxRes(0) // Original
    , m_videoFps(2) // 50 (index 2: 24, 30, 50, 60)
    , m_recordMono(false)
    , m_openEditor(true)
    , m_gifFps(50)
    , m_gifQuality(0.75)
    , m_optimizeGif(true)
    , m_gifSizeIdx(0) // 800 x auto (default)
    , m_recMic(initialMic)
    , m_recSpeaker(initialSpeaker)
    , m_recWebcam(false)
    , m_webcamRelX(0.0)
    , m_webcamRelY(0.0)
    , m_micLevel(0.0)
    , m_speakerLevel(0.0)
    , m_micTimer(new QTimer(this))
    , m_hoveredTool(-1)
    , m_hoveredSizeCard(false)
    , m_hoveredCaptureCropCard(false)
    , m_hoveredActionCard(ToolbarActionCard::None)
    , m_hoveredSettingsItem(-1)
    , m_hoveredCropMenuItem(-1)
    , m_cropMenuOpen(false)
    , m_recordConfigRequested(false)
{
    // Cover entire virtual desktop
    QRect desktop;
    for (QScreen* screen : QGuiApplication::screens())
        desktop = desktop.united(screen->geometry());
    setGeometry(desktop);

    setWindowFlags(captureOverlayWindowFlags());
    setWindowTitle(QStringLiteral("ApexShot Capture Overlay"));
    setFocusPolicy(Qt::StrongFocus);

    if (m_background.isNull())
        setAttribute(Qt::WA_TranslucentBackground, true);

    setAttribute(Qt::WA_DeleteOnClose, false);
    setAttribute(Qt::WA_StaticContents, true);
    setMouseTracking(true);
    setCursor(Qt::CrossCursor);
    m_lastCursorShape = Qt::CrossCursor;
    const QPoint initialPointer = mapFromGlobal(QCursor::pos());
    m_pointerPos = initialPointer;
    m_lastCrosshairPaintPoint = initialPointer;
    focusAndRaiseOverlay();
    QTimer::singleShot(0, this, [this]() { focusAndRaiseOverlay(); });
    QTimer::singleShot(100, this, [this]() { focusAndRaiseOverlay(); });

    if (isCrosshairMode()) {
        m_dimScreen = false;
        m_selection = QRect();
        m_hasSelection = false;
        m_lastCrosshairSelectionRect = QRect();
        m_lastCrosshairBubbleRect = crosshairBubbleRectForPoint(initialPointer);
    } else {
        const int defaultW = std::max(kMinSize, std::min(DEFAULT_SELECTION_W, width()));
        const int defaultH = std::max(kMinSize, std::min(DEFAULT_SELECTION_H, height()));
        const int defaultX = (width() - defaultW) / 2;
        const int defaultY = (height() - defaultH) / 2;
        m_selection = QRect(defaultX, defaultY, defaultW, defaultH);
        m_hasSelection = true;
    }

    // Pre-build blurred background for frosted glass (1/4 res gaussian)
    if (!m_background.isNull()) {
        QImage full = m_background.toImage().convertToFormat(QImage::Format_ARGB32);
        int bw = std::max(1, full.width() / 4);
        int bh = std::max(1, full.height() / 4);
        QImage small = full.scaled(bw, bh, Qt::IgnoreAspectRatio, Qt::SmoothTransformation);
        // Simple box blur approximation (3 passes)
        for (int pass = 0; pass < 3; ++pass) {
            small = small.scaled(bw * 2, bh * 2, Qt::IgnoreAspectRatio, Qt::SmoothTransformation)
                         .scaled(bw, bh, Qt::IgnoreAspectRatio, Qt::SmoothTransformation);
        }
        m_blurredBg = small;
    }

    m_scrollCaptureTimer->setSingleShot(true);
    connect(m_scrollCaptureTimer, &QTimer::timeout, this, &CaptureOverlay::onAutoScrollTick);

    m_countdownTimer->setSingleShot(true);
    connect(m_countdownTimer, &QTimer::timeout, this, &CaptureOverlay::onCountdownTick);

    connect(m_scrollControlPanel, &ScrollControlPanel::cancelClicked, this, &CaptureOverlay::cancelSelection);
    connect(m_scrollControlPanel, &ScrollControlPanel::doneClicked, this, [this]() {
        stopAutoScrollCapture(true); // stop and finalize
    });

    // ── Audio level timer — polls daemon for mic + speaker levels ──────────
    m_micTimer->setInterval(33);
    connect(m_micTimer, &QTimer::timeout, this, [this]() {
        if (!m_recordingPanelOpen || (!m_recMic && !m_recSpeaker)) {
            const bool hadLevels = m_micLevel > 0.0 || m_speakerLevel > 0.0;
            m_micLevel = 0.0;
            m_speakerLevel = 0.0;
            if (hadLevels) {
                update();
            }
            return;
        }

        QDBusInterface iface(QStringLiteral("org.apexshot.Daemon"),
                             QStringLiteral("/org/apexshot/Daemon"),
                             QStringLiteral("org.apexshot.Daemon"),
                             QDBusConnection::sessionBus());
        if (!iface.isValid()) {
            return;
        }

        const double previousMicLevel = m_micLevel;
        const double previousSpeakerLevel = m_speakerLevel;

        // Poll mic level
        if (m_recMic) {
            QDBusReply<double> reply = iface.call(QStringLiteral("GetMicLevel"));
            if (reply.isValid()) {
                double level = reply.value();
                if (level > m_micLevel) {
                    m_micLevel = level;
                } else {
                    m_micLevel = m_micLevel * 0.6 + level * 0.4;
                }
            }
        } else {
            m_micLevel = 0.0;
        }

        // Poll speaker level
        if (m_recSpeaker) {
            QDBusReply<double> reply = iface.call(QStringLiteral("GetSpeakerLevel"));
            if (reply.isValid()) {
                double level = reply.value();
                if (level > m_speakerLevel) {
                    m_speakerLevel = level;
                } else {
                    m_speakerLevel = m_speakerLevel * 0.6 + level * 0.4;
                }
            }
        } else {
            m_speakerLevel = 0.0;
        }

        if (m_micLevel != previousMicLevel || m_speakerLevel != previousSpeakerLevel) {
            update();
        }
    });
}
