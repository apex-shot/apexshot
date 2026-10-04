use super::hardware_encode::{self, HardwareEncoder};
use super::model::background_render::render_rounded_mask;
use super::model::{
    even_crop_rect, AudioMode, ExportQuality, VideoBackground, VideoEditState, VideoMetadata,
    DEFAULT_FRAME_RATE,
};
use anyhow::{anyhow, Context};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// Distinguishes concurrent exports' scratch directories. The process id and
/// start time are not enough: two exports that start at the same time (the
/// test suite, or two editor windows) would otherwise share one `zoom.cmd`.
static EXPORT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Whether `path` sits on a RAM-backed filesystem.
///
/// `std::env::temp_dir()` is `/tmp`, and on most Linux desktops `/tmp` is a
/// tmpfs — so scratch written there spends RAM, not disk. That matters because
/// a composite export writes `cursor.rgba`, a full-canvas RGBA frame for every
/// output frame: about 2.5 GB for 10 s of 1080p at 30 fps. On a small machine
/// that is the difference between a slow export and an exhausted system.
///
/// Only Linux can be asked; elsewhere the scratch root is assumed to be a real
/// filesystem.
fn is_ram_backed(path: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        const TMPFS_MAGIC: i64 = 0x0102_1994;
        let Ok(raw) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        let mut buf: libc::statfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statfs(raw.as_ptr(), &mut buf) } != 0 {
            return false;
        }
        buf.f_type as i64 == TMPFS_MAGIC
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        false
    }
}

/// Every directory that may hold an export's scratch tree, most preferred
/// first.
///
/// The cache directory is where new trees go. The temp directory stays in the
/// list so trees written by an older build — which always used it — are still
/// swept.
fn scratch_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(cache) = dirs::cache_dir() {
        let dir = cache.join("apexshot").join("export-scratch");
        if std::fs::create_dir_all(&dir).is_ok() {
            roots.push(dir);
        }
    }
    let temp = std::env::temp_dir();
    if !roots.contains(&temp) {
        roots.push(temp);
    }
    roots
}

/// The directory a new export's scratch tree is written to.
///
/// Prefers the cache directory, but never returns a RAM-backed one: an export
/// big enough to matter has to spend disk, not memory. If every candidate is
/// RAM-backed the temp directory is still used, because a working export beats
/// a refused one — that case is the reason the cursor track is streamed rather
/// than written at all in a later change.
fn scratch_root() -> PathBuf {
    scratch_roots()
        .into_iter()
        .find(|root| !is_ram_backed(root))
        .unwrap_or_else(std::env::temp_dir)
}

fn unique_export_dir(kind: &str, start: f64) -> PathBuf {
    scratch_root().join(format!(
        "apexshot-{kind}-{}-{}-{}",
        std::process::id(),
        (start * 1000.0) as u64,
        EXPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

/// A scratch directory that deletes itself when dropped, so an export's
/// intermediate files (`zoom.cmd`, the rounded mask) live exactly as long as
/// the ffmpeg command that reads them — on success, on error, and on an early
/// `?`. The cursor track no longer needs one: it is rendered straight into a
/// pipe, so a multi-gigabyte `cursor.rgba` is never created.
#[derive(Debug)]
struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn new(kind: &str, start: f64) -> Self {
        let path = unique_export_dir(kind, start);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// An ffmpeg argument list together with the scratch directory those arguments
/// read from, kept alive until the command has run. Derefs to the argument
/// vector so callers can inspect it directly.
#[derive(Debug)]
struct ConvertCommand {
    args: Vec<String>,
    /// Held only so its `Drop` deletes the scratch tree once the command has
    /// run; not read directly in production builds.
    #[allow(dead_code)]
    scratch: Option<ScratchDir>,
    /// The GPU warp feeding `-i pipe:3`, when the ripple is visible.
    warp: Option<super::gst_warp::WarpSetup>,
    /// The cursor overlay track feeding `-i pipe:4`, when one is drawn.
    cursor: Option<super::cursor_track::ActiveCursorTrack>,
}

impl std::ops::Deref for ConvertCommand {
    type Target = Vec<String>;

    fn deref(&self) -> &Self::Target {
        &self.args
    }
}

/// Whether a process is still running. Configured conservatively off Linux:
/// an unknown pid is treated as alive, so nothing is ever removed without a
/// `/proc` entry to check.
fn process_is_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        Path::new("/proc").join(pid.to_string()).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        true
    }
}

/// Remove scratch directories left by exports whose process is gone.
///
/// Each name carries the creating pid (`apexshot-export-<pid>-...`), so a
/// directory whose process no longer exists is stale — from a crash, a kill,
/// or a power loss that skipped [`ScratchDir`]'s drop. The sweep runs before a
/// new export so a leaked tree cannot accumulate, and never touches a live pid
/// (this process or a concurrent export).
pub(crate) fn sweep_stale_scratch_dirs() {
    for root in scratch_roots() {
        sweep_scratch_root(&root);
    }
}

/// Sweep one scratch root. Split out so the caller can cover every root a
/// previous build may have written to, not just the one this build prefers.
fn sweep_scratch_root(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(rest) = name.strip_prefix("apexshot-") else {
            continue;
        };
        // The first all-digit token after the prefix is the creating pid; the
        // kind words that precede it never contain digits.
        let Some(pid) = rest.split('-').find_map(|part| part.parse::<u32>().ok()) else {
            continue;
        };
        if pid == std::process::id() || process_is_alive(pid) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

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
    run_ffmpeg(&args, output)?;

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
        run_ffmpeg(&args, &output_path)?;
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
        let command = build_single_convert_args(state, start, end, &output_path);
        run_command(command, &output_path)?;
    } else {
        run_multi_segment_trim(state, &kept, &output_path, true)?;
    }
    Ok(output_path)
}

/// Extra headroom on top of the estimated output size.
///
/// The estimate is deliberately generous. Refusing an export that would have
/// fit costs the user one confusing error; starting one that cannot fit writes
/// a truncated file and leaves the disk full. A stream copy writes roughly the
/// source's bytes again, but a re-encode's CRF target is a quality setting, not
/// a byte budget — busy footage can come out larger than the recording it came
/// from — so the margin absorbs that instead of pretending to predict it.
const EXPORT_SIZE_MARGIN: f64 = 1.5;

/// The least free space an export is ever held to. Without it a very short
/// clip could pass a check against a disk with a handful of megabytes left,
/// which is not enough for the container, the audio, or the encoder's tail.
const MIN_EXPORT_RESERVE_BYTES: u64 = 256 * 1024 * 1024;

/// Free memory below which an encode is worth warning about. This is not a
/// gate: ffmpeg streams its inputs, so memory stays flat no matter how long
/// the clip is, and a machine with this much free memory will usually finish.
/// It is a heads-up for the small-RAM case, not a refusal.
const TIGHT_MEMORY_BYTES: u64 = 512 * 1024 * 1024;

const BYTES_PER_GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// Whether `available` bytes of free space can hold a `needed`-byte export.
///
/// Pure so the decision can be tested without touching a filesystem: the
/// interesting cases are all near the boundary, where a real disk is awkward
/// to arrange.
fn export_fits(available: u64, needed: u64) -> bool {
    available >= needed
}

/// Estimate an export's size in bytes from the source's own bitrate, scaled to
/// the length the export will actually have.
///
/// `output_seconds` is the composition length, so a trim, a cut, or a speed
/// change shrinks the estimate the way it shrinks the file. The result never
/// drops below [`MIN_EXPORT_RESERVE_BYTES`], and the margin on top is what
/// makes the answer safe rather than exact.
fn estimate_export_bytes(source_bytes: u64, source_seconds: f64, output_seconds: f64) -> u64 {
    let source_seconds = source_seconds.max(0.001);
    let output_seconds = output_seconds.max(0.0);
    let bytes_per_second = source_bytes as f64 / source_seconds;
    let scaled = bytes_per_second * output_seconds * EXPORT_SIZE_MARGIN;
    // A float-to-int cast saturates, so an absurd source can never wrap to a
    // tiny requirement and let the export through.
    scaled.max(MIN_EXPORT_RESERVE_BYTES as f64) as u64
}

/// The deepest ancestor of `path` that exists.
///
/// The output file is normally created by ffmpeg, so it does not exist yet;
/// its parent directory is what names the filesystem it will land on.
fn existing_ancestor(path: &Path) -> Option<&Path> {
    path.ancestors().find(|candidate| candidate.exists())
}

/// Free bytes on the filesystem holding `path`, or `None` when it cannot be
/// interrogated.
///
/// A `None` must never block an export: a filesystem that will not answer is
/// no evidence that the export does not fit, and refusing work the machine can
/// actually do is worse than skipping the check.
///
/// The casts widen the counter fields to `u64`. On a 64-bit target they are
/// already `u64`, so the cast is redundant there and clippy says so; keeping
/// it makes the small-target build compile as well.
#[allow(clippy::unnecessary_cast)]
fn free_disk_bytes(path: &Path) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let probe = existing_ancestor(path)?;
        let raw = std::ffi::CString::new(probe.as_os_str().as_bytes()).ok()?;
        let mut buf: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(raw.as_ptr(), &mut buf) } != 0 {
            return None;
        }
        // `f_frsize` is the fragment size the counters are expressed in, but
        // some filesystems leave it zero; `f_bsize` is the sane fallback.
        let block = if buf.f_frsize != 0 {
            buf.f_frsize as u64
        } else {
            buf.f_bsize as u64
        };
        Some(buf.f_bavail as u64 * block)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        None
    }
}

/// `MemAvailable` from `/proc/meminfo` in bytes, or `None` where it is not
/// available (non-Linux, or a kernel without the field).
fn available_memory_bytes() -> Option<u64> {
    let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
    parse_mem_available(&contents)
}

/// Parse `MemAvailable` out of `/proc/meminfo` contents. Pure so the parsing
/// and the missing-field case can be tested directly.
fn parse_mem_available(contents: &str) -> Option<u64> {
    let line = contents
        .lines()
        .find(|line| line.starts_with("MemAvailable:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// Whether `available` bytes of free memory is low enough to warn about.
fn memory_is_tight(available: u64) -> bool {
    available < TIGHT_MEMORY_BYTES
}

/// Refuse an export that cannot fit on the output's filesystem.
///
/// This runs before ffmpeg starts, so the user gets one clear sentence naming
/// the required and available space instead of a cryptic encoder error and a
/// half-written file. A filesystem that cannot be interrogated lets the export
/// through.
fn ensure_export_fits(state: &VideoEditState, output_path: &Path) -> anyhow::Result<()> {
    let needed = estimate_export_bytes(
        state.metadata.file_size_bytes,
        state.metadata.duration_seconds,
        state.composition_duration(),
    );
    let Some(available) = free_disk_bytes(output_path) else {
        return Ok(());
    };
    if export_fits(available, needed) {
        return Ok(());
    }
    let where_to = existing_ancestor(output_path).unwrap_or(output_path);
    anyhow::bail!(
        "not enough free disk space for this export: about {:.1} GB is needed but only {:.1} GB is free in {}",
        needed as f64 / BYTES_PER_GIB,
        available as f64 / BYTES_PER_GIB,
        where_to.display(),
    );
}

/// Surface a clear note when memory is short, without refusing the export.
///
/// The export streams, so it does not grow with the clip's length and a small
/// machine can still finish. Warning rather than gating keeps a low-memory box
/// usable while explaining why the encode might be slow or swap.
fn warn_if_memory_is_tight() {
    let Some(available) = available_memory_bytes() else {
        return;
    };
    if !memory_is_tight(available) {
        return;
    }
    eprintln!(
        "[export] only {:.2} GB of memory is available; this export may be slow and can push the machine into swap",
        available as f64 / BYTES_PER_GIB,
    );
}

/// Export applying the user's editor settings (for Upload and shared export).
/// Uses stream-copy when quality/dimensions are unchanged; otherwise re-encodes.
/// Falls back to convert if trim-only fails (e.g. awkward codecs/containers).
pub fn export_edited(state: &VideoEditState) -> anyhow::Result<PathBuf> {
    export_edited_to(state, state.export_path())
}

pub fn export_edited_to(state: &VideoEditState, output_path: PathBuf) -> anyhow::Result<PathBuf> {
    sweep_stale_scratch_dirs();
    ensure_export_fits(state, &output_path)?;
    warn_if_memory_is_tight();
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

/// The video output arguments for `encoder`, or libx264 when it is `None`.
///
/// Hardware is only ever `Some` after an explicit opt-in and a passing runtime
/// probe (see `super::hardware_encode`), so the software branch is the default
/// and keeps the previous export byte-for-byte.
fn export_video_args(encoder: Option<HardwareEncoder>, quality: ExportQuality) -> Vec<String> {
    match encoder {
        Some(encoder) => hardware_encode::hardware_video_args(
            encoder,
            hardware_encode::hardware_qp(quality.crf()),
        ),
        None => vec![
            "-c:v".into(),
            "libx264".into(),
            "-preset".into(),
            "veryfast".into(),
            "-crf".into(),
            quality.crf().to_string(),
        ],
    }
}

/// Append the VA-API upload to a filter graph when the chosen encoder needs it.
///
/// VA-API reads GPU frames, so the graph has to end with `hwupload`; NVENC and
/// libx264 take system-memory frames and leave the filter untouched.
fn with_hardware_upload(
    filter: Option<String>,
    encoder: Option<HardwareEncoder>,
) -> Option<String> {
    match hardware_encode::hardware_upload_suffix(encoder) {
        Some(upload) => Some(match filter {
            Some(filter) => format!("{filter},{upload}"),
            None => upload.to_string(),
        }),
        None => filter,
    }
}

fn build_single_convert_args(
    state: &VideoEditState,
    start: f64,
    end: f64,
    output_path: &Path,
) -> ConvertCommand {
    let warp = super::gst_warp::WarpSetup::from_state(state, start, end)
        .filter(|_| super::gst_warp::gl_warp_available());
    build_single_convert_args_with(state, start, end, output_path, warp)
}

fn build_single_convert_args_with(
    state: &VideoEditState,
    start: f64,
    end: f64,
    output_path: &Path,
    warp: Option<super::gst_warp::WarpSetup>,
) -> ConvertCommand {
    if state.needs_composite() {
        return build_composite_convert_args(state, start, end, output_path, warp);
    }
    let encoder = hardware_encode::selected_export_encoder();
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
    // `-vaapi_device` is a global option and the `hwupload` in the filter graph
    // resolves its device when the graph is parsed, so the device has to come
    // first.
    if let Some(encoder) = encoder {
        args.extend(hardware_encode::hardware_input_args(encoder));
    }
    if let Some(filter) = with_hardware_upload(convert_video_filter(state, speed), encoder) {
        args.push("-vf".into());
        args.push(filter);
    }
    args.extend(export_video_args(encoder, state.quality));
    args.extend(convert_audio_args(state, speed, start));
    args.push(output_path.to_string_lossy().into_owned());
    ConvertCommand {
        args,
        scratch: None,
        warp: None,
        cursor: None,
    }
}

fn build_composite_convert_args(
    state: &VideoEditState,
    start: f64,
    end: f64,
    output_path: &Path,
    warp: Option<super::gst_warp::WarpSetup>,
) -> ConvertCommand {
    let scratch = ScratchDir::new("export", start);
    let cmd_path = scratch.path.join("zoom.cmd");
    // An opt-in hardware encoder applies to the composite graph too. It is
    // `None` (libx264) unless the user asked for hardware and the runtime
    // probe passed, so the default composite export is unchanged.
    let encoder = hardware_encode::selected_export_encoder();
    // The held tail is part of the composition, so the camera command file has
    // to cover it too: preview keeps moving the zoom through a freeze, and the
    // export must match.
    let freeze = state.freeze_tail_seconds();
    let (commands, blur_samples) = build_zoom_commands(state, start, end + freeze);
    let _ = std::fs::write(&cmd_path, commands);

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

    let (cursor_w, cursor_h) = if blur_samples > 1 {
        (state.metadata.width.max(2), state.metadata.height.max(2))
    } else {
        (video_w, video_h)
    };
    let can_draw_cursor = state
        .sidecar
        .as_ref()
        .is_some_and(|sidecar| sidecar.can_render_cursor_overlay());
    // The studied ripple's visible band is a footage displacement, not a drawn
    // ring. When the GPU warp runs the overlay skips the ring; when it does
    // not, the ring stays as the fallback.
    let warp_requested = warp.is_some();
    // The track is streamed, so nothing is rendered until ffmpeg asks for it:
    // starting the writer is what decides whether a cursor can be drawn at all.
    let mut cursor_track = if can_draw_cursor {
        let track = if blur_samples > 1 {
            super::cursor_track::ActiveCursorTrack::start_with_view(
                state,
                start,
                end + freeze,
                cursor_w,
                cursor_h,
                warp_requested,
                true,
            )
        } else {
            super::cursor_track::ActiveCursorTrack::start(
                state,
                start,
                end + freeze,
                cursor_w,
                cursor_h,
                warp_requested,
            )
        };
        match track {
            Ok(track) => Some(track),
            Err(err) => {
                eprintln!("[export] cursor track unavailable, exporting without it: {err}");
                None
            }
        }
    } else {
        None
    };
    let draw_cursor = cursor_track.is_some();
    // A cursor track that could not start leaves the warp with nothing to
    // click; drop the warp so the export never silently displaces the footage.
    let warp = if draw_cursor { warp } else { None };
    let warp_ripple = warp.is_some();
    // The warp pipe adds an input ahead of the source, so every later input
    // index shifts by one.
    let input_offset = usize::from(warp_ripple);
    let cursor_index = 1 + input_offset;
    let use_wallpaper = wallpaper_path.is_some() && (out_w != video_w || out_h != video_h);
    let wallpaper_index = (if draw_cursor { 2 } else { 1 }) + input_offset;

    // A radius masks the card so the fill shows through the corners. The mask
    // is rasterized at the video rect because `alphamerge` copies its luma
    // into the frame's alpha and needs the two to be the same size. A failed
    // write just leaves the corners square rather than failing the export.
    let rounded_mask = state
        .has_corner_radius()
        .then(|| {
            let path = scratch.path.join("radius.png");
            write_rounded_mask(&path, video_w, video_h, state.background_corner_radius_px())
                .ok()
                .map(|_| path)
        })
        .flatten();
    let mask_index = 1 + input_offset + usize::from(draw_cursor) + usize::from(use_wallpaper);

    // The freeze hold pads the source *before* the zoom crop. Cloning after
    // the crop would replay one already-composited frame for the whole tail
    // and the camera would sit still while the preview keeps moving.
    //
    // When the warp runs, `[0:v]` is the raw warped frames from the GStreamer
    // pipe; otherwise it is the source. Either way the crop/zoom/background
    // path below is unchanged.
    let mut filter = if blur_samples > 1 {
        camera_blur_filter(
            state,
            &cmd_path,
            blur_samples,
            draw_cursor.then_some(cursor_index),
        )
    } else {
        format!(
            "[0:v]{}sendcmd=f={},{}scale@zs={video_w}:{video_h},crop@z=w={video_w}:h={video_h}:x=0:y=0,setsar=1",
            freeze_tail_tpad(state)
                .map(|pad| format!("{pad},"))
                .unwrap_or_default(),
            escape_filter_path(&cmd_path),
            static_crop_prefix(state),
        )
    };
    // The cursor stays on the video, not the background. It is blended in the
    // video's own 4:2:0 space: an RGB working format (what `format=auto`
    // resolves to for an RGBA overlay) round-trips the frame through RGB,
    // which shifts chroma on every cropped frame — the purple cast through
    // zooms — and makes the encoder write 4:4:4 output.
    if draw_cursor && blur_samples == 1 {
        filter.push_str(&format!(
            "[vc0];[vc0][{cursor_index}:v]overlay=0:0:eof_action=pass:shortest=0:format=yuv420"
        ));
    }
    // Turn the prepared video layer into a rounded card. The mask's luma
    // becomes the frame's alpha, so the fill behind it shows at the corners.
    if rounded_mask.is_some() {
        filter.push_str(&format!(
            "[vcard0];[vcard0][{mask_index}:v]alphamerge,format=yuva420p"
        ));
    }
    if use_wallpaper {
        if rounded_mask.is_some() {
            // `format=auto` keeps the alpha so the rounded card blends into
            // the wallpaper; the trailing `yuv420p` flattens it back for the
            // encoder, whose profile has no alpha.
            filter.push_str(&format!(
                "[vcard];[{wallpaper_index}:v]scale={out_w}:{out_h}:force_original_aspect_ratio=increase,crop={out_w}:{out_h},setsar=1[bg];[bg][vcard]overlay=(W-w)/2:(H-h)/2:format=auto,format=yuv420p"
            ));
        } else {
            // Label the prepared video frame so it can be overlaid onto the
            // wallpaper canvas.
            filter.push_str("[video];");
            filter.push_str(&format!(
                "[{wallpaper_index}:v]scale={out_w}:{out_h}:force_original_aspect_ratio=increase,crop={out_w}:{out_h},setsar=1[bg];[bg][video]overlay=(W-w)/2:(H-h)/2:format=yuv420"
            ));
        }
    } else if rounded_mask.is_some() {
        // Compositing onto a solid canvas instead of `pad` keeps the card's
        // alpha meaningful: `pad` fills the surrounding ring but leaves the
        // rounded corners transparent, and the encoder drops that alpha.
        filter.push_str(&format!(
            "[vcard];color=c={bg}:s={out_w}x{out_h}[bgc];[bgc][vcard]overlay=(W-w)/2:(H-h)/2:format=auto,format=yuv420p"
        ));
    } else if out_w != video_w || out_h != video_h {
        filter.push_str(&format!(",pad={out_w}:{out_h}:{pad_x}:{pad_y}:{bg}"));
    }
    let speed = state.speed_for_source(start);
    if (speed - 1.0).abs() > 1e-6 {
        filter.push_str(&format!(",setpts=PTS/{speed}"));
    }
    if let Some(pad) = lead_in_tpad(state) {
        filter.push(',');
        filter.push_str(&pad);
    }
    // VA-API reads GPU frames, so its upload has to be the graph's last step,
    // after every overlay, mask, and pad. NVENC and libx264 need nothing.
    if let Some(upload) = hardware_encode::hardware_upload_suffix(encoder) {
        filter.push(',');
        filter.push_str(upload);
    }

    let mut args = vec!["-y".into()];
    if warp_ripple {
        // The GPU warp feeds raw yuv420p frames over the inherited fd as
        // input 0; the source follows as input 1 for its audio.
        args.extend([
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-video_size".into(),
            format!("{}x{}", state.metadata.width, state.metadata.height),
            "-framerate".into(),
            format!("{:.6}", state.metadata.export_frame_rate()),
            "-i".into(),
            "pipe:3".into(),
        ]);
    }
    args.extend([
        "-ss".into(),
        format_seconds(start),
        "-to".into(),
        format_seconds(end),
        "-i".into(),
        state.metadata.path.to_string_lossy().into_owned(),
    ]);
    if draw_cursor {
        args.extend([
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            "rgba".into(),
            "-video_size".into(),
            format!("{cursor_w}x{cursor_h}"),
            "-framerate".into(),
            format!("{:.6}", state.metadata.export_frame_rate()),
            "-i".into(),
            "pipe:4".into(),
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
    // The mask loops like the wallpaper still so it is available for every
    // frame `alphamerge` touches; `-loop 1` keeps feeding it to the graph.
    if let Some(mask) = rounded_mask.as_ref() {
        args.extend([
            "-loop".into(),
            "1".into(),
            "-i".into(),
            mask.to_string_lossy().into_owned(),
        ]);
    }
    // `-vaapi_device` has to be registered before the complex graph that uses
    // `hwupload` is parsed.
    if let Some(encoder) = encoder {
        args.extend(hardware_encode::hardware_input_args(encoder));
    }
    if blur_samples > 1 {
        args.extend(["-filter_complex_threads".into(), "1".into()]);
    }
    args.extend(["-filter_complex".into(), filter]);
    args.extend(export_video_args(encoder, state.quality));
    args.extend(convert_audio_args(state, speed, start));
    args.push(output_path.to_string_lossy().into_owned());
    ConvertCommand {
        args,
        scratch: Some(scratch),
        warp,
        cursor: cursor_track.take(),
    }
}

/// Write the rounded-corner alpha mask the composite graph blends the card
/// through. Grayscale, because `alphamerge` reads the mask's luma as alpha.
fn write_rounded_mask(path: &Path, width: u32, height: u32, radius: f64) -> anyhow::Result<()> {
    let mask = render_rounded_mask(width, height, radius);
    let luma: Vec<u8> = mask.pixels.chunks_exact(3).map(|pixel| pixel[0]).collect();
    let gray = image::GrayImage::from_raw(mask.width, mask.height, luma)
        .ok_or_else(|| anyhow!("rounded mask buffer size mismatch"))?;
    gray.save_with_format(path, image::ImageFormat::Png)
        .context("failed to write the rounded-corner mask")?;
    Ok(())
}

/// `crop=w:h:x:y,` prepended before zoom cropping, or empty when uncropped.
fn static_crop_prefix(state: &VideoEditState) -> String {
    match state.crop {
        Some(c) => format!("crop={}:{}:{}:{},", c.width, c.height, c.x, c.y),
        None => String::new(),
    }
}

fn build_sendcmd(state: &VideoEditState, start: f64, end: f64) -> String {
    build_camera_commands(state, start, end, 1).0
}

pub(super) fn zoom_blur_view(
    state: &VideoEditState,
    timeline_t: f64,
    source_t: f64,
    sample_index: usize,
) -> (f64, f64, f64, f64) {
    let (cx, cy, cw, ch) = state.crop_or_full();
    let (scale, center) = state.eval_zoom_blur_sample_at(timeline_t, source_t, sample_index);
    let (x, y, w, h) = even_crop_rect(
        scale,
        (center.0 - cx, center.1 - cy),
        cw.max(2.0) as u32,
        ch.max(2.0) as u32,
    );
    (cx + x as f64, cy + y as f64, w as f64, h as f64)
}

pub(super) fn zoom_export_times(state: &VideoEditState, start: f64, local_t: f64) -> (f64, f64) {
    (
        state.source_to_timeline(start + local_t),
        (start + local_t).min(state.trim_end_seconds),
    )
}

fn build_zoom_commands(state: &VideoEditState, start: f64, end: f64) -> (String, usize) {
    let samples = state.zoom_blur_sample_count().clamp(1, 8);
    if samples == 1 {
        return (build_sendcmd(state, start, end), 1);
    }
    let (commands, moving) = build_camera_commands(state, start, end, samples);
    if moving {
        (commands, samples)
    } else {
        (build_sendcmd(state, start, end), 1)
    }
}

fn build_camera_commands(
    state: &VideoEditState,
    start: f64,
    end: f64,
    samples: usize,
) -> (String, bool) {
    let fps = state.metadata.export_frame_rate();
    let frames = (((end - start).max(0.0) * fps).ceil() as usize).max(1);
    let (crop_x, crop_y, source_w, source_h) = state.crop_or_full();
    let (video_w, video_h) = state.video_rect_dimensions();
    let mut lines = String::new();
    let mut moving = false;
    for index in 0..frames {
        let local_t = index as f64 / fps;
        let (timeline_t, source_t) = zoom_export_times(state, start, local_t);
        let current = zoom_blur_view(state, timeline_t, source_t, 0);
        for sample in 0..samples {
            let view = if sample == 0 {
                current
            } else {
                zoom_blur_view(state, timeline_t, source_t, sample)
            };
            moving |= view != current;
            let target = if sample == 0 {
                "z".into()
            } else {
                format!("zb{sample}")
            };
            let (x, y, w, h) = (view.0 - crop_x, view.1 - crop_y, view.2, view.3);
            let scaled_w =
                super::model::even_dimension((source_w * video_w as f64 / w).round() as u32)
                    .max(video_w);
            let scaled_h =
                super::model::even_dimension((source_h * video_h as f64 / h).round() as u32)
                    .max(video_h);
            let x = ((x * scaled_w as f64 / source_w).round() as u32).min(scaled_w - video_w) & !1;
            let y = ((y * scaled_h as f64 / source_h).round() as u32).min(scaled_h - video_h) & !1;
            lines.push_str(&format!(
                "{local_t:.3} scale@{target}s w {scaled_w};\n{local_t:.3} scale@{target}s h {scaled_h};\n{local_t:.3} crop@{target} x {x};\n{local_t:.3} crop@{target} y {y};\n"
            ));
        }
    }
    (lines, moving)
}

fn camera_blur_filter(
    state: &VideoEditState,
    cmd_path: &Path,
    samples: usize,
    cursor_index: Option<usize>,
) -> String {
    let samples = samples.clamp(1, 8);
    let (video_w, video_h) = state.video_rect_dimensions();
    let mut filter = "[0:v]".to_string();
    if let Some(pad) = freeze_tail_tpad(state) {
        filter.push_str(&pad);
        filter.push_str(if cursor_index.is_some() {
            "[zb_hold];[zb_hold]"
        } else {
            ","
        });
    }
    if let Some(cursor_index) = cursor_index {
        filter.push_str(&format!(
            "[{cursor_index}:v]overlay=0:0:eof_action=pass:shortest=0:format=yuv420,"
        ));
    }
    filter.push_str(&format!(
        "sendcmd=f={},{}split={samples}",
        escape_filter_path(cmd_path),
        static_crop_prefix(state),
    ));
    for sample in 0..samples {
        filter.push_str(&format!("[zb_in{sample}]"));
    }
    for sample in 0..samples {
        let target = if sample == 0 {
            "z".into()
        } else {
            format!("zb{sample}")
        };
        filter.push_str(&format!(
            ";[zb_in{sample}]scale@{target}s={video_w}:{video_h},crop@{target}=w={video_w}:h={video_h}:x=0:y=0,setsar=1,format=yuv420p[zb_out{sample}]"
        ));
    }
    filter.push(';');
    for sample in 0..samples {
        filter.push_str(&format!("[zb_out{sample}]"));
    }
    let weights = vec!["1"; samples].join(" ");
    filter.push_str(&format!(
        "mix=inputs={samples}:weights='{weights}':scale={}:duration=shortest,format=yuv420p",
        1.0 / samples as f64
    ));
    filter
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

/// Hold the clip's last frame for the freeze tail. Only the final segment can
/// freeze, and it is the only one whose `-to` may exceed the source, so the
/// tail pads the whole chain rather than one mid-clip segment.
fn freeze_tail_tpad(state: &VideoEditState) -> Option<String> {
    let tail = state.freeze_tail_seconds();
    if tail <= 0.001 {
        return None;
    }
    Some(format!(
        "tpad=stop_mode=clone:stop_duration={}",
        format_seconds(tail)
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
    if let Some(pad) = freeze_tail_tpad(state) {
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
    // The freeze tail is video-only: the source has no audio past its end,
    // and holding a frame silent is what every editor does by default.
    let mode = if state.muted_for_source(source_start) || state.freeze_tail_seconds() > 0.001 {
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
    let segments_scratch = ScratchDir::new("segments", 0.0);
    let tmp_dir = &segments_scratch.path;

    let placed = state.ordered_placed_segments();
    let mut segment_files = Vec::new();
    let mut cursor = 0.0;
    let last_index = placed.len().saturating_sub(1);
    for (i, &(comp, start, end)) in placed.iter().enumerate() {
        let seg_path = tmp_dir.join(format!("seg_{i:04}.mp4"));
        let mut segment_state = state.clone();
        segment_state.timeline_offset_seconds = (comp - cursor).max(0.0);
        if state.muted_for_source(start) {
            segment_state.audio_mode = AudioMode::Muted;
        }
        let mut seg_end = end;
        let freeze = state.freeze_tail_seconds();
        if i == last_index && freeze > 0.0 {
            // Hold the last frame past the source's end. The builders add the
            // hold themselves, so they get the real source end; `seg_end`
            // only tracks how long the finished segment will run.
            seg_end += freeze;
        } else {
            // Only the final segment may carry the hold; clearing it here
            // also keeps the reused `segment_state` from padding mid-clip.
            segment_state.freeze_tail = 0.0;
            segment_state.frozen_segment = None;
        }
        cursor = comp + (seg_end - start).max(0.0) / state.speed_for_source(start);
        let command = if convert {
            build_single_convert_args(&segment_state, start, end, &seg_path)
        } else {
            ConvertCommand {
                args: build_single_trim_args(&segment_state, start, end, &seg_path),
                scratch: None,
                warp: None,
                cursor: None,
            }
        };
        run_command(command, &seg_path).with_context(|| format!("failed to export segment {i}"))?;
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
    run_ffmpeg(&concat_args, output_path)?;

    Ok(())
}

fn run_command(command: ConvertCommand, output_path: &Path) -> anyhow::Result<()> {
    let ConvertCommand {
        args,
        scratch,
        warp,
        cursor,
    } = command;
    // Hold the scratch tree until the command has finished reading it.
    let _scratch = scratch;
    if warp.is_none() && cursor.is_none() {
        return run_ffmpeg(&args, output_path);
    }
    run_ffmpeg_with_inputs(&args, output_path, warp, cursor)
}

/// Run ffmpeg with the GPU warp on `pipe:3` and the cursor track on `pipe:4`.
///
/// Both writers are started before ffmpeg is spawned, and each closes its pipe
/// at EOS, so ffmpeg finalizes deterministically instead of waiting on an input
/// that never ends. Neither stream is materialised on disk: the export holds
/// one frame in flight per input, which is what keeps a composite export's
/// memory bounded instead of scaling with the clip's length.
fn run_ffmpeg_with_inputs(
    args: &[String],
    output_path: &Path,
    warp_setup: Option<super::gst_warp::WarpSetup>,
    mut cursor: Option<super::cursor_track::ActiveCursorTrack>,
) -> anyhow::Result<()> {
    let mut warp = match warp_setup {
        Some(setup) => {
            Some(super::gst_warp::ActiveGstWarp::start(&setup).map_err(|err| anyhow!(err))?)
        }
        None => None,
    };

    let mut cmd = Command::new("ffmpeg");
    cmd.args(args);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    if let Some(active) = warp.as_mut() {
        let read_fd = active
            .take_read_fd()
            .ok_or_else(|| anyhow!("warp pipeline has no pipe"))?;
        crate::recording::backend::attach_pipe_as_fd(&mut cmd, 3, read_fd);
    }
    if let Some(active) = cursor.as_mut() {
        let read_fd = active
            .take_read_fd()
            .ok_or_else(|| anyhow!("cursor track has no pipe"))?;
        crate::recording::backend::attach_pipe_as_fd(&mut cmd, 4, read_fd);
    }

    let child = cmd.spawn().context("failed to run ffmpeg")?;
    drop(cmd);
    let output = child.wait_with_output()?;
    // ffmpeg has exited, so the cursor writer has either finished or hit the
    // closed pipe. Joining it here means the track never outlives the export.
    if let Some(active) = cursor.as_mut() {
        active.finish();
    }
    let bus_error = warp.as_ref().and_then(|active| active.poll_bus_error());

    if output.status.success() {
        if let Some(active) = warp.take() {
            active.stop();
        }
        return Ok(());
    }
    if let Some(active) = warp.as_mut() {
        active.abort();
    }
    let _ = std::fs::remove_file(output_path);
    let detail = match bus_error {
        Some(bus_error) => format!("ffmpeg failed: {bus_error}"),
        None => format!(
            "ffmpeg failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    };
    Err(anyhow!(detail))
}

fn run_ffmpeg(args: &[String], output_path: &Path) -> anyhow::Result<()> {
    let output = Command::new("ffmpeg")
        .args(args)
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
    fn an_export_must_fit_in_the_free_space() {
        // The boundary is inclusive: exactly enough fits, one byte short does not.
        assert!(export_fits(1_000, 1_000));
        assert!(export_fits(1_001, 1_000));
        assert!(!export_fits(999, 1_000));
    }

    #[test]
    fn the_size_estimate_scales_with_the_output_length() {
        // 1 GiB source, one minute long. Both outputs are well clear of the
        // floor, so the ratio between them is the source bitrate, not the
        // floor.
        let source = 1024 * 1024 * 1024;
        let short = estimate_export_bytes(source, 60.0, 20.0);
        let long = estimate_export_bytes(source, 60.0, 40.0);
        assert!(long > short, "a longer export must need more space");
        // Doubling the length doubles the estimate (within rounding).
        let ratio = long as f64 / short as f64;
        assert!(
            (ratio - 2.0).abs() < 0.001,
            "the estimate must scale linearly with output length, got {ratio}"
        );
    }

    #[test]
    fn the_size_estimate_never_asks_for_less_than_the_source() {
        // A same-length export re-encodes footage of about the source's size,
        // so the generous estimate is at least that size.
        let source = 400 * 1024 * 1024;
        assert!(estimate_export_bytes(source, 120.0, 120.0) >= source);
        // A trimmed export asks for proportionally less.
        let trimmed = estimate_export_bytes(source, 120.0, 60.0);
        assert!(trimmed < estimate_export_bytes(source, 120.0, 120.0));
    }

    #[test]
    fn the_size_estimate_honours_its_floor() {
        // A tiny, very short clip still reserves the floor, so it cannot slip
        // onto a disk with almost nothing free.
        assert_eq!(
            estimate_export_bytes(1, 1000.0, 0.001),
            MIN_EXPORT_RESERVE_BYTES
        );
        assert_eq!(estimate_export_bytes(0, 0.0, 0.0), MIN_EXPORT_RESERVE_BYTES);
        // A zero-length output with a real source still reserves the floor.
        assert_eq!(
            estimate_export_bytes(1 << 30, 60.0, 0.0),
            MIN_EXPORT_RESERVE_BYTES
        );
    }

    #[test]
    fn a_zero_or_bogus_source_duration_cannot_shrink_the_estimate() {
        // `file_size_bytes / duration_seconds` would divide by zero on a
        // malformed probe; the guard must not turn that into a tiny number.
        let estimate = estimate_export_bytes(1 << 30, 0.0, 30.0);
        assert!(estimate >= MIN_EXPORT_RESERVE_BYTES);
    }

    #[test]
    fn mem_available_is_parsed_in_bytes() {
        let meminfo = "MemTotal:       4000000 kB\n\
                       MemFree:         200000 kB\n\
                       MemAvailable:    1500000 kB\n\
                       Buffers:          10000 kB\n";
        assert_eq!(parse_mem_available(meminfo), Some(1_500_000 * 1024));
    }

    #[test]
    fn a_missing_mem_available_field_is_not_an_error() {
        assert_eq!(parse_mem_available("MemTotal: 4000000 kB\n"), None);
        assert_eq!(parse_mem_available("MemAvailable: bogus kB\n"), None);
        assert_eq!(parse_mem_available(""), None);
    }

    #[test]
    fn memory_is_tight_only_below_the_warning_threshold() {
        assert!(memory_is_tight(TIGHT_MEMORY_BYTES - 1));
        assert!(!memory_is_tight(TIGHT_MEMORY_BYTES));
        assert!(!memory_is_tight(TIGHT_MEMORY_BYTES + 1));
    }

    #[test]
    fn an_existing_ancestor_names_the_target_filesystem() {
        // The output file does not exist yet; its parent is the existing
        // directory the space check has to interrogate.
        let dir = std::env::temp_dir();
        let missing = dir.join("apexshot-does-not-exist-1234567").join("out.mp4");
        assert_eq!(existing_ancestor(&missing), Some(dir.as_path()));
        assert_eq!(existing_ancestor(Path::new("/")), Some(Path::new("/")));
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

    fn moving_camera_state() -> VideoEditState {
        let mut state = VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/camera-blur.mp4"),
            duration_seconds: 1.0,
            width: 320,
            height: 240,
            file_size_bytes: 100,
            has_audio: false,
            frame_rate: 30.0,
        });
        state
            .zoom_clips
            .push(crate::recording::editor::model::ZoomClip {
                start: 0.1,
                end: 0.9,
                scale: 2.0,
                center: (210.0, 120.0),
                ease_ms: 300,
                mode: crate::recording::editor::model::ZoomMode::Manual,
                ..Default::default()
            });
        state
    }

    #[test]
    fn camera_blur_is_absent_when_disabled_or_stationary() {
        let mut state = moving_camera_state();
        let sharp = composite_graph(&state);
        assert!(!sharp.contains("split="));
        assert!(!sharp.contains("blend="));
        assert_eq!(
            build_zoom_commands(&state, 0.0, 1.0).0,
            build_sendcmd(&state, 0.0, 1.0)
        );
        state.zoom_camera.motion_blur = 1.0;
        state.zoom_clips[0].instant = true;
        state.zoom_clips[0].start = 0.0;
        state.zoom_clips[0].end = 2.0;
        let stationary = composite_graph(&state);
        assert!(!stationary.contains("split="));
        assert!(!stationary.contains("blend="));
        assert_eq!(build_zoom_commands(&state, 0.0, 1.0).1, 1);
    }

    #[test]
    fn camera_blur_splits_one_current_frame_into_eight_normalized_views() {
        let mut state = moving_camera_state();
        state.zoom_camera.motion_blur = 1.0;
        let graph = composite_graph(&state);
        assert!(graph.contains("split=8"), "{graph}");
        assert_eq!(graph.matches("crop@").count(), 8);
        assert_eq!(graph.matches("mix=inputs=8").count(), 1);
        assert!(graph.contains("weights='1 1 1 1 1 1 1 1':scale=0.125:duration=shortest"));
        assert!(!graph.contains("blend=all_expr="));
        assert!(!graph.contains("yuv420p16le"));
        assert!(!graph.contains("tmix"));
        assert!(!graph.contains("tblend"));
        assert!(!graph.contains("gblur"));
        let (commands, samples) = build_zoom_commands(&state, 0.0, 1.0);
        assert_eq!(samples, 8);
        assert_eq!(commands.lines().count(), 30 * 8 * 4);
        assert!(commands.contains("crop@zb7"));
        assert!(!commands.contains("crop@zb8"));
    }

    #[test]
    fn camera_blur_export_clock_advances_through_a_speed_adjusted_freeze() {
        let mut state = moving_camera_state();
        state.timeline_offset_seconds = 2.0;
        state.segment_starts[0] = 2.0;
        state.set_selected_clip_speed(2.0);
        let (timeline_t, source_t) = zoom_export_times(&state, 0.0, 1.4);
        assert!((timeline_t - 2.7).abs() < 1e-9);
        assert_eq!(source_t, 1.0);
    }

    #[test]
    fn camera_commands_resize_the_source_without_changing_the_output_crop() {
        let mut state = moving_camera_state();
        state.zoom_clips[0].start = 0.0;
        state.zoom_clips[0].end = 2.0;
        state.zoom_clips[0].instant = true;
        let commands = build_sendcmd(&state, 0.0, 1.0);
        assert!(commands.starts_with("0.000 scale@zs w 640;\n0.000 scale@zs h 480;\n0.000 crop@z x 260;\n0.000 crop@z y 120;"));
        assert!(!commands.contains("crop@z w ") && !commands.contains("crop@z h "));
        let graph = composite_graph(&state);
        assert!(graph.contains("scale@zs=320:240,crop@z=w=320:h=240:x=0:y=0"));
    }

    #[test]
    fn sharp_and_blurred_camera_commands_share_the_same_frame_schedule() {
        let mut state = moving_camera_state();
        state.metadata.frame_rate = 60.0;
        let sharp = build_sendcmd(&state, 0.0, 1.0);
        state.zoom_camera.motion_blur = 1.0;
        let (blurred, samples) = build_zoom_commands(&state, 0.0, 1.0);
        assert_eq!(samples, 8);
        let sharp_times: Vec<_> = sharp
            .lines()
            .step_by(4)
            .map(|line| line.split_once(' ').unwrap().0)
            .collect();
        let blurred_times: Vec<_> = blurred
            .lines()
            .step_by(4 * samples)
            .map(|line| line.split_once(' ').unwrap().0)
            .collect();
        assert_eq!(sharp_times, blurred_times);
        assert_eq!(sharp_times[1], "0.017");
        let sharp_current: Vec<_> = sharp.lines().collect();
        let blurred_current: Vec<_> = blurred
            .lines()
            .filter(|line| line.contains("crop@z ") || line.contains("scale@zs "))
            .collect();
        assert_eq!(sharp_current, blurred_current);
    }

    #[test]
    fn camera_blur_keeps_cursor_ripple_and_card_on_the_existing_composite_path() {
        use crate::recording::editor::model::{ClickEffect, VideoBackground};
        use crate::recording::editor::sidecar::ClickSample;
        let mut state = cursor_composite_state(ClickEffect::Ripple);
        state.zoom_clips = moving_camera_state().zoom_clips;
        state.zoom_clips[0].start = 0.05;
        state.zoom_clips[0].end = 0.65;
        state.zoom_clips[0].center = (32.0, 24.0);
        state.zoom_camera.motion_blur = 1.0;
        state.background = VideoBackground::Plain {
            r: 30,
            g: 40,
            b: 50,
        };
        state.background_corner_radius = 10.0;
        state.sidecar.as_mut().unwrap().clicks.push(ClickSample {
            t: 1.3,
            x: 20.0,
            y: 20.0,
            button: 1,
        });
        let args = warp_args(&state);
        let graph = graph_of(&args);
        assert!(graph.contains("[0:v][2:v]overlay=0:0"), "{graph}");
        assert!(graph.find("overlay=0:0").unwrap() < graph.find("split=8").unwrap());
        assert!(graph.find("split=8").unwrap() < graph.find("alphamerge").unwrap());
        assert!(args.windows(2).any(|pair| pair == ["-video_size", "64x48"]));
        assert!(!args.iter().any(|arg| arg.contains("cursor.rgba")));
    }

    #[test]
    fn ffmpeg_camera_blur_softens_motion_without_changing_stationary_frames() {
        assert!(Command::new("ffmpeg")
            .arg("-version")
            .output()
            .unwrap()
            .status
            .success());
        let scratch = ScratchDir::new("camera-blur-test", 0.0);
        let source = scratch.path.join("stripes.mkv");
        let created = Command::new("ffmpeg")
            .args(["-y", "-nostdin", "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i"])
            .arg("nullsrc=size=320x240:rate=30:duration=1,geq=lum='if(lt(mod(X,16),8),235,16)':cb=128:cr=128")
            .args(["-c:v", "ffv1"])
            .arg(&source)
            .output().unwrap();
        assert!(
            created.status.success(),
            "{}",
            String::from_utf8_lossy(&created.stderr)
        );
        let mut state = moving_camera_state();
        state.metadata.path = source.clone();
        let render = |state: &VideoEditState| {
            let command = build_single_convert_args_with(
                state,
                0.0,
                1.0,
                Path::new("/tmp/camera-blur-output.mkv"),
                None,
            );
            let output = Command::new("ffmpeg")
                .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
                .arg(&source)
                .args([
                    "-filter_complex_threads",
                    "1",
                    "-filter_complex",
                    graph_of(&command.args),
                    "-pix_fmt",
                    "yuv420p",
                    "-f",
                    "rawvideo",
                    "-",
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        let sharp = render(&state);
        state.zoom_camera.motion_blur = 1.0;
        let blurred = render(&state);
        let frame_size = 320 * 240 * 3 / 2;
        assert_eq!(sharp.len(), 30 * frame_size);
        assert_eq!(sharp.len(), blurred.len());
        assert_eq!(&sharp[..frame_size], &blurred[..frame_size]);
        let sharpness = |video: &[u8]| -> u64 {
            video
                .chunks_exact(frame_size)
                .skip(4)
                .take(6)
                .map(|frame| {
                    frame[..320 * 240]
                        .chunks_exact(320)
                        .map(|row| {
                            row.windows(2)
                                .map(|p| p[0].abs_diff(p[1]) as u64)
                                .sum::<u64>()
                        })
                        .sum::<u64>()
                })
                .sum()
        };
        assert!(
            sharpness(&blurred) < sharpness(&sharp),
            "moving camera exposure must soften the stripe edges"
        );
        let mut comparison = image::GrayImage::new(640, 240);
        for (offset, video) in [(0, &sharp), (320, &blurred)] {
            let frame = &video[6 * frame_size..7 * frame_size];
            for y in 0..240 {
                for x in 0..320 {
                    let luma = frame[y * 320 + x].saturating_sub(16) as u16 * 255 / 219;
                    comparison.put_pixel(
                        offset + x as u32,
                        y as u32,
                        image::Luma([luma.min(255) as u8]),
                    );
                }
            }
        }
        std::fs::create_dir_all("target/test-fixtures").unwrap();
        comparison
            .save("target/test-fixtures/camera-blur-export.png")
            .unwrap();
        state.zoom_clips[0].instant = true;
        state.zoom_clips[0].start = 0.0;
        state.zoom_clips[0].end = 2.0;
        let stationary_blurred = render(&state);
        state.zoom_camera.motion_blur = 0.0;
        assert_eq!(stationary_blurred, render(&state));
    }

    #[test]
    fn camera_blur_export_streams_the_current_cursor_before_averaging() {
        use crate::recording::editor::sidecar::{
            CaptureRegion, CursorKind, PointerSample, PointerSidecar,
        };
        let scratch = ScratchDir::new("camera-blur-cursor-test", 0.0);
        let source = scratch.path.join("source.mkv");
        let output = Command::new("ffmpeg")
            .args([
                "-y",
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=black:size=320x240:rate=30:duration=1",
                "-c:v",
                "ffv1",
            ])
            .arg(&source)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut state = moving_camera_state();
        state.metadata.path = source;
        state.zoom_camera.motion_blur = 1.0;
        let without = scratch.path.join("without-cursor.mp4");
        export_edited_to(&state, without.clone()).unwrap();
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 160.0,
            y: 120.0,
            kind: CursorKind::Default,
        });
        state.sidecar = Some(sidecar);
        let with_cursor = scratch.path.join("with-cursor.mp4");
        export_edited_to(&state, with_cursor.clone()).unwrap();
        assert_ne!(first_frame_rgba(&with_cursor), first_frame_rgba(&without));
        assert!((probe_metadata(&with_cursor).unwrap().duration_seconds - 1.0).abs() < 0.034);
        state.set_trim_end(1.4);
        state.zoom_clips[0].end = 1.4;
        for strength in [0.0, 1.0] {
            state.zoom_camera.motion_blur = strength;
            let frozen = scratch.path.join(format!("frozen-cursor-{strength}.mp4"));
            export_edited_to(&state, frozen.clone()).unwrap();
            let output = Command::new("ffmpeg")
                .args([
                    "-nostdin",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-sseof",
                    "-0.04",
                    "-i",
                ])
                .arg(&frozen)
                .args(["-frames:v", "1", "-pix_fmt", "gray", "-f", "rawvideo", "-"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                output.stdout.iter().any(|&pixel| pixel > 100),
                "the cursor must remain visible in the last frozen frame at blur {strength}"
            );
            assert!((probe_metadata(&frozen).unwrap().duration_seconds - 1.4).abs() < 0.034);
        }
    }

    #[test]
    fn an_early_ffmpeg_exit_closes_the_parent_cursor_reader_before_joining() {
        use crate::recording::editor::sidecar::{
            CaptureRegion, CursorKind, PointerSample, PointerSidecar,
        };
        let mut state = moving_camera_state();
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 160.0,
            y: 120.0,
            kind: CursorKind::Default,
        });
        state.sidecar = Some(sidecar);
        let track =
            super::super::cursor_track::ActiveCursorTrack::start(&state, 0.0, 1.0, 320, 240, false)
                .unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = run_ffmpeg_with_inputs(
                &["-version".into()],
                Path::new("/nonexistent/unused-cursor-output.mp4"),
                None,
                Some(track),
            );
            let _ = sender.send(result);
        });
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the cursor writer must stop when ffmpeg exits without consuming its pipe")
            .unwrap();
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

    fn cursor_composite_state(
        effect: crate::recording::editor::model::ClickEffect,
    ) -> VideoEditState {
        use crate::recording::editor::sidecar::{
            CaptureRegion, CursorKind, PointerSample, PointerSidecar,
        };
        let mut state = VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/input.mp4"),
            duration_seconds: 10.0,
            width: 64,
            height: 48,
            file_size_bytes: 100,
            has_audio: false,
            frame_rate: 30.0,
        });
        state.trim_start_seconds = 1.25;
        state.trim_end_seconds = 1.6;
        state.cursor.click_effect = effect;
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 10.0,
            kind: CursorKind::Default,
        });
        state.sidecar = Some(sidecar);
        state
    }

    fn composite_graph(state: &VideoEditState) -> String {
        let args = build_single_convert_args(
            state,
            state.trim_start_seconds,
            state.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
        );
        args.windows(2)
            .find(|pair| pair[0] == "-filter_complex")
            .map(|pair| pair[1].clone())
            .expect("the cursor overlay exports through the composite graph")
    }

    /// Build the command with a forced GPU warp, independent of whether the
    /// test host has the GL plugins installed.
    fn warp_args(state: &VideoEditState) -> Vec<String> {
        let warp = crate::recording::editor::gst_warp::WarpSetup::from_state(
            state,
            state.trim_start_seconds,
            state.trim_end_seconds,
        )
        .expect("a visible ripple must build a warp");
        build_single_convert_args_with(
            state,
            state.trim_start_seconds,
            state.trim_end_seconds,
            Path::new("/tmp/output.mp4"),
            Some(warp),
        )
        .args
    }

    fn graph_of(args: &[String]) -> &str {
        args.windows(2)
            .find(|pair| pair[0] == "-filter_complex")
            .map(|pair| pair[1].as_str())
            .expect("the cursor overlay exports through the composite graph")
    }

    /// Drop the scratch directory's per-command sequence number, which is the
    /// only part of the graph that differs between two builds.
    fn normalized_graph(graph: &str) -> &str {
        graph
            .split_once("zoom.cmd")
            .map(|(_, rest)| rest)
            .unwrap_or(graph)
    }

    #[test]
    fn a_visible_ripple_displaces_the_footage_before_the_cursor_overlay() {
        use crate::recording::editor::model::ClickEffect;
        use crate::recording::editor::sidecar::ClickSample;

        let mut state = cursor_composite_state(ClickEffect::Ripple);
        state.sidecar.as_mut().unwrap().clicks.push(ClickSample {
            t: 1.3,
            x: 20.0,
            y: 20.0,
            button: 1,
        });
        let args = warp_args(&state);
        let graph = graph_of(&args);
        // The GPU warp feeds raw frames over the inherited pipe as input 0;
        // the source follows as input 1, so the cursor overlay moves to 2.
        assert!(
            args.windows(2).any(|pair| pair == ["-i", "pipe:3"]),
            "the warp must enter through the inherited pipe: {args:?}"
        );
        assert!(args.windows(2).any(|pair| pair == ["-pix_fmt", "yuv420p"]));
        assert!(graph.contains("[0:v]sendcmd="), "{graph}");
        assert!(
            graph.contains("[vc0];[vc0][2:v]overlay=0:0:eof_action=pass:shortest=0:format=yuv420"),
            "the cursor overlay must follow the warp input: {graph}"
        );
        assert!(
            !graph.contains("remap"),
            "the warp must not materialise maps: {graph}"
        );
    }

    #[test]
    fn a_clip_without_a_visible_ripple_keeps_the_plain_composite_graph() {
        use crate::recording::editor::model::ClickEffect;

        // Same sidecar and window, different effect and no clicks: the graph
        // must be exactly the pre-warp graph, so the common path cannot
        // regress.
        let ripple = cursor_composite_state(ClickEffect::Ripple);
        let off = cursor_composite_state(ClickEffect::None);
        let graph = composite_graph(&ripple);
        assert_eq!(
            normalized_graph(&graph),
            normalized_graph(&composite_graph(&off))
        );
        assert!(!graph.contains("remap"));
        assert!(graph.contains("[0:v]sendcmd="));

        // A click on an effect that is not the ripple stays on the drawn path.
        use crate::recording::editor::sidecar::ClickSample;
        let mut circle = cursor_composite_state(ClickEffect::Circle);
        circle.sidecar.as_mut().unwrap().clicks.push(ClickSample {
            t: 1.3,
            x: 20.0,
            y: 20.0,
            button: 1,
        });
        assert_eq!(
            normalized_graph(&composite_graph(&circle)),
            normalized_graph(&graph)
        );
    }

    #[test]
    fn a_ripple_warp_keeps_the_wallpaper_and_mask_inputs_in_order() {
        use crate::recording::editor::model::{ClickEffect, VideoBackground};
        use crate::recording::editor::sidecar::ClickSample;

        let dir =
            std::env::temp_dir().join(format!("apexshot-ripple-wallpaper-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let wallpaper = dir.join("wallpaper.jpg");
        std::fs::write(&wallpaper, b"fake-jpg").unwrap();

        let mut state = cursor_composite_state(ClickEffect::Ripple);
        state.background = VideoBackground::Wallpaper(wallpaper);
        state.background_padding = 20.0;
        state.background_corner_radius = 10.0;
        state.sidecar.as_mut().unwrap().clicks.push(ClickSample {
            t: 1.3,
            x: 20.0,
            y: 20.0,
            button: 1,
        });
        let args = warp_args(&state);
        let graph = graph_of(&args);
        // Input order is warp pipe, source, cursor, wallpaper, mask.
        assert!(
            graph.contains("[vc0];[vc0][2:v]overlay="),
            "cursor must be input 2: {graph}"
        );
        assert!(
            graph.contains("[3:v]scale="),
            "wallpaper must be input 3: {graph}"
        );
        assert!(
            graph.contains("[4:v]alphamerge"),
            "mask must be input 4: {graph}"
        );
        let _ = std::fs::remove_dir_all(&dir);
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
    fn a_corner_radius_masks_the_card_onto_the_fill() {
        // A radius needs the composite graph and has to blend the rounded card
        // onto the fill; `pad` would leave the corners clear and the encoder
        // would drop that alpha, so the export has to overlay instead.
        let mut s = state();
        s.background = VideoBackground::Plain {
            r: 220,
            g: 30,
            b: 40,
        };
        s.background_corner_radius = 24.0;
        assert!(s.has_corner_radius());
        assert!(s.needs_composite());

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
            .expect("a radius exports through the composite graph");
        assert!(
            graph.contains("alphamerge"),
            "the card must take the mask's alpha: {graph}"
        );
        assert!(
            graph.contains("overlay=(W-w)/2:(H-h)/2:format=auto"),
            "the rounded card must blend onto the fill instead of padding: {graph}"
        );
        assert!(
            args.windows(2).any(|pair| pair == ["-loop", "1"]),
            "the mask must loop as an ffmpeg input: {args:?}"
        );
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
            graph.contains(&format!("scale@zs={video_w}:{video_h}")),
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

    /// RGB of pixel (`x`, `y`) at `t` seconds into `path`.
    fn pixel_at(path: &Path, t: f64, x: u32, y: u32) -> (u8, u8, u8) {
        let output = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-ss",
                &format!("{t:.3}"),
                "-i",
                path.to_str().unwrap(),
                "-vf",
                &format!("crop=2:2:{x}:{y}"),
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

    /// The cursor overlay is streamed into ffmpeg through a pipe rather than
    /// written to a scratch file.
    ///
    /// This is the test that proves the pipe works end to end. If the fd
    /// plumbing were wrong the export would hang, and if the stream arrived
    /// empty `eof_action=pass` would hand back an output identical to one with
    /// no cursor overlay at all — both of which this catches.
    #[test]
    fn a_composite_export_streams_the_cursor_track_into_the_output() {
        use crate::recording::editor::sidecar::{
            CaptureRegion, CursorKind, PointerSample, PointerSidecar,
        };

        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test-fixtures");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join(format!(
            "apexshot-cursor-stream-source-{}.mp4",
            std::process::id()
        ));
        let with_cursor = dir.join(format!(
            "apexshot-cursor-stream-on-{}.mp4",
            std::process::id()
        ));
        let without = dir.join(format!(
            "apexshot-cursor-stream-off-{}.mp4",
            std::process::id()
        ));

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

        // The same export twice: once with a pointer track, once without.
        let plain = VideoEditState::new(probe_metadata(&source).expect("probe the fixture"));
        export_edited_to(&plain, without.clone()).expect("export without a cursor");

        let mut tracked = VideoEditState::new(probe_metadata(&source).expect("probe the fixture"));
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        for index in 0..30 {
            sidecar.pointer.push(PointerSample {
                t: index as f64 / 30.0,
                x: 160.0,
                y: 120.0,
                kind: CursorKind::Default,
            });
        }
        tracked.sidecar = Some(sidecar);
        export_edited_to(&tracked, with_cursor.clone()).expect("export with a cursor");

        let plain_frame = first_frame_rgba(&without);
        let cursor_frame = first_frame_rgba(&with_cursor);
        assert!(!plain_frame.is_empty(), "the exported frames must decode");
        assert_eq!(
            plain_frame.len(),
            cursor_frame.len(),
            "both exports keep the same geometry"
        );
        assert_ne!(
            plain_frame, cursor_frame,
            "the streamed cursor track must change the exported pixels"
        );
    }

    /// Decode frame 0 of `path` as RGBA so two exports can be compared.
    fn first_frame_rgba(path: &Path) -> Vec<u8> {
        let output = Command::new("ffmpeg")
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(path)
            .args(["-frames:v", "1", "-pix_fmt", "rgba", "-f", "rawvideo", "-"])
            .output()
            .expect("decode a frame");
        output.stdout
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

    #[test]
    fn a_zoom_advances_through_the_frozen_tail_in_the_export() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test-fixtures");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join(format!(
            "apexshot-freeze-zoom-source-{}.mp4",
            std::process::id()
        ));
        let frozen = dir.join(format!(
            "apexshot-freeze-zoom-out-{}.mp4",
            std::process::id()
        ));

        // One second, left half black and right half white, so a crop that
        // pans right changes what a fixed output pixel samples.
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
                "color=c=black:s=320x240:r=30:d=1",
                "-vf",
                "drawbox=x=160:y=0:w=160:h=240:color=white:t=fill",
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
        assert!(state.extend_last_segment(1.0), "the hold must take");
        // The zoom lives entirely in the hold and pans onto the white half, so
        // it only reaches its framing *after* the source's last frame.
        state
            .zoom_clips
            .push(crate::recording::editor::model::ZoomClip {
                start: 1.0,
                end: 2.0,
                scale: 2.0,
                center: (240.0, 120.0),
                ease_ms: 0,
                easing: crate::recording::editor::model::ZoomEasing::Smooth,
                mode: crate::recording::editor::model::ZoomMode::Manual,
                ..Default::default()
            });
        export_edited_to(&state, frozen.clone()).expect("export the held tail");

        // Real footage, before the zoom: the top-left pixel is the black half.
        let (r0, g0, b0) = pixel_at(&frozen, 0.3, 10, 10);
        assert!(
            r0 < 60 && g0 < 60 && b0 < 60,
            "real frames before the zoom stay black, got ({r0},{g0},{b0})"
        );
        // Deep in the hold the camera has panned right, so the same output
        // pixel now samples the white half instead of a frozen, un-zoomed copy.
        let (r1, g1, b1) = pixel_at(&frozen, 1.7, 10, 10);
        assert!(
            r1 > 180 && g1 > 180 && b1 > 180,
            "the held tail must be re-cropped by the advancing zoom, got ({r1},{g1},{b1})"
        );

        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_file(&frozen);
    }

    #[test]
    fn auto_zoom_follow_camera_pans_to_the_pointer_in_the_export() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test-fixtures");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join(format!("apexshot-follow-source-{}.mp4", std::process::id()));
        let followed = dir.join(format!("apexshot-follow-out-{}.mp4", std::process::id()));
        let snapped = dir.join(format!(
            "apexshot-follow-instant-{}.mp4",
            std::process::id()
        ));

        // Left half black, right half white: a camera that pans right turns a
        // fixed output pixel from black to white.
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
                "color=c=black:s=320x240:r=30:d=2",
                "-vf",
                "drawbox=x=160:y=0:w=160:h=240:color=white:t=fill",
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
        let mut sidecar = crate::recording::editor::sidecar::PointerSidecar::new(
            0,
            crate::recording::editor::sidecar::CaptureRegion {
                x: 0,
                y: 0,
                w: 320,
                h: 240,
            },
        );
        // Pointer parked on the white half for the whole clip.
        for t in [0.0, 0.5, 1.0, 1.5, 2.0] {
            sidecar
                .pointer
                .push(crate::recording::editor::sidecar::PointerSample {
                    t,
                    x: 240.0,
                    y: 120.0,
                    kind: crate::recording::editor::sidecar::CursorKind::Default,
                });
        }
        state.sidecar = Some(sidecar);
        // The stored framing starts on the black half; the follow camera must
        // carry it onto the white half the pointer sits on.
        state
            .zoom_clips
            .push(crate::recording::editor::model::ZoomClip {
                start: 0.0,
                end: 2.0,
                scale: 2.0,
                center: (80.0, 120.0),
                ease_ms: 0,
                easing: crate::recording::editor::model::ZoomEasing::Smooth,
                mode: crate::recording::editor::model::ZoomMode::Auto,
                ..Default::default()
            });
        export_edited_to(&state, followed.clone()).expect("export the follow");
        // Late in the clip the spring has carried the viewport onto white.
        let (r1, g1, b1) = pixel_at(&followed, 1.5, 10, 10);
        assert!(
            r1 > 180 && g1 > 180 && b1 > 180,
            "the follow camera must pan onto the pointer, got ({r1},{g1},{b1})"
        );

        // An instant zoom snaps instead of chasing: early in the clip it
        // already samples white while the animated follow is still travelling.
        state.zoom_clips[0].instant = true;
        export_edited_to(&state, snapped.clone()).expect("export the snap");
        let (r0, g0, b0) = pixel_at(&snapped, 0.1, 10, 10);
        assert!(
            r0 > 180 && g0 > 180 && b0 > 180,
            "an instant zoom should snap onto the pointer, got ({r0},{g0},{b0})"
        );

        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_file(&followed);
        let _ = std::fs::remove_file(&snapped);
    }

    #[test]
    fn a_vaapi_export_uploads_the_graph_and_nvenc_leaves_it_alone() {
        // NVENC and libx264 read system-memory frames, so the graph is
        // untouched; VA-API needs a trailing upload after the last filter.
        assert_eq!(
            with_hardware_upload(None, Some(HardwareEncoder::Nvenc)),
            None
        );
        assert_eq!(with_hardware_upload(None, None), None);
        assert_eq!(
            with_hardware_upload(None, Some(HardwareEncoder::Vaapi)),
            Some(hardware_encode::VAAPI_UPLOAD_FILTER.to_string())
        );
        assert_eq!(
            with_hardware_upload(Some("scale=2:2".into()), Some(HardwareEncoder::Vaapi)),
            Some(format!(
                "scale=2:2,{}",
                hardware_encode::VAAPI_UPLOAD_FILTER
            ))
        );
        assert_eq!(
            with_hardware_upload(Some("scale=2:2".into()), Some(HardwareEncoder::Nvenc)),
            Some("scale=2:2".into())
        );
    }

    #[test]
    fn the_default_export_encoder_is_software() {
        // No opt-in: the export must be exactly libx264 at the tier's CRF, so a
        // machine with a GPU cannot change the file a user without one gets.
        let args = export_video_args(None, ExportQuality::High);
        assert!(args.windows(2).any(|pair| pair == ["-c:v", "libx264"]));
        assert!(args.windows(2).any(|pair| pair == ["-crf", "20"]));
        assert!(!args.iter().any(|arg| arg == "-qp"));
    }

    #[test]
    fn an_opted_in_hardware_encoder_replaces_software() {
        let nvenc = export_video_args(Some(HardwareEncoder::Nvenc), ExportQuality::Ultra);
        assert!(nvenc.windows(2).any(|pair| pair == ["-c:v", "h264_nvenc"]));
        assert!(!nvenc.iter().any(|arg| arg == "libx264"));
        // Ultra is CRF 16, which is the quantizer the hardware encoder gets.
        assert!(nvenc.windows(2).any(|pair| pair == ["-qp", "16"]));
        assert!(!nvenc.iter().any(|arg| arg == "-crf"));
    }
}

#[cfg(test)]
mod freeze_tests {
    use super::build_single_convert_args;
    use crate::recording::editor::model::{VideoEditState, VideoMetadata};
    use std::path::PathBuf;

    fn state() -> VideoEditState {
        VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/input.mp4"),
            duration_seconds: 10.0,
            width: 1920,
            height: 1080,
            file_size_bytes: 1024,
            has_audio: true,
            frame_rate: 30.0,
        })
    }

    fn filter_of(args: &[String]) -> String {
        args.windows(2)
            .find(|pair| pair[0] == "-vf" || pair[0] == "-filter_complex")
            .map(|pair| pair[1].clone())
            .unwrap_or_default()
    }

    #[test]
    fn a_frozen_tail_pads_the_video_filter_with_a_cloned_stop() {
        let mut state = state();
        state.extend_last_segment(1.5);
        let args =
            build_single_convert_args(&state, 0.0, 10.0, std::path::Path::new("/tmp/out.mp4"));
        let filter = filter_of(&args);
        assert!(
            filter.contains("tpad=stop_mode=clone:stop_duration=1.500"),
            "held frames must be padded at the end: {filter}"
        );
    }

    #[test]
    fn a_frozen_tail_silences_the_audio() {
        let mut state = state();
        state.extend_last_segment(1.0);
        let args =
            build_single_convert_args(&state, 0.0, 10.0, std::path::Path::new("/tmp/out.mp4"));
        assert!(
            args.iter().any(|arg| arg == "-an"),
            "the hold must not drag audio past the source end: {args:?}"
        );
    }

    #[test]
    fn a_frozen_tail_clones_before_the_zoom_crop() {
        let mut state = state();
        state.extend_last_segment(1.0);
        state
            .zoom_clips
            .push(crate::recording::editor::model::ZoomClip {
                start: 9.0,
                end: 10.5,
                scale: 2.0,
                center: (1400.0, 700.0),
                ease_ms: 600,
                easing: crate::recording::editor::model::ZoomEasing::Smooth,
                mode: crate::recording::editor::model::ZoomMode::Manual,
                ..Default::default()
            });
        let args =
            build_single_convert_args(&state, 0.0, 10.0, std::path::Path::new("/tmp/out.mp4"));
        let filter = filter_of(&args);
        let freeze = filter
            .find("tpad=stop_mode=clone")
            .expect("the hold must pad the filter graph");
        let sendcmd = filter
            .find("sendcmd=")
            .expect("a zoom must drive the crop through sendcmd");
        assert!(
            freeze < sendcmd,
            "the hold must clone frames before the zoom crop so the tail is re-cropped: {filter}"
        );
    }

    #[test]
    fn without_a_freeze_no_stop_padding_is_emitted() {
        let state = state();
        let args =
            build_single_convert_args(&state, 0.0, 10.0, std::path::Path::new("/tmp/out.mp4"));
        let filter = filter_of(&args);
        assert!(
            !filter.contains("stop_mode=clone"),
            "an ordinary export must stay untouched: {filter}"
        );
    }

    #[test]
    fn a_composite_export_removes_its_scratch_dir_when_the_command_drops() {
        // A zoom forces the composite graph, which writes `zoom.cmd` into a
        // scratch dir. The command owns that dir; dropping it must delete the
        // whole tree so an export cannot leak its scratch behind.
        let mut s = state();
        s.zoom_clips
            .push(crate::recording::editor::model::ZoomClip {
                start: 1.5,
                end: 3.3,
                scale: 1.8,
                center: (960.0, 540.0),
                ..Default::default()
            });
        assert!(s.needs_composite());
        let command = build_single_convert_args(
            &s,
            s.trim_start_seconds,
            s.trim_end_seconds,
            std::path::Path::new("/tmp/output.mp4"),
        );
        let scratch = command
            .scratch
            .as_ref()
            .expect("a composite command must own a scratch dir")
            .path
            .clone();
        assert!(
            scratch.is_dir(),
            "the scratch dir must exist while the command runs"
        );
        drop(command);
        assert!(
            !scratch.exists(),
            "dropping the command must remove its scratch dir"
        );
    }

    #[test]
    fn the_sweep_removes_scratch_from_dead_processes_only() {
        // A pid that cannot be running: the sweep must remove its tree, which
        // is what a crashed or killed export would have left behind.
        let stale = std::env::temp_dir().join(format!("apexshot-export-{}-7-0", u32::MAX - 3));
        std::fs::create_dir_all(&stale).unwrap();
        // Our own pid is live: the sweep must leave its tree alone.
        let live = std::env::temp_dir().join(format!("apexshot-export-{}-7-0", std::process::id()));
        std::fs::create_dir_all(&live).unwrap();

        super::sweep_stale_scratch_dirs();

        assert!(
            !stale.exists(),
            "stale scratch from a dead pid must be removed"
        );
        assert!(live.exists(), "live scratch must be kept");
        let _ = std::fs::remove_dir_all(&live);
    }

    #[test]
    fn export_scratch_prefers_a_root_that_is_not_ram_backed() {
        // A composite export writes a multi-gigabyte `cursor.rgba`. When any
        // candidate root is on real disk that is the one to use, because `/tmp`
        // is a tmpfs on most desktops and choosing it spends RAM instead.
        let expected = super::scratch_roots()
            .into_iter()
            .find(|root| !super::is_ram_backed(root));
        if let Some(expected) = expected {
            assert_eq!(
                super::scratch_root(),
                expected,
                "the scratch root must be the first candidate that is not RAM-backed"
            );
        }
    }

    #[test]
    fn the_sweep_removes_a_stale_tree_from_the_preferred_scratch_root() {
        // The sweep used to look only in the temp directory. New trees go to
        // the cache directory, so it has to cover both — otherwise a crashed
        // export leaves a multi-gigabyte tree where nothing ever looks.
        let root = super::scratch_root();
        let stale = root.join(format!("apexshot-export-{}-9-0", u32::MAX - 5));
        std::fs::create_dir_all(&stale).unwrap();

        super::sweep_stale_scratch_dirs();

        assert!(
            !stale.exists(),
            "a stale tree in the scratch root ({}) must be swept",
            root.display()
        );
    }
}
