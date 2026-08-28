use super::model::{
    even_crop_rect, quality_to_crf, AudioMode, VideoBackground, VideoEditState, VideoMetadata,
};
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
    let out = Command::new(name)
        .arg("-version")
        .output()
        .with_context(|| {
            format!(
                "{name} is required for the recording editor. Install ffmpeg to use this feature."
            )
        })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        anyhow::bail!(
            "{name} exited with error (status: {}):\nstdout: {stdout}\nstderr: {stderr}",
            out.status,
        );
    }
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
        let timestamp = thumbnail_timestamp(metadata.duration_seconds, index, count);
        let output_path = dir.join(format!("thumb-{index:02}.png"));
        // Prefer fast input seeking for early tiles. For the final tile, decode
        // accurately: keyframe-only -ss before -i near EOF often overshoots the
        // last frame and writes a blank/white PNG.
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y");
        if index + 1 == count {
            cmd.arg("-i")
                .arg(&metadata.path)
                .arg("-ss")
                .arg(format!("{timestamp:.3}"));
        } else {
            cmd.arg("-ss")
                .arg(format!("{timestamp:.3}"))
                .arg("-i")
                .arg(&metadata.path);
        }
        let output = cmd
            .args(["-an", "-frames:v", "1", "-vf", "scale=160:-1"])
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

pub fn generate_waveform(metadata: &VideoMetadata) -> anyhow::Result<PathBuf> {
    if !metadata.has_audio {
        anyhow::bail!("no audio stream");
    }
    let dir = thumbnail_cache_dir(&metadata.path);
    std::fs::create_dir_all(&dir)?;
    let output_path = dir.join("waveform.png");
    let filter = "showwavespic=s=1200x64:colors=0xb05c38";
    let output = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            &metadata.path.to_string_lossy(),
            "-filter_complex",
            filter,
            "-frames:v",
            "1",
            "-an",
        ])
        .arg(&output_path)
        .output()
        .context("failed to generate waveform")?;
    if !output.status.success() || !output_path.is_file() {
        anyhow::bail!(
            "ffmpeg waveform failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output_path)
}

/// Extract one frame from `input` into `output` as a poster image.
///
/// Same invocation shape as `generate_thumbnails` (fast input seek, single
/// frame, no audio) but writes exactly one frame at full size so the caller can
/// scale it however it likes. Used for the History window's recording cards.
pub fn extract_poster_frame(
    input: &Path,
    output: &Path,
    timestamp_seconds: f64,
) -> anyhow::Result<()> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create poster dir {}", parent.display()))?;
    }

    let args = vec![
        "-y".to_string(),
        "-ss".to_string(),
        format_seconds(timestamp_seconds),
        "-i".to_string(),
        input.to_string_lossy().to_string(),
        "-an".to_string(),
        "-frames:v".to_string(),
        "1".to_string(),
        output.to_string_lossy().to_string(),
    ];
    run_ffmpeg(args, output)?;

    // A seek past the end of a very short clip exits cleanly without writing
    // anything, so treat a missing file as a failure the caller can retry.
    if !output.is_file() {
        return Err(anyhow!(
            "ffmpeg wrote no poster frame for {}",
            input.display()
        ));
    }
    Ok(())
}

fn thumbnail_count(_duration_seconds: f64) -> usize {
    // Fixed for every clip: the timeline strip stretches its tiles to fill the
    // window width, so a variable count would change tile width per video. A
    // sub-1s clip sampled 12 times still yields distinct, valid frames because
    // thumbnail_timestamp spaces them across the (short) duration.
    12
}

/// Sample times for filmstrip frames.
///
/// Never seeks to exact EOF: ffprobe duration is often slightly past the last
/// decodable frame, so `-ss duration` yields a blank/white last tile.
fn thumbnail_timestamp(duration_seconds: f64, index: usize, count: usize) -> f64 {
    if count == 0 || duration_seconds <= 0.0 || !duration_seconds.is_finite() {
        return 0.0;
    }
    if count == 1 {
        return 0.0;
    }
    // Keep the last sample a little before the reported end so ffmpeg still
    // decodes a real frame (important for short clips and VFR webm).
    let epsilon = (duration_seconds * 0.02).clamp(0.04, 0.15);
    let usable_end = (duration_seconds - epsilon).max(0.0);
    usable_end * (index as f64 / (count - 1) as f64)
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

pub fn run_trim_only(state: &VideoEditState, output_path: PathBuf) -> anyhow::Result<PathBuf> {
    let kept = state.ordered_kept_segments();
    if kept.is_empty() {
        anyhow::bail!("no segments selected for export");
    }
    if kept.len() <= 1 {
        let (start, end) = kept.first().copied().unwrap();
        let args = build_single_trim_args(state, start, end, &output_path);
        run_ffmpeg(args, &output_path)?;
    } else {
        run_multi_segment_trim(state, &kept, &output_path, false)?;
    }
    Ok(output_path)
}

pub fn run_convert(state: &VideoEditState, output_path: PathBuf) -> anyhow::Result<PathBuf> {
    let kept = state.ordered_kept_segments();
    if kept.is_empty() {
        anyhow::bail!("no segments selected for export");
    }
    if kept.len() <= 1 {
        let (start, end) = kept.first().copied().unwrap();
        let args = build_single_convert_args(state, start, end, &output_path);
        run_ffmpeg(args, &output_path)?;
    } else {
        run_multi_segment_trim(state, &kept, &output_path, true)?;
    }
    Ok(output_path)
}

/// Export applying the user's editor settings (for Upload and shared export).
/// Uses stream-copy when quality/dimensions are unchanged; otherwise re-encodes.
/// Falls back to convert if trim-only fails (e.g. awkward codecs/containers).
pub fn export_edited(state: &VideoEditState) -> anyhow::Result<PathBuf> {
    export_edited_to(state, state.export_path())
}

pub fn export_edited_to(state: &VideoEditState, output_path: PathBuf) -> anyhow::Result<PathBuf> {
    if state.needs_reencode() {
        return run_convert(state, output_path);
    }
    match run_trim_only(state, output_path.clone()) {
        Ok(path) => Ok(path),
        Err(err) => {
            eprintln!("[video-editor] trim-only export failed ({err}); falling back to convert");
            run_convert(state, output_path)
        }
    }
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
            args.extend([
                "-c:a".into(),
                "aac".into(),
                "-ac".into(),
                "1".into(),
                "-b:a".into(),
                "128k".into(),
            ]);
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
    if state.needs_composite() {
        return build_composite_convert_args(state, start, end, output_path);
    }
    let mut args = vec![
        "-y".into(),
        "-ss".into(),
        format_seconds(start),
        "-to".into(),
        format_seconds(end),
        "-i".into(),
        state.metadata.path.to_string_lossy().into_owned(),
    ];
    let speed = state.speed_for_source(start);
    if let Some(filter) = convert_video_filter(state, speed) {
        args.push("-vf".into());
        args.push(filter);
    }
    args.extend([
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        quality_to_crf(state.quality).to_string(),
    ]);
    args.extend(convert_audio_args(state, speed));
    args.push(output_path.to_string_lossy().into_owned());
    args
}

fn build_composite_convert_args(
    state: &VideoEditState,
    start: f64,
    end: f64,
    output_path: &Path,
) -> Vec<String> {
    let work_dir = std::env::temp_dir().join(format!(
        "apexshot-export-{}-{}",
        std::process::id(),
        (start * 1000.0) as u64
    ));
    let _ = std::fs::create_dir_all(&work_dir);
    let cmd_path = work_dir.join("zoom.cmd");
    let cursor_path = work_dir.join("cursor.png");
    let cursor_hot = super::cursor_sprite::write_png(&cursor_path, state.cursor, "default")
        .unwrap_or((8.0, 6.0));
    let _ = std::fs::write(&cmd_path, build_sendcmd(state, start, end, cursor_hot));

    let (base_w, base_h) = state.canvas_dimensions();
    let (out_w, out_h) = state.padded_output_dimensions();
    let pad_x = ((out_w.saturating_sub(base_w)) / 2) & !1;
    let pad_y = ((out_h.saturating_sub(base_h)) / 2) & !1;
    let bg = match state.background {
        VideoBackground::Plain { r, g, b } => format!("0x{r:02X}{g:02X}{b:02X}"),
        VideoBackground::Gradient(_) => "0x2C2438".to_string(),
        VideoBackground::None => "0x111111".to_string(),
    };

    let (eff_w, eff_h) = state.effective_source_dimensions();
    let mut filter = format!(
        "[0:v]sendcmd=f={},{}crop@z=w={src_w}:h={src_h}:x=0:y=0,scale={base_w}:{base_h}:force_original_aspect_ratio=decrease,pad={base_w}:{base_h}:(ow-iw)/2:(oh-ih)/2:0x000000",
        escape_filter_path(&cmd_path),
        static_crop_prefix(state),
        src_w = eff_w.max(2),
        src_h = eff_h.max(2),
    );
    if out_w != base_w || out_h != base_h {
        filter.push_str(&format!(",pad={out_w}:{out_h}:{pad_x}:{pad_y}:{bg}"));
    }
    if let Some(pad) = lead_in_tpad(state) {
        filter.push(',');
        filter.push_str(&pad);
    }
    let speed = state.speed_for_source(start);
    if (speed - 1.0).abs() > 1e-6 {
        filter.push_str(&format!(",setpts=PTS/{speed}"));
    }
    let draw_cursor = state
        .sidecar
        .as_ref()
        .is_some_and(|sidecar| !sidecar.pointer.is_empty());
    if draw_cursor {
        filter.push_str("[v];[v][1:v]overlay@c=x=0:y=0:eof_action=pass");
    }

    let mut args = vec![
        "-y".into(),
        "-ss".into(),
        format_seconds(start),
        "-to".into(),
        format_seconds(end),
        "-i".into(),
        state.metadata.path.to_string_lossy().into_owned(),
    ];
    if draw_cursor {
        args.extend([
            "-loop".into(),
            "1".into(),
            "-i".into(),
            cursor_path.to_string_lossy().into_owned(),
        ]);
    }
    args.extend(["-filter_complex".into(), filter]);
    args.extend([
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        quality_to_crf(state.quality).to_string(),
    ]);
    args.extend(convert_audio_args(state, speed));
    if draw_cursor {
        args.push("-shortest".into());
    }
    args.push(output_path.to_string_lossy().into_owned());
    args
}

/// `crop=w:h:x:y,` prepended before zoom cropping, or empty when uncropped.
fn static_crop_prefix(state: &VideoEditState) -> String {
    match state.crop {
        Some(c) => format!("crop={}:{}:{}:{},", c.width, c.height, c.x, c.y),
        None => String::new(),
    }
}

fn build_sendcmd(
    state: &VideoEditState,
    start: f64,
    end: f64,
    cursor_hot: (f64, f64),
) -> String {
    let fps = 30.0;
    let duration = (end - start).max(0.0);
    let frames = ((duration * fps).ceil() as usize).max(1);
    let (crop_x, crop_y, eff_w, eff_h) = state.crop_or_full();
    let src_w = eff_w.max(2.0) as u32;
    let src_h = eff_h.max(2.0) as u32;
    let (base_w, base_h) = state.canvas_dimensions();
    let (out_w, out_h) = state.padded_output_dimensions();
    let pad_x = ((out_w.saturating_sub(base_w)) / 2) as f64;
    let pad_y = ((out_h.saturating_sub(base_h)) / 2) as f64;
    let mut lines = String::new();
    for index in 0..frames {
        let local_t = index as f64 / fps;
        let source_t = start + local_t;
        let (scale, center) = state.eval_zoom(source_t);
        let center = (center.0 - crop_x, center.1 - crop_y);
        let (x, y, w, h) = even_crop_rect(scale, center, src_w, src_h);
        lines.push_str(&format!(
            "{local_t:.3} crop@z w {w};\n{local_t:.3} crop@z h {h};\n{local_t:.3} crop@z x {x};\n{local_t:.3} crop@z y {y};\n"
        ));
        if let Some(sidecar) = &state.sidecar {
            if let Some(frame) = sidecar.presented_at(
                source_t,
                state.cursor.smooth,
                state.cursor.hide_idle,
                state.cursor.idle_ms,
            ) {
                if frame.alpha >= 0.12 {
                    let rel_x =
                        ((frame.x - crop_x - x as f64) / w as f64) * base_w as f64 + pad_x;
                    let rel_y =
                        ((frame.y - crop_y - y as f64) / h as f64) * base_h as f64 + pad_y;
                    let (hx, hy) = cursor_hot;
                    lines.push_str(&format!(
                        "{local_t:.3} overlay@c x {:.0};\n{local_t:.3} overlay@c y {:.0};\n",
                        (rel_x - hx).round(),
                        (rel_y - hy).round()
                    ));
                } else {
                    lines.push_str(&format!(
                        "{local_t:.3} overlay@c x -9999;\n{local_t:.3} overlay@c y -9999;\n"
                    ));
                }
            }
        }
    }
    lines
}

fn lead_in_tpad(state: &VideoEditState) -> Option<String> {
    if state.timeline_offset_seconds <= 0.001 {
        return None;
    }
    Some(format!(
        "tpad=start_duration={}:color=black",
        format_seconds(state.timeline_offset_seconds)
    ))
}

fn convert_video_filter(state: &VideoEditState, speed: f64) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(crop) = state.crop {
        parts.push(format!(
            "crop={}:{}:{}:{}",
            crop.width, crop.height, crop.x, crop.y
        ));
    }
    if (speed - 1.0).abs() > 1e-6 {
        parts.push(format!("setpts=PTS/{speed}"));
    }
    if let Some(scale) = convert_scale_filter(state) {
        parts.push(scale);
    }
    if let Some(pad) = lead_in_tpad(state) {
        parts.push(pad);
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(","))
    }
}

fn convert_audio_args(state: &VideoEditState, speed: f64) -> Vec<String> {
    let offset = state.timeline_offset_seconds;
    let tempo = atempo_filter(speed);
    if state.audio_mode == AudioMode::Muted || !state.metadata.has_audio {
        return audio_args(state.audio_mode, state.metadata.has_audio);
    }
    if offset > 0.001 || tempo.is_some() {
        let mut filters = Vec::new();
        if offset > 0.001 {
            let ms = (offset * 1000.0).round().max(1.0) as u64;
            filters.push(format!("adelay={ms}:all=1"));
        }
        if let Some(tempo) = tempo {
            filters.push(tempo);
        }
        let mut args = match state.audio_mode {
            AudioMode::Mono => vec![
                "-ac".into(),
                "1".into(),
                "-c:a".into(),
                "aac".into(),
                "-b:a".into(),
                "128k".into(),
            ],
            _ => vec!["-c:a".into(), "aac".into(), "-b:a".into(), "192k".into()],
        };
        args.extend(["-af".into(), filters.join(",")]);
        args
    } else {
        audio_args(state.audio_mode, state.metadata.has_audio)
    }
}

fn atempo_filter(speed: f64) -> Option<String> {
    if (speed - 1.0).abs() <= 1e-6 || !speed.is_finite() || speed <= 0.0 {
        return None;
    }
    let mut remaining = speed;
    let mut parts = Vec::new();
    while remaining < 0.5 - 1e-9 {
        parts.push("atempo=0.5".into());
        remaining /= 0.5;
    }
    while remaining > 100.0 + 1e-9 {
        parts.push("atempo=100".into());
        remaining /= 100.0;
    }
    parts.push(format!("atempo={remaining}"));
    Some(parts.join(","))
}

/// Scale the source into the output canvas and letterbox leftover space.
fn convert_scale_filter(state: &VideoEditState) -> Option<String> {
    let (width, height) = state.canvas_dimensions();
    let (src_w, src_h) = state.effective_source_dimensions();
    if width == src_w && height == src_h {
        return None;
    }
    Some(format!(
        "scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black"
    ))
}

fn run_multi_segment_trim(
    state: &VideoEditState,
    _segments: &[(f64, f64)],
    output_path: &Path,
    convert: bool,
) -> anyhow::Result<()> {
    let tmp_dir = std::env::temp_dir().join(format!("apexshot-segments-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)?;

    let placed = state.ordered_placed_segments();
    let mut segment_files = Vec::new();
    let mut cursor = 0.0;
    for (i, &(comp, start, end)) in placed.iter().enumerate() {
        let seg_path = tmp_dir.join(format!("seg_{i:04}.mp4"));
        let mut segment_state = state.clone();
        segment_state.timeline_offset_seconds = (comp - cursor).max(0.0);
        if state.muted_for_source(start) {
            segment_state.audio_mode = AudioMode::Muted;
        }
        cursor = comp + (end - start).max(0.0);
        let args = if convert {
            build_single_convert_args(&segment_state, start, end, &seg_path)
        } else {
            build_single_trim_args(&segment_state, start, end, &seg_path)
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

fn escape_filter_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace(':', "\\:")
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
        let args = build_single_trim_args(
            &s,
            s.trim_start_seconds,
            s.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );

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
        state.dimension_preset = crate::recording::editor::model::DimensionPreset::P720;
        let args = build_single_convert_args(
            &state,
            state.trim_start_seconds,
            state.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );

        assert!(args.windows(2).any(|pair| pair == ["-c:v", "libx264"]));
        assert!(args.windows(2).any(|pair| pair == ["-crf", "22"]));
        assert!(args.windows(2).any(|pair| {
            pair[0] == "-vf"
                && pair[1]
                    .starts_with("scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720")
        }));
        assert!(args.iter().any(|arg| arg == "-an"));
        assert_eq!(args.last().map(String::as_str), Some("/tmp/output.mp4"));
    }

    #[test]
    fn convert_with_zoom_uses_sendcmd_crop_graph() {
        let mut state = state();
        state
            .zoom_clips
            .push(crate::recording::editor::model::ZoomClip {
                start: 1.5,
                end: 3.3,
                scale: 1.8,
                center: (960.0, 540.0),
                ease_ms: 200,
                mode: crate::recording::editor::model::ZoomMode::Auto,
            });
        assert!(state.needs_reencode());
        let args = build_single_convert_args(
            &state,
            state.trim_start_seconds,
            state.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );
        assert!(args.iter().any(|arg| arg == "-filter_complex"));
        assert!(args
            .iter()
            .any(|arg| arg.contains("sendcmd") && arg.contains("crop@z")));
        assert!(!args.iter().any(|arg| arg.contains("tmix")));
        assert!(args.windows(2).any(|pair| pair == ["-c:v", "libx264"]));
    }

    #[test]
    fn convert_skips_scale_when_dimensions_match_source() {
        let mut state = state();
        state.dimension_preset = crate::recording::editor::model::DimensionPreset::Original;
        let args = build_single_convert_args(
            &state,
            state.trim_start_seconds,
            state.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );
        assert!(!args.iter().any(|arg| arg == "-vf"));
    }

    #[test]
    fn convert_with_timeline_offset_pads_black_and_delays_audio() {
        let mut state = state();
        state.timeline_offset_seconds = 2.0;
        assert!(state.needs_reencode());
        let args = build_single_convert_args(
            &state,
            state.trim_start_seconds,
            state.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );
        assert!(args
            .iter()
            .any(|arg| arg.contains("tpad=start_duration=2.000")));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "-af" && pair[1] == "adelay=2000:all=1"));
    }

    #[test]
    fn export_edited_uses_trim_when_no_reencode_needed() {
        let s = state();
        assert!(!s.needs_reencode());
        // We only assert the decision helper here; full export needs a real file.
        let mut reencode = s.clone();
        reencode.quality = 30;
        assert!(reencode.needs_reencode());
    }

    #[test]
    fn thumbnail_timestamps_start_at_zero_and_stay_before_eof() {
        let duration = 10.0;
        let count = 12;
        assert!((thumbnail_timestamp(duration, 0, count) - 0.0).abs() < 1e-9);

        let last = thumbnail_timestamp(duration, count - 1, count);
        assert!(last < duration);
        assert!(last >= duration - 0.15);
        assert!(last <= duration - 0.04);

        let mid = thumbnail_timestamp(duration, 6, count);
        assert!(mid > 0.0 && mid < last);
    }

    #[test]
    fn thumbnail_timestamp_handles_single_and_short_clips() {
        assert_eq!(thumbnail_timestamp(0.5, 0, 1), 0.0);
        let last = thumbnail_timestamp(1.0, 11, 12);
        assert!(last < 1.0);
        assert!(last >= 0.0);
    }

    #[test]
    fn thumbnail_count_is_fixed_so_tile_width_never_depends_on_duration() {
        // The timeline strip stretches its tiles to fill the window width, so a
        // duration-dependent count would make tiles visibly wider for short
        // clips. Every clip gets the same 12, including sub-1s ones.
        assert_eq!(thumbnail_count(0.4), 12);
        assert_eq!(thumbnail_count(0.99), 12);
        assert_eq!(thumbnail_count(1.0), 12);
        assert_eq!(thumbnail_count(60.0), 12);
        assert_eq!(thumbnail_count(3600.0), 12);
    }
}
