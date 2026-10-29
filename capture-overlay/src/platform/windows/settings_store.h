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

    /// The webcam layer, mirroring the Rust config's `rec_webcam_*` fields.
    struct Webcam {
        bool enabled = false;
        /// dshow friendly name. Empty asks the recorder for the first camera.
        QString device;
        /// 0 small, 1 medium, 2 large, 3 huge.
        int sizeIndex = 1;
        /// 0 circle, 1 square, 2 rectangle, 3 vertical.
        int shapeIndex = 0;
        bool flip = false;
        /// Bubble position across the capture: x left to right, y bottom to top.
        double relX = 1.0;
        double relY = 0.0;
    };

    struct Recording {
        bool showCountdown = true;
        QString exportLocation;
        QString filenamePattern;
        RecordingSettings values;
        bool includeMicrophone = false;
        bool includeSpeaker = false;
        bool showCursor = true;
        bool afterShowResult = true;
        Webcam webcam;
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

/// The device choices the window-recording surface remembers between runs.
///
/// They live beside the settings mirror rather than inside it: the Rust
/// settings window rewrites settings.json wholesale, and these are shell-owned
/// selections the Rust side neither edits nor reads.
struct RecordingDeviceChoices {
    bool present = false;
    bool microphoneEnabled = false;
    QString microphoneDevice;
    /// "off", "all", or "app".
    QString systemAudioMode = QStringLiteral("off");
    QString systemAudioApplication;
    bool cameraEnabled = false;
    QString cameraDevice;
};

/// `%APPDATA%/apexshot/settings.json`.
QString shellSettingsPath();

/// Where the person left the camera bubble, and the look they gave it, so the
/// next preview opens the way they left it instead of at the defaults.
struct WebcamPlacement {
    bool present = false;
    double relX = 1.0;
    double relY = 0.0;
    /// The bubble's width as a fraction of the display's height, which is what
    /// keeps a resized bubble the same size on any screen.
    double sizeFraction = 0.0;
    /// 0 circle, 1 square, 2 rectangle, 3 vertical.
    int shape = 0;
    bool flip = false;
};

/// `%APPDATA%/apexshot/webcam-placement.json`.
QString webcamPlacementPath();
WebcamPlacement loadWebcamPlacement();
/// Write the whole state: `sizeFraction` of 0 leaves the size to the preset.
void saveWebcamPlacement(const WebcamPlacement& placement);

/// `%APPDATA%/apexshot/recording-devices.json`.
QString recordingDeviceChoicesPath();
RecordingDeviceChoices loadRecordingDeviceChoices();
void saveRecordingDeviceChoices(const RecordingDeviceChoices& choices);

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
