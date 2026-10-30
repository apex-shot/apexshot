#include "platform/recording_settings.h"

#include <QSettings>

#include <algorithm>
#include <cmath>

namespace {

constexpr char kFpsKey[] = "recording/video_fps";
constexpr char kMaxResolutionKey[] = "recording/video_max_res";
constexpr char kQualityKey[] = "recording/video_quality";
constexpr char kMonoKey[] = "recording/video_mono";

constexpr int kDefaultFpsIndex = 1;
constexpr int kDefaultMaxResolutionIndex = 0;
constexpr int kDefaultQualityIndex = 1;

template<typename Table>
int clampedIndex(int index, const Table& table, int fallback)
{
    return index >= 0 && index < static_cast<int>(table.size()) ? index : fallback;
}

}  // namespace

namespace Platform {

void RecordingSettings::sanitize()
{
    fpsIndex = clampedIndex(fpsIndex, kRecordingFpsOptions, kDefaultFpsIndex);
    maxResolutionIndex =
        clampedIndex(maxResolutionIndex, kRecordingResolutionOptions, kDefaultMaxResolutionIndex);
    qualityIndex = clampedIndex(qualityIndex, kRecordingQualityOptions, kDefaultQualityIndex);
}

int recordingFpsForIndex(int index)
{
    if (index < 0 || index >= static_cast<int>(kRecordingFpsOptions.size()))
        return kRecordingFpsOptions[kDefaultFpsIndex];
    return kRecordingFpsOptions[static_cast<size_t>(index)];
}

QSize recordingMaxResolutionForIndex(int index)
{
    if (index < 0 || index >= static_cast<int>(kRecordingResolutionOptions.size()))
        return QSize(0, 0);
    const RecordingResolutionOption& option =
        kRecordingResolutionOptions[static_cast<size_t>(index)];
    if (option.width <= 0 || option.height <= 0)
        return QSize(0, 0);
    return QSize(option.width, option.height);
}

int recordingCrfForQualityIndex(int index)
{
    switch (index) {
    case 0:
        return 23;  // Balanced
    case 2:
        return 16;  // Ultra
    default:
        return 20;  // High, and the fallback for stale values.
    }
}

int recordingCrfResolutionReduction(int width, int height)
{
    constexpr double cutoff = 2000.0;
    const double diagonal = std::hypot(static_cast<double>(std::max(0, width)),
                                       static_cast<double>(std::max(0, height)));
    const double reduction = (1.0 - std::min(diagonal, cutoff) / cutoff) * 10.0;
    return static_cast<int>(reduction);
}

QSize fitWithinMaxResolution(const QSize& source, const QSize& max)
{
    if (source.width() <= 0 || source.height() <= 0)
        return source;
    if (max.width() <= 0 || max.height() <= 0)
        return source;
    if (source.width() <= max.width() && source.height() <= max.height())
        return source;

    const double scale = std::min(static_cast<double>(max.width()) / source.width(),
                                  static_cast<double>(max.height()) / source.height());
    int outWidth = std::max(2, static_cast<int>(std::lround(source.width() * scale)));
    int outHeight = std::max(2, static_cast<int>(std::lround(source.height() * scale)));
    outWidth -= outWidth % 2;
    outHeight -= outHeight % 2;
    return QSize(std::max(2, outWidth), std::max(2, outHeight));
}

QSize recordingOutputSize(const QSize& source, const QSize& max, double displayScale)
{
    if (source.width() < 2 || source.height() < 2)
        return QSize();

    double scale = std::isfinite(displayScale) && displayScale > 0.0
        ? std::min(1.0, displayScale) : 1.0;
    QSize ceiling = max;
    if (source.height() > source.width() && ceiling.width() > ceiling.height())
        ceiling.transpose();
    if (ceiling.width() > 0 && ceiling.height() > 0) {
        scale = std::min(scale, static_cast<double>(ceiling.width()) / source.width());
        scale = std::min(scale, static_cast<double>(ceiling.height()) / source.height());
    }
    const int width = std::max(2, static_cast<int>(std::floor(source.width() * scale)) & ~1);
    const int height = std::max(2, static_cast<int>(std::floor(source.height() * scale)) & ~1);
    return QSize(width, height);
}

QString recordingScaleFilter(const QSize& outputSize)
{
    return QStringLiteral("scale=%1:%2").arg(outputSize.width()).arg(outputSize.height());
}

QStringList recordingFpsLabels()
{
    QStringList labels;
    for (const int fps : kRecordingFpsOptions)
        labels.append(QStringLiteral("%1 FPS").arg(fps));
    return labels;
}

QStringList recordingQualityLabels()
{
    QStringList labels;
    for (const char* label : kRecordingQualityOptions)
        labels.append(QString::fromLatin1(label));
    return labels;
}

QStringList recordingResolutionLabels()
{
    QStringList labels;
    for (const RecordingResolutionOption& option : kRecordingResolutionOptions)
        labels.append(QString::fromLatin1(option.label));
    return labels;
}

RecordingSettings loadRecordingSettings(QSettings& settings)
{
    RecordingSettings out;
    out.fpsIndex = settings.value(kFpsKey, kDefaultFpsIndex).toInt();
    out.maxResolutionIndex = settings.value(kMaxResolutionKey, kDefaultMaxResolutionIndex).toInt();
    out.qualityIndex = settings.value(kQualityKey, kDefaultQualityIndex).toInt();
    out.monoAudio = settings.value(kMonoKey, false).toBool();
    out.sanitize();
    return out;
}

void saveRecordingSettings(QSettings& settings, const RecordingSettings& input)
{
    RecordingSettings out = input;
    out.sanitize();
    settings.setValue(kFpsKey, out.fpsIndex);
    settings.setValue(kMaxResolutionKey, out.maxResolutionIndex);
    settings.setValue(kQualityKey, out.qualityIndex);
    settings.setValue(kMonoKey, out.monoAudio);
    settings.sync();
}

RecordingSettings loadRecordingSettings()
{
    QSettings settings;
    return loadRecordingSettings(settings);
}

void saveRecordingSettings(const RecordingSettings& settings)
{
    QSettings store;
    saveRecordingSettings(store, settings);
}

}  // namespace Platform
