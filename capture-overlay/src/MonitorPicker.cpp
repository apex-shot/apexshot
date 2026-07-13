// SPDX-License-Identifier: GPL-3.0-or-later
// Multi-monitor picker — floating panel only (live desktop stays visible).
// No freeze/screenshot. Freeze is only for the capture-area overlay.

#include "MonitorPicker.h"

#include <QApplication>
#include <QCursor>
#include <QEvent>
#include <QEventLoop>
#include <QFocusEvent>
#include <QFont>
#include <QFontMetrics>
#include <QGuiApplication>
#include <QHBoxLayout>
#include <QKeyEvent>
#include <QLabel>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QScreen>
#include <QVBoxLayout>
#include <QWidget>
#include <QWindow>

#include <algorithm>
#include <cstdio>
#include <functional>

namespace {

constexpr QColor kBgRoot(20, 20, 20); // #141414
constexpr QColor kBgElevated(30, 31, 34);
constexpr QColor kBorder(255, 255, 255, 28);
constexpr QColor kBorderHover(255, 102, 0, 200);
constexpr QColor kAccent(255, 102, 0);
constexpr QColor kTextPrimary(255, 255, 255, 240);
constexpr QColor kTextMuted(255, 255, 255, 150);
constexpr int kPreviewW = 260;
constexpr int kPreviewH = 156;
constexpr int kCardPad = 14;
constexpr int kRadius = 14;
constexpr int kPanelRadius = 18;

QPixmap displayGlyph()
{
    QPixmap pm(kPreviewW, kPreviewH);
    pm.fill(Qt::transparent);
    QPainter p(&pm);
    p.setRenderHint(QPainter::Antialiasing);

    QPainterPath bg;
    bg.addRoundedRect(QRectF(0, 0, kPreviewW, kPreviewH), 10, 10);
    p.fillPath(bg, QColor(18, 18, 22));

    const QRectF screen(26, 20, kPreviewW - 52, kPreviewH - 54);
    QPainterPath body;
    body.addRoundedRect(screen, 8, 8);
    p.fillPath(body, QColor(28, 29, 34));
    p.setPen(QPen(QColor(255, 255, 255, 30), 1.2));
    p.drawPath(body);

    const QRectF inner = screen.adjusted(8, 8, -8, -8);
    QLinearGradient g(inner.topLeft(), inner.bottomRight());
    g.setColorAt(0, QColor(42, 44, 52));
    g.setColorAt(1, QColor(32, 34, 40));
    p.fillRect(inner, g);

    p.fillRect(QRectF(inner.x() + 10, inner.y() + 14, inner.width() * 0.45, 8),
               QColor(255, 102, 0, 90));
    p.fillRect(QRectF(inner.x() + 10, inner.y() + 30, inner.width() * 0.7, 6),
               QColor(255, 255, 255, 28));
    p.fillRect(QRectF(inner.x() + 10, inner.y() + 44, inner.width() * 0.55, 6),
               QColor(255, 255, 255, 18));

    const qreal standTop = screen.bottom() + 4;
    p.setPen(Qt::NoPen);
    p.setBrush(QColor(40, 42, 48));
    p.drawRoundedRect(QRectF(kPreviewW / 2.0 - 14, standTop, 28, 8), 2, 2);
    p.drawRoundedRect(QRectF(kPreviewW / 2.0 - 36, standTop + 8, 72, 5), 2, 2);
    return pm;
}

class MonitorCard : public QWidget
{
public:
    MonitorCard(int index, QScreen* screen, bool isPrimary, QWidget* parent)
        : QWidget(parent)
        , m_index(index)
        , m_hovered(false)
        , m_pressed(false)
        , m_isPrimary(isPrimary)
        , m_glyph(displayGlyph())
        , m_name(screen->name())
        , m_sizeText(QStringLiteral("%1 × %2")
                       .arg(screen->geometry().width())
                       .arg(screen->geometry().height()))
    {
        setCursor(Qt::PointingHandCursor);
        setFocusPolicy(Qt::StrongFocus);
        setAttribute(Qt::WA_Hover, true);
        setFixedSize(kPreviewW + kCardPad * 2, kPreviewH + kCardPad * 2 + 56);
    }

    int index() const { return m_index; }
    std::function<void(int)> onSelected;

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);
        p.setRenderHint(QPainter::SmoothPixmapTransform);

        const QRectF outer = QRectF(rect()).adjusted(1.5, 1.5, -1.5, -1.5);

        {
            QPainterPath shadow;
            shadow.addRoundedRect(outer.translated(0, 3), kRadius, kRadius);
            p.fillPath(shadow, QColor(0, 0, 0, m_hovered ? 80 : 40));
        }

        QPainterPath body;
        body.addRoundedRect(outer, kRadius, kRadius);
        p.fillPath(body, m_hovered ? QColor(36, 37, 42) : kBgElevated);

        const QColor border = (m_hovered || hasFocus()) ? kBorderHover : kBorder;
        p.setPen(QPen(border, m_hovered || hasFocus() ? 1.6 : 1.0));
        p.setBrush(Qt::NoBrush);
        p.drawPath(body);

        const qreal inset = m_pressed ? 2.0 : 0.0;
        const QRectF content = outer.adjusted(kCardPad + inset, kCardPad + inset,
                                              -(kCardPad + inset), -(kCardPad + inset));

        const QRectF glyphRect(content.x(), content.y(), content.width(), kPreviewH);
        p.drawPixmap(glyphRect.toRect(), m_glyph);

        {
            const QString num = QString::number(m_index + 1);
            QFont bf = font();
            bf.setPixelSize(11);
            bf.setWeight(QFont::DemiBold);
            p.setFont(bf);
            const QFontMetrics fm(bf);
            const int bw = std::max(22, fm.horizontalAdvance(num) + 12);
            const QRectF badge(glyphRect.x() + 8, glyphRect.y() + 8, bw, 20);
            QPainterPath bp;
            bp.addRoundedRect(badge, 6, 6);
            p.fillPath(bp, m_hovered ? kAccent : QColor(0, 0, 0, 180));
            p.setPen(Qt::white);
            p.drawText(badge, Qt::AlignCenter, num);
        }

        if (m_isPrimary) {
            QFont pf = font();
            pf.setPixelSize(10);
            pf.setWeight(QFont::Medium);
            p.setFont(pf);
            const QString chip = QStringLiteral("Primary");
            const QFontMetrics fm(pf);
            const int cw = fm.horizontalAdvance(chip) + 12;
            const QRectF chipR(glyphRect.right() - cw - 8, glyphRect.y() + 8, cw, 18);
            QPainterPath cp;
            cp.addRoundedRect(chipR, 5, 5);
            p.fillPath(cp, QColor(255, 255, 255, 22));
            p.setPen(QColor(255, 255, 255, 200));
            p.drawText(chipR, Qt::AlignCenter, chip);
        }

        const qreal metaY = glyphRect.bottom() + 12;
        QFont titleF = font();
        titleF.setPixelSize(13);
        titleF.setWeight(QFont::DemiBold);
        p.setFont(titleF);
        p.setPen(kTextPrimary);
        p.drawText(QRectF(content.x(), metaY, content.width(), 18),
                   Qt::AlignLeft | Qt::AlignVCenter,
                   QStringLiteral("Display %1").arg(m_index + 1));

        QFont subF = font();
        subF.setPixelSize(11);
        p.setFont(subF);
        p.setPen(kTextMuted);
        const QString sub = QStringLiteral("%1  ·  %2").arg(m_sizeText, m_name);
        p.drawText(QRectF(content.x(), metaY + 18, content.width(), 16),
                   Qt::AlignLeft | Qt::AlignVCenter,
                   QFontMetrics(subF).elidedText(sub, Qt::ElideMiddle, int(content.width())));
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
        m_hovered = true;
        update();
    }

    void focusOutEvent(QFocusEvent* e) override
    {
        QWidget::focusOutEvent(e);
        m_hovered = false;
        update();
    }

private:
    int m_index;
    bool m_hovered;
    bool m_pressed;
    bool m_isPrimary;
    QPixmap m_glyph;
    QString m_name;
    QString m_sizeText;
};

/// Compact floating panel — does NOT cover the full screen (live desktop stays).
class PickerPanel : public QWidget
{
public:
    explicit PickerPanel(QEventLoop* loop, int* result)
        : QWidget(nullptr,
                  Qt::Dialog | Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint)
        , m_loop(loop)
        , m_result(result)
    {
        setAttribute(Qt::WA_TranslucentBackground, true);
        setFocusPolicy(Qt::StrongFocus);
        setMouseTracking(true);
    }

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);

        const QRectF r = QRectF(rect()).adjusted(6, 4, -6, -10);

        // Drop shadow
        {
            QPainterPath sh;
            sh.addRoundedRect(r.translated(0, 6), kPanelRadius, kPanelRadius);
            p.fillPath(sh, QColor(0, 0, 0, 90));
        }

        QPainterPath panel;
        panel.addRoundedRect(r, kPanelRadius, kPanelRadius);
        p.fillPath(panel, kBgRoot);
        p.setPen(QPen(QColor(255, 255, 255, 24), 1.0));
        p.setBrush(Qt::NoBrush);
        p.drawPath(panel);

        p.setPen(QPen(QColor(255, 255, 255, 16), 1.0));
        p.drawLine(int(r.left() + 22), int(r.top() + 1),
                   int(r.right() - 22), int(r.top() + 1));
    }

    void keyPressEvent(QKeyEvent* event) override
    {
        if (event->key() == Qt::Key_Escape) {
            *m_result = -1;
            m_loop->quit();
            return;
        }
        if (event->key() >= Qt::Key_1 && event->key() <= Qt::Key_9) {
            *m_result = event->key() - Qt::Key_1;
            m_loop->quit();
            return;
        }
        QWidget::keyPressEvent(event);
    }

private:
    QEventLoop* m_loop;
    int* m_result;
};

class CancelLabel : public QLabel
{
public:
    CancelLabel(QEventLoop* loop, int* result, QWidget* parent)
        : QLabel(QStringLiteral("Cancel"), parent)
        , m_loop(loop)
        , m_result(result)
    {
        setAlignment(Qt::AlignCenter);
        setCursor(Qt::PointingHandCursor);
        setStyleSheet(QStringLiteral(
          "QLabel { color: rgba(255,255,255,0.55); background: transparent; "
          "padding: 8px 16px; font-size: 12px; }"));
    }

protected:
    void mousePressEvent(QMouseEvent*) override
    {
        *m_result = -1;
        m_loop->quit();
    }

    void enterEvent(QEvent*) override
    {
        setStyleSheet(QStringLiteral(
          "QLabel { color: rgba(255,255,255,0.9); background: transparent; "
          "padding: 8px 16px; font-size: 12px; }"));
    }

    void leaveEvent(QEvent*) override
    {
        setStyleSheet(QStringLiteral(
          "QLabel { color: rgba(255,255,255,0.55); background: transparent; "
          "padding: 8px 16px; font-size: 12px; }"));
    }

private:
    QEventLoop* m_loop;
    int* m_result;
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

    PickerPanel panel(&loop, &result);

    auto* root = new QVBoxLayout(&panel);
    root->setContentsMargins(36, 28, 36, 20);
    root->setSpacing(0);

    auto* title = new QLabel(QStringLiteral("Select a display"), &panel);
    title->setAlignment(Qt::AlignCenter);
    {
        QFont f = title->font();
        f.setPixelSize(18);
        f.setWeight(QFont::DemiBold);
        title->setFont(f);
    }
    title->setStyleSheet(QStringLiteral(
      "QLabel { color: rgba(255,255,255,0.95); background: transparent; }"));
    root->addWidget(title);

    root->addSpacing(4);

    auto* hint = new QLabel(
      QStringLiteral("Click a display  ·  Esc to cancel  ·  1–%1")
        .arg(std::min(9, screens.size())),
      &panel);
    hint->setAlignment(Qt::AlignCenter);
    {
        QFont f = hint->font();
        f.setPixelSize(12);
        hint->setFont(f);
    }
    hint->setStyleSheet(QStringLiteral(
      "QLabel { color: rgba(255,255,255,0.5); background: transparent; }"));
    root->addWidget(hint);

    root->addSpacing(22);

    auto* row = new QWidget(&panel);
    row->setAttribute(Qt::WA_TranslucentBackground, true);
    auto* rowLayout = new QHBoxLayout(row);
    rowLayout->setSpacing(16);
    rowLayout->setContentsMargins(0, 0, 0, 0);
    rowLayout->setAlignment(Qt::AlignCenter);

    QList<int> sorted;
    for (int i = 0; i < screens.size(); ++i) {
        sorted.append(i);
    }
    std::sort(sorted.begin(), sorted.end(), [&](int a, int b) {
        return screens[a]->geometry().x() < screens[b]->geometry().x();
    });

    QScreen* primary = QGuiApplication::primaryScreen();
    MonitorCard* firstCard = nullptr;

    for (int i : sorted) {
        QScreen* screen = screens[i];
        auto* card = new MonitorCard(i, screen, screen == primary, row);
        card->onSelected = [&](int index) {
            result = index;
            loop.quit();
        };
        rowLayout->addWidget(card, 0, Qt::AlignTop);
        if (!firstCard) {
            firstCard = card;
        }
    }
    root->addWidget(row, 0, Qt::AlignCenter);

    root->addSpacing(12);
    root->addWidget(new CancelLabel(&loop, &result, &panel), 0, Qt::AlignCenter);

    panel.adjustSize();
    const QSize sz = panel.sizeHint().expandedTo(panel.minimumSizeHint());
    panel.resize(sz);

    // Center on host screen — floating only, desktop remains visible around it.
    const QRect avail = host->availableGeometry();
    panel.move(avail.center() - QPoint(panel.width() / 2, panel.height() / 2));
    if (panel.windowHandle()) {
        panel.windowHandle()->setScreen(host);
    }

    panel.show();
    panel.raise();
    panel.activateWindow();
    panel.setFocus(Qt::ActiveWindowFocusReason);
    if (firstCard) {
        firstCard->setFocus(Qt::TabFocusReason);
    }
    QApplication::processEvents();

    loop.exec();
    panel.hide();
    QApplication::processEvents(QEventLoop::AllEvents, 50);
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
