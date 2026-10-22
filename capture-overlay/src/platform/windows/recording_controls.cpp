#include "platform/windows/recording_controls.h"

#include "platform/platform.h"

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

constexpr int kWidth = 232;
constexpr int kHeight = 46;
constexpr int kButtonSize = 30;
constexpr int kDiscardSize = 24;
constexpr int kRightPad = 10;
constexpr int kGap = 6;

const QColor kPanel(25, 25, 28, 248);
const QColor kText(245, 245, 247);
const QColor kMuted(164, 164, 172);
const QColor kAccent(255, 102, 0);
const QColor kRim(255, 255, 255, 54);
const QColor kHover(255, 255, 255, 22);

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

} // namespace

RecordingControls::RecordingControls(Platform::RecordingSession* session,
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

    m_elapsedMs = 0;
    connect(m_session, &Platform::RecordingSession::elapsedChanged, this, [this](qint64 ms) {
        m_elapsedMs = ms;
        update();
    });
    connect(m_session, &Platform::RecordingSession::stateChanged, this, [this] { update(); });
    connect(m_session,
            &Platform::RecordingSession::finished,
            this,
            [this](bool, const QString&, const QString&) {
                m_finished = true;
                close();
            });
}

QString RecordingControls::elapsedText() const
{
    const qint64 totalSeconds = std::max<qint64>(0, m_elapsedMs / 1000);
    const qint64 hours = totalSeconds / 3600;
    const qint64 minutes = (totalSeconds / 60) % 60;
    const qint64 seconds = totalSeconds % 60;
    if (hours > 0) {
        return QStringLiteral("%1:%2:%3")
            .arg(hours)
            .arg(minutes, 2, 10, QLatin1Char('0'))
            .arg(seconds, 2, 10, QLatin1Char('0'));
    }
    return QStringLiteral("%1:%2")
        .arg(minutes, 2, 10, QLatin1Char('0'))
        .arg(seconds, 2, 10, QLatin1Char('0'));
}

QRectF RecordingControls::pauseRect() const
{
    const qreal x = kWidth - kRightPad - kDiscardSize - kGap - kButtonSize - kGap - kButtonSize;
    return QRectF(x, (kHeight - kButtonSize) / 2.0, kButtonSize, kButtonSize);
}

QRectF RecordingControls::stopRect() const
{
    const qreal x = kWidth - kRightPad - kDiscardSize - kGap - kButtonSize;
    return QRectF(x, (kHeight - kButtonSize) / 2.0, kButtonSize, kButtonSize);
}

QRectF RecordingControls::discardRect() const
{
    const qreal x = kWidth - kRightPad - kDiscardSize;
    return QRectF(x, (kHeight - kDiscardSize) / 2.0, kDiscardSize, kDiscardSize);
}

RecordingControls::Button RecordingControls::hitTest(const QPoint& point) const
{
    if (pauseRect().contains(point))
        return Button::Pause;
    if (stopRect().contains(point))
        return Button::Stop;
    if (discardRect().contains(point))
        return Button::Discard;
    return Button::Body;
}

void RecordingControls::paintEvent(QPaintEvent*)
{
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing, true);

    const QRectF body = QRectF(rect()).adjusted(0.5, 0.5, -0.5, -0.5);

    QPainterPath shadow;
    shadow.addRoundedRect(body.translated(0, 3), 14, 14);
    painter.fillPath(shadow, QColor(0, 0, 0, 95));

    QPainterPath panel;
    panel.addRoundedRect(body, 14, 14);
    painter.fillPath(panel, kPanel);
    painter.setPen(QPen(kRim, 1.0));
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(panel);

    const bool paused = m_session->isPaused();
    const QColor dot = m_closing ? kMuted : (paused ? kMuted : kAccent);
    painter.setPen(Qt::NoPen);
    painter.setBrush(dot);
    painter.drawEllipse(QPointF(17.0, kHeight / 2.0), 4.5, 4.5);

    painter.setPen(kText);
    painter.setFont(uiFont(12, true));
    const QRectF timerRect(28.0, 0.0, pauseRect().left() - 34.0, kHeight);
    painter.drawText(timerRect, Qt::AlignVCenter | Qt::AlignLeft, elapsedText());

    // Pause / resume.
    if (m_hovered == Button::Pause && !m_closing) {
        painter.setPen(Qt::NoPen);
        painter.setBrush(kHover);
        painter.drawEllipse(pauseRect().center(), kButtonSize / 2.0 - 1.0, kButtonSize / 2.0 - 1.0);
    }
    painter.setPen(QPen(kText, 2.0, Qt::SolidLine, Qt::RoundCap));
    painter.setBrush(Qt::NoBrush);
    const QPointF pauseCenter = pauseRect().center();
    if (paused) {
        QPainterPath triangle;
        triangle.moveTo(pauseCenter.x() - 3.5, pauseCenter.y() - 5.0);
        triangle.lineTo(pauseCenter.x() + 5.0, pauseCenter.y());
        triangle.lineTo(pauseCenter.x() - 3.5, pauseCenter.y() + 5.0);
        triangle.closeSubpath();
        painter.fillPath(triangle, kText);
    } else {
        painter.drawLine(QPointF(pauseCenter.x() - 3.5, pauseCenter.y() - 5.0),
                         QPointF(pauseCenter.x() - 3.5, pauseCenter.y() + 5.0));
        painter.drawLine(QPointF(pauseCenter.x() + 3.5, pauseCenter.y() - 5.0),
                         QPointF(pauseCenter.x() + 3.5, pauseCenter.y() + 5.0));
    }

    // Stop / save.
    if (m_hovered == Button::Stop && !m_closing) {
        painter.setPen(Qt::NoPen);
        painter.setBrush(kHover);
        painter.drawEllipse(stopRect().center(), kButtonSize / 2.0 - 1.0, kButtonSize / 2.0 - 1.0);
    }
    painter.setPen(Qt::NoPen);
    painter.setBrush(m_closing ? kMuted : QColor(232, 17, 35));
    painter.drawRoundedRect(QRectF(stopRect().center() - QPointF(5, 5), QSizeF(10, 10)), 2.5, 2.5);

    // Discard.
    if (m_hovered == Button::Discard && !m_closing) {
        painter.setPen(Qt::NoPen);
        painter.setBrush(kHover);
        painter.drawEllipse(discardRect().center(), kDiscardSize / 2.0 - 1.0, kDiscardSize / 2.0 - 1.0);
    }
    painter.setPen(QPen(m_closing ? kMuted : kMuted.lighter(120), 1.8, Qt::SolidLine, Qt::RoundCap));
    const QPointF discardCenter = discardRect().center();
    painter.drawLine(discardCenter + QPointF(-3.5, -3.5), discardCenter + QPointF(3.5, 3.5));
    painter.drawLine(discardCenter + QPointF(-3.5, 3.5), discardCenter + QPointF(3.5, -3.5));
}

void RecordingControls::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) {
        QWidget::mousePressEvent(event);
        return;
    }

    switch (hitTest(event->pos())) {
    case Button::Pause:
        if (m_closing)
            return;
        if (m_session->isPaused())
            m_session->resume();
        else
            m_session->pause();
        update();
        return;
    case Button::Stop:
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

void RecordingControls::mouseMoveEvent(QMouseEvent* event)
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

void RecordingControls::mouseReleaseEvent(QMouseEvent* event)
{
    if (event->button() == Qt::LeftButton)
        m_dragging = false;
}

void RecordingControls::leaveEvent(QEvent* event)
{
    m_hovered = Button::None;
    update();
    QWidget::leaveEvent(event);
}

void RecordingControls::showEvent(QShowEvent* event)
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

void RecordingControls::positionOnScreen()
{
    QScreen* screen = screenForRegion(m_captureRegion);
    if (!screen)
        screen = QGuiApplication::primaryScreen();
    const QRect available = screen ? screen->availableGeometry() : QRect(0, 0, 1920, 1080);
    const qreal dpr = screen ? screen->devicePixelRatio() : 1.0;

    const QRect logical(
        QPoint(qRound(m_captureRegion.x() / dpr), qRound(m_captureRegion.y() / dpr)),
        QSize(qRound(m_captureRegion.width() / dpr), qRound(m_captureRegion.height() / dpr)));

    int x = logical.center().x() - width() / 2;
    int y = logical.top() - height() - 12;
    if (y < available.top() + 8)
        y = logical.bottom() + 12;
    if (y + height() > available.bottom() - 8)
        y = available.top() + 8;

    x = std::max(available.left() + 8, std::min(x, available.right() - width() - 8));
    move(x, y);
}

void RecordingControls::closeEvent(QCloseEvent* event)
{
    if (!m_finished && m_session->isActive())
        m_session->stop();
    QWidget::closeEvent(event);
}
