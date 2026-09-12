#pragma once

#include <QColor>

class QImage;
class QPainter;
class QPainterPath;

void roundedRectPath(QPainterPath& path, double x, double y,
                     double w, double h, double r);
void drawFrostedPanel(QPainter& p, double x, double y,
                      double w, double h, double radius,
                      const QImage* blurredBg,
                      double screenW, double screenH);
void drawToolbarIcon(QPainter& p, int iconIndex,
                     double cx, double cy,
                     QColor color);
