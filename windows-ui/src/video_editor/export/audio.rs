//! The recording's sound, muxed into the finished video. The filter graph is
//! built by the sound module from the project's settings; the video stream is
//! copied, never re-encoded, and the sound is AAC at 192 kbps, exactly the
//! composition's length.

use super::{End, ExportModel, ExportSettings};
use crate::recording::editor::ffmpeg;
use crate::video_editor::click_sound;
use crate::video_editor::sound::{
    graph_file_flag_for_this_ffmpeg, SoundGraph, SoundPlan, SoundSources,
};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// Add the recording's sound and its click sounds to the finished export at
/// `output`. A recording with neither leaves the file as it is. The mux polls
/// `cancel` so a Stop still stops it while the card says it is finishing.
pub(super) fn add(
    model: &ExportModel,
    settings: ExportSettings,
    output: &Path,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let videos: Vec<PathBuf> = if model.audio.is_empty() {
        vec![model.source.clone()]
    } else {
        model.audio.clone()
    };
    let sources = SoundSources::find(&videos);
    if sources.is_empty()
        && !model.effects.sound.background_active()
        && model.effects.click_sound.is_none()
    {
        return Ok(());
    }
    let plan = super::plan_render(model, settings).map_err(|end| match end {
        End::Failed(message) => message,
        End::Cancelled => "the export was cancelled".to_string(),
    })?;
    let spans = &plan.spans;
    let duration = plan.total as f64 / plan.fps;
    let sound = SoundPlan::new(
        sources,
        &model.effects,
        model.sidecar.as_ref(),
        spans,
        plan.fps,
        duration,
        model.trim_start,
        model.trim_end,
        &model.cuts,
    );
    let scratch = Scratch::create()?;
    let click = if sound.cues.is_empty() {
        None
    } else {
        let click = scratch.dir.join("click-soft.wav");
        std::fs::write(&click, click_sound::WAV)
            .map_err(|error| format!("could not write the click sound: {error}"))?;
        Some(click)
    };
    let Some(graph) = sound.graph_from(0.0, click.as_deref(), 1) else {
        return Ok(());
    };
    let script = scratch.dir.join("audio.txt");
    std::fs::write(&script, &graph.graph)
        .map_err(|error| format!("could not write the sound graph: {error}"))?;
    mux(
        output,
        &graph,
        &script,
        duration,
        settings.codec == super::Codec::Hevc,
        cancel,
    )
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

fn mux(
    video: &Path,
    graph: &SoundGraph,
    script: &Path,
    duration: f64,
    hevc: bool,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let temp = video.with_extension(format!("sound-{}.mp4", unique()));
    let mut command = crate::utils::hidden_command(ffmpeg::ffmpeg_executable());
    command
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
        .arg("-i")
        .arg(video);
    for input in &graph.inputs {
        if input.looped {
            command.args(["-stream_loop", "-1"]);
        }
        if input.seek > 0.0 {
            command.arg("-ss").arg(format!("{:.6}", input.seek));
        }
        command.arg("-i").arg(&input.path);
    }
    command
        .arg(graph_file_flag_for_this_ffmpeg())
        .arg(script)
        .args(["-map", "0:v:0", "-map", "[a]"])
        .args(["-c:v", "copy"]);
    if hevc {
        command.args(["-tag:v", "hvc1"]);
    }
    let mut child = match command
        .args(["-c:a", "aac", "-b:a", "192k"])
        .arg("-t")
        .arg(format!("{duration:.6}"))
        .args(["-movflags", "+faststart", "-f", "mp4"])
        .arg(&temp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("could not start ffmpeg: {error}"));
        }
    };
    // Drain the error stream on its own thread: ffmpeg blocks once the pipe
    // fills, and the wait below must keep seeing the process run.
    let mut stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(stderr) = stderr.as_mut() {
            let _ = stderr.read_to_end(&mut text);
        }
        text
    });
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = errors.join();
            let _ = std::fs::remove_file(&temp);
            return Err("the export was cancelled".to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = errors.join();
                let _ = std::fs::remove_file(&temp);
                return Err(format!("could not wait for ffmpeg: {error}"));
            }
        }
    };
    let stderr = errors.join().unwrap_or_default();
    if !status.success() {
        let _ = std::fs::remove_file(&temp);
        return Err(format!(
            "ffmpeg could not add the sound: {}",
            String::from_utf8_lossy(&stderr).trim()
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
    use crate::recording::editor::sidecar::{PointerSidecar, PressSample};
    use crate::video_editor::project::EffectsSettings;
    use std::process::Stdio;
    use std::time::Duration;

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
            segment_starts: Vec::new(),
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
            cursor_hide_clips: Vec::new(),
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
            segment_starts: Vec::new(),
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
            cursor_hide_clips: Vec::new(),
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
            segment_starts: Vec::new(),
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
            cursor_hide_clips: Vec::new(),
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
            segment_starts: Vec::new(),
            cursor_hide_clips: Vec::new(),
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
