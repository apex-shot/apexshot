#include "platform/windows/scroll_controls.h"

#include "platform/platform.h"
#include "platform/windows/scroll_capture.h"

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
#include <QToolTip>

#include <algorithm>
#include <cmath>

namespace {

// A compact single-row pill, the same shape and tokens as the recording bar:
// drag dots, an accent dot with the frame count, then Finish and discard. The
// bar is sized from its contents so the stitching state shrinks instead of
// leaving a gap on the right.
constexpr int kHeight = 46;
// The window carries a few extra pixels below the body so the drop shadow's
// rounded bottom edge fits instead of being clipped into a square band.
constexpr int kShadowDrop = 3;
constexpr int kWindowHeight = kHeight + kShadowDrop;
constexpr int kPad = 10;
constexpr int kButtonSize = 30;
constexpr int kFinishGap = 8;
constexpr int kTextGap = 12;
constexpr int kHandleWidth = 8;
constexpr int kDotSize = 9;
constexpr int kIconSize = 18;
constexpr int kTextX = kPad + kHandleWidth + 10 + kDotSize + 8;
constexpr int kFinishHeight = 30;
constexpr int kFinishPadX = 14;
constexpr int kMargin = 24;

const QColor kChrome(20, 20, 20, 250);
const QColor kChromeRim(255, 255, 255, 26);
const QColor kText(245, 245, 246);
const QColor kMuted(158, 158, 164);
const QColor kHover(255, 255, 255, 20);
const QColor kAccent(255, 102, 0);
const QColor kAccentHover(255, 124, 32);

// Lucide glyphs, so the bar uses the same icon set as the rest of the shell.
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
        const int id = QFontDatabase::addApplicationFont(QStringLiteral(":/fonts/Lucide.ttf"));
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

int finishWidth()
{
    static const int width =
        QFontMetrics(uiFont(12, true)).horizontalAdvance(QStringLiteral("Finish"))
        + kFinishPadX * 2;
    return width;
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

    m_textWidth = QFontMetrics(uiFont(15, true)).horizontalAdvance(statusText());
    setFixedSize(preferredWidth(), kWindowHeight);

    connect(m_session, &Platform::ScrollCaptureSession::frameCaptured, this,
            [this](int frames, int) {
                m_frames = frames;
                updateWidth();
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

QString ScrollControls::statusText() const
{
    if (m_closing)
        return QStringLiteral("Stitching...");
    if (m_frames <= 0)
        return QStringLiteral("Starting...");
    return QStringLiteral("%1 frames").arg(m_frames);
}

int ScrollControls::preferredWidth() const
{
    int width = kTextX + m_textWidth;
    if (m_closing)
        return width + kPad;
    return width + kTextGap + finishWidth() + kFinishGap + kButtonSize + kPad;
}

void ScrollControls::updateWidth()
{
    const QString text = statusText();
    const int needed = QFontMetrics(uiFont(15, true)).horizontalAdvance(text);
    bool changed = false;
    if (needed > m_textWidth) {
        m_textWidth = needed;
        changed = true;
    }

    const int target = preferredWidth();
    if (target == width() && !changed)
        return;

    // Keep the bar where the person left it instead of snapping back to the
    // default spot when the stitching state shortens it.
    const QPoint center = frameGeometry().center();
    setFixedSize(target, kWindowHeight);
    move(clampedToScreen(QPoint(center.x() - target / 2, center.y() - kHeight / 2),
                         size(),
                         QCursor::pos()));
    update();
}

QRectF ScrollControls::finishRect() const
{
    const int right = width() - kPad - kButtonSize - kFinishGap;
    return QRectF(right - finishWidth(), (kHeight - kFinishHeight) / 2.0,
                  finishWidth(), kFinishHeight);
}

QRectF ScrollControls::discardRect() const
{
    return QRectF(width() - kPad - kButtonSize, (kHeight - kButtonSize) / 2.0,
                  kButtonSize, kButtonSize);
}

ScrollControls::Button ScrollControls::hitTest(const QPoint& point) const
{
    if (m_closing)
        return Button::Body;
    if (finishRect().contains(point))
        return Button::Finish;
    if (discardRect().contains(point))
        return Button::Discard;
    return Button::Body;
}

void ScrollControls::paintEvent(QPaintEvent*)
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

    // Accent dot and the frame count.
    const QPointF dot(kPad + kHandleWidth + 10 + kDotSize / 2.0, kHeight / 2.0);
    painter.setBrush(m_closing ? kMuted : kAccent);
    painter.drawEllipse(dot, kDotSize / 2.0, kDotSize / 2.0);

    const int buttonsStart = m_closing ? width() - kPad : int(finishRect().left()) - kTextGap;
    painter.setFont(uiFont(15, true));
    painter.setPen(m_closing ? kMuted : kText);
    painter.drawText(QRectF(kTextX, 0, std::max(0, buttonsStart - kTextX), kHeight),
                     Qt::AlignVCenter | Qt::AlignLeft,
                     statusText());

    if (m_closing)
        return;

    const QRectF finish = finishRect();
    if (m_hovered == Button::Finish) {
        painter.setPen(Qt::NoPen);
        painter.setBrush(kAccentHover);
    } else {
        painter.setPen(Qt::NoPen);
        painter.setBrush(kAccent);
    }
    painter.drawRoundedRect(finish, 8, 8);
    painter.setFont(uiFont(12, true));
    painter.setPen(Qt::white);
    painter.drawText(finish, Qt::AlignCenter, QStringLiteral("Finish"));

    const QRectF discard = discardRect();
    if (m_hovered == Button::Discard) {
        painter.setPen(Qt::NoPen);
        painter.setBrush(kHover);
        painter.drawEllipse(discard.center(), discard.width() / 2.0, discard.height() / 2.0);
    }
    drawIcon(painter, kGlyphTrash, discard, kText);
}

void ScrollControls::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) {
        QWidget::mousePressEvent(event);
        return;
    }

    switch (hitTest(event->pos())) {
    case Button::Finish:
        if (m_closing)
            return;
        m_closing = true;
        updateWidth();
        m_session->stop();
        update();
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
        if (hovered == Button::Finish && !m_closing) {
            QToolTip::showText(event->globalPos(),
                               QStringLiteral("Stop now and stitch what has been captured"),
                               this);
        } else {
            QToolTip::hideText();
        }
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
    QToolTip::hideText();
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
