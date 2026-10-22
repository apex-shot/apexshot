// SPDX-License-Identifier: GPL-3.0-or-later

#include "CaptureModeToolbar.h"
#include "Sandbox.h"

#include <QApplication>
#include <QCoreApplication>
#include <QCursor>
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
#include <QToolTip>
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
constexpr int kItemWidths[] = {66, 58, 76, 76, 66, 62, 68, 58, 52};
const char* kLabels[] = {"Shot", "Video", "Display", "Window", "Area", "OCR", "Timer",
                         "Scroll", ""};

int toolbarWidth(bool enableScrollCapture)
{
    const int count = enableScrollCapture ? 9 : 8;
    int width = kOuterPad * 2;
    for (int i = 0; i < count; ++i) {
        width += (i == count - 1) ? 52 : kItemWidths[i];
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

CaptureModeToolbar::CaptureModeToolbar(QScreen* screen, bool screenIsLocked,
                                       bool recordingAvailable,
                                       bool allowAreaRecording,
                                       bool enableScrollCapture)
    : QWidget(nullptr, toolbarFlags())
    , m_screen(screen)
    , m_screenIsLocked(screenIsLocked)
    , m_recordingAvailable(recordingAvailable)
    , m_allowAreaRecording(allowAreaRecording)
    , m_enableScrollCapture(enableScrollCapture)
{
    setAttribute(Qt::WA_TranslucentBackground);
    setAttribute(Qt::WA_DeleteOnClose, false);
    setFocusPolicy(Qt::StrongFocus);
    setMouseTracking(true);
    setFixedSize(toolbarWidth(m_enableScrollCapture), kPanelHeight);
    setWindowTitle(QStringLiteral("ApexShot Capture"));

    const QRect available = screen ? screen->availableGeometry() : QRect(0, 0, width(), height());
    move(available.center().x() - width() / 2, available.top() + 28);
}

CaptureModeToolbar::Result CaptureModeToolbar::choose(QLocalServer* controlServer,
                                                       QScreen* targetScreen,
                                                       bool recordingAvailable,
                                                       bool allowAreaRecording,
                                                       bool enableScrollCapture)
{
    QScreen* screen = targetScreen;
    if (!screen) {
        screen = QGuiApplication::screenAt(QCursor::pos());
    }
    if (!screen) {
        screen = QGuiApplication::primaryScreen();
    }

    CaptureModeToolbar toolbar(screen, targetScreen != nullptr, recordingAvailable,
                               allowAreaRecording, enableScrollCapture);
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
            toolbar.m_speaker,
            toolbar.m_scroll};
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

    requestShellOverlayFocus(windowTitle());
}

int CaptureModeToolbar::itemCount() const
{
    return m_enableScrollCapture ? 9 : 8;
}

int CaptureModeToolbar::itemWidth(int index) const
{
    return index == itemCount() - 1 ? 52 : kItemWidths[index];
}

QString CaptureModeToolbar::labelFor(int index) const
{
    if (index == itemCount() - 1)
        return QString();
    if (m_enableScrollCapture && index == 7)
        return QStringLiteral("Scroll");
    return QString::fromLatin1(kLabels[index]);
}

QRectF CaptureModeToolbar::itemRect(int index) const
{
    qreal x = kOuterPad;
    for (int i = 0; i < index; ++i) {
        x += itemWidth(i);
        if (separatorAfter(i)) {
            x += 13;
        }
    }
    return QRectF(x, kOuterPad, itemWidth(index), kItemHeight);
}

int CaptureModeToolbar::hitTest(const QPoint& point) const
{
    for (int i = 0; i < itemCount(); ++i) {
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

    for (int i = 0; i < itemCount(); ++i) {
        const QRectF cell = itemRect(i);
        const bool disabled = (!m_recordingAvailable && i == 1)
            || (m_recording && i == 4 && !m_allowAreaRecording)
            || (m_scroll && (i == 2 || i == 5 || i == 6))
            || (!m_recording && !m_scroll && i == 2 && m_ocr)
            || (!m_recording && !m_scroll && i == 3 && (m_timerSeconds > 0 || m_ocr))
            || (!m_recording && !m_scroll && i == 4 && m_timerSeconds > 0)
            || (!m_recording && !m_scroll && i == 5 && m_timerSeconds > 0)
            || (!m_recording && !m_scroll && i == 6 && m_ocr);
        const bool active = (i == 0 && !m_recording && !m_scroll)
            || (i == 1 && m_recording)
            || (m_enableScrollCapture && i == 7 && m_scroll)
            || (!m_recording && !m_scroll && i == 5 && m_ocr)
            || (!m_recording && !m_scroll && i == 6 && m_timerSeconds > 0)
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
        const bool isClose = i == itemCount() - 1;
        const qreal iconY = isClose ? cell.center().y() : cell.y() + 25;
        drawIcon(painter, i, QPointF(cell.center().x(), iconY), color);

        if (!isClose) {
            QFont font(QStringLiteral("Inter"));
            font.setPixelSize(11);
            font.setWeight(active ? QFont::DemiBold : QFont::Medium);
            painter.setFont(font);
            painter.setPen(active ? Qt::white : (disabled ? QColor(115, 115, 122) : kMuted));
            QString label = labelFor(i);
            if (m_recording && i == 5) {
                label = QStringLiteral("Mic");
            } else if (m_recording && i == 6) {
                label = QStringLiteral("Speaker");
            } else if (!m_recording && !m_scroll && i == 6 && m_timerSeconds > 0) {
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
    case 7: // Scroll (Quick Access) or Close (8-item toolbar)
        if (m_enableScrollCapture) {
            painter.drawRoundedRect(QRectF(x - 6, y - 10, 12, 20), 6, 6);
            painter.drawLine(QPointF(x, y - 10), QPointF(x, y - 4));
            painter.drawLine(QPointF(x - 4, y + 2), QPointF(x, y + 6));
            painter.drawLine(QPointF(x + 4, y + 2), QPointF(x, y + 6));
            break;
        }
        painter.setPen(QPen(color, 2.6, Qt::SolidLine, Qt::RoundCap));
        painter.drawLine(QPointF(x - 6, y - 6), QPointF(x + 6, y + 6));
        painter.drawLine(QPointF(x + 6, y - 6), QPointF(x - 6, y + 6));
        break;
    case 8: // Close, when Scroll occupies 7
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
        const bool disabled = (!m_recordingAvailable && hovered == 1)
            || (m_recording && hovered == 4 && !m_allowAreaRecording)
            || (m_scroll && (hovered == 2 || hovered == 5 || hovered == 6))
            || (!m_recording && !m_scroll && hovered == 2 && m_ocr)
            || (!m_recording && !m_scroll && hovered == 3 && (m_timerSeconds > 0 || m_ocr))
            || (!m_recording && !m_scroll && hovered == 4 && m_timerSeconds > 0)
            || (!m_recording && !m_scroll && hovered == 5 && m_timerSeconds > 0)
            || (!m_recording && !m_scroll && hovered == 6 && m_ocr);
        setCursor(disabled || hovered < 0 ? Qt::ArrowCursor : Qt::PointingHandCursor);
        // A disabled item never hovers, so explain the Video slot on itself.
        if (!m_recordingAvailable && hovered == 1) {
            QToolTip::showText(event->globalPos(),
                               QStringLiteral("Video recording is not supported on this system"),
                               this);
        } else {
            QToolTip::hideText();
        }
        update();
    }
}

void CaptureModeToolbar::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton)
        return;

    const int index = hitTest(event->pos());
    if (index < 0)
        return;

    if (index == itemCount() - 1) {
        finish(Action::Cancel);
        return;
    }

    if (m_enableScrollCapture && index == 7) {
        m_recording = false;
        m_scroll = !m_scroll;
        if (m_scroll) {
            m_ocr = false;
            m_timerSeconds = 0;
        }
        update();
        return;
    }

    switch (index) {
    case 0:
        m_recording = false;
        m_scroll = false;
        update();
        break;
    case 1:
        if (!m_recordingAvailable)
            break;
        m_scroll = false;
        m_recording = true;
        m_ocr = false;
        m_timerSeconds = 0;
        update();
        break;
    case 2:
        if (m_scroll)
            break;
        if (m_recording || !m_ocr) {
            QScreen* currentScreen = m_screenIsLocked
                ? nullptr
                : QGuiApplication::screenAt(QCursor::pos());
            if (currentScreen) {
                m_screen = currentScreen;
            }
            finish(Action::Display);
        }
        break;
    case 3:
        if (m_scroll || m_recording || (m_timerSeconds == 0 && !m_ocr))
            finish(Action::Window);
        break;
    case 4:
        if ((!m_recording && !m_scroll && m_timerSeconds == 0)
            || ((m_recording || m_scroll) && m_allowAreaRecording))
            finish(Action::Area);
        break;
    case 5:
        if (m_recording) {
            m_microphone = !m_microphone;
            update();
        } else if (!m_scroll && m_timerSeconds == 0) {
            m_ocr = !m_ocr;
            update();
        }
        break;
    case 6:
        if (m_recording) {
            m_speaker = !m_speaker;
            update();
        } else if (!m_scroll && !m_ocr) {
            m_timerSeconds = m_timerSeconds == 0 ? 3 : (m_timerSeconds == 3 ? 5 : (m_timerSeconds == 5 ? 10 : 0));
            update();
        }
        break;
    default:
        break;
    }
}

void CaptureModeToolbar::leaveEvent(QEvent*)
{
    QToolTip::hideText();
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
