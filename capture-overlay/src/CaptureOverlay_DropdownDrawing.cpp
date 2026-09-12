#include "CaptureOverlay.h"

#include <QColor>
#include <QFont>
#include <QPainter>
#include <QPen>

#include <algorithm>

void CaptureOverlay::drawDropdownPopup(QPainter& p, const QRectF& anchorRect,
                                       const QStringList& options, int selectedIndex)
{
    if (options.isEmpty()) return;

    p.save();
    p.setRenderHint(QPainter::Antialiasing);

    const double itemH = 34.0;
    const double menuW = std::max(anchorRect.width(), 160.0);
    const double menuH = options.size() * itemH + 10.0;

    double menuX = anchorRect.right() - menuW;
    double menuY = anchorRect.bottom() + 4.0;

    // Check screen bounds
    if (menuX + menuW > width() - 10) menuX = width() - menuW - 10;
    if (menuY + menuH > height() - 10) menuY = anchorRect.top() - menuH - 4.0;

    QRectF menuRect(menuX, menuY, menuW, menuH);
    const QColor accentColor(176, 92, 56);

    // Background
    p.setPen(QPen(QColor(255, 255, 255, 31), 1));
    p.setBrush(QColor(20, 20, 20, 250));
    p.drawRoundedRect(menuRect, 8, 8);

    m_dropdownItemRects.clear();
    const bool hasColors = !m_dropdownColors.isEmpty();
    for (int i = 0; i < options.size(); ++i) {
        QRectF itemRect(menuX + 5, menuY + 5 + i * itemH, menuW - 10, itemH);
        m_dropdownItemRects.append(itemRect);

        bool hovered = (m_hoveredDropdownItem == i);
        if (hovered) {
            p.setPen(Qt::NoPen);
            p.setBrush(QColor(255, 255, 255, 20));
            p.drawRoundedRect(itemRect, 7, 7);
        }

        if (selectedIndex == i) {
            p.setPen(Qt::NoPen);
            p.setBrush(QColor(accentColor.red(), accentColor.green(), accentColor.blue(), 28));
            p.drawRoundedRect(itemRect, 7, 7);
        }

        // Left-aligned content
        double textX = itemRect.x() + 10;

        // Color circle (left side)
        if (hasColors && i < m_dropdownColors.size()) {
            p.setPen(Qt::NoPen);
            p.setBrush(m_dropdownColors[i]);
            p.drawEllipse(QPointF(itemRect.x() + 18, itemRect.center().y()), 7, 7);
            textX = itemRect.x() + 34;
        }

        p.setPen(selectedIndex == i ? QColor(255, 236, 220) : Qt::white);
        p.setFont(QFont("Sans", 10, selectedIndex == i ? QFont::Bold : QFont::Normal));
        p.drawText(QRectF(textX, itemRect.y(), itemRect.right() - textX - 10, itemRect.height()),
                   Qt::AlignLeft | Qt::AlignVCenter, options[i]);

        if (selectedIndex == i) {
            p.setPen(QPen(accentColor, 1.6, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
            const double cx = itemRect.right() - 16.0;
            const double cy = itemRect.center().y();
            p.drawLine(QPointF(cx - 4.0, cy), QPointF(cx - 1.0, cy + 3.0));
            p.drawLine(QPointF(cx - 1.0, cy + 3.0), QPointF(cx + 5.0, cy - 4.0));
        }
    }
    p.restore();
}
