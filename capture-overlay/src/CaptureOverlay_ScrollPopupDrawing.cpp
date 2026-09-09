#include "CaptureOverlay.h"
#include "CaptureOverlay_DrawingPrimitives_p.h"

#include <QColor>
#include <QFont>
#include <QImage>
#include <QLinearGradient>
#include <QPainter>
#include <QPainterPath>
#include <QPen>
#include <QRadialGradient>
#include <QtGlobal>

#include <algorithm>

namespace {

} // namespace

// ── Scroll Capture Popup ──────────────────────────────────────────────────
// Mirrors draw_scroll_popup from src/overlay/drawing.rs

void CaptureOverlay::drawScrollPopup(QPainter& p, double centerX, double centerY)
{
    if (!m_scrollPopupOpen) return;

    const double popupW = 360.0;
    const double popupH = 170.0;
    const double scrW = width();
    const double scrH = height();
    const double popupX = qBound(10.0, centerX - popupW / 2.0, scrW - popupW - 10.0);
    const double popupY = qBound(10.0, centerY - popupH / 2.0, scrH - popupH - 10.0);

    const QColor accentColor(176, 92, 56);

    // Warm radial glow
    {
        QRadialGradient glow(popupX + popupW / 2.0, popupY + popupH / 2.0, popupW / 2.0);
        glow.setColorAt(0, QColor(accentColor.red(), accentColor.green(), accentColor.blue(), 40));
        glow.setColorAt(0.6, QColor(0, 0, 0, 0));
        p.fillRect(QRectF(popupX - 40, popupY - 40, popupW + 80, popupH + 80), glow);
    }

    drawFrostedPanel(p, popupX, popupY, popupW, popupH, 12.0, m_blurredBg.isNull() ? nullptr : &m_blurredBg, scrW, scrH);

    // Close button
    const double closeSize = 22.0;
    const double closeX = popupX + popupW - closeSize - 10.0;
    const double closeY = popupY + 10.0;
    if (m_hoveredScrollClose) {
        p.setPen(Qt::NoPen);
        p.setBrush(QColor(204, 64, 38, 255));
    } else {
        p.setPen(Qt::NoPen);
        p.setBrush(QColor(60, 60, 60, 255));
    }
    p.drawRoundedRect(QRectF(closeX, closeY, closeSize, closeSize), 5, 5);

    // X mark
    p.setPen(QPen(QColor(255, 255, 255, 255), 1.5, Qt::SolidLine, Qt::RoundCap));
    p.drawLine(QPointF(closeX + 6, closeY + 6),
               QPointF(closeX + closeSize - 6, closeY + closeSize - 6));
    p.drawLine(QPointF(closeX + closeSize - 6, closeY + 6),
               QPointF(closeX + 6, closeY + closeSize - 6));

    // Title
    QFont titleFont("Sans", 13, QFont::Bold);
    p.setFont(titleFont);
    p.setPen(QColor(255, 255, 255, 255));
    p.drawText(QRectF(popupX + 20, popupY + 24, popupW - 60, 20), Qt::AlignLeft | Qt::AlignVCenter, "Scroll Capture");

    // Body text
    QFont bodyFont("Sans", 12);
    p.setFont(bodyFont);
    p.setPen(QColor(255, 255, 255, 180));
    p.drawText(QRectF(popupX + 20, popupY + 55, popupW - 40, 18), Qt::AlignLeft | Qt::AlignVCenter, "Scroll capture requires the ApexShot");
    p.drawText(QRectF(popupX + 20, popupY + 73, popupW - 40, 18), Qt::AlignLeft | Qt::AlignVCenter, "browser extension.");

    // CTA button — orange gradient, matching Rust overlay
    const double btnW = 182.0;
    const double btnH = 34.0;
    const double btnX = popupX + (popupW - btnW) / 2.0;
    const double btnY = popupY + 102.0;

    // Button shadow
    p.setPen(Qt::NoPen);
    p.setBrush(QColor(0, 0, 0, 56));
    p.drawRoundedRect(QRectF(btnX, btnY + 1.5, btnW, btnH), 10, 10);

    // Button gradient
    QLinearGradient btnGrad(0, btnY, 0, btnY + btnH);
    btnGrad.setColorAt(0.0, QColor(242, 116, 70, 245));
    btnGrad.setColorAt(1.0, QColor(176, 92, 56, 240));
    p.setBrush(btnGrad);
    p.drawRoundedRect(QRectF(btnX, btnY, btnW, btnH), 10, 10);

    // Button border
    p.setPen(QPen(QColor(255, 224, 196, 87), 1.0));
    p.setBrush(Qt::NoBrush);
    p.drawRoundedRect(QRectF(btnX, btnY, btnW, btnH), 10, 10);

    // Button text
    QFont btnFont("Sans", 13, QFont::Bold);
    p.setFont(btnFont);
    p.setPen(QColor(255, 255, 255, 255));
    p.drawText(QRectF(btnX, btnY, btnW, btnH), Qt::AlignCenter, "Download Extension");

    // Cache layout rects for hit testing
    m_scrollPopupRect = QRectF(popupX, popupY, popupW, popupH);
    m_scrollCloseRect = QRectF(closeX, closeY, closeSize, closeSize);
    m_scrollDownloadBtnRect = QRectF(btnX, btnY, btnW, btnH);
}
