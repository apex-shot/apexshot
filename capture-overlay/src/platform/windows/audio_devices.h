#pragma once

#include <QString>
#include <QStringList>
#include <QVector>

#include <memory>

/// Audio devices and app sessions for the Windows recording surfaces.
///
/// ffmpeg's dshow input names capture devices by their DirectShow friendly
/// name, so the pickers enumerate the same DirectShow categories and hand the
/// recorder a name it can open directly.
namespace Platform {

struct AudioApplication {
    /// Friendly name, e.g. "Google Chrome".
    QString name;
    /// Executable path, when Windows lets the session's process be inspected.
    QString executable;
    /// Process whose tree a per-app capture follows.
    quint32 processId = 0;
    /// Whether the session is playing right now; silent sessions are kept so
    /// an app can be chosen before it starts making sound.
    bool active = false;
};

/// DirectShow capture device names, in enumeration order. `video` selects the
/// video capture category; otherwise the audio capture category is used.
QStringList directShowDevices(bool video);

/// Friendly name of the default capture endpoint, empty when none is set.
QString defaultMicrophoneName();

/// Friendly name of the default output endpoint, empty when none is set.
QString defaultSpeakerName();

/// Resolve the microphone name using the same fallback as the recorder.
QString resolveMicrophoneDevice(const QStringList& devices, const QString& preferred,
                                const QString& defaultName);

/// Every app that owns a non-expired audio session on the default output,
/// playing apps first and then alphabetically.
QVector<AudioApplication> audioApplications();

/// Peak level of a selected microphone or system-audio source.
///
/// Microphones open in shared mode, so the monitor does not block ffmpeg.
class AudioLevelMonitor
{
public:
    AudioLevelMonitor();
    ~AudioLevelMonitor();

    AudioLevelMonitor(const AudioLevelMonitor&) = delete;
    AudioLevelMonitor& operator=(const AudioLevelMonitor&) = delete;

    /// Watch one capture device by friendly name. An empty name watches the
    /// default microphone. Returns false when the device cannot be opened.
    bool listen(const QString& deviceName);
    /// Start monitoring without waiting for the audio endpoint to open.
    void startMicrophone(const QString& deviceName);
    /// Monitor the default output, or one process tree when processId is nonzero.
    void startSystemAudio(quint32 processId = 0);
    void stop();

    /// Linear peak, 0..1. Zero while stopped or silent.
    float peak() const;

private:
    struct Impl;
    std::unique_ptr<Impl> d;
};

/// Records the default output, every app or one process tree, to a WAV file.
///
/// Windows has no ffmpeg capture path for loopback audio, and a per-app track
/// only exists through the process-loopback activation Windows 10 2004 added.
/// The recorder muxes the WAV with the video once the recording is finalised.
class SystemAudioCapture
{
public:
    enum class Mode {
        /// Every app on the default output.
        AllApps,
        /// One process tree, including child processes.
        Application,
    };

    SystemAudioCapture();
    ~SystemAudioCapture();

    SystemAudioCapture(const SystemAudioCapture&) = delete;
    SystemAudioCapture& operator=(const SystemAudioCapture&) = delete;

    /// Start capturing to `wavPath`. Returns once the endpoint is running, so
    /// a missing app or an unsupported Windows build fails before the video
    /// starts instead of leaving the recording without sound.
    bool start(Mode mode, quint32 processId, const QString& wavPath, QString* error);

    /// Drop everything captured from now on, without a gap in the file, so a
    /// paused recording stays in sync with its concatenated video segments.
    void setPaused(bool paused);

    /// Stop and patch the WAV header. Safe to call more than once.
    void stop();

    bool isActive() const;
    QString path() const;

private:
    struct Impl;
    std::unique_ptr<Impl> d;
};

}  // namespace Platform
