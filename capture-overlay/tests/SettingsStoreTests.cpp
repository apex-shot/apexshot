// Coverage for the JSON settings mirror shared with the Rust settings window.

#include "platform/windows/settings_store.h"

#include <QCoreApplication>
#include <QDate>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QTemporaryDir>
#include <QTime>

#include <cstdio>

namespace {

int failures = 0;

void check(bool ok, const char* expression, int line)
{
    if (ok)
        return;
    std::printf("FAIL line %d: %s\n", line, expression);
    ++failures;
}

#define CHECK(expression) check((expression), #expression, __LINE__)

QString settingsFile(const QTemporaryDir& dir)
{
    return QDir(dir.path()).filePath(QStringLiteral("apexshot/settings.json"));
}

void writeSettings(const QTemporaryDir& dir, const QByteArray& json)
{
    QDir().mkpath(QFileInfo(settingsFile(dir)).absolutePath());
    QFile file(settingsFile(dir));
    CHECK(file.open(QIODevice::WriteOnly));
    file.write(json);
}

void removeSettings(const QTemporaryDir& dir)
{
    QFile::remove(settingsFile(dir));
}

void checkDefaults(const QTemporaryDir& dir)
{
    Q_UNUSED(dir);
    const Platform::ShellSettings settings = Platform::loadShellSettings();
    CHECK(settings.screenshots.format == QStringLiteral("PNG"));
    CHECK(settings.general.playSounds);
    CHECK(settings.general.shutterSound == QStringLiteral("Camera"));
    CHECK(settings.screenshots.showCursor);
    CHECK(settings.screenshots.afterShowResult);
    CHECK(!settings.screenshots.afterCopyToClipboard);
    CHECK(settings.screenshots.afterSave);
    CHECK(settings.recording.showCountdown);
    CHECK(settings.recording.values.fpsIndex == 1);
    CHECK(settings.recording.values.maxResolutionIndex == 0);
    CHECK(settings.recording.values.qualityIndex == 1);
    CHECK(!settings.recording.values.monoAudio);
    CHECK(!settings.recording.includeMicrophone);
    CHECK(settings.recording.showCursor);
    CHECK(settings.quickAccess.position == QStringLiteral("Left"));
    CHECK(qFuzzyCompare(settings.quickAccess.overlayScale, 1.0));
    CHECK(!settings.quickAccess.autoCloseEnabled);
    CHECK(settings.quickAccess.autoCloseSeconds == 30);
    CHECK(settings.ocr.language == QStringLiteral("eng"));
    CHECK(settings.ocr.keepLineBreaks);
}

void checkParsing(const QTemporaryDir& dir)
{
    writeSettings(dir,
                  R"({
  "general": { "play_sounds": false, "shutter_sound": "Classic" },
  "screenshots": {
    "export_location": " C:/Users/me/Pictures/Shots ",
    "format": "WebP",
    "clipboard_mode": "Image Only",
    "timer_interval": 5,
    "show_cursor": false,
    "after_show_result": false,
    "after_copy_to_clipboard": true,
    "after_save": false,
    "high_dpi_suffix": true
  },
  "recording": {
    "show_countdown": false,
    "export_location": "C:/Videos/Clips",
    "filename_pattern": "Take {Date} {Time}",
    "fps_index": 3,
    "max_resolution_index": 2,
    "quality_index": 0,
    "mono_audio": true,
    "include_microphone": true,
    "include_speaker": true,
    "show_cursor": false,
    "after_show_result": false
  },
  "quick_access": {
    "position": "Right",
    "overlay_scale": 1.2,
    "auto_close_enabled": true,
    "auto_close_seconds": 45
  },
  "ocr": {
    "language": "deu",
    "keep_line_breaks": false
  }
})");

    const Platform::ShellSettings settings = Platform::loadShellSettings();
    CHECK(settings.screenshots.exportLocation == QStringLiteral("C:/Users/me/Pictures/Shots"));
    CHECK(settings.screenshots.format == QStringLiteral("WebP"));
    CHECK(!settings.general.playSounds);
    CHECK(settings.general.shutterSound == QStringLiteral("Classic"));
    CHECK(settings.screenshots.clipboardMode == QStringLiteral("Image Only"));
    CHECK(settings.screenshots.timerInterval == 5);
    CHECK(!settings.screenshots.showCursor);
    CHECK(!settings.screenshots.afterShowResult);
    CHECK(settings.screenshots.afterCopyToClipboard);
    CHECK(!settings.screenshots.afterSave);
    CHECK(settings.screenshots.highDpiSuffix);
    CHECK(!settings.recording.showCountdown);
    CHECK(settings.recording.values.fpsIndex == 3);
    CHECK(settings.recording.values.maxResolutionIndex == 2);
    CHECK(settings.recording.values.qualityIndex == 0);
    CHECK(settings.recording.values.monoAudio);
    CHECK(settings.recording.includeMicrophone);
    CHECK(settings.recording.includeSpeaker);
    CHECK(!settings.recording.showCursor);
    CHECK(!settings.recording.afterShowResult);
    CHECK(settings.quickAccess.position == QStringLiteral("Right"));
    CHECK(qFuzzyCompare(settings.quickAccess.overlayScale, 1.2));
    CHECK(settings.quickAccess.autoCloseEnabled);
    CHECK(settings.quickAccess.autoCloseSeconds == 45);
    CHECK(settings.ocr.language == QStringLiteral("deu"));
    CHECK(!settings.ocr.keepLineBreaks);
}

void checkSanitizing(const QTemporaryDir& dir)
{
    writeSettings(dir,
                  R"({
  "screenshots": { "format": "GIF", "timer_interval": 7 },
  "recording": { "fps_index": 99, "max_resolution_index": 99, "quality_index": 99,
                 "filename_pattern": "   " },
  "quick_access": { "position": "Middle", "overlay_scale": 9,
                    "auto_close_seconds": 1 },
  "ocr": { "language": "  " }
})");

    const Platform::ShellSettings settings = Platform::loadShellSettings();
    CHECK(settings.screenshots.format == QStringLiteral("PNG"));
    CHECK(settings.screenshots.timerInterval == 0);
    CHECK(settings.recording.values.fpsIndex == 1);
    CHECK(settings.recording.values.maxResolutionIndex == 0);
    CHECK(settings.recording.values.qualityIndex == 1);
    CHECK(settings.recording.filenamePattern
          == QStringLiteral("ApexShot Recording {Date} at {Time}"));
    CHECK(settings.quickAccess.position == QStringLiteral("Right"));
    CHECK(qFuzzyCompare(settings.quickAccess.overlayScale, 1.5));
    CHECK(settings.quickAccess.autoCloseSeconds == 5);
    CHECK(settings.ocr.language.isEmpty());
}

void checkPaths(const QTemporaryDir& dir)
{
    Platform::ShellSettings settings;
    settings.screenshots.exportLocation = QStringLiteral("C:/shots");
    CHECK(Platform::shellScreenshotOutputDir(settings) == QStringLiteral("C:/shots"));

    settings.recording.exportLocation = QStringLiteral("C:/videos");
    settings.recording.filenamePattern = QStringLiteral("Clip {Date} {Time}");
    const QDateTime when(QDate(2026, 10, 22), QTime(23, 45, 6));
    CHECK(Platform::shellRecordingOutputPath(settings, when)
          == QStringLiteral("C:/videos/Clip 2026-10-22 23-45-06.mp4"));
}

void checkSaving(const QTemporaryDir& dir)
{
    Platform::ShellSettings settings;
    settings.screenshots.format = QStringLiteral("PNG");
    settings.screenshots.highDpiSuffix = true;

    QImage image(4, 4, QImage::Format_RGB32);
    image.fill(Qt::red);

    const QString folder = QDir(dir.path()).filePath(QStringLiteral("shots"));
    QString path;
    QString error;
    CHECK(Platform::saveScreenshotImage(image, settings, folder, 2.0, path, error));
    CHECK(error.isEmpty());
    CHECK(QFileInfo::exists(path));
    CHECK(QFileInfo(path).suffix() == QStringLiteral("png"));
    CHECK(QFileInfo(path).fileName().contains(QStringLiteral("@2x")));
}

}  // namespace

int main(int argc, char** argv)
{
    QCoreApplication app(argc, argv);

    QTemporaryDir dir;
    if (!dir.isValid()) {
        std::printf("FAIL: could not create a temporary directory\n");
        return 1;
    }
    qputenv("APPDATA", dir.path().toUtf8());

    removeSettings(dir);
    checkDefaults(dir);
    checkParsing(dir);
    checkSanitizing(dir);
    checkPaths(dir);
    checkSaving(dir);

    if (failures != 0) {
        std::printf("%d settings-store check(s) failed\n", failures);
        return 1;
    }
    std::printf("settings-store checks passed\n");
    return 0;
}
