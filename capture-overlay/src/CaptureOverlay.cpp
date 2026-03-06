// ApexShot — Qt5 full-screen area selector overlay
// Ports the full custom UI from src/overlay.rs:
//   • Frosted-glass toolbar panel with 8 icons + hover states
//   • Size indicator panel
//   • L-shaped corner resize markers
//   • Full drag/move/resize logic

#include "CaptureOverlay.h"
#include "ScreenCapture.h"

#include <QApplication>
#include <QScreen>
#include <QPainter>
#include <QPainterPath>
#include <QMouseEvent>
#include <QKeyEvent>
#include <QFont>
#include <QFontMetrics>
#include <QGuiApplication>
#include <QImage>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QStandardPaths>
#include <QTimer>
#include <QThread>
#include <algorithm>
#include <cmath>
#include <limits>

// X11 window enumeration + auto-scroll simulation
#include <X11/Xlib.h>
#include <X11/Xatom.h>
#include <X11/extensions/XTest.h>
#undef None    // X11 defines None which conflicts with our HandlePos::None
#undef Bool

#include <QProcess>
#include <QHBoxLayout>
#include <QVBoxLayout>
#include <QPushButton>
#include <QLabel>
#include <QMessageBox>
#include <QDBusConnection>
#include <QDBusInterface>
#include <QDBusMessage>

// ── Constants (mirrors overlay.rs) ──────────────────────────────────────────
static const double HANDLE_MARKER_LENGTH    = 20.0;
static const double HANDLE_MARKER_THICKNESS = 2.5;
static const double FEATURE_PANEL_ITEM_W    = 62.0;
static const double FEATURE_PANEL_H        = 46.0;
static const double FEATURE_PANEL_RADIUS   = 13.0;
static const double FEATURE_PANEL_TOP_GAP  = 12.0;
static const double FEATURE_PANEL_MARGIN   = 16.0;
static const double SCROLL_HANDLE_DOT_RADIUS = 4.5;
static const double SCROLL_BUTTON_H = 36.0;
static const double SCROLL_BUTTON_GAP = 10.0;
static const double SCROLL_BUTTON_RADIUS = 10.0;
static const double SCROLL_BUTTON_MIN_W = 128.0;
static const int    SCROLL_CAPTURE_INTERVAL_MS = 300; // ms between captures (after settle time) - faster cadence
static const int    DEFAULT_SELECTION_W    = 600;
static const int    DEFAULT_SELECTION_H    = 744;
static const int    NUM_TOOLS              = 8;

static const char* TOOLBAR_LABELS[NUM_TOOLS] = {
    "Capture","Area","Fullscreen","Window","Scroll","Timer","OCR","Recording"
};

// ── Helpers ──────────────────────────────────────────────────────────────────

static void roundedRectPath(QPainterPath& path, double x, double y,
                             double w, double h, double r)
{
    r = std::min(r, std::min(w / 2.0, h / 2.0));
    r = std::max(r, 0.0);
    path.addRoundedRect(QRectF(x, y, w, h), r, r);
}

static void drawRoundedRect(QPainter& p, double x, double y,
                             double w, double h, double r)
{
    QPainterPath path;
    roundedRectPath(path, x, y, w, h, r);
    p.drawPath(path);
}

static bool callDaemonBool(const QString& method, int arg = 0, bool hasArg = false)
{
    QDBusInterface iface(QStringLiteral("org.apexshot.Daemon"),
                         QStringLiteral("/org/apexshot/Daemon"),
                         QStringLiteral("org.apexshot.Daemon"),
                         QDBusConnection::sessionBus());
    if (!iface.isValid()) {
        return false;
    }

    const QDBusMessage reply = hasArg ? iface.call(method, arg) : iface.call(method);
    if (reply.type() == QDBusMessage::ErrorMessage || reply.arguments().isEmpty()) {
        return false;
    }

    return reply.arguments().constFirst().toBool();
}

static bool callDaemonScrollStep(int x, int y, int steps)
{
    QDBusInterface iface(QStringLiteral("org.apexshot.Daemon"),
                         QStringLiteral("/org/apexshot/Daemon"),
                         QStringLiteral("org.apexshot.Daemon"),
                         QDBusConnection::sessionBus());
    if (!iface.isValid()) {
        return false;
    }

    const QDBusMessage reply = iface.call(QStringLiteral("ScrollStepGnome"), x, y, steps);
    if (reply.type() == QDBusMessage::ErrorMessage || reply.arguments().isEmpty()) {
        return false;
    }

    return reply.arguments().constFirst().toBool();
}

static void showWebScrollCaptureInfo(QWidget* parent)
{
    QMessageBox messageBox(parent);
    messageBox.setWindowTitle(QStringLiteral("Webpage scroll capture"));
    messageBox.setIcon(QMessageBox::Information);
    messageBox.setText(QStringLiteral("Scroll capture is available on webpages through the browser extension."));
    messageBox.setInformativeText(QStringLiteral("Use the ApexShot browser extension on the page you want to capture. After the extension sends the capture to the app, it will open in the normal screenshot preview overlay."));
    messageBox.setStandardButtons(QMessageBox::Ok);
    messageBox.exec();
}

static bool shouldUseManualScrollAssistMode()
{
    const QString platform = QGuiApplication::platformName().toLower();
    if (!platform.contains(QStringLiteral("wayland"))) {
        return false;
    }

    const QString desktop = QString::fromLocal8Bit(qgetenv("XDG_CURRENT_DESKTOP")).toLower();
    const QString session = QString::fromLocal8Bit(qgetenv("XDG_SESSION_DESKTOP")).toLower();
    return desktop.contains(QStringLiteral("gnome")) || session.contains(QStringLiteral("gnome"));
}

static void callDaemonVoid(const QString& method)
{
    QDBusInterface iface(QStringLiteral("org.apexshot.Daemon"),
                         QStringLiteral("/org/apexshot/Daemon"),
                         QStringLiteral("org.apexshot.Daemon"),
                         QDBusConnection::sessionBus());
    if (!iface.isValid()) {
        return;
    }
    iface.call(method);
}

struct ToolbarLayout {
    QRectF toolsPanel;
    QRectF sizePanel;
    QRectF itemCells[NUM_TOOLS];
};

static ToolbarLayout computeToolbarLayout(double selX, double selY,
                                           double selW, double selH,
                                           double screenW, double screenH,
                                           bool forceAbove = false)
{
    double panelW      = FEATURE_PANEL_ITEM_W * NUM_TOOLS;
    double sizePanelW  = 98.0;
    double sizePanelH  = 46.0;
    double panelGap    = 12.0;

    double groupW = panelW + panelGap + sizePanelW;
    double groupX = selX + (selW - groupW) / 2.0;
    groupX = std::max(FEATURE_PANEL_MARGIN,
             std::min(groupX, screenW - groupW - FEATURE_PANEL_MARGIN));

    double groupH    = std::max(FEATURE_PANEL_H, sizePanelH);
    double aboveY    = selY - FEATURE_PANEL_TOP_GAP - groupH;
    double belowY    = selY + selH + FEATURE_PANEL_TOP_GAP;
    bool canAbove    = aboveY >= FEATURE_PANEL_MARGIN;
    bool canBelow    = (belowY + groupH + FEATURE_PANEL_MARGIN) <= screenH;

    double groupY;
    if (forceAbove) {
        groupY = std::max(
            FEATURE_PANEL_MARGIN,
            std::min(aboveY, screenH - groupH - FEATURE_PANEL_MARGIN)
        );
    } else if (canAbove) {
        groupY = aboveY;
    } else if (canBelow) {
        groupY = belowY;
    } else {
        groupY = std::max(FEATURE_PANEL_MARGIN,
                 std::min(aboveY, screenH - groupH - FEATURE_PANEL_MARGIN));
    }

    groupY = std::max(FEATURE_PANEL_MARGIN,
             std::min(groupY, screenH - groupH - FEATURE_PANEL_MARGIN));

    ToolbarLayout layout;
    layout.toolsPanel = QRectF(groupX,
                               groupY + (groupH - FEATURE_PANEL_H) / 2.0,
                               panelW, FEATURE_PANEL_H);
    layout.sizePanel  = QRectF(groupX + panelW + panelGap,
                               groupY + (groupH - sizePanelH) / 2.0,
                               sizePanelW, sizePanelH);

    for (int i = 0; i < NUM_TOOLS; ++i) {
        layout.itemCells[i] = QRectF(layout.toolsPanel.x() + i * FEATURE_PANEL_ITEM_W,
                                      layout.toolsPanel.y(),
                                      FEATURE_PANEL_ITEM_W, FEATURE_PANEL_H);
    }
    return layout;
}

// Draw frosted glass panel (mirrors draw_frosted_panel in overlay.rs)
static void drawFrostedPanel(QPainter& p, double x, double y,
                              double w, double h, double radius,
                              const QImage* blurredBg,
                              double screenW, double screenH)
{
    // Drop shadow
    {
        QPainterPath shadow;
        roundedRectPath(shadow, x, y + 3.0, w, h, radius);
        p.fillPath(shadow, QColor(0, 0, 0, 77)); // 0.30 * 255
    }

    // Clip to panel shape
    p.save();
    QPainterPath clip;
    roundedRectPath(clip, x, y, w, h, radius);
    p.setClipPath(clip);

    // Blurred background or solid dark base
    if (blurredBg && !blurredBg->isNull()) {
        double scaleX = screenW / blurredBg->width();
        double scaleY = screenH / blurredBg->height();
        p.save();
        p.scale(scaleX, scaleY);
        p.drawImage(QPointF(0, 0), *blurredBg);
        p.restore();
    } else {
        p.fillRect(QRectF(x, y, w, h), QColor(20, 20, 20, 255));
    }

    // Dark tint (0.52 alpha)
    p.fillRect(QRectF(x, y, w, h), QColor(0, 0, 0, 133));
    // White sheen (0.10 alpha)
    p.fillRect(QRectF(x, y, w, h), QColor(255, 255, 255, 26));

    p.restore();
}

// Draw one toolbar icon (mirrors draw_toolbar_icon in overlay.rs)
static void drawToolbarIcon(QPainter& p, int iconIndex,
                             double cx, double cy,
                             QColor color)
{
    p.save();
    p.setPen(QPen(color, 1.6, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
    p.setBrush(Qt::NoBrush);

    static const double PI = M_PI;

    switch (iconIndex) {
    case 0: { // Capture — crosshair in circle
        p.drawEllipse(QPointF(cx, cy), 6.2, 6.2);
        p.drawLine(QPointF(cx - 3.2, cy), QPointF(cx + 3.2, cy));
        p.drawLine(QPointF(cx, cy - 3.2), QPointF(cx, cy + 3.2));
        break;
    }
    case 1: { // Area — corner brackets
        double h = 5.5;
        QPainterPath path;
        path.moveTo(cx - 7.0, cy - 1.5); path.lineTo(cx - 7.0, cy - h); path.lineTo(cx - 1.5, cy - h);
        path.moveTo(cx + 1.5, cy - h);   path.lineTo(cx + 7.0, cy - h); path.lineTo(cx + 7.0, cy - 1.5);
        path.moveTo(cx - 7.0, cy + 1.5); path.lineTo(cx - 7.0, cy + h); path.lineTo(cx - 1.5, cy + h);
        path.moveTo(cx + 1.5, cy + h);   path.lineTo(cx + 7.0, cy + h); path.lineTo(cx + 7.0, cy + 1.5);
        p.drawPath(path);
        break;
    }
    case 2: { // Fullscreen — monitor with stand
        QPainterPath path;
        roundedRectPath(path, cx - 7.0, cy - 6.0, 14.0, 10.5, 2.0);
        p.drawPath(path);
        p.drawLine(QPointF(cx, cy + 4.5), QPointF(cx, cy + 7.5));
        p.drawLine(QPointF(cx - 4.5, cy + 7.5), QPointF(cx + 4.5, cy + 7.5));
        break;
    }
    case 3: { // Window — browser window
        QPainterPath path;
        roundedRectPath(path, cx - 7.0, cy - 5.5, 14.0, 9.5, 1.7);
        p.drawPath(path);
        p.drawLine(QPointF(cx - 7.0, cy - 2.0), QPointF(cx + 7.0, cy - 2.0));
        break;
    }
    case 4: { // Scroll — arrow
        QPainterPath path;
        path.moveTo(cx, cy - 4.8); path.lineTo(cx, cy + 1.8);
        path.moveTo(cx - 3.2, cy - 1.0); path.lineTo(cx, cy + 1.9); path.lineTo(cx + 3.2, cy - 1.0);
        p.drawPath(path);
        break;
    }
    case 5: { // Timer — clock
        p.drawEllipse(QPointF(cx, cy), 6.0, 6.0);
        QPainterPath hands;
        hands.moveTo(cx, cy); hands.lineTo(cx, cy - 2.8);
        hands.moveTo(cx, cy); hands.lineTo(cx + 2.2, cy + 1.7);
        p.drawPath(hands);
        break;
    }
    case 6: { // OCR — "Aa" text
        p.setPen(color);
        QFont f = p.font();
        f.setFamily("Sans");
        f.setPointSizeF(8.0);
        f.setBold(true);
        p.setFont(f);
        QFontMetricsF fm(f);
        QString txt("Aa");
        QRectF br = fm.boundingRect(txt);
        p.drawText(QPointF(cx - br.width() / 2.0,
                           cy + br.height() / 2.0 - fm.descent() + 0.2), txt);
        break;
    }
    case 7: { // Recording — camera body + lens
        QPainterPath path;
        roundedRectPath(path, cx - 6.5, cy - 4.3, 10.0, 8.6, 2.0);
        p.drawPath(path);
        p.drawEllipse(QPointF(cx - 1.3, cy), 2.2, 2.2);
        // Viewfinder bump — filled
        QPainterPath bump;
        roundedRectPath(bump, cx + 3.8, cy - 2.2, 3.6, 4.4, 0.8);
        p.fillPath(bump, color);
        break;
    }
    }
    p.restore();
}

// ── ScrollControlPanel ───────────────────────────────────────────────────────
// A small floating widget shown during scroll capture with Cancel / Done
// buttons and a frame counter.  Visible while the main overlay is hidden.

ScrollControlPanel::ScrollControlPanel(QWidget* parent)
    : QWidget(parent)
{
    setWindowFlags(Qt::FramelessWindowHint
                   | Qt::WindowStaysOnTopHint
                   | Qt::BypassWindowManagerHint
                   | Qt::Tool);
    setAttribute(Qt::WA_TranslucentBackground, true);
    setAttribute(Qt::WA_ShowWithoutActivating, true);
    setFixedHeight(56);

    auto* layout = new QHBoxLayout(this);
    layout->setContentsMargins(14, 8, 14, 8);
    layout->setSpacing(10);

    m_statusLabel = new QLabel(QStringLiteral("Capturing..."), this);
    m_statusLabel->setStyleSheet(
        "color: white; font-size: 13px; font-weight: bold;"
    );

    m_frameLabel = new QLabel(QStringLiteral("0 frames"), this);
    m_frameLabel->setStyleSheet(
        "color: rgba(255,255,255,180); font-size: 12px;"
    );

    m_cancelBtn = new QPushButton(QStringLiteral("Cancel"), this);
    m_cancelBtn->setStyleSheet(
        "QPushButton {"
        "  background: rgba(255,255,255,30);"
        "  color: white;"
        "  border: 1px solid rgba(255,255,255,60);"
        "  border-radius: 8px;"
        "  padding: 6px 18px;"
        "  font-size: 12px;"
        "  font-weight: bold;"
        "}"
        "QPushButton:hover {"
        "  background: rgba(255,60,60,120);"
        "  border-color: rgba(255,100,100,160);"
        "}"
    );

    m_doneBtn = new QPushButton(QStringLiteral("Done"), this);
    m_doneBtn->setStyleSheet(
        "QPushButton {"
        "  background: rgba(0,122,255,140);"
        "  color: white;"
        "  border: 1px solid rgba(100,180,255,160);"
        "  border-radius: 8px;"
        "  padding: 6px 18px;"
        "  font-size: 12px;"
        "  font-weight: bold;"
        "}"
        "QPushButton:hover {"
        "  background: rgba(0,122,255,200);"
        "  border-color: rgba(130,200,255,200);"
        "}"
    );

    layout->addWidget(m_statusLabel);
    layout->addWidget(m_frameLabel);
    layout->addStretch();
    layout->addWidget(m_cancelBtn);
    layout->addWidget(m_doneBtn);

    connect(m_cancelBtn, &QPushButton::clicked, this, &ScrollControlPanel::cancelClicked);
    connect(m_doneBtn, &QPushButton::clicked, this, &ScrollControlPanel::doneClicked);

    setMinimumWidth(400);
}

void ScrollControlPanel::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    p.setRenderHint(QPainter::Antialiasing);
    QPainterPath path;
    path.addRoundedRect(rect().adjusted(1, 1, -1, -1), 12, 12);
    p.fillPath(path, QColor(20, 20, 24, 220));
    p.setPen(QPen(QColor(255, 255, 255, 40), 1));
    p.drawPath(path);
}

void ScrollControlPanel::setFrameCount(int count)
{
    m_frameLabel->setText(
        QString("%1 frame%2").arg(count).arg(count != 1 ? "s" : "")
    );
}

void ScrollControlPanel::setStatus(const QString& text)
{
    m_statusLabel->setText(text);
}

void ScrollControlPanel::setCapturingDone()
{
    m_statusLabel->setText(QStringLiteral("Capture complete"));
    m_statusLabel->setStyleSheet(
        "color: #6fdf6f; font-size: 13px; font-weight: bold;"
    );
}

void ScrollControlPanel::positionNear(const QRect& captureArea, const QSize& screenSize)
{
    int panelW = std::max(minimumWidth(), sizeHint().width());
    int panelH = height();

    // Position below the capture area, centered horizontally
    int x = captureArea.x() + (captureArea.width() - panelW) / 2;
    int y = captureArea.bottom() + 16;

    x = std::max(16, std::min(x, screenSize.width() - panelW - 16));
    y = std::min(y, screenSize.height() - panelH - 16);
    y = std::max(16, y);

    setGeometry(x, y, panelW, panelH);
}

// ── Constructor ───────────────────────────────────────────────────────────────

CaptureOverlay::CaptureOverlay(const QPixmap& background, QWidget* parent)
    : QWidget(parent)
    , m_background(background)
    , m_hasSelection(false)
    , m_dragging(false)
    , m_moving(false)
    , m_resizing(HandlePos::None)
    , m_dragStart(0, 0)
    , m_fullscreenMode(false)
    , m_windowMode(false)
    , m_captureIntent(CaptureIntent::Area)
    , m_scrollStage(ScrollStage::Inactive)
    , m_scrollCaptureReady(false)
    , m_scrollCaptureTimer(new QTimer(this))
    , m_scrollControlPanel(new ScrollControlPanel())
    , m_scrollSimilarCount(0)
    , m_scrollFrameCount(0)
    , m_manualScrollAssistMode(false)
    , m_hoveredWindow(-1)
    , m_hoveredTool(-1)
    , m_hoveredSizePanel(false)
{
    // Cover entire virtual desktop
    QRect desktop;
    for (QScreen* screen : QGuiApplication::screens())
        desktop = desktop.united(screen->geometry());
    setGeometry(desktop);

    setWindowFlags(Qt::FramelessWindowHint
                   | Qt::WindowStaysOnTopHint
                   | Qt::BypassWindowManagerHint
                   | Qt::Tool);

    if (m_background.isNull())
        setAttribute(Qt::WA_TranslucentBackground, true);

    setAttribute(Qt::WA_DeleteOnClose, false);
    setMouseTracking(true);
    setCursor(Qt::CrossCursor);
    grabKeyboard();

    const int defaultW = std::max(kMinSize, std::min(DEFAULT_SELECTION_W, width()));
    const int defaultH = std::max(kMinSize, std::min(DEFAULT_SELECTION_H, height()));
    const int defaultX = (width() - defaultW) / 2;
    const int defaultY = (height() - defaultH) / 2;
    m_selection = QRect(defaultX, defaultY, defaultW, defaultH);
    m_hasSelection = true;

    // Pre-build blurred background for frosted glass (1/4 res gaussian)
    if (!m_background.isNull()) {
        QImage full = m_background.toImage().convertToFormat(QImage::Format_ARGB32);
        int bw = std::max(1, full.width() / 4);
        int bh = std::max(1, full.height() / 4);
        QImage small = full.scaled(bw, bh, Qt::IgnoreAspectRatio, Qt::SmoothTransformation);
        // Simple box blur approximation (3 passes)
        for (int pass = 0; pass < 3; ++pass) {
            small = small.scaled(bw * 2, bh * 2, Qt::IgnoreAspectRatio, Qt::SmoothTransformation)
                         .scaled(bw, bh, Qt::IgnoreAspectRatio, Qt::SmoothTransformation);
        }
        m_blurredBg = small;
    }

    m_scrollCaptureTimer->setSingleShot(true);
    connect(m_scrollCaptureTimer, &QTimer::timeout, this, &CaptureOverlay::onAutoScrollTick);

    connect(m_scrollControlPanel, &ScrollControlPanel::cancelClicked, this, &CaptureOverlay::cancelSelection);
    connect(m_scrollControlPanel, &ScrollControlPanel::doneClicked, this, [this]() {
        stopAutoScrollCapture(true); // stop and finalize
    });
}

// ── Paint ─────────────────────────────────────────────────────────────────────

void CaptureOverlay::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    p.setRenderHint(QPainter::Antialiasing);
    p.setRenderHint(QPainter::TextAntialiasing);

    const QRect widgetRect = rect();
    const double sw = widgetRect.width();
    const double sh = widgetRect.height();

    // ── Background ────────────────────────────────────────────────────────────
    if (!m_background.isNull()) {
        p.drawPixmap(widgetRect, m_background);
    } else {
        p.fillRect(widgetRect, QColor(0, 0, 0, 51)); // 0.20 alpha
    }

    // ── Window mode ───────────────────────────────────────────────────────────
    if (m_windowMode) {
        p.fillRect(widgetRect, QColor(0, 0, 0, 80));
        // Draw highlight rect over hovered window
        for (int i = 0; i < m_windows.size(); ++i) {
            const WindowInfo& win = m_windows[i];
            if (!widgetRect.intersects(win.rect)) continue;
            bool hovered = (i == m_hoveredWindow);
            if (hovered) {
                // Bright highlight border
                p.setPen(QPen(QColor(0, 122, 255, 230), 3.0));
                p.setBrush(QColor(0, 122, 255, 30));
                p.drawRect(win.rect);
                // Title pill above the window
                QFont f; f.setPointSizeF(11.5); f.setBold(true); p.setFont(f);
                QFontMetricsF fm(f);
                QString label = win.title.length() > 48
                    ? win.title.left(45) + "…" : win.title;
                double tw = fm.horizontalAdvance(label);
                double pillW = tw + 28, pillH = 32;
                double px = win.rect.x() + (win.rect.width() - pillW) / 2.0;
                double py = win.rect.y() - pillH - 8;
                if (py < 8) py = win.rect.y() + 8;
                px = std::max(8.0, std::min(px, sw - pillW - 8));
                QPainterPath pill; pill.addRoundedRect(QRectF(px, py, pillW, pillH), 10, 10);
                p.fillPath(pill, QColor(0, 0, 0, 180));
                p.setPen(QColor(255, 255, 255, 240));
                p.drawText(QRectF(px, py, pillW, pillH), Qt::AlignCenter, label);
            }
        }
        // Bottom hint
        QFont hf; hf.setPointSizeF(11.0); p.setFont(hf);
        QString hint = "Click a window to capture  •  ESC to cancel";
        QFontMetrics hfm(hf);
        QRect tr = hfm.boundingRect(hint);
        tr.moveCenter(widgetRect.center() + QPoint(0, (int)(sh/2) - 48));
        QPainterPath hpill; hpill.addRoundedRect(tr.adjusted(-14,-8,14,8), 10, 10);
        p.fillPath(hpill, QColor(0,0,0,140));
        p.setPen(QColor(255,255,255,200));
        p.drawText(tr, Qt::AlignCenter, hint);
        return;
    }

    if (!m_hasSelection) {
        // Hint text
        p.fillRect(widgetRect, QColor(0, 0, 0, 30));
        QFont f; f.setPointSize(13); p.setFont(f);
        QString hint = "Drag to select an area  •  ESC to cancel";
        QFontMetrics fm(f);
        QRect tr = fm.boundingRect(hint);
        tr.moveCenter(widgetRect.center() + QPoint(0, 40));
        QPainterPath pill; pill.addRoundedRect(tr.adjusted(-14,-8,14,8), 10, 10);
        p.fillPath(pill, QColor(0,0,0,130));
        p.setPen(QColor(255,255,255,200));
        p.drawText(tr, Qt::AlignCenter, hint);
        return;
    }

    const QRect sel = m_selection.normalized();
    const double sx = sel.x(), sy = sel.y(), selW = sel.width(), selH = sel.height();

    // ── Dim outside selection (skip in fullscreen mode) ──────────────────────
    if (!m_fullscreenMode) {
        const QColor dim(0, 0, 0, 140);
        if (sy > 0)           p.fillRect(QRect(0, 0, widgetRect.width(), sy), dim);
        if (sel.bottom() < widgetRect.height()-1)
                              p.fillRect(QRect(0, sel.bottom()+1, widgetRect.width(),
                                               widgetRect.height()-sel.bottom()-1), dim);
        if (sx > 0)           p.fillRect(QRect(0, sy, sx, selH), dim);
        if (sel.right() < widgetRect.width()-1)
                              p.fillRect(QRect(sel.right()+1, sy,
                                               widgetRect.width()-sel.right()-1, selH), dim);
    } else {
        // Fullscreen mode: very subtle vignette to indicate full screen is selected
        p.fillRect(widgetRect, QColor(0, 0, 0, 26));
    }

    // ── Reveal selection area (repaint background there sharp) ────────────────
    if (!m_background.isNull()) {
        p.drawPixmap(sel, m_background, sel);
    } else {
        // No background pixmap — punch the selection area fully transparent so
        // the real screen content shows through without any dark tint.
        p.setCompositionMode(QPainter::CompositionMode_Clear);
        p.fillRect(sel, Qt::transparent);
        p.setCompositionMode(QPainter::CompositionMode_SourceOver);
    }

    // ── Selection handles ─────────────────────────────────────────────────────
    {
        const bool scrollModeActive = (m_captureIntent == CaptureIntent::Scroll);
        if (scrollModeActive) {
            if (m_scrollStage == ScrollStage::Capturing) {
                p.save();
                QRegion outside(widgetRect);
                p.setClipRegion(outside.subtracted(QRegion(sel)), Qt::ReplaceClip);
                p.setPen(QPen(QColor(255, 255, 255, 220), 2.0));
                p.setBrush(Qt::NoBrush);
                p.drawRect(sel.adjusted(-2, -2, 1, 1));
                p.restore();
            } else {
                p.setPen(QPen(QColor(255, 255, 255, 210), 1.6));
                p.setBrush(Qt::NoBrush);
                p.drawRect(sel.adjusted(0, 0, -1, -1));

                if (m_scrollStage == ScrollStage::Armed) {
                    p.setPen(QPen(QColor(22, 22, 24, 230), 1.2));
                    p.setBrush(QColor(255, 255, 255, 248));
                    for (const QPoint& center : handleCenters()) {
                        p.drawEllipse(QPointF(center.x(), center.y()),
                                      SCROLL_HANDLE_DOT_RADIUS,
                                      SCROLL_HANDLE_DOT_RADIUS);
                    }
                }
            }
        } else {
            double half = HANDLE_MARKER_LENGTH / 2.0;
            p.setPen(QPen(QColor(255,255,255,245), HANDLE_MARKER_THICKNESS,
                          Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));

            // Corners
            auto corner = [&](double ex, double ey, double dx, double dy) {
                QPainterPath path;
                path.moveTo(ex, ey + dy * half); path.lineTo(ex, ey); path.lineTo(ex + dx * half, ey);
                p.drawPath(path);
            };
            corner(sx,        sy,        +1, +1);
            corner(sx+selW,   sy,        -1, +1);
            corner(sx,        sy+selH,   +1, -1);
            corner(sx+selW,   sy+selH,   -1, -1);

            // Edge midpoints
            p.drawLine(QPointF(sx + selW/2 - half, sy),      QPointF(sx + selW/2 + half, sy));
            p.drawLine(QPointF(sx + selW/2 - half, sy+selH), QPointF(sx + selW/2 + half, sy+selH));
            p.drawLine(QPointF(sx, sy + selH/2 - half),      QPointF(sx, sy + selH/2 + half));
            p.drawLine(QPointF(sx+selW, sy + selH/2 - half), QPointF(sx+selW, sy + selH/2 + half));
        }
    }

    // ── Toolbar ───────────────────────────────────────────────────────────────
    drawToolbar(p, sx, sy, selW, selH, sw, sh);
}

// ── Draw toolbar (mirrors draw_feature_toolbar in overlay.rs) ─────────────────

void CaptureOverlay::drawToolbar(QPainter& p,
                                  double selX, double selY,
                                  double selW, double selH,
                                  double screenW, double screenH)
{
    const bool scrollModeActive = (m_captureIntent == CaptureIntent::Scroll);
    ToolbarLayout layout = computeToolbarLayout(
        selX,
        selY,
        selW,
        selH,
        screenW,
        screenH,
        scrollModeActive
    );
    const QImage* blurPtr = m_blurredBg.isNull() ? nullptr : &m_blurredBg;

    int activeTool = 1; // Area mode by default
    if (scrollModeActive) {
        activeTool = 4;
    }
    if (m_fullscreenMode) {
        activeTool = 2;
    }
    if (m_captureIntent == CaptureIntent::Ocr) {
        activeTool = 6;
    }

    // ── Tools panel frosted glass ─────────────────────────────────────────────
    drawFrostedPanel(p,
                     layout.toolsPanel.x(), layout.toolsPanel.y(),
                     layout.toolsPanel.width(), layout.toolsPanel.height(),
                     FEATURE_PANEL_RADIUS, blurPtr, screenW, screenH);

    // ── Selected style on active tool ────────────────────────────────────────
    if (activeTool >= 0 && activeTool < NUM_TOOLS) {
        QRectF cell = layout.itemCells[activeTool];
        double hx = cell.x() + 3.0, hy = cell.y() + 4.0;
        double hw = cell.width() - 6.0, hh = cell.height() - 8.0;

        QPainterPath glow;
        roundedRectPath(glow, hx - 1.0, hy - 1.0, hw + 2.0, hh + 2.0, 9.0);
        p.fillPath(glow, QColor(0, 122, 255, 54));

        QPainterPath pill;
        roundedRectPath(pill, hx, hy, hw, hh, 8.0);
        p.fillPath(pill, QColor(38, 140, 255, 66));

        p.save();
        p.setClipPath(pill);
        p.setPen(QPen(QColor(150, 206, 255, 225), 1.2));
        p.setBrush(Qt::NoBrush);
        QPainterPath rim;
        roundedRectPath(rim, hx + 0.6, hy + 0.6, hw - 1.2, hh - 1.2, 7.5);
        p.drawPath(rim);
        p.restore();
    }

    // ── Hover highlight on hovered tool ──────────────────────────────────────
    if (m_hoveredTool >= 0 && m_hoveredTool < NUM_TOOLS) {
        QRectF cell = layout.itemCells[m_hoveredTool];
        double hx = cell.x() + 3.0, hy = cell.y() + 4.0;
        double hw = cell.width() - 6.0, hh = cell.height() - 8.0;

        // Outer glow
        QPainterPath glow; roundedRectPath(glow, hx-1, hy-1, hw+2, hh+2, 9.0);
        p.fillPath(glow, QColor(255,255,255,20));
        // Main pill
        QPainterPath pill; roundedRectPath(pill, hx, hy, hw, hh, 8.0);
        p.fillPath(pill, QColor(255,255,255,66));
        // Inner rim
        p.save();
        p.setClipPath(pill);
        p.setPen(QPen(QColor(255,255,255,140), 1.2));
        p.setBrush(Qt::NoBrush);
        QPainterPath rim; roundedRectPath(rim, hx+0.6, hy+0.6, hw-1.2, hh-1.2, 7.5);
        p.drawPath(rim);
        // Top accent
        p.setPen(QPen(QColor(255,255,255,204), 1.5));
        p.drawLine(QPointF(hx+10, hy+0.75), QPointF(hx+hw-10, hy+0.75));
        p.restore();
    }

    // ── Size panel frosted glass ──────────────────────────────────────────────
    drawFrostedPanel(p,
                     layout.sizePanel.x(), layout.sizePanel.y(),
                     layout.sizePanel.width(), layout.sizePanel.height(),
                     FEATURE_PANEL_RADIUS, blurPtr, screenW, screenH);

    // ── Hover highlight on size panel ─────────────────────────────────────────
    if (m_hoveredSizePanel) {
        double hx = layout.sizePanel.x()+3, hy = layout.sizePanel.y()+3;
        double hw = layout.sizePanel.width()-6, hh = layout.sizePanel.height()-6;
        QPainterPath glow; roundedRectPath(glow, hx-1, hy-1, hw+2, hh+2, 8.0);
        p.fillPath(glow, QColor(255,255,255,18));
        QPainterPath pill; roundedRectPath(pill, hx, hy, hw, hh, 7.0);
        p.fillPath(pill, QColor(255,255,255,56));
        p.save(); p.setClipPath(pill);
        p.setPen(QPen(QColor(255,255,255,128), 1.2)); p.setBrush(Qt::NoBrush);
        QPainterPath rim; roundedRectPath(rim, hx+0.6, hy+0.6, hw-1.2, hh-1.2, 6.5);
        p.drawPath(rim);
        p.setPen(QPen(QColor(255,255,255,191), 1.5));
        p.drawLine(QPointF(hx+8, hy+0.75), QPointF(hx+hw-8, hy+0.75));
        p.restore();
    }

    // ── Tool icons + labels ───────────────────────────────────────────────────
    for (int i = 0; i < NUM_TOOLS; ++i) {
        QRectF cell = layout.itemCells[i];
        double cx = cell.x() + cell.width() / 2.0;
        bool hovered = (m_hoveredTool == i);
        bool active = (activeTool == i);
        double iconAlpha = (hovered || active) ? 1.0 : 0.98;
        double shadowAlpha = hovered ? 0.30 : (active ? 0.38 : 0.52);
        double iconY = layout.toolsPanel.y() + ((hovered || active) ? 15.5 : 16.0);
        QColor iconColor = active
            ? QColor(223, 241, 255, int(iconAlpha * 255))
            : QColor(255, 255, 255, int(iconAlpha * 255));

        // Shadow pass
        drawToolbarIcon(p, i, cx + 0.6, iconY + 0.8,
                        QColor(0,0,0, int(shadowAlpha*255)));
        // Icon pass
        drawToolbarIcon(p, i, cx, iconY, iconColor);

        // Label
        QFont f; f.setFamily("Sans"); f.setPointSizeF(7.5);
        f.setBold(hovered || active); p.setFont(f);
        QFontMetricsF fm(f);
        QString label(TOOLBAR_LABELS[i]);

        // Shadow
        p.setPen(QColor(0,0,0, int(shadowAlpha*255)));
        double tw = fm.horizontalAdvance(label);
        p.drawText(QPointF(cx - tw/2.0 + 0.6,
                           layout.toolsPanel.y() + 34.0 + 0.8), label);
        // Foreground
        p.setPen(active
            ? QColor(223,241,255, int(iconAlpha * 255))
            : QColor(255,255,255, int(iconAlpha * 255)));
        p.drawText(QPointF(cx - tw/2.0,
                           layout.toolsPanel.y() + 34.0), label);
    }

    // ── Size label ("Size" header + "WxH" value) ──────────────────────────────
    double scx = layout.sizePanel.x() + layout.sizePanel.width() / 2.0;
    QString sizeVal = QString("%1×%2").arg((int)selW).arg((int)selH);

    // "Size" header
    {
        QFont f; f.setFamily("Sans"); f.setPointSizeF(7.5); f.setBold(false); p.setFont(f);
        QFontMetricsF fm(f);
        double tw = fm.horizontalAdvance("Size");
        double ty = layout.sizePanel.y() + 14.0;
        p.setPen(QColor(0,0,0,128));
        p.drawText(QPointF(scx - tw/2.0 + 0.6, ty + 0.8), "Size");
        p.setPen(QColor(255,255,255,230));
        p.drawText(QPointF(scx - tw/2.0, ty), "Size");
    }
    // Dimension value
    {
        QFont f; f.setFamily("Sans"); f.setPointSizeF(9.0); f.setBold(true); p.setFont(f);
        QFontMetricsF fm(f);
        double tw = fm.horizontalAdvance(sizeVal);
        double ty = layout.sizePanel.y() + 30.0;
        p.setPen(QColor(0,0,0,140));
        p.drawText(QPointF(scx - tw/2.0 + 0.6, ty + 0.8), sizeVal);
        p.setPen(QColor(255,255,255,250));
        p.drawText(QPointF(scx - tw/2.0, ty), sizeVal);
    }

    if (scrollModeActive) {
        auto drawScrollButton = [&](const QRectF& rect,
                                    const QString& text,
                                    bool primary) {
            drawFrostedPanel(
                p,
                rect.x(),
                rect.y(),
                rect.width(),
                rect.height(),
                SCROLL_BUTTON_RADIUS,
                blurPtr,
                screenW,
                screenH
            );

            if (primary) {
                QPainterPath accent;
                roundedRectPath(
                    accent,
                    rect.x() + 1.0,
                    rect.y() + 1.0,
                    rect.width() - 2.0,
                    rect.height() - 2.0,
                    SCROLL_BUTTON_RADIUS - 1.0
                );
                p.fillPath(accent, QColor(0, 122, 255, 58));
            }

            QFont f;
            f.setFamily("Sans");
            f.setPointSizeF(10.0);
            f.setBold(true);
            p.setFont(f);
            p.setPen(primary ? QColor(224, 241, 255, 252) : QColor(255, 255, 255, 248));
            p.drawText(rect, Qt::AlignCenter, text);
        };

        if (m_scrollStage == ScrollStage::Armed) {
            drawScrollButton(scrollPrimaryButtonRect(), QStringLiteral("Start capture"), true);
        }
    }
}

// ── Mouse events ──────────────────────────────────────────────────────────────

void CaptureOverlay::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) return;
    const QPoint pos = event->pos();

    // Window mode — click selects the hovered window
    if (m_windowMode) {
        if (m_hoveredWindow >= 0 && m_hoveredWindow < m_windows.size()) {
            m_selection = m_windows[m_hoveredWindow].rect;
            m_hasSelection = true;
            exitWindowMode();
            confirmSelection();
        }
        return;
    }

    if (m_captureIntent == CaptureIntent::Scroll && handleScrollButtonClick(pos)) {
        return;
    }
    if (m_captureIntent == CaptureIntent::Scroll && m_scrollStage == ScrollStage::Capturing) {
        return;
    }

    // Check toolbar click — only confirm if click is within or on the selection
    // (not when clicking outside, which should start a fresh selection)
    if (m_hasSelection) {
        const QRect sel = m_selection.normalized();

        // Only allow toolbar clicks when the click point is NOT outside the
        // selection area — otherwise the user is starting a new selection and
        // accidentally grazing the toolbar should not confirm anything.
        HandlePos h = hitTest(pos);
        bool clickInsideOrOnSelection = (h != HandlePos::None);

        // Helper lambda to handle toolbar tool click
        auto handleToolClick = [&](int toolIndex) -> bool {
            if (toolIndex == 2) {
                // Fullscreen: expand selection to cover entire screen, wait for Enter
                exitScrollMode();
                exitWindowMode();
                m_selection = QRect(0, 0, width(), height());
                m_hasSelection = true;
                m_fullscreenMode = true;
                m_captureIntent = CaptureIntent::Area;
                update();
                return true;
            } else if (toolIndex == 1) {
                // Area: restore default centered area selection
                exitScrollMode();
                exitWindowMode();
                int defaultW = std::max(kMinSize, std::min(DEFAULT_SELECTION_W, width()));
                int defaultH = std::max(kMinSize, std::min(DEFAULT_SELECTION_H, height()));
                int defaultX = (width() - defaultW) / 2;
                int defaultY = (height() - defaultH) / 2;
                m_selection = QRect(defaultX, defaultY, defaultW, defaultH);
                m_hasSelection = true;
                m_fullscreenMode = false;
                m_captureIntent = CaptureIntent::Area;
                update();
                return true;
            } else if (toolIndex == 3) {
                // Window: on Wayland use GNOME DBus (exit code 3),
                // on X11 use hover-select mode
                exitScrollMode();
                std::fprintf(stderr, "[CaptureOverlay] Window tool clicked (index 3)\n");
                std::fprintf(stderr, "[CaptureOverlay] WAYLAND_DISPLAY=%s\n",
                    qgetenv("WAYLAND_DISPLAY").constData());
                if (qEnvironmentVariableIsSet("WAYLAND_DISPLAY")) {
                    std::fprintf(stderr, "[CaptureOverlay] Exiting with code 3 for window capture\n");
                    releaseKeyboard();
                    hide();
                    QApplication::exit(3); // special code: window capture via DBus
                } else {
                    std::fprintf(stderr, "[CaptureOverlay] Entering X11 window mode\n");
                    m_captureIntent = CaptureIntent::Area;
                    enterWindowMode();
                }
                return true;
            } else if (toolIndex == 4) {
                exitScrollMode();
                m_captureIntent = CaptureIntent::Area;
                update();
                showWebScrollCaptureInfo(this);
                return true;
            } else if (toolIndex == 6) {
                // OCR: enter OCR intent mode and wait for Enter/Space/double-click.
                exitScrollMode();
                m_captureIntent = CaptureIntent::Ocr;
                update();
                return true;
            } else {
                // All other tools: confirm/capture
                exitScrollMode();
                m_captureIntent = CaptureIntent::Area;
                confirmSelection();
                return true;
            }
        };

        if (clickInsideOrOnSelection) {
            ToolbarLayout layout = computeToolbarLayout(
                sel.x(),
                sel.y(),
                sel.width(),
                sel.height(),
                width(),
                height(),
                m_captureIntent == CaptureIntent::Scroll
            );
            for (int i = 0; i < NUM_TOOLS; ++i) {
                if (layout.itemCells[i].contains(pos)) {
                    std::fprintf(stderr, "[CaptureOverlay] Tool clicked (inside): index=%d\n", i);
                    handleToolClick(i);
                    return;
                }
            }
        } else {
            ToolbarLayout layout = computeToolbarLayout(
                sel.x(),
                sel.y(),
                sel.width(),
                sel.height(),
                width(),
                height(),
                m_captureIntent == CaptureIntent::Scroll
            );
            bool clickedToolbar = layout.toolsPanel.contains(pos) ||
                                  layout.sizePanel.contains(pos);
            if (clickedToolbar) {
                for (int i = 0; i < NUM_TOOLS; ++i) {
                    if (layout.itemCells[i].contains(pos)) {
                        handleToolClick(i);
                        return;
                    }
                }
                // Clicked toolbar panel background but not a specific tool —
                // do nothing (don't start a new selection from here).
                return;
            }

            // Click is outside selection AND outside toolbar — start fresh selection.
            m_dragging = true;
            m_moving = false;
            m_resizing = HandlePos::None;
            m_hasSelection = false;
            m_selection = QRect(pos, pos);
            m_dragStart = pos;
            setCursor(Qt::CrossCursor);
            update();
            return;
        }
    }

    m_dragStart = pos;

    if (m_captureIntent == CaptureIntent::Scroll && m_scrollStage == ScrollStage::Capturing) {
        return;
    }

    if (m_hasSelection) {
        HandlePos h = hitTest(pos);
        if (h == HandlePos::Inside) {
            m_moving = true;
            m_selectionAtDragStart = m_selection.normalized();
            setCursor(Qt::SizeAllCursor);
            return;
        } else if (h != HandlePos::None) {
            m_resizing = h;
            m_selectionAtDragStart = m_selection.normalized();
            return;
        }
    }

    m_dragging = true;
    m_hasSelection = false;
    m_fullscreenMode = false;
    m_selection = QRect(pos, pos);
    setCursor(Qt::CrossCursor);
}

void CaptureOverlay::mouseMoveEvent(QMouseEvent* event)
{
    const QPoint pos = event->pos();

    // Window mode — highlight the window under the cursor
    if (m_windowMode) {
        int newHover = -1;
        for (int i = 0; i < m_windows.size(); ++i) {
            if (m_windows[i].rect.contains(pos)) {
                newHover = i;
                break; // first (topmost) match wins
            }
        }
        if (newHover != m_hoveredWindow) {
            m_hoveredWindow = newHover;
            update();
        }
        return;
    }

    if (m_dragging) {
        m_selection = QRect(m_dragStart, pos);
        m_hasSelection = true;
        update();
        return;
    }

    if (m_moving) {
        QPoint delta = pos - m_dragStart;
        QRect newSel = m_selectionAtDragStart.translated(delta);
        const QRect bounds = rect();
        if (newSel.left() < bounds.left())     newSel.moveLeft(bounds.left());
        if (newSel.top()  < bounds.top())      newSel.moveTop(bounds.top());
        if (newSel.right()  > bounds.right())  newSel.moveRight(bounds.right());
        if (newSel.bottom() > bounds.bottom()) newSel.moveBottom(bounds.bottom());
        m_selection = newSel;
        update();
        return;
    }

    if (m_resizing != HandlePos::None) {
        QPoint delta = pos - m_dragStart;
        QRect r = m_selectionAtDragStart.normalized();
        switch (m_resizing) {
        case HandlePos::TopLeft:     r.setTopLeft(r.topLeft() + delta);         break;
        case HandlePos::Top:         r.setTop(r.top() + delta.y());             break;
        case HandlePos::TopRight:    r.setTopRight(r.topRight() + delta);       break;
        case HandlePos::Right:       r.setRight(r.right() + delta.x());         break;
        case HandlePos::BottomRight: r.setBottomRight(r.bottomRight() + delta); break;
        case HandlePos::Bottom:      r.setBottom(r.bottom() + delta.y());       break;
        case HandlePos::BottomLeft:  r.setBottomLeft(r.bottomLeft() + delta);   break;
        case HandlePos::Left:        r.setLeft(r.left() + delta.x());           break;
        default: break;
        }
        r = r.intersected(rect());
        if (r.width()  < kMinSize) r.setWidth(kMinSize);
        if (r.height() < kMinSize) r.setHeight(kMinSize);
        m_selection = r;
        update();
        return;
    }

    // Hover — update toolbar highlight + cursor
    if (m_hasSelection) {
        const QRect sel = m_selection.normalized();
        ToolbarLayout layout = computeToolbarLayout(
            sel.x(),
            sel.y(),
            sel.width(),
            sel.height(),
            width(),
            height(),
            m_captureIntent == CaptureIntent::Scroll
        );
        int newHover = -1;
        for (int i = 0; i < NUM_TOOLS; ++i) {
            if (layout.itemCells[i].contains(pos)) { newHover = i; break; }
        }
        bool newSizeHover = layout.sizePanel.contains(pos);
        if (newHover != m_hoveredTool || newSizeHover != m_hoveredSizePanel) {
            m_hoveredTool = newHover;
            m_hoveredSizePanel = newSizeHover;
            update();
        }
    }
    updateCursor(pos);
}

void CaptureOverlay::mouseReleaseEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) return;

    if (m_dragging) {
        m_dragging = false;
        QRect norm = m_selection.normalized();
        if (norm.width() < kMinSize || norm.height() < kMinSize)
            m_hasSelection = false;
        else { m_selection = norm; m_hasSelection = true; }
        update();
    }
    m_moving = false;
    m_resizing = HandlePos::None;
    updateCursor(event->pos());
}

void CaptureOverlay::mouseDoubleClickEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) return;
    if (!m_hasSelection) return;

    const QPoint pos = event->pos();
    // Only confirm if the double-click is actually inside the selection rect.
    // Double-clicking outside should start a new selection, not confirm — the
    // mouseDoubleClickEvent arrives instead of the second mousePressEvent, so
    // we need to replicate the "click outside → new selection" logic here.
    const QRect sel = m_selection.normalized();
    if (sel.contains(pos)) {
        if (m_captureIntent == CaptureIntent::Scroll) {
            return;
        }
        // Double-click inside the selection → confirm (take screenshot)
        confirmSelection();
    } else {
        // Double-click outside the selection → treat like a press: start fresh.
        // Check toolbar first.
        ToolbarLayout layout = computeToolbarLayout(
            sel.x(),
            sel.y(),
            sel.width(),
            sel.height(),
            width(),
            height(),
            m_captureIntent == CaptureIntent::Scroll
        );
        bool clickedToolbar = layout.toolsPanel.contains(pos) ||
                              layout.sizePanel.contains(pos);
        if (clickedToolbar) {
            for (int i = 0; i < NUM_TOOLS; ++i) {
                if (layout.itemCells[i].contains(pos)) {
                    // Reuse same handleToolClick logic
                    if (i == 2) {
                        exitScrollMode();
                        m_selection = QRect(0, 0, width(), height());
                        m_hasSelection = true;
                        m_fullscreenMode = true;
                        m_captureIntent = CaptureIntent::Area;
                        update();
                    } else if (i == 1) {
                        exitScrollMode();
                        int defaultW = std::max(kMinSize, std::min(DEFAULT_SELECTION_W, width()));
                        int defaultH = std::max(kMinSize, std::min(DEFAULT_SELECTION_H, height()));
                        m_selection = QRect((width()-defaultW)/2, (height()-defaultH)/2, defaultW, defaultH);
                        m_hasSelection = true;
                        m_fullscreenMode = false;
                        m_captureIntent = CaptureIntent::Area;
                        update();
                    } else if (i == 4) {
                        exitScrollMode();
                        m_captureIntent = CaptureIntent::Area;
                        update();
                        showWebScrollCaptureInfo(this);
                    } else if (i == 6) {
                        exitScrollMode();
                        m_captureIntent = CaptureIntent::Ocr;
                        update();
                    } else {
                        exitScrollMode();
                        m_captureIntent = CaptureIntent::Area;
                        confirmSelection();
                    }
                    return;
                }
            }
            return; // Clicked toolbar background — do nothing
        }
        // Outside selection and toolbar — start a new selection drag.
        m_dragging = true;
        m_moving = false;
        m_resizing = HandlePos::None;
        m_hasSelection = false;
        m_fullscreenMode = false;
        m_selection = QRect(pos, pos);
        m_dragStart = pos;
        setCursor(Qt::CrossCursor);
        update();
    }
}

// ── Keyboard ──────────────────────────────────────────────────────────────────

void CaptureOverlay::keyPressEvent(QKeyEvent* event)
{
    if (m_captureIntent == CaptureIntent::Scroll) {
        switch (event->key()) {
        case Qt::Key_Escape:
            cancelSelection();
            return;
        case Qt::Key_Return:
        case Qt::Key_Enter:
        case Qt::Key_Space:
            if (m_scrollStage == ScrollStage::Armed) {
                handleScrollButtonClick(scrollPrimaryButtonRect().center().toPoint());
                return;
            }
            if (m_scrollStage == ScrollStage::Capturing) {
                stopAutoScrollCapture(true);
                return;
            }
            break;
        default:
            break;
        }

        if (m_scrollStage == ScrollStage::Capturing) {
            return;
        }
    }

    bool shift = event->modifiers() & Qt::ShiftModifier;
    switch (event->key()) {
    case Qt::Key_Escape:
        if (m_windowMode) { exitWindowMode(); } else { cancelSelection(); }
        break;
    case Qt::Key_Return:
    case Qt::Key_Enter:
    case Qt::Key_Space:
        if (m_hasSelection) confirmSelection(); break;
    case Qt::Key_Left:
        if (m_hasSelection) {
            if (shift) m_selection.setRight(m_selection.right()-1);
            else       m_selection.translate(-1,0);
            update();
        } break;
    case Qt::Key_Right:
        if (m_hasSelection) {
            if (shift) m_selection.setRight(m_selection.right()+1);
            else       m_selection.translate(1,0);
            update();
        } break;
    case Qt::Key_Up:
        if (m_hasSelection) {
            if (shift) m_selection.setBottom(m_selection.bottom()-1);
            else       m_selection.translate(0,-1);
            update();
        } break;
    case Qt::Key_Down:
        if (m_hasSelection) {
            if (shift) m_selection.setBottom(m_selection.bottom()+1);
            else       m_selection.translate(0,1);
            update();
        } break;
    default: QWidget::keyPressEvent(event);
    }
}

QRectF CaptureOverlay::scrollPrimaryButtonRect() const
{
    const QRect sel = m_selection.normalized();
    if (sel.isEmpty()) {
        return QRectF();
    }

    double buttonW = std::max(
        SCROLL_BUTTON_MIN_W,
        std::min(sel.width() * 0.66, 220.0)
    );
    buttonW = std::min(buttonW, std::max(96.0, sel.width() - 12.0));

    double buttonX = sel.x() + (sel.width() - buttonW) / 2.0;
    double buttonY = sel.bottom() - SCROLL_BUTTON_H - 10.0;
    if (buttonY < sel.top() + 6.0) {
        buttonY = sel.bottom() + FEATURE_PANEL_TOP_GAP;
    }

    buttonX = std::max(FEATURE_PANEL_MARGIN,
              std::min(buttonX, width() - buttonW - FEATURE_PANEL_MARGIN));
    buttonY = std::max(FEATURE_PANEL_MARGIN,
              std::min(buttonY, height() - SCROLL_BUTTON_H - FEATURE_PANEL_MARGIN));

    return QRectF(buttonX, buttonY, buttonW, SCROLL_BUTTON_H);
}

void CaptureOverlay::enterScrollMode()
{
    if (!m_hasSelection) {
        return;
    }

    exitWindowMode();
    if (m_scrollCaptureTimer && m_scrollCaptureTimer->isActive()) {
        m_scrollCaptureTimer->stop();
    }

    for (const QString& path : m_scrollFramePaths) {
        QFile::remove(path);
    }
    m_scrollFramePaths.clear();

    if (!m_scrollCapturePath.isEmpty() && !m_scrollCaptureReady) {
        QFile::remove(m_scrollCapturePath);
    }

    m_scrollCapturePath.clear();
    m_scrollCaptureSize = QSize();
    m_scrollCaptureReady = false;
    m_scrollStage = ScrollStage::Armed;
    m_manualScrollAssistMode = false;
    setAttribute(Qt::WA_TransparentForMouseEvents, false);
    m_captureIntent = CaptureIntent::Scroll;
    m_fullscreenMode = false;
    m_dragging = false;
    m_moving = false;
    m_resizing = HandlePos::None;
    update();
}

void CaptureOverlay::exitScrollMode(bool keepIntent)
{
    if (m_scrollCaptureTimer && m_scrollCaptureTimer->isActive()) {
        m_scrollCaptureTimer->stop();
    }

    for (const QString& path : m_scrollFramePaths) {
        QFile::remove(path);
    }
    m_scrollFramePaths.clear();

    if (!m_scrollCaptureReady && !m_scrollCapturePath.isEmpty()) {
        QFile::remove(m_scrollCapturePath);
    }

    if (!keepIntent) {
        m_scrollCapturePath.clear();
        m_scrollCaptureSize = QSize();
        m_scrollCaptureReady = false;
        m_captureIntent = CaptureIntent::Area;
    }

    m_scrollStage = ScrollStage::Inactive;
    m_manualScrollAssistMode = false;
    setAttribute(Qt::WA_TransparentForMouseEvents, false);
    update();
}

bool CaptureOverlay::handleScrollButtonClick(const QPoint& pos)
{
    if (m_captureIntent != CaptureIntent::Scroll || !m_hasSelection) {
        return false;
    }

    if (m_scrollStage == ScrollStage::Armed) {
        if (!scrollPrimaryButtonRect().contains(pos)) {
            return false;
        }
        startAutoScrollCapture();
        return true;
    }

    return false;
}

// ── Auto-scroll capture methods ──────────────────────────────────────────────

void CaptureOverlay::startAutoScrollCapture()
{
    if (!m_hasSelection) return;

    m_scrollCaptureReady = false;
    m_scrollCapturePath.clear();
    m_scrollCaptureSize = QSize();
    for (const QString& framePath : m_scrollFramePaths) {
        QFile::remove(framePath);
    }
    m_scrollFramePaths.clear();

    m_dragging = false;
    m_moving = false;
    m_resizing = HandlePos::None;
    m_scrollStage = ScrollStage::Capturing;
    m_scrollSimilarCount = 0;
    m_scrollFrameCount = 0;
    m_scrollCaptureArea = m_selection.normalized().translated(geometry().topLeft());
    m_manualScrollAssistMode = shouldUseManualScrollAssistMode();

    if (m_manualScrollAssistMode) {
        setAttribute(Qt::WA_TransparentForMouseEvents, true);
        releaseKeyboard();
        hide();
    } else {
        setAttribute(Qt::WA_TransparentForMouseEvents, false);
    }
    QApplication::processEvents();

    // Position and show the control panel
    m_scrollControlPanel->setFrameCount(0);
    m_scrollControlPanel->setStatus(m_manualScrollAssistMode
        ? QStringLiteral("Scroll manually, then click Done")
        : QStringLiteral("Capturing…"));
    m_scrollControlPanel->positionNear(m_scrollCaptureArea, size());
    m_scrollControlPanel->show();

    if (!m_manualScrollAssistMode) {
        callDaemonBool(QStringLiteral("ScrollBeginGnome"));
    }
    update();

    if (!m_manualScrollAssistMode) {
        m_scrollControlPanel->hide();
        QApplication::processEvents();
    }

    // Capture the first frame immediately
    if (captureScrollFrameSilent()) {
        m_scrollControlPanel->setFrameCount(m_scrollFrameCount);
        if (!m_manualScrollAssistMode) {
            m_scrollControlPanel->show();
            m_scrollControlPanel->raise();
        }
    }

    update();
    QApplication::processEvents();

    // Start the auto-scroll loop
    if (m_scrollCaptureTimer) {
        m_scrollCaptureTimer->start();
    }
}

void CaptureOverlay::simulateScrollDown()
{
    int targetX = m_scrollCaptureArea.x() + m_scrollCaptureArea.width() / 2;
    int targetY = m_scrollCaptureArea.y() + m_scrollCaptureArea.height() / 2;

    const bool hadTransparentMouse = testAttribute(Qt::WA_TransparentForMouseEvents);
    if (!hadTransparentMouse) {
        setAttribute(Qt::WA_TransparentForMouseEvents, true);
    }
    releaseKeyboard();
    QApplication::processEvents();

    const bool daemonScrolled = callDaemonScrollStep(targetX, targetY, kScrollLinesPerTick);

    if (!hadTransparentMouse) {
        setAttribute(Qt::WA_TransparentForMouseEvents, false);
    }
    QApplication::processEvents();

    if (daemonScrolled) {
        std::fprintf(stderr, "[CaptureOverlay] Auto-scroll: portal step accepted, continuing with local fallback\n");
    }

    QString ydotoolPath;
    QStringList ydotoolPaths = {"/usr/local/bin/ydotool", "/usr/bin/ydotool", "ydotool"};
    for (const QString& path : ydotoolPaths) {
        QProcess testProc;
        testProc.setProgram(path);
        testProc.setArguments({"help"});
        testProc.start();
        if (testProc.waitForFinished(500) && testProc.exitCode() == 0) {
            ydotoolPath = path;
            break;
        }
    }

    QString wtypePath;
    QStringList wtypePaths = {"/usr/local/bin/wtype", "/usr/bin/wtype", "wtype"};
    for (const QString& candidate : wtypePaths) {
        if (candidate.startsWith('/')) {
            QFileInfo fi(candidate);
            if (fi.exists() && fi.isExecutable()) {
                wtypePath = fi.absoluteFilePath();
                break;
            }
            continue;
        }
        const QString resolved = QStandardPaths::findExecutable(candidate);
        if (!resolved.isEmpty()) {
            wtypePath = resolved;
            break;
        }
    }

    QProcessEnvironment env;
    env.insert("YDOTOOL_SOCKET", "/tmp/.ydotool_socket");

    const bool canUseYdotool = !ydotoolPath.isEmpty() && QFileInfo("/dev/uinput").isWritable();
    if (!ydotoolPath.isEmpty() && !canUseYdotool) {
        std::fprintf(stderr,
                     "[CaptureOverlay] Auto-scroll: ydotool detected but /dev/uinput is not writable; skipping ydotool path\n");
    }

    clearFocus();
    releaseKeyboard();
    QApplication::processEvents();

    const bool hadTransparentMouseFallback = testAttribute(Qt::WA_TransparentForMouseEvents);
    if (!hadTransparentMouseFallback) {
        setAttribute(Qt::WA_TransparentForMouseEvents, true);
        QApplication::processEvents();
    }

    auto restoreMouseCapture = [&]() {
        if (!hadTransparentMouseFallback) {
            setAttribute(Qt::WA_TransparentForMouseEvents, false);
            QApplication::processEvents();
        }
    };

    if (!wtypePath.isEmpty()) {
        if (canUseYdotool) {
            QProcess moveProc;
            moveProc.setProgram(ydotoolPath);
            moveProc.setProcessEnvironment(env);
            moveProc.setArguments({"mousemove", QString::number(targetX), QString::number(targetY)});
            moveProc.start();
            moveProc.waitForFinished(200);

            QProcess clickProc;
            clickProc.setProgram(ydotoolPath);
            clickProc.setProcessEnvironment(env);
            clickProc.setArguments({"click", "1"});
            clickProc.start();
            clickProc.waitForFinished(150);
            QThread::msleep(60);
        }

        bool sent = false;
        const int burst = std::max(1, kScrollLinesPerTick * 2);
        for (int i = 0; i < burst; ++i) {
            QProcess textProc;
            textProc.setProgram(wtypePath);
            textProc.setArguments({" "});
            textProc.start();
            if (textProc.waitForFinished(180) && textProc.exitCode() == 0) {
                sent = true;
            } else {
                break;
            }
        }

        if (sent) {
            restoreMouseCapture();
            std::fprintf(stderr,
                         "[CaptureOverlay] Auto-scroll: wtype text-space sent=true\n");
            return;
        }

        const QStringList keyVariants = {QStringLiteral("Page_Down"),
                                         QStringLiteral("Next"),
                                         QStringLiteral("pagedown")};
        const int pageDownCount = std::max(1, kScrollLinesPerTick);
        for (int i = 0; i < pageDownCount; ++i) {
            bool sentThisTick = false;
            for (const QString& keyName : keyVariants) {
                QProcess keyProc;
                keyProc.setProgram(wtypePath);
                keyProc.setProcessEnvironment(env);
                keyProc.setArguments({"-k", keyName});
                keyProc.start();
                if (keyProc.waitForFinished(180) && keyProc.exitCode() == 0) {
                    sent = true;
                    sentThisTick = true;
                    break;
                }
            }
            if (!sentThisTick) {
                break;
            }
        }

        restoreMouseCapture();
        std::fprintf(stderr,
                     "[CaptureOverlay] Auto-scroll: wtype PageDown sent=%s\n",
                     sent ? "true" : "false");
        return;
    }

    if (canUseYdotool) {
        QProcess moveProc;
        moveProc.setProgram(ydotoolPath);
        moveProc.setProcessEnvironment(env);
        moveProc.setArguments({"mousemove", QString::number(targetX), QString::number(targetY)});
        moveProc.start();
        moveProc.waitForFinished(200);

        QProcess clickProc;
        clickProc.setProgram(ydotoolPath);
        clickProc.setProcessEnvironment(env);
        clickProc.setArguments({"click", "1"});
        clickProc.start();
        clickProc.waitForFinished(150);

        QThread::msleep(60);

        const int wheelSteps = std::max(1, kScrollLinesPerTick);
        for (int i = 0; i < wheelSteps; ++i) {
            QProcess wheelProc;
            wheelProc.setProgram(ydotoolPath);
            wheelProc.setProcessEnvironment(env);
            wheelProc.setArguments({"click", "5"});
            wheelProc.start();
            wheelProc.waitForFinished(90);
        }

        restoreMouseCapture();
        std::fprintf(stderr, "[CaptureOverlay] Auto-scroll: ydotool wheel-down sent\n");
        return;
    }

    Display* display = XOpenDisplay(nullptr);
    if (!display) {
        restoreMouseCapture();
        std::fprintf(stderr, "[CaptureOverlay] Auto-scroll: XOpenDisplay failed\n");
        return;
    }

    XTestFakeMotionEvent(display, DefaultScreen(display), targetX, targetY, CurrentTime);
    XFlush(display);
    XTestFakeButtonEvent(display, 1, True, CurrentTime);
    XTestFakeButtonEvent(display, 1, False, CurrentTime);
    XFlush(display);
    QThread::msleep(50);

    for (int i = 0; i < kScrollLinesPerTick; ++i) {
        XTestFakeButtonEvent(display, 5, True, CurrentTime);
        XTestFakeButtonEvent(display, 5, False, CurrentTime);
        XFlush(display);
        QThread::msleep(50);
    }

    XCloseDisplay(display);
    restoreMouseCapture();
    std::fprintf(stderr, "[CaptureOverlay] Auto-scroll: X11 wheel-down simulation done\n");
}

void CaptureOverlay::onAutoScrollTick()
{
    if (m_scrollStage != ScrollStage::Capturing) {
        return;
    }

    // 1. Scroll progression
    if (!m_manualScrollAssistMode) {
        simulateScrollDown();
    }

    update();
    QApplication::processEvents();

    // 2. Wait for applications to render the scrolled content (smooth scrolling, etc)
    QThread::msleep(kScrollSettleMs);

    // 3. Capture the new frame
    if (!m_manualScrollAssistMode) {
        m_scrollControlPanel->hide();
        QApplication::processEvents();
    }

    bool captured = false;
    if (captureScrollFrameSilent()) {
        m_scrollControlPanel->setFrameCount(m_scrollFrameCount);
        captured = true;
    }

    if (captured && !m_manualScrollAssistMode) {
        // Show control panel briefly after capture to show progress
        m_scrollControlPanel->show();
        m_scrollControlPanel->raise();
    }

    // 4. Check stop conditions
    const int similarThreshold = m_manualScrollAssistMode
        ? kManualSimilarStopThreshold
        : kSimilarStopThreshold;
    if (m_scrollSimilarCount >= similarThreshold) {
        if (m_manualScrollAssistMode) {
            std::fprintf(stderr, "[CaptureOverlay] Manual scroll assist: inactivity reached, finalizing.\n");
        } else {
            std::fprintf(stderr, "[CaptureOverlay] Auto-scroll reached end of content.\n");
        }
        stopAutoScrollCapture(true);
        return;
    }

    if (m_scrollFrameCount >= kMaxScrollFrames) {
        std::fprintf(stderr, "[CaptureOverlay] Auto-scroll reached max frames (%d).\n", kMaxScrollFrames);
        stopAutoScrollCapture(true);
        return;
    }

    // 5. Schedule next capture using single-shot timer (prevents overlapping captures)
    // Total interval = settle time + processing time + small buffer
    const int nextTickDelay = SCROLL_CAPTURE_INTERVAL_MS;
    if (m_scrollCaptureTimer) {
        m_scrollCaptureTimer->start(nextTickDelay);
    }
}

bool CaptureOverlay::captureScrollFrameSilent()
{
    if (m_scrollCaptureArea.width() <= 0 || m_scrollCaptureArea.height() <= 0) {
        return false;
    }

    QString imagePath;
    QSize imageSize;
    QString error;

    // Overlay stays visible; capture-safe rendering avoids drawing capture UI inside the capture area.
    const bool ok = ScreenCapture::captureAreaToTempPng(m_scrollCaptureArea, imagePath, imageSize, error);

    if (!ok) {
        std::fprintf(stderr,
                     "[CaptureOverlay] Scroll frame capture silent failed: %s\n",
                     error.toLocal8Bit().constData());
        return false;
    }

    QImage image(imagePath);
    if (image.isNull()) {
        QFile::remove(imagePath);
        return false;
    }

    // Compare with the previous frame to see if we've stopped moving
    if (!m_scrollFramePaths.isEmpty()) {
        const QImage previous(m_scrollFramePaths.back());
        if (!previous.isNull() && imagesSimilar(previous, image, 2.0)) {
            QFile::remove(imagePath);
            m_scrollSimilarCount++;
            std::fprintf(stderr, "[CaptureOverlay] Frame similar to previous (count=%d), diff=%.1f\n", 
                m_scrollSimilarCount, getImageDiff(previous, image));
            return true; // it was captured, but it is similar
        }
    }

    // It's a new unique frame
    m_scrollSimilarCount = 0;
    m_scrollFrameCount++;
    m_scrollFramePaths.push_back(imagePath);
    std::fprintf(stderr,
        "[CaptureOverlay] Captured unique frame #%d: %s\n",
        m_scrollFrameCount, imagePath.toLocal8Bit().constData());
    return true;
}

void CaptureOverlay::stopAutoScrollCapture(bool finalize)
{
    if (!m_manualScrollAssistMode) {
        callDaemonVoid(QStringLiteral("ScrollEndGnome"));
    }
    setAttribute(Qt::WA_TransparentForMouseEvents, false);
    QApplication::processEvents();

    if (m_scrollCaptureTimer && m_scrollCaptureTimer->isActive()) {
        m_scrollCaptureTimer->stop();
    }

    m_scrollControlPanel->setCapturingDone();
    QApplication::processEvents();

    if (finalize) {
        if (finalizeScrollCapture()) {
            m_scrollControlPanel->hide();
            confirmSelection();
        } else {
            // Stitching failed
            m_scrollStage = ScrollStage::Armed;
            m_scrollControlPanel->hide();
            show(); // Show main overlay again
            raise();
            activateWindow();
            grabKeyboard();
            update();
        }
    } else {
        // Cancelled
        m_scrollStage = ScrollStage::Armed;
        m_scrollControlPanel->hide();
        show(); // Show main overlay again
        raise();
        activateWindow();
        grabKeyboard();
        update();
    }
}

bool CaptureOverlay::finalizeScrollCapture()
{

    QString stitchedPath;
    QSize stitchedSize;
    QString stitchError;
    const bool stitched = stitchScrollFrames(stitchedPath, stitchedSize, stitchError);

    for (const QString& framePath : m_scrollFramePaths) {
        QFile::remove(framePath);
    }
    m_scrollFramePaths.clear();

    if (!stitched) {
        std::fprintf(stderr,
                     "[CaptureOverlay] Scroll stitching failed: %s\n",
                     stitchError.toLocal8Bit().constData());
        if (!stitchedPath.isEmpty()) {
            QFile::remove(stitchedPath);
        }
        return false;
    }

    m_scrollCapturePath = stitchedPath;
    m_scrollCaptureSize = stitchedSize;
    m_scrollCaptureReady = true;
    m_scrollStage = ScrollStage::Inactive;
    return true;
}

bool CaptureOverlay::stitchScrollFrames(QString& outPath,
                                        QSize& outSize,
                                        QString& outError) const
{
    if (m_scrollFramePaths.isEmpty()) {
        outError = QStringLiteral("No captured scroll frames to stitch");
        return false;
    }

    std::fprintf(stderr, "[CaptureOverlay] Stitching %d frames...\n", m_scrollFramePaths.size());

    QList<QImage> frames;
    frames.reserve(m_scrollFramePaths.size());
    int targetWidth = std::numeric_limits<int>::max();

    for (const QString& path : m_scrollFramePaths) {
        QImage frame(path);
        if (frame.isNull()) {
            std::fprintf(stderr, "[CaptureOverlay] Warning: Failed to load frame: %s\n", qPrintable(path));
            continue;
        }
        frame = frame.convertToFormat(QImage::Format_ARGB32);
        targetWidth = std::min(targetWidth, frame.width());
        std::fprintf(stderr, "[CaptureOverlay] Frame size: %dx%d\n", frame.width(), frame.height());
        frames.push_back(frame);
    }

    if (frames.isEmpty()) {
        outError = QStringLiteral("Captured scroll frames are unreadable");
        return false;
    }

    if (targetWidth <= 0) {
        outError = QStringLiteral("Captured scroll frame width is invalid");
        return false;
    }

    for (QImage& frame : frames) {
        if (frame.width() > targetWidth) {
            frame = frame.copy(0, 0, targetWidth, frame.height());
        }
    }

    QImage stitched = frames.first();
    QImage previous = frames.first();
    int totalHeight = stitched.height();

    for (int i = 1; i < frames.size(); ++i) {
        const QImage next = frames[i];
        if (next.isNull()) {
            continue;
        }

        int overlap = estimateOverlapRows(previous, next);
        overlap = std::max(0, std::min(overlap, next.height() - 1));
        
        std::fprintf(stderr,
            "[CaptureOverlay] Frame %d overlap=%d append=%d prevH=%d nextH=%d\n",
            i, overlap, next.height() - overlap, previous.height(), next.height());
        
        const int appendHeight = next.height() - overlap;
        if (appendHeight <= 2) {
            std::fprintf(stderr, "[CaptureOverlay] Frame %d: skipping (too small append)\n", i);
            previous = next;
            continue;
        }

        QImage combined(stitched.width(),
                        stitched.height() + appendHeight,
                        QImage::Format_ARGB32);
        combined.fill(Qt::transparent);

        {
            QPainter painter(&combined);
            painter.drawImage(0, 0, stitched);
            painter.drawImage(0,
                              stitched.height(),
                              next,
                              0,
                              overlap,
                              next.width(),
                              appendHeight);
        }

        stitched = combined;
        totalHeight += appendHeight;
        previous = next;
    }

    std::fprintf(stderr, "[CaptureOverlay] Final stitched size: %dx%d\n", stitched.width(), stitched.height());

    const QString tempPath =
      QDir::tempPath() +
      QStringLiteral("/apexshot-scroll-%1.png")
        .arg(QDateTime::currentMSecsSinceEpoch());

    if (!stitched.save(tempPath, "PNG")) {
        outError = QStringLiteral("Failed to save stitched scroll capture");
        return false;
    }

    outPath = tempPath;
    outSize = stitched.size();
    return true;
}

bool CaptureOverlay::imagesSimilar(const QImage& a, const QImage& b, double threshold)
{
    if (a.isNull() || b.isNull()) {
        return false;
    }
    if (a.size() != b.size()) {
        return false;
    }
    const int overlap = std::min(a.height(), b.height());
    if (overlap <= 0) {
        return false;
    }
    return overlapDiffScore(a, b, overlap) <= threshold;
}

double CaptureOverlay::getImageDiff(const QImage& a, const QImage& b)
{
    if (a.isNull() || b.isNull()) {
        return 1000.0;
    }
    if (a.size() != b.size()) {
        return 1000.0;
    }
    const int overlap = std::min(a.height(), b.height());
    if (overlap <= 0) {
        return 1000.0;
    }
    return overlapDiffScore(a, b, overlap);
}

int CaptureOverlay::estimateOverlapRows(const QImage& prev, const QImage& next)
{
    if (prev.isNull() || next.isNull()) {
        return 0;
    }

    const int maxOverlap = std::min(prev.height(), next.height());
    if (maxOverlap <= 8) {
        return 0;
    }

    // Use template matching approach: find where the bottom of prev matches in next
    // This is more robust than assuming a fixed overlap
    
    const int width = std::min(prev.width(), next.width());
    const int templateHeight = std::min(100, maxOverlap / 2);  // Use 100px or half of max as template
    
    // Extract template from bottom of previous frame
    QImage prevRgba = prev.convertToFormat(QImage::Format_RGBA8888);
    QImage nextRgba = next.convertToFormat(QImage::Format_RGBA8888);
    
    if (prevRgba.isNull() || nextRgba.isNull()) {
        return 0;
    }
    
    // Search range: look for the template in the top half of next image
    const int searchStart = 0;
    const int searchEnd = std::min(maxOverlap - templateHeight, nextRgba.height() - templateHeight);
    
    if (searchEnd <= searchStart) {
        return 0;
    }
    
    double bestScore = std::numeric_limits<double>::max();
    int bestOffset = 0;
    
    // Try each possible offset
    for (int offset = searchStart; offset <= searchEnd; offset += 2) {
        double score = computeTemplateMatchScore(
            prevRgba, prevRgba.height() - templateHeight, templateHeight,
            nextRgba, offset, width);
        
        if (score < bestScore) {
            bestScore = score;
            bestOffset = offset;
        }
    }
    
    // If best match score is too high, no valid overlap found
    // Use per-pixel threshold: average difference per pixel should be reasonable
    const double avgDiffPerPixel = bestScore / (width * templateHeight);
    if (avgDiffPerPixel > 15.0) {  // More than 15 avg difference per pixel = no match
        return 0;
    }
    
    // The overlap is where the template from prev matches in next
    // bestOffset is the Y position in next where the match occurs
    // The template from the bottom of prev was found at 'bestOffset' in next
    //
    // If bestOffset is small: the template from bottom of prev is near top of next
    //   -> we scrolled a lot -> small overlap
    // If bestOffset is large: the template from bottom of prev is near bottom of next
    //   -> we scrolled a little -> large overlap
    //
    // The overlap is approximately bestOffset + templateHeight because:
    // - Content from prev.height()-templateHeight to prev.height() (the template)
    //   is now at bestOffset to bestOffset+templateHeight in next
    // - So content from 0 to bestOffset+templateHeight in next overlaps with prev
    
    const int expectedOverlap = maxOverlap / 2;  // Assume ~50% overlap
    const int detectedOverlap = bestOffset + templateHeight;
    
    // Validate the detected overlap is reasonable
    if (detectedOverlap < 20 || detectedOverlap > maxOverlap - 20) {
        // Overlap is too small or too large, use expected
        return std::max(50, maxOverlap / 3);
    }
    
    // If detected overlap is very different from expected, it might be a false match
    // Use a weighted average to be conservative
    if (std::abs(detectedOverlap - expectedOverlap) > maxOverlap / 3) {
        // Blend detected with expected to avoid extreme values
        return (detectedOverlap + expectedOverlap) / 2;
    }
    
    return detectedOverlap;
}

double CaptureOverlay::computeTemplateMatchScore(const QImage& prev, int prevY, int templateHeight,
                                              const QImage& next, int nextY, int width)
{
    if (width <= 0 || templateHeight <= 0) {
        return std::numeric_limits<double>::max();
    }
    
    const int stepX = std::max(1, width / 64);
    const int stepY = std::max(1, templateHeight / 8);
    
    double diffSum = 0.0;
    int sampleCount = 0;
    
    for (int y = 0; y < templateHeight; y += stepY) {
        const uchar* prevLine = prev.constScanLine(prevY + y);
        const uchar* nextLine = next.constScanLine(nextY + y);
        for (int x = 0; x < width; x += stepX) {
            const uchar* a = prevLine + x * 4;
            const uchar* b = nextLine + x * 4;
            // Compare RGB (skip alpha)
            diffSum += std::abs(int(a[0]) - int(b[0]));
            diffSum += std::abs(int(a[1]) - int(b[1]));
            diffSum += std::abs(int(a[2]) - int(b[2]));
            sampleCount += 3;
        }
    }
    
    return sampleCount > 0 ? diffSum / sampleCount : std::numeric_limits<double>::max();
}

double CaptureOverlay::overlapDiffScore(const QImage& prev,
                                        const QImage& next,
                                        int overlapRows)
{
    if (overlapRows <= 0) {
        return std::numeric_limits<double>::max();
    }

    const QImage prevRgba = prev.convertToFormat(QImage::Format_RGBA8888);
    const QImage nextRgba = next.convertToFormat(QImage::Format_RGBA8888);
    if (prevRgba.isNull() || nextRgba.isNull()) {
        return std::numeric_limits<double>::max();
    }

    const int width = std::min(prevRgba.width(), nextRgba.width());
    const int rows = std::min(overlapRows, std::min(prevRgba.height(), nextRgba.height()));
    if (width <= 0 || rows <= 0) {
        return std::numeric_limits<double>::max();
    }

    const int prevStartY = prevRgba.height() - rows;
    const int stepX = std::max(1, width / 64);
    const int stepY = std::max(1, rows / 42);

    double diffSum = 0.0;
    int sampleCount = 0;

    for (int y = 0; y < rows; y += stepY) {
        const uchar* prevLine = prevRgba.constScanLine(prevStartY + y);
        const uchar* nextLine = nextRgba.constScanLine(y);
        for (int x = 0; x < width; x += stepX) {
            const uchar* a = prevLine + x * 4;
            const uchar* b = nextLine + x * 4;
            diffSum += std::abs(int(a[0]) - int(b[0]));
            diffSum += std::abs(int(a[1]) - int(b[1]));
            diffSum += std::abs(int(a[2]) - int(b[2]));
            sampleCount += 3;
        }
    }

    if (sampleCount <= 0) {
        return std::numeric_limits<double>::max();
    }

    return diffSum / sampleCount;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

// ── Window enumeration (X11) ─────────────────────────────────────────────────

QList<CaptureOverlay::WindowInfo> CaptureOverlay::enumerateWindows() const
{
    QList<WindowInfo> result;
    Display* dpy = XOpenDisplay(nullptr);
    if (!dpy) return result;

    Window root = DefaultRootWindow(dpy);

    // Get _NET_CLIENT_LIST for proper window list (ordered, no hidden/desktop windows)
    Atom netClientList = XInternAtom(dpy, "_NET_CLIENT_LIST", true);
    if (netClientList == 0) {
        XCloseDisplay(dpy);
        return result;
    }

    Atom actualType;
    int actualFormat;
    unsigned long nItems, bytesAfter;
    unsigned char* data = nullptr;

    if (XGetWindowProperty(dpy, root, netClientList, 0, 1024, false,
                           XA_WINDOW, &actualType, &actualFormat,
                           &nItems, &bytesAfter, &data) == Success && data) {
        Window* windows = reinterpret_cast<Window*>(data);
        for (unsigned long i = 0; i < nItems; ++i) {
            Window win = windows[i];

            // Get window geometry in root coordinates
            Window child;
            int rx, ry;
            unsigned int rw, rh, bw, depth;
            if (!XGetGeometry(dpy, win, &root, &rx, &ry, &rw, &rh, &bw, &depth))
                continue;
            XTranslateCoordinates(dpy, win, DefaultRootWindow(dpy), 0, 0, &rx, &ry, &child);

            // Skip tiny or offscreen windows
            if ((int)rw < 50 || (int)rh < 50) continue;
            if (rx + (int)rw < 0 || ry + (int)rh < 0) continue;

            // Get window title via _NET_WM_NAME or WM_NAME
            QString title;
            Atom netWmName = XInternAtom(dpy, "_NET_WM_NAME", false);
            Atom utf8Str   = XInternAtom(dpy, "UTF8_STRING", false);
            unsigned char* nameProp = nullptr;
            Atom nameType; int nameFmt; unsigned long nameItems, nameAfter;
            if (XGetWindowProperty(dpy, win, netWmName, 0, 256, false, utf8Str,
                                   &nameType, &nameFmt, &nameItems, &nameAfter,
                                   &nameProp) == Success && nameProp) {
                title = QString::fromUtf8(reinterpret_cast<char*>(nameProp));
                XFree(nameProp);
            } else {
                char* wmName = nullptr;
                if (XFetchName(dpy, win, &wmName) && wmName) {
                    title = QString::fromLocal8Bit(wmName);
                    XFree(wmName);
                }
            }
            if (title.isEmpty()) title = "(Unnamed)";

            WindowInfo info;
            info.rect  = QRect(rx, ry, (int)rw, (int)rh);
            info.title = title;
            result.prepend(info); // prepend so topmost windows are first
        }
        XFree(data);
    }

    XCloseDisplay(dpy);
    return result;
}

void CaptureOverlay::enterWindowMode()
{
    exitScrollMode();
    m_windowMode    = true;
    m_fullscreenMode = false;
    m_captureIntent = CaptureIntent::Area;
    m_hasSelection  = false;
    m_hoveredWindow = -1;
    m_windows       = enumerateWindows();
    setCursor(Qt::CrossCursor);
    update();
}

void CaptureOverlay::exitWindowMode()
{
    m_windowMode    = false;
    m_hoveredWindow = -1;
    m_windows.clear();
    update();
}

void CaptureOverlay::confirmSelection()
{
    if (m_scrollCaptureTimer && m_scrollCaptureTimer->isActive()) {
        m_scrollCaptureTimer->stop();
    }
    releaseKeyboard();
    // Hide the overlay immediately so it doesn't appear in the screenshot
    hide();
    // Process all pending events so the compositor/X server has a chance
    // to actually remove our window from the screen before we exit.
    QApplication::processEvents();
    // Small delay to let the compositor flush the frame
    QThread::msleep(120);
    QApplication::exit(0);
}

void CaptureOverlay::cancelSelection()
{
    exitScrollMode();
    releaseKeyboard();
    QApplication::exit(1);
}

QRect CaptureOverlay::handleRect(const QPoint& center) const
{
    return QRect(center.x() - kHandleHitSize/2,
                 center.y() - kHandleHitSize/2,
                 kHandleHitSize, kHandleHitSize);
}

QList<QPoint> CaptureOverlay::handleCenters() const
{
    const QRect r = m_selection.normalized();
    int cx = r.left() + r.width()/2;
    int cy = r.top()  + r.height()/2;
    return { r.topLeft(),         QPoint(cx, r.top()),    r.topRight(),
             QPoint(r.right(),cy),
             r.bottomRight(),     QPoint(cx,r.bottom()),  r.bottomLeft(),
             QPoint(r.left(),cy) };
}

CaptureOverlay::HandlePos CaptureOverlay::hitTest(const QPoint& pos) const
{
    if (!m_hasSelection) return HandlePos::None;
    const QRect selection = m_selection.normalized();
    if (!selection.contains(pos)) return HandlePos::None;

    const QList<QPoint> centers = handleCenters();
    const HandlePos handles[] = {
        HandlePos::TopLeft,    HandlePos::Top,         HandlePos::TopRight,
        HandlePos::Right,
        HandlePos::BottomRight, HandlePos::Bottom,     HandlePos::BottomLeft,
        HandlePos::Left
    };
    for (int i = 0; i < 8; ++i)
        if (handleRect(centers[i]).contains(pos)) return handles[i];
    if (selection.contains(pos)) return HandlePos::Inside;
    return HandlePos::None;
}

void CaptureOverlay::updateCursor(const QPoint& pos)
{
    if (!m_hasSelection) { setCursor(Qt::CrossCursor); return; }

    if (m_captureIntent == CaptureIntent::Scroll && m_scrollStage == ScrollStage::Capturing) {
        setCursor(Qt::ArrowCursor);
        return;
    }

    const QRect sel = m_selection.normalized();
    ToolbarLayout layout = computeToolbarLayout(
        sel.x(),
        sel.y(),
        sel.width(),
        sel.height(),
        width(),
        height(),
        m_captureIntent == CaptureIntent::Scroll
    );
    for (int i = 0; i < NUM_TOOLS; ++i) {
        if (layout.itemCells[i].contains(pos)) {
            setCursor(Qt::PointingHandCursor);
            return;
        }
    }
    if (layout.sizePanel.contains(pos)) {
        setCursor(Qt::ArrowCursor);
        return;
    }

    if (m_captureIntent == CaptureIntent::Scroll) {
        if (m_scrollStage == ScrollStage::Armed && scrollPrimaryButtonRect().contains(pos)) {
            setCursor(Qt::PointingHandCursor);
            return;
        }
    }

    switch (hitTest(pos)) {
    case HandlePos::TopLeft:     setCursor(Qt::SizeFDiagCursor); break;
    case HandlePos::TopRight:    setCursor(Qt::SizeBDiagCursor); break;
    case HandlePos::BottomLeft:  setCursor(Qt::SizeBDiagCursor); break;
    case HandlePos::BottomRight: setCursor(Qt::SizeFDiagCursor); break;
    case HandlePos::Top:
    case HandlePos::Bottom:      setCursor(Qt::SizeVerCursor);   break;
    case HandlePos::Left:
    case HandlePos::Right:       setCursor(Qt::SizeHorCursor);   break;
    case HandlePos::Inside:      setCursor(Qt::SizeAllCursor);   break;
    default:                     setCursor(Qt::CrossCursor);     break;
    }
}
