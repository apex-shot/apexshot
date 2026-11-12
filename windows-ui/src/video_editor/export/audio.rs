//! The recording's sound, cut to the exported spans and muxed into the
//! finished video. The video stream is copied, never re-encoded; the sound is
//! AAC at 192 kbps and is padded with silence to the video's length.

use super::{End, ExportModel, ExportSettings, SourceSpan};
use crate::recording::editor::ffmpeg;
use crate::recording::editor::sidecar::PointerSidecar;
use crate::video_editor::click_sound::{self, Kind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

/// Add the recording's sound and its click sounds to the finished export at
/// `output`. A recording with neither leaves the file as it is.
pub(super) fn add(
    model: &ExportModel,
    settings: ExportSettings,
    output: &Path,
) -> Result<(), String> {
    let fallback = [model.source.clone()];
    let candidates: &[PathBuf] = if model.audio.is_empty() {
        &fallback
    } else {
        &model.audio
    };
    let source = candidates.iter().find(|path| has_audio(path));
    let clicks = model.sidecar.as_ref().filter(|_| {
        model.effects.click_sound.as_deref() == Some("soft")
            && model.effects.click_sound_volume > 0.0
    });
    if source.is_none() && clicks.is_none() {
        return Ok(());
    }
    let plan = super::plan_render(model, settings).map_err(|end| match end {
        End::Failed(message) => message,
        End::Cancelled => "the export was cancelled".to_string(),
    })?;
    let ranges = source_ranges(&plan.spans, plan.fps);
    let duration = plan.total as f64 / plan.fps;
    let cues = clicks.map_or_else(Vec::new, |sidecar| {
        click_cues(sidecar, &plan.spans, model.effects.click_sound_volume)
    });
    if source.is_none() && cues.is_empty() {
        return Ok(());
    }
    mux(
        output,
        source.map(PathBuf::as_path),
        &ranges,
        duration,
        settings.codec == super::Codec::Hevc,
        &cues,
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Cue {
    delay_ms: u64,
    gain: f64,
}

fn click_cues(sidecar: &PointerSidecar, spans: &[SourceSpan], volume: f64) -> Vec<Cue> {
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
            Cue {
                delay_ms: (event.composition_time * 1000.0).round() as u64,
                gain: volume * scale,
            }
        })
        .collect()
}

/// One kept stretch of the recording's sound, with the speed it plays at and
/// the exact composition length it has to fill.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SourceRange {
    pub start: f64,
    pub end: f64,
    pub speed: f64,
    pub duration: f64,
}

/// The source-time ranges of the kept footage, each with its own speed. Spans
/// that continue where the previous one ended at the same speed are merged, so
/// a split that removes nothing leaves no seam in the sound; a held span
/// contributes no source sound, because it plays none.
pub(super) fn source_ranges(spans: &[SourceSpan], fps: f64) -> Vec<SourceRange> {
    let mut ranges: Vec<SourceRange> = Vec::new();
    for span in spans {
        if !span.speed.is_finite() || span.speed <= 0.0 {
            continue;
        }
        let range = SourceRange {
            start: span.source_start,
            end: span.source_end,
            speed: span.speed,
            duration: span.frames as f64 / fps,
        };
        match ranges.last_mut() {
            Some(last)
                if last.speed == range.speed && (range.start - last.end).abs() < 0.5 / fps =>
            {
                last.end = range.end;
                last.duration += range.duration;
            }
            _ => ranges.push(range),
        }
    }
    ranges
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
fn range_chain(range: SourceRange) -> String {
    let mut chain = format!(
        "atrim=start={:.6}:end={:.6},asetpts=PTS-STARTPTS",
        range.start, range.end
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

fn has_audio(path: &Path) -> bool {
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

/// The filter graph that cuts the sound to `ranges` and pads it with silence,
/// so a held tail plays silence for its whole length.
fn cut_chain(ranges: &[SourceRange], duration: f64) -> String {
    match ranges {
        [] => format!("anullsrc=cl=stereo:r=48000,atrim=duration={duration:.6}"),
        [range] => format!("[1:a:0]{},apad", range_chain(*range)),
        ranges => {
            let count = ranges.len();
            let mut graph = format!("[1:a:0]asplit={count}");
            for index in 0..count {
                graph.push_str(&format!("[s{index}]"));
            }
            graph.push(';');
            for (index, range) in ranges.iter().enumerate() {
                graph.push_str(&format!("[s{index}]{}[a{index}];", range_chain(*range)));
            }
            for index in 0..count {
                graph.push_str(&format!("[a{index}]"));
            }
            graph.push_str(&format!("concat=n={count}:v=0:a=1,apad"));
            graph
        }
    }
}

/// The filter graph that cuts the sound to `ranges` and pads it with silence.
/// Input 1 is the sound source.
fn filter_graph(ranges: &[SourceRange], duration: f64) -> String {
    format!("{}[a]", cut_chain(ranges, duration))
}

/// The filter graph that mixes the cut sound with one click per cue. The click
/// sample is read once and split per cue; each branch is delayed to its time
/// and scaled, and the mix keeps the source's own level.
fn mixed_graph(
    source: Option<&[SourceRange]>,
    duration: f64,
    cues: &[Cue],
    click_input: usize,
) -> String {
    let base = match source {
        Some(ranges) => format!(
            "{},aformat=channel_layouts=stereo[base]",
            cut_chain(ranges, duration)
        ),
        None => format!("anullsrc=cl=stereo:r=48000,atrim=duration={duration:.6}[base]"),
    };
    let count = cues.len();
    let mut graph =
        format!("{base};[{click_input}:a:0]aformat=channel_layouts=stereo,asplit={count}");
    for index in 0..count {
        graph.push_str(&format!("[c{index}]"));
    }
    graph.push(';');
    for (index, cue) in cues.iter().enumerate() {
        graph.push_str(&format!(
            "[c{index}]adelay={}:all=1,volume={:.4}[e{index}];",
            cue.delay_ms, cue.gain
        ));
    }
    graph.push_str("[base]");
    for index in 0..count {
        graph.push_str(&format!("[e{index}]"));
    }
    graph.push_str(&format!(
        "amix=inputs={}:normalize=0:duration=first,apad[a]",
        count + 1
    ));
    graph
}

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn unique() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn create() -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("apexshot-sound-{}", unique()));
        std::fs::create_dir(&dir)
            .map_err(|error| format!("could not create the sound scratch folder: {error}"))?;
        Ok(Self { dir })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
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
fn graph_file_flag_for_this_ffmpeg() -> &'static str {
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

fn mux(
    video: &Path,
    source: Option<&Path>,
    ranges: &[SourceRange],
    duration: f64,
    hevc: bool,
    cues: &[Cue],
) -> Result<(), String> {
    let scratch = Scratch::create()?;
    let graph = if cues.is_empty() {
        filter_graph(ranges, duration)
    } else {
        let click_input = if source.is_some() { 2 } else { 1 };
        mixed_graph(source.map(|_| ranges), duration, cues, click_input)
    };
    let script = scratch.dir.join("audio.txt");
    std::fs::write(&script, graph)
        .map_err(|error| format!("could not write the sound graph: {error}"))?;
    let temp = video.with_extension(format!("sound-{}.mp4", unique()));
    let mut command = crate::utils::hidden_command(ffmpeg::ffmpeg_executable());
    command
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
        .arg("-i")
        .arg(video);
    if let Some(source) = source {
        command.arg("-i").arg(source);
    }
    if !cues.is_empty() {
        let click = scratch.dir.join("click-soft.wav");
        std::fs::write(&click, click_sound::WAV)
            .map_err(|error| format!("could not write the click sound: {error}"))?;
        command.arg("-i").arg(&click);
    }
    command
        .arg(graph_file_flag_for_this_ffmpeg())
        .arg(&script)
        .args(["-map", "0:v:0", "-map", "[a]"])
        .args(["-c:v", "copy"]);
    if hevc {
        command.args(["-tag:v", "hvc1"]);
    }
    let output = command
        .args(["-c:a", "aac", "-b:a", "192k"])
        .arg("-t")
        .arg(format!("{duration:.6}"))
        .args(["-movflags", "+faststart", "-f", "mp4"])
        .arg(&temp)
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("could not start ffmpeg: {error}"));
        }
    };
    if !output.status.success() {
        let _ = std::fs::remove_file(&temp);
        return Err(format!(
            "ffmpeg could not add the sound: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if let Err(error) = std::fs::rename(&temp, video) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("could not replace the export: {error}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::ExportUpdate;
    use super::*;
    use crate::recording::editor::sidecar::PressSample;
    use crate::video_editor::project::EffectsSettings;
    use std::process::Stdio;
    use std::time::Duration;

    fn range(start: f64, end: f64, speed: f64, duration: f64) -> SourceRange {
        SourceRange {
            start,
            end,
            speed,
            duration,
        }
    }

    #[test]
    fn kept_spans_join_into_one_range_when_nothing_is_removed() {
        let spans = super::super::frame_plan(1.0, 9.0, &[3.0, 5.0], &[], 30.0);
        let ranges = source_ranges(&spans, 30.0);
        assert_eq!(ranges.len(), 1, "{ranges:?}");
        assert!((ranges[0].start - 1.0).abs() < 1e-9, "{ranges:?}");
        assert!((ranges[0].end - 9.0).abs() < 1e-9, "{ranges:?}");
        assert!((ranges[0].speed - 1.0).abs() < 1e-9, "{ranges:?}");
    }

    #[test]
    fn spans_with_a_gap_keep_separate_ranges() {
        let spans = [
            SourceSpan {
                composition_start: 0.0,
                source_start: 0.0,
                source_end: 1.0,
                speed: 1.0,
                frames: 30,
            },
            SourceSpan {
                composition_start: 1.0,
                source_start: 2.0,
                source_end: 3.0,
                speed: 1.0,
                frames: 30,
            },
        ];
        let ranges = source_ranges(&spans, 30.0);
        assert_eq!(ranges.len(), 2, "{ranges:?}");
        assert!((ranges[0].start - 0.0).abs() < 1e-9, "{ranges:?}");
        assert!((ranges[0].end - 1.0).abs() < 1e-9, "{ranges:?}");
        assert!((ranges[1].start - 2.0).abs() < 1e-9, "{ranges:?}");
        assert!((ranges[1].end - 3.0).abs() < 1e-9, "{ranges:?}");
    }

    #[test]
    fn the_filter_graph_trims_pads_and_joins() {
        assert_eq!(
            filter_graph(&[range(1.0, 3.0, 1.0, 2.0)], 2.0),
            "[1:a:0]atrim=start=1.000000:end=3.000000,asetpts=PTS-STARTPTS,apad[a]"
        );
        assert_eq!(
            filter_graph(&[range(0.0, 1.0, 1.0, 1.0), range(2.0, 3.0, 1.0, 1.0)], 2.0),
            "[1:a:0]asplit=2[s0][s1];[s0]atrim=start=0.000000:end=1.000000,asetpts=PTS-STARTPTS[a0];[s1]atrim=start=2.000000:end=3.000000,asetpts=PTS-STARTPTS[a1];[a0][a1]concat=n=2:v=0:a=1,apad[a]"
        );
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
            filter_graph(&[range(1.0, 4.0, 3.0, 1.0)], 1.0),
            "[1:a:0]atrim=start=1.000000:end=4.000000,asetpts=PTS-STARTPTS,atempo=2.000000,atempo=1.500000,apad,atrim=end=1.000000,apad[a]"
        );
        assert_eq!(
            filter_graph(&[range(0.0, 8.0, 8.0, 1.0)], 1.0),
            "[1:a:0]atrim=start=0.000000:end=8.000000,asetpts=PTS-STARTPTS,atempo=2.000000,atempo=2.000000,atempo=2.000000,apad,atrim=end=1.000000,apad[a]"
        );
        assert_eq!(
            filter_graph(&[range(2.0, 10.0, 0.125, 64.0)], 64.0),
            "[1:a:0]atrim=start=2.000000:end=10.000000,asetpts=PTS-STARTPTS,atempo=0.500000,atempo=0.500000,atempo=0.500000,apad,atrim=end=64.000000,apad[a]"
        );
    }

    #[test]
    fn a_held_tail_gets_silence_instead_of_looped_sound() {
        let spans = [
            SourceSpan {
                composition_start: 0.0,
                source_start: 0.0,
                source_end: 1.0,
                speed: 1.0,
                frames: 30,
            },
            SourceSpan {
                composition_start: 1.0,
                source_start: 0.966_667,
                source_end: 0.966_667,
                speed: 0.0,
                frames: 30,
            },
        ];
        let ranges = source_ranges(&spans, 30.0);
        assert_eq!(ranges, vec![range(0.0, 1.0, 1.0, 1.0)]);
        assert_eq!(
            filter_graph(&ranges, 2.0),
            "[1:a:0]atrim=start=0.000000:end=1.000000,asetpts=PTS-STARTPTS,apad[a]"
        );
        assert_eq!(
            filter_graph(&[], 1.0),
            "anullsrc=cl=stereo:r=48000,atrim=duration=1.000000[a]"
        );
    }

    #[test]
    fn different_speeds_keep_separate_ranges() {
        let spans = [
            SourceSpan {
                composition_start: 0.0,
                source_start: 0.0,
                source_end: 1.0,
                speed: 1.0,
                frames: 30,
            },
            SourceSpan {
                composition_start: 1.0,
                source_start: 1.0,
                source_end: 3.0,
                speed: 2.0,
                frames: 30,
            },
        ];
        let ranges = source_ranges(&spans, 30.0);
        assert_eq!(
            ranges,
            vec![range(0.0, 1.0, 1.0, 1.0), range(1.0, 3.0, 2.0, 1.0)]
        );
        assert_eq!(
            filter_graph(&ranges, 2.0),
            "[1:a:0]asplit=2[s0][s1];[s0]atrim=start=0.000000:end=1.000000,asetpts=PTS-STARTPTS[a0];[s1]atrim=start=1.000000:end=3.000000,asetpts=PTS-STARTPTS,atempo=2.000000,apad,atrim=end=1.000000[a1];[a0][a1]concat=n=2:v=0:a=1,apad[a]"
        );
    }

    #[test]
    fn a_click_inside_a_fast_span_maps_through_its_speed() {
        let mut sidecar = PointerSidecar::new(
            0,
            crate::recording::editor::sidecar::CaptureRegion {
                x: 0,
                y: 0,
                w: 320,
                h: 240,
            },
        );
        sidecar.presses.push(PressSample {
            down: 2.0,
            up: 2.05,
            button: 1,
            dragged: false,
        });
        let double = [SourceSpan {
            composition_start: 0.0,
            source_start: 0.0,
            source_end: 10.0,
            speed: 2.0,
            frames: 150,
        }];
        assert_eq!(
            click_cues(&sidecar, &double, 1.0),
            vec![Cue {
                delay_ms: 1000,
                gain: 1.0
            }]
        );
        let triple = [SourceSpan {
            composition_start: 0.0,
            source_start: 0.0,
            source_end: 10.0,
            speed: 3.0,
            frames: 100,
        }];
        assert_eq!(click_cues(&sidecar, &triple, 1.0), Vec::new());
        let held = [SourceSpan {
            composition_start: 0.0,
            source_start: 0.0,
            source_end: 0.0,
            speed: 0.0,
            frames: 30,
        }];
        assert_eq!(click_cues(&sidecar, &held, 1.0), Vec::new());
    }

    #[test]
    fn many_short_sped_segments_keep_each_ranges_exact_length() {
        let fps = 30.0;
        let cuts: Vec<f64> = (1..20).map(|step| step as f64 * 0.1).collect();
        let speeds: Vec<f64> = (0..20)
            .map(|index| if index % 2 == 0 { 3.0 } else { 1.5 })
            .collect();
        let spans = super::super::frame_plan(0.0, 2.0, &cuts, &speeds, fps);
        assert_eq!(spans.len(), 20);
        let total = super::super::total_frames(&spans) as f64 / fps;
        let ranges = source_ranges(&spans, fps);
        assert_eq!(ranges.len(), spans.len(), "{ranges:?}");
        let filled: f64 = ranges.iter().map(|range| range.duration).sum();
        assert!((filled - total).abs() < 1e-9, "{filled} vs {total}");
        for range in &ranges {
            let graph = filter_graph(&[*range], total);
            let limit = format!("atrim=end={:.6}", range.duration);
            assert!(graph.contains(limit.as_str()), "{graph}");
        }
        let first = ranges[0];
        assert!((first.duration - 1.0 / fps).abs() < 1e-9, "{first:?}");
        let second = ranges[1];
        assert!((second.duration - 2.0 / fps).abs() < 1e-9, "{second:?}");
    }

    #[test]
    fn a_slow_span_fills_its_whole_output_length_before_the_next_range() {
        let fps = 30.0;
        let spans = [
            SourceSpan {
                composition_start: 0.0,
                source_start: 0.0,
                source_end: 1.0,
                speed: 0.5,
                frames: 60,
            },
            SourceSpan {
                composition_start: 2.0,
                source_start: 1.0,
                source_end: 2.0,
                speed: 1.0,
                frames: 30,
            },
        ];
        let ranges = source_ranges(&spans, fps);
        assert_eq!(
            ranges,
            vec![range(0.0, 1.0, 0.5, 2.0), range(1.0, 2.0, 1.0, 1.0)]
        );
        let filled: f64 = ranges.iter().map(|range| range.duration).sum();
        assert!((filled - 3.0).abs() < 1e-9, "{filled}");
        assert_eq!(
            filter_graph(&ranges[..1], 3.0),
            "[1:a:0]atrim=start=0.000000:end=1.000000,asetpts=PTS-STARTPTS,atempo=0.500000,apad,atrim=end=2.000000,apad[a]"
        );
        assert_eq!(
            filter_graph(&ranges, 3.0),
            "[1:a:0]asplit=2[s0][s1];[s0]atrim=start=0.000000:end=1.000000,asetpts=PTS-STARTPTS,atempo=0.500000,apad,atrim=end=2.000000[a0];[s1]atrim=start=1.000000:end=2.000000,asetpts=PTS-STARTPTS[a1];[a0][a1]concat=n=2:v=0:a=1,apad[a]"
        );
    }

    #[test]
    fn mixed_speeds_and_a_freeze_tail_keep_the_sound_in_sync() {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-export-speed-sound-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("tone.mp4");
        let generated = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args(["-y", "-hide_banner", "-loglevel", "error"])
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=30:duration=6",
            ])
            .args([
                "-f",
                "lavfi",
                "-i",
                "aevalsrc=if(between(t\\,2\\,3)\\,0.5*sin(2*PI*440*t)\\,0):s=48000:d=6",
            ])
            .args(["-pix_fmt", "yuv420p", "-c:v", "libx264", "-c:a", "aac"])
            .arg(&source)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !generated.is_ok_and(|status| status.success()) {
            eprintln!("skipping the sped export sound test: ffmpeg is unavailable");
            return;
        }
        let model = ExportModel {
            source: source.clone(),
            source_size: (320, 240),
            trim_start: 0.0,
            trim_end: 6.0,
            cuts: vec![2.0],
            segment_speeds: vec![1.0, 2.0],
            freeze_tail: 1.0,
            zooms: Vec::new(),
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: crate::recording::editor::model::CursorSettings::default(),
            sidecar: None,
            webcam: None,
            stage: None,
            canvas: None,
            crf: 30,
            audio: Vec::new(),
            effects: Default::default(),
        };
        let output = dir.join("out.mp4");
        let handle = super::super::start_export_with_settings(
            model,
            output.clone(),
            ExportSettings {
                hardware: false,
                ..ExportSettings::default()
            },
        );
        loop {
            match handle.receiver.recv_timeout(Duration::from_secs(180)) {
                Ok(ExportUpdate::Progress { .. }) => {}
                Ok(ExportUpdate::Finished(path, sound)) => {
                    assert_eq!(path, output);
                    assert!(sound.is_none(), "{sound:?}");
                    break;
                }
                Ok(ExportUpdate::Failed(message)) => panic!("export failed: {message}"),
                Ok(ExportUpdate::Cancelled) => panic!("export cancelled itself"),
                Err(error) => panic!("export did not report back: {error}"),
            }
        }
        let stream_duration = |stream: &str| -> f64 {
            let probe = crate::utils::hidden_command(ffmpeg::ffprobe_executable())
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    stream,
                    "-show_entries",
                    "stream=duration",
                    "-of",
                    "csv=p=0",
                ])
                .arg(&output)
                .output()
                .unwrap();
            String::from_utf8_lossy(&probe.stdout)
                .trim()
                .parse()
                .unwrap()
        };
        let video = stream_duration("v:0");
        let audio = stream_duration("a:0");
        assert!((video - 5.0).abs() <= 2.0 / 30.0, "the video is {video} s");
        assert!(
            (audio - video).abs() <= 1.0 / 30.0 + 0.05,
            "the sound ({audio}) does not match the video ({video})"
        );
        let detected = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args(["-hide_banner", "-nostdin", "-i"])
            .arg(&output)
            .args(["-af", "silencedetect=n=-30dB:d=0.2", "-f", "null", "-"])
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&detected.stderr);
        let values = |marker: &str| -> Vec<f64> {
            log.lines()
                .filter_map(|line| line.split(marker).nth(1))
                .filter_map(|tail| {
                    tail.trim_start()
                        .split(|c: char| c.is_whitespace() || c == '|')
                        .next()
                        .and_then(|value| value.parse().ok())
                })
                .collect()
        };
        let ends = values("silence_end: ");
        let starts = values("silence_start: ");
        let tone_start = ends
            .first()
            .copied()
            .expect("the tone should end the opening silence");
        assert!(
            (tone_start - 2.0).abs() <= 0.08,
            "the sped tone starts at {tone_start}, not 2.0: {ends:?}"
        );
        let tone_end = starts
            .iter()
            .copied()
            .find(|start| *start > tone_start + 0.1)
            .expect("the tone should end before the held tail");
        assert!(
            (tone_end - 2.5).abs() <= 0.08,
            "the 2x tone ends at {tone_end}, not 2.5: {starts:?}"
        );
        assert_eq!(
            starts.len(),
            2,
            "the held tail must stay silent, but the sound resumes: {starts:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_export_carries_the_kept_sound_at_the_right_time() {
        let dir =
            std::env::temp_dir().join(format!("apexshot-export-sound-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("tone.mp4");
        let generated = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args(["-y", "-hide_banner", "-loglevel", "error"])
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=30:duration=4",
            ])
            .args([
                "-f",
                "lavfi",
                "-i",
                "aevalsrc=if(gte(t\\,2)\\,0.5*sin(2*PI*440*t)\\,0):s=48000:d=4",
            ])
            .args([
                "-pix_fmt",
                "yuv420p",
                "-c:v",
                "libx264",
                "-c:a",
                "aac",
                "-shortest",
            ])
            .arg(&source)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !generated.is_ok_and(|status| status.success()) {
            eprintln!("skipping the export sound test: ffmpeg is unavailable");
            return;
        }
        let model = ExportModel {
            source: source.clone(),
            source_size: (320, 240),
            trim_start: 1.0,
            trim_end: 3.5,
            cuts: vec![2.5],
            segment_speeds: Vec::new(),
            freeze_tail: 0.0,
            zooms: Vec::new(),
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: crate::recording::editor::model::CursorSettings::default(),
            sidecar: None,
            webcam: None,
            stage: None,
            canvas: None,
            crf: 30,
            audio: Vec::new(),
            effects: Default::default(),
        };
        let output = dir.join("out.mp4");
        let handle = super::super::start_export_with_settings(
            model,
            output.clone(),
            ExportSettings {
                hardware: false,
                ..ExportSettings::default()
            },
        );
        loop {
            match handle.receiver.recv_timeout(Duration::from_secs(120)) {
                Ok(ExportUpdate::Progress { .. }) => {}
                Ok(ExportUpdate::Finished(path, sound)) => {
                    assert_eq!(path, output);
                    assert!(sound.is_none(), "{sound:?}");
                    break;
                }
                Ok(ExportUpdate::Failed(message)) => panic!("export failed: {message}"),
                Ok(ExportUpdate::Cancelled) => panic!("export cancelled itself"),
                Err(error) => panic!("export did not report back: {error}"),
            }
        }
        let codec = crate::utils::hidden_command(ffmpeg::ffprobe_executable())
            .args([
                "-v",
                "error",
                "-select_streams",
                "a",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
            ])
            .arg(&output)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&codec.stdout).trim(), "aac");
        let stream_duration = |stream: &str| -> f64 {
            let probe = crate::utils::hidden_command(ffmpeg::ffprobe_executable())
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    stream,
                    "-show_entries",
                    "stream=duration",
                    "-of",
                    "csv=p=0",
                ])
                .arg(&output)
                .output()
                .unwrap();
            String::from_utf8_lossy(&probe.stdout)
                .trim()
                .parse()
                .unwrap()
        };
        let audio = stream_duration("a:0");
        let video = stream_duration("v:0");
        assert!(
            (audio - video).abs() <= 1.0 / 30.0 + 0.05,
            "the sound ({audio}) does not match the video ({video})"
        );
        assert!((video - 2.5).abs() <= 2.0 / 30.0, "the video is {video} s");
        let detected = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args(["-hide_banner", "-nostdin", "-i"])
            .arg(&output)
            .args(["-af", "silencedetect=n=-30dB:d=0.2", "-f", "null", "-"])
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&detected.stderr);
        let values = |marker: &str| -> Vec<f64> {
            log.lines()
                .filter_map(|line| line.split(marker).nth(1))
                .filter_map(|tail| {
                    tail.trim_start()
                        .split(|c: char| c.is_whitespace() || c == '|')
                        .next()
                        .and_then(|value| value.parse().ok())
                })
                .collect()
        };
        let ends = values("silence_end: ");
        let starts = values("silence_start: ");
        let first_end = ends
            .first()
            .copied()
            .expect("the tone should end the silence");
        assert!(
            (first_end - 1.0).abs() <= 0.06,
            "the sound starts at {first_end}, not 1.0: {ends:?}"
        );
        assert!(
            starts.iter().all(|start| *start <= 1.1),
            "the split left a gap in the sound: {starts:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_silent_recording_exports_without_sound() {
        let dir =
            std::env::temp_dir().join(format!("apexshot-export-silent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("silent.mp4");
        let generated = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args(["-y", "-hide_banner", "-loglevel", "error"])
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=30:duration=2",
            ])
            .args(["-pix_fmt", "yuv420p", "-c:v", "libx264"])
            .arg(&source)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !generated.is_ok_and(|status| status.success()) {
            eprintln!("skipping the silent export test: ffmpeg is unavailable");
            return;
        }
        let model = ExportModel {
            source: source.clone(),
            source_size: (320, 240),
            trim_start: 0.0,
            trim_end: 1.5,
            cuts: Vec::new(),
            segment_speeds: Vec::new(),
            freeze_tail: 0.0,
            zooms: Vec::new(),
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: crate::recording::editor::model::CursorSettings::default(),
            sidecar: None,
            webcam: None,
            stage: None,
            canvas: None,
            crf: 30,
            audio: Vec::new(),
            effects: Default::default(),
        };
        let output = dir.join("out.mp4");
        let handle = super::super::start_export_with_settings(
            model,
            output.clone(),
            ExportSettings {
                hardware: false,
                ..ExportSettings::default()
            },
        );
        loop {
            match handle.receiver.recv_timeout(Duration::from_secs(120)) {
                Ok(ExportUpdate::Progress { .. }) => {}
                Ok(ExportUpdate::Finished(path, sound)) => {
                    assert_eq!(path, output);
                    assert!(sound.is_none(), "{sound:?}");
                    break;
                }
                Ok(ExportUpdate::Failed(message)) => panic!("export failed: {message}"),
                Ok(ExportUpdate::Cancelled) => panic!("export cancelled itself"),
                Err(error) => panic!("export did not report back: {error}"),
            }
        }
        let codec = crate::utils::hidden_command(ffmpeg::ffprobe_executable())
            .args([
                "-v",
                "error",
                "-select_streams",
                "a",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
            ])
            .arg(&output)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&codec.stdout).trim().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_click_graph_delays_and_scales_each_cue_over_the_kept_sound() {
        let ranges = [range(0.0, 2.0, 1.0, 2.0)];
        let cues = [
            Cue {
                delay_ms: 500,
                gain: 0.8,
            },
            Cue {
                delay_ms: 1250,
                gain: 0.6,
            },
        ];
        assert_eq!(
            mixed_graph(Some(&ranges[..]), 2.0, &cues, 2),
            "[1:a:0]atrim=start=0.000000:end=2.000000,asetpts=PTS-STARTPTS,apad,aformat=channel_layouts=stereo[base];[2:a:0]aformat=channel_layouts=stereo,asplit=2[c0][c1];[c0]adelay=500:all=1,volume=0.8000[e0];[c1]adelay=1250:all=1,volume=0.6000[e1];[base][e0][e1]amix=inputs=3:normalize=0:duration=first,apad[a]"
        );
        assert_eq!(
            mixed_graph(None, 1.5, &cues[..1], 1),
            "anullsrc=cl=stereo:r=48000,atrim=duration=1.500000[base];[1:a:0]aformat=channel_layouts=stereo,asplit=1[c0];[c0]adelay=500:all=1,volume=0.8000[e0];[base][e0]amix=inputs=2:normalize=0:duration=first,apad[a]"
        );
    }

    #[test]
    fn click_cues_follow_the_press_kind_and_volume() {
        let mut sidecar = PointerSidecar::new(
            0,
            crate::recording::editor::sidecar::CaptureRegion {
                x: 0,
                y: 0,
                w: 320,
                h: 240,
            },
        );
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
        let spans = [SourceSpan {
            composition_start: 0.0,
            source_start: 0.0,
            source_end: 10.0,
            speed: 1.0,
            frames: 300,
        }];
        assert_eq!(
            click_cues(&sidecar, &spans, 1.0),
            vec![
                Cue {
                    delay_ms: 1000,
                    gain: 1.0
                },
                Cue {
                    delay_ms: 2000,
                    gain: 0.8
                },
                Cue {
                    delay_ms: 2500,
                    gain: 0.6
                },
            ]
        );
    }

    #[test]
    fn clicks_land_at_the_pressed_times_in_a_silent_export() {
        let dir =
            std::env::temp_dir().join(format!("apexshot-export-clicks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("silent.mp4");
        let generated = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args(["-y", "-hide_banner", "-loglevel", "error"])
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=30:duration=4",
            ])
            .args(["-pix_fmt", "yuv420p", "-c:v", "libx264"])
            .arg(&source)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !generated.is_ok_and(|status| status.success()) {
            eprintln!("skipping the click sound test: ffmpeg is unavailable");
            return;
        }
        let mut sidecar = PointerSidecar::new(
            0,
            crate::recording::editor::sidecar::CaptureRegion {
                x: 0,
                y: 0,
                w: 320,
                h: 240,
            },
        );
        for down in [1.0, 2.5] {
            sidecar.presses.push(PressSample {
                down,
                up: down + 0.05,
                button: 1,
                dragged: false,
            });
        }
        let model = ExportModel {
            source: source.clone(),
            source_size: (320, 240),
            trim_start: 0.0,
            trim_end: 4.0,
            cuts: Vec::new(),
            segment_speeds: Vec::new(),
            freeze_tail: 0.0,
            zooms: Vec::new(),
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: crate::recording::editor::model::CursorSettings::default(),
            sidecar: Some(sidecar),
            webcam: None,
            stage: None,
            canvas: None,
            crf: 30,
            audio: Vec::new(),
            effects: EffectsSettings {
                click_sound: Some("soft".to_owned()),
                click_sound_volume: 1.0,
                ..EffectsSettings::default()
            },
        };
        let output = dir.join("out.mp4");
        let handle = super::super::start_export_with_settings(
            model,
            output.clone(),
            ExportSettings {
                hardware: false,
                ..ExportSettings::default()
            },
        );
        loop {
            match handle.receiver.recv_timeout(Duration::from_secs(120)) {
                Ok(ExportUpdate::Progress { .. }) => {}
                Ok(ExportUpdate::Finished(path, sound)) => {
                    assert_eq!(path, output);
                    assert!(sound.is_none(), "{sound:?}");
                    break;
                }
                Ok(ExportUpdate::Failed(message)) => panic!("export failed: {message}"),
                Ok(ExportUpdate::Cancelled) => panic!("export cancelled itself"),
                Err(error) => panic!("export did not report back: {error}"),
            }
        }
        let codec = crate::utils::hidden_command(ffmpeg::ffprobe_executable())
            .args([
                "-v",
                "error",
                "-select_streams",
                "a",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
            ])
            .arg(&output)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&codec.stdout).trim(), "aac");
        let decoded = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args(["-v", "error", "-i"])
            .arg(&output)
            .args([
                "-map", "0:a:0", "-ac", "1", "-ar", "48000", "-f", "f32le", "-",
            ])
            .output()
            .unwrap();
        let samples: Vec<f32> = decoded
            .stdout
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| f32::from_le_bytes(*bytes))
            .collect();
        let mut onsets = Vec::new();
        let mut quiet = 2400usize;
        for (index, sample) in samples.iter().enumerate() {
            if sample.abs() > 0.05 {
                if quiet >= 2400 {
                    onsets.push(index as f64 / 48000.0);
                }
                quiet = 0;
            } else {
                quiet += 1;
            }
        }
        assert_eq!(onsets.len(), 2, "the clicks are at {onsets:?}");
        assert!((onsets[0] - 1.0).abs() <= 0.05, "{onsets:?}");
        assert!((onsets[1] - 2.5).abs() <= 0.05, "{onsets:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
