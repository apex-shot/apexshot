#include "CaptureOverlay.h"
#include "CaptureOverlay_DrawingPrimitives_p.h"

#include <QColor>
#include <QFont>
#include <QFontMetrics>
#include <QImage>
#include <QPainter>
#include <QPainterPath>
#include <QPen>

#include <algorithm>

namespace {

} // namespace

void CaptureOverlay::drawSettingsMenu(QPainter& p, double panelX, double startY)
{
    const double menuW = 440.0;
    const double menuH = 390.0;
    const double menuX = std::max(10.0, std::min(panelX, (double)width() - menuW - 10.0));
    const double menuY = std::max(10.0, std::min(startY, (double)height() - menuH - 10.0));

    m_settingsPanelRect = QRectF(menuX, menuY, menuW, menuH);
    m_settingsClickableRects.clear();

    const QColor accentColor(176, 92, 56);
    drawFrostedPanel(p, menuX, menuY, menuW, menuH, 10.0, nullptr, width(), height());

    QFont titleFont(QStringLiteral("Sans"));
    titleFont.setPixelSize(14);
    titleFont.setWeight(QFont::DemiBold);
    p.setFont(titleFont);
    p.setPen(QColor(241, 241, 243, 230));
    p.drawText(QRectF(menuX + 18.0, menuY + 16.0, menuW - 36.0, 22.0),
               Qt::AlignLeft | Qt::AlignVCenter, QStringLiteral("Recording setup"));

    // Tabs
    const QStringList tabs = {"General", "Video", "GIF"};
    const double tabContainerX = menuX + 18.0;
    const double tabContainerY = menuY + 50.0;
    const double tabContainerW = menuW - 36.0;
    const double tabW = (tabContainerW - 8.0) / tabs.size();
    const double tabH = 30.0;
    const double tabStartX = tabContainerX + 4.0;
    const double tabY = tabContainerY + 4.0;

    p.setPen(QPen(QColor(255, 255, 255, 20), 1.0));
    p.setBrush(QColor(255, 255, 255, 10));
    p.drawRoundedRect(QRectF(tabContainerX, tabContainerY, tabContainerW, 38.0), 9.0, 9.0);

    for (int i = 0; i < tabs.size(); ++i) {
        QRectF tr(tabStartX + i * tabW, tabY, tabW, tabH);
        m_settingsClickableRects.append(tr); // tab rects

        bool hovered = (m_hoveredSettingsItem == i);
        if (m_settingsTab == i || hovered) {
            p.setPen(Qt::NoPen);
            p.setBrush(m_settingsTab == i ? accentColor : QColor(255, 255, 255, 20));
            p.drawRoundedRect(tr, 6.0, 6.0);
            p.setPen(m_settingsTab == i ? Qt::white : QColor(255, 255, 255, 230));
        } else {
            p.setPen(QColor(255, 255, 255, 155));
        }

        QFont tf; tf.setFamily("Sans"); tf.setPixelSize(12); tf.setWeight(m_settingsTab == i ? QFont::DemiBold : QFont::Medium);
        p.setFont(tf);
        p.drawText(tr, Qt::AlignCenter, tabs[i]);
    }

    if (m_settingsTab == 0) { // General
        double currY = menuY + 106.0;
        const double labelX = menuX + 32.0;
        const double valueX = menuX + menuW - 50.0;
        const double rowH = 44.0;

        p.setPen(Qt::NoPen);
        p.setBrush(QColor(255, 255, 255, 10));
        p.drawRoundedRect(QRectF(menuX + 18.0, currY, menuW - 36.0, rowH * 6.0), 10.0, 10.0);
        p.setPen(QPen(QColor(255, 255, 255, 13), 1.0));
        for (int i = 1; i < 6; ++i) {
            const double dividerY = currY + rowH * i;
            p.drawLine(QPointF(menuX + 32.0, dividerY), QPointF(menuX + menuW - 32.0, dividerY));
        }

        auto drawSetting = [&](const QString& label, const QString& desc, bool checked, bool* target,
                               bool disabled = false, const QString& badge = QString()) {
            QRectF labelRect(labelX, currY, 106, rowH);
            QFont labelFont(QStringLiteral("Sans")); labelFont.setPixelSize(12); labelFont.setWeight(QFont::DemiBold);
            p.setFont(labelFont);
            p.setPen(QColor(255, 255, 255, disabled ? 110 : 200));
            p.drawText(labelRect, Qt::AlignLeft | Qt::AlignVCenter, label);

            QRectF checkArea(menuX + 18.0, currY, menuW - 36.0, rowH);
            int itemIdx = m_settingsClickableRects.size();
            // Disabled rows still need a placeholder rect so the index stays
            // aligned with the click handler's switch on `itemIdx`. We use a
            // collapsed (empty) rect so it can never be hit.
            m_settingsClickableRects.append(disabled ? QRectF() : checkArea);

            bool hovered = !disabled && (m_hoveredSettingsItem == itemIdx);
            if (hovered) {
                p.setPen(Qt::NoPen);
                p.setBrush(QColor(255, 255, 255, 16));
                p.drawRoundedRect(checkArea, 6, 6);
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
                p.setPen(QPen(QColor(255, 255, 255, disabled ? 26 : 41), 1.0));
                p.setBrush(QColor(255, 255, 255, disabled ? 8 : 15));
                p.drawRoundedRect(cb, 4, 4);
            }

            QFont descFont(QStringLiteral("Sans")); descFont.setPixelSize(11);
            p.setFont(descFont);
            p.setPen(disabled ? QColor(255, 255, 255, 100) : QColor(255, 255, 255, 155));
            p.drawText(QRectF(labelX + 110.0, currY, valueX - labelX - 120.0, rowH),
                       Qt::AlignLeft | Qt::AlignVCenter, desc);

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

        drawSetting("Controls", "Keyboard shortcuts", m_recControls, &m_recControls);
        drawSetting("HiDPI", "Display scale resolution", m_hidpi, &m_hidpi);
        drawSetting("Notifications", "Do Not Disturb", m_doNotDisturb, &m_doNotDisturb);

        drawSetting("Selection", "Remember last area", m_rememberSelection, &m_rememberSelection);
        drawSetting("Dim screen", "While recording", m_dimScreen, &m_dimScreen);
        drawSetting("Countdown", "Before recording", m_showCountdown, &m_showCountdown);
    } else if (m_settingsTab == 1) { // Video
        const QRectF card(menuX + 18.0, menuY + 106.0, menuW - 36.0, 256.0);
        const double labelX = card.x() + 14.0;
        const double controlRight = card.right() - 14.0;
        const double row1Y = card.y();
        const double row2Y = row1Y + 76.0;
        const double row3Y = row2Y + 52.0;
        const double row4Y = row3Y + 52.0;

        p.setPen(Qt::NoPen);
        p.setBrush(QColor(255, 255, 255, 10));
        p.drawRoundedRect(card, 10.0, 10.0);
        p.setPen(QPen(QColor(255, 255, 255, 13), 1.0));
        for (double dividerY : {row2Y, row3Y, row4Y})
            p.drawLine(QPointF(card.x() + 14.0, dividerY), QPointF(card.right() - 14.0, dividerY));

        auto drawText = [&](const QString& text, double x, double y, bool bold, int alpha) {
            QFont font(QStringLiteral("Sans"));
            font.setPixelSize(bold ? 13 : 11);
            font.setWeight(bold ? QFont::DemiBold : QFont::Normal);
            p.setFont(font);
            p.setPen(QColor(255, 255, 255, alpha));
            p.drawText(QPointF(x, y), text);
        };
        auto drawRowHover = [&](const QRectF& row, int index) {
            if (m_hoveredSettingsItem != index) return;
            p.setPen(Qt::NoPen);
            p.setBrush(QColor(255, 255, 255, 16));
            p.drawRoundedRect(row, 7.0, 7.0);
        };
        auto drawCheck = [&](const QRectF& cb, bool checked) {
            if (checked) {
                p.setPen(Qt::NoPen); p.setBrush(accentColor); p.drawRoundedRect(cb, 4, 4);
                p.setPen(QPen(Qt::white, 2));
                p.drawLine(QPointF(cb.x() + 4, cb.y() + 9), QPointF(cb.x() + 8, cb.y() + 13));
                p.drawLine(QPointF(cb.x() + 8, cb.y() + 13), QPointF(cb.x() + 14, cb.y() + 5));
            } else {
                p.setPen(QPen(QColor(255, 255, 255, 41), 1.0));
                p.setBrush(QColor(255, 255, 255, 15));
                p.drawRoundedRect(cb, 4, 4);
            }
        };

        const int resIdx = m_settingsClickableRects.size();
        drawRowHover(QRectF(card.x(), row1Y, card.width(), 76.0), resIdx);
        drawText("Maximum resolution", labelX, row1Y + 25.0, true, 230);
        drawText("Reduce file size and upload time", labelX, row1Y + 48.0, false, 140);
        QRectF resBtn(controlRight - 136.0, row1Y + 22.0, 136.0, 30.0);
        p.setPen(Qt::NoPen);
        p.setBrush(QColor(255, 255, 255, 15));
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

        const int fpsIdx = m_settingsClickableRects.size();
        drawRowHover(QRectF(card.x(), row2Y, card.width(), 52.0), fpsIdx);
        drawText("Frame rate", labelX, row2Y + 31.0, true, 230);
        QRectF fpsBtn(controlRight - 76.0, row2Y + 11.0, 76.0, 30.0);
        p.setPen(Qt::NoPen);
        p.setBrush(QColor(255, 255, 255, 15));
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

        const int monoIdx = m_settingsClickableRects.size();
        QRectF monoRow(card.x(), row3Y, card.width(), 52.0);
        drawRowHover(monoRow, monoIdx);
        drawText("Record audio in mono", labelX, row3Y + 31.0, true, 230);
        drawCheck(QRectF(controlRight - 18.0, row3Y + 17.0, 18.0, 18.0), m_recordMono);
        m_settingsClickableRects.append(monoRow);

        const int encoderIdx = m_settingsClickableRects.size();
        QRectF encoderRow(card.x(), row4Y, card.width(), 76.0);
        drawRowHover(encoderRow, encoderIdx);
        drawText("Open video editor", labelX, row4Y + 27.0, true, 230);
        drawText("Edit quality, resolution and audio after recording", labelX, row4Y + 50.0, false, 140);
        drawCheck(QRectF(controlRight - 18.0, row4Y + 29.0, 18.0, 18.0), m_openEditor);
        m_settingsClickableRects.append(encoderRow);
    } else if (m_settingsTab == 2) { // GIF
        const QRectF card(menuX + 18.0, menuY + 106.0, menuW - 36.0, 252.0);
        const double labelX = card.x() + 14.0;
        const double controlRight = card.right() - 14.0;
        const double row1Y = card.y();
        const double row2Y = row1Y + 64.0;
        const double row3Y = row2Y + 72.0;
        const double row4Y = row3Y + 52.0;

        p.setPen(Qt::NoPen);
        p.setBrush(QColor(255, 255, 255, 10));
        p.drawRoundedRect(card, 10.0, 10.0);
        p.setPen(QPen(QColor(255, 255, 255, 13), 1.0));
        for (double dividerY : {row2Y, row3Y, row4Y})
            p.drawLine(QPointF(card.x() + 14.0, dividerY), QPointF(card.right() - 14.0, dividerY));

        auto drawLabel = [&](const QString& text, double y) {
            QFont font(QStringLiteral("Sans")); font.setPixelSize(13); font.setWeight(QFont::DemiBold);
            p.setFont(font); p.setPen(QColor(255, 255, 255, 220));
            p.drawText(QPointF(labelX, y), text);
        };
        auto drawRowHover = [&](const QRectF& row, int index) {
            if (m_hoveredSettingsItem != index) return;
            p.setPen(Qt::NoPen); p.setBrush(QColor(255, 255, 255, 16));
            p.drawRoundedRect(row, 7.0, 7.0);
        };
        auto drawCheck = [&](const QRectF& cb, bool checked) {
            if (checked) {
                p.setPen(Qt::NoPen); p.setBrush(accentColor); p.drawRoundedRect(cb, 4, 4);
                p.setPen(QPen(Qt::white, 2));
                p.drawLine(QPointF(cb.x() + 4, cb.y() + 9), QPointF(cb.x() + 8, cb.y() + 13));
                p.drawLine(QPointF(cb.x() + 8, cb.y() + 13), QPointF(cb.x() + 14, cb.y() + 5));
            } else {
                p.setPen(QPen(QColor(255, 255, 255, 41), 1.0));
                p.setBrush(QColor(255, 255, 255, 15)); p.drawRoundedRect(cb, 4, 4);
            }
        };

        const int fpsIndex = m_settingsClickableRects.size();
        drawRowHover(QRectF(card.x(), row1Y, card.width(), 64.0), fpsIndex);
        drawLabel("Frame rate", row1Y + 38.0);
        QRectF fpsBox(menuX + 140.0, row1Y + 17.0, 45.0, 30.0);
        p.setPen(Qt::NoPen);
        p.setBrush(QColor(255, 255, 255, 15));
        p.drawRoundedRect(fpsBox, 6, 6);
        p.setPen(Qt::white);
        p.setFont(QFont("Sans", 10));
        p.drawText(fpsBox, Qt::AlignCenter, QString::number(m_gifFps));

        const double sliderX = menuX + 200.0;
        const double sliderW = controlRight - sliderX;
        QRectF sliderTrack(sliderX, row1Y + 30.0, sliderW, 4.0);
        m_gifFpsTrackRect = QRectF(sliderX, row1Y + 10.0, sliderW, 44.0);
        p.setPen(Qt::NoPen);
        p.setBrush(QColor(255, 255, 255, 30));
        p.drawRoundedRect(sliderTrack, 2, 2);

        // Progress fill
        double progress = (m_gifFps - 5) / 55.0; // range 5 to 60
        QRectF progressRect(sliderX, sliderTrack.y(), sliderW * progress, 4);
        p.setBrush(accentColor);
        p.drawRoundedRect(progressRect, 2, 2);

        double handleX = sliderX + progress * sliderW;
        QRectF handle(handleX - 7, sliderTrack.center().y() - 7, 14, 14);
        p.setBrush(Qt::white);
        p.drawEllipse(handle);
        m_settingsClickableRects.append(m_gifFpsTrackRect);

        const int qualityIndex = m_settingsClickableRects.size();
        drawRowHover(QRectF(card.x(), row2Y, card.width(), 72.0), qualityIndex);
        drawLabel("Quality", row2Y + 30.0);
        const double qSliderX = menuX + 160.0;
        const double qSliderW = controlRight - qSliderX;
        QRectF qSliderTrack(qSliderX, row2Y + 27.0, qSliderW, 4);
        m_gifQualityTrackRect = QRectF(qSliderX, row2Y + 10.0, qSliderW, 46.0);
        p.setPen(Qt::NoPen);
        p.setBrush(QColor(255, 255, 255, 30));
        p.drawRoundedRect(qSliderTrack, 2, 2);

        p.setBrush(accentColor);
        p.drawRoundedRect(QRectF(qSliderX, qSliderTrack.y(), qSliderW * m_gifQuality, 4.0), 2.0, 2.0);

        // Ticks
        p.setPen(QPen(QColor(255, 255, 255, 60), 1));
        for (int i = 0; i <= 8; ++i) {
            double tx = qSliderX + (qSliderW / 8.0) * i;
            p.drawLine(QPointF(tx, qSliderTrack.y() - 4.0), QPointF(tx, qSliderTrack.y() + 8.0));
        }

        double qHandleX = qSliderX + m_gifQuality * qSliderW;
        QRectF qHandle(qHandleX - 7, qSliderTrack.center().y() - 7, 14, 14);
        p.setPen(Qt::NoPen);
        p.setBrush(Qt::white);
        p.drawEllipse(qHandle);

        p.setFont(QFont("Sans", 8));
        p.setPen(QColor(255, 255, 255, 120));
        p.drawText(QRectF(qSliderX, row2Y + 46.0, 40, 20), Qt::AlignLeft, "Low");
        p.drawText(QRectF(qSliderX + qSliderW - 40, row2Y + 46.0, 40, 20), Qt::AlignRight, "High");
        m_settingsClickableRects.append(m_gifQualityTrackRect);

        const int optimizeIndex = m_settingsClickableRects.size();
        QRectF optimizeRow(card.x(), row3Y, card.width(), 52.0);
        drawRowHover(optimizeRow, optimizeIndex);
        drawLabel("Optimize GIF", row3Y + 32.0);
        drawCheck(QRectF(controlRight - 18.0, row3Y + 17.0, 18.0, 18.0), m_optimizeGif);
        m_settingsClickableRects.append(optimizeRow);

        const int sizeIdx = m_settingsClickableRects.size();
        drawRowHover(QRectF(card.x(), row4Y, card.width(), 64.0), sizeIdx);
        drawLabel("Output size", row4Y + 38.0);
        QRectF sizeBtn(controlRight - 180.0, row4Y + 17.0, 180, 30);
        p.setPen(Qt::NoPen);
        p.setBrush(QColor(255, 255, 255, 15));
        if (m_hoveredSettingsItem == sizeIdx) p.setBrush(QColor(255, 255, 255, 20));
        p.drawRoundedRect(sizeBtn, 6, 6);
        p.setPen(Qt::white);
        const QStringList sizeOptions = {"800 x auto (default)", "640 x auto", "480 x auto", "Original"};
        p.drawText(sizeBtn.adjusted(10, 0, -25, 0), Qt::AlignLeft | Qt::AlignVCenter, sizeOptions[m_gifSizeIdx]);
        // Chevron
        p.setPen(QPen(Qt::white, 1.5));
        p.drawLine(QPointF(sizeBtn.right() - 15, sizeBtn.center().y() - 3), QPointF(sizeBtn.right() - 11, sizeBtn.center().y() + 1));
        p.drawLine(QPointF(sizeBtn.right() - 11, sizeBtn.center().y() + 1), QPointF(sizeBtn.right() - 7, sizeBtn.center().y() - 3));
        m_settingsClickableRects.append(sizeBtn);
    }

    if (m_dropdownOpen != -1) {
        drawDropdownPopup(p, m_dropdownAnchor, m_dropdownOptions,
                          m_dropdownValuePtr ? *m_dropdownValuePtr : -1);
    }
}
