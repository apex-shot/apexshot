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
    if (timerToolActive && activeTool != kTimerToolIndex) {
        drawActiveToolCell(kTimerToolIndex);
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
