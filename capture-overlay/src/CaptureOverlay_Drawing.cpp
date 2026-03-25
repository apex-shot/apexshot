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
    "Capture","Area","Fullscreen","Window","Scroll","Timer","OCR","Recording"
};

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
    case 11: { // Mic
        QPainterPath body;
        roundedRectPath(body, cx - 2.2, cy - 6.5, 4.4, 8.5, 2.2);
        p.drawPath(body);
        p.drawArc(QRectF(cx - 5.0, cy - 1.5, 10.0, 9.0), 0, -180 * 16);
        p.drawLine(QPointF(cx, cy + 3.0), QPointF(cx, cy + 6.5));
        break;
    }
    case 12: { // Window/Screen selection
        QPainterPath path;
        roundedRectPath(path, cx - 7.0, cy - 5.0, 14.0, 10.0, 2.0);
        p.drawPath(path);
        // Small person/window inside
        p.drawEllipse(QPointF(cx - 2.5, cy - 1.0), 2.0, 2.0);
        p.drawArc(QRectF(cx - 5.5, cy + 1.0, 6.0, 6.0), 0, 180 * 16);
        // Sound/Window waves
        p.drawArc(QRectF(cx + 1.0, cy - 3.0, 4.0, 6.0), -45 * 16, 90 * 16);
        p.drawArc(QRectF(cx + 3.5, cy - 2.0, 2.5, 4.0), -45 * 16, 90 * 16);
        break;
    }
    case 13: { // Video Camera Icon
        QPainterPath body;
        roundedRectPath(body, cx - 7.5, cy - 4.5, 10.0, 9.0, 2.0);
        p.drawPath(body);
        QPainterPath lens;
        lens.moveTo(cx + 2.5, cy - 3.0);
        lens.lineTo(cx + 7.5, cy - 5.5);
        lens.lineTo(cx + 7.5, cy + 5.5);
        lens.lineTo(cx + 2.5, cy + 3.0);
        lens.closeSubpath();
        p.drawPath(lens);
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
    case 16: { // Video Logo (Large)
        QPainterPath body;
        roundedRectPath(body, cx - 7.5, cy - 5.0, 10.0, 10.0, 2.5);
        p.fillPath(body, color);
        QPainterPath lens;
        lens.moveTo(cx + 2.5, cy - 3.0);
        lens.lineTo(cx + 7.5, cy - 5.5);
        lens.lineTo(cx + 7.5, cy + 5.5);
        lens.lineTo(cx + 2.5, cy + 3.0);
        lens.closeSubpath();
        p.fillPath(lens, color);
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
    } else {
        // Draw recording panel inside selection
        drawRecordingPanel(p, sx, sy, selW, selH);
    }

    // ── Webcam preview ──────────────────────────────────────────────────────
    if (m_recWebcam) {
        p.save();
        p.setRenderHint(QPainter::Antialiasing);

        // Size presets
        double previewW, previewH;
        switch (m_webcamSize) {
            case WebcamSize::Small:      previewW = 120; previewH = 160; break;
            case WebcamSize::Medium:     previewW = 200; previewH = 260; break;
            case WebcamSize::Large:      previewW = 280; previewH = 370; break;
            case WebcamSize::Huge:       previewW = 360; previewH = 480; break;
            case WebcamSize::Fullscreen: previewW = selW - 20; previewH = selH - 20; break;
        }

        // Shape adjustments
        switch (m_webcamShape) {
            case WebcamShape::Circle:
            case WebcamShape::Square:
                previewH = previewW;
                break;
            case WebcamShape::Rectangle:
                previewH = previewW * 0.75;
                break;
            case WebcamShape::Vertical:
                break;
        }

        const double margin = 10.0;
        double px = sx + margin;
        double py = sy + selH - previewH - margin;

        // Flip
        if (m_webcamFlip) {
            p.translate(px + previewW / 2.0, 0);
            p.scale(-1, 1);
            p.translate(-(px + previewW / 2.0), 0);
        }

        QRectF previewRect(px, py, previewW, previewH);

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

    // ── Visible countdown overlay (Top Center Pill) ──────────────────────────
    if (m_countdownActive && m_countdownValue > 0) {
        const double pillW = 108.0;
        const double pillH = 48.0;
        const double pillX = (sw - pillW) / 2.0;
        const double pillY = 40.0;
        const QRectF pillRect(pillX, pillY, pillW, pillH);

        p.save();
        p.setRenderHint(QPainter::Antialiasing);

        // Draw pill background
        p.setPen(Qt::NoPen);
        p.setBrush(QColor(253, 76, 70)); // Vibrant Red/Orange matching screenshot
        p.drawRoundedRect(pillRect, pillH / 2.0, pillH / 2.0);

        // Draw timer icon
        const double iconCX = pillX + 36.0;
        const double iconCY = pillY + pillH / 2.0;

        p.save();
        p.setPen(QPen(QColor(255, 255, 255, 180), 2.2, Qt::SolidLine, Qt::RoundCap));
        p.setBrush(Qt::NoBrush);
        p.drawArc(QRectF(iconCX - 8.5, iconCY - 8.5, 17, 17), 50 * 16, 260 * 16);
        p.drawLine(QPointF(iconCX, iconCY - 8.5), QPointF(iconCX, iconCY - 5.5));
        p.drawLine(QPointF(iconCX, iconCY), QPointF(iconCX + 4.5, iconCY - 4.5)); // diagonal hand
        p.restore();

        // Draw countdown number
        QFont countdownFont(QStringLiteral("Sans"));
        countdownFont.setBold(true);
        countdownFont.setPointSizeF(20);
        p.setFont(countdownFont);
        p.setPen(Qt::white);

        p.drawText(QRectF(pillX + 58, pillY, 40, pillH),
                   Qt::AlignLeft | Qt::AlignVCenter,
                   QString::number(m_countdownValue));

        p.restore();
    }
}

// ── Draw recording panel (two sections inside selection) ──────────────────────

void CaptureOverlay::drawRecordingPanel(QPainter& p,
                                          double selX, double selY,
                                          double selW, double selH)
{
    const double screenW = width();
    const double screenH = height();
    const QImage* blurPtr = m_blurredBg.isNull() ? nullptr : &m_blurredBg;

    // Brand Colors
    const QColor accentColor(122, 100, 255); // ApexShot Indigo
    const QColor secondaryAccent(255, 60, 160); // ApexShot Pinkish for some accents

    // Dimensions
    const double tileW = 60.0;
    const double tileH = 50.0;
    const double panelRadius = 10.0; // Matching editor's border-radius: 10px
    const double padding = 8.0;
    const double panelGap = 12.0;

    const double topPanelW = tileW * 5;
    const double topPanelH = tileH * 2;
    
    const double bottomPanelW = topPanelW;
    const double bottomRowH = 52.0;
    const double bottomPanelH = bottomRowH * 2;

    const double totalH = topPanelH + panelGap + bottomPanelH;

    // Center inside selection
    double panelX = selX + (selW - topPanelW) / 2.0;
    double startY = selY + (selH - totalH) / 2.0;
    const double margin = 20.0;
    panelX = std::max(selX + margin, std::min(panelX, selX + selW - topPanelW - margin));
    startY = std::max(selY + margin, std::min(startY, selY + selH - totalH - margin));

    double topY = startY;
    double bottomY = topY + topPanelH + panelGap;

    m_recPanelRect = QRectF(panelX, startY, topPanelW, totalH);
    m_recTileRects.clear();

    auto drawActiveIndicator = [&](QRectF cell, bool active) {
        if (!active) return;
        p.save();
        p.setRenderHint(QPainter::Antialiasing);
        double cx = cell.center().x();
        double cy = cell.bottom() - 8.0;

        // Draw a clean, modern, and smaller tick (checkmark)
        p.setPen(QPen(Qt::white, 1.5, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
        p.drawLine(QPointF(cx - 3.5, cy + 1), QPointF(cx - 0.5, cy + 4));
        p.drawLine(QPointF(cx - 0.5, cy + 4), QPointF(cx + 4.5, cy - 2.5));
        
        p.restore();
    };

    // ── Helper: draw brand outer glow ─────────────────────────────────────
    auto drawPanelGlow = [&](double x, double y, double w, double h, double r) {
        p.save();
        QRadialGradient glow(x + w/2.0, y + h/2.0, std::max(w, h));
        glow.setColorAt(0, QColor(accentColor.red(), accentColor.green(), accentColor.blue(), 25));
        glow.setColorAt(0.5, QColor(0, 0, 0, 0));
        p.fillRect(QRectF(x - 40, y - 40, w + 80, h + 80), glow);
        p.restore();
    };

    // ── Section 1: Top Panel ──────────────────────────────────────────────
    drawPanelGlow(panelX, topY, topPanelW, topPanelH, panelRadius);
    drawFrostedPanel(p, panelX, topY, topPanelW, topPanelH, panelRadius, blurPtr, screenW, screenH);
    
    // Draw internal separators (faint)
    p.setPen(QPen(QColor(255, 255, 255, 18), 1.0));
    p.drawLine(QPointF(panelX, topY + tileH), QPointF(panelX + topPanelW, topY + tileH));
    for (int i = 1; i < 5; ++i) {
        if (i == 1 || i == 4)
            p.drawLine(QPointF(panelX + i * tileW, topY), QPointF(panelX + i * tileW, topY + tileH));
        p.drawLine(QPointF(panelX + i * tileW, topY + tileH), QPointF(panelX + i * tileW, topY + topPanelH));
    }

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

    // Row 1: Settings, Size, Expand
    {
        QRectF r1(panelX, topY, tileW, tileH);
        m_recTileRects.append(r1);
        if (m_hoveredRecordTile == RecordPanelTile::Controls) drawTileHover(r1, 10, true, false, false, false);
        drawToolbarIcon(p, 8, r1.center().x(), r1.center().y(), Qt::white);

        QRectF rSize(panelX + tileW, topY, tileW * 3, tileH);
        m_recTileRects.append(rSize);
        if (m_hoveredRecordTile == RecordPanelTile::Size) drawTileHover(rSize);
        double cx = rSize.center().x();
        double cy = rSize.center().y();
        QString wStr = QString::number((int)selW), hStr = QString::number((int)selH);
        
        QFont f; f.setFamily("Sans"); f.setPointSizeF(11.0); f.setBold(true); p.setFont(f);
        QFontMetricsF fm(f);
        auto drawNumBox = [&](double nx, const QString& txt) {
            // Match .editor-tools-group and entry.editor-crop-size-entry styles:
            // background: #000000, border: 1px solid rgba(255, 255, 255, 0.11), radius: 6px
            QRectF box(nx - 24, cy - 13, 48, 26);
            p.setPen(QPen(QColor(255, 255, 255, 28), 1.0)); // 0.11 * 255 ≈ 28
            p.setBrush(QColor(0, 0, 0));
            p.drawRect(box);
            
            p.setFont(f);
            p.setPen(QColor(241, 241, 243)); // Match editor color: #F1F1F3
            p.drawText(box, Qt::AlignCenter, txt);
        };
        drawNumBox(cx - 36, wStr);
        p.setFont(f);
        p.setPen(QColor(accentColor.lighter(130)));
        p.drawText(QRectF(cx - 10, cy - 13, 20, 26), Qt::AlignCenter, "×");
        drawNumBox(cx + 36, hStr);

        QRectF rExpand(panelX + tileW * 4, topY, tileW, tileH);
        m_recTileRects.append(rExpand);
        if (m_hoveredRecordTile == RecordPanelTile::Crop) drawTileHover(rExpand, 10, false, true, false, false);
        drawToolbarIcon(p, 10, rExpand.center().x(), rExpand.center().y(), Qt::white);
    }

    // Row 2: Mic, Window, Video, Pointer, Keys
    {
        for (int i = 0; i < 5; ++i) {
            QRectF r(panelX + i * tileW, topY + tileH, tileW, tileH);
            m_recTileRects.append(r);
            bool hovered = (m_hoveredRecordTile == (RecordPanelTile)((int)RecordPanelTile::Mic + i));
            bool active = false;
            if (i == 0)      active = m_recMic;
            else if (i == 1) active = m_recSpeaker;
            else if (i == 2) active = m_recWebcam;
            else if (i == 3) active = m_recClicks;
            else if (i == 4) active = m_recKeystrokes;

            if (hovered) {
                // Fainter hover for certain active tiles to see animations better
                int alpha = active ? 12 : 22;
                drawTileHover(r, 10, false, false, (i == 0), (i == 4));
            }

            // Enhanced Mic animation (Multi-bar VU meter)
            if (i == 0 && active) {
                p.save();
                p.setRenderHint(QPainter::Antialiasing);
                const int numBars = 5;
                const double barW = 3.5;
                const double spacing = 1.5;
                const double totalW = numBars * barW + (numBars - 1) * spacing;
                const double maxH = 18.0;
                double baseX = r.center().x() - totalW / 2.0;
                double baseY = r.center().y() + 10.0;

                for (int b = 0; b < numBars; ++b) {
                    // Each bar has a slightly varied response based on m_micLevel
                    double offset = (double)b / (double)numBars;
                    double barLevel = std::max(0.05, m_micLevel - std::abs(offset - 0.5) * 0.3);
                    double levelH = barLevel * maxH;
                    
                    QRectF bar(baseX + b * (barW + spacing), baseY - levelH, barW, levelH);
                    
                    QLinearGradient grad(bar.topLeft(), bar.bottomLeft());
                    if (barLevel > 0.85) {
                        grad.setColorAt(0, QColor(255, 60, 60)); // Peak Red
                        grad.setColorAt(1, QColor(255, 140, 0)); // Warning Orange
                    } else if (barLevel > 0.6) {
                        grad.setColorAt(0, QColor(255, 190, 0)); // High Yellow/Gold
                        grad.setColorAt(1, QColor(255, 140, 0)); // Normal Orange
                    } else {
                        grad.setColorAt(0, QColor(255, 150, 50)); // Normal Orange
                        grad.setColorAt(1, QColor(255, 100, 0)); // Deep Orange
                    }
                    
                    p.setBrush(grad);
                    p.setPen(Qt::NoPen);
                    p.drawRoundedRect(bar, 1.5, 1.5);
                }
                p.restore();
            }

            // Speaker animation (Multi-bar VU meter — cool blue/teal)
            if (i == 1 && active) {
                p.save();
                p.setRenderHint(QPainter::Antialiasing);
                const int numBars = 5;
                const double barW = 3.5;
                const double spacing = 1.5;
                const double totalW = numBars * barW + (numBars - 1) * spacing;
                const double maxH = 18.0;
                double baseX = r.center().x() - totalW / 2.0;
                double baseY = r.center().y() + 10.0;

                for (int b = 0; b < numBars; ++b) {
                    double offset = (double)b / (double)numBars;
                    double barLevel = std::max(0.05, m_speakerLevel - std::abs(offset - 0.5) * 0.3);
                    double levelH = barLevel * maxH;

                    QRectF bar(baseX + b * (barW + spacing), baseY - levelH, barW, levelH);

                    QLinearGradient grad(bar.topLeft(), bar.bottomLeft());
                    if (barLevel > 0.85) {
                        grad.setColorAt(0, QColor(255, 80, 80));   // Peak Red
                        grad.setColorAt(1, QColor(60, 160, 255));  // Bright Blue
                    } else if (barLevel > 0.6) {
                        grad.setColorAt(0, QColor(60, 200, 255));  // Cyan
                        grad.setColorAt(1, QColor(40, 140, 255));  // Blue
                    } else {
                        grad.setColorAt(0, QColor(50, 180, 255));  // Light Blue
                        grad.setColorAt(1, QColor(0, 120, 255));   // Deep Blue
                    }

                    p.setBrush(grad);
                    p.setPen(Qt::NoPen);
                    p.drawRoundedRect(bar, 1.5, 1.5);
                }
                p.restore();
            }

            drawActiveIndicator(r, active);
            drawToolbarIcon(p, 11 + i, r.center().x(), r.center().y() - (active ? 3 : 0), Qt::white);
        }
    }

    // ── Section 2: Bottom Panel ───────────────────────────────────────────
    drawPanelGlow(panelX, bottomY, bottomPanelW, bottomPanelH, panelRadius);
    drawFrostedPanel(p, panelX, bottomY, bottomPanelW, bottomPanelH, panelRadius, blurPtr, screenW, screenH);
    
    p.setPen(QPen(QColor(255, 255, 255, 18), 1.0));
    p.drawLine(QPointF(panelX + 12, bottomY + bottomRowH), QPointF(panelX + bottomPanelW - 12, bottomY + bottomRowH));

    auto drawActionRow = [&](int rowIdx, int iconIdx, const QString& title, const QString& shortcut, RecordPanelTile tile) {
        QRectF row(panelX, bottomY + rowIdx * bottomRowH, bottomPanelW, bottomRowH);
        m_recTileRects.append(row);
        bool hovered = (m_hoveredRecordTile == tile);
        if (hovered) {
             drawTileHover(row, 10, (rowIdx == 0), (rowIdx == 0), (rowIdx == 1), (rowIdx == 1));
        }

        drawToolbarIcon(p, iconIdx, panelX + 30, row.center().y(), Qt::white);
        
        QFont f; f.setFamily("Sans"); f.setPointSizeF(12.5); f.setBold(true); p.setFont(f);
        p.setPen(Qt::white);
        p.drawText(QRectF(panelX + 60, row.y(), 200, row.height()), Qt::AlignVCenter, title);

        QFont sf; sf.setPointSizeF(11.0); p.setFont(sf);
        p.setPen(QColor(255, 255, 255, 160));
        p.drawText(QRectF(panelX + bottomPanelW - 100, row.y(), 90, row.height()), Qt::AlignVCenter | Qt::AlignRight, shortcut);
    };

    drawActionRow(0, 17, "Record GIF", "⌥ ↵", RecordPanelTile::RecordGif);
    drawActionRow(1, 16, "Record Video", "↵", RecordPanelTile::RecordVideo);

    if (m_settingsOpen) {
        drawSettingsMenu(p, panelX, startY);
    }
}

void CaptureOverlay::drawSettingsMenu(QPainter& p, double panelX, double startY)
{
    const double menuW = 440.0;
    const double menuH = 500.0;
    const double menuX = std::max(10.0, std::min(panelX + (300.0 - menuW) / 2.0, (double)width() - menuW - 10.0));
    
    // Check space above. Recording panel height is ~216px.
    double menuY = startY - menuH - 12.0; 
    if (menuY < 10.0) {
        // Not enough space above, show below the recording panel
        double panelBottom = startY + 216.0;
        if (panelBottom + menuH + 12.0 < height()) {
            menuY = panelBottom + 12.0;
        } else {
            menuY = 10.0;
        }
    }
    
    m_settingsPanelRect = QRectF(menuX, menuY, menuW, menuH);
    m_settingsClickableRects.clear();

    const QColor accentColor(122, 100, 255);
    const QImage* blurPtr = m_blurredBg.isNull() ? nullptr : &m_blurredBg;

    // Outer glow
    p.save();
    QRadialGradient glow(menuX + menuW/2.0, menuY + menuH/2.0, menuW);
    glow.setColorAt(0, QColor(accentColor.red(), accentColor.green(), accentColor.blue(), 35));
    glow.setColorAt(0.6, QColor(0, 0, 0, 0));
    p.fillRect(QRectF(menuX - 40, menuY - 40, menuW + 80, menuH + 80), glow);
    p.restore();

    drawFrostedPanel(p, menuX, menuY, menuW, menuH, 12.0, blurPtr, width(), height());

    // Tabs
    const QStringList tabs = {"General", "Video", "GIF"};
    const double tabW = 70.0;
    const double tabH = 30.0;
    double tabStartX = menuX + (menuW - tabs.size() * tabW) / 2.0;
    double tabY = menuY + 15.0;

    for (int i = 0; i < tabs.size(); ++i) {
        QRectF tr(tabStartX + i * tabW, tabY, tabW, tabH);
        m_settingsClickableRects.append(tr); // tab rects
        
        bool hovered = (m_hoveredSettingsItem == i);
        if (m_settingsTab == i || hovered) {
            p.setPen(Qt::NoPen);
            p.setBrush(QColor(255, 255, 255, m_settingsTab == i ? 45 : 22));
            p.drawRoundedRect(tr, 8.0, 8.0);
            p.setPen(Qt::white);
        } else {
            p.setPen(QColor(255, 255, 255, 180));
        }
        
        QFont tf; tf.setFamily("Sans"); tf.setPointSizeF(11.0); tf.setBold(m_settingsTab == i);
        p.setFont(tf);
        p.drawText(tr, Qt::AlignCenter, tabs[i]);
    }

    if (m_settingsTab == 0) { // General
        double currY = tabY + 50.0;
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
            
            // Draw "Options..." button for certain settings
            if (desc == "Highlight clicks") {
                QRectF btn(menuX + menuW - 100, currY + (rowH - 24) / 2.0, 75, 24);
                m_settingsClickableRects.append(btn); // button rect
                
                p.setPen(Qt::NoPen);
                p.setBrush(QColor(255, 255, 255, 35));
                p.drawRoundedRect(btn, 6, 6);
                p.setPen(Qt::white);
                p.setFont(QFont("Sans", 9));
                p.drawText(btn, Qt::AlignCenter, "Options...");
            } else if (desc == "Show keystrokes") {
                QRectF btn(menuX + menuW - 100, currY + (rowH - 24) / 2.0, 75, 24);
                m_settingsClickableRects.append(btn); // button rect
                
                p.setPen(Qt::NoPen);
                p.setBrush(QColor(255, 255, 255, 35));
                p.drawRoundedRect(btn, 6, 6);
                p.setPen(Qt::white);
                p.setFont(QFont("Sans", 9));
                p.drawText(btn, Qt::AlignCenter, "Options...");
            }
            
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
        double currY = tabY + 50.0;
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

        // 3. Audio
        drawLabel("Audio:", currY);
        QRectF audioBtn(valueX, currY, 200, 30);
        int audioIdx = m_settingsClickableRects.size();
        p.setPen(QPen(QColor(255, 255, 255, 40), 1));
        p.setBrush(QColor(255, 255, 255, 30));
        if (m_hoveredSettingsItem == audioIdx) p.setBrush(QColor(255, 255, 255, 50));
        p.drawRoundedRect(audioBtn, 6, 6);
        p.setPen(Qt::white);
        p.drawText(audioBtn, Qt::AlignCenter, "Computer Audio Settings...");
        m_settingsClickableRects.append(audioBtn);
        currY += 45;

        // 4. Record mono
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

        // 5. Video Encoder
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
        double currY = tabY + 50.0;
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
    
    // Background
    p.setPen(QPen(QColor(255, 255, 255, 40), 1));
    p.setBrush(QColor(35, 35, 35, 245));
    p.drawRoundedRect(menuRect, 8, 8);

    m_dropdownItemRects.clear();
    const bool hasColors = !m_dropdownColors.isEmpty();
    for (int i = 0; i < options.size(); ++i) {
        QRectF itemRect(menuX + 5, menuY + 5 + i * itemH, menuW - 10, itemH);
        m_dropdownItemRects.append(itemRect);

        bool hovered = (m_hoveredDropdownItem == i);
        if (hovered) {
            p.setPen(Qt::NoPen);
            p.setBrush(QColor(122, 100, 255, 180)); // ApexShot Indigo
            p.drawRoundedRect(itemRect, 6, 6);
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

        p.setPen(Qt::white);
        p.setFont(QFont("Sans", 10, selectedIndex == i ? QFont::Bold : QFont::Normal));
        p.drawText(QRectF(textX, itemRect.y(), itemRect.right() - textX - 10, itemRect.height()),
                   Qt::AlignLeft | Qt::AlignVCenter, options[i]);
        
        if (selectedIndex == i) {
            p.setBrush(Qt::white);
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

    const QColor accentColor(122, 100, 255);
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
    drawFrostedPanel(p, menuX, menuY, menuW, menuH, 12.0, blurPtr, width(), height());

    const double labelX = menuX + 25.0;
    const double valueX = menuX + 130.0;
    const double controlW = 280.0;
    const double rowH = 45.0;
    double currY = menuY + 40.0;

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
    p.setBrush(accentColor);
    p.drawRoundedRect(okBtn, 6, 6);
    p.setPen(Qt::white);
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

    const QColor accentColor(122, 100, 255);
    const QImage* blurPtr = m_blurredBg.isNull() ? nullptr : &m_blurredBg;

    drawFrostedPanel(p, menuX, menuY, menuW, menuH, 12.0, blurPtr, width(), height());

    const double labelX = menuX + 20.0;
    const double valueX = menuX + 130.0;
    const double controlW = 280.0;
    const double rowH = 45.0;
    double currY = menuY + 40.0;

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
    p.setBrush(m_showKeystrokePreview ? accentColor.lighter(120) : QColor(255, 255, 255, 30)); 
    p.drawRoundedRect(prevBtn, 6, 6);
    p.setPen(Qt::white); p.setFont(QFont("Sans", 10, m_showKeystrokePreview ? QFont::Bold : QFont::Normal)); 
    p.drawText(prevBtn, Qt::AlignCenter, "Preview");
    m_keystrokeOptionsClickableRects.append(prevBtn); // index 6

    QRectF okBtn(menuX + menuW - 90, menuY + menuH - 45, 75, 30);
    p.setPen(Qt::NoPen); p.setBrush(accentColor); p.drawRoundedRect(okBtn, 6, 6);
    p.setPen(Qt::white); p.setFont(QFont("Sans", 10, QFont::Bold)); p.drawText(okBtn, Qt::AlignCenter, "OK");
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

    int activeTool = 1;
    if (scrollModeActive) {
        activeTool = 4;
    }
    if (m_fullscreenMode) {
        activeTool = 2;
    }
    if (m_captureIntent == CaptureIntent::Ocr) {
        activeTool = 6;
    }

    const bool timerToolEnabled = m_timerCaptureEnabled && !scrollModeActive;
    const bool timerToolActive = timerToolEnabled && m_timerDelayActive && m_captureDelaySeconds > 0;

    drawFrostedPanel(p,
                     layout.toolsPanel.x(), layout.toolsPanel.y(),
                     layout.toolsPanel.width(), layout.toolsPanel.height(),
                     FEATURE_PANEL_RADIUS, blurPtr, screenW, screenH);

    auto drawActiveToolCell = [&](int toolIndex) {
        if (toolIndex < 0 || toolIndex >= NUM_TOOLS) {
            return;
        }

        QRectF cell = layout.itemCells[toolIndex];
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
    };

    drawActiveToolCell(activeTool);
    if (timerToolActive && activeTool != 5) {
        drawActiveToolCell(5);
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
        bool active = (activeTool == i) || (i == 5 && timerToolActive);
        bool enabled = (i != 5) || timerToolEnabled;
        double iconAlpha = enabled ? ((hovered || active) ? 1.0 : 0.98) : 0.42;
        double shadowAlpha = enabled ? (hovered ? 0.30 : (active ? 0.38 : 0.52)) : 0.22;
        double iconY = layout.toolsPanel.y() + ((hovered || active) ? 15.5 : 16.0);
        QColor iconColor = active
            ? QColor(223, 241, 255, int(iconAlpha * 255))
            : QColor(255, 255, 255, int(iconAlpha * 255));

        drawToolbarIcon(p, i, cx + 0.6, iconY + 0.8,
                        QColor(0,0,0, int(shadowAlpha*255)));
        drawToolbarIcon(p, i, cx, iconY, iconColor);

        QFont f; f.setFamily("Sans"); f.setPointSizeF(7.5);
        f.setBold(hovered || active); p.setFont(f);
        QFontMetricsF fm(f);
        QString label(TOOLBAR_LABELS[i]);

        p.setPen(QColor(0,0,0, int(shadowAlpha*255)));
        double tw = fm.horizontalAdvance(label);
        p.drawText(QPointF(cx - tw/2.0 + 0.6,
                           layout.toolsPanel.y() + 34.0 + 0.8), label);
        p.setPen(active
            ? QColor(223,241,255, int(iconAlpha * 255))
            : QColor(255,255,255, int(iconAlpha * 255)));
        p.drawText(QPointF(cx - tw/2.0,
                           layout.toolsPanel.y() + 34.0), label);

        if (i == 5 && timerToolActive) {
            const QString badgeText = QStringLiteral("%1s").arg(m_captureDelaySeconds);
            QFont badgeFont; badgeFont.setFamily("Sans"); badgeFont.setPointSizeF(6.6); badgeFont.setBold(true);
            p.setFont(badgeFont);
            QFontMetricsF badgeMetrics(badgeFont);
            const double badgeTextW = badgeMetrics.horizontalAdvance(badgeText);
            const double badgeW = std::max(22.0, badgeTextW + 10.0);
            const QRectF badgeRect(cell.right() - badgeW - 5.0, cell.y() + 5.0, badgeW, 14.0);
            QPainterPath badgePath;
            roundedRectPath(badgePath, badgeRect.x(), badgeRect.y(), badgeRect.width(), badgeRect.height(), 7.0);
            p.fillPath(badgePath, QColor(0, 122, 255, 230));
            p.setPen(QColor(255, 255, 255, 248));
            p.drawText(badgeRect, Qt::AlignCenter, badgeText);
        }
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
