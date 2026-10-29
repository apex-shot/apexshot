#include "platform/windows/window_recording.h"

#include <QCursor>
#include <QDialog>
#include <QDialogButtonBox>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QFont>
#include <QFontMetrics>
#include <QFormLayout>
#include <QGuiApplication>
#include <QKeyEvent>
#include <QLinearGradient>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMenu>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QRegion>
#include <QResizeEvent>
#include <QScreen>
#include <QShowEvent>
#include <QSpinBox>
#include <QTextStream>
#include <QTimer>
#include <QWheelEvent>

#include <algorithm>
#include <cmath>

namespace {

void overlayLog(const QString& text)
{
    QFile file(QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-window-ui.log")));
    if (file.open(QIODevice::Append | QIODevice::WriteOnly)) {
        QTextStream out(&file);
        out.setCodec("UTF-8");
        out << text << Qt::endl;
    }
}

/// A popup menu whose native window background is clipped to the same rounded
/// border the stylesheet draws, including submenus.
class RoundedMenu : public QMenu
{
public:
    explicit RoundedMenu(QWidget* parent = nullptr)
        : QMenu(parent)
    {
        setWindowFlag(Qt::FramelessWindowHint, true);
        setWindowFlag(Qt::NoDropShadowWindowHint, true);
        setAttribute(Qt::WA_TranslucentBackground, true);
        setAttribute(Qt::WA_NoSystemBackground, true);
        setAutoFillBackground(false);
    }

protected:
    void showEvent(QShowEvent* event) override
    {
        QMenu::showEvent(event);
        applyMask();
    }

    void resizeEvent(QResizeEvent* event) override
    {
        QMenu::resizeEvent(event);
        applyMask();
    }

private:
    void applyMask()
    {
        QPainterPath path;
        path.addRoundedRect(QRectF(rect()), 10, 10);
        setMask(QRegion(path.toFillPolygon().toPolygon()));
    }
};

const QColor kAccent(255, 102, 0);
const QColor kAccentWash(255, 102, 0, 26);
const QColor kStartTop(255, 122, 26);
const QColor kStartBottom(242, 94, 0);
const QColor kStartTopHover(255, 138, 40);
const QColor kStartBottomHover(250, 104, 10);
const QColor kText(245, 245, 247);
const QColor kMuted(165, 165, 172);
const QColor kDim(0, 0, 0, 132);
constexpr double kControlRadius = 10.0;

QFont uiFont(int pixelSize, bool bold)
{
    QFont font(QStringLiteral("Inter"));
    font.setPixelSize(pixelSize);
    font.setWeight(bold ? QFont::DemiBold : QFont::Normal);
    return font;
}

QRect virtualDesktopGeometry()
{
    QRect geometry;
    for (const QScreen* screen : QGuiApplication::screens())
        geometry = geometry.united(screen->geometry());
    return geometry;
}

QScreen* screenForMonitor(const QVector<Platform::MonitorInfo>& monitors, int index)
{
    const QList<QScreen*> screens = QGuiApplication::screens();
    if (index >= 0 && index < monitors.size()) {
        for (QScreen* screen : screens) {
            if (screen->name() == monitors.at(index).name)
                return screen;
        }
    }
    if (index >= 0 && index < screens.size())
        return screens.at(index);
    return QGuiApplication::primaryScreen();
}

/// Index of the monitor that holds a device-pixel rectangle's centre.
int monitorForFrame(const QVector<Platform::MonitorInfo>& monitors, const QRect& device)
{
    const QPoint center = device.center();
    for (int i = 0; i < monitors.size(); ++i) {
        if (monitors.at(i).geometry.contains(center))
            return i;
    }
    return 0;
}

/// Turn a device-pixel rectangle into overlay-local logical pixels, using the
/// display that holds its centre. Windows reports window frames in device
/// pixels while Qt lays the overlay out in logical ones.
QRectF logicalFrame(const QVector<Platform::MonitorInfo>& monitors,
                    const QRect& device,
                    const QRect& overlay)
{
    if (device.isEmpty() || monitors.isEmpty())
        return {};

    const int index = monitorForFrame(monitors, device);
    const Platform::MonitorInfo& monitor = monitors.at(index);
    const QScreen* screen = screenForMonitor(monitors, index);
    const QPointF logicalOrigin = screen ? QPointF(screen->geometry().topLeft())
                                         : QPointF(monitor.geometry.topLeft());
    // Qt lays widgets out in its own logical pixels, which follow the
    // screen's device pixel ratio, not the DPI the platform seam reports for
    // the monitor. On a build without high-DPI scaling those differ, and
    // mixing them paints the selection at the wrong size.
    qreal scale = screen ? screen->devicePixelRatio()
                         : (monitor.dpr > 0.0 ? monitor.dpr : 1.0);
    if (scale <= 0.0)
        scale = 1.0;
    const auto mapPoint = [&](const QPoint& point) {
        return logicalOrigin + QPointF(point - monitor.geometry.topLeft()) / scale;
    };
    const QRectF logical(mapPoint(device.topLeft()),
                         mapPoint(device.bottomRight() + QPoint(1, 1)));
    return logical.translated(-QPointF(overlay.topLeft()));
}

/// Device pixels per logical pixel for the screen a window frame sits on.
qreal scaleForFrame(const QVector<Platform::MonitorInfo>& monitors, const QRect& device)
{
    if (device.isEmpty() || monitors.isEmpty())
        return 1.0;
    const int index = monitorForFrame(monitors, device);
    const Platform::MonitorInfo& monitor = monitors.at(index);
    const QScreen* screen = screenForMonitor(monitors, index);
    const qreal scale = screen ? screen->devicePixelRatio()
                               : (monitor.dpr > 0.0 ? monitor.dpr : 1.0);
    return scale > 0.0 ? scale : 1.0;
}

void paintDimAround(QPainter& painter, const QRectF& hole, const QRectF& bounds)
{
    painter.setPen(Qt::NoPen);
    painter.setBrush(kDim);
    if (hole.isEmpty()) {
        painter.drawRect(bounds);
        return;
    }

    const QRectF top(bounds.left(), bounds.top(), bounds.width(),
                     std::max<qreal>(0.0, hole.top() - bounds.top()));
    const QRectF bottom(bounds.left(), hole.bottom(), bounds.width(),
                        std::max<qreal>(0.0, bounds.bottom() - hole.bottom()));
    const QRectF left(bounds.left(), hole.top(),
                      std::max<qreal>(0.0, hole.left() - bounds.left()), hole.height());
    const QRectF right(hole.right(), hole.top(),
                       std::max<qreal>(0.0, bounds.right() - hole.right()), hole.height());
    for (const QRectF& rect : {top, bottom, left, right}) {
        if (!rect.isEmpty())
            painter.drawRect(rect);
    }
}

QPainterPath roundedPath(const QRectF& rect, double radius)
{
    QPainterPath path;
    path.addRoundedRect(rect, radius, radius);
    return path;
}

void paintHintPill(QPainter& painter,
                   const QRectF& screen,
                   qreal centerX,
                   const QString& title,
                   const QString& hint)
{
    const QFont titleFont = uiFont(13, true);
    const QFont hintFont = uiFont(12, false);
    const QFontMetrics titleMetrics(titleFont);
    const QFontMetrics hintMetrics(hintFont);
    const double gap = 12.0;
    const double padX = 18.0;
    const double width = padX * 2.0 + titleMetrics.horizontalAdvance(title) + gap
        + hintMetrics.horizontalAdvance(hint);
    const double height = 38.0;
    const QRectF pill(centerX - width / 2.0, screen.top() + 22.0, width, height);

    painter.fillPath(roundedPath(pill.translated(0, 3), 12.0), QColor(0, 0, 0, 90));
    painter.fillPath(roundedPath(pill, 12.0), QColor(18, 18, 20, 238));

    painter.setFont(titleFont);
    painter.setPen(kText);
    painter.drawText(QRectF(pill.left() + padX, pill.top(), pill.width(), pill.height()),
                     Qt::AlignLeft | Qt::AlignVCenter,
                     title);

    painter.setFont(hintFont);
    painter.setPen(kMuted);
    painter.drawText(QRectF(pill.left() + padX + titleMetrics.horizontalAdvance(title) + gap,
                            pill.top(),
                            pill.width(),
                            pill.height()),
                     Qt::AlignLeft | Qt::AlignVCenter,
                     hint);
}

}  // namespace

namespace Platform {

// ------------------------------------------------------------- picker ------
WindowPicker::WindowPicker(Picked onPicked,
                           std::function<void()> onCancelled,
                           QString action)
    : m_onPicked(std::move(onPicked))
    , m_onCancelled(std::move(onCancelled))
    , m_action(std::move(action))
{
    setWindowFlags(Qt::Window | Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint);
    setAttribute(Qt::WA_TranslucentBackground);
    setAttribute(Qt::WA_DeleteOnClose);
    setMouseTracking(true);
    setCursor(Qt::ArrowCursor);
    setFocusPolicy(Qt::StrongFocus);
    setGeometry(virtualDesktopGeometry());

    m_refresh = new QTimer(this);
    m_refresh->setInterval(400);
    QObject::connect(m_refresh, &QTimer::timeout, this, [this] { refreshWindows(); });

    refreshWindows();
}

WindowPicker::~WindowPicker() = default;

void WindowPicker::dismiss()
{
    finish(false);
}

void WindowPicker::refreshWindows()
{
    m_windows = Platform::windows();
    m_frames.clear();
    m_frames.reserve(m_windows.size());
    const QVector<MonitorInfo> monitors = Platform::monitors();
    for (const WindowInfo& window : m_windows) {
        m_frames.append(window.minimized ? QRectF()
                                         : logicalFrame(monitors, window.geometry, geometry()));
    }

    if (m_cycled) {
        const int selected = indexOfSelected();
        if (selected >= 0) {
            m_hovered = selected;
        } else {
            m_cycled = false;
            m_selectedId = 0;
            m_hovered = m_mouseMoved ? windowAt(m_cursor) : -1;
        }
    } else {
        m_hovered = m_mouseMoved ? windowAt(m_cursor) : -1;
    }
    setCursor(m_hovered >= 0 ? Qt::PointingHandCursor : Qt::ArrowCursor);
    update();
}

int WindowPicker::indexOfSelected() const
{
    if (!m_cycled || m_selectedId == 0)
        return -1;
    for (int i = 0; i < m_windows.size(); ++i) {
        if (m_windows.at(i).id == m_selectedId)
            return i;
    }
    return -1;
}

void WindowPicker::cycleSelection(int delta)
{
    refreshWindows();

    if (m_cycleOrder.isEmpty()) {
        for (int i = 0; i < m_windows.size(); ++i) {
            if (!m_frames.at(i).isEmpty())
                m_cycleOrder.append(m_windows.at(i).id);
        }
        if (m_cycleOrder.isEmpty())
            return;
    }

    // Walk the stack as it stood when cycling began: each raise reorders the
    // live list, and following that order would bounce between two windows.
    const quintptr anchorId = m_selectedId != 0
        ? m_selectedId
        : (m_hovered >= 0 && m_hovered < m_windows.size() ? m_windows.at(m_hovered).id : 0);
    const int current = anchorId != 0 ? m_cycleOrder.indexOf(anchorId) : -1;
    const int next = current < 0
        ? (delta > 0 ? 0 : m_cycleOrder.size() - 1)
        : (current + delta + m_cycleOrder.size()) % m_cycleOrder.size();
    const quintptr id = m_cycleOrder.at(next);

    int index = -1;
    for (int i = 0; i < m_windows.size(); ++i) {
        if (m_windows.at(i).id == id) {
            index = i;
            break;
        }
    }
    if (index < 0) {
        // The window closed while we walked past it; drop it and continue.
        m_cycleOrder.remove(next);
        if (!m_cycleOrder.isEmpty())
            cycleSelection(delta);
        return;
    }

    m_cycled = true;
    m_selectedId = id;
    m_hovered = index;
    // Raising without activation keeps the overlay focused for the next Tab,
    // and shows the window the person just walked onto.
    Platform::raiseWindow(id);
    raise();
    setCursor(Qt::PointingHandCursor);
    refreshWindows();
    update();
}

int WindowPicker::windowAt(const QPointF& localPoint) const
{
    for (int i = 0; i < m_frames.size(); ++i) {
        const QRectF& frame = m_frames.at(i);
        if (!frame.isEmpty() && frame.contains(localPoint))
            return i;
    }
    return -1;
}

void WindowPicker::updateHover(const QPoint& localPoint)
{
    const int hovered = windowAt(localPoint);
    if (hovered == m_hovered)
        return;
    m_hovered = hovered;
    setCursor(hovered >= 0 ? Qt::PointingHandCursor : Qt::ArrowCursor);
    update();
}

void WindowPicker::paintEvent(QPaintEvent*)
{
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing, true);
    const QRectF bounds(rect());

    const bool hovering = m_hovered >= 0 && m_hovered < m_frames.size();
    if (hovering)
        paintDimAround(painter, m_frames.at(m_hovered), bounds);
    else
        painter.fillRect(bounds, kDim);

    if (hovering) {
        // The wash is the whole selection: no outline, no corner marks.
        painter.fillPath(roundedPath(m_frames.at(m_hovered), 8.0), kAccentWash);
    }

    QScreen* cursorScreen = QGuiApplication::screenAt(mapToGlobal(m_cursor));
    if (!cursorScreen)
        cursorScreen = QGuiApplication::primaryScreen();
    const QRect screen = cursorScreen
        ? cursorScreen->geometry().translated(-geometry().topLeft())
        : rect();
    paintHintPill(painter,
                  QRectF(screen),
                  screen.center().x(),
                  hovering ? QStringLiteral("Click to %1 this window").arg(m_action)
                           : QStringLiteral("Click a window to %1").arg(m_action),
                  QStringLiteral("Tab cycles · Esc cancels"));
}

void WindowPicker::mouseMoveEvent(QMouseEvent* event)
{
    m_cursor = event->pos();
    m_mouseMoved = true;
    m_cycled = false;
    m_selectedId = 0;
    m_cycleOrder.clear();
    updateHover(event->pos());
}

void WindowPicker::mousePressEvent(QMouseEvent* event)
{
    if (event->button() == Qt::RightButton) {
        finish(false);
        return;
    }
    if (event->button() != Qt::LeftButton)
        return;

    const int hit = windowAt(event->pos());
    if (hit < 0) {
        finish(false);
        return;
    }
    // A click is authoritative: capture the window under the pointer even
    // when Tab had walked to a different one.
    m_cycled = false;
    m_selectedId = 0;
    m_cycleOrder.clear();
    m_hovered = hit;
    finish(true);
}

void WindowPicker::keyPressEvent(QKeyEvent* event)
{
    switch (event->key()) {
    case Qt::Key_Escape:
        finish(false);
        return;
    case Qt::Key_Tab:
    case Qt::Key_Right:
    case Qt::Key_Down:
        cycleSelection(1);
        return;
    case Qt::Key_Backtab:
    case Qt::Key_Left:
    case Qt::Key_Up:
        cycleSelection(-1);
        return;
    case Qt::Key_Return:
    case Qt::Key_Enter:
        if (m_hovered >= 0 && m_hovered < m_windows.size())
            finish(true);
        return;
    default:
        break;
    }
    QWidget::keyPressEvent(event);
}

void WindowPicker::wheelEvent(QWheelEvent* event)
{
    const int steps = event->angleDelta().y();
    if (steps != 0)
        cycleSelection(steps > 0 ? -1 : 1);
    event->accept();
}

void WindowPicker::showEvent(QShowEvent* event)
{
    QWidget::showEvent(event);
    setGeometry(virtualDesktopGeometry());
    m_cursor = mapFromGlobal(QCursor::pos());
    m_mouseMoved = false;
    refreshWindows();
    overlayLog(QStringLiteral("picker shown windows=%1").arg(m_windows.size()));
    Platform::keepTopmost(static_cast<quintptr>(winId()));
    Platform::excludeFromCapture(static_cast<quintptr>(winId()));
    activateWindow();
    setFocus(Qt::ActiveWindowFocusReason);
    m_refresh->start();
}

void WindowPicker::finish(bool picked)
{
    if (m_done)
        return;
    m_done = true;
    if (m_refresh)
        m_refresh->stop();
    m_cycleOrder.clear();

    const int selected = indexOfSelected();
    const int index = selected >= 0 ? selected : m_hovered;
    const WindowInfo chosen =
        picked && index >= 0 && index < m_windows.size() ? m_windows.at(index) : WindowInfo{};
    close();
    deleteLater();

    if (picked && !chosen.title.isEmpty()) {
        if (m_onPicked)
            m_onPicked(chosen);
    } else if (m_onCancelled) {
        m_onCancelled();
    }
}

QString windowSizesPath()
{
    const QString appData = qEnvironmentVariable("APPDATA");
    if (appData.isEmpty())
        return QString();
    return QDir(appData).filePath(QStringLiteral("apexshot/window-sizes.json"));
}

QVector<QSize> loadWindowSizes()
{
    QVector<QSize> sizes;
    QFile file(windowSizesPath());
    if (!file.open(QIODevice::ReadOnly))
        return sizes;
    const QJsonDocument document = QJsonDocument::fromJson(file.readAll());
    if (!document.isArray())
        return sizes;
    for (const QJsonValue& value : document.array()) {
        const QJsonObject object = value.toObject();
        const QSize size(object.value(QStringLiteral("w")).toInt(),
                         object.value(QStringLiteral("h")).toInt());
        if (size.width() >= 200 && size.height() >= 140)
            sizes.append(size);
    }
    return sizes;
}

void saveWindowSizes(const QVector<QSize>& sizes)
{
    QJsonArray array;
    for (const QSize& size : sizes) {
        array.append(QJsonObject{{QStringLiteral("w"), size.width()},
                                 {QStringLiteral("h"), size.height()}});
    }
    const QString path = windowSizesPath();
    if (path.isEmpty())
        return;
    QDir().mkpath(QFileInfo(path).absolutePath());
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate))
        return;
    file.write(QJsonDocument(array).toJson(QJsonDocument::Indented));
}

// ------------------------------------------------------------- overlay -----
RecordingSetupOverlay::RecordingSetupOverlay(const WindowInfo& window,
                                             Started onStarted,
                                             std::function<void()> onCancelled)
    : m_window(window)
    , m_onStarted(std::move(onStarted))
    , m_onCancelled(std::move(onCancelled))
{
    setWindowFlags(Qt::Window | Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint);
    setAttribute(Qt::WA_TranslucentBackground);
    setAttribute(Qt::WA_DeleteOnClose);
    setFocusPolicy(Qt::StrongFocus);
    setMouseTracking(true);
    setGeometry(virtualDesktopGeometry());

    m_refresh = new QTimer(this);
    m_refresh->setInterval(400);
    QObject::connect(m_refresh, &QTimer::timeout, this, [this] {
        if (!m_resizing && !m_moving)
            refreshWindow();
    });
}

RecordingSetupOverlay::~RecordingSetupOverlay() = default;

void RecordingSetupOverlay::showEvent(QShowEvent* event)
{
    QWidget::showEvent(event);
    setGeometry(virtualDesktopGeometry());
    refreshWindow();
    overlayLog(QStringLiteral("overlay shown %1x%2 hole=%3,%4 %5x%6 start=%7,%8 %9x%10")
                   .arg(width())
                   .arg(height())
                   .arg(m_hole.x())
                   .arg(m_hole.y())
                   .arg(m_hole.width())
                   .arg(m_hole.height())
                   .arg(startRect().x())
                   .arg(startRect().y())
                   .arg(startRect().width())
                   .arg(startRect().height()));
    Platform::keepTopmost(static_cast<quintptr>(winId()));
    Platform::excludeFromCapture(static_cast<quintptr>(winId()));
    activateWindow();
    setFocus(Qt::ActiveWindowFocusReason);
    m_refresh->start();
}

void RecordingSetupOverlay::refreshWindow()
{
    if (m_done)
        return;
    const QRect frame = Platform::windowFrame(m_window.id);
    if (frame.isEmpty()) {
        cancel();
        return;
    }
    m_window.geometry = frame;

    const QRectF hole = logicalFrame(Platform::monitors(), frame, geometry());
    if (hole == m_hole)
        return;
    m_hole = hole;
    if (m_hoverGrip || m_hoverStart || m_hoverResize || m_hoverWindow)
        setCursor(m_hoverStart || m_hoverResize
                      ? Qt::PointingHandCursor
                      : (m_hoverGrip ? Qt::SizeFDiagCursor
                                     : (m_hoverWindow ? Qt::SizeAllCursor : Qt::ArrowCursor)));
    update();
}

double RecordingSetupOverlay::controlWidth() const
{
    // The Start button and the size chip share one width, so the chip never
    // sticks out past the button it belongs to.
    const QFontMetrics buttonMetrics(uiFont(16, true));
    const double buttonWidth =
        std::max(148.0, buttonMetrics.horizontalAdvance(QStringLiteral("Start recording")) + 52.0);
    const QFontMetrics sizeMetrics(uiFont(13, false));
    const QFontMetrics resizeMetrics(uiFont(13, true));
    const double chipWidth = 12.0 + sizeMetrics.horizontalAdvance(sizeText()) + 10.0 + 1.0
                             + 10.0 + resizeMetrics.horizontalAdvance(QStringLiteral("Resize"))
                             + 24.0 + 12.0;
    return std::max(buttonWidth, chipWidth);
}

QRectF RecordingSetupOverlay::startRect() const
{
    const double pillWidth = controlWidth();
    constexpr double pillHeight = 46.0;

    // The action sits at the centre of the window it records, not on an edge
    // of it, and stays on screen when the window is partly off-screen.
    double x = m_hole.center().x() - pillWidth / 2.0;
    double y = m_hole.center().y() - pillHeight / 2.0;
    x = std::clamp(x, 12.0, std::max(12.0, width() - pillWidth - 12.0));
    y = std::clamp(y, 12.0, std::max(12.0, height() - pillHeight - 12.0));
    return QRectF(x, y, pillWidth, pillHeight);
}

QString RecordingSetupOverlay::sizeText() const
{
    return QStringLiteral("%1 × %2")
        .arg(m_window.geometry.width())
        .arg(m_window.geometry.height());
}

QRectF RecordingSetupOverlay::headerRect() const
{
    const double chipWidth = controlWidth();
    const QRectF start = startRect();
    return QRectF(start.center().x() - chipWidth / 2.0,
                  start.top() - 46.0,
                  chipWidth,
                  32.0);
}

QRectF RecordingSetupOverlay::sizeRect() const
{
    const QRectF header = headerRect();
    const QFontMetrics metrics(uiFont(13, false));
    return QRectF(header.left() + 12.0,
                  header.top(),
                  metrics.horizontalAdvance(sizeText()),
                  header.height());
}

QRectF RecordingSetupOverlay::resizeRect() const
{
    const QRectF header = headerRect();
    const QFontMetrics metrics(uiFont(13, true));
    const double width = metrics.horizontalAdvance(QStringLiteral("Resize")) + 24.0;
    return QRectF(header.right() - 12.0 - width,
                  header.top() + 3.0,
                  width,
                  header.height() - 6.0);
}

bool RecordingSetupOverlay::fitsOnScreen(const QSize& size) const
{
    QScreen* screen = QGuiApplication::screenAt(mapToGlobal(m_hole.center().toPoint()));
    if (!screen)
        screen = QGuiApplication::primaryScreen();
    if (!screen)
        return true;
    const qreal dpr = screen->devicePixelRatio();
    const QRect available = screen->availableGeometry();
    return size.width() <= int(std::lround(available.width() * dpr)) - 16
           && size.height() <= int(std::lround(available.height() * dpr)) - 16;
}

QRectF RecordingSetupOverlay::gripRect() const
{
    constexpr double size = 26.0;
    constexpr double inset = 10.0;
    return QRectF(m_hole.right() - size - inset,
                  m_hole.bottom() - size - inset,
                  size,
                  size);
}

void RecordingSetupOverlay::paintEvent(QPaintEvent*)
{
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing, true);
    const QRectF bounds(rect());
    paintDimAround(painter, m_hole, bounds);
    if (m_hole.isEmpty())
        return;

    // The painted window: a colour wash only. No frame, no corner marks.
    painter.fillPath(roundedPath(m_hole, 8.0), kAccentWash);

    const QRectF start = startRect();
    const QPainterPath startPath = roundedPath(start, kControlRadius);
    painter.fillPath(roundedPath(start.translated(0, 4), kControlRadius), QColor(0, 0, 0, 90));
    QLinearGradient startFill(start.topLeft(), start.bottomLeft());
    startFill.setColorAt(0.0, m_hoverStart ? kStartTopHover : kStartTop);
    startFill.setColorAt(1.0, m_hoverStart ? kStartBottomHover : kStartBottom);
    painter.fillPath(startPath, startFill);

    painter.setFont(uiFont(16, true));
    painter.setPen(Qt::white);
    painter.drawText(start, Qt::AlignCenter, QStringLiteral("Start recording"));

    // The window size and its resize control sit on one dark chip above the
    // action, so they stay readable over a bright window.
    const QRectF header = headerRect();
    painter.setPen(QPen(QColor(255, 255, 255, 44), 1.0));
    painter.setBrush(QColor(16, 16, 18, 228));
    painter.drawRoundedRect(header, 8.0, 8.0);

    painter.setFont(uiFont(13, false));
    painter.setPen(kText);
    painter.drawText(sizeRect(), Qt::AlignVCenter | Qt::AlignLeft, sizeText());

    const QRectF resize = resizeRect();
    const qreal dividerX = resize.left() - 10.0;
    painter.setPen(QPen(QColor(255, 255, 255, 38), 1.0));
    painter.drawLine(QPointF(dividerX, header.top() + 8.0),
                     QPointF(dividerX, header.bottom() - 8.0));
    if (m_hoverResize) {
        painter.setPen(Qt::NoPen);
        painter.setBrush(QColor(255, 255, 255, 30));
        painter.drawRoundedRect(resize, 6.0, 6.0);
    }
    painter.setFont(uiFont(13, true));
    painter.setPen(kText);
    painter.drawText(resize, Qt::AlignCenter, QStringLiteral("Resize"));

    // A quiet corner grip: the only affordance besides Start.
    if (m_hole.width() > 90.0 && m_hole.height() > 70.0) {
        const QRectF grip = gripRect();
        painter.setPen(QPen(QColor(255, 255, 255, m_hoverGrip ? 220 : 140), 1.6,
                            Qt::SolidLine, Qt::RoundCap));
        for (int i = 0; i < 3; ++i) {
            const double length = 6.0 + i * 5.0;
            painter.drawLine(QPointF(grip.right() - length, grip.bottom()),
                             QPointF(grip.right(), grip.bottom() - length));
        }
    }
}

void RecordingSetupOverlay::mousePressEvent(QMouseEvent* event)
{
    if (event->button() == Qt::RightButton) {
        cancel();
        return;
    }
    if (event->button() != Qt::LeftButton || m_hole.isEmpty())
        return;
    if (resizeRect().adjusted(-4.0, -4.0, 4.0, 4.0).contains(event->pos())) {
        openResizeMenu();
        return;
    }
    if (startRect().contains(event->pos())) {
        finish();
        return;
    }
    if (gripRect().adjusted(-8.0, -8.0, 8.0, 8.0).contains(event->pos())) {
        beginResize(event->globalPos());
        return;
    }
    if (m_hole.contains(event->pos())) {
        beginMove(event->globalPos());
        return;
    }
    // Clicks anywhere else are absorbed so the window underneath is not
    // disturbed while its recording is being set up.
}

void RecordingSetupOverlay::mouseMoveEvent(QMouseEvent* event)
{
    if (m_resizing) {
        resizeTo(event->globalPos());
        return;
    }
    if (m_moving) {
        moveTo(event->globalPos());
        return;
    }

    const QRectF start = startRect();
    const bool hoverStart = start.contains(event->pos());
    const bool hoverResize =
        !hoverStart && resizeRect().adjusted(-4.0, -4.0, 4.0, 4.0).contains(event->pos());
    const bool hoverGrip =
        !hoverStart && !hoverResize && m_hole.width() > 90.0 && m_hole.height() > 70.0
        && gripRect().adjusted(-8.0, -8.0, 8.0, 8.0).contains(event->pos());
    const bool hoverWindow =
        !hoverStart && !hoverResize && !hoverGrip && m_hole.contains(event->pos());
    if (hoverStart != m_hoverStart || hoverResize != m_hoverResize
        || hoverGrip != m_hoverGrip || hoverWindow != m_hoverWindow) {
        m_hoverStart = hoverStart;
        m_hoverResize = hoverResize;
        m_hoverGrip = hoverGrip;
        m_hoverWindow = hoverWindow;
        setCursor(hoverStart || hoverResize
                      ? Qt::PointingHandCursor
                      : (hoverGrip ? Qt::SizeFDiagCursor
                                   : (hoverWindow ? Qt::SizeAllCursor : Qt::ArrowCursor)));
        update();
    }
}

void RecordingSetupOverlay::mouseReleaseEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton)
        return;
    if (m_moving) {
        m_moving = false;
        refreshWindow();
        setCursor(m_hole.contains(event->pos()) ? Qt::SizeAllCursor : Qt::ArrowCursor);
        return;
    }
    if (!m_resizing)
        return;
    m_resizing = false;
    refreshWindow();
    setCursor(m_hoverGrip ? Qt::SizeFDiagCursor : Qt::ArrowCursor);
}

void RecordingSetupOverlay::beginResize(const QPoint& globalPos)
{
    m_resizeStartFrame = Platform::windowFrame(m_window.id);
    if (m_resizeStartFrame.isEmpty())
        m_resizeStartFrame = m_window.geometry;
    if (m_resizeStartFrame.isEmpty())
        return;
    m_resizeStartSize = m_resizeStartFrame.size();
    m_resizePress = globalPos;
    m_resizing = true;
    setCursor(Qt::SizeFDiagCursor);
}

void RecordingSetupOverlay::resizeTo(const QPoint& globalPos)
{
    if (m_resizeStartFrame.isEmpty())
        return;

    // Windows works in device pixels while the drag arrives in logical ones.
    const qreal scale = scaleForFrame(Platform::monitors(), m_resizeStartFrame);
    const QPoint delta = globalPos - m_resizePress;
    const int width = std::max(200, m_resizeStartSize.width()
                                       + static_cast<int>(std::lround(delta.x() * scale)));
    const int height = std::max(140, m_resizeStartSize.height()
                                        + static_cast<int>(std::lround(delta.y() * scale)));
    Platform::resizeWindow(m_window.id,
                           QRect(m_resizeStartFrame.topLeft(), QSize(width, height)));
    refreshWindow();
}

QRect RecordingSetupOverlay::frameOnScreen(const QRect& frame, const QPoint& globalHint) const
{
    QScreen* screen = QGuiApplication::screenAt(globalHint);
    if (!screen)
        screen = QGuiApplication::screenAt(mapToGlobal(m_hole.center().toPoint()));
    if (!screen)
        screen = QGuiApplication::primaryScreen();
    if (!screen)
        return frame;

    const qreal dpr = screen->devicePixelRatio();
    const QRect available = screen->availableGeometry();
    const QRect device(
        QPoint(int(std::lround(available.x() * dpr)), int(std::lround(available.y() * dpr))),
        QSize(int(std::lround(available.width() * dpr)),
              int(std::lround(available.height() * dpr))));

    QRect result = frame;
    if (result.width() <= device.width()) {
        result.moveLeft(std::clamp(result.left(),
                                   device.left(),
                                   device.right() - result.width() + 1));
    } else {
        result.moveLeft(device.left());
    }
    if (result.height() <= device.height()) {
        result.moveTop(std::clamp(result.top(),
                                  device.top(),
                                  device.bottom() - result.height() + 1));
    } else {
        result.moveTop(device.top());
    }
    return result;
}

void RecordingSetupOverlay::beginMove(const QPoint& globalPos)
{
    m_moveStartFrame = Platform::windowFrame(m_window.id);
    if (m_moveStartFrame.isEmpty())
        m_moveStartFrame = m_window.geometry;
    if (m_moveStartFrame.isEmpty())
        return;
    m_movePress = globalPos;
    m_moving = true;
    setCursor(Qt::ClosedHandCursor);
}

void RecordingSetupOverlay::moveTo(const QPoint& globalPos)
{
    if (m_moveStartFrame.isEmpty())
        return;
    // The pointer is in logical pixels and the frame in device pixels.
    const qreal scale = scaleForFrame(Platform::monitors(), m_moveStartFrame);
    const QPoint delta = globalPos - m_movePress;
    const QPoint target = m_moveStartFrame.topLeft()
                          + QPoint(int(std::lround(delta.x() * scale)),
                                   int(std::lround(delta.y() * scale)));
    const QRect frame = frameOnScreen(QRect(target, m_moveStartFrame.size()), globalPos);
    Platform::resizeWindow(m_window.id, frame);
    m_window.geometry = frame;
    refreshWindow();
}

void RecordingSetupOverlay::openResizeMenu()
{
    RoundedMenu menu(this);
    menu.setWindowFlag(Qt::WindowStaysOnTopHint, true);
    menu.setFixedWidth(232);
    menu.setFont(uiFont(13, false));
    const QString sheet = QStringLiteral(
        "QMenu { background: rgba(22,22,25,240); color:#EDEDF0;"
        " border:1px solid rgba(255,255,255,34); border-radius:10px; padding:7px; }"
        "QMenu::item { padding:9px 14px 9px 8px; margin:2px 4px; border-radius:8px; }"
        "QMenu::item:selected { background: rgba(255,102,0,218); color:#ffffff; }"
        "QMenu::item:checked { background: rgba(255,102,0,90); color:#ffffff; }"
        "QMenu::separator { height:1px; background: rgba(255,255,255,22);"
        " margin:6px 12px; }");
    menu.setStyleSheet(sheet);

    const QSize current = m_window.geometry.size();
    const auto addSize = [this, &current](QMenu* target, const QSize& size) {
        QAction* action = target->addAction(
            QStringLiteral("%1 × %2").arg(size.width()).arg(size.height()));
        action->setData(QVariant::fromValue(size));
        action->setCheckable(true);
        action->setChecked(size == current);
        if (!fitsOnScreen(size))
            action->setEnabled(false);
        return action;
    };

    for (const QSize& size : {QSize(1280, 720), QSize(1920, 1080), QSize(2560, 1440)})
        addSize(&menu, size);
    menu.addSeparator();

    struct Ratio {
        QString label;
        QVector<QSize> sizes;
    };
    const QVector<Ratio> ratios = {
        {QStringLiteral("4:3"), {QSize(1440, 1080), QSize(1024, 768)}},
        {QStringLiteral("9:16"), {QSize(720, 1280), QSize(1080, 1920)}},
        {QStringLiteral("16:10"), {QSize(1920, 1200), QSize(1280, 800)}},
        {QStringLiteral("Square"), {QSize(1080, 1080), QSize(720, 720)}},
    };
    for (const Ratio& ratio : ratios) {
        auto* submenu = new RoundedMenu(&menu);
        submenu->setTitle(ratio.label);
        submenu->setStyleSheet(sheet);
        submenu->setFont(uiFont(13, false));
        menu.addMenu(submenu);
        for (const QSize& size : ratio.sizes)
            addSize(submenu, size);
    }

    menu.addSeparator();
    const QVector<QSize> saved = loadWindowSizes();
    if (saved.isEmpty()) {
        QAction* none = menu.addAction(QStringLiteral("No saved sizes"));
        none->setEnabled(false);
    } else {
        for (const QSize& size : saved)
            addSize(&menu, size);
    }
    QAction* saveCurrent = menu.addAction(QStringLiteral("Save current size"));
    menu.addSeparator();
    QAction* custom = menu.addAction(QStringLiteral("Custom…"));

    menu.ensurePolished();
    menu.adjustSize();

    QAction* chosen =
        menu.exec(mapToGlobal(resizeRect().bottomLeft().toPoint() + QPoint(0, 6)));
    if (!chosen)
        return;
    if (chosen == saveCurrent) {
        saveCurrentSize();
        return;
    }
    if (chosen == custom) {
        promptCustomSize();
        return;
    }
    const QSize size = chosen->data().value<QSize>();
    if (size.isValid())
        applyWindowSize(size);
}

void RecordingSetupOverlay::applyWindowSize(const QSize& size)
{
    if (size.width() < 200 || size.height() < 140)
        return;
    const QRect frame = Platform::windowFrame(m_window.id);
    if (frame.isEmpty())
        return;
    const QRect target =
        frameOnScreen(QRect(frame.topLeft(), size), mapToGlobal(m_hole.center().toPoint()));
    Platform::resizeWindow(m_window.id, target);
    m_window.geometry = target;
    refreshWindow();
    // The compositor applies the new frame a beat after SetWindowPos, so take
    // a second reading before the size label is trusted.
    QTimer::singleShot(80, this, [this] {
        if (!m_done)
            refreshWindow();
    });
}

void RecordingSetupOverlay::promptCustomSize()
{
    QDialog dialog(this);
    dialog.setWindowTitle(QStringLiteral("Custom size"));
    dialog.setStyleSheet(QStringLiteral(
        "QDialog { background:#19191c; }"
        "QLabel { color:#f5f5f7; }"
        "QSpinBox { background:#232327; color:#f5f5f7; padding:4px 6px;"
        " border:1px solid rgba(255,255,255,40); border-radius:6px; }"
        "QPushButton { background:#2a2a2e; color:#f5f5f7; padding:6px 16px;"
        " border:1px solid rgba(255,255,255,40); border-radius:6px; }"
        "QPushButton:hover { background:#35353a; }"));
    auto* layout = new QFormLayout(&dialog);
    auto* width = new QSpinBox(&dialog);
    width->setRange(200, 7680);
    width->setSuffix(QStringLiteral(" px"));
    width->setValue(m_window.geometry.width());
    auto* height = new QSpinBox(&dialog);
    height->setRange(140, 4320);
    height->setSuffix(QStringLiteral(" px"));
    height->setValue(m_window.geometry.height());
    layout->addRow(QStringLiteral("Width"), width);
    layout->addRow(QStringLiteral("Height"), height);
    auto* buttons =
        new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, &dialog);
    layout->addRow(buttons);
    QObject::connect(buttons, &QDialogButtonBox::accepted, &dialog, &QDialog::accept);
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    if (dialog.exec() == QDialog::Accepted)
        applyWindowSize(QSize(width->value(), height->value()));
}

void RecordingSetupOverlay::saveCurrentSize()
{
    const QSize current = m_window.geometry.size();
    QVector<QSize> sizes = loadWindowSizes();
    sizes.erase(std::remove(sizes.begin(), sizes.end(), current), sizes.end());
    sizes.prepend(current);
    while (sizes.size() > 6)
        sizes.removeLast();
    saveWindowSizes(sizes);
    overlayLog(QStringLiteral("window size saved %1x%2")
                   .arg(current.width())
                   .arg(current.height()));
}

void RecordingSetupOverlay::keyPressEvent(QKeyEvent* event)
{
    if (event->key() == Qt::Key_Escape)
        cancel();
    else if (event->key() == Qt::Key_Return || event->key() == Qt::Key_Enter)
        startNow();
}

void RecordingSetupOverlay::startNow()
{
    finish();
}

void RecordingSetupOverlay::cancelNow()
{
    cancel();
}

void RecordingSetupOverlay::finish()
{
    if (m_done)
        return;
    m_done = true;
    if (m_refresh)
        m_refresh->stop();
    close();
    deleteLater();
    if (m_onStarted)
        m_onStarted();
}

void RecordingSetupOverlay::cancel()
{
    if (m_done)
        return;
    m_done = true;
    if (m_refresh)
        m_refresh->stop();
    close();
    deleteLater();
    if (m_onCancelled)
        m_onCancelled();
}

}  // namespace Platform
