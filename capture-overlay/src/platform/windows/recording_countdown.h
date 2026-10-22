#pragma once

#include <QRect>
#include <QWidget>

#include <functional>

class QShowEvent;
class QTimer;

/// Large centred countdown shown before a recording starts.
///
/// Mirrors the recording countdown on the Linux overlay: a dark circle with
/// the remaining seconds, an accent progress ring, and a click-to-cancel
/// state. The screenshot delay keeps its small top pill.
class RecordingCountdown : public QWidget
{
public:
    RecordingCountdown(int seconds,
                       const QRect& captureRegion,
                       std::function<void()> onFinished,
                       QWidget* parent = nullptr);

protected:
    void paintEvent(QPaintEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void enterEvent(QEvent* event) override;
    void leaveEvent(QEvent* event) override;
    void showEvent(QShowEvent* event) override;

private:
    void positionOnScreen();

    int m_remaining = 3;
    int m_total = 3;
    bool m_hovered = false;
    QRect m_captureRegion;
    std::function<void()> m_onFinished;
    QTimer* m_timer = nullptr;
};
