#include "WindowPickerOverlay.h"

#include <QPainter>
#include <QPainterPath>
#include <QMouseEvent>
#include <QKeyEvent>
#include <QScreen>
#include <QGuiApplication>
#include <QApplication>
#include <QFont>
#include <QFontMetrics>
#include <QDBusConnection>
#include <QDBusInterface>
#include <QDBusReply>
#include <QJsonDocument>
#include <QJsonArray>
#include <QJsonObject>
#include <QImage>
#include <QLinearGradient>
#include <QPixmap>
#include <QByteArray>
#include <QCoreApplication>
#include <QProcess>
#include <QThread>
#include <QResizeEvent>
#include <QTimer>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QDir>
#include <cstdio>
#include <cmath>
#include <algorithm>

// ── Toolbar constants (reduced capture-toolbar subset) ───────────────────────
static const double TB_ITEM_W   = 62.0;
static const double TB_H        = 58.0;
static const double TB_RADIUS   = 13.0;
static const int    TB_NUM      = 2;
static const int    WINDOW_PICKER_TOOL_INDICES[] = {1, 3};
static const char*  TB_LABELS[] = {
    "Area", "Window"
};
static const QColor TB_WARM_FILL(176, 92, 56, 76);
static const QColor TB_WARM_RIM(255, 212, 178, 152);
static const QColor TB_HOVER_FILL(255, 255, 255, 22);
static const QColor TB_HOVER_RIM(255, 255, 255, 86);
static const QColor TB_ACTIVE_TEXT(255, 229, 206, 255);

static void tbRoundedPath(QPainterPath& path, double x, double y, double w, double h, double r) {
    r = std::min(r, std::min(w/2.0, h/2.0));
    path.addRoundedRect(QRectF(x,y,w,h), r, r);
}

static void drawTbFrostedPanel(QPainter& p, double x, double y, double w, double h, double r) {
    QPainterPath shadow; tbRoundedPath(shadow, x, y + 3, w, h, r);
    p.fillPath(shadow, QColor(0, 0, 0, 77));

    p.save();
    QPainterPath clip; tbRoundedPath(clip, x, y, w, h, r);
    p.setClipPath(clip);
    p.fillRect(QRectF(x, y, w, h), QColor(20, 20, 20, 230));
    p.fillRect(QRectF(x, y, w, h), QColor(255, 255, 255, 10));
    p.restore();

    p.setPen(QPen(QColor(255, 255, 255, 26), 1.0));
    p.setBrush(Qt::NoBrush);
    p.drawPath(clip);
}

static void drawTbIcon(QPainter& p, int idx, double cx, double cy, QColor col) {
    p.save();
    p.setPen(QPen(col, 1.6, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
    p.setBrush(Qt::NoBrush);
    switch (idx) {
    case 0: p.drawEllipse(QPointF(cx,cy),6.2,6.2);
            p.drawLine(QPointF(cx-3.2,cy),QPointF(cx+3.2,cy));
            p.drawLine(QPointF(cx,cy-3.2),QPointF(cx,cy+3.2)); break;
    case 1: { QPainterPath path;
              double h=5.5;
              path.moveTo(cx-7,cy-1.5);path.lineTo(cx-7,cy-h);path.lineTo(cx-1.5,cy-h);
              path.moveTo(cx+1.5,cy-h);path.lineTo(cx+7,cy-h);path.lineTo(cx+7,cy-1.5);
              path.moveTo(cx-7,cy+1.5);path.lineTo(cx-7,cy+h);path.lineTo(cx-1.5,cy+h);
              path.moveTo(cx+1.5,cy+h);path.lineTo(cx+7,cy+h);path.lineTo(cx+7,cy+1.5);
              p.drawPath(path); break; }
    case 2: { QPainterPath path; tbRoundedPath(path,cx-7,cy-6,14,10.5,2);
              p.drawPath(path);
              p.drawLine(QPointF(cx,cy+4.5),QPointF(cx,cy+7.5));
              p.drawLine(QPointF(cx-4.5,cy+7.5),QPointF(cx+4.5,cy+7.5)); break; }
    case 3: { QPainterPath path; tbRoundedPath(path,cx-7,cy-5.5,14,9.5,1.7);
              p.drawPath(path);
              p.drawLine(QPointF(cx-7,cy-2),QPointF(cx+7,cy-2)); break; }
    case 4: { QPainterPath path;
              path.moveTo(cx,cy-4.8);path.lineTo(cx,cy+1.8);
              path.moveTo(cx-3.2,cy-1);path.lineTo(cx,cy+1.9);path.lineTo(cx+3.2,cy-1);
              p.drawPath(path); break; }
    case 5: p.drawEllipse(QPointF(cx,cy),6,6);
            { QPainterPath h; h.moveTo(cx,cy);h.lineTo(cx,cy-2.8);
              h.moveTo(cx,cy);h.lineTo(cx+2.2,cy+1.7); p.drawPath(h); } break;
    case 6: { QFont f=p.font(); f.setPointSizeF(8); f.setBold(true); p.setFont(f);
              QFontMetricsF fm(f); QString t("Aa");
              QRectF br=fm.boundingRect(t);
              p.setPen(col);
              p.drawText(QPointF(cx-br.width()/2,cy+br.height()/2-fm.descent()+0.2),t); break; }
    case 7: { QPainterPath path; tbRoundedPath(path,cx-6.5,cy-4.3,10,8.6,2);
              p.drawPath(path); p.drawEllipse(QPointF(cx-1.3,cy),2.2,2.2);
              QPainterPath bump; tbRoundedPath(bump,cx+3.8,cy-2.2,3.6,4.4,0.8);
              p.fillPath(bump,col); break; }
    }
    p.restore();
}

static Qt::WindowFlags windowPickerWindowFlags()
{
    if (qEnvironmentVariableIsSet("WAYLAND_DISPLAY")) {
        return Qt::Popup
               | Qt::FramelessWindowHint
               | Qt::WindowStaysOnTopHint
               | Qt::BypassWindowManagerHint;
    }

    return Qt::FramelessWindowHint
           | Qt::WindowStaysOnTopHint
           | Qt::Tool;
}

// ── Toolbar helpers ───────────────────────────────────────────────────────────

QRectF WindowPickerOverlay::toolbarItemRect(int i) const
{
    double panelW = TB_ITEM_W * TB_NUM;
    double panelX = (width() - panelW) / 2.0;
    double panelY = height() - TB_H - 24.0;
    m_toolbarRect = QRectF(panelX, panelY, panelW, TB_H);
    return QRectF(panelX + i * TB_ITEM_W, panelY, TB_ITEM_W, TB_H);
}

void WindowPickerOverlay::drawToolbar(QPainter& p)
{
    const double panelW = TB_ITEM_W * TB_NUM;
    const double panelX = (width() - panelW) / 2.0;
    const double panelY = height() - TB_H - 24.0;
    m_toolbarRect = QRectF(panelX, panelY, panelW, TB_H);

    drawTbFrostedPanel(p, panelX, panelY, panelW, TB_H, TB_RADIUS);

    auto drawAccentCell = [&](const QRectF& cell, const QColor& fill, const QColor& rim) {
        const double hx = cell.x() + 4.0;
        const double hy = cell.y() + 4.0;
        const double hw = cell.width() - 8.0;
        const double hh = cell.height() - 8.0;
        QPainterPath card;
        tbRoundedPath(card, hx, hy, hw, hh, 10.0);
        p.fillPath(card, fill);
        if (rim.alpha() > 0) {
            p.save();
            p.setClipPath(card);
            p.setPen(QPen(rim, 1.2));
            p.setBrush(Qt::NoBrush);
            QPainterPath border;
            tbRoundedPath(border, hx + 0.6, hy + 0.6, hw - 1.2, hh - 1.2, 9.5);
            p.drawPath(border);
            p.restore();
        }
    };

    const int activeTool = 1;
    drawAccentCell(toolbarItemRect(activeTool), TB_WARM_FILL, QColor(0, 0, 0, 0));

    if (m_hoveredTool >= 0 && m_hoveredTool < TB_NUM) {
        drawAccentCell(toolbarItemRect(m_hoveredTool), TB_HOVER_FILL, TB_HOVER_RIM);
    }

    for (int i = 0; i < TB_NUM; ++i) {
        const QRectF cell = toolbarItemRect(i);
        const double cx = cell.x() + cell.width() / 2.0;
        const bool hovered = (m_hoveredTool == i);
        const bool active = (activeTool == i);
        const double shadowAlpha = hovered ? 0.24 : (active ? 0.32 : 0.50);
        const double iconY = cell.y() + ((hovered || active) ? 23.5 : 24.0);
        const QColor iconColor = active ? TB_ACTIVE_TEXT : QColor(255, 255, 255, 240);

        drawTbIcon(p, WINDOW_PICKER_TOOL_INDICES[i], cx + 0.6, iconY + 0.8,
                   QColor(0, 0, 0, int(shadowAlpha * 255)));
        drawTbIcon(p, WINDOW_PICKER_TOOL_INDICES[i], cx, iconY, iconColor);

        QFont f; f.setFamily("Sans"); f.setPointSizeF(7.1); f.setBold(hovered || active); p.setFont(f);
        QFontMetricsF fm(f);
        const QString label(TB_LABELS[i]);
        const double tw = fm.horizontalAdvance(label);
        p.setPen(QColor(0, 0, 0, int(shadowAlpha * 255)));
        p.drawText(QPointF(cx - tw / 2.0 + 0.6, cell.y() + 50.0 + 0.8), label);
        p.setPen(active ? TB_ACTIVE_TEXT : QColor(244, 244, 244, 240));
        p.drawText(QPointF(cx - tw / 2.0, cell.y() + 50.0), label);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

static QPixmap loadAndBlurWallpaper(int targetW, int targetH)
{
    // Get wallpaper path from gsettings
    QProcess proc;
    proc.start(QStringLiteral("gsettings"),
        {QStringLiteral("get"), QStringLiteral("org.gnome.desktop.background"),
         QStringLiteral("picture-uri")});
    proc.waitForFinished(3000);
    QString uri = QString::fromUtf8(proc.readAllStandardOutput()).trimmed();
    uri.remove(QStringLiteral("'"));
    if (uri.startsWith(QStringLiteral("file://")))
        uri = uri.mid(7);

    QImage img(uri);
    if (img.isNull()) {
        // Fallback: dark gradient
        QImage fallback(targetW, targetH, QImage::Format_RGB32);
        fallback.fill(QColor(18, 18, 24));
        return QPixmap::fromImage(fallback);
    }

    // Scale to screen size
    img = img.scaled(targetW, targetH, Qt::KeepAspectRatioByExpanding,
                     Qt::SmoothTransformation).copy(0, 0, targetW, targetH);

    // Simple box blur — fast and effective
    // Blur 3 passes at 1/4 resolution for performance
    QImage small = img.scaled(targetW / 4, targetH / 4, Qt::IgnoreAspectRatio,
                               Qt::SmoothTransformation);
    for (int i = 0; i < 3; ++i)
        small = small.scaled(small.width() / 2, small.height() / 2,
                             Qt::IgnoreAspectRatio, Qt::SmoothTransformation)
                     .scaled(targetW / 4, targetH / 4,
                             Qt::IgnoreAspectRatio, Qt::SmoothTransformation);
    img = small.scaled(targetW, targetH, Qt::IgnoreAspectRatio, Qt::SmoothTransformation);

    // Dark overlay on top of blur
    QPainter p(&img);
    p.fillRect(img.rect(), QColor(0, 0, 0, 120));
    p.end();

    return QPixmap::fromImage(img);
}

static QList<AppWindowInfo> fetchWindowsFromExtension()
{
    QList<AppWindowInfo> result;

    auto resolveCapturedPath = [](const QString& requestedPath) -> QString {
        const QStringList candidates = {
            requestedPath,
            requestedPath + QStringLiteral(".png"),
            requestedPath + QStringLiteral("-1.png"),
            requestedPath + QStringLiteral("-0.png"),
            requestedPath.endsWith(QStringLiteral(".png"))
                ? requestedPath.left(requestedPath.size() - 4) + QStringLiteral("-1.png")
                : QString(),
            requestedPath.endsWith(QStringLiteral(".png"))
                ? requestedPath.left(requestedPath.size() - 4) + QStringLiteral("-0.png")
                : QString(),
        };

        for (const QString& candidate : candidates) {
            if (candidate.isEmpty())
                continue;
            QFileInfo info(candidate);
            if (info.exists() && info.isFile() && info.size() > 0) {
                return candidate;
            }
        }
        return QString();
    };

    auto waitForCapturedPath = [&](const QString& requestedPath, int timeoutMs) -> QString {
        QElapsedTimer timer;
        timer.start();

        while (timer.elapsed() < timeoutMs) {
            const QString found = resolveCapturedPath(requestedPath);
            if (!found.isEmpty()) {
                return found;
            }
            QThread::msleep(40);
        }

        return resolveCapturedPath(requestedPath);
    };

    QDBusInterface iface(
        QStringLiteral("org.apexshot.WindowList"),
        QStringLiteral("/org/apexshot/WindowList"),
        QStringLiteral("org.apexshot.WindowList"),
        QDBusConnection::sessionBus());

    if (!iface.isValid()) {
        std::fprintf(stderr, "[WindowPicker] DBus interface not available\n");
        return result;
    }

    auto captureWindowThumbnail = [&](quint64 windowId) -> QPixmap {
        const QString requestedPath = QDir::tempPath() +
            QStringLiteral("/apexshot-thumb-%1-%2.png")
                .arg(QCoreApplication::applicationPid())
                .arg(static_cast<qulonglong>(windowId));

        QFile::remove(requestedPath);

        QDBusReply<bool> captureReply = iface.call(
            QStringLiteral("CaptureWindowById"),
            static_cast<quint32>(windowId),
            requestedPath);

        if (!captureReply.isValid() || !captureReply.value()) {
            return QPixmap();
        }

        const QString actualPath = waitForCapturedPath(requestedPath, 2200);
        if (actualPath.isEmpty()) {
            return QPixmap();
        }

        QPixmap pix(actualPath);
        QFile::remove(actualPath);
        if (actualPath != requestedPath)
            QFile::remove(requestedPath);

        return pix;
    };

    QDBusReply<QString> reply = iface.call(QStringLiteral("GetWindows"));
    if (!reply.isValid()) {
        std::fprintf(stderr, "[WindowPicker] GetWindows failed: %s\n",
            reply.error().message().toLocal8Bit().constData());
        return result;
    }

    QJsonDocument doc = QJsonDocument::fromJson(reply.value().toUtf8());
    if (!doc.isArray()) return result;

    for (const QJsonValue& v : doc.array()) {
        QJsonObject obj = v.toObject();
        AppWindowInfo info;
        info.xid     = (quint64)obj[QStringLiteral("id")].toDouble();
        info.title   = obj[QStringLiteral("title")].toString();
        info.appName = obj[QStringLiteral("app")].toString();
        info.rect    = QRect(
            obj[QStringLiteral("x")].toInt(),
            obj[QStringLiteral("y")].toInt(),
            obj[QStringLiteral("width")].toInt(),
            obj[QStringLiteral("height")].toInt());

        const QString thumbnailB64 = obj[QStringLiteral("thumbnail_b64")].toString();
        if (!thumbnailB64.isEmpty()) {
            const QByteArray decoded = QByteArray::fromBase64(thumbnailB64.toUtf8());
            if (!decoded.isEmpty()) {
                info.icon.loadFromData(decoded, "PNG");
            }
        }

        if (info.icon.isNull()) {
            info.icon = captureWindowThumbnail(info.xid);
        }

        result.append(info);
        std::fprintf(stderr, "[WindowPicker] Window: '%s' app='%s' @ %d,%d %dx%d\n",
            info.title.toLocal8Bit().constData(),
            info.appName.toLocal8Bit().constData(),
            info.rect.x(), info.rect.y(), info.rect.width(), info.rect.height());
    }
    return result;
}

// ── Layout computation ────────────────────────────────────────────────────────

// Grid layout to avoid overlap/stacking when one window is fullscreen.
// Each card preserves an approximate aspect ratio of the source window.
static QList<QRect> computeLayout(const QList<AppWindowInfo>& windows,
                                   const QRect& displayArea, int padding)
{
    if (windows.isEmpty()) return {};

    const QRect usable = displayArea.adjusted(padding, padding, -padding, -padding);
    if (usable.width() <= 0 || usable.height() <= 0) return {};

    const int count = windows.size();
    int cols = static_cast<int>(std::ceil(std::sqrt(static_cast<double>(count))));
    cols = std::clamp(cols, 1, 4);
    if (count <= 2) cols = count;
    const int rows = (count + cols - 1) / cols;

    const int gap = 16;
    const int totalGapX = gap * (cols - 1);
    const int totalGapY = gap * (rows - 1);
    const int cellW = std::max(1, (usable.width() - totalGapX) / cols);
    const int cellH = std::max(1, (usable.height() - totalGapY) / rows);

    QList<QRect> result;
    result.reserve(count);

    for (int i = 0; i < count; ++i) {
        const AppWindowInfo& w = windows[i];
        const int sourceW = std::max(1, w.rect.width());
        const int sourceH = std::max(1, w.rect.height());

        double aspect = static_cast<double>(sourceW) / static_cast<double>(sourceH);
        aspect = std::clamp(aspect, 0.65, 2.1);

        int thumbW = cellW;
        int thumbH = static_cast<int>(std::round(static_cast<double>(thumbW) / aspect));
        if (thumbH > cellH) {
            thumbH = cellH;
            thumbW = static_cast<int>(std::round(static_cast<double>(thumbH) * aspect));
        }

        thumbW = std::max(1, std::min(thumbW, cellW));
        thumbH = std::max(1, std::min(thumbH, cellH));

        const int row = i / cols;
        const int col = i % cols;
        const int baseX = usable.x() + col * (cellW + gap);
        const int baseY = usable.y() + row * (cellH + gap);

        const int x = baseX + (cellW - thumbW) / 2;
        const int y = baseY + (cellH - thumbH) / 2;

        result.append(QRect(x, y, thumbW, thumbH));
    }

    return result;
}

static void drawWindowCard(QPainter& p,
                           const QRect& thumb,
                           const AppWindowInfo& win,
                           const QPixmap& scaledThumbnail,
                           bool hovered)
{
    const bool hasThumbnail = !scaledThumbnail.isNull();

    QPainterPath cardPath;
    cardPath.addRoundedRect(thumb, 10, 10);

    if (hasThumbnail) {
        const int drawX = thumb.x() + (thumb.width() - scaledThumbnail.width()) / 2;
        const int drawY = thumb.y() + (thumb.height() - scaledThumbnail.height()) / 2;
        p.save();
        p.setClipPath(cardPath);
        p.drawPixmap(drawX, drawY, scaledThumbnail);
        p.fillRect(thumb, hovered ? QColor(0, 0, 0, 22) : QColor(0, 0, 0, 40));
        p.restore();
    }

    QLinearGradient gradient(thumb.topLeft(), thumb.bottomRight());
    if (hasThumbnail) {
        if (hovered) {
            gradient.setColorAt(0.0, QColor(176, 92, 56, 36));
            gradient.setColorAt(1.0, QColor(72, 34, 24, 44));
            p.setPen(QPen(QColor(255, 212, 178, 255), 2.8));
        } else {
            gradient.setColorAt(0.0, QColor(255, 255, 255, 12));
            gradient.setColorAt(1.0, QColor(24, 24, 30, 54));
            p.setPen(QPen(QColor(255, 255, 255, 152), 1.7));
        }
    } else if (hovered) {
        gradient.setColorAt(0.0, QColor(176, 92, 56, 84));
        gradient.setColorAt(1.0, QColor(72, 34, 24, 124));
        p.setPen(QPen(QColor(255, 212, 178, 255), 2.8));
    } else {
        gradient.setColorAt(0.0, QColor(255, 255, 255, 32));
        gradient.setColorAt(1.0, QColor(24, 24, 30, 150));
        p.setPen(QPen(QColor(255, 255, 255, 124), 1.7));
    }
    p.fillPath(cardPath, gradient);
    p.drawPath(cardPath);

    const QRect titleBar(thumb.x(), thumb.y(), thumb.width(), std::min(28, thumb.height()));
    QPainterPath titleBarPath;
    titleBarPath.addRoundedRect(titleBar, 10, 10);
    p.fillPath(titleBarPath, hovered ? QColor(176, 92, 56, 175) : QColor(0, 0, 0, 145));

    QString appLabel = win.appName.isEmpty() ? win.title : win.appName;
    if (appLabel.isEmpty()) appLabel = QStringLiteral("Window");
    if (appLabel.length() > 30) appLabel = appLabel.left(27) + QStringLiteral("…");

    QFont appFont;
    appFont.setPointSizeF(9.5);
    appFont.setBold(true);
    p.setFont(appFont);
    p.setPen(QColor(255, 255, 255, 236));
    p.drawText(titleBar.adjusted(8, 0, -8, 0), Qt::AlignLeft | Qt::AlignVCenter, appLabel);

    if (!hasThumbnail) {
        QString badge = appLabel.left(1).toUpper();
        const int badgeSize = std::max(16, std::min(56, std::min(thumb.width(), thumb.height()) / 3));
        const QRect badgeRect(thumb.center().x() - badgeSize / 2,
                              thumb.center().y() - badgeSize / 2 + 6,
                              badgeSize,
                              badgeSize);
        p.setBrush(hovered ? QColor(176, 92, 56, 220) : QColor(255, 255, 255, 52));
        p.setPen(Qt::NoPen);
        p.drawEllipse(badgeRect);

        QFont badgeFont;
        badgeFont.setPointSizeF(std::max(9.0, badgeSize * 0.38));
        badgeFont.setBold(true);
        p.setFont(badgeFont);
        p.setPen(QColor(255, 255, 255, 245));
        p.drawText(badgeRect, Qt::AlignCenter, badge);
    }

    QString windowTitle = win.title.isEmpty() ? appLabel : win.title;
    if (windowTitle.length() > 36) windowTitle = windowTitle.left(33) + QStringLiteral("…");

    QFont titleFontCard;
    titleFontCard.setPointSizeF(9.0);
    titleFontCard.setBold(false);
    p.setFont(titleFontCard);
    p.setPen(QColor(255, 255, 255, hovered ? 222 : 200));
    const QRect footerTextRect = thumb.adjusted(10, thumb.height() - 30, -10, -8);
    p.drawText(footerTextRect, Qt::AlignLeft | Qt::AlignVCenter, windowTitle);

    const QString dims = QStringLiteral("%1×%2")
                           .arg(std::max(1, win.rect.width()))
                           .arg(std::max(1, win.rect.height()));
    QFont dimsFont;
    dimsFont.setPointSizeF(8.3);
    dimsFont.setBold(false);
    p.setFont(dimsFont);
    p.setPen(QColor(255, 255, 255, hovered ? 210 : 176));
    p.drawText(footerTextRect, Qt::AlignRight | Qt::AlignVCenter, dims);

    if (hovered) {
        const QRect tickRect(thumb.right() - 24, thumb.y() + 6, 16, 16);
        p.setBrush(QColor(176, 92, 56, 234));
        p.setPen(Qt::NoPen);
        p.drawEllipse(tickRect);
        QFont tickFont;
        tickFont.setPointSizeF(9.0);
        tickFont.setBold(true);
        p.setFont(tickFont);
        p.setPen(QColor(255, 255, 255, 255));
        p.drawText(tickRect, Qt::AlignCenter, QStringLiteral("✓"));
    }
}

static QRect pickerDisplayArea(int width, int height)
{
    // Keep room for title at the top and hint + toolbar at the bottom.
    const int topInset = 84;
    const int bottomInset = 120;
    const int usableHeight = std::max(120, height - topInset - bottomInset);
    return QRect(0, topInset, width, usableHeight);
}

static QRect thumbnailDirtyRect(const QRect& thumb)
{
    return thumb.adjusted(-10, -10, 10, 10);
}

static QRect toolbarDirtyRect(const QRectF& cell)
{
    return cell.toAlignedRect().adjusted(-8, -8, 8, 8);
}

void WindowPickerOverlay::recomputeThumbnailLayout()
{
    if (width() <= 0 || height() <= 0 || m_windows.isEmpty()) {
        m_thumbnailRects.clear();
        m_scaledThumbnails.clear();
        return;
    }

    m_thumbnailRects = computeLayout(m_windows, pickerDisplayArea(width(), height()), 24);
    m_scaledThumbnails.clear();
    m_scaledThumbnails.reserve(m_thumbnailRects.size());
    for (int i = 0; i < m_thumbnailRects.size() && i < m_windows.size(); ++i) {
        if (m_windows[i].icon.isNull()) {
            m_scaledThumbnails.append(QPixmap());
            continue;
        }
        m_scaledThumbnails.append(m_windows[i].icon.scaled(
            m_thumbnailRects[i].size(),
            Qt::KeepAspectRatioByExpanding,
            Qt::SmoothTransformation));
    }
}

// ── WindowPickerOverlay ───────────────────────────────────────────────────────

WindowPickerOverlay::WindowPickerOverlay(QWidget* parent)
    : QWidget(parent)
{
    setWindowFlags(windowPickerWindowFlags());
    setAttribute(Qt::WA_TranslucentBackground, true);
    setAttribute(Qt::WA_StaticContents, true);
    setMouseTracking(true);
    setCursor(Qt::CrossCursor);

    QScreen* screen = QGuiApplication::primaryScreen();
    QRect geo = screen->geometry();
    setGeometry(geo);

    m_background = loadAndBlurWallpaper(width(), height());

    // Fetch windows from GNOME Shell extension (real positions)
    m_windows = fetchWindowsFromExtension();
    recomputeThumbnailLayout();

    // Recompute once after the first event loop cycle to ensure we use the
    // final compositor size on Wayland.
    QTimer::singleShot(0, this, [this]() {
        recomputeThumbnailLayout();
        update();
    });

    focusAndRaiseOverlay();
}

void WindowPickerOverlay::focusAndRaiseOverlay()
{
    show();
    raise();
    if (!qEnvironmentVariableIsSet("WAYLAND_DISPLAY")) {
        activateWindow();
    }
    setFocus(Qt::ActiveWindowFocusReason);
    if (QWidget::keyboardGrabber() != this) {
        grabKeyboard();
    }
    if (QWidget::mouseGrabber() != this) {
        grabMouse();
    }
}

void WindowPickerOverlay::resizeEvent(QResizeEvent* event)
{
    QWidget::resizeEvent(event);
    m_background = loadAndBlurWallpaper(width(), height());
    recomputeThumbnailLayout();
}

void WindowPickerOverlay::paintEvent(QPaintEvent* event)
{
    if (m_thumbnailRects.size() != m_windows.size()) {
        recomputeThumbnailLayout();
    }

    QPainter p(this);
    if (event) {
        p.setClipRegion(event->region());
    }
    p.setRenderHint(QPainter::Antialiasing);

    // Blurred wallpaper background
    if (!m_background.isNull())
        p.drawPixmap(0, 0, m_background);
    else
        p.fillRect(rect(), QColor(18, 18, 24));

    // Title
    QFont titleFont;
    titleFont.setPointSizeF(16.0);
    titleFont.setBold(true);
    p.setFont(titleFont);
    p.setPen(QColor(255, 255, 255, 230));
    p.drawText(QRect(0, 18, width(), 50), Qt::AlignCenter,
               QStringLiteral("Click a window to capture"));

    if (m_windows.isEmpty()) {
        QFont f; f.setPointSizeF(13.0); p.setFont(f);
        p.setPen(QColor(255, 255, 255, 160));
        p.drawText(rect(), Qt::AlignCenter,
            QStringLiteral("No windows found.\nEnable the ApexShot extension."));
        return;
    }

    if (m_thumbnailRects.isEmpty()) {
        QFont f; f.setPointSizeF(13.0); p.setFont(f);
        p.setPen(QColor(255, 255, 255, 180));
        p.drawText(rect(), Qt::AlignCenter,
            QStringLiteral("Window list found, but layout is unavailable."));
        return;
    }

    // Draw all cards in base state first.
    for (int i = 0; i < m_thumbnailRects.size(); ++i) {
        const QRect& thumb = m_thumbnailRects[i];
        const AppWindowInfo& win = m_windows[i];
        if (p.clipRegion().intersects(thumbnailDirtyRect(thumb))) {
            const QPixmap scaled =
                (i < m_scaledThumbnails.size()) ? m_scaledThumbnails[i] : QPixmap();
            drawWindowCard(p, thumb, win, scaled, false);
        }
    }

    // Draw exactly one hovered card as a second pass.
    if (m_hoveredIdx >= 0 && m_hoveredIdx < m_thumbnailRects.size() && m_hoveredIdx < m_windows.size()) {
        const QRect hoveredRect = thumbnailDirtyRect(m_thumbnailRects[m_hoveredIdx]);
        if (p.clipRegion().intersects(hoveredRect)) {
            const QPixmap scaled =
                (m_hoveredIdx < m_scaledThumbnails.size()) ? m_scaledThumbnails[m_hoveredIdx]
                                                           : QPixmap();
            drawWindowCard(p, m_thumbnailRects[m_hoveredIdx], m_windows[m_hoveredIdx], scaled, true);
        }
    }

    // Bottom hint
    QFont hintFont;
    hintFont.setPointSizeF(10.5);
    p.setFont(hintFont);
    QString hint = QStringLiteral("ESC to cancel");
    QFontMetrics hfm(hintFont);
    int hw = hfm.horizontalAdvance(hint) + 28;
    int hx = (width() - hw) / 2;
    int hy = height() - 40;
    QPainterPath hpill;
    hpill.addRoundedRect(QRectF(hx, hy, hw, 28), 10, 10);
    p.fillPath(hpill, QColor(0, 0, 0, 140));
    p.setPen(QColor(255, 255, 255, 160));
    p.drawText(QRect(hx, hy, hw, 28), Qt::AlignCenter, hint);

    // Draw toolbar at bottom center (no size panel)
    drawToolbar(p);
}

void WindowPickerOverlay::mouseMoveEvent(QMouseEvent* event)
{
    const QPoint pos = event->pos();

    // Check toolbar hover
    int newToolHover = -1;
    for (int i = 0; i < TB_NUM; ++i) {
        if (toolbarItemRect(i).contains(pos)) { newToolHover = i; break; }
    }

    // Check thumbnail hover
    int newHover = -1;
    if (newToolHover < 0) {
        for (int i = static_cast<int>(m_thumbnailRects.size()) - 1; i >= 0; --i) {
            if (m_thumbnailRects[i].contains(pos)) { newHover = i; break; }
        }
    }

    if (newHover != m_hoveredIdx || newToolHover != m_hoveredTool) {
        const int previousHover = m_hoveredIdx;
        const int previousToolHover = m_hoveredTool;
        m_hoveredIdx  = newHover;
        m_hoveredTool = newToolHover;
        QRegion dirty;
        if (previousHover >= 0 && previousHover < m_thumbnailRects.size()) {
            dirty += thumbnailDirtyRect(m_thumbnailRects[previousHover]);
        }
        if (newHover >= 0 && newHover < m_thumbnailRects.size()) {
            dirty += thumbnailDirtyRect(m_thumbnailRects[newHover]);
        }
        if (previousToolHover >= 0 && previousToolHover < TB_NUM) {
            dirty += toolbarDirtyRect(toolbarItemRect(previousToolHover));
        }
        if (newToolHover >= 0 && newToolHover < TB_NUM) {
            dirty += toolbarDirtyRect(toolbarItemRect(newToolHover));
        }
        if (dirty.isEmpty()) {
            update();
        } else {
            update(dirty);
        }
    }
}

void WindowPickerOverlay::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) return;
    const QPoint pos = event->pos();

    // Check toolbar click first
    for (int i = 0; i < TB_NUM; ++i) {
        if (toolbarItemRect(i).contains(pos)) {
            if (i == 0) {
                // Area tool — exit window picker, go to area mode
                releaseKeyboard();
                hide();
                QApplication::exit(4); // code 4 = switch to area mode
            } else if (i == 1) {
                // Window — already in window mode, do nothing
            }
            return;
        }
    }

    // Thumbnail click — select window (do hit-testing directly so selection
    // still works even before any mouse-move hover event fires).
    for (int i = 0; i < m_thumbnailRects.size() && i < m_windows.size(); ++i) {
        if (m_thumbnailRects[i].contains(pos)) {
            m_selectedWindow = m_windows[i];
            m_selected = true;
            std::fprintf(stderr, "[WindowPicker] Selected: '%s' (id=%llu)\n",
                m_selectedWindow.title.toLocal8Bit().constData(),
                (unsigned long long)m_selectedWindow.xid);
            releaseKeyboard();
            hide();
            QApplication::exit(3);
            return;
        }
    }
}

void WindowPickerOverlay::keyPressEvent(QKeyEvent* event)
{
    if (event->key() == Qt::Key_Escape) {
        releaseKeyboard();
        hide();
        QApplication::exit(1);
    }
}
