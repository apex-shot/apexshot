#include "CaptureOverlay.h"
#include "CaptureOverlay_DrawingPrimitives_p.h"
#include "CaptureOverlay_p.h"

#include <QColor>
#include <QCursor>
#include <QDateTime>
#include <QFont>
#include <QFontMetrics>
#include <QImage>
#include <QLinearGradient>
#include <QMutexLocker>
#include <QPaintEvent>
#include <QPainter>
#include <QPainterPath>
#include <QPen>
#include <QPixmap>
#include <QRadialGradient>
#include <QRegion>
#include <QTimer>

#include <algorithm>
#include <cmath>

void CaptureOverlay::paintEvent(QPaintEvent* event)
{
    QPainter p(this);
    if (event) {
        p.setClipRegion(event->region());
    }
    p.setRenderHint(QPainter::TextAntialiasing);

    const QRect widgetRect = rect();
    const double sw = widgetRect.width();
    const double sh = widgetRect.height();

    if (isCrosshairMode()) {
        if (!m_background.isNull()) {
            p.drawPixmap(widgetRect, m_background);
        }

        const QPoint guidePoint = widgetRect.contains(m_pointerPos)
            ? m_pointerPos
            : m_lastCrosshairPaintPoint;

        if (m_dragging || m_hasSelection) {
            const QRect sel = m_selection.normalized();
            p.save();
            // Distinct orange border for the selection area
            p.setPen(QPen(QColor(255, 102, 0, 240), 2.0));
            // Subtle orange fill so it's visible on white backgrounds during capture
            p.setBrush(QColor(255, 102, 0, 40));
            p.drawRect(sel.adjusted(0, 0, -1, -1));
            p.restore();
        }

        // ── Clean, native-looking size/position bubble ──────────────────────────────
        QString labelText;
        if (m_dragging || m_hasSelection) {
            const QRect sel = m_selection.normalized();
            // Use proper multiplication sign
            labelText = QStringLiteral("%1 \u00D7 %2").arg(sel.width()).arg(sel.height());
        } else {
            labelText = QStringLiteral("%1, %2").arg(guidePoint.x()).arg(guidePoint.y());
        }

        static const QFont crosshairBubbleFont = []() {
            QFont font(QStringLiteral("Sans"));
            font.setPixelSize(12);
            font.setWeight(QFont::Medium);
            return font;
        }();
        p.setFont(crosshairBubbleFont);

        const QRect bubbleRect = crosshairBubbleRectForPoint(guidePoint).adjusted(3, 3, -3, -3);

        p.save();
        p.setRenderHint(QPainter::Antialiasing);

        // Standard semi-transparent dark background
        QPainterPath bubble;
        bubble.addRoundedRect(bubbleRect, 6, 6);
        p.fillPath(bubble, QColor(0, 0, 0, 180));

        // Very subtle white border to define the edge against dark backgrounds
        p.setPen(QPen(QColor(255, 255, 255, 40), 1.0));
        p.setBrush(Qt::NoBrush);
        p.drawPath(bubble);

        // Crisp white text
        p.setPen(QColor(255, 255, 255));
        p.drawText(bubbleRect, Qt::AlignCenter, labelText);

        p.restore();
        return;
    }

    p.setRenderHint(QPainter::Antialiasing);

    // ── Background ────────────────────────────────────────────────────────────
    if (!m_background.isNull()) {
        p.drawPixmap(widgetRect, m_background);
    } else {
        p.fillRect(widgetRect, QColor(0, 0, 0, 51)); // 0.20 alpha
    }

    // ── Window picker mode (in-overlay modal, no separate process) ────────────
    if (m_windowMode) {
        drawWindowPickerMode(p, widgetRect);
        return;
    }

    if (!m_hasSelection) {
        // Hint text
        p.fillRect(widgetRect, QColor(0, 0, 0, 30));
        QFont f; f.setPointSize(13); p.setFont(f);
        QString hint = "Drag to select an area  •  ESC to cancel";
        QFontMetrics fm(f);
        QRect tr = fm.boundingRect(hint);
        tr.moveCenter(widgetRect.center() + QPoint(0, 40));
        QPainterPath pill; pill.addRoundedRect(tr.adjusted(-14,-8,14,8), 10, 10);
        p.fillPath(pill, QColor(0,0,0,130));
        p.setPen(QColor(255,255,255,200));
        p.drawText(tr, Qt::AlignCenter, hint);
        return;
    }

    const QRect sel = m_selection.normalized();
    const double sx = sel.x(), sy = sel.y(), selW = sel.width(), selH = sel.height();

    // ── Dim outside selection (skip in fullscreen mode) ──────────────────────
    if (!m_fullscreenMode) {
        const QColor dim(0, 0, 0, 140);
        if (sy > 0)           p.fillRect(QRect(0, 0, widgetRect.width(), sy), dim);
        if (sel.bottom() < widgetRect.height()-1)
                              p.fillRect(QRect(0, sel.bottom()+1, widgetRect.width(),
                                               widgetRect.height()-sel.bottom()-1), dim);
        if (sx > 0)           p.fillRect(QRect(0, sy, sx, selH), dim);
        if (sel.right() < widgetRect.width()-1)
                              p.fillRect(QRect(sel.right()+1, sy,
                                               widgetRect.width()-sel.right()-1, selH), dim);
    } else {
        // Fullscreen mode: very subtle vignette to indicate full screen is selected
        p.fillRect(widgetRect, QColor(0, 0, 0, 26));
    }

    // ── Reveal selection area (repaint background there sharp) ────────────────
    if (!m_background.isNull()) {
        p.drawPixmap(sel, m_background, sel);
    } else {
        // No background pixmap — punch the selection area fully transparent so
        // the real screen content shows through without any dark tint.
        p.setCompositionMode(QPainter::CompositionMode_Clear);
        p.fillRect(sel, Qt::transparent);
        p.setCompositionMode(QPainter::CompositionMode_SourceOver);
    }

    // Add a subtle orange tint during active drag/resize for better feedback
    if (m_dragging || m_moving || m_resizing != HandlePos::None) {
        p.fillRect(sel, QColor(255, 102, 0, 30));
    }
    if (m_countdownActive && m_windowSelectionCapture) {
        p.fillRect(sel, QColor(12, 12, 14, 82));
    }

    // ── Selection handles ─────────────────────────────────────────────────────
    {
        const bool scrollModeActive = (m_captureIntent == CaptureIntent::Scroll);
        if (scrollModeActive) {
            if (m_scrollStage == ScrollStage::Capturing) {
                p.save();
                QRegion outside(widgetRect);
                p.setClipRegion(outside.subtracted(QRegion(sel)), Qt::ReplaceClip);
                p.setPen(QPen(QColor(255, 255, 255, 220), 2.0));
                p.setBrush(Qt::NoBrush);
                p.drawRect(sel.adjusted(-2, -2, 1, 1));
                p.restore();
            } else {
                p.setPen(QPen(QColor(255, 255, 255, 210), 1.6));
                p.setBrush(Qt::NoBrush);
                p.drawRect(sel.adjusted(0, 0, -1, -1));

                if (m_scrollStage == ScrollStage::Armed) {
                    p.setPen(QPen(QColor(22, 22, 24, 230), 1.2));
                    p.setBrush(QColor(255, 255, 255, 248));
                    for (const QPoint& center : handleCenters()) {
                        p.drawEllipse(QPointF(center.x(), center.y()),
                                      SCROLL_HANDLE_DOT_RADIUS,
                                      SCROLL_HANDLE_DOT_RADIUS);
                    }
                }
            }
        } else {
            double half = HANDLE_MARKER_LENGTH / 2.0;
            // Use orange for handles to match brand and ensure visibility on white
            p.setPen(QPen(QColor(255, 102, 0, 245), HANDLE_MARKER_THICKNESS,
                          Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));

            // Corners
            auto corner = [&](double ex, double ey, double dx, double dy) {
                QPainterPath path;
                path.moveTo(ex, ey + dy * half); path.lineTo(ex, ey); path.lineTo(ex + dx * half, ey);
                p.drawPath(path);
            };
            corner(sx,        sy,        +1, +1);
            corner(sx+selW,   sy,        -1, +1);
            corner(sx,        sy+selH,   +1, -1);
            corner(sx+selW,   sy+selH,   -1, -1);

            // Edge midpoints
            p.drawLine(QPointF(sx + selW/2 - half, sy),      QPointF(sx + selW/2 + half, sy));
            p.drawLine(QPointF(sx + selW/2 - half, sy+selH), QPointF(sx + selW/2 + half, sy+selH));
            p.drawLine(QPointF(sx, sy + selH/2 - half),      QPointF(sx, sy + selH/2 + half));
            p.drawLine(QPointF(sx+selW, sy + selH/2 - half), QPointF(sx+selW, sy + selH/2 + half));
        }
    }

    // ── Toolbar (hide when recording panel is open) ────────────────────────────
    if (!m_recordingPanelOpen) {
        drawToolbar(p, sx, sy, selW, selH, sw, sh);
    } else if (!m_recordingToolsHidden) {
        // Draw recording panel inside selection
        drawRecordingPanel(p, sx, sy, selW, selH);
    }

    // ── Visible countdown overlay ───────────────────────────────────────────
    if (m_countdownActive && m_countdownValue > 0) {
        p.save();
        p.setRenderHint(QPainter::Antialiasing);

        if (!m_countdownForRecording) {
            // Capture-delay countdown: pill badge at top-center
            const double pillW = 112.0;
            const double pillH = 44.0;
            const double pillX = (sw - pillW) / 2.0;
            const double pillY = 28.0;
            const QRectF pillRect(pillX, pillY, pillW, pillH);
            m_countdownBubbleRect = pillRect;

            p.setPen(Qt::NoPen);
            p.setBrush(m_hoveredCountdownCancel
                           ? QColor(200, 60, 40, 242)
                           : QColor(233, 84, 32, 235)); // #E95420 with alpha
            p.drawRoundedRect(pillRect, pillH / 2.0, pillH / 2.0);

            // Draw timer icon (clock face) on the left side of the pill
            const double iconCx = pillX + 22.0;
            const double iconCy = pillY + pillH / 2.0;
            const double iconR = 11.0;
            p.setPen(QPen(Qt::white, 2.2, Qt::SolidLine, Qt::RoundCap));
            p.setBrush(Qt::NoBrush);
            p.drawEllipse(QPointF(iconCx, iconCy), iconR, iconR);
            // Clock hands — small hour hand pointing up-ish
            p.drawLine(QPointF(iconCx, iconCy), QPointF(iconCx, iconCy - 5.5));
            // Minute hand pointing right-ish
            p.drawLine(QPointF(iconCx, iconCy), QPointF(iconCx + 5.0, iconCy + 2.0));

            // Draw countdown number on the right side
            QFont countdownFont(QStringLiteral("Sans"));
            countdownFont.setBold(true);
            countdownFont.setPointSizeF(m_hoveredCountdownCancel ? 13.0 : 22.0);
            p.setFont(countdownFont);
            p.setPen(Qt::white);
            p.setBrush(Qt::NoBrush);

            const QRectF textRect(pillX + 40.0, pillY, pillW - 44.0, pillH);
            p.drawText(textRect,
                       Qt::AlignCenter,
                       m_hoveredCountdownCancel ? QStringLiteral("Cancel")
                                                : QString::number(m_countdownValue));
        } else {
            // Recording countdown: centered circle (3-2-1)
            const double bubbleSize = 184.0;
            const double bubbleX = (sw - bubbleSize) / 2.0;
            const double bubbleY = (sh - bubbleSize) / 2.0;
            const QRectF bubbleRect(bubbleX, bubbleY, bubbleSize, bubbleSize);
            m_countdownBubbleRect = bubbleRect;

            p.setPen(Qt::NoPen);
            p.setBrush(m_hoveredCountdownCancel
                           ? QColor(132, 38, 24, 242)
                           : QColor(0, 0, 0, 240));
            p.drawEllipse(bubbleRect);

            QFont countdownFont(QStringLiteral("Sans"));
            countdownFont.setBold(true);
            countdownFont.setPointSizeF(m_hoveredCountdownCancel ? 34.0 : 72.0);
            p.setFont(countdownFont);
            p.setPen(m_hoveredCountdownCancel ? QColor(255, 228, 214) : Qt::white);

            p.drawText(bubbleRect,
                       Qt::AlignCenter,
                       m_hoveredCountdownCancel ? QStringLiteral("Cancel")
                                                : QString::number(m_countdownValue));
        }

        p.restore();
    } else {
        m_countdownBubbleRect = QRectF();
    }

    // ── Volume popup (Mic / Speaker) ─────────────────────────────────────
    if (m_recordingPanelOpen) {
        const auto sel = m_selection.normalized();
        const double sx = sel.x(), sy = sel.y(), selW = sel.width(), selH = sel.height();
        constexpr double popupW = 64.0;
        constexpr double popupH = 184.0;
        constexpr double popupGap = 12.0;
        const double maxX = std::max(10.0, width() - popupW - 10.0);
        const double maxY = std::max(10.0, height() - popupH - 10.0);
        const double rightX = sx + selW + popupGap;
        const double leftX = sx - popupGap - popupW;
        const double popupX = rightX <= maxX
            ? rightX
            : (leftX >= 10.0 ? leftX : qBound(10.0, rightX, maxX));
        const double popupY = qBound(10.0, sy + (selH - popupH) / 2.0, maxY);

        if (m_micVolumePopupOpen) {
            drawVolumePopup(p, popupX, popupY, m_micVolume, true, true);
        } else {
            m_volumePopupRect = QRectF();
        }
        if (m_speakerVolumePopupOpen) {
            drawVolumePopup(p, popupX, popupY, m_speakerVolume, false, true);
        } else if (!m_micVolumePopupOpen) {
            m_volumePopupRect = QRectF();
        }
    }

    // ── Scroll capture popup ──────────────────────────────────────────────
    if (m_scrollPopupOpen) {
        // Center on selection area when available, otherwise on screen
        // (matches Rust overlay behavior)
        double cx, cy;
        if (m_hasSelection) {
            const auto sel = m_selection.normalized();
            cx = sel.x() + sel.width() / 2.0;
            cy = sel.y() + sel.height() / 2.0;
        } else {
            cx = width() / 2.0;
            cy = height() / 2.0;
        }
        drawScrollPopup(p, cx, cy);
    }
}
