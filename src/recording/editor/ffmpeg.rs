use super::model::{
    even_crop_rect, AudioMode, VideoBackground, VideoEditState, VideoMetadata, DEFAULT_FRAME_RATE,
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
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
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
            "stream=width,height,avg_frame_rate,r_frame_rate",
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
    let frame_rate = stream
        .avg_frame_rate
        .as_deref()
        .and_then(parse_frame_rate)
        .or_else(|| stream.r_frame_rate.as_deref().and_then(parse_frame_rate))
        .unwrap_or(DEFAULT_FRAME_RATE);
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
        frame_rate,
    })
}

/// Parse ffprobe's `avg_frame_rate` / `r_frame_rate` value, usually a
/// fraction like "30000/1001" but sometimes a bare number. Unknowns such
/// as "0/0" yield `None` so callers can fall back.
fn parse_frame_rate(value: &str) -> Option<f64> {
    let value = value.trim();
    let frame_rate = match value.split_once('/') {
        Some((numerator, denominator)) => {
            let numerator: f64 = numerator.trim().parse().ok()?;
            let denominator: f64 = denominator.trim().parse().ok()?;
            numerator / denominator
        }
        None => value.parse().ok()?,
    };
    (frame_rate.is_finite() && frame_rate > 0.0).then_some(frame_rate)
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
/// frame, no audio), but scales the result to the History cache width before
/// writing it. The History thumbnail pipeline performs the final crop.
pub fn extract_poster_frame(
    input: &Path,
    output: &Path,
    timestamp_seconds: f64,
) -> anyhow::Result<()> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create poster dir {}", parent.display()))?;
    }

    let args = poster_frame_args(input, output, timestamp_seconds);
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

fn poster_frame_args(input: &Path, output: &Path, timestamp_seconds: f64) -> Vec<String> {
    vec![
        "-y".to_string(),
        "-ss".to_string(),
        format_seconds(timestamp_seconds),
        "-i".to_string(),
        input.to_string_lossy().to_string(),
        "-an".to_string(),
        "-frames:v".to_string(),
        "1".to_string(),
        "-vf".to_string(),
        "scale=260:-2".to_string(),
        output.to_string_lossy().to_string(),
    ]
}

pub(crate) fn thumbnail_count(_duration_seconds: f64) -> usize {
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
pub(crate) fn thumbnail_timestamp(duration_seconds: f64, index: usize, count: usize) -> f64 {
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
    match if state.muted_for_source(start) {
        AudioMode::Muted
    } else {
        state.audio_mode
    } {
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
        state.quality.crf().to_string(),
    ]);
    args.extend(convert_audio_args(state, speed, start));
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
    let cursor_path = work_dir.join("cursor.rgba");
    let _ = std::fs::write(&cmd_path, build_sendcmd(state, start, end));

    // The video is the fitted rect inside the output canvas; a fixed Frame
    // insets it with the background padding, and the fill (or the black
    // unset-fill scene) covers everything around it.
    let (video_w, video_h) = state.video_rect_dimensions();
    let (out_w, out_h) = state.output_dimensions();
    let pad_x = ((out_w.saturating_sub(video_w)) / 2) & !1;
    let pad_y = ((out_h.saturating_sub(video_h)) / 2) & !1;
    let wallpaper_path = match &state.background {
        VideoBackground::Wallpaper(path) if path.is_file() => Some(path.clone()),
        _ => None,
    };
    let bg = match &state.background {
        VideoBackground::Plain { r, g, b } => format!("0x{r:02X}{g:02X}{b:02X}"),
        // Gradient presets are not offered in the video Background panel;
        // legacy projects get the flat stand-in color.
        VideoBackground::Gradient(_) => "0x2C2438".to_string(),
        VideoBackground::Wallpaper(_) => "0x111111".to_string(),
        VideoBackground::None => "0x000000".to_string(),
    };

    let (eff_w, eff_h) = state.effective_source_dimensions();
    let draw_cursor = state
        .sidecar
        .as_ref()
        .is_some_and(|sidecar| sidecar.can_render_cursor_overlay())
        && super::cursor_export::write_rgba_track(
            state,
            start,
            end,
            video_w,
            video_h,
            &cursor_path,
        )
        .is_ok();
    let use_wallpaper = wallpaper_path.is_some() && (out_w != video_w || out_h != video_h);
    let wallpaper_index = if draw_cursor { 2 } else { 1 };
    let mut filter = format!(
        "[0:v]sendcmd=f={},{}crop@z=w={src_w}:h={src_h}:x=0:y=0,scale={video_w}:{video_h},setsar=1",
        escape_filter_path(&cmd_path),
        static_crop_prefix(state),
        src_w = eff_w.max(2),
        src_h = eff_h.max(2),
    );
    if use_wallpaper {
        // Label the prepared video frame so it can be overlaid onto the
        // wallpaper canvas. Cursor stays on the video, not the background.
        if draw_cursor {
            // Blend the RGBA cursor track in the video's own 4:2:0 space. An RGB
            // working format (what `format=auto` resolves to for an RGBA overlay)
            // round-trips the frame through RGB, which shifts chroma on every
            // cropped frame — the purple cast through zooms — and makes the
            // encoder write 4:4:4 output.
            filter.push_str(
                "[video];[video][1:v]overlay=0:0:eof_action=pass:shortest=0:format=yuv420[vbase];",
            );
        } else {
            filter.push_str("[video];");
        }
        let video_label = if draw_cursor { "vbase" } else { "video" };
        filter.push_str(&format!(
            "[{wallpaper_index}:v]scale={out_w}:{out_h}:force_original_aspect_ratio=increase,crop={out_w}:{out_h},setsar=1[bg];[bg][{video_label}]overlay=(W-w)/2:(H-h)/2:format=yuv420"
        ));
    } else {
        if draw_cursor {
            // Blend the RGBA cursor track in the video's own 4:2:0 space. An RGB
            // working format (what `format=auto` resolves to for an RGBA overlay)
            // round-trips the frame through RGB, which shifts chroma on every
            // cropped frame — the purple cast through zooms — and makes the
            // encoder write 4:4:4 output.
            filter.push_str(
                "[video];[video][1:v]overlay=0:0:eof_action=pass:shortest=0:format=yuv420",
            );
        }
        if out_w != video_w || out_h != video_h {
            filter.push_str(&format!(",pad={out_w}:{out_h}:{pad_x}:{pad_y}:{bg}"));
        }
    }
    let speed = state.speed_for_source(start);
    if (speed - 1.0).abs() > 1e-6 {
        filter.push_str(&format!(",setpts=PTS/{speed}"));
    }
    if let Some(pad) = lead_in_tpad(state) {
        filter.push(',');
        filter.push_str(&pad);
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
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            "rgba".into(),
            "-video_size".into(),
            format!("{video_w}x{video_h}"),
            "-framerate".into(),
            format!("{:.6}", state.metadata.export_frame_rate()),
            "-i".into(),
            cursor_path.to_string_lossy().into_owned(),
        ]);
    }
    if let Some(wallpaper) = wallpaper_path.as_ref().filter(|_| use_wallpaper) {
        args.extend([
            "-loop".into(),
            "1".into(),
            "-i".into(),
            wallpaper.to_string_lossy().into_owned(),
        ]);
    }
    args.extend(["-filter_complex".into(), filter]);
    args.extend([
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        state.quality.crf().to_string(),
    ]);
    args.extend(convert_audio_args(state, speed, start));
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

fn build_sendcmd(state: &VideoEditState, start: f64, end: f64) -> String {
    let fps = state.metadata.export_frame_rate();
    let duration = (end - start).max(0.0);
    let frames = ((duration * fps).ceil() as usize).max(1);
    let (crop_x, crop_y, eff_w, eff_h) = state.crop_or_full();
    let src_w = eff_w.max(2.0) as u32;
    let src_h = eff_h.max(2.0) as u32;
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

fn convert_audio_args(state: &VideoEditState, speed: f64, source_start: f64) -> Vec<String> {
    let offset = state.timeline_offset_seconds;
    let tempo = atempo_filter(speed);
    let mode = if state.muted_for_source(source_start) {
        AudioMode::Muted
    } else {
        state.audio_mode
    };
    if mode == AudioMode::Muted || !state.metadata.has_audio {
        return audio_args(mode, state.metadata.has_audio);
    }
    if offset > 0.001 || tempo.is_some() {
        let mut filters = Vec::new();
        if let Some(tempo) = tempo {
            filters.push(tempo);
        }
        if offset > 0.001 {
            let ms = (offset * 1000.0).round().max(1.0) as u64;
            filters.push(format!("adelay={ms}:all=1"));
        }
        let mut args = match mode {
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
        audio_args(mode, state.metadata.has_audio)
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
        cursor = comp + (end - start).max(0.0) / state.speed_for_source(start);
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
            frame_rate: 30.0,
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
    fn poster_frame_command_scales_before_writing() {
        let args = poster_frame_args(
            Path::new("/tmp/input.mp4"),
            Path::new("/tmp/poster.png"),
            1.0,
        );

        assert!(args.windows(2).any(|pair| pair == ["-vf", "scale=260:-2"]));
        assert_eq!(args.last().map(String::as_str), Some("/tmp/poster.png"));
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
    fn clip_mute_removes_audio_from_single_segment_commands() {
        let mut s = state();
        s.set_selected_clip_muted(true);
        let trim = build_single_trim_args(
            &s,
            s.trim_start_seconds,
            s.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );
        let convert = build_single_convert_args(
            &s,
            s.trim_start_seconds,
            s.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );

        assert!(trim.iter().any(|arg| arg == "-an"));
        assert!(convert.iter().any(|arg| arg == "-an"));
    }

    #[test]
    fn convert_command_uses_h264_crf_and_audio_args() {
        let mut state = state();
        state.audio_mode = AudioMode::Muted;
        state.dimension_preset = crate::recording::editor::model::DimensionPreset::P720;
        let args = build_single_convert_args(
            &state,
            state.trim_start_seconds,
            state.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );

        assert!(args.windows(2).any(|pair| pair == ["-c:v", "libx264"]));
        assert!(args.windows(2).any(|pair| pair == ["-crf", "20"]));
        assert!(args.windows(2).any(|pair| {
            pair[0] == "-vf"
                && pair[1]
                    .starts_with("scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720")
        }));
        assert!(args.iter().any(|arg| arg == "-an"));
        assert_eq!(args.last().map(String::as_str), Some("/tmp/output.mp4"));
    }

    #[test]
    fn convert_command_uses_ultra_crf_when_quality_is_ultra() {
        let mut state = state();
        state.quality = crate::recording::editor::model::ExportQuality::Ultra;
        // Ultra must force the re-encode path, otherwise the picked CRF would
        // never reach ffmpeg on an otherwise untouched export.
        assert!(state.needs_reencode());
        let args = build_single_convert_args(
            &state,
            state.trim_start_seconds,
            state.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );

        assert!(args.windows(2).any(|pair| pair == ["-crf", "16"]));
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
                easing: crate::recording::editor::model::ZoomEasing::Glide,
                mode: crate::recording::editor::model::ZoomMode::Auto,
                ..Default::default()
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
        assert!(!args.iter().any(|arg| arg.contains("overlay@c")));
        assert!(!args.iter().any(|arg| arg.contains("tmix")));
        assert!(
            !args.iter().any(|arg| arg == "-shortest"),
            "cursor/zoom export must not cut the video to a shorter overlay track"
        );
        assert!(args.windows(2).any(|pair| pair == ["-c:v", "libx264"]));
    }

    #[test]
    fn parse_frame_rate_understands_ffprobe_values() {
        assert!((parse_frame_rate("30000/1001").unwrap() - 30_000.0 / 1001.0).abs() < 1e-9);
        assert_eq!(parse_frame_rate("60/1"), Some(60.0));
        assert_eq!(parse_frame_rate("25"), Some(25.0));
        assert_eq!(parse_frame_rate("0/0"), None);
        assert_eq!(parse_frame_rate("0/1"), None);
        assert_eq!(parse_frame_rate("n/a"), None);
    }

    #[test]
    fn zoom_command_grid_follows_the_source_frame_rate() {
        let mut state = state();
        state.metadata.frame_rate = 60.0;
        let cmd = build_sendcmd(&state, 1.25, 2.25);
        let timestamps: Vec<&str> = cmd.lines().step_by(4).collect();
        assert_eq!(timestamps.len(), 60);
        assert!(timestamps[0].starts_with("0.000 "));
        assert!(
            timestamps[1].starts_with("0.017 "),
            "second command must land one frame in: {:?}",
            timestamps[1]
        );
    }

    #[test]
    fn zoom_command_grid_falls_back_and_clamps_the_frame_rate() {
        let grid_lines = |frame_rate: f64| {
            let mut state = state();
            state.metadata.frame_rate = frame_rate;
            build_sendcmd(&state, 1.25, 2.25).lines().count()
        };
        // Unknown rates fall back to the 30 fps default.
        assert_eq!(grid_lines(0.0), 30 * 4);
        assert_eq!(grid_lines(f64::NAN), 30 * 4);
        // Absurd rates are clamped so the command file stays small.
        assert_eq!(grid_lines(10_000.0), 240 * 4);
    }

    #[test]
    fn cursor_overlay_blends_in_yuv420_never_through_rgb() {
        use crate::recording::editor::sidecar::{
            CaptureRegion, CursorKind, PointerSample, PointerSidecar,
        };

        // Small source and window so the RGBA cursor track stays tiny.
        let mut state = VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/input.mp4"),
            duration_seconds: 0.4,
            width: 64,
            height: 48,
            file_size_bytes: 100,
            has_audio: false,
            frame_rate: 30.0,
        });
        state.trim_start_seconds = 0.0;
        state.trim_end_seconds = 0.2;
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 10.0,
            kind: CursorKind::Default,
        });
        sidecar.pointer.push(PointerSample {
            t: 0.2,
            x: 40.0,
            y: 20.0,
            kind: CursorKind::Default,
        });
        state.sidecar = Some(sidecar);

        let args = build_single_convert_args(
            &state,
            state.trim_start_seconds,
            state.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );
        let graph = args
            .windows(2)
            .find(|pair| pair[0] == "-filter_complex")
            .map(|pair| pair[1].as_str())
            .expect("a recorded pointer track exports through the composite graph");

        assert!(graph.contains("overlay=0:0:eof_action=pass:shortest=0:format=yuv420"));
        // RGB working formats round-trip the frame through RGB, which shifts
        // chroma on cropped frames (a purple cast through zooms) and makes the
        // encoder write 4:4:4 output.
        for rgb in ["format=auto", "format=rgb", "format=gbrp"] {
            assert!(
                !graph.contains(rgb),
                "cursor overlay must not blend in {rgb}"
            );
        }
    }

    #[test]
    fn cursor_overlay_input_follows_the_source_frame_rate() {
        use crate::recording::editor::sidecar::{
            CaptureRegion, CursorKind, PointerSample, PointerSidecar,
        };

        let mut state = VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/input.mp4"),
            duration_seconds: 0.4,
            width: 64,
            height: 48,
            file_size_bytes: 100,
            has_audio: false,
            frame_rate: 60.0,
        });
        state.trim_start_seconds = 0.0;
        state.trim_end_seconds = 0.2;
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 10.0,
            kind: CursorKind::Default,
        });
        state.sidecar = Some(sidecar);

        let args = build_single_convert_args(
            &state,
            state.trim_start_seconds,
            state.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );
        // The raw track is declared at the rate it was generated with, or
        // its frames drift against the video.
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-framerate", "60.000000"]),
            "cursor track must enter at its generation rate: {args:?}"
        );
    }

    #[test]
    fn composite_speed_is_applied_before_timeline_padding() {
        let mut state = state();
        state.timeline_offset_seconds = 2.0;
        state.segment_starts[0] = 2.0;
        state.set_selected_clip_speed(2.0);
        state
            .zoom_clips
            .push(crate::recording::editor::model::ZoomClip {
                start: 2.0,
                end: 3.8,
                scale: 1.8,
                center: (960.0, 540.0),
                ease_ms: 200,
                easing: crate::recording::editor::model::ZoomEasing::Glide,
                mode: crate::recording::editor::model::ZoomMode::Manual,
                ..Default::default()
            });
        let args = build_single_convert_args(
            &state,
            state.trim_start_seconds,
            state.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );
        let graph = args
            .windows(2)
            .find(|pair| pair[0] == "-filter_complex")
            .map(|pair| pair[1].as_str())
            .unwrap();

        assert!(graph.contains("setpts=PTS/2,tpad=start_duration=2.000"));
    }

    #[test]
    fn audio_speed_is_applied_before_timeline_delay() {
        let mut state = state();
        state.timeline_offset_seconds = 2.0;
        let args = convert_audio_args(&state, 2.0, state.trim_start_seconds);
        let filter = args
            .windows(2)
            .find(|pair| pair[0] == "-af")
            .map(|pair| pair[1].as_str())
            .unwrap();

        assert_eq!(filter, "atempo=2,adelay=2000:all=1");
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
        reencode.quality = crate::recording::editor::model::ExportQuality::Ultra;
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

    #[test]
    fn wallpaper_background_overlays_video_onto_scaled_wallpaper() {
        let mut s = state();
        let dir =
            std::env::temp_dir().join(format!("apexshot-wallpaper-export-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let wallpaper = dir.join("wallpaper-001.jpg");
        std::fs::write(&wallpaper, b"fake-jpg").unwrap();
        s.background = VideoBackground::Wallpaper(wallpaper.clone());
        s.background_padding = 40.0;
        assert!(s.needs_composite());
        let (video_w, video_h) = s.video_rect_dimensions();
        let (out_w, out_h) = s.output_dimensions();
        assert!(out_w > video_w || out_h > video_h);
        let args = build_single_convert_args(
            &s,
            s.trim_start_seconds,
            s.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );
        let graph = args
            .windows(2)
            .find(|pair| pair[0] == "-filter_complex")
            .map(|pair| pair[1].as_str())
            .expect("wallpaper background exports through the composite graph");
        // Wallpaper is cover-scaled to the padded canvas, then the video is
        // centered on top of it instead of a solid-color pad.
        assert!(
            graph.contains("force_original_aspect_ratio=increase"),
            "wallpaper must cover-scale to the canvas: {graph}"
        );
        assert!(
            graph.contains("overlay=(W-w)/2:(H-h)/2"),
            "video must center onto the wallpaper: {graph}"
        );
        assert!(
            args.windows(2).any(|pair| pair == ["-loop", "1"]),
            "wallpaper image must loop as an ffmpeg input"
        );
        assert!(
            args.iter()
                .any(|arg| arg == &wallpaper.to_string_lossy().into_owned()),
            "wallpaper path must be passed to ffmpeg"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn frame_pick_pads_the_letterbox_with_the_fill_not_black() {
        // 4:3 recording, 16:9 frame, black-ish fill: the exported canvas stays
        // the frame size and the letterbox + padding take the fill color.
        let mut s = VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/input.mp4"),
            duration_seconds: 10.0,
            width: 1280,
            height: 960,
            file_size_bytes: 100,
            has_audio: true,
            frame_rate: 30.0,
        });
        s.trim_start_seconds = 1.25;
        s.trim_end_seconds = 8.5;
        s.apply_aspect_ratio(1920, 1080);
        s.background = VideoBackground::Plain {
            r: 44,
            g: 36,
            b: 56,
        };
        s.background_padding = 40.0;
        assert!(s.needs_composite());

        let (video_w, video_h) = s.video_rect_dimensions();
        assert_eq!(s.output_dimensions(), (1920, 1080));
        let args = build_single_convert_args(
            &s,
            s.trim_start_seconds,
            s.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );
        let graph = args
            .windows(2)
            .find(|pair| pair[0] == "-filter_complex")
            .map(|pair| pair[1].as_str())
            .expect("a fill exports through the composite graph");

        assert!(
            graph.contains(&format!("scale={video_w}:{video_h}")),
            "video layer must be the fitted rect, not the whole canvas: {graph}"
        );
        assert!(
            graph.contains("pad=1920:1080:") && graph.contains(":0x2C2438"),
            "letterbox must take the fill color instead of black: {graph}"
        );
        assert!(
            !graph.contains("0x000000"),
            "a chosen fill must never export black bars: {graph}"
        );
    }

    #[test]
    fn frame_pick_with_wallpaper_needs_no_padding_ring() {
        // The wallpaper is the canvas, so it must be used even with padding 0:
        // the video no longer covers the frame on its own.
        let mut s = state();
        let dir =
            std::env::temp_dir().join(format!("apexshot-wallpaper-frame-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let wallpaper = dir.join("wallpaper-002.jpg");
        std::fs::write(&wallpaper, b"fake-jpg").unwrap();
        s.background = VideoBackground::Wallpaper(wallpaper.clone());
        s.background_padding = 0.0;
        s.apply_aspect_ratio(1080, 1080);
        assert!(s.needs_composite());
        assert_eq!(s.output_dimensions(), (1080, 1080));

        let args = build_single_convert_args(
            &s,
            s.trim_start_seconds,
            s.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );
        let graph = args
            .windows(2)
            .find(|pair| pair[0] == "-filter_complex")
            .map(|pair| pair[1].as_str())
            .expect("wallpaper background exports through the composite graph");
        assert!(
            graph.contains("force_original_aspect_ratio=increase"),
            "wallpaper must cover-scale to the frame: {graph}"
        );
        assert!(
            graph.contains("overlay=(W-w)/2:(H-h)/2"),
            "the fitted video must center on the wallpaper: {graph}"
        );
        assert!(
            args.windows(2).any(|pair| pair == ["-loop", "1"]),
            "wallpaper image must loop as an ffmpeg input"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cursor_track_matches_the_video_rect_not_the_canvas() {
        // With a Frame picked, cursor pixels must map to the fitted video rect;
        // the RGBA track used to be canvas-sized, which misplaced the cursor
        // whenever the recording aspect did not match the frame.
        use crate::recording::editor::sidecar::{
            CaptureRegion, CursorKind, PointerSample, PointerSidecar,
        };

        let mut s = VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/input.mp4"),
            duration_seconds: 0.4,
            width: 64,
            height: 48,
            file_size_bytes: 100,
            has_audio: false,
            frame_rate: 30.0,
        });
        s.trim_start_seconds = 0.0;
        s.trim_end_seconds = 0.2;
        s.apply_aspect_ratio(64, 96);
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 10.0,
            kind: CursorKind::Default,
        });
        sidecar.pointer.push(PointerSample {
            t: 0.2,
            x: 40.0,
            y: 20.0,
            kind: CursorKind::Default,
        });
        s.sidecar = Some(sidecar);

        let (video_w, video_h) = s.video_rect_dimensions();
        assert_eq!((video_w, video_h), (64, 48));
        assert_ne!(s.output_dimensions(), (video_w, video_h));

        let args = build_single_convert_args(
            &s,
            s.trim_start_seconds,
            s.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-video_size", &format!("{video_w}x{video_h}")]),
            "cursor track must be rendered at the video rect size: {args:?}"
        );
        let graph = args
            .windows(2)
            .find(|pair| pair[0] == "-filter_complex")
            .map(|pair| pair[1].as_str())
            .expect("a pointer track exports through the composite graph");
        assert!(
            graph.contains("overlay=0:0:eof_action=pass:shortest=0:format=yuv420"),
            "cursor must blend onto the fitted video layer: {graph}"
        );
    }

    #[test]
    fn missing_wallpaper_file_falls_back_to_solid_pad() {
        let mut s = state();
        s.background = VideoBackground::Wallpaper(PathBuf::from(
            "/tmp/apexshot-definitely-missing-wallpaper.jpg",
        ));
        s.background_padding = 40.0;
        let args = build_single_convert_args(
            &s,
            s.trim_start_seconds,
            s.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );
        let graph = args
            .windows(2)
            .find(|pair| pair[0] == "-filter_complex")
            .map(|pair| pair[1].as_str())
            .expect("missing wallpaper still exports");
        assert!(
            !graph.contains("force_original_aspect_ratio=increase"),
            "missing wallpaper must not build a wallpaper overlay: {graph}"
        );
        assert!(
            !args.windows(2).any(|pair| pair == ["-loop", "1"]),
            "missing wallpaper must not add a looped input"
        );
    }

    /// First pixel of the exported frame, for letterbox color checks.
    fn corner_pixel(path: &Path) -> (u8, u8, u8) {
        let output = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                path.to_str().unwrap(),
                "-vf",
                // yuv420p needs even crop sizes, so take a 2x2 block and use
                // its first pixel.
                "crop=2:2:0:0",
                "-frames:v",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-",
            ])
            .output()
            .expect("ffmpeg pixel dump");
        assert!(output.status.success(), "pixel dump failed");
        assert!(output.stdout.len() >= 3, "pixel dump returned no bytes");
        (output.stdout[0], output.stdout[1], output.stdout[2])
    }

    #[test]
    fn framed_export_fills_the_letterbox_end_to_end() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        // Cargo's sandbox can isolate /tmp, so keep fixtures in target/.
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test-fixtures");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join(format!("apexshot-frame-source-{}.mp4", std::process::id()));
        let framed = dir.join(format!("apexshot-frame-filled-{}.mp4", std::process::id()));
        let plain = dir.join(format!("apexshot-frame-black-{}.mp4", std::process::id()));

        // 4:3 source so a 16:9 frame has to letterbox.
        let created = Command::new("ffmpeg")
            .args([
                "-y",
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=30:duration=1",
                "-pix_fmt",
                "yuv420p",
                source.to_str().unwrap(),
            ])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !created {
            return;
        }

        let metadata = probe_metadata(&source).expect("probe the fixture");
        let mut state = VideoEditState::new(metadata);
        state.apply_aspect_ratio(640, 360);
        state.background = VideoBackground::Plain {
            r: 220,
            g: 30,
            b: 40,
        };
        export_edited_to(&state, framed.clone()).expect("export with a fill");
        let filled = probe_metadata(&framed).expect("probe the framed export");
        assert_eq!((filled.width, filled.height), (640, 360));

        state.background = VideoBackground::None;
        export_edited_to(&state, plain.clone()).expect("export without a fill");
        let unfilled = probe_metadata(&plain).expect("probe the unfilled export");
        assert_eq!((unfilled.width, unfilled.height), (640, 360));

        let (fr, fg, fb) = corner_pixel(&framed);
        assert!(
            fr > 180 && fg < 80 && fb < 90,
            "letterbox must take the fill color, got rgb({fr},{fg},{fb})"
        );
        let (br, bg, bb) = corner_pixel(&plain);
        assert!(
            br < 40 && bg < 40 && bb < 40,
            "an unset fill must keep the black scene, got rgb({br},{bg},{bb})"
        );

        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_file(&framed);
        let _ = std::fs::remove_file(&plain);
    }
}
