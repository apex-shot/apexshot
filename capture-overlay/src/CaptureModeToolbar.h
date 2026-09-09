// SPDX-License-Identifier: GPL-3.0-or-later

#pragma once

#include <QColor>
#include <QRectF>
#include <QWidget>

class QPainter;
class QLocalServer;
class QScreen;

class CaptureModeToolbar : public QWidget
{
public:
    enum class Action {
        Cancel,
        Display,
        Window,
        Area,
    };

    struct Result {
        Action action = Action::Cancel;
        QScreen* screen = nullptr;
        bool ocr = false;
        int timerSeconds = 0;
        bool recording = false;
        bool microphone = false;
        bool speaker = false;
    };

    static Result choose(QLocalServer* controlServer = nullptr);

protected:
    void paintEvent(QPaintEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void leaveEvent(QEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;

private:
    explicit CaptureModeToolbar(QScreen* screen);

    QRectF itemRect(int index) const;
    int hitTest(const QPoint& point) const;
    void focusAndRaise();
    void finish(Action action);
    void drawIcon(QPainter& painter, int index, const QPointF& center, const QColor& color) const;

    QScreen* m_screen;
    Action m_action = Action::Cancel;
    int m_hovered = -1;
    bool m_finished = false;
    bool m_ocr = false;
    int m_timerSeconds = 0;
    bool m_recording = false;
    bool m_microphone = false;
    bool m_speaker = false;
};
