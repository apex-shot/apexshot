#include "CaptureOverlay_DrawingPrimitives_p.h"

#include <QFont>
#include <QFontMetrics>
#include <QImage>
#include <QPainter>
#include <QPainterPath>
#include <QPen>
#include <QRectF>
#include <QString>

#include <algorithm>
#include <cmath>

void roundedRectPath(QPainterPath& path, double x, double y,
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


// Draw frosted glass panel (mirrors draw_frosted_panel in overlay.rs)
void drawFrostedPanel(QPainter& p, double x, double y,
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
void drawToolbarIcon(QPainter& p, int iconIndex,
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
