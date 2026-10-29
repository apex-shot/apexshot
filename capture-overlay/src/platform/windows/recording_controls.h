#pragma once

#include "platform/recording.h"

#include <QRect>
#include <QRectF>
#include <QWidget>

class QCloseEvent;
class QShowEvent;

/// Floating recording bar for the Quick Access shell.
///
/// A compact pill with a drag handle, timer, pause/resume, restart, stop and
/// discard. It never owns the session; closing the bar saves a recording that
/// is still running.
class RecordingControls : public QWidget
{
    Q_OBJECT

public:
    RecordingControls(Platform::RecordingSession* session,
                      const QRect& captureRegion,
                      QWidget* parent = nullptr);

signals:
    /// The person asked to throw the current take away and start again.
    void restartRequested();

protected:
    void paintEvent(QPaintEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void leaveEvent(QEvent* event) override;
    void showEvent(QShowEvent* event) override;
    void closeEvent(QCloseEvent* event) override;

private:
    enum class Button { None, Body, Stop, Pause, Restart, Discard };

    Button hitTest(const QPoint& point) const;
    QRectF buttonRect(int index) const;
    QString elapsedText() const;
    int preferredWidth() const;
    void updateWidth();
    void positionOnScreen();

    Platform::RecordingSession* m_session = nullptr;
    QRect m_captureRegion;
    qint64 m_elapsedMs = 0;
    /// The widest timer string shown so far; the bar only grows, never
    /// jitters as the digits change.
    int m_textWidth = 0;
    Button m_hovered = Button::None;
    bool m_dragging = false;
    QPoint m_dragOffset;
    bool m_finished = false;
    bool m_closing = false;
};
