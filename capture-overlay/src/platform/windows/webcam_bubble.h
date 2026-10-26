#pragma once

#include "platform/windows/webcam_layout.h"

#include <QImage>
#include <QRect>
#include <QWidget>

class QPainterPath;

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

protected:
    void paintEvent(QPaintEvent* event) override;
    void showEvent(QShowEvent* event) override;

private:
    [[nodiscard]] QPainterPath shapePath(const QRectF& rect) const;

    QImage m_frame;
    int m_shape = 0;
    bool m_flip = false;
};

} // namespace Platform
