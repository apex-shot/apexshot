//! Chunked export with two encoders.
//!
//! The timeline is cut into contiguous pieces. Each piece is encoded to its own
//! MP4 by whichever encoder has the smaller backlog, so a hardware encoder and
//! the software encoder can work on different pieces at the same time. The
//! pieces are joined at the end without re-encoding.
//!
//! Every piece starts on an IDR and all encoders are opened with the same
//! profile, level, pixel format, frame rate, and size, so the concat demuxer
//! can copy the streams into one file. The frame plan is unchanged: pieces are
//! in order and cover every frame exactly once.

use super::{spawn_encoder, stop, Encoder, EncoderPipe, End};
use crate::recording::editor::ffmpeg;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Shortest export worth splitting. The join probe and the extra process
/// start-up cost about a second.
pub(super) const MIN_SECONDS: f64 = 6.0;
/// Target piece length.
const CHUNK_SECONDS: f64 = 4.0;
/// Upper bound on pieces, so a long recording does not pay a process start-up
/// every few seconds.
const MAX_CHUNKS: u64 = 120;
/// Frames a piece's writer may queue. Larger than the single-encoder pipe so
/// the other encoder has work while this piece drains at a boundary.
const PIPE_BUDGET: usize = 64 * 1024 * 1024;
/// Below this many logical cores, two encoders fight over the same cores.
const MIN_PARALLELISM: usize = 6;
/// Finished pieces that may wait for reaping before the oldest is joined.
const MAX_PENDING: usize = 4;

/// Whether a hybrid export makes sense on this machine and timeline.
pub(super) fn supported(primary: Encoder, total: u64, fps: f64) -> bool {
    !primary.is_software()
        && !primary.is_hevc()
        && fps > 0.0
        && total as f64 >= fps * MIN_SECONDS
        && total >= 2
        && std::thread::available_parallelism().map_or(1, |count| count.get()) >= MIN_PARALLELISM
        && mixed_join_supported(primary)
}

/// How many frames a piece holds. Short exports keep the four-second target;
/// long ones grow the piece so `MAX_CHUNKS` is never exceeded.
pub(super) fn chunk_frames(fps: f64, total: u64) -> u64 {
    let target = (fps * CHUNK_SECONDS).round().max(1.0) as u64;
    let cap = total.div_ceil(MAX_CHUNKS).max(1);
    target.max(cap).min(total.max(1))
}

fn piece_depth(frame_bytes: usize) -> usize {
    (PIPE_BUDGET / frame_bytes.max(1)).clamp(4, 16)
}

fn frames_in(total: u64, chunk_frames: u64, chunk: usize) -> u64 {
    let first = chunk as u64 * chunk_frames;
    total.saturating_sub(first).min(chunk_frames)
}

fn choose(loads: &[Load; 2], chunk: usize) -> usize {
    if chunk == 0 {
        return 0;
    }
    if loads[0].cost() <= loads[1].cost() {
        0
    } else {
        1
    }
}

/// One piece being encoded, or waiting to be joined.
struct Segment {
    pipe: Option<EncoderPipe>,
    child: Option<Child>,
    path: PathBuf,
    chunk: usize,
    encoder: usize,
    frames: u64,
    started: Instant,
}

impl Segment {
    fn is_done(&mut self) -> bool {
        let child_done = self
            .child
            .as_mut()
            .is_none_or(|child| child.try_wait().map_or(true, |status| status.is_some()));
        let pipe_done = self.pipe.as_ref().is_none_or(|pipe| pipe.is_finished());
        child_done && pipe_done
    }

    /// Close the pipe and wait for the encoder to finish the file.
    fn finish(&mut self) -> Result<(), End> {
        let pipe = self
            .pipe
            .take()
            .ok_or_else(|| End::Failed("a piece encoder stopped accepting frames".to_string()))?;
        let mut child = self
            .child
            .take()
            .ok_or_else(|| End::Failed("a piece encoder disappeared".to_string()))?;
        if let Err(error) = pipe.finish() {
            stop(&mut child);
            return Err(error);
        }
        let result = child
            .wait_with_output()
            .map_err(|error| End::Failed(format!("a piece encoder did not finish: {error}")))?;
        if !result.status.success() {
            let detail = String::from_utf8_lossy(&result.stderr);
            return Err(End::Failed(format!(
                "a piece encoder failed: {}",
                detail.trim()
            )));
        }
        Ok(())
    }

    fn kill(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.pipe.take();
    }
}

impl Drop for Segment {
    fn drop(&mut self) {
        if self.child.is_some() {
            self.kill();
        }
    }
}

#[derive(Clone, Copy)]
struct Load {
    /// Frames handed to this encoder whose piece has not finished yet.
    pending: u64,
    /// Recent wall time per frame, including the wait in the pipe.
    ms_per_frame: f64,
}

impl Default for Load {
    fn default() -> Self {
        Self {
            pending: 0,
            ms_per_frame: 0.0,
        }
    }
}

impl Load {
    fn cost(&self) -> f64 {
        self.pending as f64 * self.ms_per_frame.max(0.1)
    }
}

/// The pieces' scratch directory, removed even when the export fails.
pub(super) struct PartDir(PathBuf);

impl PartDir {
    pub(super) fn create(output: &Path) -> Result<Self, End> {
        let parent = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let path = parent.join(format!(".apexshot-parts-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).map_err(|error| {
            End::Failed(format!(
                "could not create the export scratch folder: {error}"
            ))
        })?;
        Ok(Self(path))
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for PartDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Routes frames to the piece that owns them and joins the pieces at the end.
pub(super) struct SegmentedSink {
    dir: PartDir,
    output: PathBuf,
    encoders: [Encoder; 2],
    size: (u32, u32),
    fps: f64,
    crf: u32,
    frame_bytes: usize,
    total: u64,
    chunk_frames: u64,
    parts: Vec<Option<PathBuf>>,
    loads: [Load; 2],
    active: Option<Segment>,
    pending: Vec<Segment>,
}

impl SegmentedSink {
    pub(super) fn new(
        primary: Encoder,
        output: &Path,
        size: (u32, u32),
        fps: f64,
        crf: u32,
        total: u64,
    ) -> Result<Self, End> {
        let chunk_frames = chunk_frames(fps, total);
        let pieces = total.div_ceil(chunk_frames) as usize;
        let dir = PartDir::create(output)?;
        Ok(Self {
            dir,
            output: output.to_path_buf(),
            encoders: [primary, Encoder::X264],
            size,
            fps,
            crf,
            frame_bytes: size.0 as usize * size.1 as usize * 3 / 2,
            total,
            chunk_frames,
            parts: vec![None; pieces],
            loads: [Load::default(); 2],
            active: None,
            pending: Vec::new(),
        })
    }

    /// The buffer for a frame, moving to the next piece first when the frame
    /// starts a new one.
    pub(super) fn buffer(&mut self, index: u64) -> Result<Vec<u8>, End> {
        let chunk = (index / self.chunk_frames) as usize;
        if self
            .active
            .as_ref()
            .is_none_or(|segment| segment.chunk != chunk)
        {
            self.close_active();
            self.reap()?;
            let encoder = self.choose_encoder(chunk);
            self.active = Some(self.spawn(chunk, encoder)?);
        }
        let active = self.active.as_mut().expect("the piece was spawned above");
        Ok(active
            .pipe
            .as_ref()
            .expect("the active piece keeps its pipe until it is closed")
            .buffer())
    }

    pub(super) fn submit(&mut self, frame: Vec<u8>) -> Result<(), End> {
        self.active
            .as_ref()
            .ok_or_else(|| End::Failed("no export piece is open".to_string()))?
            .pipe
            .as_ref()
            .ok_or_else(|| End::Failed("the export piece stopped accepting frames".to_string()))?
            .submit(frame)
    }

    pub(super) fn finish(mut self) -> Result<(), End> {
        self.close_active();
        while let Some(mut segment) = self.pending.pop() {
            segment.finish()?;
        }
        let parts: Vec<PathBuf> = self.parts.iter().flatten().cloned().collect();
        join_segments(&self.dir.0, &parts, &self.output, false)
    }

    fn close_active(&mut self) {
        if let Some(mut segment) = self.active.take() {
            if let Some(pipe) = segment.pipe.as_mut() {
                pipe.close();
            }
            self.parts[segment.chunk] = Some(segment.path.clone());
            self.pending.push(segment);
        }
    }

    fn spawn(&mut self, chunk: usize, encoder: usize) -> Result<Segment, End> {
        let path = self.dir.0.join(format!("piece-{chunk:05}.mp4"));
        let mut child = spawn_encoder(
            &path,
            self.size,
            self.fps,
            self.crf,
            self.encoders[encoder],
            false,
        )?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| End::Failed("a piece encoder did not open its input".to_string()))?;
        let frames = frames_in(self.total, self.chunk_frames, chunk);
        self.loads[encoder].pending += frames;
        self.parts[chunk] = Some(path.clone());
        Ok(Segment {
            pipe: Some(EncoderPipe::new(
                stdin,
                self.frame_bytes,
                piece_depth(self.frame_bytes),
            )),
            child: Some(child),
            path,
            chunk,
            encoder,
            frames,
            started: Instant::now(),
        })
    }

    /// The encoder with the smaller predicted backlog. The first piece uses
    /// the hardware encoder; after that the measured times decide.
    fn choose_encoder(&self, chunk: usize) -> usize {
        choose(&self.loads, chunk)
    }

    /// Join pieces that finished, blocking on the oldest when too many wait.
    fn reap(&mut self) -> Result<(), End> {
        let mut index = 0;
        while index < self.pending.len() {
            if self.pending[index].is_done() {
                let mut segment = self.pending.remove(index);
                self.complete(&mut segment)?;
            } else {
                index += 1;
            }
        }
        if self.pending.len() >= MAX_PENDING {
            let mut segment = self.pending.remove(0);
            self.complete(&mut segment)?;
        }
        Ok(())
    }

    fn complete(&mut self, segment: &mut Segment) -> Result<(), End> {
        let elapsed = segment.started.elapsed().as_secs_f64();
        segment.finish()?;
        let load = &mut self.loads[segment.encoder];
        load.pending = load.pending.saturating_sub(segment.frames);
        let ms = elapsed * 1000.0 / segment.frames.max(1) as f64;
        load.ms_per_frame = if load.ms_per_frame > 0.0 {
            load.ms_per_frame * 0.6 + ms * 0.4
        } else {
            ms
        };
        eprintln!(
            "[export] piece {}: {} frames at {:.1} ms/frame",
            segment.chunk + 1,
            segment.frames,
            ms
        );
        Ok(())
    }
}

/// Whether two encoders' streams can be joined without re-encoding.
///
/// The probe encodes a few frames with each and decodes the join. A machine
/// that cannot join them still exports; it just stays on one encoder.
pub(super) fn mixed_join_supported(primary: Encoder) -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        match std::env::var("APEXSHOT_EXPORT_HYBRID").ok().as_deref() {
            Some("0") => return false,
            Some("1") => return true,
            _ => {}
        }
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "apexshot-hybrid-probe-{}-{unique}",
            std::process::id()
        ));
        if fs::create_dir_all(&dir).is_err() {
            return false;
        }
        let supported = probe_pair(&dir, primary).unwrap_or(false);
        let _ = fs::remove_dir_all(&dir);
        eprintln!(
            "[export] hybrid join probe: {}",
            if supported {
                "supported"
            } else {
                "unavailable"
            }
        );
        supported
    })
}

fn probe_pair(dir: &Path, primary: Encoder) -> Result<bool, End> {
    let size = (320, 180);
    let black = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
    let mut parts = Vec::with_capacity(2);
    for (index, encoder) in [primary, Encoder::X264].into_iter().enumerate() {
        let path = dir.join(format!("probe-{index}.mp4"));
        let mut child = spawn_encoder(&path, size, 30.0, 23, encoder, false)?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| End::Failed("the probe encoder did not open its input".to_string()))?;
        for _ in 0..8 {
            if stdin.write_all(&black).is_err() {
                break;
            }
        }
        drop(stdin);
        let result = child
            .wait_with_output()
            .map_err(|error| End::Failed(format!("the probe encoder did not finish: {error}")))?;
        if !result.status.success() {
            return Ok(false);
        }
        parts.push(path);
    }
    let joined = dir.join("joined.mp4");
    join_segments(dir, &parts, &joined, false)?;
    let decoded = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(&joined)
        .args(["-f", "null", "-"])
        .output()
        .map_err(|error| End::Failed(format!("the probe decoder did not start: {error}")))?;
    Ok(decoded.status.success() && decoded.stderr.is_empty())
}

/// The stream format a packet-copy join depends on: codec, profile, level,
/// size, pixel format, and frame rate of the first video stream.
fn stream_format(path: &Path) -> Result<String, End> {
    let output = crate::utils::hidden_command(ffmpeg::ffprobe_executable())
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,profile,level,width,height,pix_fmt,r_frame_rate",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .map_err(|error| End::Failed(format!("could not start ffprobe: {error}")))?;
    let format = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || format.is_empty() {
        return Err(End::Failed(format!(
            "could not read the export piece {}",
            path.display()
        )));
    }
    Ok(format)
}

/// Every piece must share the first piece's format, or the joined file
/// switches stream parameters mid-way.
fn check_formats(parts: &[PathBuf]) -> Result<(), End> {
    let Some(first) = parts.first() else {
        return Ok(());
    };
    let expected = stream_format(first)?;
    for (index, part) in parts.iter().enumerate().skip(1) {
        let found = stream_format(part)?;
        if found != expected {
            return Err(End::Failed(format!(
                "the export pieces do not share one format (piece 1: {expected}; piece {}: {found}), so they cannot be joined",
                index + 1
            )));
        }
    }
    Ok(())
}

pub(super) fn join_segments(
    dir: &Path,
    parts: &[PathBuf],
    output: &Path,
    hevc: bool,
) -> Result<(), End> {
    if parts.is_empty() {
        return Err(End::Failed(
            "there are no export pieces to join".to_string(),
        ));
    }
    check_formats(parts)?;
    let list = dir.join("concat.txt");
    let mut text = String::new();
    for part in parts {
        text.push_str("file '");
        text.push_str(&concat_path(part));
        text.push_str("'\n");
    }
    fs::write(&list, text)
        .map_err(|error| End::Failed(format!("could not write the join list: {error}")))?;
    let mut command = crate::utils::hidden_command(ffmpeg::ffmpeg_executable());
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-y",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(&list)
        .args(["-c", "copy", "-movflags", "+faststart"]);
    if hevc {
        command.args(["-tag:v", "hvc1"]);
    }
    let result = command
        .arg(output)
        .output()
        .map_err(|error| End::Failed(format!("could not start the joiner: {error}")))?;
    if !result.status.success() {
        let detail = String::from_utf8_lossy(&result.stderr);
        return Err(End::Failed(format!(
            "could not join the export pieces: {}",
            detail.trim()
        )));
    }
    let _ = fs::remove_file(&list);
    Ok(())
}

/// The concat demuxer takes forward slashes and quotes a single quote with a
/// backslash.
fn concat_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_cover_every_frame_once() {
        for (fps, total) in [(30.0, 240), (30.0, 241), (60.0, 1000), (29.97, 7)] {
            let chunk = chunk_frames(fps, total);
            let pieces = total.div_ceil(chunk) as usize;
            let covered: u64 = (0..pieces)
                .map(|piece| frames_in(total, chunk, piece))
                .sum();
            assert_eq!(covered, total, "fps {fps}, total {total}");
            assert!(chunk >= 1);
        }
    }

    #[test]
    fn long_exports_cap_the_piece_count() {
        let total = 30 * 60 * 60;
        let chunk = chunk_frames(30.0, total);
        assert!(total.div_ceil(chunk) <= MAX_CHUNKS);
        assert!(chunk >= 30 * 4);
    }

    #[test]
    fn the_first_piece_uses_the_primary_encoder() {
        let mut loads = [Load::default(); 2];
        assert_eq!(choose(&loads, 0), 0);
        // With no measurements yet, an idle encoder wins over a busy one.
        loads[0].pending = 120;
        assert_eq!(choose(&loads, 1), 1);
        loads[1].pending = 120;
        loads[0].ms_per_frame = 1.0;
        loads[1].ms_per_frame = 2.0;
        assert_eq!(choose(&loads, 2), 0);
    }

    #[test]
    fn the_concat_list_uses_forward_slashes_and_escapes_quotes() {
        assert_eq!(
            concat_path(Path::new(r"C:\Videos\it's here\piece.mp4")),
            "C:/Videos/it\\'s here/piece.mp4"
        );
    }

    #[test]
    #[ignore = "integration: shells out to ffmpeg"]
    fn a_segmented_export_joins_every_frame() {
        let available = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !available {
            eprintln!("skipping: ffmpeg is not available");
            return;
        }
        let dir = std::env::temp_dir().join(format!("apexshot-hybrid-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let output = dir.join("out.mp4");
        let size = (320, 180);
        let total: u64 = 240;
        let primary = if Encoder::QuickSync.opens() {
            Encoder::QuickSync
        } else {
            Encoder::X264
        };
        let mut sink = SegmentedSink::new(primary, &output, size, 30.0, 28, total).unwrap();
        for index in 0..total {
            let mut frame = sink.buffer(index).unwrap();
            frame.fill(16);
            sink.submit(frame).unwrap();
        }
        sink.finish().unwrap();
        let probe = crate::utils::hidden_command(ffmpeg::ffprobe_executable())
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-count_frames",
                "-show_entries",
                "stream=nb_read_frames",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
            ])
            .arg(&output)
            .output()
            .unwrap();
        assert!(probe.status.success());
        let frames = String::from_utf8_lossy(&probe.stdout).trim().to_string();
        assert_eq!(frames, total.to_string());
        let _ = fs::remove_dir_all(&dir);
    }

    fn make_clip(dir: &Path, name: &str, profile: &str) -> PathBuf {
        let path = dir.join(name);
        let status = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=30:duration=1",
                "-c:v",
                "libx264",
                "-profile:v",
                profile,
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success(), "ffmpeg could not make {name}");
        path
    }

    fn ffmpeg_available() -> bool {
        crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn matching_pieces_join() {
        if !ffmpeg_available() {
            eprintln!("skipping: ffmpeg is not available");
            return;
        }
        let dir = std::env::temp_dir().join(format!("apexshot-join-ok-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let a = make_clip(&dir, "a.mp4", "high");
        let b = make_clip(&dir, "b.mp4", "high");
        let joined = dir.join("joined.mp4");
        assert!(join_segments(&dir, &[a, b], &joined, false).is_ok());
        assert!(joined.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mismatched_pieces_fail_with_a_clear_message() {
        if !ffmpeg_available() {
            eprintln!("skipping: ffmpeg is not available");
            return;
        }
        let dir = std::env::temp_dir().join(format!("apexshot-join-bad-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let a = make_clip(&dir, "a.mp4", "high");
        let b = make_clip(&dir, "b.mp4", "main");
        let joined = dir.join("joined.mp4");
        match join_segments(&dir, &[a, b], &joined, false) {
            Err(End::Failed(message)) => {
                assert!(message.contains("do not share one format"), "{message}")
            }
            _ => panic!("expected a format failure"),
        }
        assert!(!joined.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
