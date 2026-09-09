// SPDX-License-Identifier: GPL-3.0-or-later

#include "CaptureCountdownPill.h"

#include <QApplication>
#include <QDBusConnection>
#include <QDBusInterface>
#include <QDBusMessage>
#include <QElapsedTimer>
#include <QEventLoop>
#include <QFont>
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

namespace {

class CountdownWidget : public QWidget
{
public:
    CountdownWidget(QScreen* screen,
                    int seconds,
                    const QRect& fadeGlobalRect,
                    QEventLoop* loop)
        : QWidget(nullptr,
                  Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint
                      | Qt::BypassWindowManagerHint | Qt::Tool
                      | Qt::WindowDoesNotAcceptFocus | Qt::WindowTransparentForInput)
        , m_value(seconds)
        , m_loop(loop)
    {
        setAttribute(Qt::WA_TranslucentBackground);
        setAttribute(Qt::WA_TransparentForMouseEvents);
        setFocusPolicy(Qt::NoFocus);

        const QRect geometry = screen ? screen->geometry()
                                      : QGuiApplication::primaryScreen()->geometry();
        setGeometry(geometry);
        if (!fadeGlobalRect.isEmpty()) {
            m_fadeRect = fadeGlobalRect.translated(-geometry.topLeft());
        }

        m_timer.setInterval(1000);
        connect(&m_timer, &QTimer::timeout, this, [this]() {
            --m_value;
            if (m_value <= 0) {
                finish(true);
            } else {
                update();
            }
        });
    }

    bool completed() const { return m_completed; }
    void start() { m_timer.start(); }

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.setRenderHint(QPainter::Antialiasing);

        if (!m_fadeRect.isEmpty()) {
            painter.fillRect(m_fadeRect, QColor(12, 12, 14, 76));
        }

        const QRectF rect((width() - 118.0) / 2.0, 28.0, 118.0, 45.0);
        m_pillRect = rect;
        QPainterPath shadow;
        shadow.addRoundedRect(rect.translated(0, 3), 22, 22);
        painter.fillPath(shadow, QColor(0, 0, 0, 90));

        QPainterPath pill;
        pill.addRoundedRect(rect, 22, 22);
        painter.fillPath(pill, QColor(255, 102, 0, 242));
        painter.setPen(QPen(QColor(255, 224, 196, 120), 1));
        painter.setBrush(Qt::NoBrush);
        painter.drawPath(pill);

        const QPointF center(rect.left() + 28, rect.center().y());
        painter.setPen(QPen(Qt::white, 2, Qt::SolidLine, Qt::RoundCap));
        painter.drawEllipse(center, 10, 10);
        painter.drawLine(center, QPointF(center.x(), center.y() - 5));
        painter.drawLine(center, QPointF(center.x() + 4, center.y() + 2));

        QFont font(QStringLiteral("Inter"));
        font.setPixelSize(22);
        font.setWeight(QFont::Bold);
        painter.setFont(font);
        painter.setPen(Qt::white);
        painter.drawText(QRectF(rect.left() + 48, rect.y(), 56, rect.height()),
                         Qt::AlignCenter,
                         QString::number(m_value));
    }

    void keyPressEvent(QKeyEvent* event) override
    {
        if (event->key() == Qt::Key_Escape) {
            finish(false);
            return;
        }
        QWidget::keyPressEvent(event);
    }

    void mousePressEvent(QMouseEvent* event) override
    {
        if (event->button() == Qt::LeftButton && m_pillRect.contains(event->pos())) {
            finish(false);
        }
    }

private:
    void finish(bool completed)
    {
        m_timer.stop();
        m_completed = completed;
        hide();
        if (windowHandle()) {
            windowHandle()->setVisible(false);
        }
        QApplication::processEvents(QEventLoop::ExcludeUserInputEvents);
        m_loop->quit();
    }

    int m_value;
    bool m_completed = false;
    QEventLoop* m_loop;
    QTimer m_timer;
    QRectF m_pillRect;
    QRect m_fadeRect;
};

} // namespace

namespace CaptureCountdownPill {

bool run(QScreen* screen, int seconds, const QRect& fadeGlobalRect)
{
    if (seconds <= 0) {
        return true;
    }

    if (screen) {
        QDBusInterface shellOverlay(
            QStringLiteral("org.apexshot.ShellOverlay"),
            QStringLiteral("/org/apexshot/ShellOverlay"),
            QStringLiteral("org.apexshot.ShellOverlay"),
            QDBusConnection::sessionBus());
        if (shellOverlay.isValid()) {
            const QRect geometry = screen->geometry();
            const QDBusMessage reply = shellOverlay.call(
                QStringLiteral("ShowCaptureCountdown"),
                geometry.x(),
                geometry.y(),
                geometry.width(),
                static_cast<uint>(seconds),
                fadeGlobalRect.x(),
                fadeGlobalRect.y(),
                fadeGlobalRect.width(),
                fadeGlobalRect.height());
            if (reply.type() != QDBusMessage::ErrorMessage) {
                QElapsedTimer elapsed;
                elapsed.start();
                while (elapsed.elapsed() < seconds * 1000) {
                    QApplication::processEvents(QEventLoop::AllEvents, 20);
                    QThread::msleep(20);
                }
                shellOverlay.call(QStringLiteral("HideCountdown"));
                QApplication::processEvents(QEventLoop::AllEvents, 50);
                QThread::msleep(200);
                return true;
            }
            // A Shell method can fail after creating its actor. Always clear a
            // partial countdown before falling back to the Qt implementation.
            shellOverlay.call(QStringLiteral("HideCountdown"));
        }
    }

    QEventLoop loop;
    CountdownWidget widget(screen, seconds, fadeGlobalRect, &loop);
    widget.show();
    widget.raise();
    widget.start();
    loop.exec();

    const bool completed = widget.completed();
    QApplication::sendPostedEvents();
    QApplication::processEvents(QEventLoop::AllEvents, 50);
    if (qEnvironmentVariableIsSet("WAYLAND_DISPLAY")) {
        QThread::msleep(300);
        QApplication::processEvents(QEventLoop::AllEvents, 50);
    }
    return completed;
}

} // namespace CaptureCountdownPill
