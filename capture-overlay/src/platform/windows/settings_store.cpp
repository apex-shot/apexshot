#include "platform/windows/settings_store.h"

#include <QClipboard>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QImageWriter>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QMimeData>
#include <QStandardPaths>
#include <QUrl>
#include <QUuid>

#include <algorithm>
#include <cmath>

namespace {

QString defaultRecordingPattern()
{
    return QStringLiteral("ApexShot Recording {Date} at {Time}");
}

QString defaultRecordingDirectory()
{
    QString directory = QStandardPaths::writableLocation(QStandardPaths::MoviesLocation);
    if (directory.isEmpty())
        directory = QDir::home().filePath(QStringLiteral("Videos"));
    return QDir(directory).filePath(QStringLiteral("ApexShotRec"));
}

QString normalizedFormat(const QString& value)
{
    const QString format = value.trimmed().toUpper();
    if (format == QLatin1String("JPG") || format == QLatin1String("JPEG"))
        return QStringLiteral("JPEG");
    if (format == QLatin1String("WEBP"))
        return QStringLiteral("WebP");
    return QStringLiteral("PNG");
}

QString normalizedClipboardMode(const QString& value)
{
    const QString mode = value.trimmed();
    if (mode == QLatin1String("Image Only") || mode == QLatin1String("File Path Only"))
        return mode;
    return QStringLiteral("File & Image (default)");
}

QString normalizedPosition(const QString& value)
{
    return value == QLatin1String("Left") ? QStringLiteral("Left") : QStringLiteral("Right");
}

QString screenshotExtension(const QString& format)
{
    const QString normalized = normalizedFormat(format);
    if (normalized == QLatin1String("JPEG"))
        return QStringLiteral("jpg");
    if (normalized == QLatin1String("WebP"))
        return QStringLiteral("webp");
    return QStringLiteral("png");
}

bool isOneOf(int value, std::initializer_list<int> allowed)
{
    return std::find(allowed.begin(), allowed.end(), value) != allowed.end();
}

QString uniquePath(const QString& directory, const QString& fileName)
{
    const QFileInfo info(QDir(directory).filePath(fileName));
    QString candidate = info.absoluteFilePath();
    if (!QFileInfo::exists(candidate))
        return candidate;

    const QString base = info.completeBaseName();
    const QString suffix = info.suffix();
    for (int index = 2; index < 10000; ++index) {
        const QString name =
            suffix.isEmpty()
                ? QStringLiteral("%1 (%2)").arg(base).arg(index)
                : QStringLiteral("%1 (%2).%3").arg(base).arg(index).arg(suffix);
        candidate = QDir(directory).filePath(name);
        if (!QFileInfo::exists(candidate))
            return candidate;
    }
    return QString();
}

}  // namespace

namespace Platform {

QString shellSettingsPath()
{
    const QString appData = qEnvironmentVariable("APPDATA");
    if (!appData.isEmpty())
        return QDir(appData).filePath(QStringLiteral("apexshot/settings.json"));

    return QDir(QStandardPaths::writableLocation(QStandardPaths::AppConfigLocation))
        .filePath(QStringLiteral("settings.json"));
}

QString webcamPlacementPath()
{
    return QFileInfo(shellSettingsPath()).absoluteDir().filePath(
        QStringLiteral("webcam-placement.json"));
}

WebcamPlacement loadWebcamPlacement()
{
    WebcamPlacement placement;
    QFile file(webcamPlacementPath());
    if (!file.open(QIODevice::ReadOnly))
        return placement;

    QJsonParseError error{};
    const QJsonDocument document = QJsonDocument::fromJson(file.readAll(), &error);
    if (error.error != QJsonParseError::NoError || !document.isObject())
        return placement;

    const QJsonObject root = document.object();
    if (!root.contains(QStringLiteral("rel_x")) || !root.contains(QStringLiteral("rel_y")))
        return placement;
    placement.present = true;
    placement.relX = std::clamp(root.value(QStringLiteral("rel_x")).toDouble(placement.relX), 0.0, 1.0);
    placement.relY = std::clamp(root.value(QStringLiteral("rel_y")).toDouble(placement.relY), 0.0, 1.0);
    // Files written before the look was remembered carry only the position.
    placement.sizeFraction =
        std::clamp(root.value(QStringLiteral("size_fraction")).toDouble(0.0), 0.0, 1.0);
    placement.shape = std::clamp(root.value(QStringLiteral("shape")).toInt(0), 0, 3);
    placement.flip = root.value(QStringLiteral("flip")).toBool(false);
    return placement;
}

void saveWebcamPlacement(const WebcamPlacement& placement)
{
    QJsonObject root;
    root.insert(QStringLiteral("rel_x"), std::clamp(placement.relX, 0.0, 1.0));
    root.insert(QStringLiteral("rel_y"), std::clamp(placement.relY, 0.0, 1.0));
    if (placement.sizeFraction > 0.0)
        root.insert(QStringLiteral("size_fraction"), std::clamp(placement.sizeFraction, 0.0, 1.0));
    root.insert(QStringLiteral("shape"), std::clamp(placement.shape, 0, 3));
    root.insert(QStringLiteral("flip"), placement.flip);

    const QString path = webcamPlacementPath();
    QDir().mkpath(QFileInfo(path).absolutePath());
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate))
        return;
    file.write(QJsonDocument(root).toJson(QJsonDocument::Indented));
}

QString recordingDeviceChoicesPath()
{
    return QFileInfo(shellSettingsPath()).absoluteDir().filePath(
        QStringLiteral("recording-devices.json"));
}

RecordingDeviceChoices loadRecordingDeviceChoices()
{
    RecordingDeviceChoices choices;
    QFile file(recordingDeviceChoicesPath());
    if (!file.open(QIODevice::ReadOnly))
        return choices;

    QJsonParseError error{};
    const QJsonDocument document = QJsonDocument::fromJson(file.readAll(), &error);
    if (error.error != QJsonParseError::NoError || !document.isObject())
        return choices;

    const QJsonObject root = document.object();
    choices.present = true;
    choices.microphoneEnabled =
        root.value(QStringLiteral("microphone_enabled")).toBool(choices.microphoneEnabled);
    choices.microphoneDevice =
        root.value(QStringLiteral("microphone_device")).toString().trimmed();
    const QString mode = root.value(QStringLiteral("system_audio_mode")).toString();
    if (mode == QLatin1String("all") || mode == QLatin1String("app"))
        choices.systemAudioMode = mode;
    choices.systemAudioApplication =
        root.value(QStringLiteral("system_audio_app")).toString().trimmed();
    choices.cameraEnabled =
        root.value(QStringLiteral("camera_enabled")).toBool(choices.cameraEnabled);
    choices.cameraDevice = root.value(QStringLiteral("camera_device")).toString().trimmed();
    return choices;
}

void saveRecordingDeviceChoices(const RecordingDeviceChoices& choices)
{
    QJsonObject root;
    root.insert(QStringLiteral("microphone_enabled"), choices.microphoneEnabled);
    root.insert(QStringLiteral("microphone_device"), choices.microphoneDevice);
    root.insert(QStringLiteral("system_audio_mode"), choices.systemAudioMode);
    root.insert(QStringLiteral("system_audio_app"), choices.systemAudioApplication);
    root.insert(QStringLiteral("camera_enabled"), choices.cameraEnabled);
    root.insert(QStringLiteral("camera_device"), choices.cameraDevice);

    const QString path = recordingDeviceChoicesPath();
    QDir().mkpath(QFileInfo(path).absolutePath());
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate))
        return;
    file.write(QJsonDocument(root).toJson(QJsonDocument::Indented));
}

ShellSettings loadShellSettings()
{
    ShellSettings out;

    QFile file(shellSettingsPath());
    if (!file.open(QIODevice::ReadOnly))
        return out;

    QJsonParseError error{};
    const QJsonDocument document = QJsonDocument::fromJson(file.readAll(), &error);
    if (error.error != QJsonParseError::NoError || !document.isObject())
        return out;

    const QJsonObject root = document.object();

    const QJsonObject general = root.value(QStringLiteral("general")).toObject();
    out.general.startAtLogin =
        general.value(QStringLiteral("start_at_login")).toBool(out.general.startAtLogin);
    out.general.showTrayIcon =
        general.value(QStringLiteral("show_tray_icon")).toBool(out.general.showTrayIcon);
    out.general.theme =
        general.value(QStringLiteral("theme")).toString(out.general.theme);
    out.general.language =
        general.value(QStringLiteral("language")).toString(out.general.language);
    out.general.playSounds =
        general.value(QStringLiteral("play_sounds")).toBool(out.general.playSounds);
    out.general.shutterSound =
        general.value(QStringLiteral("shutter_sound")).toString(out.general.shutterSound);

    const QJsonObject shortcuts = root.value(QStringLiteral("shortcuts")).toObject();
    for (auto it = out.shortcuts.begin(); it != out.shortcuts.end(); ++it) {
        const QJsonValue value = shortcuts.value(it.key());
        if (value.isString())
            it.value() = value.toString();
    }

    const QJsonObject shots = root.value(QStringLiteral("screenshots")).toObject();
    out.screenshots.exportLocation =
        shots.value(QStringLiteral("export_location")).toString().trimmed();
    out.screenshots.format =
        shots.value(QStringLiteral("format")).toString(out.screenshots.format);
    out.screenshots.clipboardMode =
        shots.value(QStringLiteral("clipboard_mode")).toString(out.screenshots.clipboardMode);
    out.screenshots.timerInterval =
        shots.value(QStringLiteral("timer_interval")).toInt(out.screenshots.timerInterval);
    out.screenshots.showCursor =
        shots.value(QStringLiteral("show_cursor")).toBool(out.screenshots.showCursor);
    out.screenshots.afterShowResult =
        shots.value(QStringLiteral("after_show_result")).toBool(out.screenshots.afterShowResult);
    out.screenshots.afterCopyToClipboard =
        shots.value(QStringLiteral("after_copy_to_clipboard"))
            .toBool(out.screenshots.afterCopyToClipboard);
    out.screenshots.afterSave =
        shots.value(QStringLiteral("after_save")).toBool(out.screenshots.afterSave);
    out.screenshots.highDpiSuffix =
        shots.value(QStringLiteral("high_dpi_suffix")).toBool(out.screenshots.highDpiSuffix);

    const QJsonObject recording = root.value(QStringLiteral("recording")).toObject();
    out.recording.showCountdown =
        recording.value(QStringLiteral("show_countdown")).toBool(out.recording.showCountdown);
    out.recording.showControls =
        recording.value(QStringLiteral("show_controls")).toBool(out.recording.showControls);
    out.recording.highDpi =
        recording.value(QStringLiteral("high_dpi")).toBool(out.recording.highDpi);
    out.recording.exportLocation =
        recording.value(QStringLiteral("export_location")).toString().trimmed();
    out.recording.filenamePattern =
        recording.value(QStringLiteral("filename_pattern")).toString();
    out.recording.values.fpsIndex =
        recording.value(QStringLiteral("fps_index")).toInt(out.recording.values.fpsIndex);
    out.recording.values.maxResolutionIndex =
        recording.value(QStringLiteral("max_resolution_index"))
            .toInt(out.recording.values.maxResolutionIndex);
    out.recording.values.qualityIndex =
        recording.value(QStringLiteral("quality_index")).toInt(out.recording.values.qualityIndex);
    out.recording.values.monoAudio =
        recording.value(QStringLiteral("mono_audio")).toBool(out.recording.values.monoAudio);
    out.recording.includeMicrophone =
        recording.value(QStringLiteral("include_microphone"))
            .toBool(out.recording.includeMicrophone);
    out.recording.includeSpeaker =
        recording.value(QStringLiteral("include_speaker")).toBool(out.recording.includeSpeaker);
    out.recording.showCursor =
        recording.value(QStringLiteral("show_cursor")).toBool(out.recording.showCursor);
    out.recording.afterShowResult =
        recording.value(QStringLiteral("after_show_result")).toBool(out.recording.afterShowResult);
    out.recording.afterCopyToClipboard =
        recording.value(QStringLiteral("after_copy_to_clipboard"))
            .toBool(out.recording.afterCopyToClipboard);
    out.recording.afterSave =
        recording.value(QStringLiteral("after_save")).toBool(out.recording.afterSave);
    out.recording.afterOpenEditor =
        recording.value(QStringLiteral("after_open_editor")).toBool(out.recording.afterOpenEditor);

    const QJsonObject webcam = recording.value(QStringLiteral("webcam")).toObject();
    out.recording.webcam.enabled =
        webcam.value(QStringLiteral("enabled")).toBool(out.recording.webcam.enabled);
    out.recording.webcam.device =
        webcam.value(QStringLiteral("device")).toString(out.recording.webcam.device).trimmed();
    out.recording.webcam.sizeIndex =
        webcam.value(QStringLiteral("size_index")).toInt(out.recording.webcam.sizeIndex);
    out.recording.webcam.shapeIndex =
        webcam.value(QStringLiteral("shape_index")).toInt(out.recording.webcam.shapeIndex);
    out.recording.webcam.flip =
        webcam.value(QStringLiteral("flip")).toBool(out.recording.webcam.flip);
    out.recording.webcam.relX =
        webcam.value(QStringLiteral("rel_x")).toDouble(out.recording.webcam.relX);
    out.recording.webcam.relY =
        webcam.value(QStringLiteral("rel_y")).toDouble(out.recording.webcam.relY);

    const QJsonObject quickAccess = root.value(QStringLiteral("quick_access")).toObject();
    out.quickAccess.position =
        quickAccess.value(QStringLiteral("position")).toString(out.quickAccess.position);
    out.quickAccess.overlayScale =
        quickAccess.value(QStringLiteral("overlay_scale")).toDouble(out.quickAccess.overlayScale);
    out.quickAccess.autoCloseEnabled =
        quickAccess.value(QStringLiteral("auto_close_enabled"))
            .toBool(out.quickAccess.autoCloseEnabled);
    out.quickAccess.autoCloseSeconds =
        quickAccess.value(QStringLiteral("auto_close_seconds"))
            .toInt(out.quickAccess.autoCloseSeconds);

    const QJsonObject ocr = root.value(QStringLiteral("ocr")).toObject();
    out.ocr.language = ocr.value(QStringLiteral("language")).toString(out.ocr.language).trimmed();
    out.ocr.keepLineBreaks =
        ocr.value(QStringLiteral("keep_line_breaks")).toBool(out.ocr.keepLineBreaks);
    out.telemetryEnabled = root.value(QStringLiteral("telemetry")).toObject()
                               .value(QStringLiteral("enabled")).toBool(false);

    // --- sanitize ---
    out.screenshots.format = normalizedFormat(out.screenshots.format);
    out.screenshots.clipboardMode = normalizedClipboardMode(out.screenshots.clipboardMode);
    if (!isOneOf(out.screenshots.timerInterval, {0, 1, 3, 5, 10}))
        out.screenshots.timerInterval = 0;

    out.recording.values.sanitize();
    // The recorder indexes the shared shape and size tables with these, so a
    // stale mirror must not send it off the end of either.
    out.recording.webcam.sizeIndex = std::clamp(out.recording.webcam.sizeIndex, 0, 3);
    out.recording.webcam.shapeIndex = std::clamp(out.recording.webcam.shapeIndex, 0, 3);
    out.recording.webcam.relX = std::clamp(out.recording.webcam.relX, 0.0, 1.0);
    out.recording.webcam.relY = std::clamp(out.recording.webcam.relY, 0.0, 1.0);
    const QString pattern = out.recording.filenamePattern.trimmed();
    out.recording.filenamePattern =
        pattern.isEmpty() ? defaultRecordingPattern() : pattern;

    out.quickAccess.position = normalizedPosition(out.quickAccess.position);
    out.quickAccess.overlayScale =
        std::round(std::clamp(out.quickAccess.overlayScale, 0.5, 1.5) * 10.0) / 10.0;
    out.quickAccess.autoCloseSeconds = std::clamp(out.quickAccess.autoCloseSeconds, 5, 120);

    return out;
}

QString shellScreenshotOutputDir(const ShellSettings& settings)
{
    const QString configured = settings.screenshots.exportLocation.trimmed();
    if (!configured.isEmpty())
        return QDir::cleanPath(configured);

    QString pictures = QStandardPaths::writableLocation(QStandardPaths::PicturesLocation);
    if (pictures.isEmpty())
        pictures = QDir::home().filePath(QStringLiteral("Pictures"));
    return QDir::cleanPath(pictures);
}

QString shellRecordingOutputDir(const ShellSettings& settings)
{
    const QString configured = settings.recording.exportLocation.trimmed();
    if (!configured.isEmpty())
        return QDir::cleanPath(configured);
    return QDir::cleanPath(defaultRecordingDirectory());
}

QString shellRecordingOutputPath(const ShellSettings& settings, const QDateTime& now)
{
    QString pattern = settings.recording.filenamePattern.trimmed();
    if (pattern.isEmpty())
        pattern = defaultRecordingPattern();

    QString name = pattern;
    name.replace(QStringLiteral("{Date}"), now.toString(QStringLiteral("yyyy-MM-dd")));
    name.replace(QStringLiteral("{Time}"), now.toString(QStringLiteral("HH-mm-ss")));
    name.replace(QLatin1Char('/'), QLatin1Char('-'));
    name.replace(QLatin1Char('\\'), QLatin1Char('-'));

    return uniquePath(shellRecordingOutputDir(settings), name + QStringLiteral(".mp4"));
}

QString shellTemporaryRecordingOutputPath()
{
    const QString name = QUuid::createUuid().toString(QUuid::WithoutBraces) + QStringLiteral(".mp4");
    return QDir(QDir::tempPath()).filePath(QStringLiteral("ApexShot/") + name);
}

QMimeData* captureClipboardData(const QImage& image, const QString& filePath, const QString& mode)
{
    const QString normalized = normalizedClipboardMode(mode);
    auto* mime = new QMimeData;

    if (normalized != QLatin1String("File Path Only") && !image.isNull())
        mime->setImageData(image);

    if (normalized != QLatin1String("Image Only") && !filePath.isEmpty())
        mime->setUrls({QUrl::fromLocalFile(filePath)});

    if (!mime->hasImage() && !mime->hasUrls()) {
        delete mime;
        return nullptr;
    }
    return mime;
}

void copyCaptureToClipboard(const QImage& image, const QString& filePath, const QString& mode)
{
    if (QMimeData* mime = captureClipboardData(image, filePath, mode))
        QGuiApplication::clipboard()->setMimeData(mime);
}

bool saveScreenshotImage(const QImage& image,
                         const ShellSettings& settings,
                         const QString& directory,
                         qreal devicePixelRatio,
                         QString& outPath,
                         QString& outError)
{
    outPath.clear();
    outError.clear();

    if (image.isNull()) {
        outError = QStringLiteral("empty image");
        return false;
    }

    const QString targetDir = QDir::cleanPath(directory);
    if (!QDir().mkpath(targetDir)) {
        outError = QStringLiteral("could not create %1").arg(QDir::toNativeSeparators(targetDir));
        return false;
    }

    const QString stamp =
        QDateTime::currentDateTime().toString(QStringLiteral("yyyy-MM-dd_HH-mm-ss"));
    const bool highDpi = settings.screenshots.highDpiSuffix && devicePixelRatio > 1.01;
    const QString name = QStringLiteral("ApexShot-%1%2.%3")
                             .arg(stamp,
                                  highDpi ? QStringLiteral("@2x") : QString(),
                                  screenshotExtension(settings.screenshots.format));

    QString path = uniquePath(targetDir, name);
    if (path.isEmpty()) {
        outError = QStringLiteral("no free filename in %1")
                       .arg(QDir::toNativeSeparators(targetDir));
        return false;
    }

    const QString format = normalizedFormat(settings.screenshots.format);
    const QList<QByteArray> supported = QImageWriter::supportedImageFormats();
    if (!supported.contains(format.toLower().toLatin1())) {
        outError = QStringLiteral("%1 encoding is not available on this installation").arg(format);
        return false;
    }

    QImageWriter writer(path, format.toLatin1());
    if (format == QLatin1String("JPEG") || format == QLatin1String("WebP"))
        writer.setQuality(92);

    if (!writer.write(image)) {
        outError = writer.errorString();
        if (outError.isEmpty())
            outError = QStringLiteral("could not write %1").arg(QDir::toNativeSeparators(path));
        return false;
    }

    outPath = path;
    return true;
}

}  // namespace Platform
