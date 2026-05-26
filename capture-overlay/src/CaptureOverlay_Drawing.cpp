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
#include <QProcess>
#include <QRadialGradient>
#include <QPen>
#include <QDateTime>
#include <QCursor>
#include <QMutexLocker>
#include <QTimer>
#include <QRegion>
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
    const double actionBarW = 0.0;
    const double actionBarH = 0.0;
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
    layout.rightActionsPanel = QRectF();
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
    layout.confirmCard = QRectF();
    layout.cancelCard = QRectF();
    return layout;
}

RecordingDeckLayout computeRecordingDeckLayout(double selX, double selY,
                                               double selW, double selH,
                                               double screenW, double screenH)
{
    RecordingDeckLayout layout;
    const double railH = TOOL_CARD_H * 3.0;
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

    // Subtle white sheen (0.04 alpha) for a polished feel
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

QRect CaptureOverlay::crosshairBubbleRectForPoint(const QPoint& point) const
{
    const QRect widgetRect = rect();
    const QPoint guidePoint = widgetRect.contains(point) ? point : m_pointerPos;

    QString labelText;
    if (m_dragging || m_hasSelection) {
        const QRect sel = m_selection.normalized();
        labelText = QStringLiteral("%1 \u00D7 %2").arg(sel.width()).arg(sel.height());
    } else {
        labelText = QStringLiteral("%1, %2").arg(guidePoint.x()).arg(guidePoint.y());
    }

    QFont font(QStringLiteral("Sans"));
    font.setPixelSize(12);
    font.setWeight(QFont::Medium);
    const QFontMetrics fm(font);
    const QRect textRect = fm.boundingRect(labelText);

    const int paddingX = 10;
    const int paddingY = 6;
    const int bw = textRect.width() + paddingX * 2;
    const int bh = textRect.height() + paddingY * 2;

    const QRect bubbleRect(
        std::clamp(guidePoint.x() + 16, 8, widgetRect.width() - bw - 8),
        std::clamp(guidePoint.y() + 16, 8, widgetRect.height() - bh - 8),
        bw,
        bh);

    return bubbleRect.adjusted(-3, -3, 3, 3);
}

QRegion CaptureOverlay::crosshairDirtyRegion(const QPoint& oldPoint,
                                             const QPoint& newPoint,
                                             const QRect& oldSelection,
                                             const QRect& newSelection,
                                             bool hadSelection,
                                             bool hasSelection) const
{
    const QRect widgetRect = rect();
    QRegion dirty;
    constexpr int guidePad = 3;
    constexpr int selectionPad = 4;

    auto addGuideRegions = [&](const QPoint& point) {
        if (!widgetRect.contains(point)) {
            return;
        }
        dirty += QRect(0,
                       std::max(0, point.y() - guidePad),
                       widgetRect.width(),
                       std::min(widgetRect.height(), guidePad * 2 + 1));
        dirty += QRect(std::max(0, point.x() - guidePad),
                       0,
                       std::min(widgetRect.width(), guidePad * 2 + 1),
                       widgetRect.height());
    };

    addGuideRegions(oldPoint);
    addGuideRegions(newPoint);

    if (!m_lastCrosshairBubbleRect.isNull()) {
        dirty += m_lastCrosshairBubbleRect;
    }
    dirty += crosshairBubbleRectForPoint(newPoint);

    if (hadSelection && !oldSelection.isNull()) {
        dirty += oldSelection.adjusted(-selectionPad, -selectionPad, selectionPad, selectionPad);
    }
    if (hasSelection && !newSelection.isNull()) {
        dirty += newSelection.adjusted(-selectionPad, -selectionPad, selectionPad, selectionPad);
    }

    return dirty.intersected(widgetRect);
}

QRegion CaptureOverlay::windowHoverDirtyRegion(int index) const
{
    if (index < 0 || index >= m_windows.size()) {
        return QRegion();
    }

    const QRect widgetRect = rect();
    const WindowInfo& win = m_windows[index];
    if (!widgetRect.intersects(win.rect)) {
        return QRegion();
    }

    QRegion dirty(win.rect.adjusted(-4, -4, 4, 4));

    QFont font;
    font.setPointSizeF(11.5);
    font.setBold(true);
    QFontMetricsF metrics(font);

    QString label = win.title.length() > 48 ? win.title.left(45) + QStringLiteral("…")
                                            : win.title;
    const double textWidth = metrics.horizontalAdvance(label);
    const double pillWidth = textWidth + 28.0;
    const double pillHeight = 32.0;
    double pillX = win.rect.x() + (win.rect.width() - pillWidth) / 2.0;
    double pillY = win.rect.y() - pillHeight - 8.0;
    if (pillY < 8.0) {
        pillY = win.rect.y() + 8.0;
    }
    pillX = std::max(8.0, std::min(pillX, rect().width() - pillWidth - 8.0));

    dirty += QRectF(pillX, pillY, pillWidth, pillHeight).toAlignedRect().adjusted(-4, -4, 4, 4);
    return dirty.intersected(widgetRect);
}

void CaptureOverlay::paintEvent(QPaintEvent* event)
{
    QPainter p(this);
    if (event) {
        p.setClipRegion(event->region());
    }
    p.setRenderHint(QPainter::TextAntialiasing);

    const QRect widgetRect = rect();
    const double sw = widgetRect.width();
    const double sh = widgetRect.height();

    if (isCrosshairMode()) {
        if (!m_background.isNull()) {
            p.drawPixmap(widgetRect, m_background);
        }

        const QPoint guidePoint = widgetRect.contains(m_pointerPos)
            ? m_pointerPos
            : m_lastCrosshairPaintPoint;

        p.save();
        // Brand orange for crosshair lines (more visible on white backgrounds)
        p.setPen(QPen(QColor(255, 102, 0, 200), 1.0));
        p.drawLine(QPoint(0, guidePoint.y()), QPoint(widgetRect.width(), guidePoint.y()));
        p.drawLine(QPoint(guidePoint.x(), 0), QPoint(guidePoint.x(), widgetRect.height()));
        p.restore();

        if (m_dragging || m_hasSelection) {
            const QRect sel = m_selection.normalized();
            p.save();
            // Distinct orange border for the selection area
            p.setPen(QPen(QColor(255, 102, 0, 240), 2.0));
            // Subtle orange fill so it's visible on white backgrounds during capture
            p.setBrush(QColor(255, 102, 0, 40));
            p.drawRect(sel.adjusted(0, 0, -1, -1));
            p.restore();
        }

        // ── Clean, native-looking size/position bubble ──────────────────────────────
        QString labelText;
        if (m_dragging || m_hasSelection) {
            const QRect sel = m_selection.normalized();
            // Use proper multiplication sign
            labelText = QStringLiteral("%1 \u00D7 %2").arg(sel.width()).arg(sel.height());
        } else {
            labelText = QStringLiteral("%1, %2").arg(guidePoint.x()).arg(guidePoint.y());
        }

        static const QFont crosshairBubbleFont = []() {
            QFont font(QStringLiteral("Sans"));
            font.setPixelSize(12);
            font.setWeight(QFont::Medium);
            return font;
        }();
        p.setFont(crosshairBubbleFont);

        const QRect bubbleRect = crosshairBubbleRectForPoint(guidePoint).adjusted(3, 3, -3, -3);

        p.save();
        p.setRenderHint(QPainter::Antialiasing);

        // Standard semi-transparent dark background
        QPainterPath bubble;
        bubble.addRoundedRect(bubbleRect, 6, 6);
        p.fillPath(bubble, QColor(0, 0, 0, 180));

        // Very subtle white border to define the edge against dark backgrounds
        p.setPen(QPen(QColor(255, 255, 255, 40), 1.0));
        p.setBrush(Qt::NoBrush);
        p.drawPath(bubble);

        // Crisp white text
        p.setPen(QColor(255, 255, 255));
        p.drawText(bubbleRect, Qt::AlignCenter, labelText);

        p.restore();
        return;
    }

    p.setRenderHint(QPainter::Antialiasing);

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
        const QRegion dirtyRegion = p.clipRegion();
        for (int i = 0; i < m_windows.size(); ++i) {
            const WindowInfo& win = m_windows[i];
            if (!widgetRect.intersects(win.rect)) continue;
            bool hovered = (i == m_hoveredWindow);
            if (!hovered && !dirtyRegion.intersects(win.rect.adjusted(-4, -4, 4, 4))) continue;
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

    // Add a subtle orange tint during active drag/resize for better feedback
    if (m_dragging || m_moving || m_resizing != HandlePos::None) {
        p.fillRect(sel, QColor(255, 102, 0, 30));
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
            // Use orange for handles to match brand and ensure visibility on white
            p.setPen(QPen(QColor(255, 102, 0, 245), HANDLE_MARKER_THICKNESS,
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

    // ── Visible countdown overlay ───────────────────────────────────────────
    if (m_countdownActive && m_countdownValue > 0) {
        p.save();
        p.setRenderHint(QPainter::Antialiasing);

        if (!m_countdownForRecording) {
            // Capture-delay countdown: pill badge at top-center
            const double pillW = 112.0;
            const double pillH = 44.0;
            const double pillX = (sw - pillW) / 2.0;
            const double pillY = 28.0;
            const QRectF pillRect(pillX, pillY, pillW, pillH);
            m_countdownBubbleRect = pillRect;

            p.setPen(Qt::NoPen);
            p.setBrush(m_hoveredCountdownCancel
                           ? QColor(200, 60, 40, 242)
                           : QColor(233, 84, 32, 235)); // #E95420 with alpha
            p.drawRoundedRect(pillRect, pillH / 2.0, pillH / 2.0);

            // Draw timer icon (clock face) on the left side of the pill
            const double iconCx = pillX + 22.0;
            const double iconCy = pillY + pillH / 2.0;
            const double iconR = 11.0;
            p.setPen(QPen(Qt::white, 2.2, Qt::SolidLine, Qt::RoundCap));
            p.setBrush(Qt::NoBrush);
            p.drawEllipse(QPointF(iconCx, iconCy), iconR, iconR);
            // Clock hands — small hour hand pointing up-ish
            p.drawLine(QPointF(iconCx, iconCy), QPointF(iconCx, iconCy - 5.5));
            // Minute hand pointing right-ish
            p.drawLine(QPointF(iconCx, iconCy), QPointF(iconCx + 5.0, iconCy + 2.0));

            // Draw countdown number on the right side
            QFont countdownFont(QStringLiteral("Sans"));
            countdownFont.setBold(true);
            countdownFont.setPointSizeF(m_hoveredCountdownCancel ? 13.0 : 22.0);
            p.setFont(countdownFont);
            p.setPen(Qt::white);
            p.setBrush(Qt::NoBrush);

            const QRectF textRect(pillX + 40.0, pillY, pillW - 44.0, pillH);
            p.drawText(textRect,
                       Qt::AlignCenter,
                       m_hoveredCountdownCancel ? QStringLiteral("Cancel")
                                                : QString::number(m_countdownValue));
        } else {
            // Recording countdown: centered circle (3-2-1)
            const double bubbleSize = 184.0;
            const double bubbleX = (sw - bubbleSize) / 2.0;
            const double bubbleY = (sh - bubbleSize) / 2.0;
            const QRectF bubbleRect(bubbleX, bubbleY, bubbleSize, bubbleSize);
            m_countdownBubbleRect = bubbleRect;

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
        }

        p.restore();
    } else {
        m_countdownBubbleRect = QRectF();
    }

    // ── Volume popup (Mic / Speaker) ─────────────────────────────────────
    if (m_recordingPanelOpen) {
        const auto sel = m_selection.normalized();
        const double sx = sel.x(), sy = sel.y(), selW = sel.width(), selH = sel.height();
        // Position top-centre of selection like the settings menu (matches Rust overlay)
        const double popupX = qBound(10.0, (sx + (selW - 280.0) / 2.0), width() - 290.0);
        const double popupY = qBound(10.0, sy + 24.0, height() - 140.0);

        if (m_micVolumePopupOpen) {
            drawVolumePopup(p, popupX, popupY, "Microphone", m_micVolume, true);
        } else {
            m_volumePopupRect = QRectF();
        }
        if (m_speakerVolumePopupOpen) {
            drawVolumePopup(p, popupX, popupY, "Speaker", m_speakerVolume, true);
        } else if (!m_micVolumePopupOpen) {
            m_volumePopupRect = QRectF();
        }
    }

    // ── Scroll capture popup ──────────────────────────────────────────────
    if (m_scrollPopupOpen) {
        // Center on selection area when available, otherwise on screen
        // (matches Rust overlay behavior)
        double cx, cy;
        if (m_hasSelection) {
            const auto sel = m_selection.normalized();
            cx = sel.x() + sel.width() / 2.0;
            cy = sel.y() + sel.height() / 2.0;
        } else {
            cx = width() / 2.0;
            cy = height() / 2.0;
        }
        drawScrollPopup(p, cx, cy);
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
    const QRectF videoRect(bottomX, bottomY, ACTION_RAIL_W, ACTION_CARD_H);
    const QRectF gifRect(videoRect.right() + ACTION_CARD_GAP, bottomY, ACTION_RAIL_W, ACTION_CARD_H);
    m_recTileRects.append(videoRect);
    m_recTileRects.append(gifRect);
    drawPrimaryAction(videoRect, RecordPanelTile::RecordVideo, 16, QStringLiteral("Video"), m_recordType == RecordType::Video);
    drawPrimaryAction(gifRect, RecordPanelTile::RecordGif, 17, QStringLiteral("GIF"), m_recordType == RecordType::Gif);

    const double contextualX = std::max(10.0, std::min(selX + (selW - 440.0) / 2.0, screenW - 450.0));
    const double contextualY = std::max(10.0, std::min(selY + 24.0, screenH - 570.0));
    const QRectF contextualRect(contextualX, contextualY, 440.0, 560.0);

    if (m_settingsOpen) {
        drawSettingsMenu(p, contextualRect.x(), contextualRect.y());
    } else {
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
    const double menuH = 560.0;
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

        auto drawSetting = [&](const QString& label, const QString& desc, bool checked, bool* target,
                               bool disabled = false, const QString& badge = QString()) {
            QRectF labelRect(labelX, currY, 110, rowH);
            p.setFont(QFont("Sans", 10, QFont::Bold));
            p.setPen(QColor(255, 255, 255, disabled ? 110 : 200));
            p.drawText(labelRect, Qt::AlignRight | Qt::AlignVCenter, label);

            QRectF checkArea(valueX, currY, menuW - (valueX - menuX) - 20, rowH);
            int itemIdx = m_settingsClickableRects.size();
            // Disabled rows still need a placeholder rect so the index stays
            // aligned with the click handler's switch on `itemIdx`. We use a
            // collapsed (empty) rect so it can never be hit.
            m_settingsClickableRects.append(disabled ? QRectF() : checkArea);

            bool hovered = !disabled && (m_hoveredSettingsItem == itemIdx);
            if (hovered) {
                p.setPen(Qt::NoPen);
                p.setBrush(QColor(255, 255, 255, 12));
                p.drawRoundedRect(checkArea.adjusted(-5, 0, 5, 0), 6, 6);
            }

            // Checkbox
            QRectF cb(valueX, currY + (rowH - 18) / 2.0, 18, 18);
            p.setRenderHint(QPainter::Antialiasing);
            if (checked && !disabled) {
                p.setPen(Qt::NoPen);
                p.setBrush(accentColor);
                p.drawRoundedRect(cb, 4, 4);
                p.setPen(QPen(Qt::white, 2));
                p.drawLine(QPointF(cb.x() + 4, cb.y() + 9), QPointF(cb.x() + 8, cb.y() + 13));
                p.drawLine(QPointF(cb.x() + 8, cb.y() + 13), QPointF(cb.x() + 14, cb.y() + 5));
            } else {
                p.setPen(QPen(QColor(255, 255, 255, disabled ? 35 : 60), 1.5));
                p.setBrush(QColor(0, 0, 0, disabled ? 25 : 40));
                p.drawRoundedRect(cb, 4, 4);
            }

            p.setFont(QFont("Sans", 10, QFont::Normal));
            p.setPen(disabled ? QColor(255, 255, 255, 110) : QColor(Qt::white));
            p.drawText(QRectF(valueX + 28, currY, checkArea.width() - 28, rowH), Qt::AlignLeft | Qt::AlignVCenter, desc);

            if (disabled && !badge.isEmpty()) {
                QFont badgeFont; badgeFont.setFamily("Sans"); badgeFont.setPointSizeF(7.0); badgeFont.setBold(true);
                p.setFont(badgeFont);
                QFontMetricsF badgeFm(badgeFont);
                const double badgeTextW = badgeFm.horizontalAdvance(badge);
                const double descW = QFontMetricsF(QFont("Sans", 10, QFont::Normal)).horizontalAdvance(desc);
                const double badgeX = valueX + 28 + descW + 10.0;
                QRectF badgeRect(badgeX, currY + (rowH - 14) / 2.0, badgeTextW + 12.0, 14.0);
                p.setPen(Qt::NoPen);
                p.setBrush(QColor(255, 255, 255, 28));
                p.drawRoundedRect(badgeRect, 5.0, 5.0);
                p.setPen(QColor(255, 232, 214, 220));
                p.drawText(badgeRect, Qt::AlignCenter, badge);
            }

            currY += rowH;
        };

        drawSetting("Controls:", "Use keyboard shortcuts to control recordings (elapsed time appears in the top bar)", m_recControls, &m_recControls);
        drawSetting("Menu bar:", "Display recording time in the top bar", m_displayRecTime, &m_displayRecTime);
        drawSetting("HiDPI:", "Record at display scale resolution", m_hidpi, &m_hidpi);
        drawSetting("Notifications:", "\"Do Not Disturb\" while recording", m_doNotDisturb, &m_doNotDisturb);
        
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

        // 4. Video Editor
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

    // Confirm/cancel action cards removed — use Enter/Space to confirm, Esc to cancel

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

// ── Volume Popup (Mic / Speaker) ──────────────────────────────────────────
// Mirrors draw_volume_popup from src/overlay/drawing.rs

void CaptureOverlay::runPactlVolume(const QString& type, int pct)
{
    QStringList args;
    if (type == "mic") {
        args << "set-source-volume" << "@DEFAULT_SOURCE@" << QString("%1%").arg(pct);
    } else {
        args << "set-sink-volume" << "@DEFAULT_SINK@" << QString("%1%").arg(pct);
    }
    QProcess::startDetached("pactl", args);
}

void CaptureOverlay::drawVolumePopup(QPainter& p,
                                      double panelX, double panelY,
                                      const QString& title,
                                      double volume,
                                      bool isOpen)
{
    if (!isOpen) return;

    const double menuW = 280.0;
    const double menuH = 130.0;
    const double scrW = width();
    const double scrH = height();
    // panelX/panelY are pre-computed top-centre-of-selection positions,
    // already clamped to screen bounds by the caller. Apply the same
    // bounds clamping as a safety net.
    const double menuX = qBound(10.0, panelX, scrW - menuW - 10.0);
    const double menuY = qBound(10.0, panelY, scrH - menuH - 10.0);

    const QColor accentColor(176, 92, 56);

    // Warm radial glow
    {
        QRadialGradient glow(menuX + menuW / 2.0, menuY + menuH / 2.0, menuW);
        glow.setColorAt(0, QColor(accentColor.red(), accentColor.green(), accentColor.blue(), 40));
        glow.setColorAt(0.6, QColor(0, 0, 0, 0));
        p.fillRect(QRectF(menuX - 40, menuY - 40, menuW + 80, menuH + 80), glow);
    }

    drawFrostedPanel(p, menuX, menuY, menuW, menuH, 12.0, m_blurredBg.isNull() ? nullptr : &m_blurredBg, scrW, scrH);

    // Header: "RECORDING DEVICE" label
    QFont headerFont("Sans", 10, QFont::Bold);
    p.setFont(headerFont);
    p.setPen(QColor(255, 224, 196, 176));
    p.drawText(QRectF(menuX + 18, menuY + 20, menuW - 36, 18), Qt::AlignLeft | Qt::AlignVCenter, "RECORDING DEVICE");

    // Title (Mic / Speaker)
    QFont titleFont("Sans", 18, QFont::Bold);
    p.setFont(titleFont);
    p.setPen(QColor(245, 245, 246, 255));
    p.drawText(QRectF(menuX + 18, menuY + 42, menuW - 36, 22), Qt::AlignLeft | Qt::AlignVCenter, title);

    // Slider row
    const double rowY = menuY + 78.0;
    const double rowH = 46.0;
    const double sliderX = menuX + 83.0;
    const double sliderW = 140.0;
    const double sliderTrackH = 6.0;
    const double trackY = rowY + (rowH - sliderTrackH) / 2.0;

    // "Volume:" label
    QFont labelFont("Sans", 13, QFont::Bold);
    p.setFont(labelFont);
    p.setPen(QColor(255, 255, 255, 210));
    p.drawText(QRectF(menuX + 18, rowY, 65, rowH), Qt::AlignLeft | Qt::AlignVCenter, "Volume:");

    // Percentage badge
    const int pct = qBound(0, qRound(volume * 100.0), 100);
    QFont pctFont("Sans", 11, QFont::Bold);
    p.setFont(pctFont);
    p.setPen(QColor(255, 232, 214, 220));
    p.drawText(QRectF(menuX + menuW - 55, rowY, 43, rowH), Qt::AlignRight | Qt::AlignVCenter, QString("%1%").arg(pct));

    // Track background
    p.setPen(Qt::NoPen);
    p.setBrush(QColor(255, 255, 255, m_volumeSliderDragging ? 36 : 28));
    p.drawRoundedRect(QRectF(sliderX, trackY, sliderW, sliderTrackH), 3, 3);

    // Filled portion
    const double filledW = qBound(0.0, volume, 1.0) * sliderW;
    if (filledW > 1.0) {
        QLinearGradient fillGrad(sliderX, 0, sliderX + sliderW, 0);
        fillGrad.setColorAt(0.0, QColor(204, 122, 80, 235));
        fillGrad.setColorAt(1.0, QColor(255, 178, 122, 235));
        p.setBrush(fillGrad);
        p.drawRoundedRect(QRectF(sliderX, trackY, filledW, sliderTrackH), 3, 3);
    }

    // Slider handle
    const double handleW = m_volumeSliderDragging ? 18.0 : 14.0;
    const double handleH = 26.0;
    const double handleX = sliderX + filledW - handleW / 2.0;
    const double handleY = trackY + sliderTrackH / 2.0 - handleH / 2.0;

    // Handle shadow
    p.setBrush(QColor(0, 0, 0, 90));
    p.drawRoundedRect(QRectF(handleX + 0.6, handleY + 1.4, handleW, handleH), 6, 6);

    // Handle body gradient (white to light gray)
    QLinearGradient handleGrad(0, handleY, 0, handleY + handleH);
    handleGrad.setColorAt(0.0, QColor(255, 255, 255, 255));
    handleGrad.setColorAt(1.0, QColor(225, 225, 230, 255));
    p.setBrush(handleGrad);
    p.drawRoundedRect(QRectF(handleX, handleY, handleW, handleH), 6, 6);

    // Cache layout rects for hit testing
    m_volumePopupRect = QRectF(menuX, menuY, menuW, menuH);
    m_volumeSliderRect = QRectF(sliderX, trackY - 12, sliderW, sliderTrackH + 24);
    m_volumeHandleRect = QRectF(handleX - 4, handleY - 4, handleW + 8, handleH + 8);
}

// ── Scroll Capture Popup ──────────────────────────────────────────────────
// Mirrors draw_scroll_popup from src/overlay/drawing.rs

void CaptureOverlay::drawScrollPopup(QPainter& p, double centerX, double centerY)
{
    if (!m_scrollPopupOpen) return;

    const double popupW = 360.0;
    const double popupH = 170.0;
    const double scrW = width();
    const double scrH = height();
    const double popupX = qBound(10.0, centerX - popupW / 2.0, scrW - popupW - 10.0);
    const double popupY = qBound(10.0, centerY - popupH / 2.0, scrH - popupH - 10.0);

    const QColor accentColor(176, 92, 56);

    // Warm radial glow
    {
        QRadialGradient glow(popupX + popupW / 2.0, popupY + popupH / 2.0, popupW / 2.0);
        glow.setColorAt(0, QColor(accentColor.red(), accentColor.green(), accentColor.blue(), 40));
        glow.setColorAt(0.6, QColor(0, 0, 0, 0));
        p.fillRect(QRectF(popupX - 40, popupY - 40, popupW + 80, popupH + 80), glow);
    }

    drawFrostedPanel(p, popupX, popupY, popupW, popupH, 12.0, m_blurredBg.isNull() ? nullptr : &m_blurredBg, scrW, scrH);

    // Close button
    const double closeSize = 22.0;
    const double closeX = popupX + popupW - closeSize - 10.0;
    const double closeY = popupY + 10.0;
    if (m_hoveredScrollClose) {
        p.setPen(Qt::NoPen);
        p.setBrush(QColor(204, 64, 38, 255));
    } else {
        p.setPen(Qt::NoPen);
        p.setBrush(QColor(60, 60, 60, 255));
    }
    p.drawRoundedRect(QRectF(closeX, closeY, closeSize, closeSize), 5, 5);

    // X mark
    p.setPen(QPen(QColor(255, 255, 255, 255), 1.5, Qt::SolidLine, Qt::RoundCap));
    p.drawLine(QPointF(closeX + 6, closeY + 6),
               QPointF(closeX + closeSize - 6, closeY + closeSize - 6));
    p.drawLine(QPointF(closeX + closeSize - 6, closeY + 6),
               QPointF(closeX + 6, closeY + closeSize - 6));

    // Title
    QFont titleFont("Sans", 13, QFont::Bold);
    p.setFont(titleFont);
    p.setPen(QColor(255, 255, 255, 255));
    p.drawText(QRectF(popupX + 20, popupY + 24, popupW - 60, 20), Qt::AlignLeft | Qt::AlignVCenter, "Scroll Capture");

    // Body text
    QFont bodyFont("Sans", 12);
    p.setFont(bodyFont);
    p.setPen(QColor(255, 255, 255, 180));
    p.drawText(QRectF(popupX + 20, popupY + 55, popupW - 40, 18), Qt::AlignLeft | Qt::AlignVCenter, "Scroll capture requires the ApexShot");
    p.drawText(QRectF(popupX + 20, popupY + 73, popupW - 40, 18), Qt::AlignLeft | Qt::AlignVCenter, "browser extension.");

    // CTA button — orange gradient, matching Rust overlay
    const double btnW = 182.0;
    const double btnH = 34.0;
    const double btnX = popupX + (popupW - btnW) / 2.0;
    const double btnY = popupY + 102.0;

    // Button shadow
    p.setPen(Qt::NoPen);
    p.setBrush(QColor(0, 0, 0, 56));
    p.drawRoundedRect(QRectF(btnX, btnY + 1.5, btnW, btnH), 10, 10);

    // Button gradient
    QLinearGradient btnGrad(0, btnY, 0, btnY + btnH);
    btnGrad.setColorAt(0.0, QColor(242, 116, 70, 245));
    btnGrad.setColorAt(1.0, QColor(176, 92, 56, 240));
    p.setBrush(btnGrad);
    p.drawRoundedRect(QRectF(btnX, btnY, btnW, btnH), 10, 10);

    // Button border
    p.setPen(QPen(QColor(255, 224, 196, 87), 1.0));
    p.setBrush(Qt::NoBrush);
    p.drawRoundedRect(QRectF(btnX, btnY, btnW, btnH), 10, 10);

    // Button text
    QFont btnFont("Sans", 13, QFont::Bold);
    p.setFont(btnFont);
    p.setPen(QColor(255, 255, 255, 255));
    p.drawText(QRectF(btnX, btnY, btnW, btnH), Qt::AlignCenter, "Download Extension");

    // Cache layout rects for hit testing
    m_scrollPopupRect = QRectF(popupX, popupY, popupW, popupH);
    m_scrollCloseRect = QRectF(closeX, closeY, closeSize, closeSize);
    m_scrollDownloadBtnRect = QRectF(btnX, btnY, btnW, btnH);
}

