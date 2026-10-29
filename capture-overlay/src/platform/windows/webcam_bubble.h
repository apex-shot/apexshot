#pragma once

#include "platform/windows/webcam_layout.h"

#include <QImage>
#include <QRect>
#include <QWidget>

#include <functional>

class QPainterPath;
class QScreen;

namespace Platform {

/// The live camera bubble drawn while a recording runs.
///
/// The recorder writes the camera to its own track and the editor composites
/// it, so this window is a preview of that layer and nothing more: it is
/// excluded from capture and never becomes part of the recording's pixels.
class WebcamBubble : public QWidget
{
    Q_OBJECT

public:
    WebcamBubble(int shape, bool flip, QWidget* parent = nullptr);
    ~WebcamBubble() override;

    /// Show one frame from the recording's camera track.
    void setFrame(const QImage& frame);

    /// Whether the camera has delivered anything yet. A camera that never
    /// answers must not leave an empty bubble in the recording.
    bool hasFrame() const { return !m_frame.isNull(); }

    /// Place the window over `deviceRect`, which is in the desktop's device
    /// pixels like the capture region.
    void placeAt(const QRect& deviceRect);

    /// Let the person drag the bubble around inside the recorded region.
    /// `deviceRegion` is in device pixels, like `placeAt`.
    void setMoveBounds(const QRect& deviceRegion);

    /// Called with the bubble's final device-pixel rectangle after a drag.
    void setMovedHandler(std::function<void(const QRect&)> handler);

    /// Enable the pre-recording resize handles and camera controls.
    void setSetupMode(bool enabled);
    int shape() const { return m_shape; }
    bool flipped() const { return m_flip; }
    QRect deviceGeometry() const { return deviceRectFor(geometry()); }
    void setPreviewError(const QString& error);

protected:
    void paintEvent(QPaintEvent* event) override;
    void showEvent(QShowEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void leaveEvent(QEvent* event) override;

private:
    [[nodiscard]] QPainterPath shapePath(const QRectF& rect) const;
    [[nodiscard]] QScreen* screenForDevicePoint(const QPoint& point) const;
    [[nodiscard]] QRect logicalRectFor(const QRect& deviceRect) const;
    [[nodiscard]] QRect deviceRectFor(const QRect& logicalRect) const;
    [[nodiscard]] QRectF controlBarRect() const;
    [[nodiscard]] int resizeHandleAt(const QPoint& point) const;

    QImage m_frame;
    int m_shape = 0;
    bool m_flip = false;
    QRect m_bounds;
    QPoint m_dragOffset;
    bool m_dragging = false;
    bool m_setupMode = false;
    int m_hoveredControl = -1;
    int m_resizeHandle = -1;
    QRect m_resizeStart;
    QPoint m_resizeOrigin;
    QString m_previewError;
    std::function<void(const QRect&)> m_moved;
};

} // namespace Platform
