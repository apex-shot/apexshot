#include "platform/windows/capture_sound.h"

#include "platform/windows/settings_store.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QStringList>

#include <windows.h>
#include <mmsystem.h>

namespace {

QString soundFileName(const QString& sound)
{
    if (sound == QLatin1String("Camera"))
        return QStringLiteral("camera.wav");
    if (sound == QLatin1String("Classic"))
        return QStringLiteral("classic.wav");
    if (sound == QLatin1String("Pop"))
        return QStringLiteral("pop.wav");
    return QString();
}

QString resolveSoundPath(const QString& sound)
{
    const QString fileName = soundFileName(sound);
    if (fileName.isEmpty())
        return QString();

    QStringList directories;
    const QString override = qEnvironmentVariable("APEXSHOT_SOUND_DIR");
    if (!override.isEmpty())
        directories.append(override);

#ifdef APEXSHOT_SOUND_DIR
    directories.append(QStringLiteral(APEXSHOT_SOUND_DIR));
#endif

    const QString appDir = QCoreApplication::applicationDirPath();
    directories.append(QDir(appDir).filePath(QStringLiteral("assets/sounds")));
    directories.append(QDir(appDir).filePath(QStringLiteral("../share/apexshot/sounds")));
    // Development layout: capture-overlay/build -> repository root.
    directories.append(QDir(appDir).filePath(QStringLiteral("../../assets/sounds")));

    for (const QString& directory : directories) {
        const QString path = QDir(directory).filePath(fileName);
        if (QFileInfo::exists(path))
            return QDir::toNativeSeparators(path);
    }
    return QString();
}

}  // namespace

namespace Platform {

void playShutterSound()
{
    const ShellSettings settings = loadShellSettings();
    if (!settings.general.playSounds || settings.general.shutterSound == QLatin1String("None"))
        return;

    const QString path = resolveSoundPath(settings.general.shutterSound);
    if (path.isEmpty()) {
        qWarning("shutter sound '%s' is not available",
                 qUtf8Printable(settings.general.shutterSound));
        return;
    }

    PlaySoundW(reinterpret_cast<LPCWSTR>(path.utf16()),
               nullptr,
               SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
}

}  // namespace Platform
