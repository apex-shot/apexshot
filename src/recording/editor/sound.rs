//! The editor's sound: the microphone modes, the background music and the
//! click sounds, turned into the ffmpeg graph that the export and the preview
//! both read.
//!
//! A recording carries one mixed audio stream, so the plan has a single voice
//! track: the recording's own sound, cut and retimed the way the timeline plays
//! it. Music loops underneath from the start of the composition, and the soft
//! click lands on the recorded presses.

use super::model::{AudioMode, VideoEditState};
use super::sidecar::{PointerSidecar, PressSample};
use crate::i18n::t;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The stereo float format every stage of a sound graph speaks.
pub const FORMAT: &str = "aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo";

/// The bundled click, written out when a graph carries cues.
pub const CLICK_WAV: &[u8] = include_bytes!("../../../assets/sounds/click-soft.wav");

/// A press shorter than this pairs into one click; longer gives a down and an
/// up.
const CLICK_PAIR_WINDOW_SECONDS: f64 = 0.25;

/// A recorded click this close to a press is that press, not a second sound.
const LEGACY_CLICK_MATCH_SECONDS: f64 = 1e-6;

/// Sound is not cut out of a span playing faster than this: the cue would land
/// on motion the eye cannot follow.
const MAX_CUE_SPEED: f64 = 2.0;

/// How close two composition times have to be to count as one place.
const GAP_EPSILON: f64 = 1e-6;

/// How the microphone is processed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MicPreset {
    /// The recording keeps its own sound.
    #[default]
    Off,
    Standard,
    Loud,
    NoiseReduction,
    Podcast,
    Warm,
}

impl MicPreset {
    /// Every mode in the order the panel lays its tiles out.
    pub const ALL: [MicPreset; 6] = [
        MicPreset::Standard,
        MicPreset::Loud,
        MicPreset::NoiseReduction,
        MicPreset::Podcast,
        MicPreset::Warm,
        MicPreset::Off,
    ];

    pub fn label(self) -> String {
        match self {
            Self::Standard => t("Standard"),
            Self::Loud => t("Loud"),
            Self::NoiseReduction => t("Noise reduction"),
            Self::Podcast => t("Podcast"),
            Self::Warm => t("Warm"),
            Self::Off => t("Off"),
        }
    }

    pub fn summary(self) -> String {
        match self {
            Self::Standard => t("Even, natural voice"),
            Self::Loud => t("Punchy and compressed"),
            Self::NoiseReduction => t("Removes steady noise"),
            Self::Podcast => t("Warm, polished voice"),
            Self::Warm => t("Fuller, deeper tone"),
            Self::Off => t("As recorded"),
        }
    }

    pub fn description(self) -> String {
        match self {
            Self::Standard => t("Evens out your voice so it stays at one level."),
            Self::Loud => t("Louder, compressed voice that cuts through music."),
            Self::NoiseReduction => t("Removes steady background noise, then evens out the voice."),
            Self::Podcast => t("Full, warm voice with softened hiss and sibilance."),
            Self::Warm => t("Adds body to thin microphones."),
            Self::Off => t("Keeps the microphone exactly as it was recorded."),
        }
    }

    /// The filter chain this mode applies to the voice track, if any.
    pub fn filter(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Standard => Some("speechnorm=e=4:r=0.0005:l=1"),
            Self::Loud => Some("highpass=f=70,speechnorm=e=12.5:r=0.0001:l=1,acompressor=threshold=0.125:ratio=3:attack=5:release=120:makeup=2,alimiter=limit=0.95"),
            Self::NoiseReduction => Some("highpass=f=80,afftdn=nr=18:nf=-40:tn=1,speechnorm=e=12.5:r=0.0001:l=1,acompressor=threshold=0.125:ratio=3:attack=5:release=120:makeup=2,alimiter=limit=0.95"),
            Self::Podcast => Some("highpass=f=80,afftdn=nr=12:nf=-45,equalizer=f=150:t=q:w=1:g=3,equalizer=f=4500:t=q:w=1.5:g=2,deesser,speechnorm=e=8:r=0.0002:l=1,acompressor=threshold=0.1:ratio=3:attack=10:release=150:makeup=2,alimiter=limit=0.95"),
            Self::Warm => Some("highpass=f=60,equalizer=f=220:t=q:w=1:g=3,equalizer=f=7000:t=q:w=1:g=-3,speechnorm=e=4:r=0.0005:l=1"),
        }
    }
}

/// The project's sound.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundSettings {
    /// Master volume, 0..=1. Default 1.
    pub volume: f64,
    pub microphone_preset: MicPreset,
    /// A music file played under the whole composition, looped.
    pub background_file: Option<PathBuf>,
    /// 0..=1, default 0.05.
    pub background_volume: f64,
    pub mute_background: bool,
    /// `None` is Off; `Some("soft")` is the bundled click. Any other value is
    /// dropped when the settings are clamped.
    pub click_sound: Option<String>,
    /// 0..=1, default 0.25.
    pub click_sound_volume: f64,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            volume: 1.0,
            // A Linux recording is kept as it was made unless a mode is picked.
            microphone_preset: MicPreset::Off,
            background_file: None,
            background_volume: 0.05,
            mute_background: false,
            click_sound: None,
            click_sound_volume: 0.25,
        }
    }
}

impl SoundSettings {
    /// Volumes clamped to 0..=1 (non-finite -> default) and an unknown click
    /// sound read as Off; everything else kept.
    pub fn clamped(self) -> Self {
        let click_sound = match self.click_sound.as_deref() {
            Some(CLICK_SOFT) => Some(CLICK_SOFT.to_string()),
            _ => None,
        };
        Self {
            volume: clamp_unit(self.volume, 1.0),
            background_volume: clamp_unit(self.background_volume, 0.05),
            click_sound_volume: clamp_unit(self.click_sound_volume, 0.25),
            click_sound,
            ..self
        }
    }

    /// Whether background music will actually play: a file is set, it is not
    /// muted and its volume is above zero.
    pub fn background_active(&self) -> bool {
        self.background_file.is_some() && !self.mute_background && self.background_volume > 0.0
    }

    /// Whether the shipped click sound is on with an audible volume.
    pub fn click_active(&self) -> bool {
        self.click_sound.as_deref() == Some(CLICK_SOFT) && self.click_sound_volume > 0.0
    }

    /// Whether export must lay the composition's sound over the finished video.
    pub fn shapes_export(&self) -> bool {
        self.volume != 1.0
            || self.microphone_preset.filter().is_some()
            || self.background_active()
            || self.click_active()
    }
}

/// The one click sound the editor ships.
pub const CLICK_SOFT: &str = "soft";

/// A level that stays inside 0..=1; a value that is not a number reads as the
/// given default.
fn clamp_unit(value: f64, default: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        default
    }
}

/// Play the soft click once at `volume` (0..=1), off the main thread. Best
/// effort: a missing player or an unwritable temp folder stays silent.
pub fn preview_click(volume: f64) {
    if !volume.is_finite() {
        return;
    }
    let volume = volume.clamp(0.0, 1.0);
    std::thread::spawn(move || play_preview_click(volume));
}

fn play_preview_click(volume: f64) {
    let Some(wav) = scaled(CLICK_WAV, volume) else {
        return;
    };
    let path = click_preview_path(volume);
    if !path.is_file() && std::fs::write(&path, wav).is_err() {
        return;
    }
    // Wait for the player here rather than in the caller: this thread exists
    // for the sound, and waiting reaps the child instead of leaving it behind.
    let _ = std::process::Command::new("sh")
        .arg("-c")
        .arg(
            "if command -v pw-play >/dev/null 2>&1; then pw-play \"$1\"; \
             elif command -v paplay >/dev/null 2>&1; then paplay \"$1\"; \
             elif command -v aplay >/dev/null 2>&1; then aplay \"$1\"; \
             else exit 127; fi",
        )
        .arg("sh")
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// One scaled copy of the click per volume, so a slider that settles on the
/// same level does not rewrite the file on every tick.
fn click_preview_path(volume: f64) -> PathBuf {
    let milli = (volume * 1000.0).round() as u32;
    std::env::temp_dir().join(format!(
        "apexshot-click-preview-{}-{milli:03}.wav",
        std::process::id()
    ))
}

/// A 16-bit PCM wav with every sample scaled by `volume`. None for a wav this
/// cannot read safely, which leaves the click silent rather than garbled.
fn scaled(wav: &[u8], volume: f64) -> Option<Vec<u8>> {
    const HEADER: usize = 44;
    let valid = wav.len() >= HEADER
        && &wav[0..4] == b"RIFF"
        && &wav[8..12] == b"WAVE"
        && &wav[12..16] == b"fmt "
        && u16::from_le_bytes([wav[34], wav[35]]) == 16
        && &wav[36..40] == b"data";
    if !valid {
        return None;
    }
    let mut out = wav.to_vec();
    for sample in out[HEADER..].as_chunks_mut::<2>().0 {
        let value = f64::from(i16::from_le_bytes([sample[0], sample[1]])) * volume;
        sample.copy_from_slice(&(value.round() as i16).to_le_bytes());
    }
    Some(out)
}

/// One kept stretch of source sound, in composition order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundRange {
    /// Source seconds.
    pub start: f64,
    pub end: f64,
    pub speed: f64,
    /// The exact composition length this range fills.
    pub duration: f64,
    /// The segment index the range belongs to.
    pub segment: usize,
    /// The range plays silence: its piece is muted, or it fills a gap where no
    /// footage sits.
    pub muted: bool,
}

impl SoundRange {
    /// The gain this range plays at.
    pub fn gain(&self) -> f64 {
        if self.muted {
            0.0
        } else {
            1.0
        }
    }
}

/// One kept piece of the composition on the timeline, before it becomes a
/// range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundSlot {
    pub segment: usize,
    /// Where the piece starts on the composition ruler.
    pub composition_start: f64,
    pub source_start: f64,
    pub source_end: f64,
    pub speed: f64,
    pub muted: bool,
}

/// The slots of a state, in composition order. A piece the timeline drops is
/// not here at all; the gap it leaves becomes silence.
pub fn slots_of(state: &VideoEditState) -> Vec<SoundSlot> {
    state
        .placed_segment_slots()
        .into_iter()
        .map(
            |(index, composition_start, source_start, source_end)| SoundSlot {
                segment: index,
                composition_start,
                source_start,
                source_end,
                speed: state.segment_speed(index),
                muted: state.segment_is_muted(index),
            },
        )
        .collect()
}

/// The ranges of a composition. A gap before or between pieces becomes a range
/// with an empty source trim, which the chain pads to the gap's length, so the
/// sound after the gap keeps its place. A hold at the end needs no range: the
/// track is padded to the composition's length anyway.
pub fn sound_ranges(slots: &[SoundSlot]) -> Vec<SoundRange> {
    let mut ranges: Vec<SoundRange> = Vec::new();
    let mut elapsed = 0.0f64;
    for slot in slots {
        let start = slot.composition_start.max(0.0);
        if start > elapsed + GAP_EPSILON {
            ranges.push(SoundRange {
                // An empty trim at the piece's own source position: the chain
                // pads it to the gap's length rather than repeating sound.
                start: slot.source_start,
                end: slot.source_start,
                speed: 1.0,
                duration: start - elapsed,
                segment: slot.segment,
                muted: true,
            });
        }
        let speed = if slot.speed.is_finite() && slot.speed > 0.0 {
            slot.speed
        } else {
            1.0
        };
        ranges.push(SoundRange {
            start: slot.source_start,
            end: slot.source_end,
            speed,
            duration: (slot.source_end - slot.source_start).max(0.0) / speed,
            segment: slot.segment,
            muted: slot.muted,
        });
        elapsed = start + (slot.source_end - slot.source_start).max(0.0) / speed;
    }
    ranges
}

/// `ranges` as they play from composition time `from` on: ranges that end at or
/// before `from` are dropped, the one containing it starts part-way through.
pub fn ranges_from(ranges: &[SoundRange], from: f64) -> Vec<SoundRange> {
    let mut played: Vec<SoundRange> = Vec::new();
    let mut elapsed = 0.0f64;
    for range in ranges {
        let starts_at = elapsed;
        elapsed += range.duration;
        if elapsed <= from {
            continue;
        }
        if starts_at >= from {
            played.push(*range);
            continue;
        }
        let into = from - starts_at;
        // A gap carries an empty trim already; its end moves with its start so
        // a part-way start keeps it empty instead of inverting the trim.
        let start = range.start + into * range.speed;
        let end = if range.end > range.start {
            range.end
        } else {
            start
        };
        played.push(SoundRange {
            start,
            end,
            duration: range.duration - into,
            ..*range
        });
    }
    played
}

/// One ffmpeg input of a sound graph. A runner passes, in this order:
/// `-stream_loop -1` when `looped`, `-ss {seek:.6}` when `seek > 0`, then
/// `-i path`.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphInput {
    pub path: PathBuf,
    /// Read with `-stream_loop -1` (background music).
    pub looped: bool,
    /// Input seek in seconds; the graph's trims are already relative to it.
    pub seek: f64,
}

/// A filter graph producing the composed sound on the output label `[a]`:
/// 48 kHz, stereo, float, exactly `duration` seconds long.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundGraph {
    /// Inputs in order; the graph refers to them as `[first_input + i:a:0]`.
    pub inputs: Vec<GraphInput>,
    pub graph: String,
}

/// A click cue: composition milliseconds and gain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickCue {
    pub delay_ms: u64,
    pub gain: f64,
}

/// Everything that decides what the composition sounds like, built once per
/// change. Comparing two plans tells the preview player whether to restart.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundPlan {
    source: PathBuf,
    /// Whether the composition plays the recording's own track at all.
    has_audio: bool,
    settings: SoundSettings,
    /// From composition time 0.
    ranges: Vec<SoundRange>,
    /// The composition's whole length in seconds (freeze tail included).
    duration: f64,
    /// From composition time 0.
    cues: Vec<ClickCue>,
    mono: bool,
}

impl SoundPlan {
    /// The plan of a composition. Cues are made only when the click sound is
    /// the shipped one, its volume is above zero and the sidecar has presses.
    pub fn from_state(state: &VideoEditState) -> Self {
        let settings = state.sound.clone().clamped();
        let slots = slots_of(state);
        let cues = state
            .sidecar
            .as_ref()
            .filter(|_| settings.click_active())
            .map_or_else(Vec::new, |sidecar| {
                click_cues(sidecar, &slots, settings.click_sound_volume)
            });
        Self {
            source: state.metadata.path.clone(),
            // One mixed track: when the recording has no sound, or the whole
            // composition is muted, there is no voice to cut.
            has_audio: state.has_audio_track() && !state.is_muted(),
            settings,
            ranges: sound_ranges(&slots),
            duration: state.composition_duration(),
            cues,
            mono: state.audio_mode == AudioMode::Mono,
        }
    }

    /// A text fingerprint of everything the plan was built from, for change
    /// detection.
    pub fn key(&self) -> String {
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            self.source,
            self.has_audio,
            self.settings,
            self.ranges,
            self.duration,
            self.cues,
            self.mono
        )
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn mono(&self) -> bool {
        self.mono
    }

    /// Whether the plan carries clicks, so a runner knows to write the wav.
    pub fn has_cues(&self) -> bool {
        !self.cues.is_empty()
    }

    /// The graph that plays the composition from composition time `from` to
    /// its end. None when nothing would be heard or `from` is past the end.
    pub fn graph_from(
        &self,
        from: f64,
        click: Option<&Path>,
        first_input: usize,
    ) -> Option<SoundGraph> {
        if !from.is_finite() || from < 0.0 || from >= self.duration {
            return None;
        }
        let ranges = ranges_from(&self.ranges, from);
        let cues: Vec<ClickCue> = self
            .cues
            .iter()
            .filter_map(|cue| {
                let shifted = cue.delay_ms as f64 - from * 1000.0;
                (shifted >= 0.0).then_some(ClickCue {
                    delay_ms: shifted.round() as u64,
                    gain: cue.gain,
                })
            })
            .collect();
        build_graph(
            &self.source,
            self.has_audio,
            &self.settings,
            &ranges,
            self.duration - from,
            &cues,
            click,
            from,
            first_input,
        )
    }
}

/// The clicks of a recording as cues on the composition. Only pieces the
/// timeline plays inside the double-speed window carry a cue.
fn click_cues(sidecar: &PointerSidecar, slots: &[SoundSlot], volume: f64) -> Vec<ClickCue> {
    let spans: Vec<ClickSpan> = slots
        .iter()
        .filter(|slot| slot.speed > 0.0)
        .map(|slot| ClickSpan {
            source_start: slot.source_start,
            source_end: slot.source_end,
            composition_start: slot.composition_start,
            speed: slot.speed,
        })
        .collect();
    click_events(sidecar, &spans)
        .into_iter()
        .map(|event| {
            let scale = match event.kind {
                ClickKind::Click => 1.0,
                ClickKind::Down => 0.8,
                ClickKind::Up => 0.6,
            };
            ClickCue {
                delay_ms: (event.composition_time * 1000.0).round() as u64,
                gain: volume * scale,
            }
        })
        .collect()
}

/// Build the graph. `first_input` is the ffmpeg index of the first sound input
/// (1 in the export mux, whose input 0 is the video; 0 in the preview player).
/// `click` is the click-sound WAV read when `cues` is non-empty. None when
/// nothing would be heard.
#[allow(clippy::too_many_arguments)]
fn build_graph(
    source: &Path,
    has_audio: bool,
    settings: &SoundSettings,
    ranges: &[SoundRange],
    duration: f64,
    cues: &[ClickCue],
    click: Option<&Path>,
    background_offset: f64,
    first_input: usize,
) -> Option<SoundGraph> {
    if !duration.is_finite() || duration <= 0.0 {
        return None;
    }
    let mut inputs: Vec<GraphInput> = Vec::new();
    let mut parts: Vec<String> = Vec::new();
    let mut labels: Vec<String> = Vec::new();

    let voice_plays = has_audio && ranges.iter().any(|range| range.gain() > 0.0);
    if voice_plays {
        let input = first_input + inputs.len();
        let seek = source_seek(ranges);
        inputs.push(GraphInput {
            path: source.to_path_buf(),
            looped: false,
            seek,
        });
        push_track(
            &mut parts,
            Track {
                input,
                tag: 'm',
                label: "mic",
                ranges,
                seek,
                preset: settings.microphone_preset.filter(),
                duration,
            },
        );
        labels.push("[mic]".to_owned());
    }

    if settings.background_active() {
        if let Some(path) = settings
            .background_file
            .as_deref()
            .filter(|path| path.is_file())
        {
            let input = first_input + inputs.len();
            inputs.push(GraphInput {
                path: path.to_path_buf(),
                looped: true,
                seek: 0.0,
            });
            let mut chain = format!("[{input}:a:0]{FORMAT}");
            if background_offset > 0.0 {
                chain.push_str(&format!(
                    ",atrim=start={background_offset:.6},asetpts=PTS-STARTPTS"
                ));
            }
            chain.push_str(&format!(
                ",volume={:.4},atrim=end={duration:.6}[bg]",
                settings.background_volume
            ));
            parts.push(chain);
            labels.push("[bg]".to_owned());
        }
    }

    if !cues.is_empty() {
        if let Some(path) = click {
            let input = first_input + inputs.len();
            inputs.push(GraphInput {
                path: path.to_path_buf(),
                looped: false,
                seek: 0.0,
            });
            let count = cues.len();
            let mut split = format!("[{input}:a:0]{FORMAT},asplit={count}");
            for index in 0..count {
                split.push_str(&format!("[c{index}]"));
            }
            parts.push(split);
            for (index, cue) in cues.iter().enumerate() {
                parts.push(format!(
                    "[c{index}]adelay={}:all=1,volume={:.4}[e{index}]",
                    cue.delay_ms, cue.gain
                ));
                labels.push(format!("[e{index}]"));
            }
        }
    }

    if labels.is_empty() {
        return None;
    }
    if labels.len() == 1 {
        parts.push(format!(
            "{}volume={:.4},atrim=end={duration:.6},{FORMAT}[a]",
            labels[0], settings.volume
        ));
    } else {
        let mut mix = labels.concat();
        mix.push_str(&format!(
            "amix=inputs={}:normalize=0:duration=longest:dropout_transition=0,volume={:.4},atrim=end={duration:.6},{FORMAT}[a]",
            labels.len(),
            settings.volume
        ));
        parts.push(mix);
    }
    Some(SoundGraph {
        inputs,
        graph: parts.join(";"),
    })
}

/// Where a source track starts reading: a second before its first range, so a
/// trim at the very start of the graph never asks for a negative time.
fn source_seek(ranges: &[SoundRange]) -> f64 {
    let first = ranges
        .iter()
        .map(|range| range.start)
        .fold(f64::INFINITY, f64::min);
    (first - 1.0).max(0.0)
}

/// One source track's place in the graph: the input it reads, the labels it
/// uses, the ranges it plays, the seek its trims are relative to, and its
/// preset.
struct Track<'a> {
    input: usize,
    tag: char,
    label: &'a str,
    ranges: &'a [SoundRange],
    seek: f64,
    preset: Option<&'static str>,
    duration: f64,
}

/// One source track: every range cut out of the shared input, retimed to its
/// own speed, scaled by its piece's gain and joined back to back, then padded
/// to the composition's length.
fn push_track(parts: &mut Vec<String>, track: Track<'_>) {
    let Track {
        input,
        tag,
        label,
        ranges,
        seek,
        preset,
        duration,
    } = track;
    let mut stream = String::new();
    match ranges {
        [] => return,
        [range] => {
            stream.push_str(&format!("[{input}:a:0]{FORMAT},"));
            stream.push_str(&range_chain(*range, seek));
            stream.push_str(&format!(",volume={:.4}", range.gain()));
        }
        ranges => {
            let count = ranges.len();
            let mut split = format!("[{input}:a:0]{FORMAT},asplit={count}");
            for index in 0..count {
                split.push_str(&format!("[t{tag}{index}]"));
            }
            parts.push(split);
            for (index, range) in ranges.iter().enumerate() {
                parts.push(format!(
                    "[t{tag}{index}]{},volume={:.4}[r{tag}{index}]",
                    range_chain(*range, seek),
                    range.gain()
                ));
            }
            for index in 0..count {
                stream.push_str(&format!("[r{tag}{index}]"));
            }
            stream.push_str(&format!("concat=n={count}:v=0:a=1"));
        }
    }
    if let Some(filter) = preset {
        stream.push(',');
        stream.push_str(filter);
    }
    stream.push_str(&format!(",apad,atrim=end={duration:.6}[{label}]"));
    parts.push(stream);
}

/// The `atempo` stages that carry one speed. Each stage stays within what the
/// filter accepts, so 8x is three doublings and 1/8x three halvings.
fn tempo_chain(speed: f64) -> Vec<f64> {
    let mut stages = Vec::new();
    let mut rest = speed;
    while rest > 2.0 {
        stages.push(2.0);
        rest /= 2.0;
    }
    while rest < 0.5 {
        stages.push(0.5);
        rest /= 0.5;
    }
    if (rest - 1.0).abs() > 1e-9 {
        stages.push(rest);
    }
    stages
}

/// One range's cut, its speed, and the exact length it fills: the retimed
/// sound is padded or trimmed to the composition time the range owns, so no
/// rounding at a cut, and no shorter `atempo` output, moves the next range.
fn range_chain(range: SoundRange, seek: f64) -> String {
    let mut chain = format!(
        "atrim=start={:.6}:end={:.6},asetpts=PTS-STARTPTS",
        range.start - seek,
        range.end - seek
    );
    let stages = tempo_chain(range.speed);
    for stage in &stages {
        chain.push_str(&format!(",atempo={stage:.6}"));
    }
    if !stages.is_empty() || (range.duration - (range.end - range.start)).abs() > 1e-9 {
        chain.push_str(&format!(",apad,atrim=end={:.6}", range.duration));
    }
    chain
}

/// What a click sounds like at a moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickKind {
    Click,
    Down,
    Up,
}

/// One click on the composition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickEvent {
    pub kind: ClickKind,
    pub source_time: f64,
    pub composition_time: f64,
}

/// One piece of the composition a click can land in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickSpan {
    pub source_start: f64,
    pub source_end: f64,
    pub composition_start: f64,
    pub speed: f64,
}

/// Every click of the recording on the composition's clock, in order.
///
/// A press shorter than [`CLICK_PAIR_WINDOW_SECONDS`] is one click; a longer
/// one gives a down and an up. Recorded clicks that no press explains sound as
/// a click on their own, which keeps older recordings audible.
pub fn click_events(sidecar: &PointerSidecar, spans: &[ClickSpan]) -> Vec<ClickEvent> {
    let presses: Vec<PressSample> = sidecar
        .presses
        .iter()
        .copied()
        .filter(|press| is_time(press.down) && is_time(press.up) && press.up >= press.down)
        .collect();

    let mut downs: BTreeMap<i32, Vec<f64>> = BTreeMap::new();
    let mut source_events: Vec<(ClickKind, f64)> = Vec::new();
    for press in &presses {
        downs.entry(press.button).or_default().push(press.down);
        if press.up - press.down < CLICK_PAIR_WINDOW_SECONDS {
            source_events.push((ClickKind::Click, press.down));
        } else {
            source_events.push((ClickKind::Down, press.down));
            source_events.push((ClickKind::Up, press.up));
        }
    }
    for times in downs.values_mut() {
        times.sort_by(f64::total_cmp);
    }
    for click in sidecar.clicks.iter().filter(|click| is_time(click.t)) {
        let matched = downs.get(&click.button).is_some_and(|times| {
            let window = 2.0 * LEGACY_CLICK_MATCH_SECONDS;
            let first = times.partition_point(|&down| down < click.t - window);
            times[first..]
                .iter()
                .take_while(|&&down| down <= click.t + window)
                .any(|&down| (down - click.t).abs() <= LEGACY_CLICK_MATCH_SECONDS)
        });
        if !matched {
            source_events.push((ClickKind::Click, click.t));
        }
    }

    let mut mapped: Vec<ClickEvent> = source_events
        .into_iter()
        .filter_map(|(kind, source_time)| {
            let span = spans.iter().find(|span| span_covers(span, source_time))?;
            let composition_time =
                span.composition_start + (source_time - span.source_start) / span.speed;
            (composition_time.is_finite() && composition_time > 0.0).then_some(ClickEvent {
                kind,
                source_time,
                composition_time,
            })
        })
        .collect();
    mapped.sort_by(|a, b| {
        a.composition_time
            .total_cmp(&b.composition_time)
            .then(a.source_time.total_cmp(&b.source_time))
    });
    mapped
}

fn is_time(t: f64) -> bool {
    t.is_finite() && t >= 0.0
}

fn span_covers(span: &ClickSpan, t: f64) -> bool {
    span.source_start.is_finite()
        && span.source_end.is_finite()
        && span.composition_start.is_finite()
        && span.speed.is_finite()
        && span.speed > 0.0
        && span.speed <= MAX_CUE_SPEED
        && (span.source_start..span.source_end).contains(&t)
}

/// A cheap fingerprint of everything that decides what the composition sounds
/// like, read without building the plan. The preview compares it every frame.
pub fn change_key(state: &VideoEditState) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        state.sound,
        state.audio_mode,
        state.audio_removed,
        state.metadata.has_audio,
        state.trim_start_seconds,
        state.trim_end_seconds,
        state.cuts,
        state.segment_order,
        state.segment_starts,
        state.segment_speeds,
        state.segment_muted,
        state.segments_kept,
        state.freeze_tail,
        state.frozen_segment,
        state
            .sidecar
            .as_ref()
            .map(|sidecar| (sidecar.presses.len(), sidecar.clicks.len())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::model::VideoMetadata;
    use crate::recording::editor::project::VideoProjectFile;
    use crate::recording::editor::sidecar::{CaptureRegion, ClickSample};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn range(start: f64, end: f64, speed: f64, duration: f64) -> SoundRange {
        SoundRange {
            start,
            end,
            speed,
            duration,
            segment: 0,
            muted: false,
        }
    }

    fn piece(start: f64, end: f64, speed: f64, duration: f64, segment: usize) -> SoundRange {
        SoundRange {
            start,
            end,
            speed,
            duration,
            segment,
            muted: false,
        }
    }

    fn slot(
        segment: usize,
        composition_start: f64,
        source_start: f64,
        source_end: f64,
        speed: f64,
    ) -> SoundSlot {
        SoundSlot {
            segment,
            composition_start,
            source_start,
            source_end,
            speed,
            muted: false,
        }
    }

    fn plain() -> SoundSettings {
        SoundSettings::default()
    }

    fn sidecar() -> PointerSidecar {
        PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 320,
                h: 240,
            },
        )
    }

    fn press(down: f64, up: f64, button: i32) -> PressSample {
        PressSample {
            down,
            up,
            button,
            dragged: false,
        }
    }

    fn click(t: f64, button: i32) -> ClickSample {
        ClickSample {
            t,
            x: 0.0,
            y: 0.0,
            button,
            cursor_id: None,
        }
    }

    fn identity() -> Vec<ClickSpan> {
        vec![ClickSpan {
            source_start: 0.0,
            source_end: 100.0,
            composition_start: 0.0,
            speed: 1.0,
        }]
    }

    fn event(kind: ClickKind, source_time: f64, composition_time: f64) -> ClickEvent {
        ClickEvent {
            kind,
            source_time,
            composition_time,
        }
    }

    fn metadata(duration: f64, has_audio: bool) -> VideoMetadata {
        VideoMetadata {
            path: std::env::temp_dir().join("apexshot-sound-source.mp4"),
            duration_seconds: duration,
            width: 1280,
            height: 720,
            file_size_bytes: 1,
            has_audio,
            frame_rate: 30.0,
        }
    }

    /// Lay the segments out one after another from composition 0, the way a
    /// fresh cut list does.
    fn lay_out(state: &mut VideoEditState) {
        let mut at = 0.0;
        let mut starts = vec![0.0; state.segment_order.len()];
        let order = state.segment_order.clone();
        for index in order {
            starts[index] = at;
            if state.segments_kept.get(index).copied().unwrap_or(true) {
                at += state.segment_timeline_duration(index);
            }
        }
        state.segment_starts = starts;
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-sound-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.subsec_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ffmpeg_available() -> bool {
        Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn tone(path: &Path, spec: &str, rate: u32) -> bool {
        Command::new("ffmpeg")
            .args(["-y", "-hide_banner", "-loglevel", "error"])
            .args(["-f", "lavfi", "-i", spec])
            .arg("-c:a")
            .arg("aac")
            .arg("-ar")
            .arg(rate.to_string())
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn a_gap_between_slots_keeps_silence_in_the_sound() {
        let slots = [slot(0, 0.0, 0.0, 2.0, 1.0), slot(1, 4.0, 2.0, 4.0, 1.0)];
        let ranges = sound_ranges(&slots);
        assert_eq!(ranges.len(), 3, "{ranges:?}");
        assert_eq!(ranges[0], piece(0.0, 2.0, 1.0, 2.0, 0));
        assert_eq!(ranges[1].start, ranges[1].end, "{ranges:?}");
        assert!((ranges[1].duration - 2.0).abs() < 1e-9, "{ranges:?}");
        assert!(ranges[1].muted);
        assert_eq!(ranges[2], piece(2.0, 4.0, 1.0, 2.0, 1));

        let played = ranges_from(&ranges, 3.0);
        assert_eq!(played.len(), 2, "{played:?}");
        assert!((played[0].duration - 1.0).abs() < 1e-9, "{played:?}");
        assert!(
            range_chain(played[0], 0.0).contains("apad,atrim=end=1.000000"),
            "{}",
            range_chain(played[0], 0.0)
        );
    }

    #[test]
    fn a_leading_gap_before_the_first_slot_is_kept_as_silence() {
        let slots = [slot(0, 1.5, 0.0, 1.0, 1.0)];
        let ranges = sound_ranges(&slots);
        assert_eq!(ranges.len(), 2, "{ranges:?}");
        assert_eq!(ranges[0].start, ranges[0].end);
        assert!((ranges[0].duration - 1.5).abs() < 1e-9, "{ranges:?}");
        assert_eq!(ranges[1], piece(0.0, 1.0, 1.0, 1.0, 0));
    }

    #[test]
    fn contiguous_slots_play_back_to_back() {
        let slots = [slot(0, 0.0, 0.0, 1.0, 1.0), slot(1, 1.0, 1.0, 2.0, 1.0)];
        let ranges = sound_ranges(&slots);
        assert_eq!(
            ranges,
            vec![piece(0.0, 1.0, 1.0, 1.0, 0), piece(1.0, 2.0, 1.0, 1.0, 1)]
        );
        let filled: f64 = ranges.iter().map(|range| range.duration).sum();
        assert!((filled - 2.0).abs() < 1e-9, "{filled}");
    }

    #[test]
    fn ranges_from_starts_part_way_through() {
        let ranges = [piece(0.0, 1.0, 1.0, 1.0, 0), piece(1.0, 3.0, 2.0, 1.0, 1)];
        let played = ranges_from(&ranges, 1.5);
        assert_eq!(played.len(), 1, "{played:?}");
        assert!((played[0].start - 2.0).abs() < 1e-9, "{played:?}");
        assert!((played[0].end - 3.0).abs() < 1e-9, "{played:?}");
        assert!((played[0].duration - 0.5).abs() < 1e-9, "{played:?}");
        assert_eq!(played[0].segment, 1);
    }

    #[test]
    fn the_voice_track_trims_pads_and_joins() {
        let source = PathBuf::from("mic.m4a");
        let graph = build_graph(
            &source,
            true,
            &plain(),
            &[range(1.0, 3.0, 1.0, 2.0)],
            2.0,
            &[],
            None,
            0.0,
            1,
        )
        .expect("the voice plays");
        assert_eq!(
            graph.graph,
            format!(
                "[1:a:0]{FORMAT},atrim=start=1.000000:end=3.000000,asetpts=PTS-STARTPTS,volume=1.0000,apad,atrim=end=2.000000[mic];[mic]volume=1.0000,atrim=end=2.000000,{FORMAT}[a]"
            )
        );
        assert_eq!(graph.inputs.len(), 1);
        assert_eq!(graph.inputs[0].path, PathBuf::from("mic.m4a"));
        assert!(!graph.inputs[0].looped);
        assert_eq!(graph.inputs[0].seek, 0.0);
    }

    #[test]
    fn a_speed_becomes_a_chain_of_atempo_stages() {
        assert_eq!(tempo_chain(1.0), Vec::<f64>::new());
        assert_eq!(tempo_chain(2.0), vec![2.0]);
        assert_eq!(tempo_chain(3.0), vec![2.0, 1.5]);
        assert_eq!(tempo_chain(8.0), vec![2.0, 2.0, 2.0]);
        assert_eq!(tempo_chain(0.5), vec![0.5]);
        assert_eq!(tempo_chain(0.125), vec![0.5, 0.5, 0.5]);
    }

    #[test]
    fn a_sped_range_is_cut_retimed_and_trimmed_to_its_output_length() {
        assert_eq!(
            range_chain(range(1.0, 4.0, 3.0, 1.0), 0.0),
            "atrim=start=1.000000:end=4.000000,asetpts=PTS-STARTPTS,atempo=2.000000,atempo=1.500000,apad,atrim=end=1.000000"
        );
        assert_eq!(
            range_chain(range(0.0, 8.0, 8.0, 1.0), 0.0),
            "atrim=start=0.000000:end=8.000000,asetpts=PTS-STARTPTS,atempo=2.000000,atempo=2.000000,atempo=2.000000,apad,atrim=end=1.000000"
        );
        assert_eq!(
            range_chain(range(2.0, 10.0, 0.125, 64.0), 1.0),
            "atrim=start=1.000000:end=9.000000,asetpts=PTS-STARTPTS,atempo=0.500000,atempo=0.500000,atempo=0.500000,apad,atrim=end=64.000000"
        );
    }

    #[test]
    fn a_held_tail_gets_silence_instead_of_looped_sound() {
        let ranges = sound_ranges(&[slot(0, 0.0, 0.0, 1.0, 1.0)]);
        assert_eq!(ranges, vec![range(0.0, 1.0, 1.0, 1.0)]);
        let graph = build_graph(
            Path::new("mic.m4a"),
            true,
            &plain(),
            &ranges,
            2.0,
            &[],
            None,
            0.0,
            1,
        )
        .expect("the voice plays");
        assert!(
            graph.graph.contains(",apad,atrim=end=2.000000[mic]"),
            "{}",
            graph.graph
        );
    }

    #[test]
    fn different_speeds_keep_separate_ranges() {
        let slots = [slot(0, 0.0, 0.0, 1.0, 1.0), slot(1, 1.0, 1.0, 3.0, 2.0)];
        let ranges = sound_ranges(&slots);
        assert_eq!(
            ranges,
            vec![range(0.0, 1.0, 1.0, 1.0), piece(1.0, 3.0, 2.0, 1.0, 1)]
        );
        let graph = build_graph(
            Path::new("mic.m4a"),
            true,
            &plain(),
            &ranges,
            2.0,
            &[],
            None,
            0.0,
            1,
        )
        .expect("the voice plays");
        assert!(
            graph.graph.contains("asplit=2[tm0][tm1]"),
            "{}",
            graph.graph
        );
        assert!(
            graph.graph.contains("concat=n=2:v=0:a=1"),
            "{}",
            graph.graph
        );
    }

    #[test]
    fn a_muted_slot_keeps_its_slot_silent() {
        let mut slots = [slot(0, 0.0, 0.0, 1.0, 1.0), slot(1, 1.0, 1.0, 2.0, 1.0)];
        slots[1].muted = true;
        let ranges = sound_ranges(&slots);
        let graph = build_graph(
            Path::new("mic.m4a"),
            true,
            &plain(),
            &ranges,
            2.0,
            &[],
            None,
            0.0,
            1,
        )
        .expect("the voice plays");
        assert!(graph.graph.contains("volume=0.0000"), "{}", graph.graph);
        assert!(graph.graph.contains("concat=n=2"), "{}", graph.graph);
    }

    #[test]
    fn nothing_audible_builds_no_graph() {
        assert!(build_graph(
            Path::new("mic.m4a"),
            false,
            &plain(),
            &[],
            2.0,
            &[],
            None,
            0.0,
            0,
        )
        .is_none());
        let mut muted = range(0.0, 1.0, 1.0, 1.0);
        muted.muted = true;
        assert!(build_graph(
            Path::new("mic.m4a"),
            true,
            &plain(),
            &[muted],
            1.0,
            &[],
            None,
            0.0,
            0,
        )
        .is_none());
    }

    #[test]
    fn the_voice_preset_follows_the_concat() {
        let settings = SoundSettings {
            microphone_preset: MicPreset::Loud,
            ..SoundSettings::default()
        };
        let ranges = [piece(0.0, 1.0, 1.0, 1.0, 0), piece(1.0, 2.0, 1.0, 1.0, 1)];
        let graph = build_graph(
            Path::new("mic.m4a"),
            true,
            &settings,
            &ranges,
            2.0,
            &[],
            None,
            0.0,
            1,
        )
        .expect("the voice plays");
        let concat = graph
            .graph
            .find("concat=n=2:v=0:a=1")
            .expect("the pieces join");
        let preset = graph.graph.find("speechnorm").expect("the preset runs");
        let pad = graph
            .graph
            .find(",apad,atrim=end=2.000000[mic]")
            .expect("the track is padded");
        assert!(concat < preset && preset < pad, "{}", graph.graph);
    }

    #[test]
    fn background_loops_under_the_composition() {
        let dir = scratch("background");
        let music = dir.join("music.m4a");
        std::fs::write(&music, b"music").unwrap();
        let settings = SoundSettings {
            background_file: Some(music.clone()),
            ..SoundSettings::default()
        };
        let graph = build_graph(
            Path::new("mic.m4a"),
            false,
            &settings,
            &[],
            3.0,
            &[],
            None,
            0.0,
            1,
        )
        .expect("the background plays");
        assert_eq!(graph.inputs.len(), 1);
        assert!(graph.inputs[0].looped);
        assert_eq!(graph.inputs[0].path, music);
        assert!(graph.graph.contains("[bg]"), "{}", graph.graph);
        assert!(graph.graph.contains("volume=0.0500"), "{}", graph.graph);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn background_starts_where_the_playhead_is() {
        let dir = scratch("background-seek");
        let music = dir.join("music.m4a");
        std::fs::write(&music, b"music").unwrap();
        let settings = SoundSettings {
            background_file: Some(music),
            ..SoundSettings::default()
        };
        let graph = build_graph(
            Path::new("mic.m4a"),
            false,
            &settings,
            &[],
            2.0,
            &[],
            None,
            4.0,
            0,
        )
        .expect("the background plays");
        assert!(
            graph
                .graph
                .contains("atrim=start=4.000000,asetpts=PTS-STARTPTS"),
            "{}",
            graph.graph
        );
    }

    #[test]
    fn the_click_graph_delays_and_scales_each_cue_over_the_kept_sound() {
        let ranges = [range(0.0, 2.0, 1.0, 2.0)];
        let cues = [
            ClickCue {
                delay_ms: 500,
                gain: 0.8,
            },
            ClickCue {
                delay_ms: 1250,
                gain: 0.6,
            },
        ];
        let click = PathBuf::from("click-soft.wav");
        let graph = build_graph(
            Path::new("mic.m4a"),
            true,
            &plain(),
            &ranges,
            2.0,
            &cues,
            Some(&click),
            0.0,
            1,
        )
        .expect("the cues play over the voice");
        assert_eq!(graph.inputs.len(), 2);
        assert_eq!(graph.inputs[1].path, click);
        assert_eq!(
            graph.graph,
            format!(
                "[1:a:0]{FORMAT},atrim=start=0.000000:end=2.000000,asetpts=PTS-STARTPTS,volume=1.0000,apad,atrim=end=2.000000[mic];[2:a:0]{FORMAT},asplit=2[c0][c1];[c0]adelay=500:all=1,volume=0.8000[e0];[c1]adelay=1250:all=1,volume=0.6000[e1];[mic][e0][e1]amix=inputs=3:normalize=0:duration=longest:dropout_transition=0,volume=1.0000,atrim=end=2.000000,{FORMAT}[a]"
            )
        );

        let silent = build_graph(
            Path::new("mic.m4a"),
            false,
            &plain(),
            &[],
            1.5,
            &cues[..1],
            Some(&click),
            0.0,
            1,
        )
        .expect("the cue plays on its own");
        assert_eq!(
            silent.graph,
            format!(
                "[1:a:0]{FORMAT},asplit=1[c0];[c0]adelay=500:all=1,volume=0.8000[e0];[e0]volume=1.0000,atrim=end=1.500000,{FORMAT}[a]"
            )
        );
    }

    #[test]
    fn a_click_inside_a_fast_slot_maps_through_its_speed() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(2.0, 2.05, 1));
        let double = [ClickSpan {
            source_start: 0.0,
            source_end: 10.0,
            composition_start: 0.0,
            speed: 2.0,
        }];
        assert_eq!(
            click_events(&sidecar, &double),
            vec![event(ClickKind::Click, 2.0, 1.0)]
        );
        let triple = [ClickSpan {
            speed: 3.0,
            ..double[0]
        }];
        assert_eq!(click_events(&sidecar, &triple), Vec::new());
        let held = [ClickSpan {
            source_start: 0.0,
            source_end: 0.0,
            composition_start: 0.0,
            speed: 0.0,
        }];
        assert_eq!(click_events(&sidecar, &held), Vec::new());
    }

    #[test]
    fn click_cues_follow_the_press_kind_and_volume() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.05, 1));
        sidecar.presses.push(press(2.0, 2.5, 1));
        let slots = [slot(0, 0.0, 0.0, 10.0, 1.0)];
        assert_eq!(
            click_cues(&sidecar, &slots, 1.0),
            vec![
                ClickCue {
                    delay_ms: 1000,
                    gain: 1.0
                },
                ClickCue {
                    delay_ms: 2000,
                    gain: 0.8
                },
                ClickCue {
                    delay_ms: 2500,
                    gain: 0.6
                },
            ]
        );
        assert_eq!(click_cues(&sidecar, &slots, 0.5)[0].gain, 0.5);
    }

    #[test]
    fn a_click_at_the_very_start_plays_no_sound() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(0.0, 0.05, 1));
        assert_eq!(click_events(&sidecar, &identity()), Vec::new());
    }

    #[test]
    fn a_short_press_gives_one_click_at_its_down_time() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.1, 1));
        assert_eq!(
            click_events(&sidecar, &identity()),
            vec![event(ClickKind::Click, 1.0, 1.0)]
        );
    }

    #[test]
    fn a_long_press_gives_a_down_and_an_up() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 2.0, 1));
        assert_eq!(
            click_events(&sidecar, &identity()),
            vec![
                event(ClickKind::Down, 1.0, 1.0),
                event(ClickKind::Up, 2.0, 2.0)
            ]
        );
    }

    #[test]
    fn two_close_independent_presses_remain_two_clicks() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.05, 1));
        sidecar.presses.push(press(1.1, 1.15, 1));
        assert_eq!(
            click_events(&sidecar, &identity()),
            vec![
                event(ClickKind::Click, 1.0, 1.0),
                event(ClickKind::Click, 1.1, 1.1)
            ]
        );
    }

    #[test]
    fn an_unmatched_legacy_click_gives_one_click() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(click(3.0, 1));
        assert_eq!(
            click_events(&sidecar, &identity()),
            vec![event(ClickKind::Click, 3.0, 3.0)]
        );
    }

    #[test]
    fn a_legacy_click_matching_a_press_is_suppressed() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.05, 1));
        sidecar.clicks.push(click(1.0, 1));
        assert_eq!(
            click_events(&sidecar, &identity()),
            vec![event(ClickKind::Click, 1.0, 1.0)]
        );
    }

    #[test]
    fn a_legacy_click_pairs_only_with_a_press_of_the_same_button() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.05, 1));
        sidecar.clicks.push(click(1.0, 2));
        assert_eq!(
            click_events(&sidecar, &identity()),
            vec![
                event(ClickKind::Click, 1.0, 1.0),
                event(ClickKind::Click, 1.0, 1.0)
            ]
        );
    }

    #[test]
    fn trimmed_presses_map_through_a_double_speed_span() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.05, 1));
        sidecar.presses.push(press(3.0, 4.0, 1));
        sidecar.presses.push(press(5.0, 5.1, 1));
        sidecar.presses.push(press(6.0, 6.05, 1));
        let spans = [ClickSpan {
            source_start: 2.0,
            source_end: 6.0,
            composition_start: 0.0,
            speed: 2.0,
        }];
        assert_eq!(
            click_events(&sidecar, &spans),
            vec![
                event(ClickKind::Down, 3.0, 0.5),
                event(ClickKind::Up, 4.0, 1.0),
                event(ClickKind::Click, 5.0, 1.5),
            ]
        );
    }

    #[test]
    fn a_span_faster_than_two_times_is_skipped() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.05, 1));
        sidecar.clicks.push(click(2.0, 1));
        let spans = [ClickSpan {
            source_start: 0.0,
            source_end: 10.0,
            composition_start: 0.0,
            speed: 3.0,
        }];
        assert_eq!(click_events(&sidecar, &spans), Vec::new());
    }

    #[test]
    fn malformed_times_and_spans_are_ignored() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(f64::NAN, 1.0, 1));
        sidecar.presses.push(press(-1.0, 0.5, 1));
        sidecar.presses.push(press(2.0, 1.0, 1));
        sidecar.presses.push(press(1.0, f64::INFINITY, 1));
        sidecar.clicks.push(click(f64::NAN, 1));
        sidecar.clicks.push(click(-2.0, 1));
        let spans = [
            ClickSpan {
                source_start: 0.0,
                source_end: f64::NAN,
                composition_start: 0.0,
                speed: 1.0,
            },
            ClickSpan {
                source_start: 0.0,
                source_end: 10.0,
                composition_start: 0.0,
                speed: 0.0,
            },
            ClickSpan {
                source_start: 0.0,
                source_end: 10.0,
                composition_start: f64::NAN,
                speed: 1.0,
            },
        ];
        assert_eq!(click_events(&sidecar, &spans), Vec::new());
    }

    #[test]
    fn many_short_sped_segments_keep_each_ranges_exact_length() {
        let mut state = VideoEditState::new(metadata(2.0, true));
        for step in 1..20 {
            state.cuts.push(step as f64 * 0.1);
        }
        state.segments_kept = vec![true; 20];
        state.segment_order = (0..20).collect();
        state.segment_speeds = (0..20)
            .map(|index| if index % 2 == 0 { 3.0 } else { 1.5 })
            .collect();
        state.segment_muted = vec![false; 20];
        state.segment_starts = vec![0.0; 20];
        lay_out(&mut state);
        let plan = SoundPlan::from_state(&state);
        assert_eq!(plan.ranges.len(), 20, "{:?}", plan.ranges);
        let filled: f64 = plan.ranges.iter().map(|range| range.duration).sum();
        assert!(
            (filled - plan.duration).abs() < 1e-9,
            "{filled} vs {}",
            plan.duration
        );
        for range in &plan.ranges {
            let chain = range_chain(*range, 0.0);
            let limit = format!("atrim=end={:.6}", range.duration);
            assert!(chain.contains(limit.as_str()), "{chain}");
        }
    }

    #[test]
    fn a_slow_span_fills_its_whole_output_length_before_the_next_range() {
        let slots = [slot(0, 0.0, 0.0, 1.0, 0.5), slot(1, 2.0, 1.0, 2.0, 1.0)];
        let ranges = sound_ranges(&slots);
        assert_eq!(
            ranges,
            vec![range(0.0, 1.0, 0.5, 2.0), piece(1.0, 2.0, 1.0, 1.0, 1)]
        );
        let filled: f64 = ranges.iter().map(|range| range.duration).sum();
        assert!((filled - 3.0).abs() < 1e-9, "{filled}");
        assert_eq!(
            range_chain(ranges[0], 0.0),
            "atrim=start=0.000000:end=1.000000,asetpts=PTS-STARTPTS,atempo=0.500000,apad,atrim=end=2.000000"
        );
    }

    #[test]
    fn settings_clamp_volumes_and_drop_unknown_clicks() {
        let wild = SoundSettings {
            volume: 3.0,
            background_volume: -1.0,
            click_sound_volume: 4.0,
            click_sound: Some("chime".into()),
            microphone_preset: MicPreset::Podcast,
            ..SoundSettings::default()
        }
        .clamped();
        assert_eq!(wild.volume, 1.0);
        assert_eq!(wild.background_volume, 0.0);
        assert_eq!(wild.click_sound_volume, 1.0);
        assert_eq!(wild.microphone_preset, MicPreset::Podcast);
        assert_eq!(wild.click_sound, None);

        let broken = SoundSettings {
            volume: f64::NAN,
            background_volume: f64::INFINITY,
            ..SoundSettings::default()
        }
        .clamped();
        assert_eq!(broken.volume, 1.0);
        assert_eq!(broken.background_volume, 0.05);

        assert_eq!(SoundSettings::default().volume, 1.0);
        assert_eq!(SoundSettings::default().microphone_preset, MicPreset::Off);
        assert!(!SoundSettings::default().background_active());
        assert!(!SoundSettings::default().click_active());
        let quiet = SoundSettings {
            background_file: Some(PathBuf::from("music.m4a")),
            background_volume: 0.0,
            ..SoundSettings::default()
        };
        assert!(!quiet.background_active());
        let muted = SoundSettings {
            background_file: Some(PathBuf::from("music.m4a")),
            mute_background: true,
            ..SoundSettings::default()
        };
        assert!(!muted.background_active());
        let soft = SoundSettings {
            click_sound: Some(CLICK_SOFT.into()),
            ..SoundSettings::default()
        };
        assert!(soft.click_active());
    }

    #[test]
    fn an_untouched_sound_leaves_the_export_alone() {
        assert!(!SoundSettings::default().shapes_export());
        let music = SoundSettings {
            background_file: Some(PathBuf::from("music.m4a")),
            ..SoundSettings::default()
        };
        assert!(music.shapes_export());
        let quiet = SoundSettings {
            volume: 0.5,
            ..SoundSettings::default()
        };
        assert!(quiet.shapes_export());
        let preset = SoundSettings {
            microphone_preset: MicPreset::Standard,
            ..SoundSettings::default()
        };
        assert!(preset.shapes_export());
        let clicks = SoundSettings {
            click_sound: Some(CLICK_SOFT.into()),
            ..SoundSettings::default()
        };
        assert!(clicks.shapes_export());
        let silent_clicks = SoundSettings {
            click_sound: Some(CLICK_SOFT.into()),
            click_sound_volume: 0.0,
            ..SoundSettings::default()
        };
        assert!(!silent_clicks.shapes_export());
    }

    #[test]
    fn every_microphone_mode_fits_its_tile() {
        for preset in MicPreset::ALL {
            let summary = preset.summary();
            assert!(!summary.is_empty(), "{preset:?} has no summary");
            assert!(
                summary.chars().count() <= 22,
                "{preset:?}'s summary is {} characters: {summary}",
                summary.chars().count()
            );
            assert!(!preset.label().is_empty(), "{preset:?} has no label");
            assert!(
                !preset.description().is_empty(),
                "{preset:?} has no description"
            );
            if preset != MicPreset::Off {
                assert!(preset.filter().is_some(), "{preset:?} has no filter");
            }
        }
        assert_eq!(MicPreset::Off.filter(), None);
        assert_eq!(MicPreset::default(), MicPreset::Off);
        assert_eq!(MicPreset::ALL.len(), 6);
        assert_eq!(MicPreset::ALL[5], MicPreset::Off);
    }

    #[test]
    fn the_preview_volume_quiets_only_the_bundled_pcm() {
        const HEADER: usize = 44;
        let peaks = |bytes: &[u8]| {
            bytes[HEADER..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|sample| i32::from(i16::from_le_bytes(*sample)).abs())
                .max()
                .unwrap_or(0)
        };

        let full = scaled(CLICK_WAV, 1.0).expect("the bundled click is a 16-bit PCM wav");
        assert_eq!(full.as_slice(), CLICK_WAV);

        let quiet = scaled(CLICK_WAV, 0.25).expect("the bundled click is a 16-bit PCM wav");
        assert_eq!(quiet.len(), CLICK_WAV.len());
        assert_eq!(quiet[..HEADER], CLICK_WAV[..HEADER]);

        let full_peak = peaks(CLICK_WAV);
        assert!(full_peak > 0);
        assert!((f64::from(peaks(&quiet)) - f64::from(full_peak) / 4.0).abs() <= 1.0);

        assert!(scaled(b"not a wav", 0.5).is_none());
    }

    #[test]
    fn a_project_round_trip_keeps_the_sound() {
        let state_sound = SoundSettings {
            volume: 0.6,
            microphone_preset: MicPreset::Warm,
            background_file: Some(PathBuf::from("music.m4a")),
            background_volume: 0.2,
            mute_background: true,
            click_sound: Some(CLICK_SOFT.into()),
            click_sound_volume: 0.3,
        };
        let mut state = VideoEditState::new(metadata(4.0, true));
        state.sound = state_sound.clone();
        let file = state.to_project();
        assert_eq!(file.sound, state_sound);

        let mut restored = VideoEditState::new(metadata(4.0, true));
        restored.apply_project(file);
        assert_eq!(restored.sound, state_sound.clamped());
    }

    #[test]
    fn a_project_without_sound_loads_the_defaults() {
        let state = VideoEditState::new(metadata(4.0, true));
        let mut value = serde_json::to_value(state.to_project()).unwrap();
        let object = value.as_object_mut().expect("a project is an object");
        assert!(object.remove("sound").is_some(), "the field is written");
        let file: VideoProjectFile = serde_json::from_value(value).unwrap();
        assert_eq!(file.sound, SoundSettings::default());
        assert_eq!(file.sound.microphone_preset, MicPreset::Off);
    }

    #[test]
    fn a_fresh_state_is_still_default_with_sound() {
        let state = VideoEditState::new(metadata(4.0, true));
        assert!(state.session_is_default());
        assert_eq!(state.sound, SoundSettings::default());
    }

    #[test]
    fn clicks_only_reach_the_plan_when_the_click_sound_is_on() {
        let mut state = VideoEditState::new(metadata(4.0, true));
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.05, 1));
        state.sidecar = Some(sidecar);

        let off = SoundPlan::from_state(&state);
        assert!(off.cues.is_empty());

        state.sound.click_sound = Some(CLICK_SOFT.into());
        let on = SoundPlan::from_state(&state);
        assert_eq!(
            on.cues,
            vec![ClickCue {
                delay_ms: 1000,
                gain: 0.25
            }]
        );
        assert!(on.has_cues());
    }

    #[test]
    fn a_muted_composition_has_no_voice() {
        let mut state = VideoEditState::new(metadata(4.0, true));
        state.audio_mode = AudioMode::Muted;
        let plan = SoundPlan::from_state(&state);
        assert!(!plan.has_audio);
        assert!(plan.graph_from(0.0, None, 1).is_none());

        state.audio_mode = AudioMode::Mono;
        let plan = SoundPlan::from_state(&state);
        assert!(plan.has_audio);
        assert!(plan.mono());
        assert!(plan.graph_from(0.0, None, 1).is_some());
    }

    #[test]
    fn a_plan_past_the_end_plays_nothing() {
        let state = VideoEditState::new(metadata(2.0, true));
        let plan = SoundPlan::from_state(&state);
        assert!((plan.duration() - 2.0).abs() < 1e-9);
        assert!(plan.graph_from(2.0, None, 1).is_none());
        assert!(plan.graph_from(-1.0, None, 1).is_none());
        assert!(plan.graph_from(f64::NAN, None, 1).is_none());
    }

    #[test]
    fn the_change_key_moves_with_the_edits_and_the_settings() {
        let mut state = VideoEditState::new(metadata(4.0, true));
        let start = change_key(&state);
        assert_eq!(start, change_key(&state));
        state.sound.volume = 0.5;
        let quiet = change_key(&state);
        assert_ne!(start, quiet);
        state.cuts.push(2.0);
        assert_ne!(quiet, change_key(&state));
    }

    #[test]
    fn a_full_graph_renders_the_compositions_length() {
        if !ffmpeg_available() {
            crate::test_support::skip_without_media("skipping the sound graph test: ffmpeg is unavailable");
            return;
        }
        let dir = scratch("graph");
        let video = dir.join("clip.m4a");
        let music = dir.join("music.m4a");
        if !tone(&video, "sine=f=440:d=6", 48_000) || !tone(&music, "sine=f=220:d=1.5", 48_000) {
            crate::test_support::skip_without_media("skipping the sound graph test: ffmpeg could not write its tones");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let click = dir.join("click.wav");
        std::fs::write(&click, CLICK_WAV).unwrap();

        let mut metadata = metadata(6.0, true);
        metadata.path = video.clone();
        let mut state = VideoEditState::new(metadata);
        state.cuts = vec![2.0, 4.0];
        state.segments_kept = vec![true, true, true];
        state.segment_order = vec![0, 1, 2];
        state.segment_starts = vec![0.0; 3];
        state.segment_speeds = vec![1.0, 2.0, 1.0];
        state.segment_muted = vec![false, false, true];
        lay_out(&mut state);
        state.sound = SoundSettings {
            background_file: Some(music),
            background_volume: 0.5,
            click_sound: Some(CLICK_SOFT.into()),
            click_sound_volume: 1.0,
            ..SoundSettings::default()
        };
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.05, 1));
        state.sidecar = Some(sidecar);
        let plan = SoundPlan::from_state(&state);
        let duration = plan.duration();
        assert!((duration - 5.0).abs() < 1e-9, "{duration}");
        assert_eq!(plan.cues.len(), 1, "{:?}", plan.cues);

        for from in [0.0, 2.5] {
            let graph = plan
                .graph_from(from, Some(&click), 0)
                .expect("the mix plays");
            let mut command = Command::new("ffmpeg");
            command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
            for input in &graph.inputs {
                if input.looped {
                    command.args(["-stream_loop", "-1"]);
                }
                if input.seek > 0.0 {
                    command.arg("-ss").arg(format!("{:.6}", input.seek));
                }
                command.arg("-i").arg(&input.path);
            }
            let decoded = command
                .arg("-filter_complex")
                .arg(&graph.graph)
                .args([
                    "-map", "[a]", "-f", "f32le", "-ac", "1", "-ar", "48000", "-",
                ])
                .output()
                .expect("ffmpeg runs");
            assert!(
                decoded.status.success(),
                "{}",
                String::from_utf8_lossy(&decoded.stderr)
            );
            let samples: Vec<f32> = decoded
                .stdout
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| f32::from_le_bytes(*bytes))
                .collect();
            let seconds = samples.len() as f64 / 48000.0;
            assert!(
                (seconds - (duration - from)).abs() < 0.05,
                "{from}: {seconds:.3} s of {}",
                duration - from
            );
            if from == 0.0 {
                let rms = |start: f64, end: f64| {
                    let first = (start * 48000.0).round() as usize;
                    let last = ((end * 48000.0).round() as usize).min(samples.len());
                    let sum: f64 = samples[first..last]
                        .iter()
                        .map(|value| f64::from(*value).powi(2))
                        .sum();
                    (sum / (last - first) as f64).sqrt()
                };
                let loud = rms(0.5, 0.9);
                assert!(
                    loud > 0.05,
                    "the voice, the music and the click are audible: {loud:.3}"
                );
                let quiet = rms(4.2, 4.9);
                assert!(
                    quiet < loud,
                    "the muted piece leaves only the music: {quiet:.3} vs {loud:.3}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
