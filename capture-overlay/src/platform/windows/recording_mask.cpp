#include "platform/windows/recording_mask.h"

#include "platform/platform.h"

#include <QGuiApplication>
#include <QPainter>
#include <QScreen>
#include <QShowEvent>

#include <algorithm>
#include <cmath>

namespace {

const QColor kDim(0, 0, 0, 150);
const QColor kBorder(255, 102, 0, 190);

QScreen* screenForRegion(const QRect& deviceRect)
{
    for (QScreen* screen : QGuiApplication::screens()) {
        const qreal dpr = screen->devicePixelRatio();
        const QRect logical(
            QPoint(qRound(deviceRect.x() / dpr), qRound(deviceRect.y() / dpr)),
            QSize(qRound(deviceRect.width() / dpr), qRound(deviceRect.height() / dpr)));
        if (screen->geometry().intersects(logical))
            return screen;
    }
    return QGuiApplication::primaryScreen();
}

} // namespace

RecordingMask::RecordingMask(const QRect& deviceRegion, QWidget* parent)
    : QWidget(parent)
{
    setWindowFlags(Qt::Tool
                   | Qt::FramelessWindowHint
                   | Qt::WindowStaysOnTopHint
                   | Qt::WindowTransparentForInput
                   | Qt::WindowDoesNotAcceptFocus);
    setAttribute(Qt::WA_TranslucentBackground);
    setAttribute(Qt::WA_ShowWithoutActivating);
    setAttribute(Qt::WA_TransparentForMouseEvents);
    setAttribute(Qt::WA_DeleteOnClose);

    QScreen* screen = screenForRegion(deviceRegion);
    if (!screen)
        screen = QGuiApplication::primaryScreen();
    if (!screen)
        return;

    const qreal dpr = screen->devicePixelRatio();
    const QRect logicalRegion(
        QPoint(qRound(deviceRegion.x() / dpr), qRound(deviceRegion.y() / dpr)),
        QSize(qRound(deviceRegion.width() / dpr), qRound(deviceRegion.height() / dpr)));

    setGeometry(screen->geometry());
    m_hole = logicalRegion.translated(-geometry().topLeft()).intersected(rect());
}

void RecordingMask::paintEvent(QPaintEvent*)
{
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing, false);

    if (m_hole.isEmpty()) {
        painter.fillRect(rect(), kDim);
        return;
    }

    if (m_hole.top() > 0)
        painter.fillRect(QRect(0, 0, width(), m_hole.top()), kDim);
    if (m_hole.bottom() < height() - 1)
        painter.fillRect(QRect(0, m_hole.bottom() + 1, width(), height() - m_hole.bottom() - 1),
                         kDim);
    if (m_hole.left() > 0)
        painter.fillRect(QRect(0, m_hole.top(), m_hole.left(), m_hole.height()), kDim);
    if (m_hole.right() < width() - 1)
        painter.fillRect(QRect(m_hole.right() + 1,
                               m_hole.top(),
                               width() - m_hole.right() - 1,
                               m_hole.height()),
                         kDim);

    painter.setPen(QPen(kBorder, 2.0));
    painter.setBrush(Qt::NoBrush);
    painter.drawRect(m_hole.adjusted(0, 0, -1, -1));
}

void RecordingMask::showEvent(QShowEvent* event)
{
    QWidget::showEvent(event);
    Platform::keepTopmost(static_cast<quintptr>(winId()));
    Platform::excludeFromCapture(static_cast<quintptr>(winId()));
}
