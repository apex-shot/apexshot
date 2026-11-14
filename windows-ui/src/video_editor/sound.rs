//! The editor's sound: the microphone modes, each cut piece's volume, the
//! background music and the click sounds, turned into the ffmpeg graph that
//! the preview player and the export both read.

use super::click_sound::{self, Kind};
use crate::recording::editor::ffmpeg;
use crate::recording::editor::sidecar::PointerSidecar;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The stereo float format every stage of a sound graph speaks.
const FORMAT: &str = "aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo";

/// The rate the timeline's waveform is decoded at.
const PEAK_RATE: usize = 8000;

/// The samples one waveform value covers: 20 ms at the peak rate.
const PEAK_BUCKET: usize = PEAK_RATE / 50;

/// How the microphone is processed, the studied audio modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MicPreset {
    Off,
    #[default]
    Standard,
    Loud,
    NoiseReduction,
    Podcast,
    Warm,
}

impl MicPreset {
    /// Every mode in the order the panel lays its tiles out (two columns).
    pub const ALL: [MicPreset; 6] = [
        MicPreset::Standard,
        MicPreset::Loud,
        MicPreset::NoiseReduction,
        MicPreset::Podcast,
        MicPreset::Warm,
        MicPreset::Off,
    ];

    /// English label, passed through `t()` by the caller.
    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Loud => "Loud",
            Self::NoiseReduction => "Noise reduction",
            Self::Podcast => "Podcast",
            Self::Warm => "Warm",
            Self::Off => "Off",
        }
    }

    /// One sentence for the panel, passed through `t()` by the caller.
    pub fn description(self) -> &'static str {
        match self {
            Self::Standard => "Evens out your voice so it stays at one level.",
            Self::Loud => "Louder, compressed voice that cuts through music.",
            Self::NoiseReduction => "Removes steady background noise, then evens out the voice.",
            Self::Podcast => "Full, warm voice with softened hiss and sibilance.",
            Self::Warm => "Adds body to thin microphones.",
            Self::Off => "Keeps the microphone exactly as it was recorded.",
        }
    }

    /// One short line for inside the mode's tile, passed through `t()` by the
    /// caller.
    pub fn summary(self) -> &'static str {
        match self {
            Self::Standard => "Even, natural voice",
            Self::Loud => "Punchy and compressed",
            Self::NoiseReduction => "Removes steady noise",
            Self::Podcast => "Warm, polished voice",
            Self::Warm => "Fuller, deeper tone",
            Self::Off => "As recorded",
        }
    }

    /// The filter chain this mode applies to the voice track, if any.
    pub(super) fn filter(self) -> Option<&'static str> {
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

/// The project's sound, the studied audio settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct SoundSettings {
    /// Master volume, 0..=1. Default 1.
    pub volume: f64,
    pub mute_microphone: bool,
    pub microphone_preset: MicPreset,
    pub mute_system: bool,
    /// A music file played under the whole video, looped.
    pub background_file: Option<PathBuf>,
    /// 0..=1, default 0.05.
    pub background_volume: f64,
    pub mute_background: bool,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            volume: 1.0,
            mute_microphone: false,
            microphone_preset: MicPreset::Standard,
            mute_system: false,
            background_file: None,
            background_volume: 0.05,
            mute_background: false,
        }
    }
}

impl SoundSettings {
    /// Volumes clamped to 0..=1 (non-finite -> default); everything else kept.
    pub fn clamped(self) -> Self {
        Self {
            volume: clamp_unit(self.volume, 1.0),
            background_volume: clamp_unit(self.background_volume, 0.05),
            ..self
        }
    }

    /// Whether background music will actually play: a file is set, it is not
    /// muted and its volume is above zero.
    pub fn background_active(&self) -> bool {
        self.background_file.is_some() && !self.mute_background && self.background_volume > 0.0
    }
}

/// One cut piece's own sound, the studied per-clip volumes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct ClipSound {
    /// 0..=1, default 1.
    pub mic_volume: f64,
    /// 0..=1, default 1.
    pub system_volume: f64,
    pub muted: bool,
}

impl Default for ClipSound {
    fn default() -> Self {
        Self {
            mic_volume: 1.0,
            system_volume: 1.0,
            muted: false,
        }
    }
}

impl ClipSound {
    pub fn clamped(self) -> Self {
        Self {
            mic_volume: clamp_unit(self.mic_volume, 1.0),
            system_volume: clamp_unit(self.system_volume, 1.0),
            muted: self.muted,
        }
    }

    /// The gain the microphone (or the mixed recording track) plays at: 0 when
    /// muted.
    pub fn mic_gain(&self) -> f64 {
        if self.muted {
            0.0
        } else {
            self.mic_volume
        }
    }

    /// The gain the system sound plays at: 0 when muted.
    pub fn system_gain(&self) -> f64 {
        if self.muted {
            0.0
        } else {
            self.system_volume
        }
    }
}

/// A level that stays inside 0..=1; a value that is not a number reads as the
/// given default.
fn clamp_unit(value: f64, default: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        default
    }
}

/// The sound files a recording has.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct SoundSources {
    pub microphone: Option<PathBuf>,
    pub system: Option<PathBuf>,
    /// The MP4's own mixed track, used only when neither separate file exists.
    pub mixed: Option<PathBuf>,
}

impl SoundSources {
    /// Look for the separate tracks next to each video's sidecar, in order; the
    /// first video with at least one separate file wins. Otherwise `mixed` is
    /// the first video whose container has an audio stream (ffprobe `a:0`).
    /// Blocking (runs ffprobe); call it off the UI thread or once per document.
    pub fn find(videos: &[PathBuf]) -> Self {
        for video in videos {
            let sidecar = PointerSidecar::resolved_sidecar_path(video);
            let microphone = track_beside(&sidecar, ".mic.m4a");
            let system = track_beside(&sidecar, ".system.m4a");
            if microphone.is_some() || system.is_some() {
                return Self {
                    microphone,
                    system,
                    mixed: None,
                };
            }
        }
        Self {
            microphone: None,
            system: None,
            mixed: videos.iter().find(|path| has_audio(path)).cloned(),
        }
    }

    /// The recording has a microphone-like track: `microphone` or `mixed`.
    pub fn has_microphone(&self) -> bool {
        self.microphone.is_some() || self.mixed.is_some()
    }

    pub fn has_system(&self) -> bool {
        self.system.is_some()
    }

    /// Neither track: a silent recording.
    pub fn is_empty(&self) -> bool {
        self.microphone.is_none() && self.system.is_none() && self.mixed.is_none()
    }
}

/// A separate sound file next to a pointer sidecar, if it is there.
fn track_beside(sidecar: &Path, suffix: &str) -> Option<PathBuf> {
    let mut name = sidecar.as_os_str().to_owned();
    name.push(suffix);
    let path = PathBuf::from(name);
    path.is_file().then_some(path)
}

/// One kept stretch of source sound, in composition order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SoundRange {
    /// Source seconds.
    pub start: f64,
    pub end: f64,
    pub speed: f64,
    /// The exact composition length this range fills.
    pub duration: f64,
    /// The cut piece (segment index, 0-based) the range belongs to.
    pub segment: usize,
}

/// The ranges of a frame plan. A held span is dead air between moved pieces:
/// it becomes a range with an empty source trim, which the chain pads to the
/// span's length, so the sound after the gap keeps its place. A hold at the
/// end needs no range, because the track is padded to the composition's length
/// anyway. Adjacent spans of the same segment and speed merge; spans of
/// different segments never merge, because each piece carries its own sound.
pub(super) fn sound_ranges(
    spans: &[super::export::SourceSpan],
    fps: f64,
    trim_start: f64,
    trim_end: f64,
    cuts: &[f64],
) -> Vec<SoundRange> {
    let mut ranges: Vec<SoundRange> = Vec::new();
    for (index, span) in spans.iter().enumerate() {
        if !span.speed.is_finite() || span.speed <= 0.0 {
            let resumes = spans[index + 1..]
                .iter()
                .any(|later| later.speed.is_finite() && later.speed > 0.0);
            if resumes && span.frames > 0 {
                ranges.push(SoundRange {
                    start: span.source_start,
                    end: span.source_start,
                    speed: 1.0,
                    duration: span.frames as f64 / fps,
                    segment: segment_of(span.source_start, fps, trim_start, trim_end, cuts),
                });
            }
            continue;
        }
        let range = SoundRange {
            start: span.source_start,
            end: span.source_end,
            speed: span.speed,
            duration: span.frames as f64 / fps,
            segment: segment_of(span.source_start, fps, trim_start, trim_end, cuts),
        };
        match ranges.last_mut() {
            Some(last)
                if last.end > last.start
                    && range.end > range.start
                    && last.segment == range.segment
                    && last.speed == range.speed
                    && (range.start - last.end).abs() < 0.5 / fps =>
            {
                last.end = range.end;
                last.duration += range.duration;
            }
            _ => ranges.push(range),
        }
    }
    ranges
}

/// The cut piece a span starting at `source_start` belongs to: the cuts that
/// fall before it inside the trim, so two spans on either side of a cut never
/// share a piece.
fn segment_of(source_start: f64, fps: f64, trim_start: f64, trim_end: f64, cuts: &[f64]) -> usize {
    let edge = if fps.is_finite() && fps > 0.0 {
        source_start + 0.5 / fps
    } else {
        source_start
    };
    cuts.iter()
        .filter(|cut| cut.is_finite() && **cut > trim_start && **cut < trim_end && **cut <= edge)
        .count()
}

/// `ranges` as they play from composition time `from` on: ranges that end at or
/// before `from` are dropped, the one containing it starts part-way through.
pub(super) fn ranges_from(ranges: &[SoundRange], from: f64) -> Vec<SoundRange> {
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
        played.push(SoundRange {
            start: range.start + into * range.speed,
            duration: range.duration - into,
            ..*range
        });
    }
    played
}

/// One ffmpeg input of a sound graph. A runner passes, in this order:
/// `-stream_loop -1` when `looped`, `-ss {seek:.6}` when `seek > 0`, then
/// `-i path`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct GraphInput {
    pub path: PathBuf,
    /// Read with `-stream_loop -1` (background music).
    pub looped: bool,
    /// Input seek in seconds; the graph's trims are already relative to it.
    pub seek: f64,
}

/// A filter graph producing the composed sound on the output label `[a]`:
/// 48 kHz, stereo, float, exactly `duration` seconds long.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SoundGraph {
    /// Inputs in order; the graph refers to them as `[first_input + i:a:0]`.
    pub inputs: Vec<GraphInput>,
    pub graph: String,
}

/// A click cue: composition milliseconds and gain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ClickCue {
    pub delay_ms: u64,
    pub gain: f64,
}

/// Everything that decides what the composition sounds like, built once per
/// change. Comparing two plans tells the preview player whether to restart.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SoundPlan {
    pub sources: SoundSources,
    pub settings: SoundSettings,
    pub clips: Vec<ClipSound>,
    /// From composition time 0.
    pub ranges: Vec<SoundRange>,
    /// The composition's whole length in seconds (freeze tail included).
    pub duration: f64,
    /// From composition time 0.
    pub cues: Vec<ClickCue>,
}

impl SoundPlan {
    /// The plan of a composition. `spans` is a frame plan; cues are made only
    /// when the click sound is the shipped one, its volume is above zero and a
    /// sidecar is given.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sources: SoundSources,
        effects: &super::project::EffectsSettings,
        sidecar: Option<&PointerSidecar>,
        spans: &[super::export::SourceSpan],
        fps: f64,
        duration: f64,
        trim_start: f64,
        trim_end: f64,
        cuts: &[f64],
    ) -> Self {
        let clicks = sidecar.filter(|_| {
            effects.click_sound.as_deref() == Some("soft") && effects.click_sound_volume > 0.0
        });
        Self {
            sources,
            settings: effects.sound.clone(),
            clips: effects.clip_sounds.clone(),
            ranges: sound_ranges(spans, fps, trim_start, trim_end, cuts),
            duration,
            cues: clicks.map_or_else(Vec::new, |sidecar| {
                click_cues(sidecar, spans, effects.click_sound_volume)
            }),
        }
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
            &self.sources,
            &self.settings,
            &self.clips,
            &ranges,
            self.duration - from,
            &cues,
            click,
            from,
            first_input,
        )
    }
}

fn click_cues(
    sidecar: &PointerSidecar,
    spans: &[super::export::SourceSpan],
    volume: f64,
) -> Vec<ClickCue> {
    let spans: Vec<click_sound::Span> = spans
        .iter()
        .filter(|span| span.speed > 0.0)
        .map(|span| click_sound::Span {
            source_start: span.source_start,
            source_end: span.source_end,
            composition_start: span.composition_start,
            speed: span.speed,
        })
        .collect();
    click_sound::events(sidecar, &spans)
        .into_iter()
        .map(|event| {
            let scale = match event.kind {
                Kind::Click => 1.0,
                Kind::Down => 0.8,
                Kind::Up => 0.6,
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
/// `clips` is indexed by `SoundRange::segment`; `click` is the click-sound WAV
/// read when `cues` is non-empty. None when nothing would be heard.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_graph(
    sources: &SoundSources,
    settings: &SoundSettings,
    clips: &[ClipSound],
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
    let gain = |range: &SoundRange| clips.get(range.segment).copied().unwrap_or_default();
    let mut inputs: Vec<GraphInput> = Vec::new();
    let mut parts: Vec<String> = Vec::new();
    let mut labels: Vec<String> = Vec::new();

    let voice = sources.microphone.as_deref().or(sources.mixed.as_deref());
    let voice_plays = !settings.mute_microphone
        && !ranges.is_empty()
        && ranges.iter().any(|range| gain(range).mic_gain() > 0.0);
    if let Some(path) = voice.filter(|_| voice_plays) {
        let input = first_input + inputs.len();
        let seek = source_seek(ranges);
        let gains: Vec<f64> = ranges.iter().map(|range| gain(range).mic_gain()).collect();
        inputs.push(GraphInput {
            path: path.to_path_buf(),
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
                gains: &gains,
                seek,
                preset: settings.microphone_preset.filter(),
                duration,
            },
        );
        labels.push("[mic]".to_owned());
    }

    let system_plays = !settings.mute_system
        && !ranges.is_empty()
        && ranges.iter().any(|range| gain(range).system_gain() > 0.0);
    if let Some(path) = sources.system.as_deref().filter(|_| system_plays) {
        let input = first_input + inputs.len();
        let seek = source_seek(ranges);
        let gains: Vec<f64> = ranges
            .iter()
            .map(|range| gain(range).system_gain())
            .collect();
        inputs.push(GraphInput {
            path: path.to_path_buf(),
            looped: false,
            seek,
        });
        push_track(
            &mut parts,
            Track {
                input,
                tag: 's',
                label: "sys",
                ranges,
                gains: &gains,
                seek,
                preset: None,
                duration,
            },
        );
        labels.push("[sys]".to_owned());
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
/// uses, the ranges it plays and the gain each of them plays at.
struct Track<'a> {
    input: usize,
    tag: char,
    label: &'a str,
    ranges: &'a [SoundRange],
    gains: &'a [f64],
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
        gains,
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
            stream.push_str(&format!(",volume={:.4}", gains[0]));
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
                    gains[index]
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

/// Whether the container has an audio stream at all.
pub(super) fn has_audio(path: &Path) -> bool {
    crate::utils::hidden_command(ffmpeg::ffprobe_executable())
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .is_ok_and(|output| output.status.success() && !output.stdout.trim_ascii().is_empty())
}

/// The flag that reads the filter graph from a file. Recent builds removed the
/// old name in favour of the file-prefixed form, so the answer is taken from
/// the build's own help and kept for the process.
fn graph_file_flag(help: &str) -> &'static str {
    if help.contains("-filter_complex_script") {
        "-filter_complex_script"
    } else {
        "-/filter_complex"
    }
}

/// The graph flag this ffmpeg takes. A build whose help cannot be read is
/// asked for the current flag, and a real mux failure still reports its own
/// error.
pub(super) fn graph_file_flag_for_this_ffmpeg() -> &'static str {
    static FLAG: OnceLock<&'static str> = OnceLock::new();
    FLAG.get_or_init(|| {
        let help = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args(["-hide_banner", "-h", "full"])
            .output()
            .map(|output| {
                let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
                text.push_str(&String::from_utf8_lossy(&output.stderr));
                text
            })
            .unwrap_or_default();
        graph_file_flag(&help)
    })
}

/// The recording's loudness over source time, for the timeline's sound lane.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Peaks {
    /// Seconds each value covers.
    pub bucket_seconds: f64,
    /// Peak level per bucket, 0..=1, from source time 0.
    pub values: Vec<f32>,
}

impl Peaks {
    /// The peak over source seconds `start..end` (max of the buckets touched);
    /// 0 outside the data.
    pub fn level(&self, start: f64, end: f64) -> f32 {
        if self.values.is_empty() || !self.bucket_seconds.is_finite() || self.bucket_seconds <= 0.0
        {
            return 0.0;
        }
        if !start.is_finite() || !end.is_finite() || end < 0.0 {
            return 0.0;
        }
        let first = (start.max(0.0) / self.bucket_seconds).floor() as usize;
        let last = (end.max(0.0) / self.bucket_seconds).floor() as usize;
        if first >= self.values.len() || first > last {
            return 0.0;
        }
        let last = last.min(self.values.len() - 1);
        self.values[first..=last]
            .iter()
            .copied()
            .fold(0.0, f32::max)
    }
}

/// Decode the sources (microphone and system mixed, or the mixed track) to mono
/// and measure peaks in 20 ms buckets. Blocking; None for a silent recording.
pub(super) fn measure_peaks(sources: &SoundSources) -> Option<Peaks> {
    let voice = sources.microphone.as_deref().or(sources.mixed.as_deref());
    let system = sources.system.as_deref();
    let mut command = crate::utils::hidden_command(ffmpeg::ffmpeg_executable());
    command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
    let mut count = 0;
    for path in [voice, system].into_iter().flatten() {
        command.arg("-i").arg(path);
        count += 1;
    }
    if count == 0 {
        return None;
    }
    if count == 2 {
        command.args([
            "-filter_complex",
            "[0:a:0][1:a:0]amix=inputs=2:normalize=0,aformat=sample_fmts=flt:sample_rates=8000:channel_layouts=mono[p]",
        ]);
        command.args(["-map", "[p]"]);
    } else {
        command.args(["-map", "0:a:0", "-ac", "1", "-ar", "8000"]);
    }
    let mut child = command
        .args(["-f", "f32le", "-"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let mut values: Vec<f32> = Vec::new();
    let mut peak = 0.0f32;
    let mut filled = 0usize;
    let mut carry = [0u8; 4];
    let mut carried = 0usize;
    let mut buffer = [0u8; 65536];
    loop {
        let read = match stdout.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(_) => break,
        };
        let mut rest: &[u8] = &buffer[..read];
        if carried > 0 {
            let take = (4 - carried).min(rest.len());
            carry[carried..carried + take].copy_from_slice(&rest[..take]);
            carried += take;
            rest = &rest[take..];
            if carried == 4 {
                peak = peak.max(sample_of(&carry).abs());
                filled += 1;
                if filled == PEAK_BUCKET {
                    values.push(peak.min(1.0));
                    peak = 0.0;
                    filled = 0;
                }
                carried = 0;
            }
        }
        if rest.is_empty() {
            continue;
        }
        let (samples, tail) = rest.as_chunks::<4>();
        for bytes in samples {
            peak = peak.max(f32::from_le_bytes(*bytes).abs());
            filled += 1;
            if filled == PEAK_BUCKET {
                values.push(peak.min(1.0));
                peak = 0.0;
                filled = 0;
            }
        }
        carry[..tail.len()].copy_from_slice(tail);
        carried = tail.len();
    }
    if filled > 0 {
        values.push(peak.min(1.0));
    }
    drop(stdout);
    let finished = child.wait();
    if !finished.is_ok_and(|status| status.success()) || values.is_empty() {
        return None;
    }
    Some(Peaks {
        bucket_seconds: PEAK_BUCKET as f64 / PEAK_RATE as f64,
        values,
    })
}

fn sample_of(bytes: &[u8]) -> f32 {
    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::sidecar::{CaptureRegion, PressSample};
    use crate::video_editor::export::{frame_plan, total_frames, SourceSpan};

    fn span(
        composition_start: f64,
        source_start: f64,
        source_end: f64,
        speed: f64,
        frames: u64,
    ) -> SourceSpan {
        SourceSpan {
            composition_start,
            source_start,
            source_end,
            speed,
            frames,
        }
    }

    fn range(start: f64, end: f64, speed: f64, duration: f64) -> SoundRange {
        SoundRange {
            start,
            end,
            speed,
            duration,
            segment: 0,
        }
    }

    fn piece(start: f64, end: f64, speed: f64, duration: f64, segment: usize) -> SoundRange {
        SoundRange {
            start,
            end,
            speed,
            duration,
            segment,
        }
    }

    fn mic() -> SoundSources {
        SoundSources {
            microphone: Some(PathBuf::from("mic.m4a")),
            ..SoundSources::default()
        }
    }

    fn plain() -> SoundSettings {
        SoundSettings {
            microphone_preset: MicPreset::Off,
            ..SoundSettings::default()
        }
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

    #[test]
    fn adjacent_spans_of_one_segment_join_into_one_range() {
        let spans = [span(0.0, 0.0, 1.0, 1.0, 30), span(1.0, 1.0, 2.0, 1.0, 30)];
        assert_eq!(
            sound_ranges(&spans, 30.0, 0.0, 2.0, &[]),
            vec![range(0.0, 2.0, 1.0, 2.0)]
        );
    }

    #[test]
    fn spans_with_a_gap_keep_separate_ranges() {
        let spans = [span(0.0, 0.0, 1.0, 1.0, 30), span(1.0, 2.0, 3.0, 1.0, 30)];
        assert_eq!(
            sound_ranges(&spans, 30.0, 0.0, 3.0, &[]),
            vec![range(0.0, 1.0, 1.0, 1.0), range(2.0, 3.0, 1.0, 1.0)]
        );
    }

    #[test]
    fn ranges_never_merge_across_segments() {
        let spans = frame_plan(0.0, 4.0, &[2.0], &[], 30.0);
        assert_eq!(spans.len(), 2, "{spans:?}");
        let ranges = sound_ranges(&spans, 30.0, 0.0, 4.0, &[2.0]);
        assert_eq!(ranges.len(), 2, "{ranges:?}");
        assert_eq!(ranges[0].segment, 0);
        assert_eq!(ranges[1].segment, 1);
        assert!((ranges[0].end - ranges[1].start).abs() < 1e-9, "{ranges:?}");
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
        let graph = build_graph(
            &mic(),
            &plain(),
            &[],
            &[range(1.0, 3.0, 1.0, 2.0)],
            2.0,
            &[],
            None,
            0.0,
            1,
        )
        .expect("the microphone plays");
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
    fn the_graph_file_flag_follows_the_ffmpeg_help() {
        let legacy =
            "-filter_complex_script <string> .F.... set the global complex filtergraph from a file";
        assert_eq!(graph_file_flag(legacy), "-filter_complex_script");
        let current =
            "-/filter_complex <string> .F.... set the global complex filtergraph from a file";
        assert_eq!(graph_file_flag(current), "-/filter_complex");
        assert_eq!(graph_file_flag(""), "-/filter_complex");
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
        let spans = [
            span(0.0, 0.0, 1.0, 1.0, 30),
            span(1.0, 0.966_667, 0.966_667, 0.0, 30),
        ];
        let ranges = sound_ranges(&spans, 30.0, 0.0, 2.0, &[]);
        assert_eq!(ranges, vec![range(0.0, 1.0, 1.0, 1.0)]);
        let graph = build_graph(&mic(), &plain(), &[], &ranges, 2.0, &[], None, 0.0, 1)
            .expect("the microphone plays");
        assert!(
            graph.graph.contains(",apad,atrim=end=2.000000[mic]"),
            "{}",
            graph.graph
        );
    }

    #[test]
    fn a_gap_between_pieces_keeps_silence_in_the_sound() {
        let spans = [
            span(0.0, 0.0, 2.0, 1.0, 60),
            span(2.0, 1.966_667, 1.966_667, 0.0, 60),
            span(4.0, 2.0, 4.0, 1.0, 60),
        ];
        let ranges = sound_ranges(&spans, 30.0, 0.0, 4.0, &[2.0]);
        assert_eq!(ranges.len(), 3, "{ranges:?}");
        assert_eq!(ranges[0], piece(0.0, 2.0, 1.0, 2.0, 0));
        assert_eq!(ranges[1].start, ranges[1].end, "{ranges:?}");
        assert!((ranges[1].duration - 2.0).abs() < 1e-9, "{ranges:?}");
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
    fn different_speeds_keep_separate_ranges() {
        let spans = [span(0.0, 0.0, 1.0, 1.0, 30), span(1.0, 1.0, 3.0, 2.0, 30)];
        let ranges = sound_ranges(&spans, 30.0, 0.0, 3.0, &[]);
        assert_eq!(
            ranges,
            vec![range(0.0, 1.0, 1.0, 1.0), range(1.0, 3.0, 2.0, 1.0)]
        );
        let graph = build_graph(&mic(), &plain(), &[], &ranges, 2.0, &[], None, 0.0, 1)
            .expect("the microphone plays");
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
    fn a_muted_clip_keeps_its_slot_silent() {
        let ranges = [piece(0.0, 1.0, 1.0, 1.0, 0), piece(1.0, 2.0, 1.0, 1.0, 1)];
        let clips = [
            ClipSound::default(),
            ClipSound {
                muted: true,
                ..ClipSound::default()
            },
        ];
        let graph = build_graph(&mic(), &plain(), &clips, &ranges, 2.0, &[], None, 0.0, 1)
            .expect("the microphone plays");
        assert!(graph.graph.contains("volume=0.0000"), "{}", graph.graph);
        assert!(graph.graph.contains("concat=n=2"), "{}", graph.graph);
    }

    #[test]
    fn nothing_audible_builds_no_graph() {
        assert!(build_graph(
            &SoundSources::default(),
            &SoundSettings::default(),
            &[],
            &[],
            2.0,
            &[],
            None,
            0.0,
            0,
        )
        .is_none());
        let muted = SoundSettings {
            mute_microphone: true,
            ..SoundSettings::default()
        };
        assert!(build_graph(
            &mic(),
            &muted,
            &[],
            &[range(0.0, 1.0, 1.0, 1.0)],
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
        let graph = build_graph(&mic(), &settings, &[], &ranges, 2.0, &[], None, 0.0, 1)
            .expect("the microphone plays");
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
        let music = std::env::temp_dir().join(format!("apexshot-sound-bg-{}", std::process::id()));
        std::fs::write(&music, b"music").unwrap();
        let settings = SoundSettings {
            background_file: Some(music.clone()),
            ..SoundSettings::default()
        };
        let graph = build_graph(
            &SoundSources::default(),
            &settings,
            &[],
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
        let _ = std::fs::remove_file(&music);
    }

    #[test]
    fn a_click_inside_a_fast_span_maps_through_its_speed() {
        let mut sidecar = sidecar();
        sidecar.presses.push(PressSample {
            down: 2.0,
            up: 2.05,
            button: 1,
            dragged: false,
        });
        let double = [span(0.0, 0.0, 10.0, 2.0, 150)];
        assert_eq!(
            click_cues(&sidecar, &double, 1.0),
            vec![ClickCue {
                delay_ms: 1000,
                gain: 1.0
            }]
        );
        let triple = [span(0.0, 0.0, 10.0, 3.0, 100)];
        assert_eq!(click_cues(&sidecar, &triple, 1.0), Vec::new());
        let held = [span(0.0, 0.0, 0.0, 0.0, 30)];
        assert_eq!(click_cues(&sidecar, &held, 1.0), Vec::new());
    }

    #[test]
    fn many_short_sped_segments_keep_each_ranges_exact_length() {
        let fps = 30.0;
        let cuts: Vec<f64> = (1..20).map(|step| step as f64 * 0.1).collect();
        let speeds: Vec<f64> = (0..20)
            .map(|index| if index % 2 == 0 { 3.0 } else { 1.5 })
            .collect();
        let spans = frame_plan(0.0, 2.0, &cuts, &speeds, fps);
        assert_eq!(spans.len(), 20);
        let total = total_frames(&spans) as f64 / fps;
        let ranges = sound_ranges(&spans, fps, 0.0, 2.0, &cuts);
        assert_eq!(ranges.len(), spans.len(), "{ranges:?}");
        let filled: f64 = ranges.iter().map(|range| range.duration).sum();
        assert!((filled - total).abs() < 1e-9, "{filled} vs {total}");
        for range in &ranges {
            let chain = range_chain(*range, 0.0);
            let limit = format!("atrim=end={:.6}", range.duration);
            assert!(chain.contains(limit.as_str()), "{chain}");
        }
        let first = ranges[0];
        assert!((first.duration - 1.0 / fps).abs() < 1e-9, "{first:?}");
        let second = ranges[1];
        assert!((second.duration - 2.0 / fps).abs() < 1e-9, "{second:?}");
    }

    #[test]
    fn a_slow_span_fills_its_whole_output_length_before_the_next_range() {
        let spans = [span(0.0, 0.0, 1.0, 0.5, 60), span(2.0, 1.0, 2.0, 1.0, 30)];
        let ranges = sound_ranges(&spans, 30.0, 0.0, 3.0, &[]);
        assert_eq!(
            ranges,
            vec![range(0.0, 1.0, 0.5, 2.0), range(1.0, 2.0, 1.0, 1.0)]
        );
        let filled: f64 = ranges.iter().map(|range| range.duration).sum();
        assert!((filled - 3.0).abs() < 1e-9, "{filled}");
        assert_eq!(
            range_chain(ranges[0], 0.0),
            "atrim=start=0.000000:end=1.000000,asetpts=PTS-STARTPTS,atempo=0.500000,apad,atrim=end=2.000000"
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
            &mic(),
            &plain(),
            &[],
            &ranges,
            2.0,
            &cues,
            Some(&click),
            0.0,
            1,
        )
        .expect("the cues play over the microphone");
        assert_eq!(graph.inputs.len(), 2);
        assert_eq!(graph.inputs[1].path, click);
        assert_eq!(
            graph.graph,
            format!(
                "[1:a:0]{FORMAT},atrim=start=0.000000:end=2.000000,asetpts=PTS-STARTPTS,volume=1.0000,apad,atrim=end=2.000000[mic];[2:a:0]{FORMAT},asplit=2[c0][c1];[c0]adelay=500:all=1,volume=0.8000[e0];[c1]adelay=1250:all=1,volume=0.6000[e1];[mic][e0][e1]amix=inputs=3:normalize=0:duration=longest:dropout_transition=0,volume=1.0000,atrim=end=2.000000,{FORMAT}[a]"
            )
        );

        let silent = build_graph(
            &SoundSources::default(),
            &plain(),
            &[],
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
    fn click_cues_follow_the_press_kind_and_volume() {
        let mut sidecar = sidecar();
        sidecar.presses.push(PressSample {
            down: 1.0,
            up: 1.05,
            button: 1,
            dragged: false,
        });
        sidecar.presses.push(PressSample {
            down: 2.0,
            up: 2.5,
            button: 1,
            dragged: false,
        });
        let spans = [span(0.0, 0.0, 10.0, 1.0, 300)];
        assert_eq!(
            click_cues(&sidecar, &spans, 1.0),
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
    }

    #[test]
    fn peaks_level_takes_the_loudest_bucket() {
        let peaks = Peaks {
            bucket_seconds: 0.02,
            values: vec![0.1, 0.9, 0.2],
        };
        assert!((peaks.level(0.0, 0.0) - 0.1).abs() < 1e-6);
        assert!((peaks.level(0.02, 0.04) - 0.9).abs() < 1e-6);
        assert!((peaks.level(0.0, 0.06) - 0.9).abs() < 1e-6);
        assert_eq!(peaks.level(1.0, 2.0), 0.0);
        assert_eq!(peaks.level(-1.0, -0.5), 0.0);
    }

    #[test]
    fn settings_clamp_volumes() {
        let wild = SoundSettings {
            volume: 3.0,
            background_volume: -1.0,
            microphone_preset: MicPreset::Podcast,
            ..SoundSettings::default()
        }
        .clamped();
        assert_eq!(wild.volume, 1.0);
        assert_eq!(wild.background_volume, 0.0);
        assert_eq!(wild.microphone_preset, MicPreset::Podcast);

        let broken = SoundSettings {
            volume: f64::NAN,
            background_volume: f64::INFINITY,
            ..SoundSettings::default()
        }
        .clamped();
        assert_eq!(broken.volume, 1.0);
        assert_eq!(broken.background_volume, 0.05);

        let clip = ClipSound {
            mic_volume: 2.0,
            system_volume: f64::NAN,
            muted: true,
        }
        .clamped();
        assert_eq!(clip.mic_volume, 1.0);
        assert_eq!(clip.system_volume, 1.0);
        assert_eq!(clip.mic_gain(), 0.0);
        assert_eq!(clip.system_gain(), 0.0);

        assert_eq!(SoundSettings::default().volume, 1.0);
        assert_eq!(
            SoundSettings::default().microphone_preset,
            MicPreset::Standard
        );
        assert!(!SoundSettings::default().background_active());
        let quiet = SoundSettings {
            background_file: Some(PathBuf::from("music.m4a")),
            background_volume: 0.0,
            ..SoundSettings::default()
        };
        assert!(!quiet.background_active());
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
        }
    }

    #[test]
    fn a_full_graph_mixes_every_source_for_its_exact_length() {
        let dir = std::env::temp_dir().join(format!("apexshot-sound-graph-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mic = dir.join("mic.m4a");
        let system = dir.join("system.m4a");
        let music = dir.join("music.m4a");
        for (path, tone) in [
            (&mic, "sine=f=440:d=6"),
            (&system, "sine=f=880:d=6"),
            (&music, "sine=f=220:d=1.5"),
        ] {
            let generated =
                crate::utils::hidden_command(crate::recording::editor::ffmpeg::ffmpeg_executable())
                    .args(["-y", "-hide_banner", "-loglevel", "error"])
                    .args(["-f", "lavfi", "-i", tone])
                    .args(["-c:a", "aac"])
                    .arg(path)
                    .output();
            if !generated.is_ok_and(|output| output.status.success()) {
                eprintln!("skipping the sound graph test: ffmpeg is unavailable");
                let _ = std::fs::remove_dir_all(&dir);
                return;
            }
        }
        let click = dir.join("click.wav");
        std::fs::write(&click, super::click_sound::WAV).unwrap();

        let spans = frame_plan(0.0, 6.0, &[2.0, 4.0], &[1.0, 2.0, 1.0], 30.0);
        let duration = total_frames(&spans) as f64 / 30.0;
        assert!((duration - 5.0).abs() < 1e-9, "{duration}");
        let plan = SoundPlan {
            sources: SoundSources {
                microphone: Some(mic),
                system: Some(system),
                mixed: None,
            },
            settings: SoundSettings {
                background_file: Some(music),
                background_volume: 0.5,
                ..SoundSettings::default()
            },
            clips: vec![
                ClipSound::default(),
                ClipSound::default(),
                ClipSound {
                    muted: true,
                    ..ClipSound::default()
                },
            ],
            ranges: sound_ranges(&spans, 30.0, 0.0, 6.0, &[2.0, 4.0]),
            duration,
            cues: vec![ClickCue {
                delay_ms: 1000,
                gain: 1.0,
            }],
        };
        let from_zero = plan
            .graph_from(0.0, Some(&click), 0)
            .expect("the mix plays");
        let from_middle = plan
            .graph_from(2.5, Some(&click), 0)
            .expect("the rest plays");

        for (from, graph) in [(0.0, &from_zero), (2.5, &from_middle)] {
            let script = dir.join(format!("graph-{from}.txt"));
            std::fs::write(&script, &graph.graph).unwrap();
            let mut command =
                crate::utils::hidden_command(crate::recording::editor::ffmpeg::ffmpeg_executable());
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
            command.arg(graph_file_flag_for_this_ffmpeg()).arg(&script);
            command.args([
                "-map", "[a]", "-f", "f32le", "-ac", "1", "-ar", "48000", "-",
            ]);
            let decoded = command.output().expect("ffmpeg runs");
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
                assert!(loud > 0.05, "the three sources are audible: {loud:.3}");
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
