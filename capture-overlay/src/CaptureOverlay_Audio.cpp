#include "CaptureOverlay.h"
#include "CaptureOverlay_DrawingPrimitives_p.h"

#include <QPainter>
#include <QPainterPath>
#include <QPen>
#include <QProcess>
#include <QRegularExpression>
#include <QtGlobal>

#include <algorithm>

namespace {

void drawVolumeIcon(QPainter& p, double cx, double cy, QColor color, bool microphone)
{
    p.save();
    p.setPen(QPen(color, 1.6, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
    p.setBrush(Qt::NoBrush);

    if (microphone) {
        QPainterPath capsule;
        roundedRectPath(capsule, cx - 3.1, cy - 7.0, 6.2, 9.6, 3.1);
        p.drawPath(capsule);

        p.drawLine(QPointF(cx - 5.0, cy - 0.3), QPointF(cx - 5.0, cy + 1.6));
        p.drawLine(QPointF(cx + 5.0, cy - 0.3), QPointF(cx + 5.0, cy + 1.6));
        p.drawArc(QRectF(cx - 5.0, cy - 1.5, 10.0, 8.4), 180 * 16, 180 * 16);
        p.drawLine(QPointF(cx, cy + 6.1), QPointF(cx, cy + 8.3));
        p.drawLine(QPointF(cx - 3.4, cy + 8.3), QPointF(cx + 3.4, cy + 8.3));
    } else {
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
    }

    p.restore();
}

} // namespace

void CaptureOverlay::runPactlVolume(const QString& type, int pct)
{
    QStringList args;
    if (type == "mic") {
        args << "set-source-volume" << "@DEFAULT_SOURCE@" << QString("%1%").arg(pct);
    } else {
        args << "set-sink-volume" << "@DEFAULT_SINK@" << QString("%1%").arg(pct);
    }
    if (!QProcess::startDetached(QStringLiteral("pactl"), args)) {
        const QString device = type == QStringLiteral("mic")
            ? QStringLiteral("@DEFAULT_AUDIO_SOURCE@")
            : QStringLiteral("@DEFAULT_AUDIO_SINK@");
        QProcess::startDetached(
            QStringLiteral("wpctl"),
            {QStringLiteral("set-volume"), device, QStringLiteral("%1%").arg(pct)});
    }
}

double CaptureOverlay::readPactlVolume(const QString& type, double fallback)
{
    const QStringList args = type == QStringLiteral("mic")
        ? QStringList{QStringLiteral("get-source-volume"), QStringLiteral("@DEFAULT_SOURCE@")}
        : QStringList{QStringLiteral("get-sink-volume"), QStringLiteral("@DEFAULT_SINK@")};

    QString output;
    {
        QProcess process;
        process.start(QStringLiteral("pactl"), args);
        if (process.waitForStarted(250) && process.waitForFinished(500)
            && process.exitStatus() == QProcess::NormalExit && process.exitCode() == 0) {
            output = QString::fromUtf8(process.readAllStandardOutput());
        }
    }

    if (!output.isEmpty()) {
        const QRegularExpressionMatch match =
            QRegularExpression(QStringLiteral("(\\d+(?:\\.\\d+)?)%"))
                .match(output);
        if (match.hasMatch()) {
            bool ok = false;
            const double percent = match.captured(1).toDouble(&ok);
            if (ok) {
                return std::clamp(percent / 100.0, 0.0, 1.0);
            }
        }
    }

    const QString device = type == QStringLiteral("mic")
        ? QStringLiteral("@DEFAULT_AUDIO_SOURCE@")
        : QStringLiteral("@DEFAULT_AUDIO_SINK@");
    QProcess process;
    process.start(QStringLiteral("wpctl"), {QStringLiteral("get-volume"), device});
    if (!process.waitForStarted(250) || !process.waitForFinished(500)
        || process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0) {
        return fallback;
    }
    output = QString::fromUtf8(process.readAllStandardOutput());
    const QRegularExpressionMatch match =
        QRegularExpression(QStringLiteral("Volume:\\s*(\\d+(?:\\.\\d+)?)"))
            .match(output);
    bool ok = false;
    const double volume = match.hasMatch() ? match.captured(1).toDouble(&ok) : fallback;
    return ok ? std::clamp(volume, 0.0, 1.0) : fallback;
}

void CaptureOverlay::drawVolumePopup(QPainter& p,
                                     double panelX, double panelY,
                                     double volume,
                                     bool microphone,
                                     bool isOpen)
{
    if (!isOpen) return;

    const double menuW = 64.0;
    const double menuH = 184.0;
    const double scrW = width();
    const double scrH = height();
    // panelX/panelY are pre-computed top-centre-of-selection positions,
    // already clamped to screen bounds by the caller. Apply the same
    // bounds clamping as a safety net.
    const double menuX = qBound(10.0, panelX, scrW - menuW - 10.0);
    const double menuY = qBound(10.0, panelY, scrH - menuH - 10.0);

    const double radius = menuW / 2.0;
    const double filledH = qBound(0.0, volume, 1.0) * menuH;
    QPainterPath pill;
    roundedRectPath(pill, menuX, menuY, menuW, menuH, radius);
    p.fillPath(pill, QColor(20, 20, 20));

    p.save();
    p.setClipPath(pill);
    p.fillRect(QRectF(menuX, menuY + menuH - filledH, menuW, filledH), QColor(176, 92, 56));
    p.restore();

    p.setPen(QPen(QColor(255, 255, 255, m_volumeSliderDragging ? 41 : 26), 1.0));
    p.setBrush(Qt::NoBrush);
    p.drawPath(pill);

    p.save();
    p.translate(menuX + menuW / 2.0, menuY + menuH / 2.0);
    p.scale(1.3, 1.3);
    drawVolumeIcon(p, 0.0, 0.0, QColor(241, 241, 243), microphone);
    p.restore();

    // Cache layout rects for hit testing
    m_volumePopupRect = QRectF(menuX, menuY, menuW, menuH);
    m_volumeSliderRect = m_volumePopupRect;
    m_volumeHandleRect = QRectF();
}
