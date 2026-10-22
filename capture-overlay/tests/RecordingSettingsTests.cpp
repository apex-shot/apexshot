// Unit coverage for the Windows recording settings tables and persistence.
//
// These checks mirror the Linux mappings, so a value saved on one platform
// keeps its meaning when the Windows settings surface arrives.

#include "platform/recording_settings.h"

#include <QCoreApplication>
#include <QSettings>
#include <QTemporaryDir>

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

void checkDefaults()
{
    Platform::RecordingSettings settings;
    settings.sanitize();
    CHECK(settings.fpsIndex == 1);            // 30 fps
    CHECK(settings.maxResolutionIndex == 0);  // Original
    CHECK(settings.qualityIndex == 1);        // High
    CHECK(!settings.monoAudio);
}

void checkFpsTable()
{
    CHECK(Platform::recordingFpsForIndex(0) == 24);
    CHECK(Platform::recordingFpsForIndex(1) == 30);
    CHECK(Platform::recordingFpsForIndex(2) == 50);
    CHECK(Platform::recordingFpsForIndex(3) == 60);
    CHECK(Platform::recordingFpsForIndex(-1) == 30);
    CHECK(Platform::recordingFpsForIndex(4) == 30);
    CHECK(Platform::recordingFpsLabels().size() == 4);
    CHECK(Platform::recordingFpsLabels().at(2) == QStringLiteral("50 FPS"));
}

void checkResolutionTable()
{
    CHECK(Platform::recordingMaxResolutionForIndex(0).isEmpty());
    CHECK(Platform::recordingMaxResolutionForIndex(1) == QSize(1920, 1080));
    CHECK(Platform::recordingMaxResolutionForIndex(2) == QSize(1280, 720));
    CHECK(Platform::recordingMaxResolutionForIndex(3) == QSize(2560, 1440));
    CHECK(Platform::recordingMaxResolutionForIndex(4) == QSize(1600, 900));
    CHECK(Platform::recordingMaxResolutionForIndex(5) == QSize(854, 480));
    CHECK(Platform::recordingMaxResolutionForIndex(6) == QSize(3840, 2160));
    CHECK(Platform::recordingMaxResolutionForIndex(-1).isEmpty());
    CHECK(Platform::recordingMaxResolutionForIndex(7).isEmpty());

    const QStringList labels = Platform::recordingResolutionLabels();
    CHECK(labels.size() == 7);
    CHECK(labels.at(0) == QStringLiteral("Original"));
    CHECK(labels.at(6) == QStringLiteral("2160p"));

    // The display order maps back to the same labels the indices describe.
    CHECK(Platform::kRecordingResolutionDisplayOrder.size() == labels.size());
    CHECK(labels.at(Platform::kRecordingResolutionDisplayOrder.at(1)) == QStringLiteral("2160p"));
}

void checkQualityTable()
{
    CHECK(Platform::recordingCrfForQualityIndex(0) == 23);
    CHECK(Platform::recordingCrfForQualityIndex(1) == 20);
    CHECK(Platform::recordingCrfForQualityIndex(2) == 16);
    CHECK(Platform::recordingCrfForQualityIndex(-1) == 20);
    CHECK(Platform::recordingCrfForQualityIndex(9) == 20);

    const QStringList labels = Platform::recordingQualityLabels();
    CHECK(labels.size() == 3);
    CHECK(labels.at(0) == QStringLiteral("Balanced"));
    CHECK(labels.at(2) == QStringLiteral("Ultra"));
}

void checkResolutionCompensation()
{
    CHECK(Platform::recordingCrfResolutionReduction(1920, 1080) == 0);
    CHECK(Platform::recordingCrfResolutionReduction(1920, 1200) == 0);
    CHECK(Platform::recordingCrfResolutionReduction(1280, 720) == 2);
    CHECK(Platform::recordingCrfResolutionReduction(640, 480) == 6);
}

void checkFitTable()
{
    const QSize fullHd(1920, 1080);
    CHECK(Platform::fitWithinMaxResolution(QSize(1920, 1200), QSize(2560, 1440)) ==
          QSize(1920, 1200));
    CHECK(Platform::fitWithinMaxResolution(QSize(1920, 1200), QSize(1920, 1080)) ==
          QSize(1728, 1080));
    CHECK(Platform::fitWithinMaxResolution(QSize(1920, 1080), QSize(1280, 720)) ==
          QSize(1280, 720));
    // A target at or above the source never upscales.
    CHECK(Platform::fitWithinMaxResolution(QSize(640, 480), fullHd) == QSize(640, 480));
    CHECK(Platform::fitWithinMaxResolution(QSize(640, 480), QSize()) == QSize(640, 480));
    CHECK(Platform::fitWithinMaxResolution(QSize(), fullHd).isEmpty());

    // Odd sizes are rounded down to encoder-safe even dimensions.
    const QSize odd = Platform::fitWithinMaxResolution(QSize(1921, 1081), fullHd);
    CHECK(odd.width() % 2 == 0);
    CHECK(odd.height() % 2 == 0);
    CHECK(odd.width() <= fullHd.width());
    CHECK(odd.height() <= fullHd.height());
}

void checkPersistence(const QString& path)
{
    {
        QSettings store(path, QSettings::IniFormat);
        Platform::RecordingSettings saved;
        saved.fpsIndex = 3;
        saved.maxResolutionIndex = 6;
        saved.qualityIndex = 2;
        saved.monoAudio = true;
        Platform::saveRecordingSettings(store, saved);
    }

    QSettings store(path, QSettings::IniFormat);
    const Platform::RecordingSettings loaded = Platform::loadRecordingSettings(store);
    CHECK(loaded.fpsIndex == 3);
    CHECK(loaded.maxResolutionIndex == 6);
    CHECK(loaded.qualityIndex == 2);
    CHECK(loaded.monoAudio);
}

void checkLoadSanitizes(const QString& path)
{
    {
        QSettings store(path, QSettings::IniFormat);
        store.setValue(QStringLiteral("recording/video_fps"), 9);
        store.setValue(QStringLiteral("recording/video_max_res"), -4);
        store.setValue(QStringLiteral("recording/video_quality"), 42);
        store.sync();
    }

    QSettings store(path, QSettings::IniFormat);
    const Platform::RecordingSettings loaded = Platform::loadRecordingSettings(store);
    CHECK(loaded.fpsIndex == 1);
    CHECK(loaded.maxResolutionIndex == 0);
    CHECK(loaded.qualityIndex == 1);
}

}  // namespace

int main(int argc, char** argv)
{
    QCoreApplication app(argc, argv);
    QCoreApplication::setOrganizationName(QStringLiteral("ApexShotTests"));
    QCoreApplication::setApplicationName(QStringLiteral("RecordingSettingsTests"));

    checkDefaults();
    checkFpsTable();
    checkResolutionTable();
    checkQualityTable();
    checkResolutionCompensation();
    checkFitTable();

    QTemporaryDir directory;
    if (!directory.isValid()) {
        std::printf("FAIL: could not create a temporary directory\n");
        return 1;
    }
    checkPersistence(directory.filePath(QStringLiteral("settings.ini")));
    checkLoadSanitizes(directory.filePath(QStringLiteral("stale.ini")));

    if (failures == 0) {
        std::printf("OK: recording settings\n");
        return 0;
    }
    std::printf("FAIL: %d recording settings check(s)\n", failures);
    return 1;
}
