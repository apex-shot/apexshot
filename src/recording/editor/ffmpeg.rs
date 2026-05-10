use super::model::{edited_output_path, quality_to_crf, AudioMode, VideoEditState, VideoMetadata};
use anyhow::{anyhow, Context};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn ensure_tools_available() -> anyhow::Result<()> {
    ensure_tool("ffmpeg")?;
    ensure_tool("ffprobe")?;
    Ok(())
}

fn ensure_tool(name: &str) -> anyhow::Result<()> {
    Command::new(name)
        .arg("-version")
        .output()
        .with_context(|| {
            format!(
                "{name} is required for the recording editor. Install ffmpeg to use this feature."
            )
        })?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ProbeRoot {
    streams: Option<Vec<ProbeStream>>,
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

pub fn probe_metadata(path: &Path) -> anyhow::Result<VideoMetadata> {
    ensure_tools_available()?;

    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .with_context(|| format!("failed to run ffprobe for {}", path.display()))?;

    if !output.status.success() {
        return Err(anyhow!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let root: ProbeRoot =
        serde_json::from_slice(&output.stdout).context("failed to parse ffprobe metadata")?;
    let stream = root
        .streams
        .as_ref()
        .and_then(|streams| streams.first())
        .ok_or_else(|| anyhow!("unsupported video: no video stream found"))?;
    let width = stream
        .width
        .ok_or_else(|| anyhow!("unsupported video: missing width"))?;
    let height = stream
        .height
        .ok_or_else(|| anyhow!("unsupported video: missing height"))?;
    let duration_seconds = root
        .format
        .and_then(|format| format.duration)
        .and_then(|duration| duration.parse::<f64>().ok())
        .ok_or_else(|| anyhow!("unsupported video: missing duration"))?;

    if duration_seconds <= 0.0 || !duration_seconds.is_finite() {
        return Err(anyhow!("unsupported video: invalid duration"));
    }

    let file_size_bytes = std::fs::metadata(path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?
        .len();
    let has_audio = probe_has_audio(path)?;

    Ok(VideoMetadata {
        path: path.to_path_buf(),
        duration_seconds,
        width,
        height,
        file_size_bytes,
        has_audio,
    })
}

fn probe_has_audio(path: &Path) -> anyhow::Result<bool> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=index",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .with_context(|| format!("failed to run ffprobe audio scan for {}", path.display()))?;

    if !output.status.success() {
        return Ok(false);
    }

    let root: ProbeRoot =
        serde_json::from_slice(&output.stdout).context("failed to parse ffprobe audio metadata")?;
    Ok(root.streams.is_some_and(|streams| !streams.is_empty()))
}

pub fn thumbnail_cache_dir(input: &Path) -> PathBuf {
    let mut dir = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
    dir.push("apexshot");
    dir.push("video-editor");
    let mut hash = 1469598103934665603_u64;
    for byte in input.to_string_lossy().as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    dir.push(format!("{}-{hash:x}", std::process::id()));
    dir
}

pub fn generate_thumbnails(metadata: &VideoMetadata) -> anyhow::Result<Vec<PathBuf>> {
    let dir = thumbnail_cache_dir(&metadata.path);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create thumbnail dir {}", dir.display()))?;

    let count = thumbnail_count(metadata.duration_seconds);
    let mut paths = Vec::with_capacity(count);
    for index in 0..count {
        let timestamp = if count <= 1 {
            0.0
        } else {
            metadata.duration_seconds * (index as f64 / (count - 1) as f64)
        };
        let output_path = dir.join(format!("thumb-{index:02}.png"));
        let output = Command::new("ffmpeg")
            .arg("-y")
            .arg("-ss")
            .arg(format!("{timestamp:.3}"))
            .arg("-i")
            .arg(&metadata.path)
            .args(["-frames:v", "1", "-vf", "scale=160:-1"])
            .arg(&output_path)
            .output()
            .with_context(|| format!("failed to generate thumbnail {}", output_path.display()))?;

        if !output.status.success() {
            return Err(anyhow!(
                "ffmpeg thumbnail generation failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        paths.push(output_path);
    }

    Ok(paths)
}

fn thumbnail_count(duration_seconds: f64) -> usize {
    if duration_seconds < 1.0 {
        1
    } else {
        12
    }
}

pub fn audio_args(mode: AudioMode, has_audio: bool) -> Vec<String> {
    match mode {
        AudioMode::Unchanged if has_audio => vec!["-c:a".into(), "copy".into()],
        AudioMode::Unchanged => Vec::new(),
        AudioMode::Mono => vec![
            "-ac".into(),
            "1".into(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "128k".into(),
        ],
        AudioMode::Muted => vec!["-an".into()],
    }
}

pub fn run_trim_only(state: &VideoEditState) -> anyhow::Result<PathBuf> {
    let output_path = edited_output_path(&state.metadata.path);
    let kept = state.ordered_kept_segments();
    if kept.len() <= 1 {
        let (start, end) = kept.first().copied().unwrap_or((
            state.trim_start_seconds,
            state.trim_end_seconds,
        ));
        let args = build_single_trim_args(state, start, end, &output_path);
        run_ffmpeg(args, &output_path)?;
    } else {
        run_multi_segment_trim(state, &kept, &output_path, false)?;
    }
    Ok(output_path)
}

pub fn run_convert(state: &VideoEditState) -> anyhow::Result<PathBuf> {
    let output_path = edited_output_path(&state.metadata.path);
    let kept = state.ordered_kept_segments();
    if kept.len() <= 1 {
        let (start, end) = kept.first().copied().unwrap_or((
            state.trim_start_seconds,
            state.trim_end_seconds,
        ));
        let args = build_single_convert_args(state, start, end, &output_path);
        run_ffmpeg(args, &output_path)?;
    } else {
        run_multi_segment_trim(state, &kept, &output_path, true)?;
    }
    Ok(output_path)
}

fn build_single_trim_args(
    state: &VideoEditState,
    start: f64,
    end: f64,
    output_path: &Path,
) -> Vec<String> {
    let mut args = vec![
        "-y".into(),
        "-ss".into(),
        format_seconds(start),
        "-to".into(),
        format_seconds(end),
        "-i".into(),
        state.metadata.path.to_string_lossy().into_owned(),
        "-c:v".into(),
        "copy".into(),
    ];
    // Apply audio mode (mute/mono work even with video stream copy)
    match state.audio_mode {
        AudioMode::Muted => args.push("-an".into()),
        AudioMode::Mono => {
            args.extend(["-c:a".into(), "aac".into(), "-ac".into(), "1".into(), "-b:a".into(), "128k".into()]);
        }
        AudioMode::Unchanged => {
            if state.metadata.has_audio {
                args.extend(["-c:a".into(), "copy".into()]);
            }
        }
    }
    args.push(output_path.to_string_lossy().into_owned());
    args
}

fn build_single_convert_args(
    state: &VideoEditState,
    start: f64,
    end: f64,
    output_path: &Path,
) -> Vec<String> {
    let (width, height) = state.target_dimensions();
    let mut args = vec![
        "-y".into(),
        "-ss".into(),
        format_seconds(start),
        "-to".into(),
        format_seconds(end),
        "-i".into(),
        state.metadata.path.to_string_lossy().into_owned(),
        "-vf".into(),
        format!("scale={width}:{height}"),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        quality_to_crf(state.quality).to_string(),
    ];
    args.extend(audio_args(state.audio_mode, state.metadata.has_audio));
    args.push(output_path.to_string_lossy().into_owned());
    args
}

fn run_multi_segment_trim(
    state: &VideoEditState,
    segments: &[(f64, f64)],
    output_path: &Path,
    convert: bool,
) -> anyhow::Result<()> {
    let tmp_dir = std::env::temp_dir().join(format!("apexshot-segments-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)?;

    let mut segment_files = Vec::new();
    for (i, &(start, end)) in segments.iter().enumerate() {
        let seg_path = tmp_dir.join(format!("seg_{i:04}.mp4"));
        let args = if convert {
            build_single_convert_args(state, start, end, &seg_path)
        } else {
            build_single_trim_args(state, start, end, &seg_path)
        };
        run_ffmpeg(args, &seg_path).with_context(|| format!("failed to export segment {i}"))?;
        segment_files.push(seg_path);
    }

    // Build concat list
    let list_path = tmp_dir.join("concat.txt");
    let list_content = segment_files
        .iter()
        .map(|p| format!("file '{}'", p.display()))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&list_path, &list_content)?;

    // Concat
    let concat_args = vec![
        "-y".into(),
        "-f".into(),
        "concat".into(),
        "-safe".into(),
        "0".into(),
        "-i".into(),
        list_path.to_string_lossy().into_owned(),
        "-c".into(),
        "copy".into(),
        output_path.to_string_lossy().into_owned(),
    ];
    run_ffmpeg(concat_args, output_path)?;

    // Cleanup
    let _ = std::fs::remove_dir_all(&tmp_dir);
    Ok(())
}

fn run_ffmpeg(args: Vec<String>, output_path: &Path) -> anyhow::Result<()> {
    let output = Command::new("ffmpeg")
        .args(&args)
        .output()
        .context("failed to run ffmpeg")?;

    if output.status.success() {
        return Ok(());
    }

    let _ = std::fs::remove_file(output_path);
    Err(anyhow!(
        "ffmpeg failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn format_seconds(value: f64) -> String {
    format!("{:.3}", value.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> VideoEditState {
        let metadata = VideoMetadata {
            path: PathBuf::from("/tmp/input.mp4"),
            duration_seconds: 10.0,
            width: 1920,
            height: 1080,
            file_size_bytes: 100,
            has_audio: true,
        };
        let mut state = VideoEditState::new(metadata);
        state.trim_start_seconds = 1.25;
        state.trim_end_seconds = 8.5;
        state
    }

    #[test]
    fn audio_mode_builds_expected_ffmpeg_args() {
        assert_eq!(audio_args(AudioMode::Unchanged, true), ["-c:a", "copy"]);
        assert!(audio_args(AudioMode::Unchanged, false).is_empty());
        assert_eq!(
            audio_args(AudioMode::Mono, true),
            ["-ac", "1", "-c:a", "aac", "-b:a", "128k"]
        );
        assert_eq!(audio_args(AudioMode::Muted, true), ["-an"]);
    }

    #[test]
    fn trim_only_command_uses_stream_copy() {
        let s = state();
        let args = build_single_trim_args(&s, s.trim_start_seconds, s.trim_end_seconds, Path::new("/tmp/output.mp4"));

        assert!(args.windows(2).any(|pair| pair == ["-c:v", "copy"]));
        assert!(args.windows(2).any(|pair| pair == ["-ss", "1.250"]));
        assert!(args.windows(2).any(|pair| pair == ["-to", "8.500"]));
        assert_eq!(args.last().map(String::as_str), Some("/tmp/output.mp4"));
    }

    #[test]
    fn convert_command_uses_h264_crf_and_audio_args() {
        let mut state = state();
        state.quality = 70;
        state.audio_mode = AudioMode::Muted;
        let args = build_single_convert_args(&state, state.trim_start_seconds, state.trim_end_seconds, Path::new("/tmp/output.mp4"));

        assert!(args.windows(2).any(|pair| pair == ["-c:v", "libx264"]));
        assert!(args.windows(2).any(|pair| pair == ["-crf", "22"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-vf", "scale=1920:1080"]));
        assert!(args.iter().any(|arg| arg == "-an"));
        assert_eq!(args.last().map(String::as_str), Some("/tmp/output.mp4"));
    }
}
