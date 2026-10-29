#include "platform/windows/recording_controls.h"

#include "platform/platform.h"

#include <QCloseEvent>
#include <QCursor>
#include <QFont>
#include <QFontDatabase>
#include <QFontMetrics>
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

// A compact single-row pill: drag dots, record dot, timer, then the action
// buttons. The bar is sized from its contents so the saving state shrinks
// instead of leaving a gap on the right.
constexpr int kHeight = 46;
// The window carries a few extra pixels below the body so the drop shadow's
// rounded bottom edge fits instead of being clipped into a square band.
constexpr int kShadowDrop = 3;
constexpr int kWindowHeight = kHeight + kShadowDrop;
constexpr int kPad = 10;
constexpr int kButtonSize = 30;
constexpr int kButtonGap = 2;
constexpr int kButtonCount = 4;
constexpr int kHandleWidth = 8;
constexpr int kDotSize = 9;
constexpr int kIconSize = 18;
constexpr int kTextX = kPad + kHandleWidth + 10 + kDotSize + 8;

const QColor kChrome(20, 20, 20, 250);
const QColor kChromeRim(255, 255, 255, 26);
const QColor kText(245, 245, 246);
const QColor kMuted(158, 158, 164);
const QColor kHover(255, 255, 255, 20);
const QColor kRecord(244, 99, 87);

// Lucide glyphs, so the bar uses the same icon set as the Quick Access bar.
constexpr ushort kGlyphPause = 0xE12E;
constexpr ushort kGlyphPlay = 0xE13C;
constexpr ushort kGlyphRestart = 0xE148;
constexpr ushort kGlyphTrash = 0xE18D;

QFont uiFont(int pixelSize, bool bold)
{
    QFont font(QStringLiteral("Inter"));
    font.setPixelSize(pixelSize);
    font.setWeight(bold ? QFont::DemiBold : QFont::Normal);
    return font;
}

QString iconFamily()
{
    static const QString family = [] {
        const int id =
            QFontDatabase::addApplicationFont(QStringLiteral(":/fonts/Lucide.ttf"));
        if (id < 0)
            return QString();
        const QStringList families = QFontDatabase::applicationFontFamilies(id);
        return families.isEmpty() ? QString() : families.first();
    }();
    return family;
}

void drawIcon(QPainter& painter, ushort glyph, const QRectF& rect, const QColor& color)
{
    const QString family = iconFamily();
    if (family.isEmpty())
        return;
    QFont font(family);
    font.setPixelSize(kIconSize);
    QPainterPath path;
    path.addText(0, 0, font, QString(QChar(glyph)));
    const QRectF bounds = path.boundingRect();
    painter.save();
    painter.translate(rect.center() - bounds.center());
    painter.fillPath(path, color);
    painter.restore();
}

QPoint clampedToScreen(const QPoint& topLeft, const QSize& size, const QPoint& cursor)
{
    QScreen* screen = QGuiApplication::screenAt(cursor);
    if (!screen)
        screen = QGuiApplication::screenAt(
            QPoint(topLeft.x() + size.width() / 2, topLeft.y() + size.height() / 2));
    if (!screen)
        return topLeft;
    const QRect available = screen->availableGeometry();
    QPoint point = topLeft;
    point.setX(std::clamp(point.x(),
                          available.left(),
                          std::max(available.left(), available.right() - size.width() + 1)));
    point.setY(std::clamp(point.y(),
                          available.top(),
                          std::max(available.top(), available.bottom() - size.height() + 1)));
    return point;
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
    setFixedSize(preferredWidth(), kWindowHeight);

    m_elapsedMs = 0;
    m_textWidth = QFontMetrics(uiFont(15, true)).horizontalAdvance(QStringLiteral("00:00"));
    connect(m_session, &Platform::RecordingSession::elapsedChanged, this, [this](qint64 ms) {
        m_elapsedMs = ms;
        const int needed =
            QFontMetrics(uiFont(15, true)).horizontalAdvance(elapsedText());
        if (needed > m_textWidth) {
            m_textWidth = needed;
            updateWidth();
        }
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

int RecordingControls::preferredWidth() const
{
    const QFontMetrics metrics(uiFont(15, true));
    const QString text = m_closing ? QStringLiteral("Saving") : QString();
    const int textWidth = m_closing ? metrics.horizontalAdvance(text)
                                    : (m_textWidth > 0
                                           ? m_textWidth
                                           : metrics.horizontalAdvance(QStringLiteral("00:00")));
    int width = kTextX + textWidth;
    if (!m_closing)
        width += 8 + kButtonCount * kButtonSize + (kButtonCount - 1) * kButtonGap;
    return width + kPad;
}

void RecordingControls::updateWidth()
{
    const int target = preferredWidth();
    if (target == width())
        return;
    // Keep the bar where the person left it instead of snapping back to the
    // default spot when the saving state shortens it.
    const QPoint center = frameGeometry().center();
    setFixedSize(target, kWindowHeight);
    move(clampedToScreen(QPoint(center.x() - target / 2, center.y() - kHeight / 2),
                         size(),
                         QCursor::pos()));
    update();
}

QRectF RecordingControls::buttonRect(int index) const
{
    const int right = width() - kPad;
    const int x = right - (kButtonCount - index) * kButtonSize
                  - (kButtonCount - 1 - index) * kButtonGap;
    return QRectF(x, (kHeight - kButtonSize) / 2.0, kButtonSize, kButtonSize);
}

RecordingControls::Button RecordingControls::hitTest(const QPoint& point) const
{
    if (!m_closing) {
        static const Button order[] = {Button::Pause, Button::Restart, Button::Stop,
                                       Button::Discard};
        for (int i = 0; i < kButtonCount; ++i) {
            if (buttonRect(i).contains(point))
                return order[i];
        }
    }
    return Button::Body;
}

void RecordingControls::paintEvent(QPaintEvent*)
{
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing, true);

    const QRectF chrome(0.5, 0.5, width() - 1.0, kHeight - 1.0);
    constexpr qreal radius = 10.0;

    QPainterPath shadow;
    shadow.addRoundedRect(chrome.translated(0, 3), radius, radius);
    painter.fillPath(shadow, QColor(0, 0, 0, 100));

    QPainterPath chromePath;
    chromePath.addRoundedRect(chrome, radius, radius);
    painter.fillPath(chromePath, kChrome);
    painter.setPen(QPen(kChromeRim, 1.0));
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(chromePath);

    // Drag dots.
    painter.setPen(Qt::NoPen);
    painter.setBrush(QColor(kMuted.red(), kMuted.green(), kMuted.blue(), 150));
    const qreal handleX = kPad + kHandleWidth / 2.0;
    const qreal handleY = kHeight / 2.0;
    for (int column = 0; column < 2; ++column) {
        for (int row = 0; row < 3; ++row) {
            painter.drawEllipse(QPointF(handleX + (column == 0 ? -2.5 : 2.5),
                                        handleY + (row - 1) * 4.0),
                                1.3,
                                1.3);
        }
    }

    // Record dot and timer.
    const QPointF dot(kPad + kHandleWidth + 10 + kDotSize / 2.0, kHeight / 2.0);
    painter.setBrush(m_closing ? kMuted : kRecord);
    painter.drawEllipse(dot, kDotSize / 2.0, kDotSize / 2.0);

    painter.setFont(uiFont(15, true));
    painter.setPen(m_closing ? kMuted : kText);
    const int buttonsStart =
        m_closing ? width() - kPad : int(buttonRect(0).left()) - 8;
    painter.drawText(
        QRectF(kTextX, 0, std::max(0, buttonsStart - kTextX), kHeight),
        Qt::AlignVCenter | Qt::AlignLeft,
        m_closing ? QStringLiteral("Saving") : elapsedText());

    if (m_closing)
        return;

    // Pause / resume.
    if (m_hovered == Button::Pause) {
        painter.setPen(Qt::NoPen);
        painter.setBrush(kHover);
        painter.drawEllipse(buttonRect(0).center(), kButtonSize / 2.0, kButtonSize / 2.0);
    }
    drawIcon(painter,
             m_session->isPaused() ? kGlyphPlay : kGlyphPause,
             buttonRect(0),
             kText);

    // Restart.
    if (m_hovered == Button::Restart) {
        painter.setPen(Qt::NoPen);
        painter.setBrush(kHover);
        painter.drawEllipse(buttonRect(1).center(), kButtonSize / 2.0, kButtonSize / 2.0);
    }
    drawIcon(painter, kGlyphRestart, buttonRect(1), kText);

    // Stop: a filled red rounded square, as in the reference.
    if (m_hovered == Button::Stop) {
        painter.setPen(Qt::NoPen);
        painter.setBrush(kHover);
        painter.drawEllipse(buttonRect(2).center(), kButtonSize / 2.0, kButtonSize / 2.0);
    }
    painter.setPen(Qt::NoPen);
    painter.setBrush(kRecord);
    painter.drawRoundedRect(QRectF(buttonRect(2).center() - QPointF(5.5, 5.5),
                                   QSizeF(11.0, 11.0)),
                            3.0,
                            3.0);

    // Discard.
    if (m_hovered == Button::Discard) {
        painter.setPen(Qt::NoPen);
        painter.setBrush(kHover);
        painter.drawEllipse(buttonRect(3).center(), kButtonSize / 2.0, kButtonSize / 2.0);
    }
    drawIcon(painter, kGlyphTrash, buttonRect(3), kText);
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
        updateWidth();
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
    case Button::Restart:
        if (m_closing)
            return;
        emit restartRequested();
        return;
    case Button::Discard:
        if (m_closing)
            return;
        m_closing = true;
        updateWidth();
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
        move(clampedToScreen(event->globalPos() - m_dragOffset, size(), event->globalPos()));
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
        y = available.top() + 16;
    } else {
        constexpr int gap = 12;
        const int minY = available.top() + 16;
        const int maxY = available.bottom() - height() - 16;

        x = region.center().x() - width() / 2;

        // Prefer the dimmed space below the selection, then above it. Only
        // when neither side fits does the bar move inside the recording, and
        // then it stays at the top so the bottom of the area stays readable.
        y = region.bottom() + 1 + gap;
        if (y > maxY) {
            const int above = region.top() - gap - height();
            y = above >= minY ? above : region.top() + 16;
        }

        x = std::max(available.left() + 16,
                     std::min(x, available.right() - width() - 16));
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
