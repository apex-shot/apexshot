#include "CaptureOverlay.h"
#include "CaptureOverlay_p.h"

#include <QDBusConnection>
#include <QDBusInterface>
#include <QDBusReply>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QPainter>
#include <QPainterPath>
#include <QFontMetrics>
#include <QGuiApplication>
#include <QScreen>
#include <cstdio>
#include <cmath>

#include <X11/Xlib.h>
#include <X11/Xatom.h>
#undef None
#undef Bool

namespace {

constexpr int kWindowPickerToolCount = 2;
constexpr double kWindowPickerToolW = 62.0;
constexpr double kWindowPickerToolH = 62.0;
constexpr double kWindowPickerToolRadius = 13.0;
constexpr int kWindowPickerToolIcons[kWindowPickerToolCount] = {1, 3}; // Area, Window
const char* kWindowPickerToolLabels[kWindowPickerToolCount] = {"Area", "Window"};

// Simple list-row metrics (no thumbnails / previews).
constexpr int kListRowH = 52;
constexpr int kListRowGap = 6;
constexpr int kListPanelMaxW = 560;
constexpr int kListPanelPad = 10;

void drawWindowPickerToolIcon(QPainter& p, int iconId, double cx, double cy, const QColor& col)
{
    p.save();
    p.setRenderHint(QPainter::Antialiasing, true);
    p.setPen(QPen(col, 1.7, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
    p.setBrush(Qt::NoBrush);

    switch (iconId) {
    case 1: // Area
        p.drawRoundedRect(QRectF(cx - 7, cy - 5.5, 14, 11), 2.0, 2.0);
        break;
    case 3: // Window
        p.drawRoundedRect(QRectF(cx - 7.5, cy - 6, 12, 9.5), 1.8, 1.8);
        p.drawRoundedRect(QRectF(cx - 4.5, cy - 3.5, 12, 9.5), 1.8, 1.8);
        break;
    default:
        break;
    }
    p.restore();
}

void drawListRow(QPainter& p,
                 const QRect& row,
                 const QString& title,
                 const QString& appName,
                 const QSize& size,
                 bool hovered)
{
    QPainterPath path;
    path.addRoundedRect(row, 12, 12);

    if (hovered) {
        p.fillPath(path, QColor(176, 92, 56, 210));
        p.setPen(QPen(QColor(255, 212, 178, 255), 1.6));
    } else {
        p.fillPath(path, QColor(255, 255, 255, 18));
        p.setPen(QPen(QColor(255, 255, 255, 36), 1.0));
    }
    p.setBrush(Qt::NoBrush);
    p.drawPath(path);

    // App initial badge (not a preview — just a label).
    QString appLabel = appName.isEmpty() ? title : appName;
    if (appLabel.isEmpty()) {
        appLabel = QStringLiteral("Window");
    }
    const QString badge = appLabel.left(1).toUpper();
    const QRect badgeRect(row.x() + 12, row.y() + (row.height() - 30) / 2, 30, 30);
    p.setPen(Qt::NoPen);
    p.setBrush(hovered ? QColor(255, 255, 255, 40) : QColor(255, 255, 255, 28));
    p.drawEllipse(badgeRect);
    QFont badgeFont;
    badgeFont.setPointSizeF(12.0);
    badgeFont.setBold(true);
    p.setFont(badgeFont);
    p.setPen(QColor(255, 255, 255, 240));
    p.drawText(badgeRect, Qt::AlignCenter, badge);

    const int textLeft = badgeRect.right() + 12;
    const int textRight = row.right() - 12;
    const int textW = std::max(40, textRight - textLeft);

    QFont appFont;
    appFont.setPointSizeF(11.0);
    appFont.setBold(true);
    p.setFont(appFont);
    p.setPen(QColor(255, 255, 255, 240));
    QString appLine = appLabel;
    QFontMetrics afm(appFont);
    appLine = afm.elidedText(appLine, Qt::ElideRight, textW);
    p.drawText(QRect(textLeft, row.y() + 8, textW, 20), Qt::AlignLeft | Qt::AlignVCenter, appLine);

    QFont titleFont;
    titleFont.setPointSizeF(9.5);
    p.setFont(titleFont);
    p.setPen(QColor(255, 255, 255, hovered ? 210 : 170));
    QString titleLine = title.isEmpty() ? appLabel : title;
    QFontMetrics tfm(titleFont);
    titleLine = tfm.elidedText(titleLine, Qt::ElideRight, textW - 70);
    p.drawText(QRect(textLeft, row.y() + 26, textW - 70, 18),
               Qt::AlignLeft | Qt::AlignVCenter,
               titleLine);

    const QString dims = QStringLiteral("%1×%2")
                             .arg(std::max(1, size.width()))
                             .arg(std::max(1, size.height()));
    QFont dimsFont;
    dimsFont.setPointSizeF(8.5);
    p.setFont(dimsFont);
    p.setPen(QColor(255, 255, 255, hovered ? 200 : 150));
    p.drawText(QRect(textLeft, row.y() + 26, textW, 18),
               Qt::AlignRight | Qt::AlignVCenter,
               dims);
}

} // namespace

// ── Origin helpers ────────────────────────────────────────────────────────────

QPoint CaptureOverlay::windowListDesktopOrigin() const
{
    if (isVisible()) {
        return mapToGlobal(QPoint(0, 0));
    }
    if (m_targetScreen) {
        return m_targetScreen->geometry().topLeft();
    }
    return desktopOriginForLocalCoordinates();
}

QRect CaptureOverlay::targetScreenDesktopGeometry() const
{
    if (m_targetScreen) {
        return m_targetScreen->geometry();
    }
    if (QScreen* primary = QGuiApplication::primaryScreen()) {
        return primary->geometry();
    }
    return QRect(0, 0, std::max(1, width()), std::max(1, height()));
}

// Kept for API completeness; list-picker capture uses freeze/area crop only.
bool CaptureOverlay::captureWindowByIdToTemp(quint64 /*windowId*/,
                                             QString& /*outPath*/,
                                             QSize& /*outSize*/) const
{
    return false;
}

// ── Window enumeration (metadata only — no previews) ─────────────────────────

QList<CaptureOverlay::WindowInfo> CaptureOverlay::enumerateWindowsFromX11() const
{
    QList<WindowInfo> result;
    Display* dpy = XOpenDisplay(nullptr);
    if (!dpy) {
        return result;
    }

    Window root = DefaultRootWindow(dpy);
    Atom netClientList = XInternAtom(dpy, "_NET_CLIENT_LIST", true);
    if (netClientList == 0) {
        XCloseDisplay(dpy);
        return result;
    }

    Atom actualType;
    int actualFormat;
    unsigned long nItems = 0;
    unsigned long bytesAfter = 0;
    unsigned char* data = nullptr;

    if (XGetWindowProperty(dpy,
                           root,
                           netClientList,
                           0,
                           1024,
                           false,
                           XA_WINDOW,
                           &actualType,
                           &actualFormat,
                           &nItems,
                           &bytesAfter,
                           &data) == Success
        && data) {
        Window* windows = reinterpret_cast<Window*>(data);
        const QPoint origin = windowListDesktopOrigin();
        const QRect screenGeo = targetScreenDesktopGeometry();

        for (unsigned long i = 0; i < nItems; ++i) {
            Window win = windows[i];

            Window child = 0;
            int rx = 0;
            int ry = 0;
            unsigned int rw = 0;
            unsigned int rh = 0;
            unsigned int bw = 0;
            unsigned int depth = 0;
            if (!XGetGeometry(dpy, win, &root, &rx, &ry, &rw, &rh, &bw, &depth)) {
                continue;
            }
            XTranslateCoordinates(dpy, win, DefaultRootWindow(dpy), 0, 0, &rx, &ry, &child);

            if ((int)rw < 50 || (int)rh < 50) {
                continue;
            }

            const QRect desktopRect(rx, ry, (int)rw, (int)rh);
            if (!screenGeo.intersects(desktopRect)) {
                continue;
            }

            QString title;
            Atom netWmName = XInternAtom(dpy, "_NET_WM_NAME", false);
            Atom utf8Str = XInternAtom(dpy, "UTF8_STRING", false);
            unsigned char* nameProp = nullptr;
            Atom nameType;
            int nameFmt = 0;
            unsigned long nameItems = 0;
            unsigned long nameAfter = 0;
            if (XGetWindowProperty(dpy,
                                   win,
                                   netWmName,
                                   0,
                                   256,
                                   false,
                                   utf8Str,
                                   &nameType,
                                   &nameFmt,
                                   &nameItems,
                                   &nameAfter,
                                   &nameProp)
                    == Success
                && nameProp) {
                title = QString::fromUtf8(reinterpret_cast<char*>(nameProp));
                XFree(nameProp);
            } else {
                char* wmName = nullptr;
                if (XFetchName(dpy, win, &wmName) && wmName) {
                    title = QString::fromLocal8Bit(wmName);
                    XFree(wmName);
                }
            }
            if (title.isEmpty()) {
                title = QStringLiteral("(Unnamed)");
            }

            WindowInfo info;
            info.id = static_cast<quint64>(win);
            info.desktopRect = desktopRect;
            info.rect = QRect(rx - origin.x(), ry - origin.y(), (int)rw, (int)rh);
            info.title = title;
            info.appName = title;
            result.prepend(info);
        }
        XFree(data);
    }

    XCloseDisplay(dpy);
    return result;
}

QList<CaptureOverlay::WindowInfo> CaptureOverlay::enumerateWindowsFromExtension() const
{
    QList<WindowInfo> result;

    QDBusInterface iface(QStringLiteral("org.apexshot.WindowList"),
                         QStringLiteral("/org/apexshot/WindowList"),
                         QStringLiteral("org.apexshot.WindowList"),
                         QDBusConnection::sessionBus());
    if (!iface.isValid()) {
        std::fprintf(stderr,
                     "[CaptureOverlay] Window list D-Bus unavailable; trying X11 fallback\n");
        return result;
    }

    // Fast metadata-only call (no per-window surface thumbnails).
    iface.setTimeout(5000);
    QDBusReply<QString> reply = iface.call(QStringLiteral("GetWindows"));
    if (!reply.isValid()) {
        std::fprintf(stderr,
                     "[CaptureOverlay] GetWindows failed: %s\n",
                     reply.error().message().toLocal8Bit().constData());
        return result;
    }

    const QJsonDocument doc = QJsonDocument::fromJson(reply.value().toUtf8());
    if (!doc.isArray()) {
        return result;
    }

    const QPoint origin = windowListDesktopOrigin();
    const QRect screenGeo = targetScreenDesktopGeometry();

    for (const QJsonValue& value : doc.array()) {
        const QJsonObject obj = value.toObject();
        const int x = obj.value(QStringLiteral("x")).toInt();
        const int y = obj.value(QStringLiteral("y")).toInt();
        const int w = obj.value(QStringLiteral("width")).toInt();
        const int h = obj.value(QStringLiteral("height")).toInt();
        if (w < 32 || h < 32) {
            continue;
        }
        if (obj.value(QStringLiteral("apexshot")).toBool()) {
            continue;
        }

        const QRect desktopRect(x, y, w, h);
        if (!screenGeo.intersects(desktopRect)) {
            continue;
        }

        WindowInfo info;
        info.id = static_cast<quint64>(obj.value(QStringLiteral("id")).toDouble());
        info.desktopRect = desktopRect;
        info.rect = QRect(x - origin.x(), y - origin.y(), w, h);
        info.title = obj.value(QStringLiteral("title")).toString();
        info.appName = obj.value(QStringLiteral("app")).toString();
        if (info.title.isEmpty()) {
            info.title = info.appName.isEmpty() ? QStringLiteral("Window") : info.appName;
        }
        // Intentionally no thumbnail — list picker only.
        result.append(info);
        std::fprintf(stderr,
                     "[CaptureOverlay] Window list: id=%llu '%s' app='%s' %dx%d\n",
                     static_cast<unsigned long long>(info.id),
                     info.title.toLocal8Bit().constData(),
                     info.appName.toLocal8Bit().constData(),
                     w,
                     h);
    }

    return result;
}

QList<CaptureOverlay::WindowInfo> CaptureOverlay::enumerateWindows() const
{
    // Metadata only: extension (all workspaces on GNOME) then X11.
    QList<WindowInfo> windows = enumerateWindowsFromExtension();
    if (windows.isEmpty()) {
        windows = enumerateWindowsFromX11();
    }
    return windows;
}

// ── Layout / hit testing ─────────────────────────────────────────────────────

void CaptureOverlay::recomputeWindowPickerLayout()
{
    m_windowCardRects.clear();
    if (width() <= 0 || height() <= 0 || m_windows.isEmpty()) {
        return;
    }

    const int topInset = 84;
    const int bottomInset = 120;
    const int usableHeight = std::max(120, height() - topInset - bottomInset);
    const int panelW = std::min(kListPanelMaxW, width() - 48);
    const int panelX = (width() - panelW) / 2;
    const int totalH = m_windows.size() * kListRowH
                       + std::max(0, m_windows.size() - 1) * kListRowGap
                       + kListPanelPad * 2;
    const int panelH = std::min(usableHeight, totalH);
    const int panelY = topInset + std::max(0, (usableHeight - panelH) / 2);

    // Visible rows from the top of the list panel (no scroll yet).
    int y = panelY + kListPanelPad;
    const int rowW = panelW - kListPanelPad * 2;
    for (int i = 0; i < m_windows.size(); ++i) {
        if (y + kListRowH > panelY + panelH - kListPanelPad) {
            break; // remaining rows clipped; still keep rects for hit-test of visible ones
        }
        m_windowCardRects.append(QRect(panelX + kListPanelPad, y, rowW, kListRowH));
        y += kListRowH + kListRowGap;
    }

    // If more windows than fit, still allocate remaining rows below for hit testing
    // by extending with a simple scroll-less overflow (show as many as fit).
    // Users with many windows get the top-of-z-order set first.
    Q_UNUSED(m_windowCardRects);
}

QRectF CaptureOverlay::windowPickerToolbarRect() const
{
    const double panelW = kWindowPickerToolW * kWindowPickerToolCount;
    const double panelX = (width() - panelW) / 2.0;
    const double panelY = height() - kWindowPickerToolH - 24.0;
    return QRectF(panelX, panelY, panelW, kWindowPickerToolH);
}

QRectF CaptureOverlay::windowPickerToolbarItemRect(int index) const
{
    const QRectF panel = windowPickerToolbarRect();
    return QRectF(panel.x() + index * kWindowPickerToolW,
                  panel.y(),
                  kWindowPickerToolW,
                  kWindowPickerToolH);
}

int CaptureOverlay::hitTestWindowPickerToolbar(const QPoint& pos) const
{
    for (int i = 0; i < kWindowPickerToolCount; ++i) {
        if (windowPickerToolbarItemRect(i).contains(pos)) {
            return i;
        }
    }
    return -1;
}

int CaptureOverlay::hitTestWindowPickerCard(const QPoint& pos) const
{
    for (int i = m_windowCardRects.size() - 1; i >= 0; --i) {
        if (m_windowCardRects[i].contains(pos)) {
            return i;
        }
    }
    return -1;
}

// ── Mode enter / exit ────────────────────────────────────────────────────────

void CaptureOverlay::openWindowPickerMode()
{
    enterWindowMode();
}

void CaptureOverlay::enterWindowMode()
{
    exitScrollMode();
    m_captureCropMenuOpen = false;
    m_hoveredCaptureCropMenuItem = -1;
    m_captureCropMenuPanelRect = QRectF();
    m_captureCropMenuItemRects.clear();
    m_recordingPanelOpen = false;
    m_settingsOpen = false;
    m_scrollPopupOpen = false;

    m_selectionBeforeWindowMode = m_selection;
    m_hadSelectionBeforeWindowMode = m_hasSelection;
    m_fullscreenBeforeWindowMode = m_fullscreenMode;

    m_windowMode = true;
    m_fullscreenMode = false;
    m_captureIntent = CaptureIntent::Area;
    m_hasSelection = false;
    m_hoveredWindow = -1;
    m_hoveredWindowTool = -1;
    m_preCapturedImagePath.clear();
    m_windows = enumerateWindows();
    recomputeWindowPickerLayout();

    std::fprintf(stderr,
                 "[CaptureOverlay] Entered window list picker (%d windows)\n",
                 m_windows.size());

    setCursor(Qt::ArrowCursor);
    update();
}

void CaptureOverlay::exitWindowMode(bool restoreAreaSelection)
{
    const bool wasWindowMode = m_windowMode;
    m_windowMode = false;
    m_hoveredWindow = -1;
    m_hoveredWindowTool = -1;
    m_windows.clear();
    m_windowCardRects.clear();

    if (wasWindowMode && restoreAreaSelection) {
        if (m_hadSelectionBeforeWindowMode) {
            m_selection = m_selectionBeforeWindowMode;
            m_hasSelection = true;
            m_fullscreenMode = m_fullscreenBeforeWindowMode;
        } else {
            const int defaultW = std::max(kMinSize, std::min(DEFAULT_SELECTION_W, width()));
            const int defaultH = std::max(kMinSize, std::min(DEFAULT_SELECTION_H, height()));
            m_selection = QRect((width() - defaultW) / 2,
                                (height() - defaultH) / 2,
                                defaultW,
                                defaultH);
            m_hasSelection = true;
            m_fullscreenMode = false;
        }
        m_captureIntent = CaptureIntent::Area;
        m_captureAspectRatioIndex = 0;
    }

    setCursor(Qt::ArrowCursor);
    update();
}

// ── Painting: simple list, no previews ───────────────────────────────────────

void CaptureOverlay::drawWindowPickerMode(QPainter& p, const QRect& widgetRect)
{
    if (m_windowCardRects.size() != m_windows.size()
        && !(m_windowCardRects.size() < m_windows.size() && !m_windows.isEmpty())) {
        recomputeWindowPickerLayout();
    }
    if (m_windowCardRects.isEmpty() && !m_windows.isEmpty()) {
        recomputeWindowPickerLayout();
    }

    p.fillRect(widgetRect, QColor(0, 0, 0, 120));

    QFont titleFont;
    titleFont.setPointSizeF(16.0);
    titleFont.setBold(true);
    p.setFont(titleFont);
    p.setPen(QColor(255, 255, 255, 230));
    p.drawText(QRect(0, 18, width(), 50),
               Qt::AlignCenter,
               QStringLiteral("Select a window"));

    if (m_windows.isEmpty()) {
        QFont f;
        f.setPointSizeF(12.5);
        p.setFont(f);
        p.setPen(QColor(255, 255, 255, 175));
        p.drawText(widgetRect.adjusted(40, 0, -40, -40),
                   Qt::AlignCenter,
                   QStringLiteral(
                       "No windows found on this display.\n"
                       "Click Area to go back.\n"
                       "On GNOME Wayland, enable the ApexShot extension for the window list."));
    } else {
        // Panel behind the list
        if (!m_windowCardRects.isEmpty()) {
            const QRect first = m_windowCardRects.first();
            const QRect last = m_windowCardRects.last();
            const QRect panel(first.x() - kListPanelPad,
                              first.y() - kListPanelPad,
                              first.width() + kListPanelPad * 2,
                              last.bottom() - first.y() + kListPanelPad * 2 + 1);
            QPainterPath panelPath;
            panelPath.addRoundedRect(panel, 16, 16);
            p.fillPath(panelPath, QColor(20, 20, 26, 230));
            p.setPen(QPen(QColor(255, 255, 255, 28), 1.0));
            p.setBrush(Qt::NoBrush);
            p.drawPath(panelPath);
        }

        for (int i = 0; i < m_windowCardRects.size() && i < m_windows.size(); ++i) {
            if (i == m_hoveredWindow) {
                continue;
            }
            const WindowInfo& win = m_windows[i];
            drawListRow(p,
                        m_windowCardRects[i],
                        win.title,
                        win.appName,
                        win.desktopRect.isEmpty() ? win.rect.size() : win.desktopRect.size(),
                        false);
        }
        if (m_hoveredWindow >= 0 && m_hoveredWindow < m_windowCardRects.size()
            && m_hoveredWindow < m_windows.size()) {
            const WindowInfo& win = m_windows[m_hoveredWindow];
            drawListRow(p,
                        m_windowCardRects[m_hoveredWindow],
                        win.title,
                        win.appName,
                        win.desktopRect.isEmpty() ? win.rect.size() : win.desktopRect.size(),
                        true);
        }

        if (m_windowCardRects.size() < m_windows.size()) {
            QFont moreFont;
            moreFont.setPointSizeF(9.5);
            p.setFont(moreFont);
            p.setPen(QColor(255, 255, 255, 150));
            const int more = m_windows.size() - m_windowCardRects.size();
            p.drawText(QRect(0, height() - 130, width(), 20),
                       Qt::AlignCenter,
                       QStringLiteral("+%1 more on this display (not shown)").arg(more));
        }
    }

    // Bottom hint
    QFont hintFont;
    hintFont.setPointSizeF(10.5);
    p.setFont(hintFont);
    const QString hint = QStringLiteral("ESC or Area to go back  •  Click a row to capture");
    QFontMetrics hfm(hintFont);
    const int hw = hfm.horizontalAdvance(hint) + 28;
    const int hx = (width() - hw) / 2;
    const int hy = height() - 108;
    QPainterPath hpill;
    hpill.addRoundedRect(QRectF(hx, hy, hw, 28), 10, 10);
    p.fillPath(hpill, QColor(0, 0, 0, 140));
    p.setPen(QColor(255, 255, 255, 165));
    p.drawText(QRect(hx, hy, hw, 28), Qt::AlignCenter, hint);

    // Reduced toolbar: Area + Window
    const QRectF panel = windowPickerToolbarRect();
    QPainterPath panelPath;
    panelPath.addRoundedRect(panel, kWindowPickerToolRadius, kWindowPickerToolRadius);
    p.fillPath(panelPath, QColor(28, 28, 34, 220));
    p.setPen(QPen(QColor(255, 255, 255, 28), 1.0));
    p.setBrush(Qt::NoBrush);
    p.drawPath(panelPath);

    const QColor warmFill(176, 92, 56, 210);
    const QColor hoverFill(255, 255, 255, 28);
    const QColor hoverRim(255, 212, 178, 160);
    const QColor activeText(255, 236, 220, 255);

    auto drawAccentCell = [&](const QRectF& cell, const QColor& fill, const QColor& rim) {
        const double hx = cell.x() + 4.0;
        const double hy = cell.y() + 4.0;
        const double hw = cell.width() - 8.0;
        const double hh = cell.height() - 8.0;
        QPainterPath card;
        card.addRoundedRect(QRectF(hx, hy, hw, hh), 10.0, 10.0);
        p.fillPath(card, fill);
        if (rim.alpha() > 0) {
            p.setPen(QPen(rim, 1.2));
            p.setBrush(Qt::NoBrush);
            p.drawPath(card);
        }
    };

    drawAccentCell(windowPickerToolbarItemRect(1), warmFill, QColor(0, 0, 0, 0));
    if (m_hoveredWindowTool >= 0 && m_hoveredWindowTool < kWindowPickerToolCount) {
        drawAccentCell(windowPickerToolbarItemRect(m_hoveredWindowTool), hoverFill, hoverRim);
    }

    for (int i = 0; i < kWindowPickerToolCount; ++i) {
        const QRectF cell = windowPickerToolbarItemRect(i);
        const double cx = cell.x() + cell.width() / 2.0;
        const bool hovered = (m_hoveredWindowTool == i);
        const bool active = (i == 1);
        const double iconY = cell.y() + ((hovered || active) ? 23.5 : 24.0);
        const QColor iconColor = active ? activeText : QColor(255, 255, 255, 240);

        drawWindowPickerToolIcon(p,
                                 kWindowPickerToolIcons[i],
                                 cx + 0.6,
                                 iconY + 0.8,
                                 QColor(0, 0, 0, hovered ? 62 : 118));
        drawWindowPickerToolIcon(p, kWindowPickerToolIcons[i], cx, iconY, iconColor);

        QFont f;
        f.setFamily(QStringLiteral("Sans"));
        f.setPointSizeF(7.1);
        f.setBold(hovered || active);
        p.setFont(f);
        QFontMetricsF fm(f);
        const QString label(kWindowPickerToolLabels[i]);
        const double tw = fm.horizontalAdvance(label);
        p.setPen(QColor(0, 0, 0, hovered ? 62 : 118));
        p.drawText(QPointF(cx - tw / 2.0 + 0.6, cell.y() + 50.0 + 0.8), label);
        p.setPen(active ? activeText : QColor(244, 244, 244, 240));
        p.drawText(QPointF(cx - tw / 2.0, cell.y() + 50.0), label);
    }
}

QRegion CaptureOverlay::windowHoverDirtyRegion(int index) const
{
    if (index < 0 || index >= m_windowCardRects.size()) {
        return QRegion();
    }
    return QRegion(m_windowCardRects[index].adjusted(-8, -8, 8, 8));
}
