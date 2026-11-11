//! The recording's sound, cut to the exported spans and muxed into the
//! finished video. The video stream is copied, never re-encoded; the sound is
//! AAC at 192 kbps and is padded with silence to the video's length.

use super::{End, ExportModel, ExportSettings, SourceSpan};
use crate::recording::editor::ffmpeg;
use std::path::{Path, PathBuf};

/// Add the recording's sound to the finished export at `output`. A recording
/// without sound leaves the file as it is.
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
    let Some(source) = candidates.iter().find(|path| has_audio(path)) else {
        return Ok(());
    };
    let plan = super::plan_render(model, settings).map_err(|end| match end {
        End::Failed(message) => message,
        End::Cancelled => "the export was cancelled".to_string(),
    })?;
    let ranges = source_ranges(&plan.spans, plan.fps);
    let duration = plan.total as f64 / plan.fps;
    mux(
        output,
        source,
        &ranges,
        duration,
        settings.codec == super::Codec::Hevc,
    )
}

/// The source-time ranges of the kept footage. Spans that continue where the
/// previous one ended are merged, so a split that removes nothing leaves no
/// seam in the sound.
pub(super) fn source_ranges(spans: &[SourceSpan], fps: f64) -> Vec<(f64, f64)> {
    let mut ranges: Vec<(f64, f64)> = Vec::new();
    for span in spans {
        let start = span.source_start;
        let end = start + span.frames as f64 / fps;
        match ranges.last_mut() {
            Some(last) if (start - last.1).abs() < 0.5 / fps => last.1 = end,
            _ => ranges.push((start, end)),
        }
    }
    ranges
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

/// The filter graph that cuts the sound to `ranges` and pads it with silence.
/// Input 1 is the sound source.
fn filter_graph(ranges: &[(f64, f64)]) -> String {
    let trim =
        |start: f64, end: f64| format!("atrim=start={start:.6}:end={end:.6},asetpts=PTS-STARTPTS");
    match ranges {
        [] => String::new(),
        [(start, end)] => format!("[1:a:0]{},apad[a]", trim(*start, *end)),
        ranges => {
            let count = ranges.len();
            let mut graph = format!("[1:a:0]asplit={count}");
            for index in 0..count {
                graph.push_str(&format!("[s{index}]"));
            }
            graph.push(';');
            for (index, (start, end)) in ranges.iter().enumerate() {
                graph.push_str(&format!("[s{index}]{}[a{index}];", trim(*start, *end)));
            }
            for index in 0..count {
                graph.push_str(&format!("[a{index}]"));
            }
            graph.push_str(&format!("concat=n={count}:v=0:a=1,apad[a]"));
            graph
        }
    }
}

fn mux(
    video: &Path,
    source: &Path,
    ranges: &[(f64, f64)],
    duration: f64,
    hevc: bool,
) -> Result<(), String> {
    let temp = video.with_extension("sound.mp4");
    let _ = std::fs::remove_file(&temp);
    let mut command = crate::utils::hidden_command(ffmpeg::ffmpeg_executable());
    command
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
        .arg("-i")
        .arg(video)
        .arg("-i")
        .arg(source)
        .arg("-filter_complex")
        .arg(filter_graph(ranges))
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
    use std::process::Stdio;
    use std::time::Duration;

    #[test]
    fn kept_spans_join_into_one_range_when_nothing_is_removed() {
        let spans = super::super::frame_plan(1.0, 9.0, &[3.0, 5.0], 30.0);
        let ranges = source_ranges(&spans, 30.0);
        assert_eq!(ranges.len(), 1, "{ranges:?}");
        assert!((ranges[0].0 - 1.0).abs() < 1e-9, "{ranges:?}");
        assert!((ranges[0].1 - 9.0).abs() < 1e-9, "{ranges:?}");
    }

    #[test]
    fn spans_with_a_gap_keep_separate_ranges() {
        let spans = [
            SourceSpan {
                composition_start: 0.0,
                source_start: 0.0,
                frames: 30,
            },
            SourceSpan {
                composition_start: 1.0,
                source_start: 2.0,
                frames: 30,
            },
        ];
        let ranges = source_ranges(&spans, 30.0);
        assert_eq!(ranges.len(), 2, "{ranges:?}");
        assert!((ranges[0].0 - 0.0).abs() < 1e-9, "{ranges:?}");
        assert!((ranges[0].1 - 1.0).abs() < 1e-9, "{ranges:?}");
        assert!((ranges[1].0 - 2.0).abs() < 1e-9, "{ranges:?}");
        assert!((ranges[1].1 - 3.0).abs() < 1e-9, "{ranges:?}");
    }

    #[test]
    fn the_filter_graph_trims_pads_and_joins() {
        assert_eq!(
            filter_graph(&[(1.0, 3.0)]),
            "[1:a:0]atrim=start=1.000000:end=3.000000,asetpts=PTS-STARTPTS,apad[a]"
        );
        assert_eq!(
            filter_graph(&[(0.0, 1.0), (2.0, 3.0)]),
            "[1:a:0]asplit=2[s0][s1];[s0]atrim=start=0.000000:end=1.000000,asetpts=PTS-STARTPTS[a0];[s1]atrim=start=2.000000:end=3.000000,asetpts=PTS-STARTPTS[a1];[a0][a1]concat=n=2:v=0:a=1,apad[a]"
        );
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
            zooms: Vec::new(),
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: crate::recording::editor::model::CursorSettings::default(),
            sidecar: None,
            webcam: None,
            canvas: None,
            crf: 30,
            audio: Vec::new(),
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
            zooms: Vec::new(),
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: crate::recording::editor::model::CursorSettings::default(),
            sidecar: None,
            webcam: None,
            canvas: None,
            crf: 30,
            audio: Vec::new(),
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
}
