// Coverage for the JSON settings mirror shared with the Rust settings window.

#include "platform/windows/settings_store.h"

#include <QCoreApplication>
#include <QDate>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QMimeData>
#include <QTemporaryDir>
#include <QTime>
#include <QUrl>

#include <cstdio>
#include <memory>

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
    CHECK(!settings.general.startAtLogin);
    CHECK(settings.general.showTrayIcon);
    CHECK(settings.general.theme == QStringLiteral("system"));
    CHECK(settings.general.language == QStringLiteral("system"));
    CHECK(settings.shortcuts.value(QStringLiteral("quick_capture")) == QStringLiteral("Ctrl+Alt+Shift+M"));
    CHECK(settings.shortcuts.value(QStringLiteral("record_screen")).isEmpty());
    CHECK(settings.screenshots.showCursor);
    CHECK(settings.screenshots.afterShowResult);
    CHECK(!settings.screenshots.afterCopyToClipboard);
    CHECK(settings.screenshots.afterSave);
    CHECK(settings.recording.showCountdown);
    CHECK(settings.recording.showControls);
    CHECK(settings.recording.highDpi);
    CHECK(settings.recording.values.fpsIndex == 1);
    CHECK(settings.recording.values.maxResolutionIndex == 0);
    CHECK(settings.recording.values.qualityIndex == 1);
    CHECK(!settings.recording.values.monoAudio);
    CHECK(!settings.recording.includeMicrophone);
    CHECK(settings.recording.showCursor);
    CHECK(!settings.recording.afterCopyToClipboard);
    CHECK(settings.recording.afterSave);
    CHECK(!settings.recording.afterOpenEditor);
    CHECK(!settings.recording.webcam.enabled);
    CHECK(settings.recording.webcam.device.isEmpty());
    CHECK(settings.recording.webcam.sizeIndex == 1);
    CHECK(settings.recording.webcam.shapeIndex == 0);
    CHECK(!settings.recording.webcam.flip);
    CHECK(qFuzzyCompare(settings.recording.webcam.relX, 1.0));
    CHECK(qFuzzyCompare(settings.recording.webcam.relY, 0.0));
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
  "general": { "start_at_login": true, "show_tray_icon": false,
               "theme": "dark", "language": "fr",
               "play_sounds": false, "shutter_sound": "Classic" },
  "shortcuts": { "quick_capture": "Ctrl+Alt+M", "capture_area": "",
                 "record_screen": "Ctrl+Alt+R" },
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
    "show_controls": false,
    "high_dpi": false,
    "export_location": "C:/Videos/Clips",
    "filename_pattern": "Take {Date} {Time}",
    "fps_index": 3,
    "max_resolution_index": 2,
    "quality_index": 0,
    "mono_audio": true,
    "include_microphone": true,
    "include_speaker": true,
    "show_cursor": false,
    "after_show_result": false,
    "after_copy_to_clipboard": true,
    "after_save": false,
    "after_open_editor": true,
    "webcam": {
      "enabled": true,
      "device": " Integrated Camera ",
      "size_index": 3,
      "shape_index": 2,
      "flip": true,
      "rel_x": 0.25,
      "rel_y": 0.8
    }
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
    CHECK(settings.general.startAtLogin);
    CHECK(!settings.general.showTrayIcon);
    CHECK(settings.general.theme == QStringLiteral("dark"));
    CHECK(settings.general.language == QStringLiteral("fr"));
    CHECK(settings.shortcuts.value(QStringLiteral("quick_capture")) == QStringLiteral("Ctrl+Alt+M"));
    CHECK(settings.shortcuts.value(QStringLiteral("capture_area")).isEmpty());
    CHECK(settings.shortcuts.value(QStringLiteral("record_screen")) == QStringLiteral("Ctrl+Alt+R"));
    CHECK(settings.shortcuts.value(QStringLiteral("capture_fullscreen")) == QStringLiteral("Ctrl+Alt+Shift+F"));
    CHECK(settings.screenshots.clipboardMode == QStringLiteral("Image Only"));
    CHECK(settings.screenshots.timerInterval == 5);
    CHECK(!settings.screenshots.showCursor);
    CHECK(!settings.screenshots.afterShowResult);
    CHECK(settings.screenshots.afterCopyToClipboard);
    CHECK(!settings.screenshots.afterSave);
    CHECK(settings.screenshots.highDpiSuffix);
    CHECK(!settings.recording.showCountdown);
    CHECK(!settings.recording.showControls);
    CHECK(!settings.recording.highDpi);
    CHECK(settings.recording.values.fpsIndex == 3);
    CHECK(settings.recording.values.maxResolutionIndex == 2);
    CHECK(settings.recording.values.qualityIndex == 0);
    CHECK(settings.recording.values.monoAudio);
    CHECK(settings.recording.includeMicrophone);
    CHECK(settings.recording.includeSpeaker);
    CHECK(!settings.recording.showCursor);
    CHECK(!settings.recording.afterShowResult);
    CHECK(settings.recording.afterCopyToClipboard);
    CHECK(!settings.recording.afterSave);
    CHECK(settings.recording.afterOpenEditor);
    CHECK(settings.recording.webcam.enabled);
    CHECK(settings.recording.webcam.device == QStringLiteral("Integrated Camera"));
    CHECK(settings.recording.webcam.sizeIndex == 3);
    CHECK(settings.recording.webcam.shapeIndex == 2);
    CHECK(settings.recording.webcam.flip);
    CHECK(qFuzzyCompare(settings.recording.webcam.relX, 0.25));
    CHECK(qFuzzyCompare(settings.recording.webcam.relY, 0.8));
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
                 "filename_pattern": "   ",
                 "webcam": { "size_index": 9, "shape_index": -4,
                             "rel_x": 4.5, "rel_y": -1.0 } },
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
    CHECK(settings.recording.webcam.sizeIndex == 3);
    CHECK(settings.recording.webcam.shapeIndex == 0);
    CHECK(qFuzzyCompare(settings.recording.webcam.relX, 1.0));
    CHECK(qFuzzyCompare(settings.recording.webcam.relY, 0.0));
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
    settings.recording.exportLocation = dir.path();
    const QString first = Platform::shellRecordingOutputPath(settings, when);
    QFile existing(first);
    CHECK(existing.open(QIODevice::WriteOnly));
    existing.close();
    const QString next = Platform::shellRecordingOutputPath(settings, when);
    CHECK(next != first);
    CHECK(next.endsWith(QStringLiteral(" (2).mp4")));
    const QString temporary = Platform::shellTemporaryRecordingOutputPath();
    CHECK(QFileInfo(temporary).absolutePath()
          == QDir(QDir::tempPath()).filePath(QStringLiteral("ApexShot")));
    CHECK(temporary.endsWith(QStringLiteral(".mp4")));
    CHECK(temporary != Platform::shellTemporaryRecordingOutputPath());
}

void checkRecordingDeviceChoices(const QTemporaryDir& dir)
{
    const QString path = Platform::recordingDeviceChoicesPath();
    CHECK(path == QDir(dir.path()).filePath(QStringLiteral("apexshot/recording-devices.json")));
    QFile::remove(path);
    CHECK(!Platform::loadRecordingDeviceChoices().present);

    Platform::RecordingDeviceChoices saved;
    saved.microphoneEnabled = true;
    saved.microphoneDevice = QStringLiteral("Microphone Array");
    saved.systemAudioMode = QStringLiteral("app");
    saved.systemAudioApplication = QStringLiteral("Google Chrome");
    saved.cameraEnabled = true;
    saved.cameraDevice = QStringLiteral("Integrated Camera");
    Platform::saveRecordingDeviceChoices(saved);

    const Platform::RecordingDeviceChoices loaded = Platform::loadRecordingDeviceChoices();
    CHECK(loaded.present);
    CHECK(loaded.microphoneEnabled);
    CHECK(loaded.microphoneDevice == QStringLiteral("Microphone Array"));
    CHECK(loaded.systemAudioMode == QStringLiteral("app"));
    CHECK(loaded.systemAudioApplication == QStringLiteral("Google Chrome"));
    CHECK(loaded.cameraEnabled);
    CHECK(loaded.cameraDevice == QStringLiteral("Integrated Camera"));
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
    const QString originalPath = path;

    settings.screenshots.highDpiSuffix = false;
    CHECK(Platform::saveScreenshotImage(image, settings, folder, 2.0, path, error));
    CHECK(!QFileInfo(path).fileName().contains(QStringLiteral("@2x")));
    settings.screenshots.highDpiSuffix = true;
    CHECK(Platform::saveScreenshotImage(image, settings, folder, 1.0, path, error));
    CHECK(!QFileInfo(path).fileName().contains(QStringLiteral("@2x")));

    CHECK(Platform::saveScreenshotImage(image, settings, folder, 2.0, path, error));
    CHECK(path != originalPath);
    CHECK(QFileInfo::exists(originalPath));

    for (const auto& entry : {qMakePair(QStringLiteral("JPEG"), QStringLiteral("jpg")),
                              qMakePair(QStringLiteral("WebP"), QStringLiteral("webp"))}) {
        settings.screenshots.format = entry.first;
        CHECK(Platform::saveScreenshotImage(image, settings, folder, 1.0, path, error));
        CHECK(QFileInfo(path).suffix() == entry.second);
        const QImage restored(path);
        CHECK(!restored.isNull());
        CHECK(restored.size() == image.size());
    }

    QFile nonDirectory(QDir(dir.path()).filePath(QStringLiteral("not-a-folder")));
    CHECK(nonDirectory.open(QIODevice::WriteOnly));
    nonDirectory.close();
    CHECK(!Platform::saveScreenshotImage(image, settings,
                                          nonDirectory.fileName() + QStringLiteral("/child"),
                                          1.0, path, error));
    CHECK(path.isEmpty());
    CHECK(!error.isEmpty());
}

void checkClipboardData()
{
    QImage image(4, 4, QImage::Format_RGB32);
    image.fill(Qt::red);
    const QString path = QStringLiteral("C:/shots/capture.png");
    const auto create = [&](const QString& mode, const QString& file = QString()) {
        return std::unique_ptr<QMimeData>(Platform::captureClipboardData(
            image, file.isNull() ? path : file, mode));
    };

    auto both = create(QStringLiteral("File & Image (default)"));
    CHECK(both && both->hasImage() && both->hasUrls());
    CHECK(both && !both->urls().isEmpty() && both->urls().first() == QUrl::fromLocalFile(path));
    auto imageOnly = create(QStringLiteral("Image Only"));
    CHECK(imageOnly && imageOnly->hasImage() && !imageOnly->hasUrls());
    auto fileOnly = create(QStringLiteral("File Path Only"));
    CHECK(fileOnly && !fileOnly->hasImage() && fileOnly->hasUrls());
    auto fallback = create(QStringLiteral("unknown"));
    CHECK(fallback && fallback->hasImage() && fallback->hasUrls());
    auto missingFile = create(QStringLiteral("File Path Only"), QStringLiteral(""));
    CHECK(!missingFile);
    auto missingBoth = create(QStringLiteral("File & Image (default)"), QStringLiteral(""));
    CHECK(missingBoth && missingBoth->hasImage() && !missingBoth->hasUrls());
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
    checkRecordingDeviceChoices(dir);
    checkSaving(dir);
    checkClipboardData();

    if (failures != 0) {
        std::printf("%d settings-store check(s) failed\n", failures);
        return 1;
    }
    std::printf("settings-store checks passed\n");
    return 0;
}
