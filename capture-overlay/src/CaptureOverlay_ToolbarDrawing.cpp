#include "CaptureOverlay.h"
#include "CaptureOverlay_DrawingPrimitives_p.h"
#include "CaptureOverlay_p.h"

#include <QColor>
#include <QFont>
#include <QFontMetrics>
#include <QImage>
#include <QPainter>
#include <QPainterPath>
#include <QPen>

#include <algorithm>
#include <cmath>

const char* TOOLBAR_LABELS[NUM_TOOLS] = {
    "Area", "Fullscreen", "Scroll", "Timer", "OCR", "Recording"
};

// Icon glyph ids for drawToolbarIcon (Window glyph id 3 intentionally unused)
const int TOOLBAR_ICON_IDS[NUM_TOOLS] = {
    1, 2, 4, 5, 6, 7
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

// Keep bottom-anchored chrome above the app dock.
constexpr double kDockLift = 76.0;
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

    // Indices: 0 Area, 1 Fullscreen, 2 Scroll, 3 Timer, 4 OCR, 5 Recording
    int activeTool = 0;
    if (scrollModeActive) {
        activeTool = 2;
    }
    if (m_fullscreenMode) {
        activeTool = 1;
    }
    if (m_captureIntent == CaptureIntent::Ocr) {
        activeTool = 4;
    }
    if (m_captureIntent == CaptureIntent::Record) {
        activeTool = 5;
    }

    const bool timerToolEnabled = m_timerCaptureEnabled && !scrollModeActive;
    const bool timerToolActive = timerToolEnabled && m_timerDelayActive && m_captureDelaySeconds > 0;
    constexpr int kTimerToolIndex = 3;

    if (!m_captureMenuAreaMode) {
        drawFrostedPanel(p,
                         layout.leftToolsPanel.x(), layout.leftToolsPanel.y(),
                         layout.leftToolsPanel.width(), layout.leftToolsPanel.height(),
                         FEATURE_PANEL_RADIUS, blurPtr, screenW, screenH);
    }

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

    if (!m_captureMenuAreaMode) {
        drawActiveToolCell(activeTool);
        if (timerToolActive && activeTool != kTimerToolIndex) {
            drawActiveToolCell(kTimerToolIndex);
        }
    }

    // ── Hover highlight on hovered tool ──────────────────────────────────────
    if (!m_captureMenuAreaMode && m_hoveredTool >= 0 && m_hoveredTool < NUM_TOOLS) {
        drawAccentCard(
            layout.toolCells[m_hoveredTool],
            QColor(255, 255, 255, 22),
            QColor(255, 255, 255, 86),
            10.0,
            false
        );
    }

    // ── Tool icons + labels ───────────────────────────────────────────────────
    for (int i = 0; !m_captureMenuAreaMode && i < NUM_TOOLS; ++i) {
        QRectF cell = layout.toolCells[i];
        double cx = cell.x() + cell.width() / 2.0;
        bool hovered = (m_hoveredTool == i);
        bool active = (activeTool == i) || (i == kTimerToolIndex && timerToolActive);
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

        if (i == kTimerToolIndex && timerToolActive) {
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

    Q_UNUSED(selW);
    Q_UNUSED(selH);
    auto drawActionLabel = [&](const QRectF& rect, const QString& text, bool primary) {
        QFont f; f.setFamily("Sans"); f.setPointSizeF(9.0); f.setBold(true); p.setFont(f);
        p.setPen(QColor(0, 0, 0, primary ? 132 : 118));
        p.drawText(rect.translated(0.6, 0.8), Qt::AlignCenter, text);
        p.setPen(primary ? QColor(255, 231, 214, 248) : QColor(244, 244, 244, 244));
        p.drawText(rect, Qt::AlignCenter, text);
    };

    // Old FRAME size/crop card + aspect dropdown removed — the top-center
    // instruction bar now owns aspect selection. Keep cached rects cleared.
    Q_UNUSED(drawActionLabel);
    Q_UNUSED(kRecordingAspectOptions);
    Q_UNUSED(kRecordingAspectOptionCount);
    Q_UNUSED(kDockLift);
    m_captureCropMenuOpen = false;
    m_captureCropMenuPanelRect = QRectF();
    m_captureCropMenuItemRects.clear();

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

// ── Top-center instruction bar ("Draw an area" frame) ─────────────────────────
// Screen-fixed chrome. The final screenshot crops the pre-overlay freeze (or
// re-captures after hide()+settle in main.cpp), so this bar can safely overlap
// the selection area on screen without ever appearing in the saved image.
// Selection drags may pass underneath it: presses starting inside the bar are
// swallowed for aspect/button handling, drags started elsewhere continue
// normally even when the pointer moves over the bar.

namespace {
constexpr int kTopBarPillAspectIndex[4] = {0, 7, 3, 1}; // Free, 16:9, 4:3, 1:1
const char* kTopBarPillLabels[4] = {"Free", "16:9", "4:3", "1:1"};
constexpr double kTopBarAspectRatios[4] = {0.0, 16.0 / 9.0, 4.0 / 3.0, 1.0};

// Crop dropdown rows (matches reference screenshot).
enum TopBarCropRow {
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
struct TopBarCropOption {
    const char* label;
    double ratio; // 0.0 = Free / N/A for action rows
};
constexpr TopBarCropOption kTopBarCropOptions[CropRowCount] = {
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
constexpr double kTopBarCropMenuW = 196.0;
constexpr double kTopBarCropItemH = 32.0;

bool ratiosEqual(double a, double b)
{
    if (a <= 0.0 && b <= 0.0) {
        return true;
    }
    if (a <= 0.0 || b <= 0.0) {
        return false;
    }
    return std::fabs(a - b) < 0.001;
}

int captureAspectIndexForRatio(double ratio)
{
    if (ratio <= 0.0) {
        return 0;
    }
    if (ratiosEqual(ratio, 1.0)) {
        return 1;
    }
    if (ratiosEqual(ratio, 4.0 / 3.0)) {
        return 3;
    }
    if (ratiosEqual(ratio, 16.0 / 9.0)) {
        return 7;
    }
    return -1; // custom ratio with no legacy index
}
}

bool CaptureOverlay::topBarVisible() const
{
    if (isCrosshairMode() || m_windowMode || m_recordingPanelOpen) {
        return false;
    }
    if (m_countdownActive) {
        return false;
    }
    if (m_captureIntent == CaptureIntent::Scroll && m_scrollStage == ScrollStage::Capturing) {
        return false;
    }
    return m_captureIntent != CaptureIntent::Record;
}

int CaptureOverlay::hitTestTopBarAspect(const QPoint& pos) const
{
    if (!topBarVisible()) {
        return -1;
    }
    const TopBarLayout layout = computeTopBarLayout(width());
    if (!layout.valid || !layout.bar.contains(pos)) {
        return -1;
    }
    for (int i = 0; i < 4; ++i) {
        if (layout.aspectPills[i].contains(pos)) {
            return i;
        }
    }
    return -1;
}

CaptureOverlay::TopBarButton CaptureOverlay::hitTestTopBarButton(const QPoint& pos) const
{
    if (!topBarVisible()) {
        return TopBarButton::None;
    }
    const TopBarLayout layout = computeTopBarLayout(width());
    if (!layout.valid || !layout.bar.contains(pos)) {
        return TopBarButton::None;
    }
    static const TopBarButton order[2] = {
        TopBarButton::Crop, TopBarButton::Cancel
    };
    for (int i = 0; i < 2; ++i) {
        if (layout.buttons[i].contains(pos)) {
            return order[i];
        }
    }
    return TopBarButton::None;
}

bool CaptureOverlay::pointInTopBar(const QPoint& pos) const
{
    if (!topBarVisible()) {
        return false;
    }
    const TopBarLayout layout = computeTopBarLayout(width());
    return layout.valid && layout.bar.contains(pos);
}

void CaptureOverlay::applyTopBarAspectToSelection()
{
    const double ratio = m_topBarAspectRatio;
    if (ratio <= 0.0 || !m_hasSelection) {
        update();
        return;
    }
    const QRect bounds = rect();
    QRect sel = m_selection.normalized();
    double newW = sel.width();
    double newH = newW / ratio;
    if (newH > sel.height()) {
        newH = sel.height();
        newW = newH * ratio;
    }
    newW = std::max<double>(kMinSize, std::min<double>(newW, bounds.width()));
    newH = std::max<double>(kMinSize, std::min<double>(newH, bounds.height()));
    const QPoint center = sel.center();
    int x = center.x() - static_cast<int>(std::round(newW / 2.0));
    int y = center.y() - static_cast<int>(std::round(newH / 2.0));
    int w = std::max(kMinSize, static_cast<int>(std::round(newW)));
    int h = std::max(kMinSize, static_cast<int>(std::round(newH)));
    x = std::max(0, std::min(x, bounds.width() - w));
    y = std::max(0, std::min(y, bounds.height() - h));
    m_selection = QRect(x, y, w, h);
    m_hasSelection = true;
    m_fullscreenMode = false;
    update();
}

void CaptureOverlay::handleTopBarAspectClick(int pillIndex)
{
    if (pillIndex < 0 || pillIndex >= 4) {
        return;
    }
    m_topBarAspectRatio = kTopBarAspectRatios[pillIndex];
    m_captureAspectRatioIndex = kTopBarPillAspectIndex[pillIndex];
    m_captureCropMenuOpen = false;
    m_topBarCropMenuOpen = false;
    applyTopBarAspectToSelection();
}

void CaptureOverlay::handleTopBarButtonClick(TopBarButton button)
{
    switch (button) {
    case TopBarButton::Crop: {
        m_topBarCropMenuOpen = !m_topBarCropMenuOpen;
        m_hoveredTopBarCropItem = -1;
        update();
        break;
    }
    case TopBarButton::Cancel:
        m_topBarCropMenuOpen = false;
        cancelSelection();
        break;
    case TopBarButton::None:
        break;
    }
}

void CaptureOverlay::handleTopBarCropMenuClick(const QPoint& pos)
{
    if (!m_topBarCropMenuOpen) {
        return;
    }
    int hit = -1;
    for (int i = 0; i < m_topBarCropMenuItemRects.size(); ++i) {
        if (m_topBarCropMenuItemRects[i].contains(pos)) {
            hit = i;
            break;
        }
    }
    if (hit < 0) {
        return;
    }
    if (hit == CropRowReset) {
        const int defaultW = std::max(kMinSize, std::min(DEFAULT_SELECTION_W, width()));
        const int defaultH = std::max(kMinSize, std::min(DEFAULT_SELECTION_H, height()));
        m_selection = QRect((width() - defaultW) / 2, (height() - defaultH) / 2, defaultW, defaultH);
        m_hasSelection = true;
        m_fullscreenMode = false;
        m_topBarAspectRatio = 0.0;
        m_captureAspectRatioIndex = 0;
        m_topBarCropMenuOpen = false;
        m_hoveredTopBarCropItem = -1;
        update();
        return;
    }
    if (hit == CropRowSnap) {
        m_topBarSnapToRatios = !m_topBarSnapToRatios;
        update();
        return;
    }
    if (hit >= CropRowFree && hit <= CropRow21_9) {
        const double ratio = kTopBarCropOptions[hit].ratio;
        m_topBarAspectRatio = ratio;
        m_captureAspectRatioIndex = captureAspectIndexForRatio(ratio);
        m_topBarCropMenuOpen = false;
        m_hoveredTopBarCropItem = -1;
        applyTopBarAspectToSelection();
    }
}

void CaptureOverlay::drawTopInstructionBar(QPainter& p, double screenW, double /*screenH*/)
{
    if (!topBarVisible()) {
        return;
    }
    const TopBarLayout layout = computeTopBarLayout(screenW);
    if (!layout.valid) {
        return;
    }
    const QRectF bar = layout.bar;
    p.save();
    p.setRenderHint(QPainter::Antialiasing);
    // Shadow + solid dark body + rim (matches quick-capture panel language).
    {
        QPainterPath shadow;
        roundedRectPath(shadow, bar.x(), bar.y() + 3.0, bar.width(), bar.height(), TOP_BAR_RADIUS);
        p.fillPath(shadow, QColor(0, 0, 0, 105));
        QPainterPath body;
        roundedRectPath(body, bar.x(), bar.y(), bar.width(), bar.height(), TOP_BAR_RADIUS);
        p.fillPath(body, QColor(25, 25, 28, 246));
        p.setPen(QPen(QColor(255, 255, 255, 54), 1.2));
        p.drawPath(body);
    }
    // Label.
    {
        QFont f(QStringLiteral("Sans"));
        f.setPointSizeF(10.5);
        f.setBold(true);
        p.setFont(f);
        p.setPen(QColor(244, 244, 246, 245));
        p.drawText(layout.labelRect, Qt::AlignVCenter | Qt::AlignLeft, QStringLiteral("Draw an area"));
    }
    // Separators.
    {
        const double sep1X = layout.labelRect.right() + 12.0;
        const double sep2X = layout.aspectPills[3].right() + 12.0;
        p.setPen(QPen(QColor(255, 255, 255, 30), 1.0));
        p.drawLine(QPointF(sep1X, bar.y() + 10.0), QPointF(sep1X, bar.y() + bar.height() - 10.0));
        p.drawLine(QPointF(sep2X, bar.y() + 10.0), QPointF(sep2X, bar.y() + bar.height() - 10.0));
    }
    // Aspect pills (active when the current ratio matches, Free = 0.0).
    for (int i = 0; i < 4; ++i) {
        const QRectF pill = layout.aspectPills[i];
        const bool active = ratiosEqual(m_topBarAspectRatio, kTopBarAspectRatios[i]);
        const bool hovered = (m_hoveredTopBarAspect == i);
        QPainterPath path;
        roundedRectPath(path, pill.x(), pill.y(), pill.width(), pill.height(), 8.0);
        if (active) {
            p.fillPath(path, QColor(255, 102, 0, 205));
        } else if (hovered) {
            p.fillPath(path, QColor(255, 255, 255, 26));
        } else {
            p.fillPath(path, QColor(255, 255, 255, 12));
        }
        QFont f(QStringLiteral("Sans"));
        f.setPointSizeF(9.5);
        f.setBold(active || hovered);
        p.setFont(f);
        p.setPen(active ? QColor(255, 255, 255) : QColor(232, 232, 236));
        p.drawText(pill, Qt::AlignCenter, QString::fromUtf8(kTopBarPillLabels[i]));
    }
    // Action buttons: crop (opens ratio dropdown) + cancel X.
    static const TopBarButton buttonOrder[2] = {TopBarButton::Crop, TopBarButton::Cancel};
    for (int i = 0; i < 2; ++i) {
        const QRectF btn = layout.buttons[i];
        const bool hovered = (m_hoveredTopBarButton == buttonOrder[i]);
        const bool cropActive = (i == 0 && (m_topBarCropMenuOpen || m_topBarAspectRatio > 0.0));
        QPainterPath path;
        roundedRectPath(path, btn.x(), btn.y(), btn.width(), btn.height(), 8.0);
        if (cropActive) {
            p.fillPath(path, QColor(255, 102, 0, 205));
        } else if (hovered) {
            p.fillPath(path, QColor(255, 255, 255, 26));
        }
        const double cx = btn.center().x();
        const double cy = btn.center().y();
        const QColor iconColor = (cropActive) ? QColor(255, 255, 255) : QColor(240, 240, 244);
        if (i == 0) { // Crop icon (matches editor toolbar crop glyph)
            drawToolbarIcon(p, 10, cx, cy, iconColor);
        } else { // Cancel: X
            p.setPen(QPen(iconColor, 1.7, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
            p.setBrush(Qt::NoBrush);
            p.drawLine(QPointF(cx - 4.5, cy - 4.5), QPointF(cx + 4.5, cy + 4.5));
            p.drawLine(QPointF(cx - 4.5, cy + 4.5), QPointF(cx + 4.5, cy - 4.5));
        }
    }
    p.restore();
    drawTopBarCropMenu(p, screenW, 0.0);
}

void CaptureOverlay::drawTopBarCropMenu(QPainter& p, double screenW, double screenH)
{
    Q_UNUSED(screenH);
    if (!topBarVisible() || !m_topBarCropMenuOpen) {
        if (!m_topBarCropMenuOpen) {
            m_topBarCropMenuPanelRect = QRectF();
            m_topBarCropMenuItemRects.clear();
        }
        return;
    }
    const TopBarLayout layout = computeTopBarLayout(screenW);
    if (!layout.valid) {
        return;
    }
    const QRectF anchor = layout.buttons[0];
    const double menuW = kTopBarCropMenuW;
    const double menuH = (CropRowCount * kTopBarCropItemH) + 10.0;
    const double menuX = std::max(10.0, std::min(anchor.center().x() - menuW / 2.0, screenW - menuW - 10.0));
    const double anchorBottom = anchor.bottom() + 8.0;
    const double menuY = std::max(layout.bar.bottom() + 4.0, anchorBottom);
    m_topBarCropMenuPanelRect = QRectF(menuX, menuY, menuW, menuH);
    m_topBarCropMenuItemRects.clear();
    p.save();
    p.setRenderHint(QPainter::Antialiasing);
    {
        QPainterPath shadow;
        roundedRectPath(shadow, menuX, menuY + 3.0, menuW, menuH, 12.0);
        p.fillPath(shadow, QColor(0, 0, 0, 105));
        QPainterPath body;
        roundedRectPath(body, menuX, menuY, menuW, menuH, 12.0);
        p.fillPath(body, QColor(25, 25, 28, 246));
        p.setPen(QPen(QColor(255, 255, 255, 54), 1.2));
        p.drawPath(body);
    }
    for (int i = 0; i < CropRowCount; ++i) {
        const QRectF itemRect(menuX + 5.0, menuY + 5.0 + (i * kTopBarCropItemH), menuW - 10.0, kTopBarCropItemH);
        m_topBarCropMenuItemRects.append(itemRect);
        const QRectF indicatorRect(itemRect.x() + 8.0, itemRect.y(), 18.0, itemRect.height());
        const QRectF labelRect(itemRect.x() + 30.0, itemRect.y(), itemRect.width() - 40.0, itemRect.height());
        if (i == m_hoveredTopBarCropItem) {
            QPainterPath hoverPath;
            roundedRectPath(hoverPath, itemRect.x(), itemRect.y(), itemRect.width(), itemRect.height(), 7.0);
            p.fillPath(hoverPath, QColor(255, 255, 255, 24));
            p.setPen(QPen(QColor(255, 102, 0, 170), 1.0));
            p.drawPath(hoverPath);
        }
        bool checked = false;
        if (i == CropRowFree) {
            checked = (m_topBarAspectRatio <= 0.0);
        } else if (i >= CropRow1_1 && i <= CropRow21_9) {
            checked = ratiosEqual(m_topBarAspectRatio, kTopBarCropOptions[i].ratio);
        } else if (i == CropRowSnap) {
            checked = m_topBarSnapToRatios;
        }
        if (checked && i != CropRowReset) {
            p.setPen(QPen(QColor(255, 255, 255), 1.5));
            p.setBrush(Qt::NoBrush);
            const double cy = indicatorRect.center().y();
            p.drawLine(QPointF(indicatorRect.x() + 3.5, cy), QPointF(indicatorRect.x() + 6.5, cy + 3.0));
            p.drawLine(QPointF(indicatorRect.x() + 6.5, cy + 3.0), QPointF(indicatorRect.x() + 12.5, cy - 4.0));
        }
        QFont itemFont(QStringLiteral("Sans"));
        itemFont.setPointSizeF(i == CropRowReset ? 10.0 : 10.0);
        itemFont.setBold(checked || i == CropRowReset);
        p.setFont(itemFont);
        p.setPen(QColor(242, 242, 244));
        p.drawText(labelRect, Qt::AlignVCenter | Qt::AlignLeft,
                   QString::fromUtf8(kTopBarCropOptions[i].label));
    }
    // Separators: below "Reset selection" and above "Snap to ratios".
    p.setPen(QPen(QColor(255, 255, 255, 26), 1.0));
    {
        const double y1 = menuY + 5.0 + kTopBarCropItemH;
        p.drawLine(QPointF(menuX + 12.0, y1), QPointF(menuX + menuW - 12.0, y1));
        const double y2 = menuY + 5.0 + (CropRowSnap * kTopBarCropItemH);
        p.drawLine(QPointF(menuX + 12.0, y2), QPointF(menuX + menuW - 12.0, y2));
    }
    p.restore();
}
