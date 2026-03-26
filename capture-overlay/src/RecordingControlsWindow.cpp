#include "RecordingControlsWindow.h"

#include <QApplication>
#include <QCoreApplication>
#include <QDBusInterface>
#include <QDBusReply>
#include <QFrame>
#include <QGuiApplication>
#include <QHBoxLayout>
#include <QMenu>
#include <QPainter>
#include <QPainterPath>
#include <QScreen>
#include <QShowEvent>
#include <QVBoxLayout>
#include <algorithm>
#include <cmath>

namespace {

constexpr int kBarWidth = 380;
constexpr int kBarHeight = 62;
constexpr int kMargin = 24;
constexpr int kDockSafe = 64;
constexpr int kGap = 2;

class IconButton : public QPushButton
{
public:
    enum class Kind {
        Stop,
        Pause,
        Play,
        Restart,
        Delete,
        Menu,
    };

    explicit IconButton(Kind kind, QWidget* parent = nullptr)
      : QPushButton(parent)
      , m_kind(kind)
    {
        setCursor(Qt::PointingHandCursor);
        setFlat(true);
        setFixedSize(62, 52);
    }

    void setKind(Kind kind)
    {
        m_kind = kind;
        update();
    }

protected:
    void paintEvent(QPaintEvent* event) override
    {
        Q_UNUSED(event);

        QPainter painter(this);
        painter.setRenderHint(QPainter::Antialiasing, true);

        QColor bg = Qt::transparent;
        if (isDown()) {
            bg = QColor(255, 255, 255, 34);
        } else if (underMouse()) {
            bg = QColor(255, 255, 255, 22);
        }

        if (bg != Qt::transparent) {
            painter.setPen(Qt::NoPen);
            painter.setBrush(bg);
            painter.drawRoundedRect(rect().adjusted(2, 2, -2, -2), 10, 10);
        }

        const QPointF c(width() / 2.0, height() / 2.0);
        QPen iconPen(QColor(245, 245, 246), 1.8, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin);
        painter.setPen(iconPen);
        painter.setBrush(Qt::NoBrush);

        switch (m_kind) {
        case Kind::Stop: {
            QPen ringPen(QColor(244, 99, 87), 1.8);
            painter.setPen(ringPen);
            painter.drawEllipse(QRectF(c.x() - 10.0, c.y() - 10.0, 20.0, 20.0));
            painter.setBrush(QColor(244, 99, 87));
            painter.setPen(Qt::NoPen);
            painter.drawRoundedRect(QRectF(c.x() - 4.0, c.y() - 4.0, 8.0, 8.0), 1.5, 1.5);
            break;
        }
        case Kind::Pause: {
            painter.drawEllipse(QRectF(c.x() - 10.0, c.y() - 10.0, 20.0, 20.0));
            iconPen.setWidthF(2.2);
            painter.setPen(iconPen);
            painter.drawLine(QPointF(c.x() - 3.5, c.y() - 5.5), QPointF(c.x() - 3.5, c.y() + 5.5));
            painter.drawLine(QPointF(c.x() + 3.5, c.y() - 5.5), QPointF(c.x() + 3.5, c.y() + 5.5));
            break;
        }
        case Kind::Play: {
            painter.drawEllipse(QRectF(c.x() - 10.0, c.y() - 10.0, 20.0, 20.0));
            QPainterPath path;
            path.moveTo(c.x() - 3.0, c.y() - 5.5);
            path.lineTo(c.x() + 5.0, c.y());
            path.lineTo(c.x() - 3.0, c.y() + 5.5);
            path.closeSubpath();
            painter.fillPath(path, QColor(245, 245, 246));
            break;
        }
        case Kind::Restart: {
            double r = 8.5;
            QRectF arcRect(c.x() - r, c.y() - r, r * 2.0, r * 2.0);
            painter.drawArc(arcRect, 60 * 16, 280 * 16);
            
            // Arrow head at the end (roughly 60 degrees)
            QPainterPath head;
            head.moveTo(c.x() + r * cos(60 * M_PI / 180.0), c.y() - r * sin(60 * M_PI / 180.0));
            head.lineTo(c.x() + (r + 4.0) * cos(35 * M_PI / 180.0), c.y() - (r + 4.0) * sin(35 * M_PI / 180.0));
            head.lineTo(c.x() + (r - 4.0) * cos(35 * M_PI / 180.0), c.y() - (r - 4.0) * sin(35 * M_PI / 180.0));
            head.closeSubpath();
            painter.fillPath(head, QColor(245, 245, 246));
            break;
        }
        case Kind::Delete: {
            painter.setPen(iconPen);
            painter.drawRoundedRect(QRectF(c.x() - 5.5, c.y() - 3.5, 11.0, 13.0), 1.5, 1.5);
            painter.drawLine(QPointF(c.x() - 8.0, c.y() - 5.0), QPointF(c.x() + 8.0, c.y() - 5.0));
            painter.drawRoundedRect(QRectF(c.x() - 2.5, c.y() - 7.5, 5.0, 2.5), 0.8, 0.8);
            painter.drawLine(QPointF(c.x() - 2.0, c.y() - 1.0), QPointF(c.x() - 2.0, c.y() + 6.0));
            painter.drawLine(QPointF(c.x() + 2.0, c.y() - 1.0), QPointF(c.x() + 2.0, c.y() + 6.0));
            break;
        }
        case Kind::Menu: {
            painter.setPen(QPen(QColor(160, 160, 165), 1.8));
            double mw = 5.0;
            painter.drawLine(QPointF(c.x() - mw, c.y() - 5.0), QPointF(c.x() + mw, c.y() - 5.0));
            painter.drawLine(QPointF(c.x() - mw, c.y()), QPointF(c.x() + mw, c.y()));
            painter.drawLine(QPointF(c.x() - mw, c.y() + 5.0), QPointF(c.x() + mw, c.y() + 5.0));
            break;
        }
        }
    }

private:
    Kind m_kind;
};

static QFrame* separator()
{
    auto* container = new QFrame;
    container->setFixedWidth(1);
    auto* layout = new QVBoxLayout(container);
    layout->setContentsMargins(0, 18, 0, 18);
    auto* line = new QFrame(container);
    line->setFixedWidth(1);
    line->setStyleSheet("background: rgba(255, 255, 255, 0.08);");
    layout->addWidget(line);
    return container;
}

static QRect fallbackScreenRect()
{
    return QRect(0, 0, 1920, 1080);
}

static QScreen* screenForCaptureRect(const QRect& captureRect)
{
    if (!captureRect.isEmpty()) {
        if (QScreen* exact = QGuiApplication::screenAt(captureRect.center())) {
            return exact;
        }

        int bestArea = 0;
        QScreen* bestScreen = nullptr;
        for (QScreen* screen : QGuiApplication::screens()) {
            const QRect overlap = screen->availableGeometry().intersected(captureRect);
            const int area = overlap.width() * overlap.height();
            if (area > bestArea) {
                bestArea = area;
                bestScreen = screen;
            }
        }
        if (bestScreen) {
            return bestScreen;
        }
    }

    return QGuiApplication::primaryScreen();
}

static QRect availableScreenRect(const QRect& captureRect)
{
    if (QScreen* screen = screenForCaptureRect(captureRect)) {
        return screen->availableGeometry();
    }
    return fallbackScreenRect();
}

static QPoint computeBarPosition(const QRect& captureRect, bool isFullscreen)
{
    const QRect screenRect = availableScreenRect(captureRect);
    const int minX = screenRect.x() + kMargin;
    const int maxX = std::max(minX, screenRect.x() + screenRect.width() - kBarWidth - kMargin);
    const int topY = screenRect.y() + kMargin;

    if (isFullscreen || captureRect.isEmpty()) {
        return QPoint(
          screenRect.x() + (screenRect.width() - kBarWidth) / 2,
          topY);
    }

    const int x = std::clamp(
      captureRect.x() + (captureRect.width() - kBarWidth) / 2,
      minX,
      maxX);

    const int belowY = captureRect.y() + captureRect.height() + kGap;
    const int maxY = screenRect.y() + screenRect.height() - kBarHeight - kMargin;
    if (belowY + kBarHeight + kDockSafe <= screenRect.y() + screenRect.height()) {
        return QPoint(x, belowY);
    }

    const int aboveY = captureRect.y() - kBarHeight - kGap;
    if (aboveY >= topY) {
        return QPoint(x, aboveY);
    }

    return QPoint(x, std::clamp(aboveY, topY, maxY));
}

} // namespace

RecordingControlsWindow::RecordingControlsWindow(const QString& dbusDest,
                                                 const QString& sessionId,
                                                 const QRect& captureRect,
                                                 bool isFullscreen,
                                                 bool showTimer,
                                                 QWidget* parent)
  : QWidget(parent)
  , m_dbusDest(dbusDest)
  , m_sessionId(sessionId)
  , m_captureRect(captureRect)
  , m_isFullscreen(isFullscreen)
  , m_showTimer(showTimer)
{
    setWindowFlags(Qt::FramelessWindowHint | Qt::Tool | Qt::WindowStaysOnTopHint);
    setAttribute(Qt::WA_TranslucentBackground, true);
    setFixedSize(kBarWidth, kBarHeight);
    setupUi();
    resetTimer();
}

void RecordingControlsWindow::showEvent(QShowEvent* event)
{
    QWidget::showEvent(event);
    positionWindow();
    if (!m_positioned) {
        QTimer::singleShot(0, this, [this]() { positionWindow(); });
        m_positioned = true;
    }
}

void RecordingControlsWindow::closeEvent(QCloseEvent* event)
{
    if (m_uiTimer) {
        m_uiTimer->stop();
    }
    QWidget::closeEvent(event);
    QCoreApplication::quit();
}

void RecordingControlsWindow::setupUi()
{
    auto* outer = new QVBoxLayout(this);
    outer->setContentsMargins(0, 0, 0, 0);

    auto* chrome = new QFrame(this);
    chrome->setObjectName("recordingControlsChrome");
    chrome->setStyleSheet(
      "#recordingControlsChrome {"
      "background: rgb(20, 20, 20);"
      "border: 1px solid rgba(255, 255, 255, 0.10);"
      "border-radius: 12px;"
      "}"
      "QLabel { color: rgb(246, 246, 247); }");

    auto* row = new QHBoxLayout(chrome);
    row->setContentsMargins(6, 5, 6, 5);
    row->setSpacing(0);

    auto* stopSegment = new QFrame(chrome);
    stopSegment->setStyleSheet(
      "background: rgb(30, 31, 34);"
      "border-radius: 8px;");
    auto* stopLayout = new QHBoxLayout(stopSegment);
    stopLayout->setContentsMargins(12, 6, 16, 6);
    stopLayout->setSpacing(8);

    auto* stopButton = new IconButton(IconButton::Kind::Stop, stopSegment);
    stopButton->setFixedSize(28, 28);
    stopButton->setStyleSheet("background: transparent;");
    stopLayout->addWidget(stopButton, 0, Qt::AlignVCenter);

    m_timerLabel = new QLabel(QStringLiteral("0:00"), stopSegment);
    QFont timerFont = font();
    timerFont.setBold(true);
    timerFont.setPointSizeF(15.0);
    m_timerLabel->setFont(timerFont);
    m_timerLabel->setStyleSheet("color: rgb(244, 99, 87); background: transparent;");
    m_timerLabel->setVisible(m_showTimer);
    stopLayout->addWidget(m_timerLabel, 0, Qt::AlignVCenter);

    row->addWidget(stopSegment);
    row->addWidget(separator());

    m_pauseButton = new IconButton(IconButton::Kind::Pause, chrome);
    row->addWidget(m_pauseButton);
    row->addWidget(separator());

    m_restartButton = new IconButton(IconButton::Kind::Restart, chrome);
    row->addWidget(m_restartButton);
    row->addWidget(separator());

    m_deleteButton = new IconButton(IconButton::Kind::Delete, chrome);
    row->addWidget(m_deleteButton);
    row->addWidget(separator());

    m_menuButton = new IconButton(IconButton::Kind::Menu, chrome);
    row->addWidget(m_menuButton);

    outer->addWidget(chrome);

    m_uiTimer = new QTimer(this);
    m_uiTimer->setInterval(250);
    connect(m_uiTimer, &QTimer::timeout, this, [this]() { updateTimerText(); });
    m_uiTimer->start();

    connect(stopButton, &QPushButton::clicked, this, [this]() {
        if (sendCommand(QStringLiteral("Stop"))) {
            close();
        }
    });

    connect(m_pauseButton, &QPushButton::clicked, this, [this]() {
        const QString method = m_paused ? QStringLiteral("Resume") : QStringLiteral("Pause");
        if (sendCommand(method)) {
            setPaused(!m_paused);
        }
    });

    connect(m_restartButton, &QPushButton::clicked, this, [this]() {
        if (sendCommand(QStringLiteral("Restart"))) {
            setPaused(false);
            resetTimer();
        }
    });

    connect(m_deleteButton, &QPushButton::clicked, this, [this]() {
        if (sendCommand(QStringLiteral("Discard"))) {
            close();
        }
    });

    connect(m_menuButton, &QPushButton::clicked, this, [this]() {
        QMenu menu(this);
        menu.addAction(QStringLiteral("ApexShot recording controls"))->setEnabled(false);
        menu.exec(m_menuButton->mapToGlobal(QPoint(0, m_menuButton->height() + 6)));
    });
}

void RecordingControlsWindow::positionWindow()
{
    move(computeBarPosition(m_captureRect, m_isFullscreen));
    raise();
}

bool RecordingControlsWindow::sendCommand(const QString& methodName)
{
    QDBusInterface iface(
      m_dbusDest,
      QStringLiteral("/org/apexshot/RecordingControl"),
      QStringLiteral("org.apexshot.RecordingControl"),
      QDBusConnection::sessionBus());

    if (!iface.isValid()) {
        return false;
    }

    QDBusReply<bool> reply = iface.call(methodName, m_sessionId);
    return reply.isValid() && reply.value();
}

void RecordingControlsWindow::setPaused(bool paused)
{
    if (m_paused == paused) {
        return;
    }

    if (paused) {
        m_elapsedBeforePauseMs += m_elapsedTimer.elapsed();
        m_uiTimer->stop();
    } else {
        m_elapsedTimer.restart();
        m_uiTimer->start();
    }

    m_paused = paused;
    static_cast<IconButton*>(m_pauseButton)
      ->setKind(paused ? IconButton::Kind::Play : IconButton::Kind::Pause);
    updateTimerText();
}

void RecordingControlsWindow::resetTimer()
{
    m_elapsedBeforePauseMs = 0;
    m_paused = false;
    m_elapsedTimer.restart();
    if (m_uiTimer) {
        m_uiTimer->start();
    }
    if (m_pauseButton) {
        static_cast<IconButton*>(m_pauseButton)->setKind(IconButton::Kind::Pause);
    }
    updateTimerText();
}

void RecordingControlsWindow::updateTimerText()
{
    if (!m_timerLabel) {
        return;
    }

    qint64 elapsedMs = m_elapsedBeforePauseMs;
    if (!m_paused) {
        elapsedMs += m_elapsedTimer.elapsed();
    }
    m_timerLabel->setText(formatElapsed(elapsedMs));
}

QString RecordingControlsWindow::formatElapsed(qint64 elapsedMs) const
{
    const qint64 totalSeconds = std::max<qint64>(0, elapsedMs / 1000);
    const qint64 minutes = totalSeconds / 60;
    const qint64 seconds = totalSeconds % 60;
    return QStringLiteral("%1:%2")
      .arg(minutes)
      .arg(seconds, 2, 10, QLatin1Char('0'));
}
