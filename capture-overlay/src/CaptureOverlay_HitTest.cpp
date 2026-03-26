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
    if (m_scrollCaptureTimer && m_scrollCaptureTimer->isActive()) {
        m_scrollCaptureTimer->stop();
    }

    if (m_timerDelayActive && m_captureDelaySeconds > 0 && !m_countdownActive) {
        m_countdownActive = true;
        for (int remaining = m_captureDelaySeconds; remaining > 0; --remaining) {
            m_countdownValue = remaining;
            update();
            QApplication::processEvents();
            QThread::sleep(1);
        }
        m_countdownActive = false;
        m_countdownValue = 0;
        update();
        QApplication::processEvents();
    }

    releaseKeyboard();
    hide();
    QApplication::processEvents();
    QThread::msleep(120);
    QApplication::exit(0);
}

void CaptureOverlay::confirmRecordingSelection()
{
    if (m_showCountdown && !m_countdownActive) {
        m_countdownActive = true;
        for (int remaining = 3; remaining > 0; --remaining) {
            m_countdownValue = remaining;
            update();
            QApplication::processEvents();
            QThread::sleep(1);
        }
        m_countdownActive = false;
        m_countdownValue = 0;
        update();
        QApplication::processEvents();
    }

    releaseKeyboard();
    hide();
    QApplication::processEvents();
    QThread::msleep(120);
    QApplication::exit(0);
}

void CaptureOverlay::cancelSelection()
{
    exitScrollMode();
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
    if (!selection.contains(pos)) return HandlePos::None;

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

    // Full panel: Controls, Size, Crop, Mic, Speaker, Record, Click, Keystrokes, RecordGif, RecordVideo
    static const RecordPanelTile tileOrder[] = {
        RecordPanelTile::Controls, RecordPanelTile::Size, RecordPanelTile::Crop,
        RecordPanelTile::Mic, RecordPanelTile::Speaker, RecordPanelTile::Webcam,
        RecordPanelTile::Click, RecordPanelTile::Keystrokes,
        RecordPanelTile::RecordGif, RecordPanelTile::RecordVideo
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
    for (int i = 0; i < NUM_TOOLS; ++i) {
        if (layout.itemCells[i].contains(pos)) {
            setCursor(Qt::PointingHandCursor);
            return;
        }
    }
    if (layout.sizePanel.contains(pos)) {
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
    case HandlePos::Inside:      setCursor(Qt::SizeAllCursor);   break;
    default:                     setCursor(Qt::CrossCursor);     break;
    }
}
