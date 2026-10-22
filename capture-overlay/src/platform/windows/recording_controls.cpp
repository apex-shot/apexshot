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

constexpr int kWidth = 260;
constexpr int kHeight = 62;
constexpr int kMargin = 24;

// Same chrome and control tokens as the Linux recording bar.
const QColor kChrome(20, 20, 20);
const QColor kChromeRim(255, 255, 255, 26);
const QColor kSegment(30, 31, 34);
const QColor kSegmentHover(40, 41, 44);
const QColor kText(245, 245, 246);
const QColor kMuted(160, 160, 165);
const QColor kHover(255, 255, 255, 22);
const QColor kPressed(255, 255, 255, 34);
const QColor kSeparator(255, 255, 255, 20);
const QColor kRecord(244, 99, 87);

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

void drawPauseIcon(QPainter& painter, const QRectF& rect, bool paused)
{
    painter.setPen(QPen(kText, 2.0, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
    painter.setBrush(Qt::NoBrush);
    const QPointF c = rect.center();
    const qreal r = 10.0;
    painter.drawEllipse(QRectF(c.x() - r, c.y() - r, r * 2.0, r * 2.0));

    if (paused) {
        QPainterPath triangle;
        triangle.moveTo(c.x() - 3.0, c.y() - 5.5);
        triangle.lineTo(c.x() + 5.0, c.y());
        triangle.lineTo(c.x() - 3.0, c.y() + 5.5);
        triangle.closeSubpath();
        painter.fillPath(triangle, kText);
    } else {
        painter.drawLine(QPointF(c.x() - 3.5, c.y() - 5.5), QPointF(c.x() - 3.5, c.y() + 5.5));
        painter.drawLine(QPointF(c.x() + 3.5, c.y() - 5.5), QPointF(c.x() + 3.5, c.y() + 5.5));
    }
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
        .arg(minutes)
        .arg(seconds, 2, 10, QLatin1Char('0'));
}

QRectF RecordingControls::stopRect() const
{
    return QRectF(6, 5, 130, 52);
}

QRectF RecordingControls::pauseRect() const
{
    return QRectF(141, 5, 56, 52);
}

QRectF RecordingControls::discardRect() const
{
    return QRectF(202, 5, 56, 52);
}

RecordingControls::Button RecordingControls::hitTest(const QPoint& point) const
{
    if (stopRect().contains(point))
        return Button::Stop;
    if (pauseRect().contains(point))
        return Button::Pause;
    if (discardRect().contains(point))
        return Button::Discard;
    return Button::Body;
}

void RecordingControls::paintEvent(QPaintEvent*)
{
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing, true);

    // Chrome: the same 20,20,20 bar with a hairline rim the Linux controls use.
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

    // Stop + timer segment.
    const QRectF stop = stopRect();
    painter.setPen(Qt::NoPen);
    painter.setBrush(m_hovered == Button::Stop && !m_closing ? kSegmentHover : kSegment);
    painter.drawRoundedRect(stop, 8, 8);

    const QPointF stopCenter(stop.left() + 26.0, stop.center().y());
    const QColor recordColor = m_closing ? kMuted : kRecord;
    painter.setPen(QPen(recordColor, 1.8));
    painter.setBrush(Qt::NoBrush);
    painter.drawEllipse(stopCenter, 10.0, 10.0);
    painter.setPen(Qt::NoPen);
    painter.setBrush(recordColor);
    painter.drawRoundedRect(
        QRectF(stopCenter - QPointF(4.0, 4.0), QSizeF(8.0, 8.0)), 1.5, 1.5);

    painter.setFont(uiFont(19, true));
    painter.setPen(m_closing ? kMuted : kRecord);
    painter.drawText(QRectF(stop.left() + 48.0, stop.top(), stop.width() - 56.0, stop.height()),
                     Qt::AlignVCenter | Qt::AlignLeft,
                     m_closing ? QStringLiteral("Saving") : elapsedText());

    if (!m_closing) {
        // Hairline rules matching the Linux bar.
        painter.setPen(QPen(kSeparator, 1.0));
        painter.drawLine(QPointF(138.5, 18.0), QPointF(138.5, kHeight - 18.0));
        painter.drawLine(QPointF(200.5, 18.0), QPointF(200.5, kHeight - 18.0));

        // Pause / resume.
        if (m_hovered == Button::Pause) {
            painter.setPen(Qt::NoPen);
            painter.setBrush(kHover);
            painter.drawRoundedRect(pauseRect().adjusted(2, 2, -2, -2), 10, 10);
        }
        drawPauseIcon(painter, pauseRect(), m_session->isPaused());

        // Discard.
        if (m_hovered == Button::Discard) {
            painter.setPen(Qt::NoPen);
            painter.setBrush(kHover);
            painter.drawRoundedRect(discardRect().adjusted(2, 2, -2, -2), 10, 10);
        }
        drawTrashIcon(painter, discardRect());
    }
}

void RecordingControls::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) {
        QWidget::mousePressEvent(event);
        return;
    }

    switch (hitTest(event->pos())) {
    case Button::Stop:
        if (m_closing)
            return;
        m_closing = true;
        m_session->stop();
        update();
        return;
    case Button::Pause:
        if (m_closing)
            return;
        if (m_session->isPaused())
            m_session->resume();
        else
            m_session->pause();
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

        // Prefer the dimmed space below the selection, then above it. Only
        // when neither side fits does the bar move inside the recording, and
        // then it stays at the top so the bottom of the area stays readable.
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

void RecordingControls::closeEvent(QCloseEvent* event)
{
    if (!m_finished && m_session->isActive())
        m_session->stop();
    QWidget::closeEvent(event);
}
