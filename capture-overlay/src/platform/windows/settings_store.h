#pragma once

#include "platform/recording_settings.h"

#include <QDateTime>
#include <QImage>
#include <QString>

/// Settings shared between the Rust settings window and the C++ shell.
///
/// The Rust window owns `%APPDATA%/apexshot/config.yml` (the Linux config) and
/// mirrors the fields the Windows shell applies into
/// `%APPDATA%/apexshot/settings.json`. Reading JSON here keeps the shell free of
/// a YAML dependency while leaving the Rust config the source of truth.
namespace Platform {

struct ShellSettings {
    struct General {
        bool startAtLogin = false;
        bool showTrayIcon = true;
        QString theme = QStringLiteral("system");
        QString language = QStringLiteral("system");
        bool playSounds = true;
        QString shutterSound = QStringLiteral("Camera");
    } general;

    struct Screenshots {
        /// Empty means the user's Pictures folder.
        QString exportLocation;
        QString format = QStringLiteral("PNG");
        QString clipboardMode = QStringLiteral("File & Image (default)");
        int timerInterval = 0;
        bool showCursor = true;
        bool afterShowResult = true;
        bool afterCopyToClipboard = false;
        bool afterSave = true;
        bool highDpiSuffix = false;
    } screenshots;

    struct Recording {
        bool showCountdown = true;
        QString exportLocation;
        QString filenamePattern;
        RecordingSettings values;
        bool includeMicrophone = false;
        bool includeSpeaker = false;
        bool showCursor = true;
        bool afterShowResult = true;
    } recording;

    struct QuickAccess {
        QString position = QStringLiteral("Left");
        double overlayScale = 1.0;
        bool autoCloseEnabled = false;
        int autoCloseSeconds = 30;
    } quickAccess;

    struct Ocr {
        /// Tesseract-style code from the Rust config (`eng`, `spa`, ...).
        QString language = QStringLiteral("eng");
        bool keepLineBreaks = true;
    } ocr;
};

/// `%APPDATA%/apexshot/settings.json`.
QString shellSettingsPath();

/// Read the JSON mirror, applying defaults for missing or invalid values.
ShellSettings loadShellSettings();

/// Resolve the screenshot folder (empty config means Pictures).
QString shellScreenshotOutputDir(const ShellSettings& settings);

/// Resolve the recording folder (empty config means Videos/ApexShotRec).
QString shellRecordingOutputDir(const ShellSettings& settings);

/// Expand `{Date}` and `{Time}` in the filename pattern and join the folder.
QString shellRecordingOutputPath(const ShellSettings& settings, const QDateTime& now);

/// Clipboard behaviour matching the Linux setting values.
void copyCaptureToClipboard(const QImage& image, const QString& filePath, const QString& mode);

/// Encode `image` into `directory`, creating it and avoiding an existing name.
bool saveScreenshotImage(const QImage& image,
                         const ShellSettings& settings,
                         const QString& directory,
                         qreal devicePixelRatio,
                         QString& outPath,
                         QString& outError);

}  // namespace Platform
