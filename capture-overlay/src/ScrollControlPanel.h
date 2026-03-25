// ScrollControlPanel.h
#pragma once

#include <QWidget>
#include <QPushButton>
#include <QLabel>

class QTimer;

// Small floating control panel shown during scroll capture.
class ScrollControlPanel : public QWidget
{
    Q_OBJECT

public:
    explicit ScrollControlPanel(QWidget* parent = nullptr);

    void setFrameCount(int count);
    void setStatus(const QString& text);
    void setCapturingDone();
    void positionNear(const QRect& captureArea, const QSize& screenSize);

protected:
    void paintEvent(QPaintEvent* event) override;

signals:
    void cancelClicked();
    void doneClicked();

private:
    QLabel*      m_statusLabel;
    QLabel*      m_frameLabel;
    QPushButton* m_cancelBtn;
    QPushButton* m_doneBtn;
};
