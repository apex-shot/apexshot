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

constexpr int kWidth = 318;
constexpr int kHeight = 56;
constexpr int kPauseSize = 34;
constexpr int kStopWidth = 78;
constexpr int kStopHeight = 32;
constexpr int kDiscardSize = 30;
constexpr int kRightPad = 12;
constexpr int kGap = 8;

// Tokens shared with the Quick Access shell and the result card.
const QColor kCard(18, 18, 18, 240);
const QColor kText(245, 245, 247);
const QColor kMuted(164, 164, 172);
const QColor kRim(255, 255, 255, 54);
const QColor kHover(255, 255, 255, 22);
const QColor kDanger(232, 17, 35);
const QColor kDangerHover(240, 34, 52);
const QColor kRecording(232, 17, 35);

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

    m_pulseTimer = new QTimer(this);
    m_pulseTimer->setInterval(700);
    connect(m_pulseTimer, &QTimer::timeout, this, [this] {
        m_pulse = !m_pulse;
        update();
    });
    m_pulseTimer->start();
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

QRectF RecordingControls::discardRect() const
{
    return QRectF(kWidth - kRightPad - kDiscardSize,
                  (kHeight - kDiscardSize) / 2.0,
                  kDiscardSize,
                  kDiscardSize);
}

QRectF RecordingControls::stopRect() const
{
    return QRectF(discardRect().left() - kGap - kStopWidth,
                  (kHeight - kStopHeight) / 2.0,
                  kStopWidth,
                  kStopHeight);
}

QRectF RecordingControls::pauseRect() const
{
    return QRectF(stopRect().left() - kGap - kPauseSize,
                  (kHeight - kPauseSize) / 2.0,
                  kPauseSize,
                  kPauseSize);
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
    shadow.addRoundedRect(body.translated(0, 4), 18, 18);
    painter.fillPath(shadow, QColor(0, 0, 0, 120));

    QPainterPath panel;
    panel.addRoundedRect(body, 18, 18);
    painter.fillPath(panel, kCard);
    painter.setPen(QPen(kRim, 1.0));
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(panel);

    // Recording lamp: a soft outer glow and a solid core, dimmed once the
    // session is saving or paused.
    const bool paused = m_session->isPaused();
    const QPointF lamp(21.0, kHeight / 2.0);
    const int glowAlpha = m_closing || paused ? 20 : (m_pulse ? 70 : 38);
    painter.setPen(Qt::NoPen);
    painter.setBrush(QColor(kRecording.red(), kRecording.green(), kRecording.blue(), glowAlpha));
    painter.drawEllipse(lamp, 8.0, 8.0);
    painter.setBrush(m_closing || paused ? kMuted : kRecording);
    painter.drawEllipse(lamp, 4.5, 4.5);

    const qreal textX = 36.0;
    painter.setFont(uiFont(9, true));
    painter.setPen(m_closing ? kMuted : QColor(255, 132, 132));
    painter.drawText(QRectF(textX, 10.0, 110.0, 12.0),
                     Qt::AlignLeft | Qt::AlignVCenter,
                     m_closing ? QStringLiteral("SAVING") : QStringLiteral("REC"));

    painter.setFont(uiFont(15, true));
    painter.setPen(kText);
    painter.drawText(QRectF(textX, 24.0, 118.0, 22.0),
                     Qt::AlignLeft | Qt::AlignVCenter,
                     elapsedText());

    // Pause / resume.
    if (!m_closing) {
        if (m_hovered == Button::Pause) {
            painter.setPen(Qt::NoPen);
            painter.setBrush(kHover);
            painter.drawEllipse(pauseRect().center(), kPauseSize / 2.0 - 1.0,
                                kPauseSize / 2.0 - 1.0);
        }
        painter.setPen(QPen(kText, 2.0, Qt::SolidLine, Qt::RoundCap));
        painter.setBrush(Qt::NoBrush);
        const QPointF center = pauseRect().center();
        if (paused) {
            QPainterPath triangle;
            triangle.moveTo(center.x() - 3.5, center.y() - 5.5);
            triangle.lineTo(center.x() + 5.5, center.y());
            triangle.lineTo(center.x() - 3.5, center.y() + 5.5);
            triangle.closeSubpath();
            painter.fillPath(triangle, kText);
        } else {
            painter.drawLine(QPointF(center.x() - 3.5, center.y() - 5.5),
                             QPointF(center.x() - 3.5, center.y() + 5.5));
            painter.drawLine(QPointF(center.x() + 3.5, center.y() - 5.5),
                             QPointF(center.x() + 3.5, center.y() + 5.5));
        }
    }

    // Labelled stop button: the primary action, in the app's danger colour.
    const bool stopHovered = m_hovered == Button::Stop && !m_closing;
    const QRectF stop = stopRect();
    painter.setPen(Qt::NoPen);
    painter.setBrush(m_closing ? QColor(70, 70, 76) : (stopHovered ? kDangerHover : kDanger));
    painter.drawRoundedRect(stop, 10, 10);
    painter.setFont(uiFont(12, true));
    painter.setPen(m_closing ? kMuted : Qt::white);
    painter.drawText(stop, Qt::AlignCenter,
                     m_closing ? QStringLiteral("Saving") : QStringLiteral("Stop"));

    // Discard.
    if (!m_closing) {
        if (m_hovered == Button::Discard) {
            painter.setPen(Qt::NoPen);
            painter.setBrush(kHover);
            painter.drawEllipse(discardRect().center(), kDiscardSize / 2.0 - 1.0,
                                kDiscardSize / 2.0 - 1.0);
        }
        painter.setPen(QPen(m_hovered == Button::Discard ? kText : kMuted,
                            1.8,
                            Qt::SolidLine,
                            Qt::RoundCap));
        const QPointF center = discardRect().center();
        painter.drawLine(center + QPointF(-3.5, -3.5), center + QPointF(3.5, 3.5));
        painter.drawLine(center + QPointF(-3.5, 3.5), center + QPointF(3.5, -3.5));
    }
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
        if (hovered == Button::Stop)
            setCursor(Qt::PointingHandCursor);
        else if (hovered == Button::Pause || hovered == Button::Discard)
            setCursor(Qt::PointingHandCursor);
        else
            setCursor(Qt::SizeAllCursor);
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
