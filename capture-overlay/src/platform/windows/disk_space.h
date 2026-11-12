#pragma once

#include <QString>
#include <QStringList>

#include <functional>

namespace Platform {

/// Free space a recording keeps on its volumes. Below it the recorder stops
/// cleanly and keeps what it has, the way the studied recorder stops on low
/// disk space instead of letting the encoder die mid-write.
constexpr qint64 kRecordingDiskReserveBytes = 512LL * 1024 * 1024;

/// Bytes available to this user on the volume holding `path`, or -1 when the
/// size cannot be read. A path that does not exist yet is resolved through its
/// nearest existing parent directory.
qint64 freeDiskBytes(const QString& path);

/// The smallest known free size across `paths`, or -1 when none is known.
qint64 lowestFreeDiskBytes(const QStringList& paths);

/// Test seam: replace the probe `freeDiskBytes` uses. An empty function
/// restores the real probe.
void setFreeDiskBytesProbeForTesting(std::function<qint64(const QString&)> probe);

} // namespace Platform
