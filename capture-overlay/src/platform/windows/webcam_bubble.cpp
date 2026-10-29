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
    setAttribute(Qt::WA_DeleteOnClose);
    setFocusPolicy(Qt::NoFocus);
    setMouseTracking(true);
    setCursor(Qt::OpenHandCursor);
    setWindowTitle(QStringLiteral("ApexShot webcam"));
    auto* topmost = new QTimer(this);
    topmost->setInterval(2000);
    connect(topmost, &QTimer::timeout, this, [this] {
        if (isVisible())
            Platform::keepTopmost(static_cast<quintptr>(winId()));
    });
    topmost->start();
}

WebcamBubble::~WebcamBubble() = default;

void WebcamBubble::setFrame(const QImage& frame)
{
    m_frame = frame;
    m_previewError.clear();
    update();
}

void WebcamBubble::setPreviewError(const QString& error)
{
    m_previewError = error;
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

void WebcamBubble::setSetupMode(bool enabled)
{
    m_setupMode = enabled;
    m_resizeHandle = -1;
    m_hoveredControl = -1;
    update();
}

QRectF WebcamBubble::controlBarRect() const
{
    return QRectF((width() - 88.0) / 2.0, 14.0, 88.0, 36.0);
}

int WebcamBubble::resizeHandleAt(const QPoint& point) const
{
    const QPoint handles[] = {QPoint(8, 8), QPoint(width() - 9, 8),
                              QPoint(8, height() - 9), QPoint(width() - 9, height() - 9)};
    for (int i = 0; i < 4; ++i) {
        if ((point - handles[i]).manhattanLength() <= 18)
            return i;
    }
    return -1;
}

void WebcamBubble::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) {
        QWidget::mousePressEvent(event);
        return;
    }
    if (m_setupMode && controlBarRect().contains(event->pos())) {
        if (event->pos().x() < controlBarRect().center().x()) {
            m_flip = !m_flip;
        } else {
            m_shape = (m_shape + 1) % 4;
            const int newHeight = std::max(1, int(std::lround(width() * kWebcamShapeAspects[m_shape])));
            QRect next(geometry().x(), geometry().y(), width(), newHeight);
            if (m_bounds.isValid())
                next = webcamFitRect(m_bounds, next);
            setGeometry(next);
        }
        update();
        if (m_moved)
            m_moved(deviceRectFor(geometry()));
        event->accept();
        return;
    }
    if (m_setupMode && (m_resizeHandle = resizeHandleAt(event->pos())) >= 0) {
        m_resizeStart = geometry();
        m_resizeOrigin = event->globalPos();
        event->accept();
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
    if (m_resizeHandle >= 0) {
        const bool left = (m_resizeHandle % 2) == 0;
        const bool top = m_resizeHandle < 2;
        const QPoint delta = event->globalPos() - m_resizeOrigin;
        const double aspect = kWebcamShapeAspects[m_shape];
        const int changeX = left ? -delta.x() : delta.x();
        const int changeY = top ? -delta.y() : delta.y();
        int desired = m_resizeStart.width() + (std::abs(changeX) >= std::abs(changeY / aspect)
                                                   ? changeX : int(std::lround(changeY / aspect)));
        int maxWidth = 100000;
        if (m_bounds.isValid()) {
            const int anchorX = left ? m_resizeStart.x() + m_resizeStart.width() : m_resizeStart.x();
            const int anchorY = top ? m_resizeStart.y() + m_resizeStart.height() : m_resizeStart.y();
            const int availableWidth = left ? anchorX - m_bounds.x()
                                            : m_bounds.x() + m_bounds.width() - anchorX;
            const int availableHeight = top ? anchorY - m_bounds.y()
                                            : m_bounds.y() + m_bounds.height() - anchorY;
            maxWidth = std::max(1, std::min(availableWidth, int(availableHeight / aspect)));
        }
        const int newWidth = std::clamp(desired, std::min(96, maxWidth), maxWidth);
        const int newHeight = std::max(1, int(std::lround(newWidth * aspect)));
        const int x = left ? m_resizeStart.x() + m_resizeStart.width() - newWidth : m_resizeStart.x();
        const int y = top ? m_resizeStart.y() + m_resizeStart.height() - newHeight : m_resizeStart.y();
        setGeometry(x, y, newWidth, newHeight);
        event->accept();
        return;
    }
    if (!m_dragging) {
        if (m_setupMode) {
            const int handle = resizeHandleAt(event->pos());
            const bool overBar = controlBarRect().contains(event->pos());
            const int hovered = overBar ? (event->pos().x() < controlBarRect().center().x() ? 0 : 1)
                                        : -1;
            if (hovered != m_hoveredControl) {
                m_hoveredControl = hovered;
                update();
            }
            setCursor(overBar ? Qt::PointingHandCursor
                              : (handle < 0 ? Qt::OpenHandCursor
                                            : (handle == 0 || handle == 3 ? Qt::SizeFDiagCursor
                                                                          : Qt::SizeBDiagCursor)));
            if (overBar)
                setToolTip(event->pos().x() < controlBarRect().center().x()
                               ? QStringLiteral("Mirror webcam")
                               : QStringLiteral("Change webcam shape"));
            else
                setToolTip(QString());
        }
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
    if (event->button() == Qt::LeftButton && m_resizeHandle >= 0) {
        m_resizeHandle = -1;
        if (m_moved)
            m_moved(deviceRectFor(geometry()));
        event->accept();
        return;
    }
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

void WebcamBubble::leaveEvent(QEvent* event)
{
    if (m_hoveredControl != -1) {
        m_hoveredControl = -1;
        update();
    }
    QWidget::leaveEvent(event);
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
            const double sourceAspect = double(m_frame.width()) / m_frame.height();
            const double targetAspect = double(width()) / height();
            QRectF source(0, 0, m_frame.width(), m_frame.height());
            if (sourceAspect > targetAspect) {
                source.setWidth(m_frame.height() * targetAspect);
                source.moveLeft((m_frame.width() - source.width()) / 2.0);
            } else {
                source.setHeight(m_frame.width() / targetAspect);
                source.moveTop((m_frame.height() - source.height()) / 2.0);
            }
            mask.drawImage(frameRect, m_frame, source);
        }
    }
    painter.drawImage(0, 0, canvas);

    painter.setPen(QPen(QColor(255, 255, 255, 40), 1.5));
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(path);

    if (!m_previewError.isEmpty()) {
        painter.setPen(Qt::white);
        painter.drawText(rect.adjusted(18, 55, -18, -18), Qt::AlignCenter | Qt::TextWordWrap,
                         m_previewError);
    }
    if (m_setupMode) {
        painter.setPen(QPen(QColor(255, 255, 255, 220), 1.5));
        painter.setBrush(QColor(30, 30, 32, 230));
        const QPointF handles[] = {QPointF(8, 8), QPointF(width() - 9, 8),
                                   QPointF(8, height() - 9), QPointF(width() - 9, height() - 9)};
        for (const QPointF& point : handles)
            painter.drawEllipse(point, 4.5, 4.5);

        const QRectF bar = controlBarRect();
        painter.setPen(Qt::NoPen);
        painter.setBrush(QColor(0, 0, 0, 110));
        painter.drawRoundedRect(bar.translated(0, 2), 9, 9);
        painter.setPen(QPen(QColor(255, 255, 255, 42), 1));
        painter.setBrush(QColor(20, 20, 20, 250));
        painter.drawRoundedRect(bar, 9, 9);
        if (m_hoveredControl >= 0) {
            painter.setPen(Qt::NoPen);
            painter.setBrush(QColor(255, 255, 255, 16));
            painter.drawRoundedRect(QRectF(bar.left() + 3 + m_hoveredControl * 41,
                                           bar.top() + 3, 41, bar.height() - 6), 7, 7);
        }
        painter.setPen(QPen(m_flip ? QColor(255, 138, 64) : QColor(185, 186, 193), 2.0,
                            Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
        painter.setBrush(Qt::NoBrush);
        painter.save();
        const QPointF mirror(bar.left() + 24.0, bar.center().y());
        painter.translate(mirror - QPointF(8.5, 8.5));
        painter.scale(17.0 / 24.0, 17.0 / 24.0);
        QPainterPath mirrorOutline;
        mirrorOutline.moveTo(8, 3);
        mirrorOutline.lineTo(5, 3);
        mirrorOutline.quadTo(3, 3, 3, 5);
        mirrorOutline.lineTo(3, 19);
        mirrorOutline.quadTo(3, 21, 5, 21);
        mirrorOutline.lineTo(8, 21);
        mirrorOutline.moveTo(16, 3);
        mirrorOutline.lineTo(19, 3);
        mirrorOutline.quadTo(21, 3, 21, 5);
        mirrorOutline.lineTo(21, 19);
        mirrorOutline.quadTo(21, 21, 19, 21);
        mirrorOutline.lineTo(16, 21);
        painter.drawPath(mirrorOutline);
        for (int y : {2, 8, 14, 20})
            painter.drawLine(QPointF(12, y), QPointF(12, y + 2));
        painter.restore();
        painter.setPen(QPen(QColor(185, 186, 193), 1.5, Qt::SolidLine,
                            Qt::RoundCap, Qt::RoundJoin));
        const QPointF shape(bar.left() + 65.0, bar.center().y());
        if (m_shape == 0)
            painter.drawEllipse(shape, 8, 8);
        else {
            const double w = m_shape == 3 ? 12 : 17;
            const double h = m_shape == 2 ? 12 : 17;
            painter.drawRoundedRect(QRectF(shape.x() - w / 2, shape.y() - h / 2, w, h), 2, 2);
        }
    }
}

void WebcamBubble::showEvent(QShowEvent* event)
{
    QWidget::showEvent(event);
    Platform::keepTopmost(static_cast<quintptr>(winId()));
    // The camera is composited from its own track after the recording, so
    // the on-screen preview must not be burned into the desktop capture a
    // second time.
    Platform::excludeFromCapture(static_cast<quintptr>(winId()));

}

} // namespace Platform
