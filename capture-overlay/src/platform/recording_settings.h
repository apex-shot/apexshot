#pragma once

#include <QSize>
#include <QStringList>

#include <array>

class QSettings;

/// Recording settings shared by the Windows recorder and, later, its settings
/// surface.
///
/// The tables and their order mirror the Linux settings fields
/// (`rec_video_fps`, `rec_video_max_res`, and `rec_video_quality`) so a value
/// saved on one platform means the same thing on the other. A settings surface
/// may offer only a subset; the recorder always reads the complete set.
namespace Platform {

/// One entry in the "Maximum resolution" list. A zero width means native size.
struct RecordingResolutionOption
{
    const char* label;
    int width;
    int height;
};

/// Frame-rate choices. The index is the stored value, exactly like
/// `rec_video_fps` on Linux.
inline constexpr std::array<int, 4> kRecordingFpsOptions = {24, 30, 50, 60};

/// Maximum-resolution choices. Index 0 is native; every other entry is a
/// ceiling, so a source at or below it is never upscaled.
///
/// Linux appended later targets instead of reordering, so saved configs kept
/// their meaning. Changes to this list must append for the same reason.
inline constexpr std::array<RecordingResolutionOption, 7> kRecordingResolutionOptions = {{
    {"Original", 0, 0},
    {"1080p", 1920, 1080},
    {"720p", 1280, 720},
    {"1440p", 2560, 1440},
    {"900p", 1600, 900},
    {"480p", 854, 480},
    {"2160p", 3840, 2160},
}};

/// Resolution indices in the order the settings surface shows them: Original,
/// then largest to smallest. The index, not this position, is stored.
inline constexpr std::array<int, 7> kRecordingResolutionDisplayOrder = {0, 6, 3, 1, 4, 2, 5};

/// Quality tiers. The index is the stored value, exactly like
/// `rec_video_quality` on Linux.
inline constexpr std::array<const char*, 3> kRecordingQualityOptions = {
    "Balanced",
    "High",
    "Ultra",
};

/// The complete recording settings a session needs.
struct RecordingSettings
{
    /// Index into kRecordingFpsOptions. The default is 30 fps.
    int fpsIndex = 1;
    /// Index into kRecordingResolutionOptions. The default is native size.
    int maxResolutionIndex = 0;
    /// Index into kRecordingQualityOptions. The default is High.
    int qualityIndex = 1;
    /// Record both audio channels as a single mono track.
    bool monoAudio = false;

    /// Put every index back inside its table. Callers use this on load and
    /// before persisting, so a hand-edited or stale config cannot reach a
    /// request.
    void sanitize();
};

/// Frames per second for a stored index. Out-of-range values read as 30 fps.
int recordingFpsForIndex(int index);

/// Maximum output size for a stored index. Out-of-range values and index 0
/// mean native size, returned as an empty size.
QSize recordingMaxResolutionForIndex(int index);

/// x264 CRF for a stored quality index: Balanced 23, High 20, Ultra 16.
/// Out-of-range values read as High.
int recordingCrfForQualityIndex(int index);

/// CRF compensation for a downscaled output: up to -10 below a 2000 px
/// diagonal so capped resolutions stay sharp. Matches the Linux mapping.
int recordingCrfResolutionReduction(int width, int height);

/// Aspect-preserving fit of `source` inside `max`, rounded to even dimensions
/// and never upscaled. An empty or non-positive `max` leaves the source alone.
QSize fitWithinMaxResolution(const QSize& source, const QSize& max);

/// Labels in index order, for a future settings surface. Resolution labels are
/// paired with kRecordingResolutionDisplayOrder.
QStringList recordingFpsLabels();
QStringList recordingQualityLabels();
QStringList recordingResolutionLabels();

/// Read and write the recording group in an explicit store. Values are option
/// indices in the same sense as the Linux config fields.
RecordingSettings loadRecordingSettings(QSettings& settings);
void saveRecordingSettings(QSettings& settings, const RecordingSettings& values);

/// Read and write the application's default QSettings store.
RecordingSettings loadRecordingSettings();
void saveRecordingSettings(const RecordingSettings& settings);

}  // namespace Platform
