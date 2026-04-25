#include "CaptureOverlay.h"
#include "CaptureOverlay_p.h"
#include <QApplication>
#include <QCursor>
#include <QPoint>
#include <QRect>
#include <QList>
#include <QThread>
#include <QTimer>

void CaptureOverlay::cycleCaptureDelay()
{
    if (!m_timerCaptureEnabled || m_captureIntent == CaptureIntent::Scroll) {
        return;
    }

    switch (m_captureDelaySeconds) {
    case 0:
        m_captureDelaySeconds = 3;
        break;
    case 3:
        m_captureDelaySeconds = 5;
        break;
    case 5:
        m_captureDelaySeconds = 10;
        break;
    default:
        m_captureDelaySeconds = 0;
        break;
    }

    update();
}

void CaptureOverlay::confirmSelection()
{
    m_recordConfigRequested = false;
    m_captureCropMenuOpen = false;
    m_hoveredCaptureCropMenuItem = -1;
    m_captureCropMenuPanelRect = QRectF();
    m_captureCropMenuItemRects.clear();
    if (m_scrollCaptureTimer && m_scrollCaptureTimer->isActive()) {
        m_scrollCaptureTimer->stop();
    }

    if (m_timerDelayActive && m_captureDelaySeconds > 0 && !m_countdownActive) {
        m_countdownActive = true;
        m_countdownValue = m_captureDelaySeconds;
        m_countdownForRecording = false;
        m_countdownCancelRequested = false;
        m_countdownTimer->setInterval(1000);
        m_countdownTimer->start();
        update();
        return;
    }

    releaseKeyboard();
    hide();
    QApplication::processEvents();
    QThread::msleep(120);
    QApplication::exit(0);
}

void CaptureOverlay::confirmRecordingSelection()
{
    m_recordConfigRequested = false;
    m_micTimer->stop();
    stopWebcamCapture();
    m_recordingToolsHidden = true;
    m_hoveredRecordTile = RecordPanelTile::None;
    m_cropMenuOpen = false;
    m_hoveredCropMenuItem = -1;
    m_countdownCancelRequested = false;
    m_hoveredCountdownCancel = false;
    update();
    QApplication::processEvents();

    if (m_showCountdown && !m_countdownActive) {
        m_countdownActive = true;
        m_countdownValue = 3;
        m_countdownForRecording = true;
        m_countdownCancelRequested = false;
        m_hoveredCountdownCancel = false;
        m_countdownTimer->setInterval(1000);
        m_countdownTimer->start();
        update();
        return;
    }

    releaseKeyboard();
    hide();
    QApplication::processEvents();
    QThread::msleep(120);
    QApplication::exit(0);
}

void CaptureOverlay::onCountdownTick()
{
    if (!m_countdownActive)
        return;

    // Check for cancel request (set by clicking the countdown bubble)
    if (m_countdownCancelRequested) {
        m_countdownActive = false;
        m_countdownValue = 0;
        m_countdownCancelRequested = false;
        m_hoveredCountdownCancel = false;
        if (m_countdownForRecording) {
            m_recordingToolsHidden = false;
            if (m_recWebcam && m_webcamDevice >= 0) {
                startWebcamCapture();
            }
        }
        update();
        return;
    }

    m_countdownValue--;
    if (m_countdownValue > 0) {
        update();
        m_countdownTimer->start();
        return;
    }

    // Countdown finished — proceed with capture/recording
    m_countdownActive = false;
    m_countdownValue = 0;
    m_hoveredCountdownCancel = false;
    releaseKeyboard();
    hide();
    QApplication::processEvents();
    QThread::msleep(120);
    QApplication::exit(0);
}

void CaptureOverlay::resetRecordingPanelToAreaMode(bool keepSelection)
{
    Q_UNUSED(keepSelection);
    m_recordingPanelOpen = false;
    m_micTimer->stop();
    m_recordingToolsHidden = false;
    m_settingsOpen = false;
    m_clickOptionsOpen = false;
    m_keystrokeOptionsOpen = false;
    m_cropMenuOpen = false;
    m_hoveredRecordTile = RecordPanelTile::None;
    m_hoveredCropMenuItem = -1;
    m_captureIntent = CaptureIntent::Area;
    m_recordType = RecordType::None;
    m_showKeystrokePreview = false;
    m_keyPreviews.clear();
    m_clickPreviews.clear();
    m_countdownCancelRequested = false;
    m_hoveredCountdownCancel = false;
    stopClickAnimTimer();
    stopWebcamCapture();
    m_micLevel = 0.0;
    m_speakerLevel = 0.0;
}

void CaptureOverlay::cancelSelection()
{
    m_recordConfigRequested = false;
    resetRecordingPanelToAreaMode();
    m_captureCropMenuOpen = false;
    m_hoveredCaptureCropMenuItem = -1;
    m_captureCropMenuPanelRect = QRectF();
    m_captureCropMenuItemRects.clear();
    exitScrollMode();
    if (m_countdownActive) {
        m_countdownActive = false;
        m_countdownValue = 0;
        m_countdownTimer->stop();
    }
    releaseKeyboard();
    QApplication::exit(1);
}

QRect CaptureOverlay::handleRect(const QPoint& center) const
{
    return QRect(center.x() - kHandleHitSize/2,
                 center.y() - kHandleHitSize/2,
                 kHandleHitSize, kHandleHitSize);
}

QList<QPoint> CaptureOverlay::handleCenters() const
{
    const QRect r = m_selection.normalized();
    int cx = r.left() + r.width()/2;
    int cy = r.top()  + r.height()/2;
    return { r.topLeft(),         QPoint(cx, r.top()),    r.topRight(),
             QPoint(r.right(),cy),
             r.bottomRight(),     QPoint(cx,r.bottom()),  r.bottomLeft(),
             QPoint(r.left(),cy) };
}

CaptureOverlay::HandlePos CaptureOverlay::hitTest(const QPoint& pos) const
{
    if (!m_hasSelection) return HandlePos::None;
    const QRect selection = m_selection.normalized();

    const QList<QPoint> centers = handleCenters();
    const HandlePos handles[] = {
        HandlePos::TopLeft,    HandlePos::Top,         HandlePos::TopRight,
        HandlePos::Right,
        HandlePos::BottomRight, HandlePos::Bottom,     HandlePos::BottomLeft,
        HandlePos::Left
    };
    for (int i = 0; i < 8; ++i)
        if (handleRect(centers[i]).contains(pos)) return handles[i];
    if (selection.contains(pos)) return HandlePos::Inside;
    return HandlePos::None;
}

// ── Recording panel hit testing ──────────────────────────────────────────────

CaptureOverlay::RecordPanelTile CaptureOverlay::hitTestRecordingPanel(const QPoint& pos) const
{
    if (!m_recordingPanelOpen) {
        return RecordPanelTile::None;
    }

    // Rect order must match the append order in drawRecordingPanel().
    static const RecordPanelTile tileOrder[] = {
        RecordPanelTile::Controls, RecordPanelTile::Size, RecordPanelTile::Crop,
        RecordPanelTile::Mic, RecordPanelTile::Speaker, RecordPanelTile::Webcam,
        RecordPanelTile::Click, RecordPanelTile::Keystrokes,
        RecordPanelTile::RecordVideo, RecordPanelTile::RecordGif
    };

    for (int i = 0; i < (int)m_recTileRects.size() && i < 10; ++i) {
        if (m_recTileRects[i].contains(pos)) {
            return tileOrder[i];
        }
    }

    return RecordPanelTile::None;
}

void CaptureOverlay::updateCursor(const QPoint& pos)
{
    if (!m_hasSelection) { setCursor(Qt::CrossCursor); return; }

    if (m_captureIntent == CaptureIntent::Scroll && m_scrollStage == ScrollStage::Capturing) {
        setCursor(Qt::ArrowCursor);
        return;
    }

    // Check recording panel tiles first
    if (m_recordingPanelOpen) {
        RecordPanelTile tile = hitTestRecordingPanel(pos);
        if (tile != RecordPanelTile::None) {
            setCursor(Qt::PointingHandCursor);
            return;
        }
        // Still allow resize handles when panel is open
        switch (hitTest(pos)) {
        case HandlePos::TopLeft:     setCursor(Qt::SizeFDiagCursor); break;
        case HandlePos::TopRight:    setCursor(Qt::SizeBDiagCursor); break;
        case HandlePos::BottomLeft:  setCursor(Qt::SizeBDiagCursor); break;
        case HandlePos::BottomRight: setCursor(Qt::SizeFDiagCursor); break;
        case HandlePos::Top:
        case HandlePos::Bottom:      setCursor(Qt::SizeVerCursor);   break;
        case HandlePos::Left:
        case HandlePos::Right:       setCursor(Qt::SizeHorCursor);   break;
        case HandlePos::Inside:      setCursor(Qt::OpenHandCursor);  break;
        default:                     setCursor(Qt::ArrowCursor);     break;
        }
        return;
    }

    const QRect sel = m_selection.normalized();
    ToolbarLayout layout = computeToolbarLayout(
        sel.x(),
        sel.y(),
        sel.width(),
        sel.height(),
        width(),
        height(),
        m_captureIntent == CaptureIntent::Scroll
    );
    if (m_captureCropMenuOpen) {
        for (int i = 0; i < m_captureCropMenuItemRects.size(); ++i) {
            if (m_captureCropMenuItemRects[i].contains(pos)) {
                setCursor(Qt::PointingHandCursor);
                return;
            }
        }
    }
    for (int i = 0; i < NUM_TOOLS; ++i) {
        if (layout.toolCells[i].contains(pos)) {
            setCursor(Qt::PointingHandCursor);
            return;
        }
    }
    if (layout.cropCard.contains(pos)) {
        setCursor(Qt::PointingHandCursor);
        return;
    }
    if (layout.sizeCard.contains(pos)) {
        setCursor(Qt::ArrowCursor);
        return;
    }

    if (m_captureIntent == CaptureIntent::Scroll) {
        if (m_scrollStage == ScrollStage::Armed && scrollPrimaryButtonRect().contains(pos)) {
            setCursor(Qt::PointingHandCursor);
            return;
        }
    }

    switch (hitTest(pos)) {
    case HandlePos::TopLeft:     setCursor(Qt::SizeFDiagCursor); break;
    case HandlePos::TopRight:    setCursor(Qt::SizeBDiagCursor); break;
    case HandlePos::BottomLeft:  setCursor(Qt::SizeBDiagCursor); break;
    case HandlePos::BottomRight: setCursor(Qt::SizeFDiagCursor); break;
    case HandlePos::Top:
    case HandlePos::Bottom:      setCursor(Qt::SizeVerCursor);   break;
    case HandlePos::Left:
    case HandlePos::Right:       setCursor(Qt::SizeHorCursor);   break;
    case HandlePos::Inside:      setCursor(Qt::OpenHandCursor);  break;
    default:                     setCursor(Qt::CrossCursor);     break;
    }
}
