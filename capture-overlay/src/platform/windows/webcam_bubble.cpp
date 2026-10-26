#include "platform/windows/webcam_bubble.h"

#include "platform/platform.h"

#include <QGuiApplication>
#include <QPainter>
#include <QPainterPath>
#include <QScreen>
#include <QShowEvent>
#include <QTimer>

#include <algorithm>
#include <cmath>

namespace Platform {

WebcamBubble::WebcamBubble(int shape, bool flip, QWidget* parent)
    : QWidget(parent, Qt::Tool | Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint
                           | Qt::WindowDoesNotAcceptFocus | Qt::NoDropShadowWindowHint)
    , m_shape(std::clamp(shape, 0, 3))
    , m_flip(flip)
{
    setAttribute(Qt::WA_TranslucentBackground);
    setAttribute(Qt::WA_ShowWithoutActivating);
    setAttribute(Qt::WA_TransparentForMouseEvents);
    setFocusPolicy(Qt::NoFocus);
    setWindowTitle(QStringLiteral("ApexShot webcam"));
}

WebcamBubble::~WebcamBubble() = default;

void WebcamBubble::setFrame(const QImage& frame)
{
    m_frame = frame;
    update();
}

void WebcamBubble::placeAt(const QRect& deviceRect)
{
    if (deviceRect.isEmpty())
        return;
    // The rect is in device pixels, so each screen has to be compared in its
    // own device pixels rather than through Qt's logical geometry.
    QScreen* screen = nullptr;
    for (QScreen* candidate : QGuiApplication::screens()) {
        const qreal candidateDpr = candidate->devicePixelRatio();
        const QRect logical = candidate->geometry();
        const QRect device(QPoint(int(std::lround(logical.x() * candidateDpr)),
                                  int(std::lround(logical.y() * candidateDpr))),
                           QSize(int(std::lround(logical.width() * candidateDpr)),
                                 int(std::lround(logical.height() * candidateDpr))));
        if (device.contains(deviceRect.center())) {
            screen = candidate;
            break;
        }
    }
    if (!screen)
        screen = QGuiApplication::screenAt(deviceRect.center());
    if (!screen)
        screen = QGuiApplication::primaryScreen();
    const qreal dpr = screen ? screen->devicePixelRatio() : 1.0;
    // The capture region and the rect handed in are in device pixels; the
    // window is placed in the compositor's logical pixels.
    const QRect logical(int(std::lround(deviceRect.x() / dpr)),
                        int(std::lround(deviceRect.y() / dpr)),
                        std::max(1, int(std::lround(deviceRect.width() / dpr))),
                        std::max(1, int(std::lround(deviceRect.height() / dpr))));
    setGeometry(logical);
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

    painter.save();
    painter.setClipPath(path);
    if (m_flip) {
        // Mirror the picture, not the outline, so the bubble keeps its place.
        painter.translate(width(), 0);
        painter.scale(-1.0, 1.0);
    }
    const QRectF frameRect(0.0, 0.0, width(), height());
    if (m_frame.isNull()) {
        painter.fillRect(frameRect, QColor(18, 18, 20, 200));
    } else {
        painter.drawImage(frameRect, m_frame);
    }
    painter.restore();

    painter.setPen(QPen(QColor(255, 255, 255, 40), 1.5));
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(path);
}

void WebcamBubble::showEvent(QShowEvent* event)
{
    QWidget::showEvent(event);
    Platform::keepTopmost(static_cast<quintptr>(winId()));
    // The editor composites the track file; a bubble left in the capture would
    // burn a second, unremovable copy into the recording.
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
