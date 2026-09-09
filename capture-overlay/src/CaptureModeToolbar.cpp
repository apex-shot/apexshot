// SPDX-License-Identifier: GPL-3.0-or-later

#include "CaptureModeToolbar.h"

#include <QApplication>
#include <QCoreApplication>
#include <QCursor>
#include <QDBusConnection>
#include <QDBusInterface>
#include <QEventLoop>
#include <QFont>
#include <QFontMetricsF>
#include <QGuiApplication>
#include <QKeyEvent>
#include <QLocalServer>
#include <QLocalSocket>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QScreen>
#include <QTimer>
#include <QWindow>

#include <algorithm>

namespace {

constexpr QColor kPanel(25, 25, 28, 246);
constexpr QColor kAccent(255, 102, 0);
constexpr QColor kText(245, 245, 247);
constexpr QColor kMuted(164, 164, 172);
constexpr int kPanelHeight = 82;
constexpr int kOuterPad = 8;
constexpr int kItemHeight = 66;
constexpr int kItemWidths[] = {66, 58, 76, 76, 66, 62, 68, 52};
constexpr int kItemCount = sizeof(kItemWidths) / sizeof(kItemWidths[0]);
const char* kLabels[] = {"Shot", "Video", "Display", "Window", "Area", "OCR", "Timer", ""};

int toolbarWidth()
{
    int width = kOuterPad * 2;
    for (const int itemWidth : kItemWidths) {
        width += itemWidth;
    }
    return width + 3 * 13;
}

bool separatorAfter(int index)
{
    return index == 1 || index == 4 || index == 6;
}

Qt::WindowFlags toolbarFlags()
{
    Qt::WindowFlags flags = Qt::Dialog
        | Qt::Tool
        | Qt::FramelessWindowHint
        | Qt::WindowStaysOnTopHint;
    if (qEnvironmentVariableIsSet("WAYLAND_DISPLAY")) {
        flags |= Qt::BypassWindowManagerHint;
    }
    return flags;
}

} // namespace

CaptureModeToolbar::CaptureModeToolbar(QScreen* screen)
    : QWidget(nullptr, toolbarFlags())
    , m_screen(screen)
{
    setAttribute(Qt::WA_TranslucentBackground);
    setAttribute(Qt::WA_DeleteOnClose, false);
    setFocusPolicy(Qt::StrongFocus);
    setMouseTracking(true);
    setFixedSize(toolbarWidth(), kPanelHeight);
    setWindowTitle(QStringLiteral("ApexShot Capture"));

    const QRect available = screen ? screen->availableGeometry() : QRect(0, 0, width(), height());
    move(available.center().x() - width() / 2, available.top() + 28);
}

CaptureModeToolbar::Result CaptureModeToolbar::choose(QLocalServer* controlServer)
{
    QScreen* screen = QGuiApplication::screenAt(QCursor::pos());
    if (!screen) {
        screen = QGuiApplication::primaryScreen();
    }

    CaptureModeToolbar toolbar(screen);
    QEventLoop loop;
    QObject::connect(&toolbar, &QObject::destroyed, &loop, &QEventLoop::quit);
    if (controlServer) {
        QObject::connect(controlServer, &QLocalServer::newConnection, &toolbar,
                         [controlServer, &toolbar]() {
            while (QLocalSocket* socket = controlServer->nextPendingConnection()) {
                const auto handleRequest = [socket, &toolbar]() {
                    const QByteArray request = socket->readAll().trimmed();
                    if (request == "focus") {
                        toolbar.focusAndRaise();
                    } else if (request == "cancel") {
                        toolbar.finish(Action::Cancel);
                    }
                };
                QObject::connect(socket, &QLocalSocket::readyRead, &toolbar, handleRequest);
                QObject::connect(socket, &QLocalSocket::disconnected,
                                 socket, &QObject::deleteLater);
                if (socket->bytesAvailable() > 0) {
                    handleRequest();
                }
            }
        });
    }
    toolbar.focusAndRaise();
    QTimer::singleShot(100, &toolbar, [&toolbar]() {
        if (toolbar.isVisible()) {
            toolbar.focusAndRaise();
        }
    });
    QTimer::singleShot(300, &toolbar, [&toolbar]() {
        if (toolbar.isVisible()) {
            toolbar.focusAndRaise();
        }
    });

    while (!toolbar.m_finished && toolbar.isVisible()) {
        loop.processEvents(QEventLoop::AllEvents | QEventLoop::WaitForMoreEvents);
    }

    toolbar.hide();
    if (toolbar.windowHandle()) {
        toolbar.windowHandle()->setVisible(false);
    }
    QApplication::processEvents(QEventLoop::ExcludeUserInputEvents);
    return {toolbar.m_action,
            toolbar.m_screen,
            toolbar.m_ocr,
            toolbar.m_timerSeconds,
            toolbar.m_recording,
            toolbar.m_microphone,
            toolbar.m_speaker};
}

void CaptureModeToolbar::focusAndRaise()
{
    if (isMinimized()) {
        showNormal();
    } else {
        show();
    }
    raise();
    activateWindow();
    if (windowHandle()) {
        windowHandle()->requestActivate();
    }
    setFocus(Qt::ActiveWindowFocusReason);

    QDBusInterface shellOverlay(
        QStringLiteral("org.apexshot.ShellOverlay"),
        QStringLiteral("/org/apexshot/ShellOverlay"),
        QStringLiteral("org.apexshot.ShellOverlay"),
        QDBusConnection::sessionBus());
    if (shellOverlay.isValid()) {
        shellOverlay.asyncCall(QStringLiteral("FocusCaptureMenu"),
                               static_cast<qlonglong>(QCoreApplication::applicationPid()));
    }
}

QRectF CaptureModeToolbar::itemRect(int index) const
{
    qreal x = kOuterPad;
    for (int i = 0; i < index; ++i) {
        x += kItemWidths[i];
        if (separatorAfter(i)) {
            x += 13;
        }
    }
    return QRectF(x, kOuterPad, kItemWidths[index], kItemHeight);
}

int CaptureModeToolbar::hitTest(const QPoint& point) const
{
    for (int i = 0; i < kItemCount; ++i) {
        if (itemRect(i).contains(point)) {
            return i;
        }
    }
    return -1;
}

void CaptureModeToolbar::finish(Action action)
{
    m_action = action;
    m_finished = true;
    hide();
}

void CaptureModeToolbar::paintEvent(QPaintEvent*)
{
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing);
    painter.setRenderHint(QPainter::TextAntialiasing);

    const QRectF panel = QRectF(rect()).adjusted(1.5, 1.5, -1.5, -4.5);
    QPainterPath shadow;
    shadow.addRoundedRect(panel.translated(0, 3), 22, 22);
    painter.fillPath(shadow, QColor(0, 0, 0, 105));

    QPainterPath body;
    body.addRoundedRect(panel, 22, 22);
    painter.fillPath(body, kPanel);
    painter.setPen(QPen(QColor(255, 255, 255, 54), 1.2));
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(body);

    const QRectF modeGroup(itemRect(0).left(),
                           itemRect(0).top(),
                           itemRect(1).right() - itemRect(0).left(),
                           itemRect(0).height());
    QPainterPath modeGroupPath;
    modeGroupPath.addRoundedRect(modeGroup, 15, 15);
    painter.fillPath(modeGroupPath, QColor(5, 5, 7, 235));
    painter.setPen(QPen(QColor(255, 255, 255, 18), 1));
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(modeGroupPath);

    for (int i = 0; i < kItemCount; ++i) {
        const QRectF cell = itemRect(i);
        const bool disabled = (m_recording && (i == 3 || i == 4))
            || (!m_recording && i == 2 && m_ocr)
            || (!m_recording && i == 3 && (m_timerSeconds > 0 || m_ocr))
            || (!m_recording && i == 4 && m_timerSeconds > 0)
            || (!m_recording && i == 5 && m_timerSeconds > 0)
            || (!m_recording && i == 6 && m_ocr);
        const bool active = (i == 0 && !m_recording)
            || (i == 1 && m_recording)
            || (!m_recording && i == 5 && m_ocr)
            || (!m_recording && i == 6 && m_timerSeconds > 0)
            || (m_recording && i == 5 && m_microphone)
            || (m_recording && i == 6 && m_speaker);
        const bool hovered = i == m_hovered && !disabled;

        if (active || hovered) {
            const QRectF highlight = cell.adjusted(4, 4, -4, -4);
            QPainterPath highlightPath;
            highlightPath.addRoundedRect(highlight, 13, 13);
            painter.fillPath(highlightPath,
                             active ? QColor(kAccent.red(), kAccent.green(), kAccent.blue(), 205)
                                    : QColor(255, 255, 255, 24));
            if (hovered && !active) {
                painter.setPen(QPen(QColor(kAccent.red(), kAccent.green(), kAccent.blue(), 170), 1));
                painter.drawPath(highlightPath);
            }
        }

        QColor color = active ? Qt::white : (disabled ? QColor(115, 115, 122) : kText);
        const qreal iconY = i == 7 ? cell.center().y() : cell.y() + 25;
        drawIcon(painter, i, QPointF(cell.center().x(), iconY), color);

        if (i != 7) {
            QFont font(QStringLiteral("Inter"));
            font.setPixelSize(11);
            font.setWeight(active ? QFont::DemiBold : QFont::Medium);
            painter.setFont(font);
            painter.setPen(active ? Qt::white : (disabled ? QColor(115, 115, 122) : kMuted));
            QString label = QString::fromLatin1(kLabels[i]);
            if (m_recording && i == 5) {
                label = QStringLiteral("Mic");
            } else if (m_recording && i == 6) {
                label = QStringLiteral("Speaker");
            } else if (!m_recording && i == 6 && m_timerSeconds > 0) {
                label = QStringLiteral("%1s").arg(m_timerSeconds);
            }
            painter.drawText(QRectF(cell.x(), cell.y() + 40, cell.width(), 18), Qt::AlignCenter, label);
        }

        if (separatorAfter(i)) {
            const qreal x = cell.right() + 6.5;
            painter.setPen(QPen(QColor(255, 255, 255, 35), 1));
            painter.drawLine(QPointF(x, 20), QPointF(x, height() - 20));
        }
    }
}

void CaptureModeToolbar::drawIcon(QPainter& painter,
                                  int index,
                                  const QPointF& center,
                                  const QColor& color) const
{
    painter.save();
    painter.setPen(QPen(color, 2.0, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
    painter.setBrush(Qt::NoBrush);
    const qreal x = center.x();
    const qreal y = center.y();

    switch (index) {
    case 0: // Screenshot
        painter.drawRoundedRect(QRectF(x - 10, y - 7, 20, 15), 2.5, 2.5);
        painter.drawEllipse(QPointF(x + 4.5, y - 2.5), 1.5, 1.5);
        painter.drawLine(QPointF(x - 7, y + 5), QPointF(x - 2, y));
        painter.drawLine(QPointF(x - 2, y), QPointF(x + 2, y + 4));
        break;
    case 1: // Video, reserved for the recording phase
        painter.drawRoundedRect(QRectF(x - 10, y - 7, 14, 14), 2.5, 2.5);
        painter.drawLine(QPointF(x + 5, y - 4), QPointF(x + 10, y - 7));
        painter.drawLine(QPointF(x + 10, y - 7), QPointF(x + 10, y + 7));
        painter.drawLine(QPointF(x + 10, y + 7), QPointF(x + 5, y + 4));
        break;
    case 2: // Display
        painter.drawRoundedRect(QRectF(x - 10, y - 8, 20, 14), 2, 2);
        painter.drawLine(QPointF(x, y + 6), QPointF(x, y + 10));
        painter.drawLine(QPointF(x - 5, y + 10), QPointF(x + 5, y + 10));
        break;
    case 3: // Window
        painter.drawRoundedRect(QRectF(x - 10, y - 8, 20, 16), 3, 3);
        painter.drawLine(QPointF(x - 10, y - 3), QPointF(x + 10, y - 3));
        painter.drawPoint(QPointF(x - 6, y - 5.5));
        break;
    case 4: // Area
        painter.drawLine(QPointF(x - 9, y - 3), QPointF(x - 9, y - 9));
        painter.drawLine(QPointF(x - 9, y - 9), QPointF(x - 3, y - 9));
        painter.drawLine(QPointF(x + 3, y - 9), QPointF(x + 9, y - 9));
        painter.drawLine(QPointF(x + 9, y - 9), QPointF(x + 9, y - 3));
        painter.drawLine(QPointF(x - 9, y + 3), QPointF(x - 9, y + 9));
        painter.drawLine(QPointF(x - 9, y + 9), QPointF(x - 3, y + 9));
        painter.drawLine(QPointF(x + 3, y + 9), QPointF(x + 9, y + 9));
        painter.drawLine(QPointF(x + 9, y + 9), QPointF(x + 9, y + 3));
        break;
    case 5: { // OCR / microphone
        if (m_recording) {
            painter.drawRoundedRect(QRectF(x - 4.5, y - 9, 9, 14), 4.5, 4.5);
            painter.drawLine(QPointF(x - 8, y + 1), QPointF(x - 8, y + 4));
            painter.drawArc(QRectF(x - 8, y - 2, 16, 13), 180 * 16, 180 * 16);
            painter.drawLine(QPointF(x, y + 10), QPointF(x, y + 13));
            painter.drawLine(QPointF(x - 5, y + 13), QPointF(x + 5, y + 13));
            if (!m_microphone) {
                painter.drawLine(QPointF(x - 10, y - 10), QPointF(x + 10, y + 11));
            }
            break;
        }
        QFont font(QStringLiteral("Inter"));
        font.setPixelSize(13);
        font.setWeight(QFont::Bold);
        painter.setFont(font);
        painter.setPen(color);
        painter.drawText(QRectF(x - 12, y - 10, 24, 20), Qt::AlignCenter, QStringLiteral("Aa"));
        break;
    }
    case 6: // Timer / speaker
        if (m_recording) {
            QPainterPath speaker;
            speaker.moveTo(x - 10, y - 3);
            speaker.lineTo(x - 5, y - 3);
            speaker.lineTo(x + 1, y - 9);
            speaker.lineTo(x + 1, y + 9);
            speaker.lineTo(x - 5, y + 3);
            speaker.lineTo(x - 10, y + 3);
            speaker.closeSubpath();
            painter.drawPath(speaker);
            if (m_speaker) {
                painter.drawArc(QRectF(x - 3, y - 8, 16, 16), -55 * 16, 110 * 16);
            } else {
                painter.drawLine(QPointF(x - 10, y - 10), QPointF(x + 10, y + 10));
            }
            break;
        }
        painter.drawEllipse(QPointF(x, y + 1), 8, 8);
        painter.drawLine(QPointF(x, y - 7), QPointF(x, y - 10));
        painter.drawLine(QPointF(x - 3, y - 10), QPointF(x + 3, y - 10));
        painter.drawLine(QPointF(x, y + 1), QPointF(x, y - 4));
        painter.drawLine(QPointF(x, y + 1), QPointF(x + 4, y + 3));
        break;
    case 7: // Close
        painter.setPen(QPen(color, 2.6, Qt::SolidLine, Qt::RoundCap));
        painter.drawLine(QPointF(x - 6, y - 6), QPointF(x + 6, y + 6));
        painter.drawLine(QPointF(x + 6, y - 6), QPointF(x - 6, y + 6));
        break;
    }
    painter.restore();
}

void CaptureModeToolbar::mouseMoveEvent(QMouseEvent* event)
{
    const int hovered = hitTest(event->pos());
    if (hovered != m_hovered) {
        m_hovered = hovered;
        const bool disabled = (m_recording && (hovered == 3 || hovered == 4))
            || (!m_recording && hovered == 2 && m_ocr)
            || (!m_recording && hovered == 3 && (m_timerSeconds > 0 || m_ocr))
            || (!m_recording && hovered == 4 && m_timerSeconds > 0)
            || (!m_recording && hovered == 5 && m_timerSeconds > 0)
            || (!m_recording && hovered == 6 && m_ocr);
        setCursor(disabled || hovered < 0 ? Qt::ArrowCursor : Qt::PointingHandCursor);
        update();
    }
}

void CaptureModeToolbar::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) {
        return;
    }
    switch (hitTest(event->pos())) {
    case 0:
        m_recording = false;
        update();
        break;
    case 1:
        m_recording = true;
        m_ocr = false;
        m_timerSeconds = 0;
        update();
        break;
    case 2:
        if (m_recording || !m_ocr) {
            if (QScreen* currentScreen = QGuiApplication::screenAt(QCursor::pos())) {
                m_screen = currentScreen;
            }
            finish(Action::Display);
        }
        break;
    case 3:
        if (!m_recording && m_timerSeconds == 0 && !m_ocr) {
            finish(Action::Window);
        }
        break;
    case 4:
        if (!m_recording && m_timerSeconds == 0) {
            finish(Action::Area);
        }
        break;
    case 5:
        if (m_recording) {
            m_microphone = !m_microphone;
            update();
        } else if (m_timerSeconds == 0) {
            m_ocr = !m_ocr;
            update();
        }
        break;
    case 6:
        if (m_recording) {
            m_speaker = !m_speaker;
            update();
        } else if (!m_ocr) {
            m_timerSeconds = m_timerSeconds == 0 ? 3 : (m_timerSeconds == 3 ? 5 : (m_timerSeconds == 5 ? 10 : 0));
            update();
        }
        break;
    case 7:
        finish(Action::Cancel);
        break;
    default:
        break;
    }
}

void CaptureModeToolbar::leaveEvent(QEvent*)
{
    if (m_hovered != -1) {
        m_hovered = -1;
        update();
    }
}

void CaptureModeToolbar::keyPressEvent(QKeyEvent* event)
{
    if (event->key() == Qt::Key_Escape) {
        finish(Action::Cancel);
        return;
    }
    QWidget::keyPressEvent(event);
}
