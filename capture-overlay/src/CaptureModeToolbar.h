// SPDX-License-Identifier: GPL-3.0-or-later

#pragma once

#include <QColor>
#include <QRectF>
#include <QString>
#include <QWidget>

#include <functional>

#ifdef Q_OS_WIN
#include "platform/windows/audio_devices.h"
#endif

class QCloseEvent;
class QFont;
class QPainter;
class QLocalServer;
class QScreen;
class QTimer;

class CaptureModeToolbar : public QWidget
{
public:
    enum class Action {
        Cancel,
        Display,
        Window,
        Area,
    };

    /// Device state carried into the toolbar, so its Mic, Speaker, and Webcam
    /// items start where the last recording left them.
    struct DeviceDefaults {
        bool microphone = false;
        QString microphoneDevice;
        bool speaker = false;
        QString systemAudioApplication;
        quint32 systemAudioProcessId = 0;
        bool camera = false;
        QString cameraDevice;
    };

    struct Result {
        Action action = Action::Cancel;
        QScreen* screen = nullptr;
        bool ocr = false;
        int timerSeconds = 0;
        bool recording = false;
        bool microphone = false;
        QString microphoneDevice;
        bool speaker = false;
        QString systemAudioApplication;
        quint32 systemAudioProcessId = 0;
        bool camera = false;
        QString cameraDevice;
        bool scroll = false;
        /// Set only when a window recording is being set up: the toolbar
        /// stays open over the picker and the selected window so devices can
        /// still be changed until Start. The caller owns it.
        CaptureModeToolbar* toolbar = nullptr;
    };

    // When a display was chosen before opening the toolbar, keep the toolbar
    // and every resulting capture action scoped to that display.
    //
    // `recordingAvailable` is false on platforms that refuse video recording
    // (Fedora). The Video item then stays visible but disabled and says why,
    // rather than accepting a click the recorder will reject.
    //
    // `allowAreaRecording` lets a host that has no recording panel of its own
    // record the Area item. The Linux overlay keeps the default: its recording
    // panel selects the region, so the toolbar's Area slot stays a screenshot
    // action there.
    // `enableScrollCapture` adds a Scroll mode that combines with Window or
    // Area. It is opt-in so the Linux overlay keeps its existing toolbar. A
    // host that enables it also gets a Webcam item while recording, which
    // takes the Scroll slot, so window recordings can turn the camera on
    // before they start.
    static Result choose(QLocalServer* controlServer = nullptr,
                         QScreen* targetScreen = nullptr,
                         bool recordingAvailable = true,
                         bool allowAreaRecording = false,
                         bool enableScrollCapture = false,
                         bool persistForWindowSetup = false);

    /// Same, carrying the device state a previous recording left behind.
    /// Kept as an overload rather than a `{}` default argument: GCC will not
    /// convert a brace initialiser to a type nested in the class that is
    /// still being defined.
    static Result choose(QLocalServer* controlServer,
                         QScreen* targetScreen,
                         bool recordingAvailable,
                         bool allowAreaRecording,
                         bool enableScrollCapture,
                         const DeviceDefaults& defaults,
                         bool persistForWindowSetup = false,
                         std::function<void(bool, const QString&)> cameraChanged = {});

    /// Current device state, read when a persisted toolbar's window setup
    /// starts recording.
    DeviceDefaults deviceState() const;

    /// Called when the Close item is clicked while the toolbar is persisting
    /// over a window setup. The handler dismisses the picker or the selected
    /// window; the toolbar closes itself after it returns.
    void setCloseHandler(std::function<void()> handler);

protected:
    void paintEvent(QPaintEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void leaveEvent(QEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;
    void closeEvent(QCloseEvent* event) override;

private:
    friend class CaptureModeToolbarTests;
    explicit CaptureModeToolbar(QScreen* screen, bool screenIsLocked,
                                bool recordingAvailable,
                                bool allowAreaRecording,
                                bool enableScrollCapture,
                                const DeviceDefaults& defaults);

    int itemCount() const;
    int itemWidth(int index) const;
    int preferredWidth() const;
    QString labelFor(int index) const;
    QString displayedLabel(int index, const QFont& font) const;
    QRectF itemRect(int index) const;
    bool separatorAfter(int index) const;
    int hitTest(const QPoint& point) const;
    bool cameraSlot(int index) const;
    void focusAndRaise();
    void finish(Action action);
    void drawIcon(QPainter& painter, int index, const QPointF& center, const QColor& color) const;
    void drawFallbackIcon(QPainter& painter, int index, const QPointF& center,
                          const QColor& color) const;
    QPoint menuAnchor(int index) const;
    void openDeviceMenu(int index);
    void toggleCamera();
    void loadCameraDefaults();
    void notifyCameraChanged();
    void enterSetupMode();
    void closeSetup();
    void resizeForMode();
#ifdef Q_OS_WIN
    void refreshDevices();
    void updateAudioMonitoring();
#endif

    QScreen* m_screen;
    bool m_screenIsLocked = false;
    bool m_recordingAvailable = true;
    bool m_allowAreaRecording = false;
    bool m_enableScrollCapture = false;
    Action m_action = Action::Cancel;
    int m_hovered = -1;
    bool m_finished = false;
    bool m_ocr = false;
    int m_timerSeconds = 0;
    bool m_recording = false;
    bool m_microphone = false;
    QString m_microphoneDevice;
    bool m_speaker = false;
    QString m_systemAudioApplication;
    quint32 m_systemAudioProcessId = 0;
    bool m_camera = false;
    QString m_cameraDevice;
    bool m_scroll = false;
    bool m_setupMode = false;
    std::function<void()> m_onClose;
    std::function<void(bool, const QString&)> m_cameraChanged;
#ifdef Q_OS_WIN
    QString m_outputDevice;
    int m_deviceItemWidth = 160;
    Platform::AudioLevelMonitor m_microphoneMonitor;
    Platform::AudioLevelMonitor m_systemAudioMonitor;
    QString m_monitoredMicrophone;
    QString m_monitoredOutput;
    quint32 m_monitoredProcessId = 0;
    bool m_monitoringMicrophone = false;
    bool m_monitoringSystemAudio = false;
    QTimer* m_meterTimer = nullptr;
#endif
};
