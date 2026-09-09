#include "CaptureOverlay.h"
#include "CaptureOverlay_p.h"
#include "CaptureOverlay_DrawingPrimitives_p.h"
#include <QPainter>
#include <QPainterPath>
#include <QPaintEvent>
#include <QFont>
#include <QFontMetrics>
#include <QImage>
#include <QPixmap>
#include <QColor>
#include <QLinearGradient>
#include <QRadialGradient>
#include <QPen>
#include <QDateTime>
#include <QCursor>
#include <QMutexLocker>
#include <QTimer>
#include <QRegion>
#include <algorithm>
#include <cmath>

QRect CaptureOverlay::crosshairBubbleRectForPoint(const QPoint& point) const
{
    const QRect widgetRect = rect();
    const QPoint guidePoint = widgetRect.contains(point) ? point : m_pointerPos;

    QString labelText;
    if (m_dragging || m_hasSelection) {
        const QRect sel = m_selection.normalized();
        labelText = QStringLiteral("%1 \u00D7 %2").arg(sel.width()).arg(sel.height());
    } else {
        labelText = QStringLiteral("%1, %2").arg(guidePoint.x()).arg(guidePoint.y());
    }
    QFont font(QStringLiteral("Sans"));
    font.setPixelSize(12);
    font.setWeight(QFont::Medium);
    const QFontMetrics fm(font);
    const QRect textRect = fm.boundingRect(labelText);

    const int paddingX = 10;
    const int paddingY = 6;
    const int bw = textRect.width() + paddingX * 2;
    const int bh = textRect.height() + paddingY * 2;

    const QRect bubbleRect(
        std::clamp(guidePoint.x() + 16, 8, widgetRect.width() - bw - 8),
        std::clamp(guidePoint.y() + 16, 8, widgetRect.height() - bh - 8),
        bw,
        bh);

    return bubbleRect.adjusted(-3, -3, 3, 3);
}

QRegion CaptureOverlay::crosshairDirtyRegion(const QPoint& oldPoint,
                                             const QPoint& newPoint,
                                             const QRect& oldSelection,
                                             const QRect& newSelection,
                                             bool hadSelection,
                                             bool hasSelection) const
{
    const QRect widgetRect = rect();
    QRegion dirty;
    constexpr int selectionPad = 4;

    if (!m_lastCrosshairBubbleRect.isNull()) {
        dirty += m_lastCrosshairBubbleRect;
    }
    dirty += crosshairBubbleRectForPoint(newPoint);

    if (hadSelection && !oldSelection.isNull()) {
        dirty += oldSelection.adjusted(-selectionPad, -selectionPad, selectionPad, selectionPad);
    }
    if (hasSelection && !newSelection.isNull()) {
        dirty += newSelection.adjusted(-selectionPad, -selectionPad, selectionPad, selectionPad);
    }

    return dirty.intersected(widgetRect);
}
