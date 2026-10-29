#pragma once

#include <QRect>
#include <QRectF>
#include <QString>
#include <QWidget>

class QCloseEvent;
class QShowEvent;

namespace Platform {
class ScrollCaptureSession;
}

/// Floating control bar for a scrolling capture.
///
/// Matches the recording pill: drag dots, an accent dot with the frame count,
/// then a Finish action and a discard button. The capture saves automatically
/// when the scroll reaches the end; Finish stops early and stitches the
/// portion captured so far.
class ScrollControls : public QWidget
{
    Q_OBJECT

public:
    ScrollControls(Platform::ScrollCaptureSession* session,
                   const QRect& captureRegion,
                   QWidget* parent = nullptr);

protected:
    void paintEvent(QPaintEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void leaveEvent(QEvent* event) override;
    void showEvent(QShowEvent* event) override;
    void closeEvent(QCloseEvent* event) override;

private:
    enum class Button { None, Body, Finish, Discard };

    Button hitTest(const QPoint& point) const;
    QRectF finishRect() const;
    QRectF discardRect() const;
    QString statusText() const;
    int preferredWidth() const;
    void updateWidth();
    void positionOnScreen();

    Platform::ScrollCaptureSession* m_session = nullptr;
    QRect m_captureRegion;
    int m_frames = 0;
    /// The widest status string shown so far; the bar only grows, never
    /// jitters as the frame count changes.
    int m_textWidth = 0;
    Button m_hovered = Button::None;
    bool m_dragging = false;
    QPoint m_dragOffset;
    bool m_finished = false;
    bool m_closing = false;
};
