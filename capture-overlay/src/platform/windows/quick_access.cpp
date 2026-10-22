#include "platform/windows/quick_access.h"

#include "CaptureModeToolbar.h"
#include "CaptureOverlay_DrawingPrimitives_p.h"
#include "platform/platform.h"
#include "platform/recording.h"
#include "platform/windows/ocr.h"
#include "platform/windows/qr.h"
#include "platform/windows/recording_controls.h"

#include <QAction>
#include <QApplication>
#include <QClipboard>
#include <QCursor>
#include <QDateTime>
#include <QDesktopServices>
#include <QDir>
#include <QFileInfo>
#include <QFile>
#include <QFont>
#include <QFontDatabase>
#include <QFontMetrics>
#include <QGuiApplication>
#include <QHBoxLayout>
#include <QIcon>
#include <QKeyEvent>
#include <QLabel>
#include <QMenu>
#include <QMessageBox>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QRegion>
#include <QScreen>
#include <QStandardPaths>
#include <QSystemTrayIcon>
#include <QTimer>
#include <QToolButton>
#include <QUrl>
#include <QVBoxLayout>
#include <QWidget>

#include <algorithm>
#include <cmath>
#include <functional>
#include <utility>

namespace {

// ---------------------------------------------------------------- tokens ---
// Verbatim from capture-overlay/src/CaptureModeToolbar.cpp,
// CaptureOverlay_p.h, CaptureOverlay_Paint.cpp and CaptureOverlay_Layout.cpp.
const QColor kPanel(25, 25, 28, 246);
const QColor kAccent(255, 102, 0);
const QColor kText(245, 245, 247);
const QColor kCard(18, 18, 18, 235);
const QColor kRim(255, 255, 255, 54);
const QColor kDim(0, 0, 0, 140);
const QColor kDropdownAccent(176, 92, 56);

constexpr double kHandleLength = 20.0;
constexpr double kHandleThickness = 2.5;
constexpr double kTopBarY = 16.0;
constexpr double kTopBarH = 44.0;
constexpr double kTopBarAspectH = 28.0;
constexpr double kTopBarBtnSize = 32.0;
constexpr double kFeaturePanelMargin = 16.0;
constexpr double kTopBarRadius = 14.0;

const char* kPillLabels[4] = {"Free", "16:9", "4:3", "1:1"};
const double kAspectRatios[4] = {0.0, 16.0 / 9.0, 4.0 / 3.0, 1.0};

// Crop menu rows, verbatim from CaptureOverlay_ToolbarDrawing.cpp.
enum CropRow {
    CropRowReset = 0,
    CropRowFree = 1,
    CropRow1_1 = 2,
    CropRow2_1 = 3,
    CropRow3_2 = 4,
    CropRow4_3 = 5,
    CropRow9_16 = 6,
    CropRow16_9 = 7,
    CropRow16_10 = 8,
    CropRow21_9 = 9,
    CropRowSnap = 10,
    CropRowCount = 11
};

struct CropOption {
    const char* label;
    double ratio;
};

constexpr CropOption kCropOptions[CropRowCount] = {
    {"Reset selection", 0.0},
    {"Free", 0.0},
    {"1:1", 1.0},
    {"2:1", 2.0},
    {"3:2", 3.0 / 2.0},
    {"4:3", 4.0 / 3.0},
    {"9:16", 9.0 / 16.0},
    {"16:9", 16.0 / 9.0},
    {"16:10", 16.0 / 10.0},
    {"21:9", 21.0 / 9.0},
    {"Snap to ratios", 0.0},
};

constexpr double kCropMenuW = 196.0;
constexpr double kCropItemH = 32.0;

bool ratiosEqual(double a, double b)
{
    return std::abs(a - b) < 0.01;
}

// ---------------------------------------------------------------- fonts -----
QString uiFamily()
{
    static const QString family = [] {
        const QStringList families = QFontDatabase().families();
        for (const QString& candidate : {QStringLiteral("Inter"), QStringLiteral("Segoe UI")}) {
            if (families.contains(candidate))
                return candidate;
        }
        return QStringLiteral("Segoe UI");
    }();
    return family;
}

void registerBundledFonts()
{
    const QString fontDir = QStringLiteral(APEXSHOT_FONT_DIR);
    int loaded = 0;
    for (const QString& file : {QStringLiteral("Inter-Regular.ttf"),
                                QStringLiteral("Inter-Medium.ttf"),
                                QStringLiteral("Inter-SemiBold.ttf"),
                                QStringLiteral("Inter-Bold.ttf")}) {
        if (QFontDatabase::addApplicationFont(QDir(fontDir).filePath(file)) >= 0)
            ++loaded;
    }

    QFile log(QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-quickaccess.log")));
    if (log.open(QIODevice::Append | QIODevice::WriteOnly)) {
        log.write(QStringLiteral("fonts: %1/4 from %2; Inter present=%3; family=%4\n")
                      .arg(loaded)
                      .arg(fontDir)
                      .arg(QFontDatabase().families().contains(QStringLiteral("Inter")) ? 1 : 0)
                      .arg(uiFamily())
                      .toUtf8());
    }
}

QFont uiFont(int pixelSize, bool bold = false)
{
    QFont font(uiFamily());
    font.setPixelSize(pixelSize);
    font.setWeight(bold ? QFont::DemiBold : QFont::Normal);
    return font;
}

void debugLog(const QString& text)
{
    QFile log(QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-quickaccess.log")));
    if (log.open(QIODevice::Append | QIODevice::WriteOnly)) {
        log.write(text.toUtf8());
        log.write("\n");
    }
}

QString saveCapture(const QImage& image)
{
    const QString name = QStringLiteral("apexshot-quick-%1.png")
                             .arg(QDateTime::currentDateTime().toString(
                                 QStringLiteral("yyyyMMdd-HHmmss-zzz")));
    const QString path = QDir(QDir::tempPath()).filePath(name);
    return image.save(path, "PNG") ? path : QString();
}

QString videoOutputPath()
{
    QString directory = QStandardPaths::writableLocation(QStandardPaths::MoviesLocation);
    if (directory.isEmpty())
        directory = QDir::home().filePath(QStringLiteral("Videos"));
    directory = QDir(directory).filePath(QStringLiteral("ApexShotRec"));

    const QString name = QStringLiteral("ApexShot Recording %1.mp4")
                             .arg(QDateTime::currentDateTime().toString(
                                 QStringLiteral("yyyy-MM-dd HH-mm-ss")));
    return QDir(directory).filePath(name);
}

QIcon appIcon()
{
    QPixmap pixmap(64, 64);
    pixmap.fill(Qt::transparent);

    QPainter painter(&pixmap);
    painter.setRenderHint(QPainter::Antialiasing);
    painter.setPen(Qt::NoPen);
    painter.setBrush(kAccent);
    painter.drawRoundedRect(QRectF(2, 2, 60, 60), 14, 14);

    painter.setBrush(Qt::NoBrush);
    painter.setPen(QPen(Qt::white, 4.0, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
    painter.drawRoundedRect(QRectF(18, 22, 28, 20), 4, 4);
    painter.drawEllipse(QPointF(37, 30), 3.0, 3.0);
    painter.drawLine(QPointF(22, 38), QPointF(29, 31));
    painter.drawLine(QPointF(29, 31), QPointF(34, 36));
    painter.end();

    return QIcon(pixmap);
}

void paintPanel(QPainter& painter, const QRectF& rect, const QColor& fill, qreal radius)
{
    painter.setRenderHint(QPainter::Antialiasing, true);

    QPainterPath shadow;
    shadow.addRoundedRect(rect.translated(0, 3), radius, radius);
    painter.fillPath(shadow, QColor(0, 0, 0, 105));

    QPainterPath body;
    body.addRoundedRect(rect, radius, radius);
    painter.fillPath(body, fill);
    painter.setPen(QPen(kRim, 1.2));
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(body);
}

QString sizeLabel(const QImage& image, const QString& path)
{
    const qint64 bytes = path.isEmpty() ? 0 : QFileInfo(path).size();
    return QStringLiteral("%1 \u00D7 %2   -   %3 KB")
        .arg(image.width())
        .arg(image.height())
        .arg(bytes / 1024);
}

// ------------------------------------------------------------- top bar ------
struct TopBarLayout {
    QRectF bar;
    QRectF label;
    QRectF pills[4];
    QRectF buttons[2];
    bool valid = false;
};

TopBarLayout topBarLayout(double screenW)
{
    // Mirrors CaptureOverlay::computeTopBarLayout, but the label column is
    // measured instead of fixed: Inter 4.1 is a few pixels wider than the
    // Inter builds the Linux layout was tuned against, and a fixed 108px rect
    // clips "Draw an area".
    constexpr double padX = 14.0;
    constexpr double minLabelW = 108.0;
    constexpr double sepGap = 12.0;
    constexpr double sepW = 1.0;
    constexpr double pillGap = 6.0;
    constexpr double btnGap = 6.0;
    constexpr double pillW[4] = {52.0, 52.0, 48.0, 44.0};

    static const QFont labelFont = [] {
        QFont font(uiFamily());
        font.setPointSizeF(10.5);
        font.setBold(true);
        return font;
    }();
    static const double labelW = std::max(
        minLabelW, QFontMetrics(labelFont).horizontalAdvance(QStringLiteral("Draw an area")) + 10.0);

    const double pillsW = pillW[0] + pillW[1] + pillW[2] + pillW[3] + pillGap * 3;
    const double btnsW = kTopBarBtnSize * 2 + btnGap;
    const double totalW = padX * 2 + labelW + sepGap * 2 + sepW + sepGap * 2 + sepW
        + pillsW + btnsW;

    TopBarLayout layout;
    if (totalW > screenW - kFeaturePanelMargin * 2)
        return layout;

    const double barX = std::max(kFeaturePanelMargin, (screenW - totalW) / 2.0);
    layout.bar = QRectF(barX, kTopBarY, totalW, kTopBarH);

    double cx = barX + padX;
    layout.label = QRectF(cx, kTopBarY, labelW, kTopBarH);
    cx += labelW + sepGap + sepW + sepGap;

    const double pillY = kTopBarY + (kTopBarH - kTopBarAspectH) / 2.0;
    for (int i = 0; i < 4; ++i) {
        layout.pills[i] = QRectF(cx, pillY, pillW[i], kTopBarAspectH);
        cx += pillW[i] + pillGap;
    }

    cx -= pillGap;
    cx += sepGap + sepW + sepGap;
    const double btnY = kTopBarY + (kTopBarH - kTopBarBtnSize) / 2.0;
    for (int i = 0; i < 2; ++i) {
        layout.buttons[i] = QRectF(cx, btnY, kTopBarBtnSize, kTopBarBtnSize);
        cx += kTopBarBtnSize + btnGap;
    }

    layout.valid = true;
    return layout;
}

// --------------------------------------------------------- list popup -------
/// Dropdown copied from CaptureOverlay::drawDropdownPopup: 34px rows,
/// (20,20,20,250) body, radius 8, hover (255,255,255,20), accent (176,92,56).
class ListPopup : public QWidget
{
public:
    ListPopup(QStringList items,
              const QRect& anchorGlobal,
              int width,
              std::function<void(int)> onPick)
        : m_items(std::move(items))
        , m_onPick(std::move(onPick))
    {
        setWindowFlags(Qt::Popup | Qt::FramelessWindowHint | Qt::NoDropShadowWindowHint);
        setAttribute(Qt::WA_TranslucentBackground, true);
        setMouseTracking(true);

        const int itemH = 34;
        const int menuH = m_items.size() * itemH + 10;
        int menuX = anchorGlobal.right() - width;
        int menuY = anchorGlobal.bottom() + 4;

        if (QScreen* screen = QGuiApplication::screenAt(anchorGlobal.center())) {
            const QRect available = screen->availableGeometry();
            if (menuX + width > available.right() - 10)
                menuX = available.right() - width - 10;
            if (menuY + menuH > available.bottom() - 10)
                menuY = anchorGlobal.top() - menuH - 4;
        }

        setGeometry(menuX, menuY, width, menuH);
        setFixedSize(width, menuH);
    }

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.setRenderHint(QPainter::Antialiasing);

        painter.setPen(QPen(QColor(255, 255, 255, 31), 1));
        painter.setBrush(QColor(20, 20, 20, 250));
        painter.drawRoundedRect(QRectF(rect()).adjusted(0.5, 0.5, -0.5, -0.5), 8, 8);

        painter.setFont(QFont(uiFamily(), 10));
        for (int i = 0; i < m_items.size(); ++i) {
            const QRectF item(5, 5 + i * 34.0, width() - 10, 34);
            if (i == m_hovered) {
                painter.setPen(Qt::NoPen);
                painter.setBrush(QColor(255, 255, 255, 20));
                painter.drawRoundedRect(item, 7, 7);
                painter.setPen(Qt::NoPen);
                painter.setBrush(QColor(kDropdownAccent.red(), kDropdownAccent.green(),
                                        kDropdownAccent.blue(), 28));
                painter.drawRoundedRect(item, 7, 7);
            }

            painter.setPen(i == m_hovered ? QColor(255, 236, 220) : Qt::white);
            QFont font(uiFamily(), 10);
            font.setBold(i == m_hovered);
            painter.setFont(font);
            painter.drawText(item.adjusted(10, 0, -10, 0),
                             Qt::AlignLeft | Qt::AlignVCenter,
                             m_items.at(i));
        }
    }

    void mouseMoveEvent(QMouseEvent* event) override
    {
        const int hovered = (event->pos().y() - 5) / 34;
        const int bounded = (hovered >= 0 && hovered < m_items.size()) ? hovered : -1;
        if (bounded != m_hovered) {
            m_hovered = bounded;
            setCursor(bounded >= 0 ? Qt::PointingHandCursor : Qt::ArrowCursor);
            update();
        }
    }

    void mouseReleaseEvent(QMouseEvent* event) override
    {
        const int index = (event->pos().y() - 5) / 34;
        if (index >= 0 && index < m_items.size() && m_onPick) {
            auto callback = m_onPick;
            close();
            callback(index);
            return;
        }
        close();
    }

    void keyPressEvent(QKeyEvent* event) override
    {
        if (event->key() == Qt::Key_Escape)
            close();
    }

private:
    QStringList m_items;
    std::function<void(int)> m_onPick;
    int m_hovered = -1;
};

// ------------------------------------------------------- area selector ------
/// Frozen-frame area selection, painted with the overlay's own rules.
class AreaSelector : public QWidget
{
public:
    /// The image is the cropped selection; the rectangle is that selection in
    /// frozen-image pixels, which the recorder maps back to desktop pixels.
    using Done = std::function<void(const QImage&, const QRect&)>;
    enum class Mode { Area, Crosshair };

    AreaSelector(QImage frozen, Mode mode, Done done)
        : m_frozen(std::move(frozen))
        , m_mode(mode)
        , m_done(std::move(done))
    {
        setWindowFlags(Qt::Window | Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint);
        setCursor(Qt::CrossCursor);
        setAttribute(Qt::WA_DeleteOnClose, true);
        setMouseTracking(true);
        setFocusPolicy(Qt::StrongFocus);
    }

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.setRenderHint(QPainter::SmoothPixmapTransform, true);
        painter.drawImage(rect(), m_frozen);

        // Crosshair mode: no dim, no chrome, just the frozen frame and a live
        // coordinate bubble. Release captures immediately.
        if (m_mode == Mode::Crosshair) {
            if (m_hasSelection) {
                const QRect selection = m_selection.normalized();
                painter.setPen(QPen(QColor(255, 102, 0, 240), 2.0));
                painter.setBrush(Qt::NoBrush);
                painter.drawRect(selection.adjusted(0, 0, -1, -1));
            }
            drawCrosshairBubble(painter);
            return;
        }

        const QRect sel = m_selection.normalized();

        if (m_hasSelection) {
            // Dim outside the selection, exactly as CaptureOverlay_Paint does.
            const QColor dim(0, 0, 0, 140);
            if (sel.y() > 0)
                painter.fillRect(QRect(0, 0, width(), sel.y()), dim);
            if (sel.bottom() < height() - 1)
                painter.fillRect(QRect(0, sel.bottom() + 1, width(), height() - sel.bottom() - 1), dim);
            if (sel.x() > 0)
                painter.fillRect(QRect(0, sel.y(), sel.x(), sel.height()), dim);
            if (sel.right() < width() - 1)
                painter.fillRect(QRect(sel.right() + 1, sel.y(), width() - sel.right() - 1, sel.height()), dim);

            painter.setPen(QPen(QColor(255, 102, 0, 240), 2.0));
            painter.setBrush(Qt::NoBrush);
            painter.drawRect(sel.adjusted(0, 0, -1, -1));

            // Corner brackets and edge midpoint ticks.
            painter.setBrush(Qt::NoBrush);
            painter.setPen(QPen(QColor(255, 102, 0, 245),
                                kHandleThickness,
                                Qt::SolidLine,
                                Qt::RoundCap,
                                Qt::RoundJoin));
            const double half = kHandleLength / 2.0;
            auto corner = [&](double x, double y, double dx, double dy) {
                QPainterPath path;
                path.moveTo(x, y + dy * half);
                path.lineTo(x, y);
                path.lineTo(x + dx * half, y);
                painter.drawPath(path);
            };
            corner(sel.left(), sel.top(), +1, +1);
            corner(sel.right(), sel.top(), -1, +1);
            corner(sel.left(), sel.bottom(), +1, -1);
            corner(sel.right(), sel.bottom(), -1, -1);

            const double midX = sel.left() + sel.width() / 2.0;
            const double midY = sel.top() + sel.height() / 2.0;
            painter.drawLine(QPointF(midX - half, sel.top()), QPointF(midX + half, sel.top()));
            painter.drawLine(QPointF(midX - half, sel.bottom()), QPointF(midX + half, sel.bottom()));
            painter.drawLine(QPointF(sel.left(), midY - half), QPointF(sel.left(), midY + half));
            painter.drawLine(QPointF(sel.right(), midY - half), QPointF(sel.right(), midY + half));

            drawSizeBubble(painter, sel);
        } else {
            painter.fillRect(rect(), QColor(0, 0, 0, 60));
        }

        drawTopBar(painter);
    }

    void mousePressEvent(QMouseEvent* event) override
    {
        if (event->button() != Qt::LeftButton) {
            finish(QImage());
            return;
        }

        if (m_mode == Mode::Crosshair) {
            m_origin = event->pos();
            m_selection = QRect(m_origin, QSize());
            m_hasSelection = false;
            m_dragging = true;
            update();
            return;
        }

        if (m_cropMenuOpen) {
            const int row = cropItemAt(event->pos());
            if (row >= 0) {
                applyCropRow(row);
                return;
            }
            m_cropMenuOpen = false;
            m_hoveredCropItem = -1;
            update();
            return;
        }

        const TopBarLayout layout = topBarLayout(width());
        if (layout.valid) {
            for (int i = 0; i < 4; ++i) {
                if (layout.pills[i].contains(event->pos())) {
                    m_ratio = kAspectRatios[i];
                    applyRatioToSelection();
                    update();
                    return;
                }
            }
            if (layout.buttons[0].contains(event->pos())) {
                m_cropMenuOpen = true;
                m_hoveredCropItem = -1;
                debugLog(QStringLiteral("crop menu opened"));
                update();
                return;
            }
            if (layout.buttons[1].contains(event->pos())) {
                finish(QImage());
                return;
            }
        }

        m_origin = event->pos();
        m_selection = QRect(m_origin, QSize());
        m_hasSelection = false;
        update();
    }

    void mouseMoveEvent(QMouseEvent* event) override
    {
        m_pointer = event->pos();

        if (m_mode == Mode::Crosshair) {
            if (m_dragging) {
                m_selection = QRect(m_origin, event->pos()).normalized();
                m_hasSelection = m_selection.width() > 1 && m_selection.height() > 1;
            }
            update();
            return;
        }

        if (m_cropMenuOpen) {
            const int row = cropItemAt(event->pos());
            if (row != m_hoveredCropItem) {
                m_hoveredCropItem = row;
                update();
            }
            return;
        }

        if (!(event->buttons() & Qt::LeftButton)) {
            update();
            return;
        }

        QRect rect = QRect(m_origin, event->pos()).normalized();
        if (m_ratio > 0.0) {
            const int w = rect.width();
            const int h = qRound(w / m_ratio);
            rect.setHeight(h);
        }
        m_selection = rect.intersected(this->rect());
        m_hasSelection = m_selection.width() > 1 && m_selection.height() > 1;
        update();
    }

    void mouseReleaseEvent(QMouseEvent* event) override
    {
        if (event->button() != Qt::LeftButton)
            return;

        if (m_mode == Mode::Crosshair) {
            if (!m_dragging)
                return;
            m_dragging = false;
            if (m_selection.width() < 8 || m_selection.height() < 8) {
                finish(QImage());
                return;
            }
            finish(croppedImage());
            return;
        }
        if (!m_hasSelection || m_selection.width() < 8 || m_selection.height() < 8) {
            m_hasSelection = false;
            update();
            return;
        }
        if (m_snapToRatios)
            snapToNearestRatio();
        finish(croppedImage());
    }

    void keyPressEvent(QKeyEvent* event) override
    {
        if (event->key() == Qt::Key_Escape)
            finish(QImage());
    }

private:
    void drawSizeBubble(QPainter& painter, const QRect& sel)
    {
        QFont font(uiFamily());
        font.setPixelSize(12);
        font.setWeight(QFont::Medium);
        painter.setFont(font);

        const QString label = QStringLiteral("%1 \u00D7 %2").arg(sel.width()).arg(sel.height());
        const QFontMetrics metrics(font);
        const QSize textSize = metrics.size(Qt::TextSingleLine, label);

        QRect bubble(sel.left(), qMax(4, sel.top() - textSize.height() - 12),
                     textSize.width() + 14, textSize.height() + 8);

        QPainterPath path;
        path.addRoundedRect(bubble, 6, 6);
        painter.fillPath(path, QColor(0, 0, 0, 180));
        painter.setPen(QPen(QColor(255, 255, 255, 40), 1.0));
        painter.setBrush(Qt::NoBrush);
        painter.drawPath(path);
        painter.setPen(QColor(255, 255, 255));
        painter.drawText(bubble, Qt::AlignCenter, label);
    }

    /// Crosshair bubble: "x, y" while idle, "W × H" once dragging. Geometry
    /// and styling follow crosshairBubbleRectForPoint.
    void drawCrosshairBubble(QPainter& painter)
    {
        QString label;
        if (m_dragging || m_hasSelection) {
            const QRect sel = m_selection.normalized();
            label = QStringLiteral("%1 \u00D7 %2").arg(sel.width()).arg(sel.height());
        } else {
            label = QStringLiteral("%1, %2").arg(m_pointer.x()).arg(m_pointer.y());
        }

        QFont font(uiFamily());
        font.setPixelSize(12);
        font.setWeight(QFont::Medium);
        painter.setFont(font);

        const QFontMetrics metrics(font);
        const QRect textRect = metrics.boundingRect(label);
        const int bw = textRect.width() + 20;
        const int bh = textRect.height() + 12;
        const QRect bubble(std::clamp(m_pointer.x() + 16, 8, width() - bw - 8),
                           std::clamp(m_pointer.y() + 16, 8, height() - bh - 8),
                           bw,
                           bh);

        QPainterPath path;
        path.addRoundedRect(bubble, 6, 6);
        painter.fillPath(path, QColor(0, 0, 0, 180));
        painter.setPen(QPen(QColor(255, 255, 255, 40), 1.0));
        painter.setBrush(Qt::NoBrush);
        painter.drawPath(path);
        painter.setPen(QColor(255, 255, 255));
        painter.drawText(bubble, Qt::AlignCenter, label);
    }

    void drawTopBar(QPainter& painter)
    {
        const TopBarLayout layout = topBarLayout(width());
        if (!layout.valid)
            return;

        painter.save();
        painter.setRenderHint(QPainter::Antialiasing);

        QPainterPath shadow;
        shadow.addRoundedRect(layout.bar.translated(0, 3), kTopBarRadius, kTopBarRadius);
        painter.fillPath(shadow, QColor(0, 0, 0, 105));

        QPainterPath body;
        body.addRoundedRect(layout.bar, kTopBarRadius, kTopBarRadius);
        painter.fillPath(body, kPanel);
        painter.setPen(QPen(QColor(255, 255, 255, 54), 1.2));
        painter.setBrush(Qt::NoBrush);
        painter.drawPath(body);

        QFont labelFont(uiFamily());
        labelFont.setPointSizeF(10.5);
        labelFont.setBold(true);
        painter.setFont(labelFont);
        painter.setPen(QColor(244, 244, 246, 245));
        painter.drawText(layout.label, Qt::AlignVCenter | Qt::AlignLeft,
                         QStringLiteral("Draw an area"));

        painter.setPen(QPen(QColor(255, 255, 255, 30), 1.0));
        const double sep1 = layout.label.right() + 12.0;
        const double sep2 = layout.pills[3].right() + 12.0;
        painter.drawLine(QPointF(sep1, layout.bar.top() + 10),
                         QPointF(sep1, layout.bar.bottom() - 10));
        painter.drawLine(QPointF(sep2, layout.bar.top() + 10),
                         QPointF(sep2, layout.bar.bottom() - 10));

        for (int i = 0; i < 4; ++i) {
            const QRectF pill = layout.pills[i];
            const bool active = std::abs(m_ratio - kAspectRatios[i]) < 0.001;
            const bool hovered = pill.contains(m_pointer);

            QPainterPath path;
            path.addRoundedRect(pill, 8, 8);
            if (active)
                painter.fillPath(path, QColor(255, 102, 0, 205));
            else if (hovered)
                painter.fillPath(path, QColor(255, 255, 255, 26));
            else
                painter.fillPath(path, QColor(255, 255, 255, 12));

            QFont font(uiFamily());
            font.setPointSizeF(9.5);
            font.setBold(active || hovered);
            painter.setFont(font);
            painter.setPen(active ? QColor(255, 255, 255) : QColor(232, 232, 236));
            painter.drawText(pill, Qt::AlignCenter, QString::fromLatin1(kPillLabels[i]));
        }

        // Crop glyph and cancel X.
        for (int i = 0; i < 2; ++i) {
            const QRectF button = layout.buttons[i];
            const bool hovered = button.contains(m_pointer);
            const bool cropActive = (i == 0 && (m_cropMenuOpen || m_ratio > 0.0));
            QPainterPath path;
            path.addRoundedRect(button, 8, 8);
            if (cropActive)
                painter.fillPath(path, QColor(255, 102, 0, 205));
            else if (hovered)
                painter.fillPath(path, QColor(255, 255, 255, 26));

            const double cx = button.center().x();
            const double cy = button.center().y();
            const QColor iconColor = cropActive ? QColor(255, 255, 255) : QColor(240, 240, 244);
            if (i == 0) {
                // The overlay's own crop glyph, rather than a hand-drawn one.
                drawToolbarIcon(painter, 10, cx, cy, iconColor);
            } else {
                painter.setPen(QPen(iconColor, 1.7, Qt::SolidLine, Qt::RoundCap,
                                    Qt::RoundJoin));
                painter.setBrush(Qt::NoBrush);
                painter.drawLine(QPointF(cx - 4.5, cy - 4.5), QPointF(cx + 4.5, cy + 4.5));
                painter.drawLine(QPointF(cx - 4.5, cy + 4.5), QPointF(cx + 4.5, cy - 4.5));
            }
        }

        drawCropMenu(painter);

        painter.restore();
    }

    /// Crop dropdown, copied from CaptureOverlay::drawTopBarCropMenu.
    void drawCropMenu(QPainter& painter)
    {
        if (!m_cropMenuOpen)
            return;

        const TopBarLayout layout = topBarLayout(width());
        if (!layout.valid)
            return;

        const QRectF menu = cropMenuRect(layout);
        const double menuX = menu.x();
        const double menuY = menu.y();
        const double menuW = menu.width();
        const double menuH = menu.height();

        QPainterPath shadow;
        shadow.addRoundedRect(QRectF(menuX, menuY + 3.0, menuW, menuH), 12, 12);
        painter.fillPath(shadow, QColor(0, 0, 0, 105));

        QPainterPath body;
        body.addRoundedRect(QRectF(menuX, menuY, menuW, menuH), 12, 12);
        painter.fillPath(body, kPanel);
        painter.setPen(QPen(QColor(255, 255, 255, 54), 1.2));
        painter.setBrush(Qt::NoBrush);
        painter.drawPath(body);

        for (int i = 0; i < CropRowCount; ++i) {
            const QRectF itemRect(menuX + 5.0, menuY + 5.0 + i * kCropItemH, menuW - 10.0, kCropItemH);
            const QRectF indicatorRect(itemRect.x() + 8.0, itemRect.y(), 18.0, itemRect.height());
            const QRectF labelRect(itemRect.x() + 30.0, itemRect.y(),
                                   itemRect.width() - 40.0, itemRect.height());

            if (i == m_hoveredCropItem) {
                QPainterPath hover;
                hover.addRoundedRect(itemRect, 7, 7);
                painter.fillPath(hover, QColor(255, 255, 255, 24));
                painter.setPen(QPen(QColor(255, 102, 0, 170), 1.0));
                painter.setBrush(Qt::NoBrush);
                painter.drawPath(hover);
            }

            bool checked = false;
            if (i == CropRowFree)
                checked = m_ratio <= 0.0;
            else if (i >= CropRow1_1 && i <= CropRow21_9)
                checked = ratiosEqual(m_ratio, kCropOptions[i].ratio);
            else if (i == CropRowSnap)
                checked = m_snapToRatios;

            if (checked && i != CropRowReset) {
                painter.setPen(QPen(QColor(255, 255, 255), 1.5));
                painter.setBrush(Qt::NoBrush);
                const double cy = indicatorRect.center().y();
                painter.drawLine(QPointF(indicatorRect.x() + 3.5, cy),
                                 QPointF(indicatorRect.x() + 6.5, cy + 3.0));
                painter.drawLine(QPointF(indicatorRect.x() + 6.5, cy + 3.0),
                                 QPointF(indicatorRect.x() + 12.5, cy - 4.0));
            }

            QFont itemFont(uiFamily());
            itemFont.setPointSizeF(10.0);
            itemFont.setBold(checked || i == CropRowReset);
            painter.setFont(itemFont);
            painter.setPen(QColor(242, 242, 244));
            painter.drawText(labelRect, Qt::AlignVCenter | Qt::AlignLeft,
                             QString::fromLatin1(kCropOptions[i].label));
        }

        painter.setPen(QPen(QColor(255, 255, 255, 26), 1.0));
        const double y1 = menuY + 5.0 + kCropItemH;
        painter.drawLine(QPointF(menuX + 12.0, y1), QPointF(menuX + menuW - 12.0, y1));
        const double y2 = menuY + 5.0 + CropRowSnap * kCropItemH;
        painter.drawLine(QPointF(menuX + 12.0, y2), QPointF(menuX + menuW - 12.0, y2));
    }

    QRectF cropMenuRect(const TopBarLayout& layout) const
    {
        const QRectF anchor = layout.buttons[0];
        const double menuH = CropRowCount * kCropItemH + 10.0;
        const double menuX = std::max(
            10.0, std::min(anchor.center().x() - kCropMenuW / 2.0, width() - kCropMenuW - 10.0));
        const double menuY = std::max(layout.bar.bottom() + 4.0, anchor.bottom() + 8.0);
        return QRectF(menuX, menuY, kCropMenuW, menuH);
    }

    int cropItemAt(const QPoint& point) const
    {
        const TopBarLayout layout = topBarLayout(width());
        if (!layout.valid)
            return -1;
        const QRectF menu = cropMenuRect(layout);
        if (!menu.contains(point))
            return -1;
        const int index = static_cast<int>((point.y() - menu.top() - 5.0) / kCropItemH);
        return (index >= 0 && index < CropRowCount) ? index : -1;
    }

    void applyCropRow(int row)
    {
        if (row == CropRowReset) {
            m_selection = QRect();
            m_hasSelection = false;
        } else if (row == CropRowSnap) {
            m_snapToRatios = !m_snapToRatios;
        } else if (row > CropRowSnap) {
            return;
        } else {
            m_ratio = kCropOptions[row].ratio;
            applyRatioToSelection();
        }
        m_cropMenuOpen = false;
        m_hoveredCropItem = -1;
        update();
    }

    void snapToNearestRatio()
    {
        if (!m_hasSelection)
            return;
        const double current = static_cast<double>(m_selection.width())
            / std::max(1, m_selection.height());
        double best = 0.0;
        double bestDelta = 1e9;
        for (int i = CropRow1_1; i <= CropRow21_9; ++i) {
            const double delta = std::abs(kCropOptions[i].ratio - current);
            if (delta < bestDelta) {
                bestDelta = delta;
                best = kCropOptions[i].ratio;
            }
        }
        if (best <= 0.0)
            return;
        m_ratio = best;
        applyRatioToSelection();
    }

    void applyRatioToSelection()
    {
        if (!m_hasSelection || m_ratio <= 0.0)
            return;
        QRect rect = m_selection.normalized();
        const int height = qRound(rect.width() / m_ratio);
        if (height > 0)
            rect.setHeight(height);
        m_selection = rect.intersected(this->rect());
        m_hasSelection = !m_selection.isEmpty();
    }

    QRect selectionInImage() const
    {
        if (width() <= 0 || height() <= 0)
            return {};
        const qreal scaleX = static_cast<qreal>(m_frozen.width()) / width();
        const qreal scaleY = static_cast<qreal>(m_frozen.height()) / height();
        return QRect(qRound(m_selection.x() * scaleX),
                     qRound(m_selection.y() * scaleY),
                     qRound(m_selection.width() * scaleX),
                     qRound(m_selection.height() * scaleY))
            .intersected(m_frozen.rect());
    }

    QImage croppedImage() const
    {
        const QRect inImage = selectionInImage();
        if (inImage.isEmpty())
            return {};
        return m_frozen.copy(inImage);
    }

    void finish(const QImage& result)
    {
        const QRect selection = selectionInImage();
        if (m_done)
            m_done(result, selection);
        close();
    }

    QImage m_frozen;
    Mode m_mode = Mode::Area;
    Done m_done;
    QPoint m_origin;
    QPoint m_pointer;
    QRect m_selection;
    bool m_hasSelection = false;
    bool m_dragging = false;
    double m_ratio = 0.0;
    bool m_cropMenuOpen = false;
    int m_hoveredCropItem = -1;
    bool m_snapToRatios = false;
};

// ------------------------------------------------------ countdown pill ------
/// Capture-delay countdown, painted like the overlay's pill and held above the
/// topmost window so it stays visible while other windows are opened.
class CountdownPill : public QWidget
{
public:
    CountdownPill(int seconds, std::function<void()> onFinished)
        : m_remaining(seconds)
        , m_onFinished(std::move(onFinished))
    {
        setWindowFlags(Qt::Tool | Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint);
        setAttribute(Qt::WA_TranslucentBackground, true);
        setAttribute(Qt::WA_ShowWithoutActivating, true);
        setAttribute(Qt::WA_DeleteOnClose, true);
        setFixedSize(112, 44);
        setCursor(Qt::PointingHandCursor);
        setToolTip(QStringLiteral("Click to cancel"));

        if (QScreen* screen = QGuiApplication::primaryScreen()) {
            const QRect work = screen->availableGeometry();
            move(work.center().x() - width() / 2, work.top() + 28);
        }

        m_timer.setInterval(1000);
        connect(&m_timer, &QTimer::timeout, this, [this] {
            if (--m_remaining > 0) {
                keepOnTop();
                update();
                return;
            }
            m_timer.stop();
            auto finished = m_onFinished;
            m_onFinished = nullptr;
            close();
            if (finished)
                finished();
        });
        m_timer.start();
    }

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.setRenderHint(QPainter::Antialiasing);

        painter.setPen(Qt::NoPen);
        painter.setBrush(m_hovered ? QColor(200, 60, 40, 242) : QColor(233, 84, 32, 235));
        painter.drawRoundedRect(QRectF(rect()), height() / 2.0, height() / 2.0);

        const double cx = 22.0;
        const double cy = height() / 2.0;
        painter.setPen(QPen(Qt::white, 2.2, Qt::SolidLine, Qt::RoundCap));
        painter.setBrush(Qt::NoBrush);
        painter.drawEllipse(QPointF(cx, cy), 11.0, 11.0);
        painter.drawLine(QPointF(cx, cy), QPointF(cx, cy - 5.5));
        painter.drawLine(QPointF(cx, cy), QPointF(cx + 5.0, cy + 2.0));

        QFont font(uiFamily());
        font.setBold(true);
        // 22pt in the Linux build fills most of the 44px pill; at 1:1 on
        // Windows that reads oversized, so the number sits at 16pt.
        font.setPointSizeF(m_hovered ? 13.0 : 16.0);
        painter.setFont(font);
        painter.setPen(Qt::white);

        // Centred in the space right of the clock face, and swapped for a
        // Cancel label on hover, as the overlay does.
        const QRectF textRect(40.0, 0.0, width() - 44.0, height());
        painter.drawText(textRect,
                         Qt::AlignCenter,
                         m_hovered ? QStringLiteral("Cancel")
                                   : QString::number(m_remaining));
    }

    void mousePressEvent(QMouseEvent*) override
    {
        m_timer.stop();
        m_onFinished = nullptr;
        close();
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

    void showEvent(QShowEvent*) override { keepOnTop(); }

private:
    void keepOnTop()
    {
        raise();
        Platform::keepTopmost(static_cast<quintptr>(winId()));
    }

    int m_remaining = 0;
    bool m_hovered = false;
    QTimer m_timer;
    std::function<void()> m_onFinished;
};

// --------------------------------------------------------- result card ------
class ResultCard : public QWidget
{
public:
    ResultCard(const QImage& image, const QString& path, const QString& ocrText,
               const QString& hintText)
    {
        setWindowFlags(Qt::Tool | Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint);
        setAttribute(Qt::WA_TranslucentBackground, true);
        setAttribute(Qt::WA_DeleteOnClose, true);

        auto* layout = new QVBoxLayout(this);
        layout->setContentsMargins(24, 22, 24, 24);
        layout->setSpacing(8);

        auto* thumb = new QLabel;
        thumb->setPixmap(QPixmap::fromImage(image).scaled(
            420, 240, Qt::KeepAspectRatio, Qt::SmoothTransformation));
        thumb->setAlignment(Qt::AlignCenter);
        layout->addWidget(thumb);

        auto* info = new QLabel(sizeLabel(image, path));
        info->setFont(uiFont(12));
        info->setStyleSheet(QStringLiteral("color:#a4a4ac;"));
        layout->addWidget(info);

        auto* hint = new QLabel(hintText);
        hint->setFont(uiFont(12, true));
        hint->setStyleSheet(QStringLiteral("color:#7fd18b;"));
        layout->addWidget(hint);

        if (!ocrText.isEmpty()) {
            auto* text = new QLabel(ocrText);
            text->setWordWrap(true);
            text->setFont(uiFont(12));
            text->setTextInteractionFlags(Qt::TextSelectableByMouse);
            text->setMaximumHeight(140);
            text->setStyleSheet(QStringLiteral(
                "color:#e8e8ec; background:rgba(255,255,255,14); "
                "border-radius:8px; padding:8px;"));
            layout->addWidget(text);
        }

        auto* row = new QHBoxLayout;
        row->setSpacing(8);

        auto* open = new QToolButton;
        open->setObjectName(QStringLiteral("primary"));
        open->setText(QStringLiteral("Open"));
        open->setCursor(Qt::PointingHandCursor);
        open->setFont(uiFont(13, true));
        connect(open, &QToolButton::clicked, this, [path] {
            QDesktopServices::openUrl(QUrl::fromLocalFile(path));
        });
        row->addWidget(open);

        auto* folder = new QToolButton;
        folder->setText(QStringLiteral("Show in folder"));
        folder->setCursor(Qt::PointingHandCursor);
        folder->setFont(uiFont(13, true));
        connect(folder, &QToolButton::clicked, this, [path] {
            QDesktopServices::openUrl(QUrl::fromLocalFile(QFileInfo(path).absolutePath()));
        });
        row->addWidget(folder);

        row->addStretch();

        auto* dismiss = new QToolButton;
        dismiss->setObjectName(QStringLiteral("danger"));
        dismiss->setText(QStringLiteral("Dismiss"));
        dismiss->setCursor(Qt::PointingHandCursor);
        dismiss->setFont(uiFont(13, true));
        connect(dismiss, &QToolButton::clicked, this, &QWidget::close);
        row->addWidget(dismiss);

        layout->addLayout(row);

        setStyleSheet(QStringLiteral(
            "QToolButton { color:#f5f5f7; background:transparent; border:none; "
            "border-radius:10px; padding:7px 16px; } "
            "QToolButton#primary { background:#f3f4f6; color:#111111; } "
            "QToolButton#primary:hover { background:#ffffff; } "
            "QToolButton#danger { color:#ffffff; background:rgba(232,17,35,190); "
            "border-radius:999px; } "
            "QToolButton#danger:hover { background:#e81123; }"));

        adjustSize();

        if (QScreen* screen = QGuiApplication::primaryScreen()) {
            const QRect work = screen->availableGeometry();
            move(work.right() - width() - 20, work.top() + 20);
        }

        QTimer::singleShot(15000, this, &QWidget::close);
    }

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        paintPanel(painter, QRectF(rect()).adjusted(0.5, 0.5, -0.5, -0.5), kCard, 16);
    }
};

// --------------------------------------------------------------- shell ------
struct CaptureOptions {
    int timerSeconds = 0;
    bool ocr = false;
    bool recording = false;
    bool microphone = false;
    bool speaker = false;
};

class Shell : public QObject
{
public:
    void openMenu()
    {
        if (m_menuOpen)
            return;
        m_menuOpen = true;

        QString recordingError;
        const bool recordingAvailable = Platform::RecordingSession::available(&recordingError);
        // Quick Access has no recording panel of its own, so the toolbar's
        // Area slot is how an area recording is chosen.
        const CaptureModeToolbar::Result result =
            CaptureModeToolbar::choose(nullptr, nullptr, recordingAvailable, true);

        m_menuOpen = false;

        CaptureOptions options;
        options.timerSeconds = result.timerSeconds;
        options.ocr = result.ocr;
        options.recording = result.recording;
        options.microphone = result.microphone;
        options.speaker = result.speaker;
        if (options.recording && !recordingAvailable) {
            debugLog(QStringLiteral("recording unavailable: %1").arg(recordingError));
            return;
        }

        switch (result.action) {
        case CaptureModeToolbar::Action::Display:
            withCountdown(options, [this, options] { captureDisplay(options); });
            break;
        case CaptureModeToolbar::Action::Window:
            withCountdown(options, [this, options] { chooseWindow(options); });
            break;
        case CaptureModeToolbar::Action::Area:
            if (QGuiApplication::keyboardModifiers() & Qt::ShiftModifier) {
                withCountdown(options, [this, options] { captureCrosshair(options); });
            } else {
                withCountdown(options, [this, options] { captureArea(options); });
            }
            break;
        case CaptureModeToolbar::Action::Cancel:
            break;
        }
    }

    void demoArea()
    {
        Platform::CaptureResult frozen;
        QString error;
        const bool ok = Platform::captureVirtualDesktop(false, frozen, error);
        debugLog(QStringLiteral("demoArea: ok=%1 err=%2 image=%3x%4 monitors=%5")
                     .arg(ok ? 1 : 0)
                     .arg(error)
                     .arg(frozen.image.width())
                     .arg(frozen.image.height())
                     .arg(Platform::monitors().size()));
        if (ok)
            startArea(frozen, AreaSelector::Mode::Area, CaptureOptions{});
    }

    void demoCrosshair()
    {
        Platform::CaptureResult frozen;
        QString error;
        if (Platform::captureVirtualDesktop(false, frozen, error))
            startArea(frozen, AreaSelector::Mode::Crosshair, CaptureOptions{});
    }

    void demoOcr()
    {
        CaptureOptions options;
        options.ocr = true;
        captureDisplay(options);
    }

    void demoTimer()
    {
        CaptureOptions options;
        options.timerSeconds = 3;
        withCountdown(options, [this] { captureDisplay(CaptureOptions{}); });
    }

    void demoWindows()
    {
        chooseWindow(CaptureOptions{});
    }

    void recordScreen()
    {
        CaptureOptions options;
        options.recording = true;
        captureDisplay(options);
    }

    void recordArea()
    {
        CaptureOptions options;
        options.recording = true;
        captureArea(options);
    }

    void recordWindow()
    {
        CaptureOptions options;
        options.recording = true;
        chooseWindow(options);
    }

    void demoRecord()
    {
        CaptureOptions options;
        options.recording = true;
        captureDisplay(options);
        QTimer::singleShot(3000, this, [this] {
            if (m_activeSession)
                m_activeSession->stop();
        });
    }

    bool hasActiveRecording() const
    {
        return m_activeSession && m_activeSession->isActive();
    }

    /// Save the current recording before the app is allowed to quit.
    void stopActiveRecording()
    {
        if (m_activeSession)
            m_activeSession->stop();
    }

private:
    void withCountdown(const CaptureOptions& options, std::function<void()> action)
    {
        if (options.timerSeconds <= 0) {
            action();
            return;
        }
        auto* pill = new CountdownPill(options.timerSeconds, std::move(action));
        pill->show();
    }

    void captureDisplay(const CaptureOptions& options)
    {
        const Platform::MonitorInfo target = Platform::monitorAt(QCursor::pos());
        const QVector<Platform::MonitorInfo> all = Platform::monitors();
        int index = 0;
        for (int i = 0; i < all.size(); ++i) {
            if (all.at(i).id == target.id) {
                index = i;
                break;
            }
        }

        if (options.recording) {
            Platform::CaptureResult still;
            QString stillError;
            Platform::captureMonitor(index, false, still, stillError);
            startRecording(target.geometry,
                           still.image,
                           options,
                           Platform::RecordingRequest::Target::Screen);
            return;
        }

        Platform::CaptureResult capture;
        QString error;
        if (!Platform::captureMonitor(index, false, capture, error))
            return;
        present(capture.image, options);
    }

    void captureArea(const CaptureOptions& options)
    {
        Platform::CaptureResult frozen;
        QString error;
        if (!Platform::captureVirtualDesktop(false, frozen, error))
            return;
        startArea(frozen, AreaSelector::Mode::Area, options);
    }

    void captureCrosshair(const CaptureOptions& options)
    {
        Platform::CaptureResult frozen;
        QString error;
        if (!Platform::captureVirtualDesktop(false, frozen, error))
            return;
        startArea(frozen, AreaSelector::Mode::Crosshair, options);
    }

    void startArea(const Platform::CaptureResult& frozen, AreaSelector::Mode mode,
                   const CaptureOptions& options)
    {
        auto* selector =
            new AreaSelector(frozen.image, mode, [this, options, frozen](const QImage& cropped,
                                                                        const QRect& selection) {
                if (selection.isEmpty() || cropped.isNull())
                    return;
                if (options.recording) {
                    const QRect desktop(selection.topLeft() + frozen.geometry.topLeft(),
                                        selection.size());
                    startRecording(desktop, cropped, options);
                } else {
                    present(cropped, options);
                }
            });

        QRect virtualGeometry;
        for (const QScreen* screen : QGuiApplication::screens())
            virtualGeometry = virtualGeometry.united(screen->geometry());

        debugLog(QStringLiteral("startArea: screen=%1x%2 image=%3x%4")
                     .arg(virtualGeometry.width())
                     .arg(virtualGeometry.height())
                     .arg(frozen.image.width())
                     .arg(frozen.image.height()));

        selector->setGeometry(virtualGeometry);
        selector->show();
        selector->raise();
        selector->activateWindow();
        selector->setFocus();

        debugLog(QStringLiteral("startArea: visible=%1 geom=%2x%3 at %4,%5")
                     .arg(selector->isVisible() ? 1 : 0)
                     .arg(selector->width())
                     .arg(selector->height())
                     .arg(selector->x())
                     .arg(selector->y()));
    }

    void chooseWindow(const CaptureOptions& options)
    {
        const QVector<Platform::WindowInfo> windows = Platform::windows();
        debugLog(QStringLiteral("chooseWindow: enumerated=%1").arg(windows.size()));
        if (windows.isEmpty())
            return;

        QStringList items;
        QVector<Platform::WindowInfo> eligible;
        for (const Platform::WindowInfo& window : windows) {
            eligible.append(window);

            QString title = window.title;
            if (title.size() > 64)
                title = title.left(61) + QStringLiteral("...");
            if (window.minimized)
                title += QStringLiteral("  (minimized)");
            items.append(QStringLiteral("%1    %2 \u00D7 %3")
                             .arg(title)
                             .arg(window.geometry.width())
                             .arg(window.geometry.height()));
        }
        if (items.isEmpty())
            return;

        for (int i = 0; i < items.size() && i < 6; ++i)
            debugLog(QStringLiteral("  window[%1] %2").arg(i).arg(items.at(i)));

        const QRect anchor(QCursor::pos(), QSize(1, 1));
        auto* popup = new ListPopup(items, anchor, 460, [this, eligible, options](int index) {
            if (index < 0 || index >= eligible.size())
                return;
            const Platform::WindowInfo& window = eligible.at(index);
            Platform::CaptureResult capture;
            QString error;
            if (!Platform::captureWindow(window.id, false, capture, error))
                return;
            if (options.recording) {
                startRecording(window.geometry,
                               capture.image,
                               options,
                               Platform::RecordingRequest::Target::Window,
                               window.id);
            } else {
                present(capture.image, options);
            }
        });
        popup->show();
        popup->raise();
    }

    void startRecording(
        const QRect& region,
        const QImage& thumbnail,
        const CaptureOptions& options,
        Platform::RecordingRequest::Target target = Platform::RecordingRequest::Target::Area,
        quintptr windowId = 0)
    {
        if (region.isEmpty())
            return;
        if (m_activeSession && m_activeSession->isActive()) {
            debugLog(QStringLiteral("recording ignored: a session is already active"));
            return;
        }

        auto* session = new Platform::RecordingSession(this);
        m_activeSession = session;

        Platform::RecordingRequest request;
        request.target = target;
        request.region = region;
        request.windowId = windowId;
        request.outputPath = videoOutputPath();
        request.includeCursor = true;
        request.includeMicrophone = options.microphone;
        request.includeSpeaker = options.speaker;
        request.fps = 30;
        request.crf = 20;

        connect(session,
                &Platform::RecordingSession::warning,
                this,
                [](const QString& message) {
                    QMessageBox::information(nullptr, QStringLiteral("Recording"), message);
                });

        connect(session,
                &Platform::RecordingSession::finished,
                this,
                [this, session, thumbnail](bool kept,
                                           const QString& path,
                                           const QString& error) {
                    if (m_activeSession == session)
                        m_activeSession = nullptr;
                    session->deleteLater();
                    if (kept)
                        presentRecording(thumbnail, path);
                    else if (!error.isEmpty())
                        QMessageBox::warning(nullptr, QStringLiteral("Recording"), error);
                });

        QString error;
        if (!session->start(request, error)) {
            if (m_activeSession == session)
                m_activeSession = nullptr;
            session->deleteLater();
            QMessageBox::warning(nullptr, QStringLiteral("Recording"), error);
            return;
        }

        debugLog(QStringLiteral("recording: %1,%2 %3x%4 mic=%5 speaker=%6 -> %7")
                     .arg(region.x())
                     .arg(region.y())
                     .arg(region.width())
                     .arg(region.height())
                     .arg(options.microphone ? 1 : 0)
                     .arg(options.speaker ? 1 : 0)
                     .arg(request.outputPath));

        auto* controls = new RecordingControls(session, region);
        controls->show();
        controls->raise();
    }

    void presentRecording(const QImage& thumbnail, const QString& path)
    {
        const QImage thumb = thumbnail.isNull()
            ? QImage(2, 2, QImage::Format_ARGB32)
            : thumbnail;
        auto* card = new ResultCard(thumb,
                                    path,
                                    QString(),
                                    QStringLiteral("Recording saved"));
        card->show();
        card->raise();
        card->activateWindow();
    }

    void present(const QImage& image, const CaptureOptions& options)
    {
        if (image.isNull())
            return;

        const QString path = saveCapture(image);

        QString text;
        bool ocrFailed = false;
        bool isQr = false;
        QString ocrError;
        if (options.ocr) {
            // QR first: ApexShot's OCR toggle also reads codes.
            QByteArray payload;
            QString qrError;
            if (Platform::decodeQr(image, payload, qrError)) {
                isQr = true;
                text = QString::fromUtf8(payload).trimmed();
                debugLog(QStringLiteral("qr detected: %1 bytes").arg(payload.size()));
            } else {
                debugLog(QStringLiteral("qr: %1").arg(qrError));
                if (!Platform::recognizeText(image, text, ocrError)) {
                    ocrFailed = true;
                    debugLog(QStringLiteral("ocr failed: %1").arg(ocrError));
                }
                text = text.trimmed();
                debugLog(QStringLiteral("ocr chars=%1").arg(text.size()));
            }
        }

        QString hint = QStringLiteral("Copied to clipboard");
        if (options.ocr) {
            if (isQr) {
                QGuiApplication::clipboard()->setText(text);
                hint = QStringLiteral("QR code detected - copied");
            } else if (!text.isEmpty()) {
                QGuiApplication::clipboard()->setText(text);
                hint = QStringLiteral("Text copied to clipboard");
            } else {
                QGuiApplication::clipboard()->setImage(image);
                hint = ocrFailed ? QStringLiteral("OCR unavailable - image copied")
                                 : QStringLiteral("No text found - image copied");
            }
        } else {
            QGuiApplication::clipboard()->setImage(image);
        }

        auto* card = new ResultCard(image, path, text, hint);
        card->show();
        card->raise();
        card->activateWindow();
    }

    bool m_menuOpen = false;
    Platform::RecordingSession* m_activeSession = nullptr;
};

} // namespace

namespace QuickAccess {

int run(QApplication& app)
{
    registerBundledFonts();
    app.setQuitOnLastWindowClosed(false);

    {
        // Measure the top bar's text against its fixed columns, so clipping is
        // visible in the log instead of only on screen.
        QFont label(uiFamily());
        label.setPointSizeF(10.5);
        label.setBold(true);
        QFont pill(uiFamily());
        pill.setPointSizeF(9.5);
        pill.setBold(true);
        const QFontMetrics labelMetrics(label);
        const QFontMetrics pillMetrics(pill);
        QFile log(QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-quickaccess.log")));
        if (log.open(QIODevice::Append | QIODevice::WriteOnly)) {
            log.write(QStringLiteral("layout: label=%1 (min 108) pills 52/52/48/44 -> "
                                     "Free=%2 16:9=%3 4:3=%4 1:1=%5\n")
                          .arg(labelMetrics.horizontalAdvance(QStringLiteral("Draw an area")))
                          .arg(pillMetrics.horizontalAdvance(QStringLiteral("Free")))
                          .arg(pillMetrics.horizontalAdvance(QStringLiteral("16:9")))
                          .arg(pillMetrics.horizontalAdvance(QStringLiteral("4:3")))
                          .arg(pillMetrics.horizontalAdvance(QStringLiteral("1:1")))
                          .toUtf8());
        }
    }

    auto* shell = new Shell;

    const QStringList arguments = QCoreApplication::arguments();
    if (arguments.contains(QStringLiteral("--demo-area"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoArea(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-windows"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoWindows(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-crosshair"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoCrosshair(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-ocr"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoOcr(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-timer"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoTimer(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-record"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoRecord(); });
        return app.exec();
    }

    auto* tray = new QSystemTrayIcon(appIcon(), &app);
    tray->setToolTip(QStringLiteral("ApexShot"));

    auto* menu = new QMenu;
    menu->setFont(uiFont(12));
    menu->setStyleSheet(QStringLiteral(
        "QMenu { background:#19191c; color:#f5f5f7; "
        "border:1px solid rgba(255,255,255,54); border-radius:12px; padding:6px; } "
        "QMenu::item { padding:7px 16px; border-radius:8px; } "
        "QMenu::item:selected { background:rgba(255,102,0,205); color:#ffffff; }"));

    QAction* capture = menu->addAction(QStringLiteral("Capture..."));
    QAction* crosshair = menu->addAction(QStringLiteral("Crosshair capture"));

    QString recordingError;
    const bool recordingAvailable = Platform::RecordingSession::available(&recordingError);
    if (!recordingAvailable)
        debugLog(QStringLiteral("recording disabled: %1").arg(recordingError));

    QMenu* recordMenu = menu->addMenu(QStringLiteral("Record"));
    QAction* recordScreen = recordMenu->addAction(QStringLiteral("Screen"));
    QAction* recordArea = recordMenu->addAction(QStringLiteral("Area"));
    QAction* recordWindow = recordMenu->addAction(QStringLiteral("Window"));
    recordMenu->setEnabled(recordingAvailable);
    recordMenu->setToolTipsVisible(true);
    if (!recordingAvailable)
        recordMenu->setToolTip(recordingError);

    menu->addSeparator();
    QAction* quit = menu->addAction(QStringLiteral("Quit"));
    tray->setContextMenu(menu);
    tray->show();

    QObject::connect(capture, &QAction::triggered, shell, [shell] { shell->openMenu(); });
    QObject::connect(crosshair, &QAction::triggered, shell,
                     [shell] { shell->demoCrosshair(); });
    QObject::connect(recordScreen, &QAction::triggered, shell,
                     [shell] { shell->recordScreen(); });
    QObject::connect(recordArea, &QAction::triggered, shell,
                     [shell] { shell->recordArea(); });
    QObject::connect(recordWindow, &QAction::triggered, shell,
                     [shell] { shell->recordWindow(); });
    QObject::connect(quit, &QAction::triggered, &app, [shell, &app] {
        // A quit would destroy the session and discard its segments. Save
        // first; the next Quit, once the result card is up, exits.
        if (shell->hasActiveRecording()) {
            shell->stopActiveRecording();
            return;
        }
        app.quit();
    });
    QObject::connect(tray, &QSystemTrayIcon::activated, shell,
                     [shell](QSystemTrayIcon::ActivationReason reason) {
                         if (reason == QSystemTrayIcon::Trigger)
                             shell->openMenu();
                     });

    QTimer::singleShot(0, shell, [shell] { shell->openMenu(); });

    return app.exec();
}

} // namespace QuickAccess
