// SPDX-License-Identifier: GPL-3.0-or-later

#pragma once

#include <QString>
#include <QVector>

/// PulseAudio/PipeWire devices behind the Linux Quick Capture menus.
///
/// The recorder opens Pulse sources by name, so the menus hand back the
/// source name and show the device's display name.
namespace Platform {

struct AudioSourceDevice {
    QString name;
    /// "Digital Microphone (Raptor Lake-P/U/H cAVS)", the way the Windows
    /// toolbar names an endpoint.
    QString description;
};

struct AudioOutputDevice {
    QString name;
    QString description;
    /// The `.monitor` source the recorder opens for this output.
    QString monitorSource;
    /// The server's current default output.
    bool isDefault = false;
};

/// One app with a stream on the default output.
struct AudioApplication {
    QString name;
    quint32 processId = 0;
};

/// Every capture source that is a real microphone, in pactl order.
QVector<AudioSourceDevice> pulseMicrophones();

/// Every output (sink) with the monitor source that records it.
QVector<AudioOutputDevice> pulseOutputs();

/// Apps currently playing audio, one row per process.
QVector<AudioApplication> pulseApplications();

/// The current default source, empty when Pulse cannot be queried.
QString defaultMicrophoneSource();

/// Monitor source of the current default output, empty when unavailable.
QString defaultSpeakerMonitor();

}  // namespace Platform
