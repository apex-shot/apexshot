// SPDX-License-Identifier: GPL-3.0-or-later

#pragma once

#include <QString>
#include <QVector>

/// PulseAudio/PipeWire devices behind the Linux Quick Capture menus.
///
/// The recorder opens Pulse sources by name, so the menus hand back the
/// source name and show the device description.
namespace Platform {

struct AudioSourceDevice {
    QString name;
    QString description;
};

struct AudioOutputDevice {
    QString name;
    QString description;
    /// The `.monitor` source the recorder opens for this output.
    QString monitorSource;
};

/// Every capture source that is a real microphone, in pactl order.
QVector<AudioSourceDevice> pulseMicrophones();

/// Every output (sink) with the monitor source that records it.
QVector<AudioOutputDevice> pulseOutputs();

/// The current default source, empty when Pulse cannot be queried.
QString defaultMicrophoneSource();

/// Monitor source of the current default output, empty when unavailable.
QString defaultSpeakerMonitor();

}  // namespace Platform
