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
/// Matches the recording bar: an inset status/frame-count segment, then a
/// Done action and a discard button. Done stops scrolling and stitches what
/// has been captured so far.
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
    enum class Button { None, Body, Done, Discard };

    Button hitTest(const QPoint& point) const;
    QRectF segmentRect() const;
    QRectF doneRect() const;
    QRectF discardRect() const;
    void positionOnScreen();

    Platform::ScrollCaptureSession* m_session = nullptr;
    QRect m_captureRegion;
    QString m_status;
    int m_frames = 0;
    Button m_hovered = Button::None;
    bool m_dragging = false;
    QPoint m_dragOffset;
    bool m_finished = false;
    bool m_closing = false;
};
