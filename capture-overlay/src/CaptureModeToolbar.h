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
        bool scroll = false;
    };

    // When a display was chosen before opening the toolbar, keep the toolbar
    // and every resulting capture action scoped to that display.
    //
    // `recordingAvailable` is false on platforms that refuse video recording
    // (Fedora). The Video item then stays visible but disabled and says why,
    // rather than accepting a click the recorder will reject.
    //
    // `allowAreaRecording` lets a host that has no recording panel of its own
    // record the Area item. The Linux overlay keeps the default: its recording
    // panel selects the region, so the toolbar's Area slot stays a screenshot
    // action there.
    // `enableScrollCapture` adds a Scroll mode that combines with Window or
    // Area. It is opt-in so the Linux overlay keeps its existing toolbar.
    static Result choose(QLocalServer* controlServer = nullptr,
                         QScreen* targetScreen = nullptr,
                         bool recordingAvailable = true,
                         bool allowAreaRecording = false,
                         bool enableScrollCapture = false);

protected:
    void paintEvent(QPaintEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void leaveEvent(QEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;

private:
    explicit CaptureModeToolbar(QScreen* screen, bool screenIsLocked = false,
                                bool recordingAvailable = true,
                                bool allowAreaRecording = false,
                                bool enableScrollCapture = false);

    int itemCount() const;
    int itemWidth(int index) const;
    QString labelFor(int index) const;
    QRectF itemRect(int index) const;
    int hitTest(const QPoint& point) const;
    void focusAndRaise();
    void finish(Action action);
    void drawIcon(QPainter& painter, int index, const QPointF& center, const QColor& color) const;

    QScreen* m_screen;
    bool m_screenIsLocked = false;
    bool m_recordingAvailable = true;
    bool m_allowAreaRecording = false;
    bool m_enableScrollCapture = false;
    Action m_action = Action::Cancel;
    int m_hovered = -1;
    bool m_finished = false;
    bool m_ocr = false;
    int m_timerSeconds = 0;
    bool m_recording = false;
    bool m_microphone = false;
    bool m_speaker = false;
    bool m_scroll = false;
};
