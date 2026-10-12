//! Thumbnail generation for the History window.
//!
//! Decoding happens on a small pool of background threads and finished images
//! are reported back through an `mpsc` channel the UI thread drains, so a folder
//! full of large captures never blocks the grid. Generated thumbnails are cached
//! on disk keyed to the source file, which makes reopening the window fast.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::SystemTime;

use super::scan::{CaptureEntry, MediaKind};

/// Card image size. Thumbnails are baked to exactly these pixels so GTK 4.6,
/// which has no `content-fit`, still renders an evenly cropped grid.
pub const THUMB_WIDTH: u32 = 260;
pub const THUMB_HEIGHT: u32 = 150;

/// Number of decode threads. Enough to keep a grid filling quickly without
/// starving the main loop of CPU on modest machines.
const WORKER_THREADS: usize = 3;

/// Where generated thumbnails live, following the cache layout the recording
/// editor and the cloud thumbnail cache already use.
pub fn cache_dir() -> PathBuf {
    let mut dir = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
    dir.push("apexshot");
    dir.push("history-thumbnails");
    dir
}

fn fnv1a_hex(value: &str) -> String {
    let mut hash = 1469598103934665603_u64;
    for byte in value.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("{hash:x}")
}

/// Cache filename for a capture. Includes modification time and size so an
/// edited or replaced file re-renders instead of showing a stale image.
pub fn cache_key(path: &Path, modified: Option<SystemTime>, size_bytes: u64) -> String {
    let stamp = modified
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!(
        "{}-{stamp}-{size_bytes}.png",
        fnv1a_hex(&path.to_string_lossy())
    )
}

fn cache_path_for(entry: &CaptureEntry) -> PathBuf {
    cache_dir().join(cache_key(&entry.path, entry.modified, entry.size_bytes))
}

/// Produce (or reuse) the on-disk thumbnail for a local capture.
///
/// Blocking: only call this from a worker thread.
pub fn thumbnail_for_entry(entry: &CaptureEntry) -> Result<PathBuf, String> {
    let cached = cache_path_for(entry);
    if cached.is_file() {
        return Ok(cached);
    }

    let source = match entry.kind {
        // Animated GIFs decode to their first frame with the image crate, so
        // only real video containers need to go through ffmpeg.
        MediaKind::Image => PosterSource::Direct(entry.path.clone()),
        MediaKind::Video if is_gif(&entry.path) => PosterSource::Direct(entry.path.clone()),
        MediaKind::Video => PosterSource::Extracted(extract_video_poster(&entry.path)?),
    };

    let decode_path = match &source {
        PosterSource::Direct(path) => path.clone(),
        PosterSource::Extracted(path) => path.clone(),
    };

    let result = write_scaled_thumbnail(&decode_path, &cached);

    if let PosterSource::Extracted(temp) = source {
        let _ = std::fs::remove_file(&temp);
        if let Some(parent) = temp.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }

    result.map(|_| cached)
}

enum PosterSource {
    Direct(PathBuf),
    Extracted(PathBuf),
}

fn is_gif(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("gif"))
        .unwrap_or(false)
}

/// Pull a poster frame out of a recording with the recording editor's ffmpeg
/// helper, into that module's cache directory convention.
fn extract_video_poster(path: &Path) -> Result<PathBuf, String> {
    let dir = crate::recording::editor::ffmpeg::thumbnail_cache_dir(path);
    let poster = dir.join("history-poster.png");

    // One second in usually beats frame zero (fades, black leader). Fall back to
    // the very first frame for clips shorter than that.
    if crate::recording::editor::ffmpeg::extract_poster_frame(path, &poster, 1.0).is_ok() {
        return Ok(poster);
    }
    crate::recording::editor::ffmpeg::extract_poster_frame(path, &poster, 0.0)
        .map(|_| poster)
        .map_err(|e| format!("Could not read a frame from this recording: {e}"))
}

/// Decode `source`, crop-scale it to the card size and store it at `target`.
///
/// The write goes to a per-call temp file first so two workers racing on the
/// same cache entry never leave a half-written PNG behind.
fn write_scaled_thumbnail(source: &Path, target: &Path) -> Result<(), String> {
    let image = image::open(source).map_err(|e| format!("Could not decode this file: {e}"))?;
    let scaled = image.resize_to_fill(
        THUMB_WIDTH,
        THUMB_HEIGHT,
        image::imageops::FilterType::Triangle,
    );

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create the thumbnail cache: {e}"))?;
    }

    // Unique per call: two workers racing the same cache entry must not share
    // a temp path (the counter is process-unique, the pid keeps cross-process
    // names apart).
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    let temp = target.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    // The temp name ends in `.tmp`, which `save` cannot map to an encoder, so
    // write PNG explicitly through the shared encoder.
    crate::utils::png_io::save_image(&temp, &scaled)
        .map_err(|e| format!("Could not write the thumbnail: {e}"))?;
    std::fs::rename(&temp, target).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("Could not store the thumbnail: {e}")
    })
}

/// What a job should turn into an image.
#[derive(Debug, Clone)]
pub enum ThumbnailSource {
    /// A capture on disk.
    Local(CaptureEntry),
    /// A cloud upload's remote thumbnail URL.
    Remote(String),
}

/// A queued thumbnail job.
pub struct ThumbnailRequest {
    /// Card identifier, echoed back so the UI can find the right widget.
    pub id: u64,
    /// Batch identifier; a refresh bumps it so stale results are dropped.
    pub generation: u64,
    pub source: ThumbnailSource,
    pub reply: Sender<ThumbnailReady>,
}

/// A finished thumbnail job.
#[derive(Debug)]
pub struct ThumbnailReady {
    pub id: u64,
    pub generation: u64,
    pub result: Result<PathBuf, String>,
}

static GENERATION: AtomicU64 = AtomicU64::new(1);

/// A fresh batch identifier. Unique across every page in the window.
pub fn next_generation() -> u64 {
    GENERATION.fetch_add(1, Ordering::Relaxed)
}

struct PoolInner {
    jobs: VecDeque<ThumbnailRequest>,
    cancelled: HashSet<u64>,
}

struct Pool {
    inner: Mutex<PoolInner>,
    wake: Condvar,
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    let pool = POOL.get_or_init(|| Pool {
        inner: Mutex::new(PoolInner {
            jobs: VecDeque::new(),
            cancelled: HashSet::new(),
        }),
        wake: Condvar::new(),
    });

    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        for index in 0..WORKER_THREADS {
            std::thread::Builder::new()
                .name(format!("apexshot-history-thumb-{index}"))
                .spawn(move || worker_loop(pool))
                .ok();
        }
    });

    pool
}

fn worker_loop(pool: &'static Pool) {
    loop {
        let request = {
            let mut inner = pool.inner.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                match inner.jobs.pop_front() {
                    Some(request) => {
                        if inner.cancelled.contains(&request.generation) {
                            // Superseded by a refresh: skip without decoding.
                            continue;
                        }
                        break request;
                    }
                    None => {
                        inner = pool.wake.wait(inner).unwrap_or_else(|e| e.into_inner());
                    }
                }
            }
        };

        let result = match &request.source {
            ThumbnailSource::Local(entry) => thumbnail_for_entry(entry),
            ThumbnailSource::Remote(url) => crate::cloud::listing::cached_thumbnail(url)
                .map_err(|e| format!("Could not load this thumbnail: {e}")),
        };

        // A closed receiver just means the window went away.
        let _ = request.reply.send(ThumbnailReady {
            id: request.id,
            generation: request.generation,
            result,
        });
    }
}

/// Queue a thumbnail job on the shared worker pool.
pub fn submit(request: ThumbnailRequest) {
    let pool = pool();
    let mut inner = pool.inner.lock().unwrap_or_else(|e| e.into_inner());
    inner.jobs.push_back(request);
    drop(inner);
    pool.wake.notify_one();
}

/// Drop everything still queued for `generation`. In-flight jobs finish but the
/// UI ignores their results.
pub fn cancel_generation(generation: u64) {
    let pool = pool();
    let mut inner = pool.inner.lock().unwrap_or_else(|e| e.into_inner());
    inner.jobs.retain(|job| job.generation != generation);
    inner.cancelled.insert(generation);
    // Keep the cancelled set from growing without bound over a long session.
    if inner.cancelled.len() > 256 {
        let keep: HashSet<u64> = inner
            .cancelled
            .iter()
            .copied()
            .filter(|gen| *gen + 64 >= generation)
            .collect();
        inner.cancelled = keep;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn entry(path: &str, modified: Option<SystemTime>, size: u64) -> CaptureEntry {
        CaptureEntry {
            path: PathBuf::from(path),
            display_name: path.rsplit('/').next().unwrap_or(path).to_string(),
            modified,
            size_bytes: size,
            kind: MediaKind::Image,
        }
    }

    #[test]
    fn cache_keys_are_stable_for_the_same_file() {
        let when = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000));
        let a = entry("/home/user/Pictures/shot.png", when, 42);
        assert_eq!(cache_path_for(&a), cache_path_for(&a));
        assert!(cache_path_for(&a).starts_with(cache_dir()));
        assert!(cache_path_for(&a)
            .extension()
            .is_some_and(|ext| ext == "png"));
    }

    #[test]
    fn cache_keys_change_when_the_source_changes() {
        let when = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000));
        let later = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(2_000));

        let base = cache_key(Path::new("/shots/a.png"), when, 42);
        assert_ne!(base, cache_key(Path::new("/shots/b.png"), when, 42));
        assert_ne!(base, cache_key(Path::new("/shots/a.png"), later, 42));
        assert_ne!(base, cache_key(Path::new("/shots/a.png"), when, 43));
        // A file with no reported timestamp still gets a usable key.
        assert!(cache_key(Path::new("/shots/a.png"), None, 42).ends_with("-0-42.png"));
    }

    #[test]
    fn generations_are_unique() {
        let first = next_generation();
        let second = next_generation();
        assert_ne!(first, second);
    }

    #[test]
    fn cancelled_generations_never_reach_a_worker() {
        let (tx, rx) = std::sync::mpsc::channel();
        let generation = next_generation();
        cancel_generation(generation);
        submit(ThumbnailRequest {
            id: 1,
            generation,
            // A path that cannot exist: if the job ran anyway a result would
            // still arrive, which is what this test rules out.
            source: ThumbnailSource::Local(entry(
                "/nonexistent/apexshot-history-cancelled.png",
                None,
                0,
            )),
            reply: tx,
        });

        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }

    #[test]
    fn live_generations_do_reach_a_worker() {
        let (tx, rx) = std::sync::mpsc::channel();
        let generation = next_generation();
        submit(ThumbnailRequest {
            id: 7,
            generation,
            source: ThumbnailSource::Local(entry(
                "/nonexistent/apexshot-history-live.png",
                None,
                0,
            )),
            reply: tx,
        });

        let ready = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("worker reported a result");
        assert_eq!(ready.id, 7);
        assert_eq!(ready.generation, generation);
        assert!(ready.result.is_err());
    }

    #[test]
    fn valid_images_produce_a_cached_thumbnail() {
        // Regression: the cache write used to fail for every file because the
        // temp path's `.tmp` extension gave `save` no format to infer.
        let dir = std::env::temp_dir().join(format!(
            "apexshot-history-thumb-valid-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let path = dir.join("valid.png");

        // A real decodable image, larger than the card so the scale path runs.
        image::DynamicImage::new_rgb8(640, 360)
            .save(&path)
            .expect("write source image");
        let size = std::fs::metadata(&path).expect("source metadata").len();

        let valid = CaptureEntry {
            path: path.clone(),
            display_name: "valid.png".to_string(),
            modified: None,
            size_bytes: size,
            kind: MediaKind::Image,
        };

        let thumb = thumbnail_for_entry(&valid).expect("thumbnail renders");
        assert!(thumb.is_file());
        let decoded = image::open(&thumb).expect("cached thumbnail decodes");
        assert_eq!(decoded.width(), THUMB_WIDTH);
        assert_eq!(decoded.height(), THUMB_HEIGHT);

        let _ = std::fs::remove_file(&thumb);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undecodable_files_report_an_error_instead_of_panicking() {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-history-thumb-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let path = dir.join("broken.png");
        std::fs::write(&path, b"not really a png").expect("write file");

        let broken = CaptureEntry {
            path: path.clone(),
            display_name: "broken.png".to_string(),
            modified: None,
            size_bytes: 16,
            kind: MediaKind::Image,
        };
        assert!(thumbnail_for_entry(&broken).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
