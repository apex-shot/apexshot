#pragma once

#include <QElapsedTimer>
#include <QLabel>
#include <QPushButton>
#include <QTimer>
#include <QWidget>

class RecordingControlsWindow : public QWidget
{
public:
    RecordingControlsWindow(const QString& dbusDest,
                            const QString& sessionId,
                            const QRect& captureRect,
                            bool isFullscreen,
                            bool showTimer,
                            QWidget* parent = nullptr);

protected:
    void showEvent(QShowEvent* event) override;

private:
    void setupUi();
    void positionWindow();
    bool sendCommand(const QString& methodName);
    void setPaused(bool paused);
    void resetTimer();
    void updateTimerText();
    QString formatElapsed(qint64 elapsedMs) const;

    QString m_dbusDest;
    QString m_sessionId;
    QRect m_captureRect;
    bool m_isFullscreen;
    bool m_showTimer;
    bool m_paused = false;
    qint64 m_elapsedBeforePauseMs = 0;
    bool m_positioned = false;

    QElapsedTimer m_elapsedTimer;
    QTimer* m_uiTimer = nullptr;
    QLabel* m_timerLabel = nullptr;
    QPushButton* m_pauseButton = nullptr;
    QPushButton* m_restartButton = nullptr;
    QPushButton* m_deleteButton = nullptr;
    QPushButton* m_menuButton = nullptr;
};
