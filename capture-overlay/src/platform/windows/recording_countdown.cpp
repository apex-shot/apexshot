#include "platform/windows/recording_countdown.h"

#include "platform/platform.h"

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

constexpr int kDiameter = 184;
constexpr int kRingInset = 5;
constexpr int kRingWidth = 3;

const QColor kBackdrop(0, 0, 0, 242);
const QColor kCancelBackdrop(132, 38, 24, 242);
const QColor kRim(255, 255, 255, 34);
const QColor kTrack(255, 255, 255, 26);
const QColor kAccent(255, 102, 0, 225);
const QColor kText(255, 255, 255);
const QColor kCancelText(255, 228, 214);

QFont countdownFont(int pixelSize)
{
    QFont font(QStringLiteral("Inter"));
    font.setPixelSize(pixelSize);
    font.setWeight(QFont::Bold);
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

} // namespace

RecordingCountdown::RecordingCountdown(int seconds,
                                       const QRect& captureRegion,
                                       std::function<void()> onFinished,
                                       QWidget* parent)
    : QWidget(parent)
    , m_remaining(seconds)
    , m_total(seconds)
    , m_captureRegion(captureRegion)
    , m_onFinished(std::move(onFinished))
{
    setWindowFlags(Qt::Tool | Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint);
    setAttribute(Qt::WA_TranslucentBackground);
    setAttribute(Qt::WA_ShowWithoutActivating);
    setAttribute(Qt::WA_DeleteOnClose);
    setFixedSize(kDiameter, kDiameter);
    setCursor(Qt::PointingHandCursor);
    setToolTip(QStringLiteral("Click to cancel"));

    m_timer = new QTimer(this);
    m_timer->setInterval(1000);
    connect(m_timer, &QTimer::timeout, this, [this] {
        if (--m_remaining > 0) {
            raise();
            Platform::keepTopmost(static_cast<quintptr>(winId()));
            update();
            return;
        }

        m_timer->stop();
        auto finished = m_onFinished;
        m_onFinished = nullptr;
        close();
        if (finished)
            finished();
    });
    m_timer->start();
}

void RecordingCountdown::paintEvent(QPaintEvent*)
{
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing, true);

    const QRectF circle =
        QRectF(rect()).adjusted(kRingInset, kRingInset, -kRingInset, -kRingInset);

    // A soft shadow lifts the circle off the desktop.
    QPainterPath shadow;
    shadow.addEllipse(circle.adjusted(-1, 2, 1, 4));
    painter.fillPath(shadow, QColor(0, 0, 0, 110));

    painter.setPen(Qt::NoPen);
    painter.setBrush(m_hovered ? kCancelBackdrop : kBackdrop);
    painter.drawEllipse(circle);

    painter.setPen(QPen(kRim, 1.0));
    painter.setBrush(Qt::NoBrush);
    painter.drawEllipse(circle);

    // Remaining-time ring, draining clockwise from the top.
    const QRectF ring =
        QRectF(rect()).adjusted(kRingInset + 3, kRingInset + 3, -kRingInset - 3, -kRingInset - 3);
    painter.setPen(QPen(kTrack, kRingWidth, Qt::SolidLine, Qt::RoundCap));
    painter.drawEllipse(ring);

    if (m_total > 0 && m_remaining > 0) {
        const int span = -qRound(360.0 * 16.0 * m_remaining / m_total);
        painter.setPen(QPen(kAccent, kRingWidth, Qt::SolidLine, Qt::RoundCap));
        painter.drawArc(ring, 90 * 16, span);
    }

    painter.setFont(countdownFont(m_hovered ? 30 : 88));
    painter.setPen(m_hovered ? kCancelText : kText);
    painter.drawText(circle,
                     Qt::AlignCenter,
                     m_hovered ? QStringLiteral("Cancel") : QString::number(m_remaining));
}

void RecordingCountdown::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) {
        QWidget::mousePressEvent(event);
        return;
    }

    m_timer->stop();
    m_onFinished = nullptr;
    close();
}

void RecordingCountdown::enterEvent(QEvent* event)
{
    m_hovered = true;
    update();
    QWidget::enterEvent(event);
}

void RecordingCountdown::leaveEvent(QEvent* event)
{
    m_hovered = false;
    update();
    QWidget::leaveEvent(event);
}

void RecordingCountdown::showEvent(QShowEvent* event)
{
    QWidget::showEvent(event);
    positionOnScreen();
    Platform::keepTopmost(static_cast<quintptr>(winId()));
    Platform::excludeFromCapture(static_cast<quintptr>(winId()));
}

void RecordingCountdown::positionOnScreen()
{
    QScreen* screen = screenForRegion(m_captureRegion);
    if (!screen)
        screen = QGuiApplication::primaryScreen();
    const QRect available = screen ? screen->availableGeometry() : QRect(0, 0, 1920, 1080);
    const QPoint center = available.center();

    int x = center.x() - width() / 2;
    int y = center.y() - height() / 2;
    x = std::max(available.left() + 8, std::min(x, available.right() - width() - 8));
    y = std::max(available.top() + 8, std::min(y, available.bottom() - height() - 8));
    move(x, y);
}
