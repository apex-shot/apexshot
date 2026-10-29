#include "platform/windows/webcam_bubble.h"

#include "platform/platform.h"

#include <QGuiApplication>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QScreen>
#include <QShowEvent>
#include <QTimer>

#include <algorithm>
#include <cmath>
#include <utility>

namespace Platform {

WebcamBubble::WebcamBubble(int shape, bool flip, QWidget* parent)
    : QWidget(parent, Qt::Tool | Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint
                           | Qt::WindowDoesNotAcceptFocus | Qt::NoDropShadowWindowHint)
    , m_shape(std::clamp(shape, 0, 3))
    , m_flip(flip)
{
    setAttribute(Qt::WA_TranslucentBackground);
    setAttribute(Qt::WA_ShowWithoutActivating);
    setFocusPolicy(Qt::NoFocus);
    setCursor(Qt::OpenHandCursor);
    setWindowTitle(QStringLiteral("ApexShot webcam"));
}

WebcamBubble::~WebcamBubble() = default;

void WebcamBubble::setFrame(const QImage& frame)
{
    m_frame = frame;
    update();
}

QScreen* WebcamBubble::screenForDevicePoint(const QPoint& point) const
{
    // The rects are in device pixels, so each screen has to be compared in its
    // own device pixels rather than through Qt's logical geometry.
    for (QScreen* candidate : QGuiApplication::screens()) {
        const qreal candidateDpr = candidate->devicePixelRatio();
        const QRect logical = candidate->geometry();
        const QRect device(QPoint(int(std::lround(logical.x() * candidateDpr)),
                                  int(std::lround(logical.y() * candidateDpr))),
                           QSize(int(std::lround(logical.width() * candidateDpr)),
                                 int(std::lround(logical.height() * candidateDpr))));
        if (device.contains(point))
            return candidate;
    }
    if (QScreen* screen = QGuiApplication::screenAt(point))
        return screen;
    return QGuiApplication::primaryScreen();
}

QRect WebcamBubble::logicalRectFor(const QRect& deviceRect) const
{
    QScreen* screen = screenForDevicePoint(deviceRect.center());
    const qreal dpr = screen ? screen->devicePixelRatio() : 1.0;
    // The capture region and the rect handed in are in device pixels; the
    // window is placed in the compositor's logical pixels.
    return QRect(int(std::lround(deviceRect.x() / dpr)),
                 int(std::lround(deviceRect.y() / dpr)),
                 std::max(1, int(std::lround(deviceRect.width() / dpr))),
                 std::max(1, int(std::lround(deviceRect.height() / dpr))));
}

QRect WebcamBubble::deviceRectFor(const QRect& logicalRect) const
{
    QScreen* screen = QGuiApplication::screenAt(logicalRect.center());
    if (!screen)
        screen = QGuiApplication::primaryScreen();
    const qreal dpr = screen ? screen->devicePixelRatio() : 1.0;
    return QRect(int(std::lround(logicalRect.x() * dpr)),
                 int(std::lround(logicalRect.y() * dpr)),
                 std::max(1, int(std::lround(logicalRect.width() * dpr))),
                 std::max(1, int(std::lround(logicalRect.height() * dpr))));
}

void WebcamBubble::placeAt(const QRect& deviceRect)
{
    if (deviceRect.isEmpty())
        return;
    setGeometry(logicalRectFor(deviceRect));
}

void WebcamBubble::setMoveBounds(const QRect& deviceRegion)
{
    m_bounds = deviceRegion.isEmpty() ? QRect() : logicalRectFor(deviceRegion);
}

void WebcamBubble::setMovedHandler(std::function<void(const QRect&)> handler)
{
    m_moved = std::move(handler);
}

void WebcamBubble::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) {
        QWidget::mousePressEvent(event);
        return;
    }
    m_dragging = true;
    m_dragOffset = event->globalPos() - frameGeometry().topLeft();
    setCursor(Qt::ClosedHandCursor);
    raise();
    event->accept();
}

void WebcamBubble::mouseMoveEvent(QMouseEvent* event)
{
    if (!m_dragging) {
        QWidget::mouseMoveEvent(event);
        return;
    }
    QPoint target = event->globalPos() - m_dragOffset;
    if (m_bounds.isValid()) {
        const int maxX = std::max(m_bounds.left(), m_bounds.right() - width() + 1);
        const int maxY = std::max(m_bounds.top(), m_bounds.bottom() - height() + 1);
        target.setX(std::clamp(target.x(), m_bounds.left(), maxX));
        target.setY(std::clamp(target.y(), m_bounds.top(), maxY));
    }
    move(target);
    event->accept();
}

void WebcamBubble::mouseReleaseEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton || !m_dragging) {
        QWidget::mouseReleaseEvent(event);
        return;
    }
    m_dragging = false;
    setCursor(Qt::OpenHandCursor);
    if (m_moved)
        m_moved(deviceRectFor(geometry()));
    event->accept();
}

QPainterPath WebcamBubble::shapePath(const QRectF& rect) const
{
    QPainterPath path;
    if (m_shape == 0) {
        path.addEllipse(rect);
        return path;
    }
    const double radius = rect.width() * (m_shape == 1 ? 0.04 : 0.06);
    path.addRoundedRect(rect, radius, radius);
    return path;
}

void WebcamBubble::paintEvent(QPaintEvent* event)
{
    Q_UNUSED(event);
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing);

    const QRectF rect(0.5, 0.5, width() - 1.0, height() - 1.0);
    const QPainterPath path = shapePath(rect);

    // A QPainter clip path is one bit, so clipping the frame directly leaves
    // the stair-step edge the bubble used to show. Paint the mask into an
    // alpha canvas instead and let its antialiased edges do the cut.
    const qreal dpr = devicePixelRatioF();
    QImage canvas(QSize(std::max(1, int(std::lround(width() * dpr))),
                        std::max(1, int(std::lround(height() * dpr)))),
                  QImage::Format_ARGB32_Premultiplied);
    canvas.setDevicePixelRatio(dpr);
    canvas.fill(Qt::transparent);
    {
        QPainter mask(&canvas);
        mask.setRenderHint(QPainter::Antialiasing);
        mask.setPen(Qt::NoPen);
        mask.setBrush(Qt::white);
        mask.drawPath(shapePath(QRectF(0.0, 0.0, width(), height())));
        mask.setCompositionMode(QPainter::CompositionMode_SourceIn);
        if (m_flip) {
            // Mirror the picture, not the outline, so the bubble keeps its
            // place.
            mask.translate(width(), 0);
            mask.scale(-1.0, 1.0);
        }
        const QRectF frameRect(0.0, 0.0, width(), height());
        if (m_frame.isNull()) {
            mask.fillRect(frameRect, QColor(18, 18, 20, 200));
        } else {
            mask.drawImage(frameRect, m_frame);
        }
    }
    painter.drawImage(0, 0, canvas);

    painter.setPen(QPen(QColor(255, 255, 255, 40), 1.5));
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(path);
}

void WebcamBubble::showEvent(QShowEvent* event)
{
    QWidget::showEvent(event);
    Platform::keepTopmost(static_cast<quintptr>(winId()));
    // The camera is composited from its own track after the recording, so
    // the on-screen preview must not be burned into the desktop capture a
    // second time.
    Platform::excludeFromCapture(static_cast<quintptr>(winId()));

    auto* topmost = new QTimer(this);
    topmost->setInterval(2000);
    connect(topmost, &QTimer::timeout, this, [this] {
        if (isVisible())
            Platform::keepTopmost(static_cast<quintptr>(winId()));
    });
    topmost->start();
}

} // namespace Platform
