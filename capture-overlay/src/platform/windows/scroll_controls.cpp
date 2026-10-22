#include "platform/windows/scroll_controls.h"

#include "platform/platform.h"
#include "platform/windows/scroll_capture.h"

#include <QCloseEvent>
#include <QCursor>
#include <QFont>
#include <QGuiApplication>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QScreen>
#include <QShowEvent>
#include <QTimer>

#include <algorithm>
#include <cmath>

namespace {

constexpr int kWidth = 284;
constexpr int kHeight = 62;
constexpr int kMargin = 24;

const QColor kChrome(20, 20, 20);
const QColor kChromeRim(255, 255, 255, 26);
const QColor kSegment(30, 31, 34);
const QColor kText(245, 245, 246);
const QColor kMuted(160, 160, 165);
const QColor kHover(255, 255, 255, 22);
const QColor kSeparator(255, 255, 255, 20);
const QColor kAccent(255, 102, 0);
const QColor kAccentHover(255, 124, 32);

QFont uiFont(int pixelSize, bool bold)
{
    QFont font(QStringLiteral("Inter"));
    font.setPixelSize(pixelSize);
    font.setWeight(bold ? QFont::DemiBold : QFont::Normal);
    return font;
}

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

void drawScrollGlyph(QPainter& painter, const QPointF& center)
{
    painter.setPen(QPen(kMuted, 1.6, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
    painter.setBrush(Qt::NoBrush);
    painter.drawRoundedRect(
        QRectF(center.x() - 7.0, center.y() - 10.0, 14.0, 20.0), 6.0, 6.0);
    painter.drawLine(QPointF(center.x(), center.y() - 10.0),
                     QPointF(center.x(), center.y() - 5.0));
    painter.drawLine(QPointF(center.x(), center.y() + 5.0),
                     QPointF(center.x(), center.y() + 10.0));
}

void drawTrashIcon(QPainter& painter, const QRectF& rect)
{
    painter.setPen(QPen(kText, 1.6, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
    painter.setBrush(Qt::NoBrush);
    const QPointF c = rect.center();
    painter.drawRoundedRect(QRectF(c.x() - 5.5, c.y() - 3.0, 11.0, 12.5), 1.5, 1.5);
    painter.drawLine(QPointF(c.x() - 8.0, c.y() - 5.0), QPointF(c.x() + 8.0, c.y() - 5.0));
    painter.drawRoundedRect(QRectF(c.x() - 2.5, c.y() - 7.5, 5.0, 2.5), 0.8, 0.8);
    painter.drawLine(QPointF(c.x() - 2.0, c.y() - 1.0), QPointF(c.x() - 2.0, c.y() + 6.0));
    painter.drawLine(QPointF(c.x() + 2.0, c.y() - 1.0), QPointF(c.x() + 2.0, c.y() + 6.0));
}

} // namespace

ScrollControls::ScrollControls(Platform::ScrollCaptureSession* session,
                               const QRect& captureRegion,
                               QWidget* parent)
    : QWidget(parent)
    , m_session(session)
    , m_captureRegion(captureRegion)
{
    setWindowFlags(Qt::Tool | Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint);
    setAttribute(Qt::WA_TranslucentBackground);
    setAttribute(Qt::WA_ShowWithoutActivating);
    setAttribute(Qt::WA_DeleteOnClose);
    setMouseTracking(true);
    setCursor(Qt::ArrowCursor);
    setFixedSize(kWidth, kHeight);

    connect(m_session, &Platform::ScrollCaptureSession::statusChanged, this,
            [this](const QString& status) {
                m_status = status;
                update();
            });
    connect(m_session, &Platform::ScrollCaptureSession::frameCaptured, this,
            [this](int frames, int) {
                m_frames = frames;
                update();
            });
    connect(m_session,
            &Platform::ScrollCaptureSession::finished,
            this,
            [this](bool, const QImage&, const QString&) {
                m_finished = true;
                close();
            });
}

QRectF ScrollControls::segmentRect() const
{
    return QRectF(6, 5, 150, 52);
}

QRectF ScrollControls::doneRect() const
{
    return QRectF(162, 14, 70, 34);
}

QRectF ScrollControls::discardRect() const
{
    return QRectF(238, 5, 40, 52);
}

ScrollControls::Button ScrollControls::hitTest(const QPoint& point) const
{
    if (doneRect().contains(point))
        return Button::Done;
    if (discardRect().contains(point))
        return Button::Discard;
    return Button::Body;
}

void ScrollControls::paintEvent(QPaintEvent*)
{
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing, true);

    const QRectF chrome = QRectF(rect()).adjusted(0.5, 0.5, -0.5, -0.5);
    QPainterPath shadow;
    shadow.addRoundedRect(chrome.translated(0, 3), 12, 12);
    painter.fillPath(shadow, QColor(0, 0, 0, 105));

    QPainterPath chromePath;
    chromePath.addRoundedRect(chrome, 12, 12);
    painter.fillPath(chromePath, kChrome);
    painter.setPen(QPen(kChromeRim, 1.0));
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(chromePath);

    const QRectF segment = segmentRect();
    painter.setPen(Qt::NoPen);
    painter.setBrush(kSegment);
    painter.drawRoundedRect(segment, 8, 8);

    drawScrollGlyph(painter, QPointF(27.0, segment.center().y()));

    painter.setFont(uiFont(9, true));
    painter.setPen(kMuted);
    painter.drawText(QRectF(44.0, 10.0, 106.0, 12.0),
                     Qt::AlignLeft | Qt::AlignVCenter,
                     QStringLiteral("SCROLL"));

    painter.setFont(uiFont(15, true));
    painter.setPen(m_closing ? kMuted : kText);
    const QString label = m_closing
        ? QStringLiteral("Stitching...")
        : (m_frames <= 0 ? QStringLiteral("Starting...")
                         : QStringLiteral("%1 frames").arg(m_frames));
    painter.drawText(QRectF(44.0, 24.0, 106.0, 22.0),
                     Qt::AlignLeft | Qt::AlignVCenter,
                     label);

    painter.setPen(QPen(kSeparator, 1.0));
    painter.drawLine(QPointF(158.5, 18.0), QPointF(158.5, kHeight - 18.0));
    painter.drawLine(QPointF(234.5, 18.0), QPointF(234.5, kHeight - 18.0));

    const QRectF done = doneRect();
    painter.setPen(Qt::NoPen);
    painter.setBrush(m_closing
                         ? QColor(70, 70, 76)
                         : (m_hovered == Button::Done ? kAccentHover : kAccent));
    painter.drawRoundedRect(done, 10, 10);
    painter.setFont(uiFont(12, true));
    painter.setPen(m_closing ? kMuted : Qt::white);
    painter.drawText(done, Qt::AlignCenter, QStringLiteral("Done"));

    if (m_hovered == Button::Discard && !m_closing) {
        painter.setPen(Qt::NoPen);
        painter.setBrush(kHover);
        painter.drawRoundedRect(discardRect().adjusted(2, 2, -2, -2), 10, 10);
    }
    drawTrashIcon(painter, discardRect());
}

void ScrollControls::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) {
        QWidget::mousePressEvent(event);
        return;
    }

    switch (hitTest(event->pos())) {
    case Button::Done:
        if (m_closing)
            return;
        m_closing = true;
        m_session->stop();
        update();
        return;
    case Button::Discard:
        if (m_closing)
            return;
        m_closing = true;
        m_session->discard();
        update();
        return;
    case Button::Body:
        m_dragging = true;
        m_dragOffset = event->globalPos() - frameGeometry().topLeft();
        return;
    case Button::None:
        return;
    }
}

void ScrollControls::mouseMoveEvent(QMouseEvent* event)
{
    if (m_dragging) {
        move(event->globalPos() - m_dragOffset);
        return;
    }

    const Button hovered = hitTest(event->pos());
    if (hovered != m_hovered) {
        m_hovered = hovered;
        setCursor(hovered == Button::Body ? Qt::SizeAllCursor : Qt::PointingHandCursor);
        update();
    }
}

void ScrollControls::mouseReleaseEvent(QMouseEvent* event)
{
    if (event->button() == Qt::LeftButton)
        m_dragging = false;
}

void ScrollControls::leaveEvent(QEvent* event)
{
    m_hovered = Button::None;
    update();
    QWidget::leaveEvent(event);
}

void ScrollControls::showEvent(QShowEvent* event)
{
    QWidget::showEvent(event);
    positionOnScreen();
    Platform::keepTopmost(static_cast<quintptr>(winId()));
    Platform::excludeFromCapture(static_cast<quintptr>(winId()));

    auto* topmost = new QTimer(this);
    topmost->setInterval(2000);
    connect(topmost, &QTimer::timeout, this, [this] {
        if (isVisible())
            Platform::keepTopmost(static_cast<quintptr>(winId()));
    });
    topmost->start();
}

void ScrollControls::positionOnScreen()
{
    QScreen* screen = screenForRegion(m_captureRegion);
    if (!screen)
        screen = QGuiApplication::primaryScreen();
    const QRect available = screen ? screen->availableGeometry() : QRect(0, 0, 1920, 1080);
    const qreal dpr = screen ? screen->devicePixelRatio() : 1.0;

    const QRect region(
        QPoint(qRound(m_captureRegion.x() / dpr), qRound(m_captureRegion.y() / dpr)),
        QSize(qRound(m_captureRegion.width() / dpr), qRound(m_captureRegion.height() / dpr)));

    const bool fullscreen =
        region.width() >= available.width() - 8 && region.height() >= available.height() - 8;

    int x = 0;
    int y = 0;
    if (fullscreen || region.isEmpty()) {
        x = available.center().x() - width() / 2;
        y = available.top() + kMargin;
    } else {
        constexpr int gap = 12;
        const int minY = available.top() + kMargin;
        const int maxY = available.bottom() - height() - kMargin;

        x = region.center().x() - width() / 2;
        y = region.bottom() + 1 + gap;
        if (y > maxY) {
            const int above = region.top() - gap - height();
            y = above >= minY ? above : region.top() + kMargin;
        }

        x = std::max(available.left() + kMargin,
                     std::min(x, available.right() - width() - kMargin));
        y = std::max(minY, std::min(y, maxY));
    }
    move(x, y);
}

void ScrollControls::closeEvent(QCloseEvent* event)
{
    if (!m_finished && m_session->isActive())
        m_session->stop();
    QWidget::closeEvent(event);
}
