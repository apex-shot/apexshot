#include "platform/windows/disk_space.h"

#include <QDir>
#include <QFileInfo>

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

namespace Platform {
namespace {

std::function<qint64(const QString&)>& probeOverride()
{
    static std::function<qint64(const QString&)> probe;
    return probe;
}

QString existingDirectory(const QString& path)
{
    QFileInfo info(path);
    QString candidate = info.isDir() ? info.absoluteFilePath() : info.absolutePath();
    while (!candidate.isEmpty() && !QFileInfo(candidate).isDir()) {
        const QString parent = QFileInfo(candidate).absolutePath();
        if (parent == candidate)
            return QString();
        candidate = parent;
    }
    return candidate;
}

} // namespace

qint64 freeDiskBytes(const QString& path)
{
    if (probeOverride())
        return probeOverride()(path);
    if (path.isEmpty())
        return -1;
    const QString directory = existingDirectory(path);
    if (directory.isEmpty())
        return -1;
    ULARGE_INTEGER available{};
    const std::wstring native = QDir::toNativeSeparators(directory).toStdWString();
    if (!GetDiskFreeSpaceExW(native.c_str(), &available, nullptr, nullptr))
        return -1;
    return static_cast<qint64>(available.QuadPart);
}

qint64 lowestFreeDiskBytes(const QStringList& paths)
{
    qint64 lowest = -1;
    for (const QString& path : paths) {
        const qint64 free = freeDiskBytes(path);
        if (free >= 0 && (lowest < 0 || free < lowest))
            lowest = free;
    }
    return lowest;
}

void setFreeDiskBytesProbeForTesting(std::function<qint64(const QString&)> probe)
{
    probeOverride() = std::move(probe);
}

} // namespace Platform
