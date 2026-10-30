// Unit coverage for the Windows recording settings tables and persistence.
//
// These checks mirror the Linux mappings, so a value saved on one platform
// keeps its meaning when the Windows settings surface arrives.

#include "platform/recording_settings.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QProcess>
#include <QSettings>
#include <QStandardPaths>
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
    CHECK(labels.at(6) == QStringLiteral("2160p (4K)"));

    // The display order maps back to the same labels the indices describe.
    CHECK(Platform::kRecordingResolutionDisplayOrder.size() == labels.size());
    CHECK(labels.at(Platform::kRecordingResolutionDisplayOrder.at(1)) ==
          QStringLiteral("2160p (4K)"));
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

void checkRecordingOutputSizes()
{
    const QSize fourK(3840, 2160);
    CHECK(Platform::recordingOutputSize(fourK, QSize()) == fourK);
    CHECK(Platform::recordingOutputSize(fourK, fourK) == fourK);
    CHECK(Platform::recordingOutputSize(QSize(7680, 4320), fourK) == fourK);
    CHECK(Platform::recordingOutputSize(QSize(2160, 3840), fourK) == QSize(2160, 3840));
    CHECK(Platform::recordingOutputSize(QSize(4320, 7680), fourK) == QSize(2160, 3840));
    CHECK(Platform::recordingOutputSize(fourK, QSize(1920, 1080)) == QSize(1920, 1080));
    CHECK(Platform::recordingOutputSize(fourK, fourK, 0.5) == QSize(1920, 1080));
    CHECK(Platform::recordingOutputSize(QSize(1920, 1200), QSize(1920, 1080)) ==
          QSize(1728, 1080));
    CHECK(Platform::recordingOutputSize(QSize(1921, 1081), QSize()) == QSize(1920, 1080));
    CHECK(Platform::recordingOutputSize(QSize(640, 480), fourK) == QSize(640, 480));
    CHECK(Platform::recordingOutputSize(QSize(1, 100), fourK).isEmpty());
    CHECK(Platform::recordingScaleFilter(fourK) == QStringLiteral("scale=3840:2160"));
}

void checkSynthetic4k(const QString& directory)
{
    if (qEnvironmentVariableIsEmpty("APEXSHOT_TEST_4K"))
        return;
    const QString ffmpeg = QStandardPaths::findExecutable(QStringLiteral("ffmpeg"));
    const QString ffprobe = QStandardPaths::findExecutable(QStringLiteral("ffprobe"));
    if (ffmpeg.isEmpty() || ffprobe.isEmpty()) {
        CHECK(false && "ffmpeg and ffprobe are required for the synthetic 4K test");
        return;
    }
    for (const QSize& ceiling : {QSize(), QSize(1920, 1080)}) {
        const QSize outputSize = Platform::recordingOutputSize(QSize(3840, 2160), ceiling);
        const QString path = QDir(directory).filePath(
            QStringLiteral("synthetic-%1.mp4").arg(outputSize.width()));
        QProcess encode;
        encode.start(ffmpeg, {QStringLiteral("-v"), QStringLiteral("error"),
                              QStringLiteral("-f"), QStringLiteral("lavfi"),
                              QStringLiteral("-i"), QStringLiteral("testsrc2=size=3840x2160:rate=30"),
                              QStringLiteral("-frames:v"), QStringLiteral("2"),
                              QStringLiteral("-vf"), Platform::recordingScaleFilter(outputSize),
                              QStringLiteral("-c:v"), QStringLiteral("libx264"),
                              QStringLiteral("-preset"), QStringLiteral("ultrafast"),
                              QStringLiteral("-threads"), QStringLiteral("2"), path});
        CHECK(encode.waitForFinished(60000));
        CHECK(encode.exitStatus() == QProcess::NormalExit && encode.exitCode() == 0);
        if (!QFileInfo::exists(path))
            continue;
        QProcess probe;
        probe.start(ffprobe, {QStringLiteral("-v"), QStringLiteral("error"),
                              QStringLiteral("-select_streams"), QStringLiteral("v:0"),
                              QStringLiteral("-show_entries"), QStringLiteral("stream=width,height,r_frame_rate"),
                              QStringLiteral("-of"), QStringLiteral("csv=p=0"), path});
        CHECK(probe.waitForFinished(10000));
        CHECK(probe.exitStatus() == QProcess::NormalExit && probe.exitCode() == 0);
        const QByteArray expected = QStringLiteral("%1,%2,30/1")
                                        .arg(outputSize.width()).arg(outputSize.height()).toUtf8();
        CHECK(probe.readAllStandardOutput().trimmed() == expected);
    }
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
    checkRecordingOutputSizes();

    QTemporaryDir directory;
    if (!directory.isValid()) {
        std::printf("FAIL: could not create a temporary directory\n");
        return 1;
    }
    checkPersistence(directory.filePath(QStringLiteral("settings.ini")));
    checkLoadSanitizes(directory.filePath(QStringLiteral("stale.ini")));
    checkSynthetic4k(directory.path());

    if (failures == 0) {
        std::printf("OK: recording settings\n");
        return 0;
    }
    std::printf("FAIL: %d recording settings check(s)\n", failures);
    return 1;
}
