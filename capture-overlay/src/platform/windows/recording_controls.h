#pragma once

#include "platform/recording.h"

#include <QRect>
#include <QRectF>
#include <QWidget>

class QCloseEvent;
class QShowEvent;

/// Floating recording bar for the Quick Access shell.
///
/// Follows the Linux recording controls: an inset stop/timer segment, then
/// quiet pause and discard buttons separated by hairline rules. It never owns
/// the session; closing the bar saves a recording that is still running.
class RecordingControls : public QWidget
{
    Q_OBJECT

public:
    RecordingControls(Platform::RecordingSession* session,
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
    enum class Button { None, Body, Stop, Pause, Discard };

    Button hitTest(const QPoint& point) const;
    QRectF stopRect() const;
    QRectF pauseRect() const;
    QRectF discardRect() const;
    QString elapsedText() const;
    void positionOnScreen();

    Platform::RecordingSession* m_session = nullptr;
    QRect m_captureRegion;
    qint64 m_elapsedMs = 0;
    Button m_hovered = Button::None;
    bool m_dragging = false;
    QPoint m_dragOffset;
    bool m_finished = false;
    bool m_closing = false;
};
