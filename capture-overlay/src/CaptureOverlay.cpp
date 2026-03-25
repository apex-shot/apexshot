#include "CaptureOverlay.h"
#include "CaptureOverlay_p.h"
#include <QApplication>
#include <QGuiApplication>
#include <QScreen>
#include <QImage>
#include <QRect>
#include <QTimer>
#include <QDBusConnection>
#include <QDBusInterface>
#include <QDBusReply>
#include <gst/gst.h>

void CaptureOverlay::onMicLevelUpdated(double) { /* unused — using polling */ }
// ── Constructor ───────────────────────────────────────────────────────────────

CaptureOverlay::CaptureOverlay(const QPixmap& background, QWidget* parent,
                               bool timerCaptureEnabled,
                               bool initialMic, bool initialSpeaker)
    : QWidget(parent)
    , m_background(background)
    , m_hasSelection(false)
    , m_dragging(false)
    , m_moving(false)
    , m_resizing(HandlePos::None)
    , m_dragStart(0, 0)
    , m_fullscreenMode(false)
    , m_windowMode(false)
    , m_timerCaptureEnabled(timerCaptureEnabled)
    , m_timerDelayActive(timerCaptureEnabled)
    , m_captureDelaySeconds(5)
    , m_countdownActive(false)
    , m_countdownValue(0)
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
    , m_hidpi(false)
    , m_doNotDisturb(true)
    , m_showCursor(true)
    , m_recClicks(false)
    , m_recKeystrokes(false)
    , m_rememberSelection(false)
    , m_dimScreen(true)
    , m_showCountdown(true)
    , m_clickOptionsOpen(false)
    , m_clickSize(0.3)
    , m_clickColor(0)
    , m_clickStyle(0)
    , m_clickAnimate(true)
    , m_sliderDragging(false)
    , m_keySliderDragging(false)
    , m_clickAnimTimer(nullptr)
    , m_clickAnimPhase(0.0)
    , m_keystrokeOptionsOpen(false)
    , m_showKeystrokePreview(false)
    , m_keySize(0.32) // Matches screenshot better as default
    , m_keyPosition(0) // Bottom-Center
    , m_keyAppearance(0) // Dark
    , m_keyBlurBg(true)
    , m_keyFilter(0) // Show all keys
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
    , m_micLevel(0.0)
    , m_speakerLevel(0.0)
    , m_micTimer(new QTimer(this))
    , m_hoveredTool(-1)
    , m_hoveredSizePanel(false)
    , m_hoveredSettingsItem(-1)
{
    // Init GStreamer for webcam capture
    static bool gstInited = false;
    if (!gstInited) {
        int argc = 0;
        char* argv[] = {nullptr};
        gst_init(&argc, nullptr);
        gstInited = true;
    }

    // Cover entire virtual desktop
    QRect desktop;
    for (QScreen* screen : QGuiApplication::screens())
        desktop = desktop.united(screen->geometry());
    setGeometry(desktop);

    setWindowFlags(Qt::FramelessWindowHint
                   | Qt::WindowStaysOnTopHint
                   | Qt::BypassWindowManagerHint
                   | Qt::Tool);

    if (m_background.isNull())
        setAttribute(Qt::WA_TranslucentBackground, true);

    setAttribute(Qt::WA_DeleteOnClose, false);
    setMouseTracking(true);
    setCursor(Qt::CrossCursor);
    grabKeyboard();

    const int defaultW = std::max(kMinSize, std::min(DEFAULT_SELECTION_W, width()));
    const int defaultH = std::max(kMinSize, std::min(DEFAULT_SELECTION_H, height()));
    const int defaultX = (width() - defaultW) / 2;
    const int defaultY = (height() - defaultH) / 2;
    m_selection = QRect(defaultX, defaultY, defaultW, defaultH);
    m_hasSelection = true;

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

    connect(m_scrollControlPanel, &ScrollControlPanel::cancelClicked, this, &CaptureOverlay::cancelSelection);
    connect(m_scrollControlPanel, &ScrollControlPanel::doneClicked, this, [this]() {
        stopAutoScrollCapture(true); // stop and finalize
    });

    // ── Audio level timer — polls daemon for mic + speaker levels ──────────
    m_micTimer->setInterval(33);
    connect(m_micTimer, &QTimer::timeout, this, [this]() {
        QDBusInterface iface(QStringLiteral("org.apexshot.Daemon"),
                             QStringLiteral("/org/apexshot/Daemon"),
                             QStringLiteral("org.apexshot.Daemon"),
                             QDBusConnection::sessionBus());
        if (iface.isValid()) {
            // Poll mic level
            if (m_recordingPanelOpen && m_recMic) {
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
            if (m_recordingPanelOpen && m_recSpeaker) {
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

            update(); // repaint for animation
        }
    });
    m_micTimer->start();
}
