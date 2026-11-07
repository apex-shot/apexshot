#include "platform/windows/capture_sound.h"

#include "platform/windows/settings_store.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QHash>
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

/// A quarter second of digital silence, used to keep the audio endpoint
/// streaming while a selection is on screen so the shutter starts at once.
QByteArray silentWave()
{
    constexpr quint32 rate = 44100;
    constexpr quint32 samples = rate / 4;
    constexpr quint32 dataBytes = samples * 2;
    constexpr quint32 riffBytes = 36 + dataBytes;

    QByteArray wave;
    wave.reserve(44 + static_cast<int>(dataBytes));
    const auto appendAscii = [&wave](const char* text) { wave.append(text, 4); };
    const auto appendLE = [&wave](quint32 value, int bytes) {
        for (int i = 0; i < bytes; ++i)
            wave.append(static_cast<char>((value >> (8 * i)) & 0xFF));
    };

    appendAscii("RIFF");
    appendLE(riffBytes, 4);
    appendAscii("WAVEfmt ");
    appendLE(16, 4);
    appendLE(1, 2);
    appendLE(1, 2);
    appendLE(rate, 4);
    appendLE(rate * 2, 4);
    appendLE(2, 2);
    appendLE(16, 2);
    appendAscii("data");
    appendLE(dataBytes, 4);
    wave.append(static_cast<int>(dataBytes), static_cast<char>(0));
    return wave;
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

void primeShutterSound()
{
    const ShellSettings settings = loadShellSettings();
    if (!settings.general.playSounds || settings.general.shutterSound == QLatin1String("None"))
        return;

    // Looping silence holds the endpoint open, so the shutter that follows is
    // already streaming when the capture lands instead of waiting for the
    // engine to wake up.
    static const QByteArray silence = silentWave();
    static bool warned = false;
    if (!PlaySoundW(reinterpret_cast<LPCWSTR>(silence.constData()),
                    nullptr,
                    SND_MEMORY | SND_ASYNC | SND_LOOP | SND_NODEFAULT)) {
        if (!warned) {
            warned = true;
            qWarning("the shutter sound could not be primed");
        }
    }
}

void releaseShutterSound()
{
    PlaySoundW(nullptr, nullptr, 0);
}

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

    // Read the wave once and keep it: PlaySound reads the buffer on its own
    // thread, while reopening the file on every capture puts the click behind
    // the frame it is meant to mark.
    static QHash<QString, QByteArray> waves;
    auto wave = waves.find(path);
    if (wave == waves.end()) {
        QFile file(path);
        QByteArray data;
        if (file.open(QIODevice::ReadOnly))
            data = file.readAll();
        wave = waves.insert(path, data);
    }
    if (wave->isEmpty()) {
        PlaySoundW(reinterpret_cast<LPCWSTR>(path.utf16()),
                   nullptr,
                   SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
        return;
    }

    if (!PlaySoundW(reinterpret_cast<LPCWSTR>(wave->constData()),
                    nullptr,
                    SND_MEMORY | SND_ASYNC | SND_NODEFAULT)) {
        qWarning("the shutter sound could not be played: %s", qUtf8Printable(path));
    }
}

}  // namespace Platform
