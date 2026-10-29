#include "CaptureModeToolbar.h"
#include "platform/windows/audio_devices.h"

#include <QApplication>
#include <QFont>
#include <QFontMetrics>
#include <QGuiApplication>
#include <QScreen>

#include <cstdio>

class CaptureModeToolbarTests
{
public:
    static bool run()
    {
        CaptureModeToolbar::DeviceDefaults defaults;
        CaptureModeToolbar toolbar(QGuiApplication::primaryScreen(), false, true, true, true,
                                   defaults);
        toolbar.m_recording = true;
        toolbar.m_microphone = true;
        toolbar.m_microphoneDevice = QStringLiteral("Studio microphone");
        toolbar.m_speaker = true;
        toolbar.m_outputDevice = QStringLiteral("Desktop speakers");
        toolbar.m_camera = true;
        toolbar.m_cameraDevice = QStringLiteral("USB camera");
        toolbar.resizeForMode();

        if (toolbar.labelFor(5) != QStringLiteral("Studio microphone")
            || toolbar.labelFor(6) != QStringLiteral("Desktop speakers")
            || toolbar.labelFor(7) != QStringLiteral("USB camera"))
            return false;
        toolbar.m_systemAudioApplication = QStringLiteral("Music player");
        toolbar.m_systemAudioProcessId = 42;
        if (toolbar.labelFor(6) != QStringLiteral("Music player"))
            return false;

        const int shortWidth = toolbar.preferredWidth();
        toolbar.m_microphoneDevice = QString(200, QLatin1Char('M'));
        QFont font(QStringLiteral("Inter"));
        font.setPixelSize(13);
        font.setWeight(QFont::DemiBold);
        const QString displayed = toolbar.displayedLabel(5, font);
        if (!displayed.endsWith(QChar(0x2026))
            || QFontMetrics(font).horizontalAdvance(displayed)
                > toolbar.itemWidth(5) - 12 * 2 - 17 - 8
            || toolbar.preferredWidth() != shortWidth)
            return false;

        toolbar.m_microphone = false;
        toolbar.m_speaker = false;
        toolbar.m_camera = false;
        if (toolbar.labelFor(5) != QStringLiteral("No microphone")
            || toolbar.labelFor(6) != QStringLiteral("No system audio")
            || toolbar.labelFor(7) != QStringLiteral("No camera"))
            return false;
        const CaptureModeToolbar::DeviceDefaults state = toolbar.deviceState();
        if (state.microphone || state.speaker || state.camera)
            return false;

        const QStringList devices{QStringLiteral("Headset"), QStringLiteral("Microphone array")};
        if (Platform::resolveMicrophoneDevice(devices, QStringLiteral("headset"), {})
                != QStringLiteral("Headset")
            || Platform::resolveMicrophoneDevice(devices, QStringLiteral("missing"),
                                                 QStringLiteral("Headset"))
                != QStringLiteral("Headset")
            || Platform::resolveMicrophoneDevice(devices, {}, {})
                != QStringLiteral("Microphone array")
            || !Platform::resolveMicrophoneDevice({}, QStringLiteral("missing"), {}).isEmpty())
            return false;

        Platform::AudioLevelMonitor monitor;
        monitor.startMicrophone(QStringLiteral("ApexShot nonexistent test device"));
        monitor.stop();
        return monitor.peak() == 0.0f;
    }
};

int main(int argc, char** argv)
{
    QApplication app(argc, argv);
    if (!CaptureModeToolbarTests::run()) {
        std::fprintf(stderr, "Capture toolbar device state test failed\n");
        return 1;
    }
    return 0;
}
