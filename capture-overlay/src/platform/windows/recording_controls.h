#pragma once

#include "platform/recording.h"

#include <QRect>
#include <QWidget>

class QCloseEvent;
class QShowEvent;

/// Floating recording pill for the Quick Access shell.
///
/// Shows elapsed time and offers pause/resume, stop/save, and discard. It
/// never owns the session: closing the pill saves an active recording.
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
    enum class Button { None, Body, Pause, Stop, Discard };

    Button hitTest(const QPoint& point) const;
    QRectF pauseRect() const;
    QRectF stopRect() const;
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
