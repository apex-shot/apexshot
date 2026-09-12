#include "CaptureOverlay.h"
#include "CaptureOverlay_DrawingPrimitives_p.h"
#include "CaptureOverlay_p.h"

#include <QColor>
#include <QFont>
#include <QFontMetrics>
#include <QImage>
#include <QLinearGradient>
#include <QPainter>
#include <QPainterPath>
#include <QPen>

#include <algorithm>

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

} // namespace

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
    const QRectF videoRect(bottomX, bottomY, ACTION_RAIL_W, ACTION_CARD_H);
    const QRectF gifRect(videoRect.right() + ACTION_CARD_GAP, bottomY, ACTION_RAIL_W, ACTION_CARD_H);
    drawFrostedPanel(p, videoRect.x(), videoRect.y(), videoRect.width(), videoRect.height(),
                     panelRadius, blurPtr, screenW, screenH);
    drawFrostedPanel(p, gifRect.x(), gifRect.y(), gifRect.width(), gifRect.height(),
                     panelRadius, blurPtr, screenW, screenH);
    m_recTileRects.append(videoRect);
    m_recTileRects.append(gifRect);
    drawPrimaryAction(videoRect, RecordPanelTile::RecordVideo, 16, QStringLiteral("Video"), m_recordType == RecordType::Video);
    drawPrimaryAction(gifRect, RecordPanelTile::RecordGif, 17, QStringLiteral("GIF"), m_recordType == RecordType::Gif);

    const double contextualX = std::max(10.0, std::min(selX + (selW - 440.0) / 2.0, screenW - 450.0));
    const double contextualY = std::max(10.0, std::min(selY + 24.0, screenH - 400.0));
    const QRectF contextualRect(contextualX, contextualY, 440.0, 390.0);

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
