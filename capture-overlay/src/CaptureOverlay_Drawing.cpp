#include "CaptureOverlay.h"
#include "CaptureOverlay_p.h"
#include <QPainter>
#include <QPainterPath>
#include <QPaintEvent>
#include <QFont>
#include <QFontMetrics>
#include <QImage>
#include <QPixmap>
#include <QColor>
#include <QLinearGradient>
#include <QRadialGradient>
#include <QPen>
#include <QDateTime>
#include <QMutexLocker>
#include <QTimer>
#include <algorithm>
#include <cmath>

const char* TOOLBAR_LABELS[NUM_TOOLS] = {
    "Area","Fullscreen","Window","Scroll","Timer","OCR","Recording"
};

const int TOOLBAR_ICON_IDS[NUM_TOOLS] = {
    1, 2, 3, 4, 5, 6, 7
};

namespace {
struct AspectRatioOption {
    const char* label;
    double ratio;
};

constexpr AspectRatioOption kRecordingAspectOptions[] = {
    {"Freeform", 0.0},
    {"1 : 1 (Square)", 1.0},
    {"5 : 4 (10 : 8)", 5.0 / 4.0},
    {"4 : 3", 4.0 / 3.0},
    {"7 : 5", 7.0 / 5.0},
    {"3 : 2", 3.0 / 2.0},
    {"16 : 10", 16.0 / 10.0},
    {"16 : 9", 16.0 / 9.0},
    {"2.35 : 1", 2.35},
    {"2 : 3", 2.0 / 3.0},
    {"9 : 16", 9.0 / 16.0},
};

constexpr int kRecordingAspectOptionCount =
    static_cast<int>(sizeof(kRecordingAspectOptions) / sizeof(kRecordingAspectOptions[0]));
}

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

ToolbarLayout computeToolbarLayout(double selX, double selY,
                                           double selW, double selH,
                                           double screenW, double screenH,
                                           bool forceAbove)
{
    ToolbarLayout layout;
    const double toolPanelH = TOOL_CARD_H * NUM_TOOLS;
    const double actionBarW = (2.0 * ACTION_RAIL_W) + ACTION_CARD_GAP;
    const double actionBarH = ACTION_CARD_H;
    const double sizeCardW = 152.0;
    const double cropCardW = 62.0;
    const double topClusterH = REC_TOP_CLUSTER_H;
    const double topGroupW = sizeCardW + ACTION_CARD_GAP + cropCardW;
    const double centerY = selY + (selH / 2.0);

    const double leftCandidateX = selX - TOOL_RAIL_GAP - TOOL_RAIL_W;
    const bool toolRailClamped = leftCandidateX < FEATURE_PANEL_MARGIN;
    const double leftPanelX = std::max(FEATURE_PANEL_MARGIN, leftCandidateX);

    const double leftPanelY = std::max(
        FEATURE_PANEL_MARGIN,
        std::min(centerY - (toolPanelH / 2.0), screenH - toolPanelH - FEATURE_PANEL_MARGIN)
    );

    const double preferredActionX = selX + (selW - actionBarW) / 2.0;
    const double actionBarX = std::max(
        FEATURE_PANEL_MARGIN,
        std::min(preferredActionX, screenW - actionBarW - FEATURE_PANEL_MARGIN)
    );
    const double preferredActionY = selY + selH + FEATURE_PANEL_TOP_GAP;
    const bool actionBelowFits = (preferredActionY + actionBarH + FEATURE_PANEL_MARGIN) <= screenH;
    const double actionBarY = actionBelowFits
        ? preferredActionY
        : std::max(FEATURE_PANEL_MARGIN, screenH - actionBarH - FEATURE_PANEL_MARGIN);

    const double preferredTopX = selX + (selW - topGroupW) / 2.0;
    const double topGroupX = std::max(
        FEATURE_PANEL_MARGIN,
        std::min(preferredTopX, screenW - topGroupW - FEATURE_PANEL_MARGIN)
    );
    const double preferredSizeY = selY - FEATURE_PANEL_TOP_GAP - topClusterH;
    const bool sizeAboveFits = preferredSizeY >= FEATURE_PANEL_MARGIN;
    const double sizeCardY = sizeAboveFits
        ? preferredSizeY
        : FEATURE_PANEL_MARGIN;

    layout.compactMode = forceAbove || toolRailClamped || !actionBelowFits || !sizeAboveFits;

    layout.leftToolsPanel = QRectF(leftPanelX, leftPanelY, TOOL_RAIL_W, toolPanelH);
    layout.rightActionsPanel = QRectF(actionBarX, actionBarY, actionBarW, actionBarH);
    for (int i = 0; i < NUM_TOOLS; ++i) {
        layout.toolCells[i] = QRectF(
            layout.leftToolsPanel.x(),
            layout.leftToolsPanel.y() + (i * TOOL_CARD_H),
            TOOL_RAIL_W,
            TOOL_CARD_H
        );
    }

    layout.topCluster = QRectF(
        topGroupX,
        sizeCardY,
        topGroupW,
        topClusterH
    );
    layout.sizeCard = QRectF(
        topGroupX,
        sizeCardY,
        sizeCardW,
        topClusterH
    );
    layout.cropCard = QRectF(
        layout.sizeCard.right() + ACTION_CARD_GAP,
        sizeCardY,
        cropCardW,
        topClusterH
    );
    layout.confirmCard = QRectF(
        layout.rightActionsPanel.x(),
        layout.rightActionsPanel.y(),
        ACTION_RAIL_W,
        ACTION_CARD_H
    );
    layout.cancelCard = QRectF(
        layout.confirmCard.right() + ACTION_CARD_GAP,
        layout.rightActionsPanel.y(),
        ACTION_RAIL_W,
        ACTION_CARD_H
    );
    return layout;
}

RecordingDeckLayout computeRecordingDeckLayout(double selX, double selY,
                                               double selW, double selH,
                                               double screenW, double screenH)
{
    RecordingDeckLayout layout;
    const double railH = TOOL_CARD_H * 5.0;
    const double centerY = selY + (selH / 2.0);
    const double leftCandidateX = selX - TOOL_RAIL_GAP - TOOL_RAIL_W;
    const double leftPanelX = std::max(FEATURE_PANEL_MARGIN, leftCandidateX);
    const double leftPanelY = std::max(
        FEATURE_PANEL_MARGIN,
        std::min(centerY - (railH / 2.0), screenH - railH - FEATURE_PANEL_MARGIN)
    );

    const double preferredTopX = selX + (selW - REC_TOP_CLUSTER_W) / 2.0;
    const double topX = std::max(
        FEATURE_PANEL_MARGIN,
        std::min(preferredTopX, screenW - REC_TOP_CLUSTER_W - FEATURE_PANEL_MARGIN)
    );
    const double preferredTopY = selY - FEATURE_PANEL_TOP_GAP - REC_TOP_CLUSTER_H;
    const bool topFits = preferredTopY >= FEATURE_PANEL_MARGIN;
    const double topY = topFits ? preferredTopY : FEATURE_PANEL_MARGIN;

    const double actionBarW = (2.0 * ACTION_RAIL_W) + ACTION_CARD_GAP;
    const double preferredActionX = selX + (selW - actionBarW) / 2.0;
    const double actionX = std::max(
        FEATURE_PANEL_MARGIN,
        std::min(preferredActionX, screenW - actionBarW - FEATURE_PANEL_MARGIN)
    );
    const double preferredActionY = selY + selH + FEATURE_PANEL_TOP_GAP;
    const bool actionFits = (preferredActionY + ACTION_CARD_H + FEATURE_PANEL_MARGIN) <= screenH;
    const double actionY = actionFits
        ? preferredActionY
        : std::max(FEATURE_PANEL_MARGIN, screenH - ACTION_CARD_H - FEATURE_PANEL_MARGIN);

    layout.placedAbove = !actionFits;
    layout.leftToggleRail = QRectF(leftPanelX, leftPanelY, TOOL_RAIL_W, railH);
    layout.topCluster = QRectF(topX, topY, REC_TOP_CLUSTER_W, REC_TOP_CLUSTER_H);
    layout.bottomActionBar = QRectF(actionX, actionY, actionBarW, ACTION_CARD_H);
    layout.deckBounds = layout.leftToggleRail.united(layout.topCluster).united(layout.bottomActionBar);
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
        
        // Dark glass tint matching editor root background (#141414 at ~90% opacity)
        p.fillRect(QRectF(x, y, w, h), QColor(20, 20, 20, 230));
    } else {
        // Solid background matching editor root background (#141414)
        p.fillRect(QRectF(x, y, w, h), QColor(20, 20, 20));
    }

    // Subtle white sheen (0.04 alpha) for a premium feel
    p.fillRect(QRectF(x, y, w, h), QColor(255, 255, 255, 10));

    // Panel border (matching editor's .editor-root border: 1px solid rgba(255, 255, 255, 0.10))
    p.setPen(QPen(QColor(255, 255, 255, 26), 1.0));
    p.setBrush(Qt::NoBrush);
    p.drawPath(clip);

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
    case 7: { // Recording — same video camera glyph as recording action
        QPainterPath path;
        roundedRectPath(path, cx - 8.0, cy - 5.0, 10.5, 10.0, 2.5);
        p.drawPath(path);
        QPainterPath lens;
        lens.moveTo(cx + 2.4, cy - 2.8);
        lens.lineTo(cx + 7.4, cy - 5.2);
        lens.lineTo(cx + 7.4, cy + 5.2);
        lens.lineTo(cx + 2.4, cy + 2.8);
        lens.closeSubpath();
        p.drawPath(lens);
        break;
    }
    // Recording panel icons (8-12)
    case 8: { // Settings/Sliders
        // Three vertical lines with sliders
        for (int i = 0; i < 3; ++i) {
            double x = cx - 4.5 + i * 4.5;
            p.drawLine(QPointF(x, cy - 6.0), QPointF(x, cy + 6.0));
            double sliderY = (i == 0) ? cy - 2.0 : (i == 1 ? cy + 2.0 : cy - 1.0);
            p.drawEllipse(QPointF(x, sliderY), 1.8, 1.8);
        }
        break;
    }
    case 9: { // Size - matches screenshot (just layout box)
        break; 
    }
    case 10: { // Crop (matching editor toolbar icon)
        p.setPen(QPen(color, 1.6, Qt::SolidLine, Qt::FlatCap, Qt::MiterJoin));
        double s = 10.5; // main square side
        double t = 2.8;  // tail length
        double o = 1.2;  // overlap offset
        
        // Top-left part
        p.drawLine(QPointF(cx - s/2 - t, cy - s/2 + o), QPointF(cx + s/2 - o, cy - s/2 + o));
        p.drawLine(QPointF(cx - s/2 + o, cy - s/2 - t), QPointF(cx - s/2 + o, cy + s/2 - o));
        
        // Bottom-right part
        p.drawLine(QPointF(cx + s/2 + t, cy + s/2 - o), QPointF(cx - s/2 + o, cy + s/2 - o));
        p.drawLine(QPointF(cx + s/2 - o, cy + s/2 + t), QPointF(cx + s/2 - o, cy - s/2 + o));
        break;
    }
    case 11: { // Mic - Adwaita-style symbolic microphone
        QPainterPath capsule;
        roundedRectPath(capsule, cx - 3.1, cy - 7.0, 6.2, 9.6, 3.1);
        p.drawPath(capsule);

        p.drawLine(QPointF(cx - 5.0, cy - 0.3), QPointF(cx - 5.0, cy + 1.6));
        p.drawLine(QPointF(cx + 5.0, cy - 0.3), QPointF(cx + 5.0, cy + 1.6));
        p.drawArc(QRectF(cx - 5.0, cy - 1.5, 10.0, 8.4), 180 * 16, 180 * 16);
        p.drawLine(QPointF(cx, cy + 6.1), QPointF(cx, cy + 8.3));
        p.drawLine(QPointF(cx - 3.4, cy + 8.3), QPointF(cx + 3.4, cy + 8.3));
        break;
    }
    case 12: { // Speaker - Adwaita-style symbolic audio volume
        QPainterPath body;
        body.moveTo(cx - 6.8, cy - 2.3);
        body.lineTo(cx - 4.4, cy - 2.3);
        body.lineTo(cx - 1.2, cy - 5.1);
        body.lineTo(cx - 1.2, cy + 5.1);
        body.lineTo(cx - 4.4, cy + 2.3);
        body.lineTo(cx - 6.8, cy + 2.3);
        body.closeSubpath();
        p.drawPath(body);

        p.drawArc(QRectF(cx - 0.8, cy - 4.8, 5.6, 9.6), -40 * 16, 80 * 16);
        p.drawArc(QRectF(cx + 1.2, cy - 6.8, 8.0, 13.6), -40 * 16, 80 * 16);
        break;
    }
    case 13: { // Camera - Adwaita-style photo camera symbolic
        QPainterPath body;
        roundedRectPath(body, cx - 7.2, cy - 4.6, 14.4, 9.8, 2.2);
        p.drawPath(body);
        p.drawEllipse(QPointF(cx, cy + 0.3), 3.0, 3.0);

        QPainterPath topBridge;
        topBridge.moveTo(cx - 4.6, cy - 4.6);
        topBridge.lineTo(cx - 2.2, cy - 6.8);
        topBridge.lineTo(cx + 1.8, cy - 6.8);
        topBridge.lineTo(cx + 3.8, cy - 4.6);
        p.drawPath(topBridge);
        break;
    }
    case 14: { // Mouse cursor with sunburst
        p.save();
        p.setPen(QPen(color, 1.6, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
        // Pointer cursor
        QPainterPath path;
        path.moveTo(cx - 0.5, cy - 6.5);
        path.lineTo(cx - 0.5, cy + 5.0);
        path.lineTo(cx + 2.5, cy + 1.5);
        path.lineTo(cx + 7.0, cy + 2.0);
        path.closeSubpath();
        p.drawPath(path);
        p.drawLine(QPointF(cx + 2.5, cy + 1.5), QPointF(cx + 5.5, cy + 6.0));
        
        // Starburst at tip
        double tx = cx - 0.5, ty = cy - 6.5;
        p.setPen(QPen(color, 1.2));
        for (int i = 0; i < 6; ++i) {
            double ang = i * M_PI / 3.0;
            p.drawLine(QPointF(tx + cos(ang)*3.5, ty + sin(ang)*3.5),
                       QPointF(tx + cos(ang)*6.0, ty + sin(ang)*6.0));
        }
        p.restore();
        break;
    }
    case 15: { // Command Key (⌘) in rounded square
        QPainterPath box;
        roundedRectPath(box, cx - 8.5, cy - 8.5, 17.0, 17.0, 3.5);
        p.drawPath(box);
        
        // Command symbol
        p.save();
        p.setPen(QPen(color, 1.8));
        double r = 2.4;
        p.drawEllipse(QPointF(cx - r, cy - r), r, r);
        p.drawEllipse(QPointF(cx + r, cy - r), r, r);
        p.drawEllipse(QPointF(cx - r, cy + r), r, r);
        p.drawEllipse(QPointF(cx + r, cy + r), r, r);
        // Connectors
        p.drawLine(QPointF(cx - r, cy - r + 0.5), QPointF(cx - r, cy + r - 0.5));
        p.drawLine(QPointF(cx + r, cy - r + 0.5), QPointF(cx + r, cy + r - 0.5));
        p.drawLine(QPointF(cx - r + 0.5, cy - r), QPointF(cx + r - 0.5, cy - r));
        p.drawLine(QPointF(cx - r + 0.5, cy + r), QPointF(cx + r - 0.5, cy + r));
        p.restore();
        break;
    }
    case 16: { // Video - Adwaita-style symbolic video camera
        QPainterPath body;
        roundedRectPath(body, cx - 8.0, cy - 5.0, 10.5, 10.0, 2.5);
        p.drawPath(body);
        QPainterPath lens;
        lens.moveTo(cx + 2.4, cy - 2.8);
        lens.lineTo(cx + 7.4, cy - 5.2);
        lens.lineTo(cx + 7.4, cy + 5.2);
        lens.lineTo(cx + 2.4, cy + 2.8);
        lens.closeSubpath();
        p.drawPath(lens);
        break;
    }
    case 17: { // GIF Logo (Large)
        p.setPen(Qt::NoPen);
        p.setBrush(color);
        QPainterPath box;
        roundedRectPath(box, cx - 9, cy - 6, 18, 12, 3);
        p.drawPath(box);
        p.setPen(QColor(0,0,0,180));
        QFont f = p.font(); f.setPointSizeF(6.5); f.setBold(true); p.setFont(f);
        p.drawText(QRectF(cx - 9, cy - 6, 18, 12), Qt::AlignCenter, "GIF");
        break;
    }
    }
    p.restore();
}

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

    // ── Toolbar (hide when recording panel is open) ────────────────────────────
    if (!m_recordingPanelOpen) {
        drawToolbar(p, sx, sy, selW, selH, sw, sh);
    } else if (!m_recordingToolsHidden) {
        // Draw recording panel inside selection
        drawRecordingPanel(p, sx, sy, selW, selH);
    }

    // ── Webcam preview ──────────────────────────────────────────────────────
    if (m_recordingPanelOpen && m_recWebcam) {
        p.save();
        p.setRenderHint(QPainter::Antialiasing);
        const QRectF previewRect = webcamPreviewRect(sx, sy, selW, selH);
        const double previewW = previewRect.width();
        const double previewH = previewRect.height();
        const double px = previewRect.x();
        const double py = previewRect.y();

        // Flip
        if (m_webcamFlip) {
            p.translate(px + previewW / 2.0, 0);
            p.scale(-1, 1);
            p.translate(-(px + previewW / 2.0), 0);
        }

        // Create clipping path for the shape
        QPainterPath clipPath;
        if (m_webcamShape == WebcamShape::Circle) {
            clipPath.addEllipse(previewRect);
        } else {
            double radius = (m_webcamShape == WebcamShape::Square) ? 8 : 12;
            clipPath.addRoundedRect(previewRect, radius, radius);
        }

        // Draw frame if available
        QPixmap frame;
        { QMutexLocker lock(&m_webcamMutex); frame = m_webcamFrame; }

        if (!frame.isNull()) {
            p.setClipPath(clipPath);
            p.drawPixmap(previewRect.toRect(), frame);
            p.setClipping(false);
        } else {
            // Dark placeholder
            p.setBrush(QColor(0, 0, 0, 180));
            p.setPen(Qt::NoPen);
            p.drawPath(clipPath);
        }

        // Shape border
        p.setPen(QPen(QColor(255, 255, 255, 40), 1.5));
        p.setBrush(Qt::NoBrush);
        p.drawPath(clipPath);

        // Device label
        QString label = "Webcam";
        if (m_webcamDevice >= 0) {
            label = QStringLiteral("Camera %1").arg(m_webcamDevice);
        }
        QFont wf; wf.setFamily("Sans"); wf.setPointSizeF(10.0); wf.setBold(true);
        p.setFont(wf);
        p.setPen(QColor(255, 255, 255, 120));
        p.drawText(QRectF(px + 8, py + previewH - 22, previewW - 16, 18),
                   Qt::AlignLeft | Qt::AlignVCenter, label);
        p.restore();
    }

    // ── Keystroke preview — drawn AFTER sub-panels so it's always on top ─────
    if (m_showKeystrokePreview) {
        drawKeystrokePreview(p, sx, sy, selW, selH);
    }

    // ── Visible countdown overlay (Centered Circle) ──────────────────────────
    if (m_countdownActive && m_countdownValue > 0) {
        const double bubbleSize = 184.0;
        const double bubbleX = (sw - bubbleSize) / 2.0;
        const double bubbleY = (sh - bubbleSize) / 2.0;
        const QRectF bubbleRect(bubbleX, bubbleY, bubbleSize, bubbleSize);
        m_countdownBubbleRect = bubbleRect;

        p.save();
        p.setRenderHint(QPainter::Antialiasing);

        p.setPen(Qt::NoPen);
        p.setBrush(m_hoveredCountdownCancel
                       ? QColor(132, 38, 24, 242)
                       : QColor(0, 0, 0, 240));
        p.drawEllipse(bubbleRect);

        QFont countdownFont(QStringLiteral("Sans"));
        countdownFont.setBold(true);
        countdownFont.setPointSizeF(m_hoveredCountdownCancel ? 34.0 : 72.0);
        p.setFont(countdownFont);
        p.setPen(m_hoveredCountdownCancel ? QColor(255, 228, 214) : Qt::white);

        p.drawText(bubbleRect,
                   Qt::AlignCenter,
                   m_hoveredCountdownCancel ? QStringLiteral("Cancel")
                                            : QString::number(m_countdownValue));

        p.restore();
    } else {
        m_countdownBubbleRect = QRectF();
    }
}

// ── Draw recording panel (capture-style rails around selection) ───────────────

void CaptureOverlay::drawRecordingPanel(QPainter& p,
                                          double selX, double selY,
                                          double selW, double selH)
{
    const double screenW = width();
    const double screenH = height();
    const QImage* blurPtr = m_blurredBg.isNull() ? nullptr : &m_blurredBg;
    const RecordingDeckLayout deck = computeRecordingDeckLayout(selX, selY, selW, selH, screenW, screenH);
    m_recordingToggleRailRect = deck.leftToggleRail;
    m_recordingTopClusterRect = deck.topCluster;
    m_recordingBottomBarRect = deck.bottomActionBar;
    m_recPanelRect = deck.deckBounds;

    const QColor warmAccent(176, 92, 56);
    const QColor warmRim(255, 212, 178);
    m_recTileRects.clear();
    const double panelRadius = 10.0;

    // ── Helper: draw brand rounded hover ─────────────────────────────────
    auto drawTileHover = [&](QRectF r, double radius = 10.0, bool topLeft = false, bool topRight = false, bool bottomLeft = false, bool bottomRight = false) {
        QPainterPath path;
        if (topLeft || topRight || bottomLeft || bottomRight) {
            // Match panel corners if specified
            double tr = topLeft ? panelRadius : radius;
            double trr = topRight ? panelRadius : radius;
            double blr = bottomLeft ? panelRadius : radius;
            double brr = bottomRight ? panelRadius : radius;
            
            path.moveTo(r.x() + tr, r.y());
            path.lineTo(r.right() - trr, r.y());
            path.quadTo(r.right(), r.y(), r.right(), r.y() + trr);
            path.lineTo(r.right(), r.bottom() - brr);
            path.quadTo(r.right(), r.bottom(), r.right() - brr, r.bottom());
            path.lineTo(r.x() + blr, r.bottom());
            path.quadTo(r.x(), r.bottom(), r.x(), r.bottom() - blr);
            path.lineTo(r.x(), r.y() + tr);
            path.quadTo(r.x(), r.y(), r.x() + tr, r.y());
        } else {
            path.addRoundedRect(r, radius, radius);
        }
        p.fillPath(path, QColor(255, 255, 255, 22));
    };

    auto drawMeter = [&](const QRectF& r, double level, bool warm) {
        p.save();
        p.setRenderHint(QPainter::Antialiasing);
        const int numSegments = 4;
        const double segmentW = 10.0;
        const double segmentH = 4.5;
        const double spacing = 3.0;
        const double totalW = numSegments * segmentW + (numSegments - 1) * spacing;
        const double baseX = r.center().x() - totalW / 2.0;
        const double baseY = r.bottom() - 17.0;
        const double clampedLevel = std::max(0.0, std::min(1.0, level));

        QColor activeStart = warm ? QColor(255, 214, 153) : QColor(172, 224, 255);
        QColor activeEnd = warm ? QColor(255, 134, 52) : QColor(76, 154, 255);
        QColor inactiveFill = QColor(255, 255, 255, 42);
        QColor inactiveBorder = QColor(255, 255, 255, 18);

        for (int b = 0; b < numSegments; ++b) {
            const double threshold = static_cast<double>(b + 1) / static_cast<double>(numSegments);
            const bool lit = clampedLevel >= (threshold - 0.18);
            QRectF seg(baseX + b * (segmentW + spacing), baseY, segmentW, segmentH);

            if (lit) {
                QLinearGradient grad(seg.topLeft(), seg.topRight());
                grad.setColorAt(0.0, activeStart);
                grad.setColorAt(1.0, activeEnd);
                p.setBrush(grad);
                p.setPen(Qt::NoPen);
            } else {
                p.setBrush(inactiveFill);
                p.setPen(QPen(inactiveBorder, 0.9));
            }
            p.drawRoundedRect(seg, 2.2, 2.2);
        }
        p.restore();
    };

    auto drawModuleTile = [&](const QRectF& r,
                              RecordPanelTile tile,
                              int iconIdx,
                              bool active,
                              const QString& label = QString(),
                              bool warm = false,
                              bool showMeter = false,
                              double meterLevel = 0.0) {
        if (m_hoveredRecordTile == tile) {
            drawTileHover(r, 10.0);
        }
        if (active) {
            QPainterPath activePath;
            roundedRectPath(activePath, r.x() + 3.0, r.y() + 3.0, r.width() - 6.0, r.height() - 6.0, 9.0);
            p.fillPath(activePath, warm ? QColor(warmAccent.red(), warmAccent.green(), warmAccent.blue(), 76)
                                        : QColor(255, 255, 255, 18));
        }
        const bool hovered = (m_hoveredRecordTile == tile);
        const bool hasLabel = !label.isEmpty();
        const double iconAlpha = (hovered || active) ? 1.0 : 0.94;
        const double shadowAlpha = hovered ? 0.24 : (active ? 0.32 : 0.50);
        const double iconY = hasLabel
            ? r.y() + ((hovered || active) ? 19.5 : 20.0)
            : r.center().y() - ((hovered || active) ? 2.5 : 2.0);
        const QRectF meterSafeRect(r.x() + 4.0, r.bottom() - 22.0, r.width() - 8.0, 10.0);
        const QPointF iconCenter(r.center().x(), iconY);
        const QColor iconColor = active
            ? QColor(255, 229, 206, int(iconAlpha * 255))
            : QColor(255, 255, 255, int(iconAlpha * 255));
        drawToolbarIcon(p, iconIdx, iconCenter.x() + 0.6, iconCenter.y() + 0.8,
                        QColor(0, 0, 0, int(shadowAlpha * 255)));
        drawToolbarIcon(p, iconIdx, iconCenter.x(), iconCenter.y(), iconColor);
        if (hasLabel) {
            QFont f; f.setFamily("Sans"); f.setPointSizeF(8.0); f.setBold(hovered || active);
            p.setFont(f);
            QFontMetricsF fm(f);
            const double tw = fm.horizontalAdvance(label);
            p.setPen(QColor(0, 0, 0, int(shadowAlpha * 255)));
            p.drawText(QPointF(r.center().x() - tw / 2.0 + 0.6, r.y() + 50.8), label);
            p.setPen(active
                ? QColor(255, 229, 206, int(iconAlpha * 255))
                : QColor(244, 244, 244, int(iconAlpha * 255)));
            p.drawText(QPointF(r.center().x() - tw / 2.0, r.y() + 50.0), label);
        }
        if (showMeter) {
            p.save();
            p.setPen(Qt::NoPen);
            p.setBrush(QColor(0, 0, 0, 24));
            p.drawRoundedRect(meterSafeRect.adjusted(-4.0, -1.0, 4.0, 1.0), 5.0, 5.0);
            p.restore();
            drawMeter(r, meterLevel, warm);
        }
    };

    auto drawPrimaryAction = [&](const QRectF& rect,
                                 RecordPanelTile tile,
                                 int iconIdx,
                                 const QString& title,
                                  bool primary) {
        const bool hovered = (m_hoveredRecordTile == tile);
        const bool active = hovered || primary;
        const double iconAlpha = active ? 1.0 : 0.94;
        const double shadowAlpha = hovered ? 0.24 : (primary ? 0.32 : 0.50);
        if (hovered) {
            drawTileHover(rect, 10.0);
        }

        QPainterPath path;
        roundedRectPath(path, rect.x() + 3.0, rect.y() + 3.0, rect.width() - 6.0, rect.height() - 6.0, 9.0);
        p.fillPath(path, primary ? QColor(warmAccent.red(), warmAccent.green(), warmAccent.blue(), 88)
                                 : QColor(255, 255, 255, 18));
        p.save();
        p.setClipPath(path);
        p.setPen(QPen(primary ? warmRim : QColor(255, 255, 255, 110), 1.1));
        p.setBrush(Qt::NoBrush);
        QPainterPath rim;
        roundedRectPath(rim, rect.x() + 3.8, rect.y() + 3.8, rect.width() - 7.6, rect.height() - 7.6, 8.4);
        p.drawPath(rim);
        p.restore();

        const double iconY = rect.center().y() - (hovered ? 0.5 : 0.0);
        drawToolbarIcon(p, iconIdx, rect.x() + 28.6, iconY + 0.8,
                        QColor(0, 0, 0, int(shadowAlpha * 255)));
        drawToolbarIcon(p, iconIdx, rect.x() + 28.0, iconY,
                        QColor(255, 255, 255, int(iconAlpha * 255)));
        QFont titleFont; titleFont.setFamily("Sans"); titleFont.setPointSizeF(11.8); titleFont.setBold(true);
        p.setFont(titleFont);
        const double textX = rect.x() + 50.0;
        const double textY = rect.y() + 30.0;
        p.setPen(QColor(0, 0, 0, int(shadowAlpha * 255)));
        p.drawText(QPointF(textX + 0.6, textY + 0.8), title);
        p.setPen(primary
            ? QColor(255, 232, 214, int(iconAlpha * 255))
            : QColor(245, 245, 246, int(iconAlpha * 255)));
        p.drawText(QPointF(textX, textY), title);
    };

    drawFrostedPanel(p, deck.leftToggleRail.x(), deck.leftToggleRail.y(), deck.leftToggleRail.width(), deck.leftToggleRail.height(),
                     panelRadius, blurPtr, screenW, screenH);

    drawFrostedPanel(p, deck.topCluster.x(), deck.topCluster.y(), deck.topCluster.width(), deck.topCluster.height(),
                     panelRadius, blurPtr, screenW, screenH);

    drawFrostedPanel(p, deck.bottomActionBar.x(), deck.bottomActionBar.y(), deck.bottomActionBar.width(), deck.bottomActionBar.height(),
                     panelRadius, blurPtr, screenW, screenH);

    const double railX = deck.leftToggleRail.x();
    const double railY = deck.leftToggleRail.y();
    const double topX = deck.topCluster.x();
    const double topY = deck.topCluster.y();
    const double bottomX = deck.bottomActionBar.x();
    const double bottomY = deck.bottomActionBar.y();

    const QRectF controlsRect(topX, topY, 62.0, REC_TOP_CLUSTER_H);
    const QRectF sizeRect(controlsRect.right() + ACTION_CARD_GAP, topY, 152.0, REC_TOP_CLUSTER_H);
    const QRectF cropRect(sizeRect.right() + ACTION_CARD_GAP, topY, 62.0, REC_TOP_CLUSTER_H);

    const QRectF micRect(railX, railY + TOOL_CARD_H * 0.0, TOOL_RAIL_W, TOOL_CARD_H);
    const QRectF speakerRect(railX, railY + TOOL_CARD_H * 1.0, TOOL_RAIL_W, TOOL_CARD_H);
    const QRectF webcamRect(railX, railY + TOOL_CARD_H * 2.0, TOOL_RAIL_W, TOOL_CARD_H);
    const QRectF clickRect(railX, railY + TOOL_CARD_H * 3.0, TOOL_RAIL_W, TOOL_CARD_H);
    const QRectF keysRect(railX, railY + TOOL_CARD_H * 4.0, TOOL_RAIL_W, TOOL_CARD_H);

    m_recTileRects.append(controlsRect);
    drawModuleTile(controlsRect, RecordPanelTile::Controls, 8, m_settingsOpen, QString(), false, false, 0.0);

    m_recTileRects.append(sizeRect);
    if (m_hoveredRecordTile == RecordPanelTile::Size) {
        drawTileHover(sizeRect, 10.0);
    }
    {
        const QString sizeVal = QString("%1×%2").arg((int)selW).arg((int)selH);
        QFont headerFont; headerFont.setFamily("Sans"); headerFont.setPointSizeF(7.2); headerFont.setBold(true);
        p.setFont(headerFont);
        p.setPen(QColor(255, 224, 196, 196));
        p.drawText(QRectF(sizeRect.x(), sizeRect.y() + 8.0, sizeRect.width(), 12.0), Qt::AlignCenter, QStringLiteral("FRAME"));

        QFont valueFont; valueFont.setFamily("Sans"); valueFont.setPointSizeF(11.0); valueFont.setBold(true);
        p.setFont(valueFont);
        p.setPen(QColor(245, 245, 246));
        p.drawText(QRectF(sizeRect.x(), sizeRect.y() + 20.0, sizeRect.width(), 20.0), Qt::AlignCenter, sizeVal);
    }

    m_recTileRects.append(cropRect);
    drawModuleTile(cropRect, RecordPanelTile::Crop, 10, m_recordAspectRatioIndex != 0 || m_cropMenuOpen, QString(), true, false, 0.0);

    m_recTileRects.append(micRect);
    drawModuleTile(micRect, RecordPanelTile::Mic, 11, m_recMic, QStringLiteral("Mic"), true, m_recMic, m_micLevel);
    m_recTileRects.append(speakerRect);
    drawModuleTile(speakerRect, RecordPanelTile::Speaker, 12, m_recSpeaker, QStringLiteral("Speaker"), false, m_recSpeaker, m_speakerLevel);
    m_recTileRects.append(webcamRect);
    drawModuleTile(webcamRect, RecordPanelTile::Webcam, 13, m_recWebcam, QStringLiteral("Cam"), false, false, 0.0);
    m_recTileRects.append(clickRect);
    drawModuleTile(clickRect, RecordPanelTile::Click, 14, m_recClicks, QStringLiteral("Clicks"), false, false, 0.0);
    m_recTileRects.append(keysRect);
    drawModuleTile(keysRect, RecordPanelTile::Keystrokes, 15, m_recKeystrokes, QStringLiteral("Keys"), false, false, 0.0);

    const QRectF videoRect(bottomX, bottomY, ACTION_RAIL_W, ACTION_CARD_H);
    const QRectF gifRect(videoRect.right() + ACTION_CARD_GAP, bottomY, ACTION_RAIL_W, ACTION_CARD_H);
    m_recTileRects.append(videoRect);
    m_recTileRects.append(gifRect);
    drawPrimaryAction(videoRect, RecordPanelTile::RecordVideo, 16, QStringLiteral("Video"), true);
    drawPrimaryAction(gifRect, RecordPanelTile::RecordGif, 17, QStringLiteral("GIF"), false);

    const double contextualX = std::max(10.0, std::min(selX + (selW - 440.0) / 2.0, screenW - 450.0));
    const double contextualY = std::max(10.0, std::min(selY + 24.0, screenH - 510.0));
    const QRectF contextualRect(contextualX, contextualY, 440.0, 500.0);

    if (m_settingsOpen) {
        drawSettingsMenu(p, contextualRect.x(), contextualRect.y());
    } else {
        if (m_clickOptionsOpen) {
            drawClickOptions(p, contextualRect);
        }
        if (m_keystrokeOptionsOpen) {
            drawKeystrokeOptions(p, contextualRect);
        }
        if (m_dropdownOpen != -1) {
            drawDropdownPopup(p, m_dropdownAnchor, m_dropdownOptions,
                              m_dropdownValuePtr ? *m_dropdownValuePtr : -1);
        }
    }

    if (m_cropMenuOpen) {
        const double itemH = 34.0;
        const double menuW = 196.0;
        const double menuH = (kRecordingAspectOptionCount * itemH) + 10.0;
        const double menuX = std::max(10.0, std::min(cropRect.center().x() - (menuW / 2.0), screenW - menuW - 10.0));
        const double menuY = std::max(10.0, std::min(cropRect.bottom() + 8.0, screenH - menuH - 10.0));
        m_cropMenuPanelRect = QRectF(menuX, menuY, menuW, menuH);
        m_cropMenuItemRects.clear();

        drawFrostedPanel(p, menuX, menuY, menuW, menuH, 12.0, blurPtr, screenW, screenH);

        for (int i = 0; i < kRecordingAspectOptionCount; ++i) {
            const QRectF itemRect(menuX + 5.0, menuY + 5.0 + (i * itemH), menuW - 10.0, itemH);
            const QRectF indicatorRect(itemRect.x() + 8.0, itemRect.y(), 18.0, itemRect.height());
            const QRectF labelRect(itemRect.x() + 30.0, itemRect.y(), itemRect.width() - 40.0, itemRect.height());
            m_cropMenuItemRects.append(itemRect);

            if (i == m_hoveredCropMenuItem) {
                p.setPen(Qt::NoPen);
                p.setBrush(QColor(255, 255, 255, 18));
                p.drawRoundedRect(itemRect, 7.0, 7.0);
            }

            const bool selected = (i == m_recordAspectRatioIndex);
            if (selected) {
                p.setPen(Qt::NoPen);
                p.setBrush(QColor(warmAccent.red(), warmAccent.green(), warmAccent.blue(), 94));
                p.drawRoundedRect(itemRect.adjusted(1.0, 1.0, -1.0, -1.0), 7.0, 7.0);
            }

            if (selected) {
                p.setPen(QPen(QColor(255, 238, 224), 1.5));
                const double cy = indicatorRect.center().y();
                p.drawLine(QPointF(indicatorRect.x() + 3.5, cy), QPointF(indicatorRect.x() + 6.5, cy + 3.0));
                p.drawLine(QPointF(indicatorRect.x() + 6.5, cy + 3.0), QPointF(indicatorRect.x() + 12.5, cy - 4.0));
            }

            QFont itemFont(QStringLiteral("Sans"));
            itemFont.setPointSizeF(10.0);
            itemFont.setBold(selected);
            p.setFont(itemFont);
            p.setPen(selected ? QColor(255, 240, 226) : QColor(242, 242, 244));
            p.drawText(labelRect,
                       Qt::AlignVCenter | Qt::AlignLeft,
                       QString::fromUtf8(kRecordingAspectOptions[i].label));
        }
    } else {
        m_cropMenuPanelRect = QRectF();
        m_cropMenuItemRects.clear();
    }
}

void CaptureOverlay::drawSettingsMenu(QPainter& p, double panelX, double startY)
{
    const double menuW = 440.0;
    const double menuH = 500.0;
    const double menuX = std::max(10.0, std::min(panelX, (double)width() - menuW - 10.0));
    const double menuY = std::max(10.0, std::min(startY, (double)height() - menuH - 10.0));
    
    m_settingsPanelRect = QRectF(menuX, menuY, menuW, menuH);
    m_settingsClickableRects.clear();

    const QColor accentColor(176, 92, 56);
    const QColor accentRim(255, 214, 186);
    const QImage* blurPtr = m_blurredBg.isNull() ? nullptr : &m_blurredBg;

    p.save();
    QRadialGradient glow(menuX + menuW/2.0, menuY + menuH/2.0, menuW);
    glow.setColorAt(0, QColor(accentColor.red(), accentColor.green(), accentColor.blue(), 40));
    glow.setColorAt(0.6, QColor(0, 0, 0, 0));
    p.fillRect(QRectF(menuX - 40, menuY - 40, menuW + 80, menuH + 80), glow);
    p.restore();

    drawFrostedPanel(p, menuX, menuY, menuW, menuH, 12.0, blurPtr, width(), height());

    p.setFont(QFont("Sans", 8, QFont::Bold));
    p.setPen(QColor(255, 224, 196, 176));
    p.drawText(QRectF(menuX + 18.0, menuY + 18.0, menuW - 36.0, 12.0),
               Qt::AlignLeft | Qt::AlignVCenter, QStringLiteral("RECORDING CONTROLS"));
    p.setFont(QFont("Sans", 14, QFont::Bold));
    p.setPen(QColor(245, 245, 246));
    p.drawText(QRectF(menuX + 18.0, menuY + 28.0, menuW - 36.0, 22.0),
               Qt::AlignLeft | Qt::AlignVCenter, QStringLiteral("Recording Setup"));

    // Tabs
    const QStringList tabs = {"General", "Video", "GIF"};
    const double tabW = 78.0;
    const double tabH = 32.0;
    double tabStartX = menuX + (menuW - tabs.size() * tabW) / 2.0;
    double tabY = menuY + 64.0;

    for (int i = 0; i < tabs.size(); ++i) {
        QRectF tr(tabStartX + i * tabW, tabY, tabW, tabH);
        m_settingsClickableRects.append(tr); // tab rects
        
        bool hovered = (m_hoveredSettingsItem == i);
        if (m_settingsTab == i || hovered) {
            p.setPen(QPen(m_settingsTab == i ? accentRim : QColor(255, 255, 255, 28), 1.0));
            p.setBrush(m_settingsTab == i ? QColor(accentColor.red(), accentColor.green(), accentColor.blue(), 84)
                                          : QColor(255, 255, 255, 14));
            p.drawRoundedRect(tr, 9.0, 9.0);
            p.setPen(m_settingsTab == i ? QColor(255, 236, 220) : QColor(255, 255, 255, 220));
        } else {
            p.setPen(QColor(255, 255, 255, 150));
        }

        QFont tf; tf.setFamily("Sans"); tf.setPointSizeF(10.3); tf.setBold(m_settingsTab == i || hovered);
        p.setFont(tf);
        p.drawText(tr, Qt::AlignCenter, tabs[i]);
    }

    if (m_settingsTab == 0) { // General
        double currY = menuY + 110.0;
        const double labelX = menuX + 25.0;
        const double valueX = menuX + 140.0;
        const double rowH = 32.0;

        auto drawSetting = [&](const QString& label, const QString& desc, bool checked, bool* target) {
            QRectF labelRect(labelX, currY, 110, rowH);
            p.setFont(QFont("Sans", 10, QFont::Bold));
            p.setPen(QColor(255, 255, 255, 200));
            p.drawText(labelRect, Qt::AlignRight | Qt::AlignVCenter, label);

            QRectF checkArea(valueX, currY, menuW - (valueX - menuX) - 20, rowH);
            int itemIdx = m_settingsClickableRects.size();
            m_settingsClickableRects.append(checkArea); // settings row rect
            
            bool hovered = (m_hoveredSettingsItem == itemIdx);
            if (hovered) {
                p.setPen(Qt::NoPen);
                p.setBrush(QColor(255, 255, 255, 12));
                p.drawRoundedRect(checkArea.adjusted(-5, 0, 5, 0), 6, 6);
            }

            // Checkbox
            QRectF cb(valueX, currY + (rowH - 18) / 2.0, 18, 18);
            p.setRenderHint(QPainter::Antialiasing);
            if (checked) {
                p.setPen(Qt::NoPen);
                p.setBrush(accentColor);
                p.drawRoundedRect(cb, 4, 4);
                p.setPen(QPen(Qt::white, 2));
                p.drawLine(QPointF(cb.x() + 4, cb.y() + 9), QPointF(cb.x() + 8, cb.y() + 13));
                p.drawLine(QPointF(cb.x() + 8, cb.y() + 13), QPointF(cb.x() + 14, cb.y() + 5));
            } else {
                p.setPen(QPen(QColor(255, 255, 255, 60), 1.5));
                p.setBrush(QColor(0, 0, 0, 40));
                p.drawRoundedRect(cb, 4, 4);
            }

            p.setFont(QFont("Sans", 10, QFont::Normal));
            p.setPen(Qt::white);
            p.drawText(QRectF(valueX + 28, currY, checkArea.width() - 28, rowH), Qt::AlignLeft | Qt::AlignVCenter, desc);

            currY += rowH;
        };

        drawSetting("Controls:", "Show controls while recording", m_recControls, &m_recControls);
        drawSetting("Menu bar:", "Display recording time", m_displayRecTime, &m_displayRecTime);
        drawSetting("HiDPI:", "Record at display scale resolution", m_hidpi, &m_hidpi);
        drawSetting("Notifications:", "\"Do Not Disturb\" while recording", m_doNotDisturb, &m_doNotDisturb);
        
        currY += 10.0; // Gap
        drawSetting("Cursor:", "Show cursor", m_showCursor, &m_showCursor);
        drawSetting("", "Highlight clicks", m_recClicks, &m_recClicks);
        
        currY += 10.0; // Gap
        drawSetting("Keyboard:", "Show keystrokes", m_recKeystrokes, &m_recKeystrokes);
        
        currY += 10.0; // Gap
        drawSetting("Recording area:", "Remember last selection", m_rememberSelection, &m_rememberSelection);
        drawSetting("", "Dim screen while recording", m_dimScreen, &m_dimScreen);
        drawSetting("", "Show countdown", m_showCountdown, &m_showCountdown);
    } else if (m_settingsTab == 1) { // Video
        double currY = menuY + 110.0;
        const double labelX = menuX + 20.0;
        const double valueX = menuX + 130.0;
        const double rowH = 45.0;

        auto drawLabel = [&](const QString& txt, double y) {
            p.setFont(QFont("Sans", 10, QFont::Bold));
            p.setPen(QColor(255, 255, 255, 200));
            p.drawText(QRectF(labelX, y, 100, 30), Qt::AlignRight | Qt::AlignVCenter, txt);
        };

        auto drawSubtext = [&](const QString& txt, double y) {
            p.setFont(QFont("Sans", 9));
            p.setPen(QColor(255, 255, 255, 120));
            p.drawText(QRectF(valueX, y, menuW - (valueX - menuX) - 25, 80), Qt::AlignLeft | Qt::TextWordWrap, txt);
        };

        // 1. Max resolution
        drawLabel("Max resolution:", currY);
        QRectF resBtn(valueX, currY, 140, 30);
        int resIdx = m_settingsClickableRects.size();
        p.setPen(QPen(QColor(255, 255, 255, 40), 1));
        p.setBrush(QColor(0, 0, 0, 60));
        if (m_hoveredSettingsItem == resIdx) p.setBrush(QColor(255, 255, 255, 20));
        p.drawRoundedRect(resBtn, 6, 6);
        p.setPen(Qt::white);
        p.setFont(QFont("Sans", 10));
        const QStringList resOptions = {"Original", "1080p", "720p"};
        p.drawText(resBtn.adjusted(10, 0, -25, 0), Qt::AlignLeft | Qt::AlignVCenter, resOptions[m_videoMaxRes]);
        // Chevron
        p.setPen(QPen(Qt::white, 1.5));
        p.drawLine(QPointF(resBtn.right() - 15, resBtn.center().y() - 3), QPointF(resBtn.right() - 11, resBtn.center().y() + 1));
        p.drawLine(QPointF(resBtn.right() - 11, resBtn.center().y() + 1), QPointF(resBtn.right() - 7, resBtn.center().y() - 3));
        m_settingsClickableRects.append(resBtn); 
        currY += 35;
        drawSubtext("Set maximum resolution to reduce file size and upload time.", currY);
        currY += 55;

        // 2. Video FPS
        drawLabel("Video FPS:", currY);
        QRectF fpsBtn(valueX, currY, 80, 30);
        int fpsIdx = m_settingsClickableRects.size();
        p.setPen(QPen(QColor(255, 255, 255, 40), 1));
        p.setBrush(QColor(0, 0, 0, 60));
        if (m_hoveredSettingsItem == fpsIdx) p.setBrush(QColor(255, 255, 255, 20));
        p.drawRoundedRect(fpsBtn, 6, 6);
        p.setPen(Qt::white);
        const QStringList fpsOptions = {"24", "30", "50", "60"};
        p.drawText(fpsBtn.adjusted(10, 0, -25, 0), Qt::AlignLeft | Qt::AlignVCenter, fpsOptions[m_videoFps]);
        // Chevron
        p.setPen(QPen(Qt::white, 1.5));
        p.drawLine(QPointF(fpsBtn.right() - 15, fpsBtn.center().y() - 3), QPointF(fpsBtn.right() - 11, fpsBtn.center().y() + 1));
        p.drawLine(QPointF(fpsBtn.right() - 11, fpsBtn.center().y() + 1), QPointF(fpsBtn.right() - 7, fpsBtn.center().y() - 3));
        m_settingsClickableRects.append(fpsBtn);
        currY += 50;

        // 3. Record mono
        QRectF monoRow(valueX, currY, 200, 30);
        QRectF cb1(valueX, currY + (30 - 18) / 2.0, 18, 18);
        int monoIdx = m_settingsClickableRects.size();
        if (m_recordMono) {
            p.setPen(Qt::NoPen); p.setBrush(accentColor); p.drawRoundedRect(cb1, 4, 4);
            p.setPen(QPen(Qt::white, 2));
            p.drawLine(QPointF(cb1.x() + 4, cb1.y() + 9), QPointF(cb1.x() + 8, cb1.y() + 13));
            p.drawLine(QPointF(cb1.x() + 8, cb1.y() + 13), QPointF(cb1.x() + 14, cb1.y() + 5));
        } else {
            p.setPen(QPen(QColor(255, 255, 255, 60), 1.5)); p.setBrush(QColor(0, 0, 0, 40)); p.drawRoundedRect(cb1, 4, 4);
        }
        
        bool hoveredMono = (m_hoveredSettingsItem == monoIdx);
        if (hoveredMono) {
            p.setPen(Qt::NoPen);
            p.setBrush(QColor(255, 255, 255, 12));
            p.drawRoundedRect(monoRow.adjusted(-5, 0, 5, 0), 6, 6);
        }

        p.setPen(Qt::white);
        p.drawText(QRectF(valueX + 28, currY, 172, 30), Qt::AlignLeft | Qt::AlignVCenter, "Record audio in mono");
        m_settingsClickableRects.append(monoRow);
        currY += 50;

        // 4. Video Encoder
        drawLabel("Video Encoder:", currY);
        QRectF encoderRow(valueX, currY, 250, 30);
        QRectF cb2(valueX, currY + (30 - 18) / 2.0, 18, 18);
        int encoderIdx = m_settingsClickableRects.size();
        if (m_openEditor) {
            p.setPen(Qt::NoPen); p.setBrush(accentColor); p.drawRoundedRect(cb2, 4, 4);
            p.setPen(QPen(Qt::white, 2));
            p.drawLine(QPointF(cb2.x() + 4, cb2.y() + 9), QPointF(cb2.x() + 8, cb2.y() + 13));
            p.drawLine(QPointF(cb2.x() + 8, cb2.y() + 13), QPointF(cb2.x() + 14, cb2.y() + 5));
        } else {
            p.setPen(QPen(QColor(255, 255, 255, 60), 1.5)); p.setBrush(QColor(0, 0, 0, 40)); p.drawRoundedRect(cb2, 4, 4);
        }
        
        bool hovered = (m_hoveredSettingsItem == encoderIdx);
        if (hovered) {
            p.setPen(Qt::NoPen);
            p.setBrush(QColor(255, 255, 255, 12));
            p.drawRoundedRect(encoderRow.adjusted(-5, 0, 5, 0), 6, 6);
        }

        p.setPen(Qt::white);
        p.drawText(QRectF(valueX + 28, currY, 222, 30), Qt::AlignLeft | Qt::AlignVCenter, "Open Video Editor after recording");
        m_settingsClickableRects.append(encoderRow);
        currY += 35;
        drawSubtext("Use Video Editor to change the recording quality, resolution and adjust audio settings.", currY);

    } else if (m_settingsTab == 2) { // GIF
        double currY = menuY + 110.0;
        const double labelX = menuX + 20.0;
        const double valueX = menuX + 130.0;
        const double controlW = 220.0;
        const double rowH = 45.0;

        auto drawLabel = [&](const QString& txt, double y) {
            p.setFont(QFont("Sans", 10, QFont::Bold));
            p.setPen(QColor(255, 255, 255, 200));
            p.drawText(QRectF(labelX, y, 100, 30), Qt::AlignRight | Qt::AlignVCenter, txt);
        };

        auto drawSubtext = [&](const QString& txt, double y) {
            p.setFont(QFont("Sans", 9));
            p.setPen(QColor(255, 255, 255, 120));
            p.drawText(QRectF(valueX, y, menuW - (valueX - menuX) - 25, 80), Qt::AlignLeft | Qt::TextWordWrap, txt);
        };

        // 1. GIF FPS
        drawLabel("GIF FPS:", currY);
        QRectF fpsBox(valueX, currY, 45, 30);
        p.setPen(QPen(QColor(255, 255, 255, 28), 1.0));
        p.setBrush(QColor(0, 0, 0, 80));
        p.drawRoundedRect(fpsBox, 6, 6);
        p.setPen(Qt::white);
        p.setFont(QFont("Sans", 10));
        p.drawText(fpsBox, Qt::AlignCenter, QString::number(m_gifFps));

        double sliderX = valueX + 55;
        double sliderW = 220.0; // Fixed slider width for GIF FPS
        QRectF sliderTrack(sliderX, currY + (30 - 4) / 2.0, sliderW, 4);
        m_gifFpsTrackRect = QRectF(sliderX, currY, sliderW, 30);
        p.setPen(Qt::NoPen);
        p.setBrush(QColor(255, 255, 255, 30));
        p.drawRoundedRect(sliderTrack, 2, 2);
        
        // Progress fill
        double progress = (m_gifFps - 5) / 55.0; // range 5 to 60
        QRectF progressRect(sliderX, currY + (30 - 4) / 2.0, sliderW * progress, 4);
        p.setBrush(accentColor);
        p.drawRoundedRect(progressRect, 2, 2);

        double handleX = sliderX + progress * sliderW;
        QRectF handle(handleX - 10, currY + (30 - 20) / 2.0, 20, 20);
        p.setBrush(Qt::white);
        p.drawEllipse(handle);
        m_settingsClickableRects.append(QRectF(sliderX, currY, sliderW, 30)); // index 3 in GIF tab
        
        currY += 50;

        // 2. GIF Quality
        drawLabel("GIF quality:", currY);
        double qSliderW = 160.0;
        QRectF qSliderTrack(valueX, currY + (30 - 4) / 2.0, qSliderW, 4);
        m_gifQualityTrackRect = QRectF(valueX, currY, qSliderW, 30);
        p.setPen(Qt::NoPen);
        p.setBrush(QColor(255, 255, 255, 30));
        p.drawRoundedRect(qSliderTrack, 2, 2);
        
        // Ticks
        p.setPen(QPen(QColor(255, 255, 255, 60), 1));
        for (int i = 0; i <= 8; ++i) {
            double tx = valueX + (qSliderW / 8.0) * i;
            p.drawLine(QPointF(tx, currY + 15 - 5), QPointF(tx, currY + 15 + 5));
        }

        double qHandleX = valueX + m_gifQuality * qSliderW;
        QRectF qHandle(qHandleX - 5, currY + (30 - 18) / 2.0, 10, 18);
        p.setPen(Qt::NoPen);
        p.setBrush(Qt::white);
        p.drawRoundedRect(qHandle, 3, 3);
        
        p.setFont(QFont("Sans", 8));
        p.setPen(QColor(255, 255, 255, 120));
        p.drawText(QRectF(valueX, currY + 28, 40, 20), Qt::AlignLeft, "Low");
        p.drawText(QRectF(valueX + qSliderW - 40, currY + 28, 40, 20), Qt::AlignRight, "High");
        
        m_settingsClickableRects.append(QRectF(valueX, currY, qSliderW, 30)); // index 4 in GIF tab

        // Optimize Checkbox
        QRectF optCheck(valueX + qSliderW + 10, currY, 120, 30);
        QRectF cb(optCheck.x(), currY + (30 - 18) / 2.0, 18, 18);
        if (m_optimizeGif) {
            p.setPen(Qt::NoPen); p.setBrush(accentColor); p.drawRoundedRect(cb, 4, 4);
            p.setPen(QPen(Qt::white, 2));
            p.drawLine(QPointF(cb.x() + 4, cb.y() + 9), QPointF(cb.x() + 8, cb.y() + 13));
            p.drawLine(QPointF(cb.x() + 8, cb.y() + 13), QPointF(cb.x() + 14, cb.y() + 5));
        } else {
            p.setPen(QPen(QColor(255, 255, 255, 60), 1.5)); p.setBrush(QColor(0, 0, 0, 40)); p.drawRoundedRect(cb, 4, 4);
        }
        p.setPen(Qt::white); p.setFont(QFont("Sans", 10));
        p.drawText(optCheck.adjusted(25, 0, 0, 0), Qt::AlignLeft | Qt::AlignVCenter, "Optimize GIFs");
        m_settingsClickableRects.append(optCheck); // index 5 in GIF tab

        currY += 55;
        drawSubtext("Setting the quality to maximum can speed up the processing time, but it will increase file size.", currY);
        
        currY += 60;

        // 3. GIF size
        drawLabel("GIF size:", currY);
        QRectF sizeBtn(valueX, currY, 180, 30);
        int sizeIdx = m_settingsClickableRects.size();
        p.setPen(QPen(QColor(255, 255, 255, 40), 1));
        p.setBrush(QColor(0, 0, 0, 60));
        if (m_hoveredSettingsItem == sizeIdx) p.setBrush(QColor(255, 255, 255, 20));
        p.drawRoundedRect(sizeBtn, 6, 6);
        p.setPen(Qt::white);
        const QStringList sizeOptions = {"800 x auto (default)", "640 x auto", "480 x auto", "Original"};
        p.drawText(sizeBtn.adjusted(10, 0, -25, 0), Qt::AlignLeft | Qt::AlignVCenter, sizeOptions[m_gifSizeIdx]);
        // Chevron
        p.setPen(QPen(Qt::white, 1.5));
        p.drawLine(QPointF(sizeBtn.right() - 15, sizeBtn.center().y() - 3), QPointF(sizeBtn.right() - 11, sizeBtn.center().y() + 1));
        p.drawLine(QPointF(sizeBtn.right() - 11, sizeBtn.center().y() + 1), QPointF(sizeBtn.right() - 7, sizeBtn.center().y() - 3));
        m_settingsClickableRects.append(sizeBtn); // index 6 in GIF tab
        
        currY += 35;
        drawSubtext("Set maximum resolution of your GIFs. Changing it will affect file size and quality. ApexShot will only downscale the GIF if needed.", currY);
    }

    if (m_clickOptionsOpen) {
        drawClickOptions(p, m_settingsPanelRect);
    }
    if (m_keystrokeOptionsOpen) {
        drawKeystrokeOptions(p, m_settingsPanelRect);
    }

    if (m_dropdownOpen != -1) {
        drawDropdownPopup(p, m_dropdownAnchor, m_dropdownOptions, 
                          m_dropdownValuePtr ? *m_dropdownValuePtr : -1);
    }
}

void CaptureOverlay::drawDropdownPopup(QPainter& p, const QRectF& anchorRect,
                                        const QStringList& options, int selectedIndex)
{
    if (options.isEmpty()) return;

    p.save();
    p.setRenderHint(QPainter::Antialiasing);

    const double itemH = 34.0;
    const double menuW = std::max(anchorRect.width(), 160.0);
    const double menuH = options.size() * itemH + 10.0;
    
    double menuX = anchorRect.x();
    double menuY = anchorRect.bottom() + 4.0;
    
    // Check screen bounds
    if (menuX + menuW > width() - 10) menuX = width() - menuW - 10;
    if (menuY + menuH > height() - 10) menuY = anchorRect.top() - menuH - 4.0;

    QRectF menuRect(menuX, menuY, menuW, menuH);
    const QColor accentColor(176, 92, 56);
    const QColor accentRim(255, 214, 186);
    
    // Background
    p.setPen(QPen(QColor(255, 255, 255, 34), 1));
    p.setBrush(QColor(24, 20, 20, 244));
    p.drawRoundedRect(menuRect, 10, 10);

    m_dropdownItemRects.clear();
    const bool hasColors = !m_dropdownColors.isEmpty();
    for (int i = 0; i < options.size(); ++i) {
        QRectF itemRect(menuX + 5, menuY + 5 + i * itemH, menuW - 10, itemH);
        m_dropdownItemRects.append(itemRect);

        bool hovered = (m_hoveredDropdownItem == i);
        if (hovered) {
            p.setPen(QPen(accentRim, 1.0));
            p.setBrush(QColor(accentColor.red(), accentColor.green(), accentColor.blue(), 170));
            p.drawRoundedRect(itemRect, 7, 7);
        }

        // Left-aligned content
        double textX = itemRect.x() + 10;

        // Color circle (left side)
        if (hasColors && i < m_dropdownColors.size()) {
            p.setPen(Qt::NoPen);
            p.setBrush(m_dropdownColors[i]);
            p.drawEllipse(QPointF(itemRect.x() + 18, itemRect.center().y()), 7, 7);
            textX = itemRect.x() + 34;
        }

        p.setPen(selectedIndex == i ? QColor(255, 236, 220) : Qt::white);
        p.setFont(QFont("Sans", 10, selectedIndex == i ? QFont::Bold : QFont::Normal));
        p.drawText(QRectF(textX, itemRect.y(), itemRect.right() - textX - 10, itemRect.height()),
                   Qt::AlignLeft | Qt::AlignVCenter, options[i]);
        
        if (selectedIndex == i) {
            p.setBrush(accentRim);
            p.setPen(Qt::NoPen);
            p.drawEllipse(QPointF(itemRect.right() - 15, itemRect.center().y()), 2.5, 2.5);
        }
    }
    p.restore();
}

void CaptureOverlay::drawClickOptions(QPainter& p, const QRectF& parentRect)
{
    const double menuW = 440.0;
    const double menuH = 500.0;
    const double menuX = parentRect.x();
    const double menuY = parentRect.y();
    
    m_clickOptionsPanelRect = parentRect;
    m_clickOptionsClickableRects.clear();

    const QColor accentColor(176, 92, 56);
    const QColor accentRim(255, 214, 186);
    const QImage* blurPtr = m_blurredBg.isNull() ? nullptr : &m_blurredBg;

    // Color palette
    static const QStringList colorNames = {
        "Gray", "Indigo", "Red", "Blue", "Green", "Yellow", "Orange", "Purple", "White"
    };
    static const QList<QColor> colorValues = {
        QColor(180, 180, 180),  // Gray
        QColor(122, 100, 255),  // Indigo
        QColor(255, 60, 60),    // Red
        QColor(60, 120, 255),   // Blue
        QColor(60, 200, 80),    // Green
        QColor(255, 210, 50),   // Yellow
        QColor(255, 150, 30),   // Orange
        QColor(180, 60, 220),   // Purple
        QColor(255, 255, 255),  // White
    };
    auto clickColorValue = [&](int idx) -> QColor {
        return (idx >= 0 && idx < colorValues.size()) ? colorValues[idx] : colorValues[0];
    };

    // Redraw base panel to "overlay" the settings menu (or we could just draw over it)
    // Actually, drawing over it is fine.
    p.save();
    QRadialGradient glow(menuX + menuW/2.0, menuY + menuH/2.0, menuW);
    glow.setColorAt(0, QColor(accentColor.red(), accentColor.green(), accentColor.blue(), 42));
    glow.setColorAt(0.6, QColor(0, 0, 0, 0));
    p.fillRect(QRectF(menuX - 40, menuY - 40, menuW + 80, menuH + 80), glow);
    p.restore();
    drawFrostedPanel(p, menuX, menuY, menuW, menuH, 12.0, blurPtr, width(), height());
    p.setFont(QFont("Sans", 8, QFont::Bold));
    p.setPen(QColor(255, 224, 196, 176));
    p.drawText(QRectF(menuX + 18.0, menuY + 18.0, menuW - 36.0, 12.0),
               Qt::AlignLeft | Qt::AlignVCenter, QStringLiteral("CLICK HIGHLIGHTS"));
    p.setFont(QFont("Sans", 14, QFont::Bold));
    p.setPen(QColor(245, 245, 246));
    p.drawText(QRectF(menuX + 18.0, menuY + 28.0, menuW - 36.0, 22.0),
               Qt::AlignLeft | Qt::AlignVCenter, QStringLiteral("Click Overlay"));

    const double labelX = menuX + 25.0;
    const double valueX = menuX + 130.0;
    const double controlW = 280.0;
    const double rowH = 45.0;
    double currY = menuY + 78.0;

    auto drawLabel = [&](const QString& txt) {
        p.setFont(QFont("Sans", 10, QFont::Bold));
        p.setPen(QColor(255, 255, 255, 200));
        p.drawText(QRectF(labelX, currY, 90, rowH), Qt::AlignRight | Qt::AlignVCenter, txt);
    };

    // 1. Size Slider
    drawLabel("Size:");
    QRectF sliderTrack(valueX, currY + (rowH - 4) / 2.0, controlW, 4);
    m_sliderTrackRect = sliderTrack;
    p.setPen(Qt::NoPen);
    p.setBrush(QColor(255, 255, 255, 30));
    p.drawRoundedRect(sliderTrack, 2, 2);
    
    double handleX = valueX + m_clickSize * controlW;
    QRectF handle(handleX - 8, currY + (rowH - 24) / 2.0, 16, 24);
    p.setBrush(Qt::white);
    p.drawRoundedRect(handle, 4, 4);
    m_clickOptionsClickableRects.append(QRectF(valueX, currY, controlW, rowH)); // index 0: slider

    currY += rowH;

    // 2. Color Dropdown
    drawLabel("Color:");
    QRectF colorBtn(valueX, currY + (rowH - 30) / 2.0, 160, 30);
    p.setPen(QPen(QColor(255, 255, 255, 40), 1));
    p.setBrush(QColor(0, 0, 0, 60));
    p.drawRoundedRect(colorBtn, 6, 6);
    
    // Color circle
    p.setPen(Qt::NoPen);
    p.setBrush(clickColorValue(m_clickColor));
    p.drawEllipse(QPointF(colorBtn.x() + 15, colorBtn.center().y()), 7, 7);
    
    p.setPen(Qt::white);
    p.setFont(QFont("Sans", 10));
    p.drawText(colorBtn.adjusted(30, 0, -20, 0), Qt::AlignLeft | Qt::AlignVCenter, colorNames[m_clickColor]);
    
    // Chevron
    p.setPen(QPen(Qt::white, 1.5));
    p.drawLine(QPointF(colorBtn.right() - 15, colorBtn.center().y() - 3), QPointF(colorBtn.right() - 11, colorBtn.center().y() + 1));
    p.drawLine(QPointF(colorBtn.right() - 11, colorBtn.center().y() + 1), QPointF(colorBtn.right() - 7, colorBtn.center().y() - 3));
    
    m_clickOptionsClickableRects.append(colorBtn); // index 1: color

    currY += rowH;

    // 3. Style Dropdown
    drawLabel("Style:");
    QRectF styleBtn(valueX, currY + (rowH - 30) / 2.0, 80, 30);
    p.setPen(QPen(QColor(255, 255, 255, 40), 1));
    p.setBrush(QColor(0, 0, 0, 60));
    p.drawRoundedRect(styleBtn, 6, 6);
    
    p.setPen(Qt::white);
    p.drawText(styleBtn.adjusted(10, 0, -20, 0), Qt::AlignLeft | Qt::AlignVCenter, "Outline");
    
    p.setPen(QPen(Qt::white, 1.5));
    p.drawLine(QPointF(styleBtn.right() - 15, styleBtn.center().y() - 3), QPointF(styleBtn.right() - 11, styleBtn.center().y() + 1));
    p.drawLine(QPointF(styleBtn.right() - 11, styleBtn.center().y() + 1), QPointF(styleBtn.right() - 7, styleBtn.center().y() - 3));

    m_clickOptionsClickableRects.append(styleBtn); // index 2: style

    currY += rowH;

    // 4. Animation Checkbox
    drawLabel("Animation:");
    QRectF animRow(valueX, currY, controlW, rowH);
    QRectF cb(valueX, currY + (rowH - 18) / 2.0, 18, 18);
    if (m_clickAnimate) {
        p.setPen(Qt::NoPen);
        p.setBrush(accentColor);
        p.drawRoundedRect(cb, 4, 4);
        p.setPen(QPen(Qt::white, 2));
        p.drawLine(QPointF(cb.x() + 4, cb.y() + 9), QPointF(cb.x() + 8, cb.y() + 13));
        p.drawLine(QPointF(cb.x() + 8, cb.y() + 13), QPointF(cb.x() + 14, cb.y() + 5));
    } else {
        p.setPen(QPen(QColor(255, 255, 255, 60), 1.5));
        p.setBrush(QColor(0, 0, 0, 40));
        p.drawRoundedRect(cb, 4, 4);
    }
    p.setPen(Qt::white);
    p.drawText(QRectF(valueX + 28, currY, controlW - 28, rowH), Qt::AlignLeft | Qt::AlignVCenter, "Animate clicks");
    
    m_clickOptionsClickableRects.append(animRow); // index 3: animate

    currY += rowH + 10;

    // 5. Preview Area
    QRectF previewArea(menuX + 20, currY, menuW - 40, 130);
    p.setPen(QPen(QColor(255, 255, 255, 20), 1));
    p.setBrush(QColor(0, 0, 0, 40));
    p.drawRoundedRect(previewArea, 10, 10);
    
    // Draw click previews (each lives for 1.5 seconds then fades out)
    const qint64 CLICK_LIFETIME_MS = 1500;
    if (m_clickPreviews.isEmpty()) {
        p.setPen(QColor(255, 255, 255, 120));
        p.drawText(previewArea, Qt::AlignCenter, "Click here to preview");
    } else {
        p.save();
        p.setClipRect(previewArea.adjusted(1, 1, -1, -1));

        const double baseRadius = 6.0 + m_clickSize * 28.0; // 6 to 34
        const QColor clickColor = clickColorValue(m_clickColor);
        const qint64 now = QDateTime::currentMSecsSinceEpoch();

        for (int i = 0; i < m_clickPreviews.size(); ++i) {
            const auto& cp = m_clickPreviews[i];
            QPointF pt = cp.pos;
            if (!previewArea.contains(pt)) continue;

            qint64 age = now - cp.birthMs;
            double progress = std::min(1.0, (double)age / CLICK_LIFETIME_MS); // 0→1 over lifetime
            double fadeAlpha = 1.0 - progress; // full opacity → zero

            if (fadeAlpha <= 0.01) continue; // expired, skip (will be removed by timer)

            double radius = baseRadius;

            // Animation: pulsing ring that expands outward
            if (m_clickAnimate) {
                double phase = std::fmod((double)age / 500.0, 1.0); // cycle every 500ms
                double pulseRadius = radius + phase * 25.0;
                double pulseAlpha = (1.0 - phase) * 200.0 * fadeAlpha;

                if (pulseAlpha > 1.0) {
                    QColor pulseColor = clickColor;
                    pulseColor.setAlpha((int)pulseAlpha);
                    p.setPen(QPen(pulseColor, 2));
                    p.setBrush(Qt::NoBrush);
                    p.drawEllipse(pt, pulseRadius, pulseRadius);
                }
            }

            // Main click circle — fade out based on age
            QColor c = clickColor;
            c.setAlpha((int)(255 * fadeAlpha));
            if (m_clickStyle == 1) { // Filled
                p.setPen(Qt::NoPen);
                p.setBrush(c);
                p.drawEllipse(pt, radius, radius);
            } else { // Outline
                p.setPen(QPen(c, 3));
                p.setBrush(Qt::NoBrush);
                p.drawEllipse(pt, radius, radius);
            }
        }
        p.restore();
    }
    
    m_clickOptionsClickableRects.append(previewArea); // index 4: preview

    // 6. OK Button
    QRectF okBtn(menuX + menuW - 90, menuY + menuH - 45, 70, 30);
    p.setPen(Qt::NoPen);
    p.setBrush(QColor(accentColor.red(), accentColor.green(), accentColor.blue(), 210));
    p.drawRoundedRect(okBtn, 6, 6);
    p.setPen(accentRim);
    p.setFont(QFont("Sans", 10, QFont::Bold));
    p.drawText(okBtn, Qt::AlignCenter, "OK");
    
    m_clickOptionsClickableRects.append(okBtn); // index 5: OK
}

void CaptureOverlay::drawKeystrokeOptions(QPainter& p, const QRectF& parentRect)
{
    const double menuW = 440.0;
    const double menuH = 500.0;
    const double menuX = parentRect.x();
    const double menuY = parentRect.y();
    
    m_keystrokeOptionsPanelRect = parentRect;
    m_keystrokeOptionsClickableRects.clear();

    const QColor accentColor(176, 92, 56);
    const QColor accentRim(255, 214, 186);
    const QImage* blurPtr = m_blurredBg.isNull() ? nullptr : &m_blurredBg;

    p.save();
    QRadialGradient glow(menuX + menuW/2.0, menuY + menuH/2.0, menuW);
    glow.setColorAt(0, QColor(accentColor.red(), accentColor.green(), accentColor.blue(), 42));
    glow.setColorAt(0.6, QColor(0, 0, 0, 0));
    p.fillRect(QRectF(menuX - 40, menuY - 40, menuW + 80, menuH + 80), glow);
    p.restore();
    drawFrostedPanel(p, menuX, menuY, menuW, menuH, 12.0, blurPtr, width(), height());
    p.setFont(QFont("Sans", 8, QFont::Bold));
    p.setPen(QColor(255, 224, 196, 176));
    p.drawText(QRectF(menuX + 18.0, menuY + 18.0, menuW - 36.0, 12.0),
               Qt::AlignLeft | Qt::AlignVCenter, QStringLiteral("KEYSTROKE DISPLAY"));
    p.setFont(QFont("Sans", 14, QFont::Bold));
    p.setPen(QColor(245, 245, 246));
    p.drawText(QRectF(menuX + 18.0, menuY + 28.0, menuW - 36.0, 22.0),
               Qt::AlignLeft | Qt::AlignVCenter, QStringLiteral("Keyboard Overlay"));

    const double labelX = menuX + 20.0;
    const double valueX = menuX + 130.0;
    const double controlW = 280.0;
    const double rowH = 45.0;
    double currY = menuY + 78.0;

    auto drawLabel = [&](const QString& txt) {
        p.setFont(QFont("Sans", 10, QFont::Bold));
        p.setPen(QColor(255, 255, 255, 200));
        p.drawText(QRectF(labelX, currY, 100, rowH), Qt::AlignRight | Qt::AlignVCenter, txt);
    };

    // 1. Size
    drawLabel("Size:");
    QRectF sliderTrack(valueX, currY + (rowH - 4) / 2.0, controlW, 4);
    m_keySliderTrackRect = sliderTrack;
    p.setPen(Qt::NoPen);
    p.setBrush(QColor(255, 255, 255, 30));
    p.drawRoundedRect(sliderTrack, 2, 2);
    // Draw tick marks
    p.setPen(QPen(QColor(255, 255, 255, 60), 1));
    for (int i = 0; i <= 4; ++i) {
        double tx = valueX + (controlW / 4.0) * i;
        p.drawLine(QPointF(tx, currY + rowH/2.0 - 6), QPointF(tx, currY + rowH/2.0 + 6));
    }
    double handleX = valueX + m_keySize * controlW;
    QRectF handle(handleX - 8, currY + (rowH - 24) / 2.0, 16, 24);
    p.setPen(Qt::NoPen);
    p.setBrush(Qt::white);
    p.drawRoundedRect(handle, 4, 4);
    m_keystrokeOptionsClickableRects.append(QRectF(valueX, currY, controlW, rowH)); // index 0

    currY += rowH;

    // 2. Position
    drawLabel("Position:");
    QRectF posBtn(valueX, currY + (rowH - 30) / 2.0, 160, 30);
    p.setPen(QPen(QColor(255, 255, 255, 40), 1));
    p.setBrush(QColor(0, 0, 0, 60));
    p.drawRoundedRect(posBtn, 6, 6);
    p.setPen(Qt::white);
    p.setFont(QFont("Sans", 10));
    const QStringList posNames = {"Bottom-Center", "Bottom-Left", "Bottom-Right", "Top-Center", "Top-Left", "Top-Right"};
    p.drawText(posBtn.adjusted(10, 0, -20, 0), Qt::AlignLeft | Qt::AlignVCenter, 
               (m_keyPosition >= 0 && m_keyPosition < posNames.size()) ? posNames[m_keyPosition] : "Bottom-Center");
    // Chevron
    p.setPen(QPen(Qt::white, 1.5));
    p.drawLine(QPointF(posBtn.right() - 15, posBtn.center().y() - 3), QPointF(posBtn.right() - 11, posBtn.center().y() + 1));
    p.drawLine(QPointF(posBtn.right() - 11, posBtn.center().y() + 1), QPointF(posBtn.right() - 7, posBtn.center().y() - 3));
    m_keystrokeOptionsClickableRects.append(posBtn); // index 1

    currY += rowH;

    // 3. Appearance
    drawLabel("Appearance:");
    QRectF appBtn(valueX, currY + (rowH - 30) / 2.0, 100, 30);
    p.setPen(QPen(QColor(255, 255, 255, 40), 1));
    p.setBrush(QColor(0, 0, 0, 60));
    p.drawRoundedRect(appBtn, 6, 6);
    p.setPen(Qt::white);
    const QStringList appNames = {"Dark", "Light"};
    p.drawText(appBtn.adjusted(10, 0, -20, 0), Qt::AlignLeft | Qt::AlignVCenter, 
               (m_keyAppearance >= 0 && m_keyAppearance < appNames.size()) ? appNames[m_keyAppearance] : "Dark");
    // Chevron
    p.setPen(QPen(Qt::white, 1.5));
    p.drawLine(QPointF(appBtn.right() - 15, appBtn.center().y() - 3), QPointF(appBtn.right() - 11, appBtn.center().y() + 1));
    p.drawLine(QPointF(appBtn.right() - 11, appBtn.center().y() + 1), QPointF(appBtn.right() - 7, appBtn.center().y() - 3));
    m_keystrokeOptionsClickableRects.append(appBtn); // index 2

    currY += rowH;

    // 4. Blur background
    QRectF blurRow(valueX, currY, controlW, 30);
    QRectF cb(valueX, currY + (30 - 18) / 2.0, 18, 18);
    p.setRenderHint(QPainter::Antialiasing);
    if (m_keyBlurBg) {
        p.setPen(Qt::NoPen); p.setBrush(accentColor); p.drawRoundedRect(cb, 4, 4);
        p.setPen(QPen(Qt::white, 2));
        p.drawLine(QPointF(cb.x() + 4, cb.y() + 9), QPointF(cb.x() + 8, cb.y() + 13));
        p.drawLine(QPointF(cb.x() + 8, cb.y() + 13), QPointF(cb.x() + 14, cb.y() + 5));
    } else {
        p.setPen(QPen(QColor(255, 255, 255, 60), 1.5)); p.setBrush(QColor(0, 0, 0, 40)); p.drawRoundedRect(cb, 4, 4);
    }
    p.setPen(Qt::white); p.setFont(QFont("Sans", 10));
    p.drawText(QRectF(valueX + 28, currY, controlW - 28, 30), Qt::AlignLeft | Qt::AlignVCenter, "Blur background");
    m_keystrokeOptionsClickableRects.append(blurRow); // index 3

    currY += rowH;

    // 5. Keystrokes
    drawLabel("Keystrokes:");
    auto drawRadio = [&](const QString& txt, bool active, double y) {
        QRectF row(valueX, y, controlW, 30);
        QRectF rb(valueX, y + (30 - 18) / 2.0, 18, 18);
        p.setPen(QPen(QColor(255, 255, 255, 60), 1.5));
        p.setBrush(QColor(0, 0, 0, 40));
        p.drawEllipse(rb);
        if (active) {
            p.setPen(Qt::NoPen); p.setBrush(accentColor); p.drawEllipse(rb.adjusted(3, 3, -3, -3));
            // Inner white dot for "modern radio" look matching screenshot
            p.setBrush(Qt::white); p.drawEllipse(rb.center(), 2.5, 2.5);
        }
        p.setPen(Qt::white);
        p.drawText(QRectF(valueX + 28, y, controlW - 28, 30), Qt::AlignLeft | Qt::AlignVCenter, txt);
        return row;
    };
    m_keystrokeOptionsClickableRects.append(drawRadio("Show all keys", m_keyFilter == 0, currY)); // index 4
    currY += 32;
    m_keystrokeOptionsClickableRects.append(drawRadio("Show only command keys", m_keyFilter == 1, currY)); // index 5

    currY += 40;
    p.setFont(QFont("Sans", 9));
    p.setPen(QColor(255, 255, 255, 120));
    p.drawText(QRectF(valueX, currY, 220, 50), Qt::AlignLeft | Qt::TextWordWrap, 
               "ApexShot can't display any keystrokes you make within password fields.");

    // OK / Preview buttons
    QRectF prevBtn(menuX + 15, menuY + menuH - 45, 90, 30);
    p.setPen(Qt::NoPen); 
    p.setBrush(m_showKeystrokePreview ? QColor(accentColor.red(), accentColor.green(), accentColor.blue(), 190)
                                      : QColor(255, 255, 255, 24)); 
    p.drawRoundedRect(prevBtn, 6, 6);
    p.setPen(m_showKeystrokePreview ? accentRim : QColor(245, 245, 246));
    p.setFont(QFont("Sans", 10, m_showKeystrokePreview ? QFont::Bold : QFont::Normal)); 
    p.drawText(prevBtn, Qt::AlignCenter, "Preview");
    m_keystrokeOptionsClickableRects.append(prevBtn); // index 6

    QRectF okBtn(menuX + menuW - 90, menuY + menuH - 45, 75, 30);
    p.setPen(Qt::NoPen); p.setBrush(QColor(accentColor.red(), accentColor.green(), accentColor.blue(), 210)); p.drawRoundedRect(okBtn, 6, 6);
    p.setPen(accentRim); p.setFont(QFont("Sans", 10, QFont::Bold)); p.drawText(okBtn, Qt::AlignCenter, "OK");
    m_keystrokeOptionsClickableRects.append(okBtn); // index 7
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

    int activeTool = 0;
    if (scrollModeActive) {
        activeTool = 3;
    }
    if (m_fullscreenMode) {
        activeTool = 1;
    }
    if (m_captureIntent == CaptureIntent::Ocr) {
        activeTool = 5;
    }
    if (m_captureIntent == CaptureIntent::Record) {
        activeTool = 6;
    }

    const bool timerToolEnabled = m_timerCaptureEnabled && !scrollModeActive;
    const bool timerToolActive = timerToolEnabled && m_timerDelayActive && m_captureDelaySeconds > 0;

    drawFrostedPanel(p,
                     layout.leftToolsPanel.x(), layout.leftToolsPanel.y(),
                     layout.leftToolsPanel.width(), layout.leftToolsPanel.height(),
                     FEATURE_PANEL_RADIUS, blurPtr, screenW, screenH);

    drawFrostedPanel(p,
                     layout.topCluster.x(), layout.topCluster.y(),
                     layout.topCluster.width(), layout.topCluster.height(),
                     FEATURE_PANEL_RADIUS, blurPtr, screenW, screenH);

    drawFrostedPanel(p,
                     layout.rightActionsPanel.x(), layout.rightActionsPanel.y(),
                     layout.rightActionsPanel.width(), layout.rightActionsPanel.height(),
                     12.0, blurPtr, screenW, screenH);

    auto drawAccentCard = [&](const QRectF& rect,
                              const QColor& fill,
                              const QColor& rim,
                              double radius,
                              bool drawBorder = true) {
        const double hx = rect.x() + 4.0;
        const double hy = rect.y() + 4.0;
        const double hw = rect.width() - 8.0;
        const double hh = rect.height() - 8.0;

        QPainterPath card;
        roundedRectPath(card, hx, hy, hw, hh, radius);
        p.fillPath(card, fill);

        p.save();
        p.setClipPath(card);
        if (drawBorder) {
            p.setPen(QPen(rim, 1.2));
            p.setBrush(Qt::NoBrush);
            QPainterPath border;
            roundedRectPath(border, hx + 0.6, hy + 0.6, hw - 1.2, hh - 1.2, std::max(0.0, radius - 0.5));
            p.drawPath(border);
        }
        p.restore();
    };

    auto drawActiveToolCell = [&](int toolIndex) {
        if (toolIndex < 0 || toolIndex >= NUM_TOOLS) {
            return;
        }

        drawAccentCard(
            layout.toolCells[toolIndex],
            QColor(176, 92, 56, 76),
            QColor(255, 212, 178, 152),
            10.0,
            false
        );
    };

    drawActiveToolCell(activeTool);
    if (timerToolActive && activeTool != 4) {
        drawActiveToolCell(4);
    }

    // ── Hover highlight on hovered tool ──────────────────────────────────────
    if (m_hoveredTool >= 0 && m_hoveredTool < NUM_TOOLS) {
        drawAccentCard(
            layout.toolCells[m_hoveredTool],
            QColor(255, 255, 255, 22),
            QColor(255, 255, 255, 86),
            10.0,
            false
        );
    }

    if (m_hoveredSizeCard) {
        drawAccentCard(
            layout.sizeCard,
            QColor(255, 255, 255, 40),
            QColor(255, 255, 255, 136),
            9.0,
            false
        );
    }
    if (m_hoveredCaptureCropCard || m_captureCropMenuOpen || m_captureAspectRatioIndex > 0) {
        drawAccentCard(
            layout.cropCard,
            (m_captureCropMenuOpen || m_captureAspectRatioIndex > 0)
                ? QColor(176, 92, 56, 76)
                : QColor(255, 255, 255, 22),
            (m_captureCropMenuOpen || m_captureAspectRatioIndex > 0)
                ? QColor(255, 212, 178, 152)
                : QColor(255, 255, 255, 86),
            9.0,
            false
        );
    }

    // ── Tool icons + labels ───────────────────────────────────────────────────
    for (int i = 0; i < NUM_TOOLS; ++i) {
        QRectF cell = layout.toolCells[i];
        double cx = cell.x() + cell.width() / 2.0;
        bool hovered = (m_hoveredTool == i);
        bool active = (activeTool == i) || (i == 4 && timerToolActive);
        double iconAlpha = (hovered || active) ? 1.0 : 0.94;
        double shadowAlpha = hovered ? 0.24 : (active ? 0.32 : 0.50);
        double iconY = cell.y() + ((hovered || active) ? 23.5 : 24.0);
        QColor iconColor = active
            ? QColor(255, 229, 206, int(iconAlpha * 255))
            : QColor(255, 255, 255, int(iconAlpha * 255));

        drawToolbarIcon(p, TOOLBAR_ICON_IDS[i], cx + 0.6, iconY + 0.8,
                        QColor(0,0,0, int(shadowAlpha*255)));
        drawToolbarIcon(p, TOOLBAR_ICON_IDS[i], cx, iconY, iconColor);

        QFont f; f.setFamily("Sans"); f.setPointSizeF(7.1);
        f.setBold(hovered || active); p.setFont(f);
        QFontMetricsF fm(f);
        QString label(TOOLBAR_LABELS[i]);

        p.setPen(QColor(0,0,0, int(shadowAlpha*255)));
        double tw = fm.horizontalAdvance(label);
        p.drawText(QPointF(cx - tw/2.0 + 0.6,
                           cell.y() + 50.0 + 0.8), label);
        p.setPen(active
            ? QColor(255,229,206, int(iconAlpha * 255))
            : QColor(244,244,244, int(iconAlpha * 255)));
        p.drawText(QPointF(cx - tw/2.0,
                           cell.y() + 50.0), label);

        if (i == 4 && timerToolActive) {
            const QString badgeText = QStringLiteral("%1s").arg(m_captureDelaySeconds);
            QFont badgeFont; badgeFont.setFamily("Sans"); badgeFont.setPointSizeF(6.6); badgeFont.setBold(true);
            p.setFont(badgeFont);
            QFontMetricsF badgeMetrics(badgeFont);
            const double badgeTextW = badgeMetrics.horizontalAdvance(badgeText);
            const double badgeW = std::max(22.0, badgeTextW + 10.0);
            const QRectF badgeRect(cell.right() - badgeW - 6.0, cell.y() + 6.0, badgeW, 14.0);
            QPainterPath badgePath;
            roundedRectPath(badgePath, badgeRect.x(), badgeRect.y(), badgeRect.width(), badgeRect.height(), 7.0);
            p.fillPath(badgePath, QColor(178, 84, 42, 230));
            p.setPen(QColor(255, 255, 255, 248));
            p.drawText(badgeRect, Qt::AlignCenter, badgeText);
        }
    }

    // ── Right rail text ───────────────────────────────────────────────────────
    double scx = layout.sizeCard.x() + layout.sizeCard.width() / 2.0;
    QString sizeVal = QString("%1×%2").arg((int)selW).arg((int)selH);

    {
        QFont f; f.setFamily("Sans"); f.setPointSizeF(7.2); f.setBold(true); p.setFont(f);
        QFontMetricsF fm(f);
        const QString header = QStringLiteral("FRAME");
        double tw = fm.horizontalAdvance(header);
        double ty = layout.sizeCard.y() + 17.0;
        p.setPen(QColor(0,0,0,128));
        p.drawText(QPointF(scx - tw/2.0 + 0.6, ty + 0.8), header);
        p.setPen(QColor(255,224,196,214));
        p.drawText(QPointF(scx - tw/2.0, ty), header);
    }
    {
        QFont f; f.setFamily("Sans"); f.setPointSizeF(9.4); f.setBold(true); p.setFont(f);
        QFontMetricsF fm(f);
        double tw = fm.horizontalAdvance(sizeVal);
        double ty = layout.sizeCard.y() + 39.0;
        p.setPen(QColor(0,0,0,140));
        p.drawText(QPointF(scx - tw/2.0 + 0.6, ty + 0.8), sizeVal);
        p.setPen(QColor(255,255,255,248));
        p.drawText(QPointF(scx - tw/2.0, ty), sizeVal);
    }

    {
        const QRectF cropRect = layout.cropCard;
        const bool hovered = m_hoveredCaptureCropCard;
        const bool active = m_captureCropMenuOpen || m_captureAspectRatioIndex > 0;
        const QColor iconColor = active
            ? QColor(255, 229, 206)
            : QColor(255, 255, 255, 242);
        const double cx = cropRect.center().x();
        const double iconY = cropRect.y() + ((hovered || active) ? 27.0 : 27.5);

        drawToolbarIcon(p, 10, cx + 0.6, iconY + 0.8, QColor(0, 0, 0, hovered ? 62 : 118));
        drawToolbarIcon(p, 10, cx, iconY, iconColor);
    }

    auto drawActionLabel = [&](const QRectF& rect, const QString& text, bool primary) {
        QFont f; f.setFamily("Sans"); f.setPointSizeF(9.0); f.setBold(true); p.setFont(f);
        p.setPen(QColor(0, 0, 0, primary ? 132 : 118));
        p.drawText(rect.translated(0.6, 0.8), Qt::AlignCenter, text);
        p.setPen(primary ? QColor(255, 231, 214, 248) : QColor(244, 244, 244, 244));
        p.drawText(rect, Qt::AlignCenter, text);
    };

    drawAccentCard(
        layout.confirmCard,
        m_hoveredActionCard == ToolbarActionCard::Confirm
            ? QColor(176, 92, 56, 88)
            : QColor(176, 92, 56, 60),
        QColor(255, 212, 178, 214),
        8.0
    );
    drawAccentCard(
        layout.cancelCard,
        m_hoveredActionCard == ToolbarActionCard::Cancel
            ? QColor(255, 255, 255, 34)
            : QColor(255, 255, 255, 18),
        QColor(255, 255, 255, 112),
        8.0
    );

    drawActionLabel(layout.confirmCard, QStringLiteral("CAPTURE"), true);
    drawActionLabel(layout.cancelCard, QStringLiteral("CANCEL"), false);

    if (m_captureIntent != CaptureIntent::Record && m_captureCropMenuOpen) {
        const QRectF cropRect = layout.cropCard;
        const double itemH = 34.0;
        const double menuW = 196.0;
        const double menuH = (kRecordingAspectOptionCount * itemH) + 10.0;
        const double menuX = std::max(10.0, std::min(cropRect.center().x() - (menuW / 2.0), screenW - menuW - 10.0));
        const double menuY = std::max(10.0, std::min(cropRect.bottom() + 8.0, screenH - menuH - 10.0));
        m_captureCropMenuPanelRect = QRectF(menuX, menuY, menuW, menuH);
        m_captureCropMenuItemRects.clear();

        drawFrostedPanel(p, menuX, menuY, menuW, menuH, 12.0, blurPtr, screenW, screenH);

        for (int i = 0; i < kRecordingAspectOptionCount; ++i) {
            const QRectF itemRect(menuX + 5.0, menuY + 5.0 + (i * itemH), menuW - 10.0, itemH);
            const QRectF indicatorRect(itemRect.x() + 8.0, itemRect.y(), 18.0, itemRect.height());
            const QRectF labelRect(itemRect.x() + 30.0, itemRect.y(), itemRect.width() - 40.0, itemRect.height());
            m_captureCropMenuItemRects.append(itemRect);

            if (i == m_hoveredCaptureCropMenuItem) {
                p.setPen(Qt::NoPen);
                p.setBrush(QColor(255, 255, 255, 18));
                p.drawRoundedRect(itemRect, 7.0, 7.0);
            }

            const bool selected = (i == m_captureAspectRatioIndex);
            if (selected) {
                p.setPen(Qt::NoPen);
                p.setBrush(QColor(176, 92, 56, 94));
                p.drawRoundedRect(itemRect.adjusted(1.0, 1.0, -1.0, -1.0), 7.0, 7.0);
                p.setPen(QPen(QColor(255, 238, 224), 1.5));
                const double cy = indicatorRect.center().y();
                p.drawLine(QPointF(indicatorRect.x() + 3.5, cy), QPointF(indicatorRect.x() + 6.5, cy + 3.0));
                p.drawLine(QPointF(indicatorRect.x() + 6.5, cy + 3.0), QPointF(indicatorRect.x() + 12.5, cy - 4.0));
            }

            QFont itemFont(QStringLiteral("Sans"));
            itemFont.setPointSizeF(10.0);
            itemFont.setBold(selected);
            p.setFont(itemFont);
            p.setPen(selected ? QColor(255, 240, 226) : QColor(242, 242, 244));
            p.drawText(labelRect, Qt::AlignVCenter | Qt::AlignLeft,
                       QString::fromUtf8(kRecordingAspectOptions[i].label));
        }
    } else {
        m_captureCropMenuPanelRect = QRectF();
        m_captureCropMenuItemRects.clear();
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

void CaptureOverlay::startClickAnimTimer()
{
    if (!m_clickAnimTimer) {
        m_clickAnimTimer = new QTimer(this);
        m_clickAnimTimer->setInterval(16); // ~60fps
        connect(m_clickAnimTimer, &QTimer::timeout, this, [this]() {
            const qint64 CLICK_LIFETIME_MS = 1500;
            const qint64 now = QDateTime::currentMSecsSinceEpoch();

            // Remove expired click previews
            m_clickPreviews.erase(
                std::remove_if(m_clickPreviews.begin(), m_clickPreviews.end(),
                    [&](const ClickPreview& cp) { return (now - cp.birthMs) >= CLICK_LIFETIME_MS; }),
                m_clickPreviews.end());

            // Remove expired key previews
            const qint64 KEY_LIFETIME_MS = 2000;
            m_keyPreviews.erase(
                std::remove_if(m_keyPreviews.begin(), m_keyPreviews.end(),
                    [&](const KeyPreview& kp) { return (now - kp.birthMs) >= KEY_LIFETIME_MS; }),
                m_keyPreviews.end());

            if (m_clickPreviews.isEmpty() && m_keyPreviews.isEmpty()) {
                stopClickAnimTimer();
                return;
            }
            update();
        });
    }
    if (!m_clickAnimTimer->isActive()) {
        m_clickAnimTimer->start();
    }
}

void CaptureOverlay::stopClickAnimTimer()
{
    if (m_clickAnimTimer && m_clickAnimTimer->isActive()) {
        m_clickAnimTimer->stop();
    }
    m_clickAnimPhase = 0.0;
}

void CaptureOverlay::drawKeystrokePreview(QPainter& p, double sx, double sy, double selW, double selH)
{
    p.save();
    p.setRenderHint(QPainter::Antialiasing);

    const double baseW = 120.0;
    const double baseH = 60.0;
    const double scale = 0.6 + m_keySize;
    double previewW = baseW * scale;
    double previewH = baseH * scale;
    const double margin = 16.0 * scale;

    // Gather display strings: live key presses or static demo
    const qint64 KEY_LIFETIME_MS = 2000;
    const qint64 now = QDateTime::currentMSecsSinceEpoch();

    // Remove expired keys
    m_keyPreviews.erase(
        std::remove_if(m_keyPreviews.begin(), m_keyPreviews.end(),
            [&](const KeyPreview& kp) { return (now - kp.birthMs) >= KEY_LIFETIME_MS; }),
        m_keyPreviews.end());

    QStringList displayKeys;
    QList<double> keyAlphas; // per-key fade alpha
    if (!m_keyPreviews.isEmpty()) {
        // Show live key presses (most recent up to 5)
        int start = std::max(0, m_keyPreviews.size() - 5);
        for (int i = start; i < m_keyPreviews.size(); ++i) {
            displayKeys.append(m_keyPreviews[i].text);
            double age = (double)(now - m_keyPreviews[i].birthMs) / KEY_LIFETIME_MS;
            keyAlphas.append(1.0 - age); // fade out
        }
    } else {
        // Static demo: ⌘ ⇧ F
        displayKeys = QStringList() << "\u2318" << "\u21E7" << "F";
        keyAlphas = {1.0, 1.0, 1.0};
    }

    // Widen box for more keys
    if (displayKeys.size() > 3) {
        previewW = std::max(previewW, (displayKeys.size() * 30.0 + 40.0) * scale);
    }

    // Position
    double kx, ky;
    switch (m_keyPosition) {
        case 0: kx = sx + (selW - previewW) / 2.0; ky = sy + selH - previewH - margin; break;
        case 1: kx = sx + margin; ky = sy + selH - previewH - margin; break;
        case 2: kx = sx + selW - previewW - margin; ky = sy + selH - previewH - margin; break;
        case 3: kx = sx + (selW - previewW) / 2.0; ky = sy + margin; break;
        case 4: kx = sx + margin; ky = sy + margin; break;
        case 5: kx = sx + selW - previewW - margin; ky = sy + margin; break;
        default: kx = sx + (selW - previewW) / 2.0; ky = sy + selH - previewH - margin; break;
    }

    QRectF boxRect(kx, ky, previewW, previewH);

    // Appearance colors
    QColor bgColor = (m_keyAppearance == 0) ? QColor(20, 20, 24, 230) : QColor(245, 245, 250, 230);
    QColor iconColor = (m_keyAppearance == 0) ? Qt::white : Qt::black;

    // Blur background — draw the blurred image clipped to the preview rect
    // so the area behind the box shows through as a frosted glass effect
    if (m_keyBlurBg && !m_blurredBg.isNull()) {
        p.save();
        QPainterPath clip;
        clip.addRoundedRect(boxRect, 12 * scale, 12 * scale);
        p.setClipPath(clip);

        // Scale the 1/4-res blur image back up to full screen
        double bgScaleX = (double)width() / m_blurredBg.width();
        double bgScaleY = (double)height() / m_blurredBg.height();
        p.scale(bgScaleX, bgScaleY);
        p.drawImage(QPointF(0, 0), m_blurredBg);
        p.restore();

        // Semi-transparent tint so text is readable
        bgColor = (m_keyAppearance == 0)
            ? QColor(20, 20, 24, 120)   // much more transparent with blur
            : QColor(245, 245, 250, 120);
    }

    p.setPen(QPen(QColor(iconColor.red(), iconColor.green(), iconColor.blue(), 50), 1.2));
    p.setBrush(bgColor);
    p.drawRoundedRect(boxRect, 12 * scale, 12 * scale);

    // Draw keys
    const double iconSize = 22.0 * scale;
    const double spacing = 10.0 * scale;
    const double totalGroupW = displayKeys.size() * iconSize + (displayKeys.size() - 1) * spacing;
    double startX = kx + (previewW - totalGroupW) / 2.0;
    double cy = ky + previewH / 2.0;

    QFont keyFont;
    keyFont.setFamily("Sans");
    keyFont.setBold(true);
    keyFont.setPointSizeF(16 * scale);

    for (int i = 0; i < displayKeys.size(); ++i) {
        double cx = startX + i * (iconSize + spacing) + iconSize / 2.0;
        double alpha = keyAlphas.value(i, 1.0);

        QColor c = iconColor;
        c.setAlphaF(alpha);

        p.setPen(QPen(c, 2.0 * scale, Qt::SolidLine, Qt::RoundCap));
        p.setBrush(Qt::NoBrush);

        const QString& key = displayKeys[i];
        if (key == "\u2318") {
            // Command key ⌘
            double r = 2.8 * scale;
            p.drawEllipse(QPointF(cx - r, cy - r), r, r);
            p.drawEllipse(QPointF(cx + r, cy - r), r, r);
            p.drawEllipse(QPointF(cx - r, cy + r), r, r);
            p.drawEllipse(QPointF(cx + r, cy + r), r, r);
            p.drawLine(QPointF(cx - r, cy - r + 0.5), QPointF(cx - r, cy + r - 0.5));
            p.drawLine(QPointF(cx + r, cy - r + 0.5), QPointF(cx + r, cy + r - 0.5));
            p.drawLine(QPointF(cx - r + 0.5, cy - r), QPointF(cx + r - 0.5, cy - r));
            p.drawLine(QPointF(cx - r + 0.5, cy + r), QPointF(cx + r - 0.5, cy + r));
        } else if (key == "\u21E7") {
            // Shift key ⇧
            QPainterPath shift;
            shift.moveTo(cx, cy - 8 * scale);
            shift.lineTo(cx - 7 * scale, cy + 1 * scale);
            shift.lineTo(cx - 3.5 * scale, cy + 1 * scale);
            shift.lineTo(cx - 3.5 * scale, cy + 8 * scale);
            shift.lineTo(cx + 3.5 * scale, cy + 8 * scale);
            shift.lineTo(cx + 3.5 * scale, cy + 1 * scale);
            shift.lineTo(cx + 7 * scale, cy + 1 * scale);
            shift.closeSubpath();
            p.drawPath(shift);
        } else if (key == "\u232B") {
            // Backspace ⌫
            double s = 6 * scale;
            QPainterPath bp;
            bp.moveTo(cx + s, cy - s);
            bp.lineTo(cx - s, cy - s);
            bp.lineTo(cx - s * 1.5, cy);
            bp.lineTo(cx - s, cy + s);
            bp.lineTo(cx + s, cy + s);
            bp.lineTo(cx + s * 0.5, cy);
            bp.closeSubpath();
            p.drawPath(bp);
            p.drawLine(QPointF(cx - s * 0.2, cy - s * 0.4), QPointF(cx + s * 0.2, cy + s * 0.4));
            p.drawLine(QPointF(cx + s * 0.2, cy - s * 0.4), QPointF(cx - s * 0.2, cy + s * 0.4));
        } else if (key == "\u21A9") {
            // Enter ↩
            double s = 6 * scale;
            QPainterPath ep;
            ep.moveTo(cx - s, cy - s);
            ep.lineTo(cx + s, cy - s);
            ep.lineTo(cx + s, cy + s * 0.3);
            ep.lineTo(cx + s * 0.3, cy + s * 0.3);
            ep.lineTo(cx + s * 0.3, cy + s);
            ep.lineTo(cx - s * 1.3, cy + s * 0.3);
            ep.lineTo(cx - s * 1.3, cy - s * 0.3);
            ep.lineTo(cx - s, cy - s * 0.3);
            ep.closeSubpath();
            p.drawPath(ep);
        } else if (key == " ") {
            // Space bar
            double w = iconSize * 0.6;
            double h = 3 * scale;
            p.drawRoundedRect(QRectF(cx - w/2, cy - h, w, h * 2), h, h);
        } else {
            // Regular key — draw text
            p.setFont(keyFont);
            p.setPen(c);
            p.drawText(QRectF(cx - iconSize/2.0, ky, iconSize, previewH), Qt::AlignCenter, key);
        }
    }

    p.restore();
}
