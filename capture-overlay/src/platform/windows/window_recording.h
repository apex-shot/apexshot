#pragma once

#include "platform/platform.h"

#include <QPoint>
#include <QRectF>
#include <QSize>
#include <QString>
#include <QVector>
#include <QWidget>

#include <functional>

class QTimer;
class QWheelEvent;

namespace Platform {

/// Full-screen picker for a window action: record, capture, or scroll.
///
/// The desktop is dimmed and the window under the pointer is washed with the
/// accent colour: no outline, no title chip, so the window itself stays the
/// subject. Tab, the arrow keys, or the wheel walk the window stack and raise
/// each selection so a covered window can be reached; clicking hands it back
/// without starting anything.
class WindowPicker : public QWidget
{
    Q_OBJECT

public:
    using Picked = std::function<void(const WindowInfo&)>;

    WindowPicker(Picked onPicked,
                 std::function<void()> onCancelled,
                 QString action = QStringLiteral("record"));
    ~WindowPicker() override;

    /// Cancel as Escape or a right click would.
    void dismiss();

protected:
    void paintEvent(QPaintEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;
    void wheelEvent(QWheelEvent* event) override;
    void showEvent(QShowEvent* event) override;

private:
    void refreshWindows();
    int windowAt(const QPointF& localPoint) const;
    int indexOfSelected() const;
    void updateHover(const QPoint& localPoint);
    void cycleSelection(int delta);
    void finish(bool picked);

    Picked m_onPicked;
    std::function<void()> m_onCancelled;
    QString m_action;
    QVector<WindowInfo> m_windows;
    QVector<QRectF> m_frames;
    /// Set while Tab or the wheel is walking the stack, so the hover refresh
    /// does not pull the selection back to whatever is under the pointer.
    bool m_cycled = false;
    quintptr m_selectedId = 0;
    /// The stack order captured when cycling began; raising each selection
    /// reorders the live list, so the walk needs its own stable copy.
    QVector<quintptr> m_cycleOrder;
    QTimer* m_refresh = nullptr;
    int m_hovered = -1;
    QPoint m_cursor;
    bool m_mouseMoved = false;
    bool m_done = false;
};

/// Paints the selected window with the accent colour and its two controls:
/// the window size with a Resize menu above, and Start recording. A corner
/// grip also resizes the window. Devices are chosen on the capture toolbar.
class RecordingSetupOverlay : public QWidget
{
    Q_OBJECT

public:
    using Started = std::function<void()>;

    RecordingSetupOverlay(const WindowInfo& window,
                          Started onStarted,
                          std::function<void()> onCancelled);
    ~RecordingSetupOverlay() override;

    /// Record with the device choices the caller already holds, as the Start
    /// button and the Return key do.
    void startNow();

    /// Cancel as Escape or a right click would.
    void cancelNow();

protected:
    void paintEvent(QPaintEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;
    void showEvent(QShowEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;

private:
    void refreshWindow();
    double controlWidth() const;
    QRectF startRect() const;
    QRectF headerRect() const;
    QRectF sizeRect() const;
    QRectF resizeRect() const;
    QRectF gripRect() const;
    QString sizeText() const;
    bool fitsOnScreen(const QSize& size) const;
    void openResizeMenu();
    void applyWindowSize(const QSize& size);
    void promptCustomSize();
    void saveCurrentSize();
    void beginResize(const QPoint& globalPos);
    void resizeTo(const QPoint& globalPos);
    void beginMove(const QPoint& globalPos);
    void moveTo(const QPoint& globalPos);
    QRect frameOnScreen(const QRect& frame, const QPoint& globalHint) const;
    void finish();
    void cancel();

    WindowInfo m_window;
    Started m_onStarted;
    std::function<void()> m_onCancelled;
    QTimer* m_refresh = nullptr;
    QRectF m_hole;
    QPoint m_resizePress;
    QSize m_resizeStartSize;
    QRect m_resizeStartFrame;
    bool m_resizing = false;
    bool m_moving = false;
    QPoint m_movePress;
    QRect m_moveStartFrame;
    bool m_hoverStart = false;
    bool m_hoverResize = false;
    bool m_hoverGrip = false;
    bool m_hoverWindow = false;
    bool m_done = false;
};

}  // namespace Platform
