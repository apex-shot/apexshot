// SPDX-License-Identifier: GPL-3.0-or-later

#include "CaptureModeToolbar.h"
#include "Sandbox.h"

#include <QApplication>
#include <QCoreApplication>
#include <QCursor>
#include <QDir>
#include <QEventLoop>
#include <QFont>
#include <QFile>
#include <QFontDatabase>
#include <QFontMetrics>
#include <QFontMetricsF>
#include <QGuiApplication>
#include <QHash>
#include <QIcon>
#include <QKeyEvent>
#include <QLocalServer>
#include <QLocalSocket>
#include <QMenu>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QPixmap>
#include <QScreen>
#include <QTimer>
#include <QToolTip>
#include <QWindow>

#ifdef Q_OS_WIN
#include "platform/windows/audio_devices.h"
#endif

#include <algorithm>
#include <memory>

namespace {

constexpr QColor kAccent(255, 102, 0);
constexpr QColor kText(245, 245, 247);
constexpr QColor kMuted(164, 164, 172);
constexpr int kPanelHeight = 58;
constexpr int kOuterPad = 6;
constexpr int kItemHeight = 44;
constexpr int kSeparator = 12;
constexpr int kIconSize = 17;
constexpr int kPanelRadius = 13;
constexpr int kIconPad = 12;
constexpr int kIconGap = 8;
const char* kLabels[] = {"Shot", "Video", "Display", "Window", "Area", "OCR", "Timer",
                         "Scroll", ""};


QString lucideFamily()
{
    static const QString family = [] {
        const int id =
            QFontDatabase::addApplicationFont(QStringLiteral(":/fonts/Lucide.ttf"));
        if (id < 0)
            return QString();
        const QStringList families = QFontDatabase::applicationFontFamilies(id);
        QFile log(QDir(QDir::tempPath()).filePath(
            QStringLiteral("apexshot-quickaccess.log")));
        if (log.open(QIODevice::Append | QIODevice::WriteOnly)) {
            log.write(QStringLiteral("lucide: id=%1 families=%2\n")
                          .arg(id)
                          .arg(families.join(QStringLiteral(",")))
                          .toUtf8());
        }
        return families.isEmpty() ? QString() : families.first();
    }();
    return family;
}

QPainterPath lucidePath(const QString& family, ushort glyph, int pixelSize)
{
    const QString key = QStringLiteral("%1:%2:%3").arg(family).arg(glyph).arg(pixelSize);
    static QHash<QString, QPainterPath> cache;
    const auto cached = cache.constFind(key);
    if (cached != cache.constEnd())
        return cached.value();

    QFont font(family);
    font.setPixelSize(pixelSize);
    QPainterPath path;
    path.addText(0, 0, font, QString(QChar(glyph)));
    cache.insert(key, path);
    return path;
}

/// A menu styled like a Settings dropdown that paints the tick and the
/// submenu chevron in one trailing column. QMenu puts action icons on the
/// left and its own arrow wherever the style says, so both marks are drawn
/// over the finished item geometry instead.
class DeviceMenu : public QMenu
{
public:
    using QMenu::QMenu;

protected:
    void paintEvent(QPaintEvent* event) override
    {
        QMenu::paintEvent(event);
        const QString family = lucideFamily();
        if (family.isEmpty())
            return;

        QPainter painter(this);
        painter.setRenderHint(QPainter::Antialiasing);
        QFont font(family);
        font.setPixelSize(15);
        painter.setFont(font);
        for (QAction* action : actions()) {
            if (!action->isVisible() || action->isSeparator())
                continue;
            const QRect item = actionGeometry(action);
            if (item.isNull())
                continue;
            const QRectF mark(item.right() - 24.0, item.top(), 16.0, item.height());
            if (action->menu()) {
                painter.setPen(QColor(255, 255, 255, 168));
                painter.drawText(mark, Qt::AlignCenter,
                                 QString(QChar(static_cast<ushort>(0xE06F))));
            } else if (action->property("ticked").toBool()) {
                painter.setPen(QColor(255, 255, 255, 219));
                painter.drawText(mark, Qt::AlignCenter,
                                 QString(QChar(static_cast<ushort>(0xE06C))));
            }
        }
    }
};

void roundMenu(QMenu& menu)
{
    // QMenu's native frame paints a square panel inside the rounded
    // stylesheet. Clipping the window to the same radius removes it.
    menu.ensurePolished();
    menu.adjustSize();
    QPainterPath path;
    path.addRoundedRect(QRectF(menu.rect()), 10, 10);
    menu.setMask(QRegion(path.toFillPolygon().toPolygon()));
}

QString menuLabel(const QString& text, const QFont& font)
{
    return QFontMetrics(font).elidedText(text, Qt::ElideRight, 216);
}

void applyMenuStyle(QMenu& menu)
{
    // The toolbar is topmost, so its device menus need the same z-order.
    menu.setWindowFlag(Qt::WindowStaysOnTopHint, true);
    menu.setWindowFlag(Qt::FramelessWindowHint, true);
    menu.setWindowFlag(Qt::NoDropShadowWindowHint, true);
    menu.setAttribute(Qt::WA_TranslucentBackground, true);
    menu.setAttribute(Qt::WA_NoSystemBackground, true);
    menu.setAutoFillBackground(false);
    menu.setFixedWidth(304);
    QFont font(QStringLiteral("Inter"));
    font.setPixelSize(14);
    font.setWeight(QFont::Medium);
    menu.setFont(font);
    menu.setStyleSheet(QStringLiteral(
        "QMenu { background:#252526; color: rgba(255,255,255,230);"
        " border:1px solid rgba(255,255,255,26); border-radius:10px;"
        " padding:4px; }"
        "QMenu::item { padding:7px 26px 7px 8px; border-radius:6px; }"
        "QMenu::item:selected { background: rgba(255,255,255,18); color:#ffffff; }"
        "QMenu::item:disabled { color: rgba(255,255,255,60); }"
        "QMenu::separator { height:1px; background: rgba(255,255,255,16);"
        " margin:4px 8px; }"
        "QMenu::right-arrow { image: none; width:0px; height:0px; }"));
}

QString resolveCameraDevice(const QString& preferred)
{
#ifdef Q_OS_WIN
    const QStringList cameras = Platform::directShowDevices(true);
    if (cameras.contains(preferred, Qt::CaseInsensitive))
        return preferred;
    return cameras.isEmpty() ? QString() : cameras.first();
#else
    return preferred;
#endif
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
                                       bool enableScrollCapture,
                                       const DeviceDefaults& defaults)
    : QWidget(nullptr, toolbarFlags())
    , m_screen(screen)
    , m_screenIsLocked(screenIsLocked)
    , m_recordingAvailable(recordingAvailable)
    , m_allowAreaRecording(allowAreaRecording)
    , m_enableScrollCapture(enableScrollCapture)
    , m_microphone(defaults.microphone)
    , m_microphoneDevice(defaults.microphoneDevice)
    , m_speaker(defaults.speaker)
    , m_systemAudioApplication(defaults.systemAudioApplication)
    , m_systemAudioProcessId(defaults.systemAudioProcessId)
    , m_camera(defaults.camera)
    , m_cameraDevice(defaults.cameraDevice)
{
    setAttribute(Qt::WA_TranslucentBackground);
    setAttribute(Qt::WA_DeleteOnClose, false);
    setFocusPolicy(Qt::StrongFocus);
    setMouseTracking(true);
    setFixedSize(preferredWidth(), kPanelHeight);
    setWindowTitle(QStringLiteral("ApexShot Capture"));

    const QRect available = screen ? screen->availableGeometry() : QRect(0, 0, width(), height());
    move(available.center().x() - width() / 2, available.top() + 28);

    if (m_enableScrollCapture && m_camera)
        loadCameraDefaults();
}

CaptureModeToolbar::Result CaptureModeToolbar::choose(QLocalServer* controlServer,
                                                       QScreen* targetScreen,
                                                       bool recordingAvailable,
                                                       bool allowAreaRecording,
                                                       bool enableScrollCapture,
                                                       bool persistForWindowSetup)
{
    const DeviceDefaults defaults;
    return choose(controlServer, targetScreen, recordingAvailable, allowAreaRecording,
                  enableScrollCapture, defaults, persistForWindowSetup);
}

CaptureModeToolbar::Result CaptureModeToolbar::choose(QLocalServer* controlServer,
                                                       QScreen* targetScreen,
                                                       bool recordingAvailable,
                                                       bool allowAreaRecording,
                                                       bool enableScrollCapture,
                                                       const DeviceDefaults& defaults,
                                                       bool persistForWindowSetup)
{
    QScreen* screen = targetScreen;
    if (!screen) {
        screen = QGuiApplication::screenAt(QCursor::pos());
    }
    if (!screen) {
        screen = QGuiApplication::primaryScreen();
    }

    std::unique_ptr<CaptureModeToolbar> toolbarHolder(
        new CaptureModeToolbar(screen, targetScreen != nullptr, recordingAvailable,
                               allowAreaRecording, enableScrollCapture, defaults));
    CaptureModeToolbar& toolbar = *toolbarHolder;
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
    CaptureModeToolbar* toolbarPointer = &toolbar;
    QTimer::singleShot(100, &toolbar, [toolbarPointer]() {
        if (toolbarPointer->isVisible() && !toolbarPointer->m_setupMode) {
            toolbarPointer->focusAndRaise();
        }
    });
    QTimer::singleShot(300, &toolbar, [toolbarPointer]() {
        if (toolbarPointer->isVisible() && !toolbarPointer->m_setupMode) {
            toolbarPointer->focusAndRaise();
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

    Result result{toolbar.m_action,
                  toolbar.m_screen,
                  toolbar.m_ocr,
                  toolbar.m_timerSeconds,
                  toolbar.m_recording,
                  toolbar.m_microphone,
                  toolbar.m_microphoneDevice,
                  toolbar.m_speaker,
                  toolbar.m_systemAudioApplication,
                  toolbar.m_systemAudioProcessId,
                  toolbar.m_camera,
                  toolbar.m_cameraDevice,
                  toolbar.m_scroll,
                  nullptr};
    if (persistForWindowSetup && toolbar.m_action == Action::Window
        && toolbar.m_recording) {
        // The window picker and the selected window still need the device
        // items, so the toolbar outlives the modal loop. The caller closes it
        // when the recording starts or the setup is cancelled.
        toolbar.enterSetupMode();
        toolbar.show();
        toolbar.raise();
        result.toolbar = toolbarHolder.release();
    }
    return result;
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
    if (index == itemCount() - 1)
        return 44;

    QFont font(QStringLiteral("Inter"));
    font.setPixelSize(13);
    font.setWeight(QFont::DemiBold);
    const QFontMetrics metrics(font);
    const auto measure = [&metrics](const QString& text) {
        return metrics.horizontalAdvance(text);
    };

    int textWidth = 0;
    switch (index) {
    case 5:
        textWidth = std::max(measure(QStringLiteral("OCR")), measure(QStringLiteral("Mic")));
        break;
    case 6:
        textWidth = std::max({measure(QStringLiteral("Timer")),
                              measure(QStringLiteral("Speaker")),
                              measure(QStringLiteral("10s"))});
        break;
    case 7:
        textWidth = std::max(measure(QStringLiteral("Scroll")),
                             measure(QStringLiteral("Webcam")));
        break;
    default:
        textWidth = measure(labelFor(index));
        break;
    }

    // The same rhythm a Settings row uses: 12px before the icon, a 17px
    // glyph, an 8px gap, and 12px after the label. Measuring the widest
    // label a slot can show keeps "Video" and "Display" inside their cells
    // when the label turns bold.
    return kIconPad + kIconSize + kIconGap + textWidth + kIconPad;
}

int CaptureModeToolbar::preferredWidth() const
{
    int width = kOuterPad * 2;
    for (int i = 0; i < itemCount(); ++i) {
        width += itemWidth(i);
        if (separatorAfter(i))
            width += kSeparator;
    }
    return width;
}

QString CaptureModeToolbar::labelFor(int index) const
{
    if (index == itemCount() - 1)
        return QString();
    if (cameraSlot(index))
        return QStringLiteral("Webcam");
    if (m_enableScrollCapture && index == 7)
        return QStringLiteral("Scroll");
    return QString::fromLatin1(kLabels[index]);
}

/// While recording, the Scroll slot carries the Webcam toggle instead. Scroll
/// is a capture mode and has no meaning once a recording is being set up.
bool CaptureModeToolbar::cameraSlot(int index) const
{
    return m_enableScrollCapture && m_recording && index == 7;
}

QRectF CaptureModeToolbar::itemRect(int index) const
{
    qreal x = kOuterPad;
    for (int i = 0; i < index; ++i) {
        x += itemWidth(i);
        if (separatorAfter(i))
            x += kSeparator;
    }
    return QRectF(x, kOuterPad, itemWidth(index), kItemHeight);
}

bool CaptureModeToolbar::separatorAfter(int index) const
{
    return index == 1 || index == 4 || (m_enableScrollCapture ? index == 7 : index == 6);
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

    const QRectF panel(1.5, 1.5, width() - 3.0, kItemHeight + 2.0 * kOuterPad - 3.0);
    QPainterPath shadow;
    shadow.addRoundedRect(panel.translated(0, 3), kPanelRadius, kPanelRadius);
    painter.fillPath(shadow, QColor(0, 0, 0, 110));

    QPainterPath body;
    body.addRoundedRect(panel, kPanelRadius, kPanelRadius);
    // The settings window's body colour, so Quick Access reads as the same
    // surface rather than a lighter panel of its own.
    painter.fillPath(body, QColor(20, 20, 20, 250));
    painter.setPen(QPen(QColor(255, 255, 255, 42), 1.0));
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(body);

    // Shot and Video are one switch, not two buttons: an inset track holds
    // both halves and the selected half carries the accent, the same shape
    // every other selector in Settings draws.
    const QRectF shotCell = itemRect(0);
    const QRectF videoCell = itemRect(1);
    const QRectF modeTrack(shotCell.left() + 3.0, shotCell.top() + 4.0,
                           videoCell.right() - shotCell.left() - 6.0,
                           shotCell.height() - 8.0);
    const double trackRadius = kPanelRadius * modeTrack.height() / panel.height();
    QPainterPath modeTrackPath;
    modeTrackPath.addRoundedRect(modeTrack, trackRadius, trackRadius);
    painter.fillPath(modeTrackPath, QColor(255, 255, 255, 18));

    const int selectedMode = m_recording ? 1 : 0;
    for (int i = 0; i < 2; ++i) {
        const QRectF cell = itemRect(i);
        const bool isSelected = i == selectedMode;
        const bool hoveredHalf = i == m_hovered && !isSelected
            && !(!m_recordingAvailable && i == 1);
        if (!isSelected && !hoveredHalf)
            continue;
        const QRectF thumb = cell.adjusted(4.5, 6.0, -4.5, -6.0);
        QPainterPath thumbPath;
        const double thumbRadius = trackRadius * thumb.height() / modeTrack.height();
        thumbPath.addRoundedRect(thumb, thumbRadius, thumbRadius);
        painter.fillPath(thumbPath,
                         isSelected ? QColor(kAccent.red(), kAccent.green(), kAccent.blue(), 235)
                                    : QColor(255, 255, 255, 22));
    }

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
            || (!m_recording && !m_scroll && i == 6 && m_timerSeconds > 0);
        // Mic and Speaker are the recording phase's slots: on the shot side
        // 5 and 6 are OCR and Timer and must not inherit the last recording's
        // device state. Their selected look is the bar's orange, as before.
        const bool deviceOn = (m_recording && ((i == 5 && m_microphone) || (i == 6 && m_speaker)))
            || (cameraSlot(i) && m_camera);
        const bool hovered = i == m_hovered && !disabled;

        if ((active || hovered) && i > 1) {
            const QRectF highlight = cell.adjusted(3, 3, -3, -3);
            QPainterPath highlightPath;
            highlightPath.addRoundedRect(highlight, kPanelRadius - 3.0, kPanelRadius - 3.0);
            painter.fillPath(highlightPath,
                             active ? QColor(kAccent.red(), kAccent.green(), kAccent.blue(), 224)
                                    : QColor(255, 255, 255, 16));
        }

        // Settings' nav icons are a step dimmer than their labels; the
        // toolbar keeps that split so the two surfaces read the same.
        const QColor iconColor = active ? Qt::white
            : (deviceOn ? QColor(255, 138, 64)
                        : (disabled ? QColor(115, 115, 122)
                                    : QColor(185, 186, 193)));
        const bool isClose = i == itemCount() - 1;
        QFont groupFont(QStringLiteral("Inter"));
        groupFont.setPixelSize(13);
        groupFont.setWeight(active || deviceOn ? QFont::DemiBold : QFont::Medium);
        QString label = isClose ? QString() : labelFor(i);
        if (!isClose && m_recording && i == 5) {
            label = QStringLiteral("Mic");
        } else if (!isClose && m_recording && i == 6) {
            label = QStringLiteral("Speaker");
        } else if (!isClose && !m_recording && !m_scroll && i == 6 && m_timerSeconds > 0) {
            label = QStringLiteral("%1s").arg(m_timerSeconds);
        }
        const double labelWidth =
            isClose ? 0.0 : QFontMetricsF(groupFont).horizontalAdvance(label);
        const double groupWidth = isClose ? 0.0 : kIconSize + kIconGap + labelWidth;
        const QPointF iconCenter(isClose ? cell.center().x()
                                        : cell.center().x() - groupWidth / 2.0
                                              + kIconSize / 2.0,
                                 cell.center().y());
        drawIcon(painter, i, iconCenter,
                 isClose && !hovered ? QColor(190, 190, 196) : iconColor);

        if (!isClose) {
            painter.setFont(groupFont);
            painter.setPen(active || deviceOn ? Qt::white
                                              : (disabled ? QColor(115, 115, 122)
                                                          : QColor(228, 228, 232)));
            const double textLeft = cell.center().x() - groupWidth / 2.0 + kIconSize + kIconGap;
            const QFontMetricsF labelMetrics(groupFont);
            // Align the middle of the cap height, not the whole line box: the
            // icon is centred on its painted outline below.
            painter.drawText(QPointF(textLeft,
                                     cell.center().y() + labelMetrics.capHeight() / 2.0),
                             label);
        }

        if (separatorAfter(i)) {
            const qreal x = cell.right() + 7.0;
            painter.setPen(QPen(QColor(255, 255, 255, 28), 1));
            painter.drawLine(QPointF(x, 11), QPointF(x, height() - 12));
        }
    }
}

void CaptureModeToolbar::drawIcon(QPainter& painter,
                                 int index,
                                 const QPointF& center,
                                 const QColor& color) const
{
    const QString family = lucideFamily();
    if (family.isEmpty()) {
        drawFallbackIcon(painter, index, center, color);
        return;
    }

    ushort glyph = 0xE1B2; // x
    switch (index) {
    case 0:
        glyph = 0xE064; // camera
        break;
    case 1:
        glyph = 0xE1A5; // video
        break;
    case 2:
        glyph = 0xE11D; // monitor
        break;
    case 3:
        glyph = 0xE426; // app-window
        break;
    case 4:
        glyph = 0xE257; // scan
        break;
    case 5:
        glyph = m_recording ? 0xE118 : 0xE538; // mic / scan-text
        break;
    case 6:
        glyph = m_recording ? 0xE1AB : 0xE1E0; // volume-2 / timer
        break;
    case 7:
        if (cameraSlot(7))
            glyph = 0xE205; // webcam
        else if (m_enableScrollCapture)
            glyph = 0xE28E; // mouse
        else
            glyph = 0xE1B2; // x
        break;
    default:
        break;
    }

    const QPainterPath path = lucidePath(family, glyph, kIconSize);
    const QRectF ink = path.boundingRect();
    painter.save();
    painter.translate(center.x() - ink.center().x(), center.y() - ink.center().y());
    painter.fillPath(path, color);
    painter.restore();
}

void CaptureModeToolbar::drawFallbackIcon(QPainter& painter,
                                  int index,
                                  const QPointF& center,
                                  const QColor& color) const
{
    painter.save();
    const qreal fallbackScale = kIconSize / 23.0;
    painter.translate(center);
    painter.scale(fallbackScale, fallbackScale);
    painter.translate(-center);
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
    case 7: // Webcam while recording, else Scroll (Quick Access) or Close
        if (cameraSlot(7)) {
            painter.drawRoundedRect(QRectF(x - 10, y - 7, 14, 14), 2.5, 2.5);
            painter.drawEllipse(QPointF(x - 3, y), 3.2, 3.2);
            painter.drawLine(QPointF(x + 4, y - 3), QPointF(x + 10, y - 7));
            painter.drawLine(QPointF(x + 10, y - 7), QPointF(x + 10, y + 7));
            painter.drawLine(QPointF(x + 10, y + 7), QPointF(x + 4, y + 3));
            if (!m_camera)
                painter.drawLine(QPointF(x - 10, y - 10), QPointF(x + 10, y + 11));
            break;
        }
        if (m_enableScrollCapture) {
            // Two downward chevrons, matching the toolbar's line style.
            painter.drawLine(QPointF(x - 7, y - 7), QPointF(x, y));
            painter.drawLine(QPointF(x + 7, y - 7), QPointF(x, y));
            painter.drawLine(QPointF(x - 7, y + 1), QPointF(x, y + 8));
            painter.drawLine(QPointF(x + 7, y + 1), QPointF(x, y + 8));
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
    const int index = hitTest(event->pos());
    if (m_setupMode) {
        // The device items and Close stay live while the window setup is
        // open; changing capture mode would abandon the pending recording.
        if (event->button() == Qt::RightButton) {
            if (index >= 0)
                openDeviceMenu(index);
            return;
        }
        if (event->button() != Qt::LeftButton)
            return;
        if (index == itemCount() - 1) {
            closeSetup();
            return;
        }
        if (index != 5 && index != 6 && !cameraSlot(index))
            return;
    }
    if (event->button() == Qt::RightButton) {
        if (index >= 0)
            openDeviceMenu(index);
        return;
    }
    if (event->button() != Qt::LeftButton)
        return;

    if (index < 0)
        return;

    if (cameraSlot(index)) {
        toggleCamera();
        return;
    }

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
    if (m_setupMode) {
        if (event->key() == Qt::Key_Escape)
            closeSetup();
        return;
    }
    if (event->key() == Qt::Key_Escape) {
        finish(Action::Cancel);
        return;
    }
    QWidget::keyPressEvent(event);
}

void CaptureModeToolbar::loadCameraDefaults()
{
#ifdef Q_OS_WIN
    const QString device = resolveCameraDevice(m_cameraDevice);
    if (device.isEmpty()) {
        m_camera = false;
        m_cameraDevice.clear();
    } else {
        m_cameraDevice = device;
    }
#endif
}

void CaptureModeToolbar::toggleCamera()
{
    if (m_camera) {
        m_camera = false;
        update();
        return;
    }

#ifdef Q_OS_WIN
    const QStringList cameras = Platform::directShowDevices(true);
    if (cameras.isEmpty()) {
        QToolTip::showText(QCursor::pos(), QStringLiteral("No camera detected"), this);
        return;
    }
    if (!cameras.contains(m_cameraDevice, Qt::CaseInsensitive))
        m_cameraDevice = cameras.first();
#endif
    m_camera = true;
    update();
}

QPoint CaptureModeToolbar::menuAnchor(int index) const
{
    const QRectF cell = itemRect(index);
    return mapToGlobal(QPoint(qRound(cell.left()), height() + 6));
}

void CaptureModeToolbar::openDeviceMenu(int index)
{
#ifdef Q_OS_WIN
    if (m_recording && index == 5) {
        DeviceMenu menu(this);
        applyMenuStyle(menu);

        const QStringList devices = Platform::directShowDevices(false);
        for (const QString& device : devices) {
            QAction* action = menu.addAction(menuLabel(device, menu.font()));
            action->setProperty("ticked", m_microphone && device == m_microphoneDevice);
            action->setData(device);
        }
        if (!devices.isEmpty())
            menu.addSeparator();
        QAction* off = menu.addAction(QStringLiteral("Don't record microphone"));
        off->setProperty("ticked", !m_microphone);

        roundMenu(menu);
        QAction* chosen = menu.exec(menuAnchor(index));
        if (!chosen)
            return;
        if (chosen == off) {
            m_microphone = false;
        } else if (!chosen->data().toString().isEmpty()) {
            m_microphone = true;
            m_microphoneDevice = chosen->data().toString();
        }
        update();
        return;
    }

    if (m_recording && index == 6) {
        DeviceMenu menu(this);
        applyMenuStyle(menu);

        QAction* all = menu.addAction(QStringLiteral("Record system audio from all apps"));
        all->setProperty("ticked", m_speaker && m_systemAudioApplication.isEmpty());

        DeviceMenu* one = new DeviceMenu(QStringLiteral("Record audio from one app"), &menu);
        menu.addAction(one->menuAction());
        one->setFont(menu.font());
        one->setStyleSheet(menu.styleSheet());
        // Qt butts a submenu against its parent. Nudge it clear once it is
        // placed, so the two panels read as separate surfaces.
        QObject::connect(one, &QMenu::aboutToShow, one, [one]() {
            QTimer::singleShot(0, one, [one]() { one->move(one->x() + 6, one->y()); });
        });

        QVector<QAction*> appActions;
        const QVector<Platform::AudioApplication> applications =
            Platform::audioApplications();
        if (applications.isEmpty()) {
            QAction* empty = one->addAction(QStringLiteral("No apps are playing audio"));
            empty->setEnabled(false);
        } else {
            for (const Platform::AudioApplication& application : applications) {
                QAction* action =
                    one->addAction(menuLabel(application.name, one->font()));
                action->setProperty("ticked", m_speaker
                                   && m_systemAudioApplication.compare(
                                          application.name, Qt::CaseInsensitive)
                                       == 0);
                action->setData(application.processId);
                action->setProperty("appName", application.name);
                appActions.append(action);
            }
        }

        menu.addSeparator();
        QAction* off = menu.addAction(QStringLiteral("Don't record system audio"));
        off->setProperty("ticked", !m_speaker);

        roundMenu(*one);
        roundMenu(menu);
        QAction* chosen = menu.exec(menuAnchor(index));
        if (!chosen)
            return;
        if (chosen == all) {
            m_speaker = true;
            m_systemAudioApplication.clear();
            m_systemAudioProcessId = 0;
        } else if (chosen == off) {
            m_speaker = false;
            m_systemAudioApplication.clear();
            m_systemAudioProcessId = 0;
        } else if (appActions.contains(chosen)) {
            m_speaker = true;
            m_systemAudioProcessId = chosen->data().toUInt();
            m_systemAudioApplication = chosen->property("appName").toString();
        }
        update();
        return;
    }

    if (cameraSlot(index)) {
        DeviceMenu menu(this);
        applyMenuStyle(menu);

        const QStringList cameras = Platform::directShowDevices(true);
        if (cameras.isEmpty()) {
            QAction* empty = menu.addAction(QStringLiteral("No camera detected"));
            empty->setEnabled(false);
        } else {
            for (const QString& camera : cameras) {
                QAction* action = menu.addAction(menuLabel(camera, menu.font()));
                action->setProperty("ticked", m_camera && camera == m_cameraDevice);
                action->setData(camera);
            }
            menu.addSeparator();
        }
        QAction* off = menu.addAction(QStringLiteral("Don't record camera"));
        off->setProperty("ticked", !m_camera);

        roundMenu(menu);
        QAction* chosen = menu.exec(menuAnchor(index));
        if (!chosen)
            return;
        if (chosen == off) {
            m_camera = false;
        } else if (!chosen->data().toString().isEmpty()) {
            m_camera = true;
            m_cameraDevice = chosen->data().toString();
        }
        update();
        return;
    }
#else
    Q_UNUSED(index);
#endif
}

CaptureModeToolbar::DeviceDefaults CaptureModeToolbar::deviceState() const
{
    DeviceDefaults state;
    state.microphone = m_microphone;
    state.microphoneDevice = m_microphoneDevice;
    state.speaker = m_speaker;
    state.systemAudioApplication = m_systemAudioApplication;
    state.systemAudioProcessId = m_systemAudioProcessId;
    state.camera = m_camera;
    state.cameraDevice = m_cameraDevice;
    return state;
}

void CaptureModeToolbar::setCloseHandler(std::function<void()> handler)
{
    m_onClose = std::move(handler);
}

void CaptureModeToolbar::enterSetupMode()
{
    m_setupMode = true;
    m_finished = false;
    setAttribute(Qt::WA_DeleteOnClose, true);
}

void CaptureModeToolbar::closeSetup()
{
    if (!m_setupMode)
        return;
    m_setupMode = false;
    auto handler = std::move(m_onClose);
    m_onClose = nullptr;
    if (handler)
        handler();
    // The handler closes the picker or the overlay, whose cancel callback
    // closes this toolbar too; closing here is harmless and covers the path
    // where the setup was already gone.
    close();
}
