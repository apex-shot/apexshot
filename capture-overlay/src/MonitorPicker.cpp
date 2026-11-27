// SPDX-License-Identifier: GPL-3.0-or-later
// Multi-monitor picker — floating panel only.
// Freeze is taken before this UI (see main.cpp) so picker never appears in it.

#include "MonitorPicker.h"
#include "Sandbox.h"

#include <QApplication>
#include <QCloseEvent>
#include <QCursor>
#include <QEvent>
#include <QEventLoop>
#include <QFocusEvent>
#include <QFont>
#include <QFontMetrics>
#include <QGuiApplication>
#include <QKeyEvent>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QScreen>
#include <QThread>
#include <QTimer>
#include <QWidget>
#include <QWindow>

#include <algorithm>
#include <cstdio>
#include <functional>
#include <memory>

namespace {

// The picker sits on the same sheet as the onboarding and update windows: a
// borderless #191919 surface, a centred headline over quiet copy, and
// #222222 cards. Keep these values identical to the GTK picker.
QColor warmAlpha(qreal alpha)
{
    QColor color(243, 240, 236);
    color.setAlphaF(alpha);
    return color;
}

const QColor kPanelFill(0x19, 0x19, 0x19);
const QColor kCardFill(0x22, 0x22, 0x22);
const QColor kCardHoverFill(0x2b, 0x2b, 0x2b);
const QColor kCardPressedFill(0x30, 0x30, 0x30);
const QColor kWash = warmAlpha(0.06);
const QColor kWashActive = warmAlpha(0.10);
const QColor kText(0xf3, 0xf0, 0xec);
const QColor kTextMuted(0xa3, 0xa0, 0x9c);
const QColor kCloseIdle(0xb9, 0xb4, 0xad);
const QColor kAccent(0xff, 0x67, 0x0d);

constexpr int kPanelRadius = 12;
constexpr int kInset = 36;
constexpr int kCloseSize = 28;
constexpr int kCloseMargin = 12;
constexpr int kHeadlineHeight = 34;
constexpr int kCopyGap = 6;
constexpr int kCopyHeight = 20;
constexpr int kCardsGap = 24;
constexpr int kCardGap = 12;
constexpr int kCardWidth = 220;
constexpr int kCardHeight = 182;
constexpr int kCardRadius = 9;
constexpr int kCardPad = 14;
constexpr int kPreviewWidth = 192;
constexpr int kPreviewHeight = 108;
constexpr int kMinPanelWidth = 380;

constexpr int kCardsTop = kInset + kHeadlineHeight + kCopyGap + kCopyHeight + kCardsGap;
constexpr int kPanelHeight = kCardsTop + kCardHeight + kInset;

int rowWidthFor(int count)
{
    return count * kCardWidth + (count - 1) * kCardGap;
}

int panelWidthFor(int count)
{
    return std::max(kMinPanelWidth, kInset * 2 + rowWidthFor(count));
}

class MonitorCard : public QWidget
{
public:
    // `index` is the screen's position in QGuiApplication::screens();
    // `displayNumber` is its left-to-right position, which is what the card
    // shows and what the number keys select.
    MonitorCard(int index, int displayNumber, QScreen* screen, bool isPrimary, QWidget* parent)
        : QWidget(parent)
        , m_index(index)
        , m_displayNumber(displayNumber)
        , m_hovered(false)
        , m_pressed(false)
        , m_isPrimary(isPrimary)
        , m_name(screen->name())
        , m_aspect(16.0 / 9.0)
    {
        const QSize geom = screen->geometry().size();
        m_sizeText = QStringLiteral("%1 × %2")
                       .arg(geom.width())
                       .arg(geom.height());
        if (geom.height() > 0) {
            m_aspect = qreal(geom.width()) / qreal(geom.height());
        }

        setCursor(Qt::PointingHandCursor);
        setFocusPolicy(Qt::StrongFocus);
        setAttribute(Qt::WA_Hover, true);
        setFixedSize(kCardWidth, kCardHeight);
    }

    int index() const { return m_index; }
    std::function<void(int)> onSelected;

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);

        const bool focused = hasFocus();
        const bool hovered = m_hovered;

        const bool active = hovered || focused || m_pressed;

        QPainterPath body;
        body.addRoundedRect(QRectF(rect()), kCardRadius, kCardRadius);
        p.fillPath(body, m_pressed ? kCardPressedFill
                                   : (hovered || focused ? kCardHoverFill : kCardFill));

        // The display as a plain rectangle at its real aspect ratio, centred
        // in the preview box, with the display number inside it.
        const QRectF box(kCardPad, kCardPad, kPreviewWidth, kPreviewHeight);
        qreal monW = box.width();
        qreal monH = monW / m_aspect;
        if (monH > box.height()) {
            monH = box.height();
            monW = monH * m_aspect;
        }
        const QPointF centre = box.center();
        const QRectF monitor(centre.x() - monW / 2.0, centre.y() - monH / 2.0,
                             monW, monH);
        QPainterPath screenPath;
        screenPath.addRoundedRect(monitor, 6, 6);
        p.fillPath(screenPath, active ? kWashActive : kWash);

        {
            QFont numberFont = font();
            numberFont.setPixelSize(20);
            numberFont.setWeight(QFont::DemiBold);
            p.setFont(numberFont);
            p.setPen(active ? kAccent : kText);
            p.drawText(monitor, Qt::AlignCenter, QString::number(m_displayNumber));
        }

        const qreal textX = kCardPad;
        const qreal textW = kCardWidth - kCardPad * 2;
        const qreal nameY = kCardPad + kPreviewHeight + 10;

        {
            QFont nameFont = font();
            nameFont.setPixelSize(14);
            nameFont.setWeight(QFont::Medium);
            p.setFont(nameFont);
            p.setPen(kText);
            p.drawText(QRectF(textX, nameY, textW, 18),
                       Qt::AlignLeft | Qt::AlignVCenter,
                       QStringLiteral("Display %1").arg(m_displayNumber));
        }

        {
            QFont metaFont = font();
            metaFont.setPixelSize(13);
            metaFont.setWeight(QFont::Normal);
            p.setFont(metaFont);
            p.setPen(kTextMuted);
            QString meta = QStringLiteral("%1 · %2").arg(m_name, m_sizeText);
            if (m_isPrimary) {
                meta += QStringLiteral(" · Primary");
            }
            const QString shown =
              QFontMetrics(metaFont).elidedText(meta, Qt::ElideRight, int(textW));
            p.drawText(QRectF(textX, nameY + 18 + 2, textW, 16),
                       Qt::AlignLeft | Qt::AlignVCenter, shown);
        }
    }

    void mousePressEvent(QMouseEvent* e) override
    {
        if (e->button() == Qt::LeftButton) {
            m_pressed = true;
            update();
        }
    }

    void mouseReleaseEvent(QMouseEvent* e) override
    {
        if (e->button() == Qt::LeftButton && m_pressed) {
            m_pressed = false;
            update();
            if (rect().contains(e->pos()) && onSelected) {
                onSelected(m_index);
            }
        }
    }

    void enterEvent(QEvent*) override
    {
        m_hovered = true;
        update();
    }

    void leaveEvent(QEvent*) override
    {
        m_hovered = false;
        m_pressed = false;
        update();
    }

    void keyPressEvent(QKeyEvent* event) override
    {
        if (event->key() == Qt::Key_Return || event->key() == Qt::Key_Enter
            || event->key() == Qt::Key_Space) {
            if (onSelected) {
                onSelected(m_index);
            }
            return;
        }
        QWidget::keyPressEvent(event);
    }

    void focusInEvent(QFocusEvent* e) override
    {
        QWidget::focusInEvent(e);
        update();
    }

    void focusOutEvent(QFocusEvent* e) override
    {
        QWidget::focusOutEvent(e);
        update();
    }

private:
    int m_index;
    int m_displayNumber;
    bool m_hovered;
    bool m_pressed;
    bool m_isPrimary;
    QString m_name;
    QString m_sizeText;
    qreal m_aspect;
};

class CloseButton : public QWidget
{
public:
    explicit CloseButton(QWidget* parent)
        : QWidget(parent)
        , m_hovered(false)
    {
        setFixedSize(kCloseSize, kCloseSize);
        setCursor(Qt::PointingHandCursor);
        setAttribute(Qt::WA_Hover, true);
        setToolTip(QStringLiteral("Cancel"));
    }

    std::function<void()> onCancel;

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);

        if (m_hovered) {
            QPainterPath bg;
            bg.addEllipse(QRectF(rect()));
            p.fillPath(bg, kWash);
        }

        p.setPen(QPen(m_hovered ? kText : kCloseIdle, 1.5, Qt::SolidLine, Qt::RoundCap));
        const QPointF c = QRectF(rect()).center();
        constexpr qreal kHalf = 4.5;
        p.drawLine(QPointF(c.x() - kHalf, c.y() - kHalf),
                   QPointF(c.x() + kHalf, c.y() + kHalf));
        p.drawLine(QPointF(c.x() - kHalf, c.y() + kHalf),
                   QPointF(c.x() + kHalf, c.y() - kHalf));
    }

    void enterEvent(QEvent*) override
    {
        m_hovered = true;
        update();
    }

    void leaveEvent(QEvent*) override
    {
        m_hovered = false;
        update();
    }

    void mousePressEvent(QMouseEvent* e) override
    {
        if (e->button() == Qt::LeftButton && onCancel) {
            onCancel();
        }
    }

private:
    bool m_hovered;
};

/// Compact floating panel — does NOT cover the full screen (live desktop stays).
class PickerPanel : public QWidget
{
public:
    explicit PickerPanel(QEventLoop* loop, int* result)
        : QWidget(nullptr,
                   Qt::Dialog | Qt::Tool | Qt::FramelessWindowHint
                       | Qt::WindowStaysOnTopHint
                       | (qEnvironmentVariableIsSet("WAYLAND_DISPLAY")
                              ? Qt::BypassWindowManagerHint
                              : Qt::WindowFlags()))
        , m_loop(loop)
        , m_result(result)
        , m_dismissing(false)
        , m_panelSize(kMinPanelWidth, kPanelHeight)
    {
        setAttribute(Qt::WA_TranslucentBackground);
        setAttribute(Qt::WA_OpaquePaintEvent, false);
        setAttribute(Qt::WA_DeleteOnClose, false);
        setFocusPolicy(Qt::StrongFocus);
        setMouseTracking(true);
        setWindowTitle(QStringLiteral("ApexShot Display Picker"));
    }

    void setPanelSize(const QSize& size) { m_panelSize = size; }
    /// Screen indexes in left-to-right order, for the number keys.
    void setOrder(const QList<int>& order) { m_order = order; }

    /// Unmap immediately and drain the event loop so the compositor drops the
    /// surface before freeze / capture-overlay runs (avoids ghost picker).
    void dismissAndQuit(int code)
    {
        if (m_dismissing) {
            return;
        }
        m_dismissing = true;
        *m_result = code;
        hide();
        if (windowHandle()) {
            windowHandle()->setVisible(false);
        }
        // Let Wayland process the unmap before the event loop returns.
        QApplication::processEvents(QEventLoop::ExcludeUserInputEvents);
        m_loop->quit();
    }

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);

        const QRectF panel(0, 0, m_panelSize.width(), m_panelSize.height());

        // Opaque and borderless; only the area outside the corners is clear.
        QPainterPath fillPath;
        fillPath.addRoundedRect(panel, kPanelRadius, kPanelRadius);
        p.fillPath(fillPath, kPanelFill);

        const qreal textW = panel.width() - kInset * 2;

        QFont headline = font();
        headline.setPixelSize(28);
        headline.setWeight(QFont::DemiBold);
        headline.setLetterSpacing(QFont::AbsoluteSpacing, -0.6);
        p.setFont(headline);
        p.setPen(kText);
        p.drawText(QRectF(kInset, kInset, textW, kHeadlineHeight), Qt::AlignCenter,
                   QStringLiteral("Choose a display"));

        QFont copy = font();
        copy.setPixelSize(15);
        copy.setWeight(QFont::Normal);
        p.setFont(copy);
        p.setPen(kTextMuted);
        p.drawText(QRectF(kInset, kInset + kHeadlineHeight + kCopyGap, textW, kCopyHeight),
                   Qt::AlignCenter, QStringLiteral("Pick the screen to capture."));
    }

    void keyPressEvent(QKeyEvent* event) override
    {
        if (event->key() == Qt::Key_Escape) {
            dismissAndQuit(-1);
            return;
        }
        if (event->key() >= Qt::Key_1 && event->key() <= Qt::Key_9) {
            const int position = event->key() - Qt::Key_1;
            if (position < m_order.size()) {
                dismissAndQuit(m_order.at(position));
            }
            return;
        }
        QWidget::keyPressEvent(event);
    }

    void closeEvent(QCloseEvent* event) override
    {
        if (!m_dismissing) {
            dismissAndQuit(-1);
        }
        QWidget::closeEvent(event);
    }

private:
    QEventLoop* m_loop;
    int* m_result;
    bool m_dismissing;
    QSize m_panelSize;
    QList<int> m_order;
};

} // namespace

namespace MonitorPicker {

int selectMonitorIndex(const QList<QScreen*>& screens)
{
    if (screens.isEmpty()) {
        return -1;
    }

    int result = -1;
    QEventLoop loop;

    QScreen* host = QGuiApplication::screenAt(QCursor::pos());
    if (!host) {
        host = QGuiApplication::primaryScreen();
    }
    if (!host) {
        return 0;
    }

    // Heap-allocate so we can destroy the surface before returning; stack
    // destruction alone is not enough for Wayland unmap timing.
    auto panel = std::make_unique<PickerPanel>(&loop, &result);

    const int panelWidth = panelWidthFor(screens.size());
    panel->setPanelSize(QSize(panelWidth, kPanelHeight));
    panel->setFixedSize(panelWidth, kPanelHeight);

    PickerPanel* panelPtr = panel.get();

    auto* closeButton = new CloseButton(panel.get());
    closeButton->onCancel = [panelPtr]() {
        panelPtr->dismissAndQuit(-1);
    };
    closeButton->move(panelWidth - kCloseMargin - kCloseSize, kCloseMargin);

    QList<int> sorted;
    for (int i = 0; i < screens.size(); ++i) {
        sorted.append(i);
    }
    std::sort(sorted.begin(), sorted.end(), [&](int a, int b) {
        return screens[a]->geometry().x() < screens[b]->geometry().x();
    });

    panel->setOrder(sorted);
    QScreen* primary = QGuiApplication::primaryScreen();
    MonitorCard* firstCard = nullptr;

    // One row of cards, centred in the panel however wide the panel is.
    int x = (panelWidth - rowWidthFor(screens.size())) / 2;
    const int y = kCardsTop;
    int displayNumber = 0;
    for (int i : sorted) {
        QScreen* screen = screens[i];
        auto* card =
          new MonitorCard(i, ++displayNumber, screen, screen == primary, panel.get());
        card->onSelected = [panelPtr](int index) {
            panelPtr->dismissAndQuit(index);
        };
        card->setGeometry(x, y, kCardWidth, kCardHeight);
        if (!firstCard) {
            firstCard = card;
        }
        x += kCardWidth + kCardGap;
    }

    // Center on host screen — floating only, desktop remains visible around it.
    const QRect avail = host->availableGeometry();
    panel->move(avail.center() - QPoint(panel->width() / 2, panel->height() / 2));
    if (panel->windowHandle()) {
        panel->windowHandle()->setScreen(host);
    }

    panel->show();
    panel->raise();
    panel->activateWindow();
    if (panel->windowHandle()) {
        panel->windowHandle()->requestActivate();
    }
    const auto requestShellFocus = [panelPtr]() {
        requestShellOverlayFocus(panelPtr->windowTitle());
    };
    requestShellFocus();
    QTimer::singleShot(100, panel.get(), requestShellFocus);
    QTimer::singleShot(300, panel.get(), requestShellFocus);
    panel->setFocus(Qt::ActiveWindowFocusReason);
    if (firstCard) {
        firstCard->setFocus(Qt::TabFocusReason);
    }
    QApplication::processEvents();

    loop.exec();

    // Force-destroy the surface and wait for the compositor to finish unmap
    // before the caller freezes the desktop / opens the capture overlay.
    panel->hide();
    if (panel->windowHandle()) {
        panel->windowHandle()->setVisible(false);
    }
    panel->close();
    panel.reset();
    QApplication::processEvents(QEventLoop::AllEvents, 100);
    QThread::msleep(120);
    QApplication::processEvents(QEventLoop::AllEvents, 100);

    return result;
}

QScreen* selectTargetScreen()
{
    const QList<QScreen*> screens = QGuiApplication::screens();
    if (screens.isEmpty()) {
        return nullptr;
    }

    // Single display: skip the picker and go straight to capture.
    if (screens.size() == 1) {
        return screens.first();
    }

    const int index = selectMonitorIndex(screens);
    if (index < 0 || index >= screens.size()) {
        std::fprintf(stderr, "apexshot-capture: monitor picker cancelled\n");
        return nullptr;
    }

    std::fprintf(stderr,
                 "apexshot-capture: monitor picker selected index=%d name=%s "
                 "geom=%dx%d+%d+%d\n",
                 index,
                  screens[index]->name().toLocal8Bit().constData(),
                  screens[index]->geometry().width(),
                  screens[index]->geometry().height(),
                  screens[index]->geometry().x(),
                  screens[index]->geometry().y());
    return screens[index];
}

} // namespace MonitorPicker
