#pragma once

#include <QRect>
#include <QWidget>

class QShowEvent;

/// Dims everything outside a recording's capture rectangle.
///
/// Shown for area and window recordings so the recorded region stays clear
/// while the surrounding desktop is muted. The window is click-through and
/// excluded from capture, so it neither blocks the recorded app nor appears in
/// the video.
class RecordingMask : public QWidget
{
public:
    explicit RecordingMask(const QRect& deviceRegion, QWidget* parent = nullptr);

protected:
    void paintEvent(QPaintEvent* event) override;
    void showEvent(QShowEvent* event) override;

private:
    QRect m_hole;
};
