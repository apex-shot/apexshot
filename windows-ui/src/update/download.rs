//! Streaming installer download with progress and cancellation.
//!
//! The worker is a plain thread because `ureq` blocks. The window drains its
//! events on the GTK main loop and never waits on the network itself.

use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};

use super::telemetry;

const USER_AGENT: &str = concat!("ApexShot/", env!("CARGO_PKG_VERSION"));
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const REPORT_INTERVAL: Duration = Duration::from_millis(100);
const BUFFER_SIZE: usize = 64 * 1024;
/// Tries per source before falling back or giving up; each retry resumes.
const ATTEMPTS_PER_SOURCE: u32 = 5;
/// The speed shown is an average over roughly this long, so it reads steadily.
const SPEED_WINDOW: Duration = Duration::from_secs(2);

/// One update to the download view.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// `received` bytes of `total` are on disk. `total` stays `None` when the
    /// server does not send a length. `bytes_per_second` measures the window
    /// since the previous report.
    Progress {
        received: u64,
        total: Option<u64>,
        bytes_per_second: f64,
    },
    /// The complete installer is at this path.
    Finished(PathBuf),
    /// The user asked to stop; any partial file has been removed.
    Cancelled,
    /// The download could not finish. The message is safe to show.
    Failed(String),
}

/// Start downloading `url` into the app's update cache.
///
/// Returns the channel the caller drains and the path the installer will have
/// once it is complete.
pub fn start(
    url: &str,
    file_name: &str,
    cancel: Arc<AtomicBool>,
) -> Result<(Receiver<Event>, PathBuf), String> {
    let directory = update_directory()
        .ok_or_else(|| "Could not find a local app-data folder for the update.".to_string())?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Could not create the update folder: {error}"))?;

    let destination = directory.join(sanitize_file_name(file_name));
    let partial = partial_path(&destination);
    let (sender, receiver) = async_channel::bounded(64);
    let url = url.to_string();
    let worker_destination = destination.clone();
    let telemetry = telemetry::Installer::new(&url, file_name);

    std::thread::spawn(move || {
        telemetry.started();
        // A stale partial from an earlier run is never trusted.
        let _ = fs::remove_file(&partial);
        let mut progress = Progress::default();
        // The mirror is tried first; GitHub stays the source of truth. A mirror
        // that fails hands over to GitHub, which resumes from the bytes already
        // on disk when it serves the same file.
        let mut result = Err(DownloadError::Failed(String::new()));
        if let Some(mirror) = mirror_url(&url) {
            result = download_with_retries(&mirror, &partial, &cancel, &sender, &mut progress);
        }
        if matches!(result, Err(DownloadError::Failed(_))) {
            result = download_with_retries(&url, &partial, &cancel, &sender, &mut progress);
        }
        if result.is_ok() {
            result = finish(&partial, &worker_destination, &progress);
        }
        match result {
            Ok(()) => {
                let size = fs::metadata(&worker_destination).map_or(0, |metadata| metadata.len());
                telemetry.completed(size);
                let _ = sender.send_blocking(Event::Finished(worker_destination));
            }
            Err(DownloadError::Cancelled) => {
                telemetry.failed("cancelled");
                let _ = fs::remove_file(&partial);
                let _ = sender.send_blocking(Event::Cancelled);
            }
            Err(DownloadError::Failed(message)) => {
                telemetry.failed("error");
                let _ = fs::remove_file(&partial);
                let _ = sender.send_blocking(Event::Failed(message));
            }
        }
    });

    Ok((receiver, destination))
}

const GITHUB_RELEASE_DOWNLOADS: &str = "https://github.com/apex-shot/apexshot/releases/download/";
/// The Cloudflare mirror holds the latest releases at the same `<tag>/<name>`
/// paths as GitHub's release downloads.
const RELEASE_MIRROR: &str = "https://dl.apexshot.org/";

/// The mirror's copy of a GitHub release asset, if `url` is one.
fn mirror_url(url: &str) -> Option<String> {
    let path = url.strip_prefix(GITHUB_RELEASE_DOWNLOADS)?;
    (!path.is_empty()).then(|| format!("{RELEASE_MIRROR}{path}"))
}

/// `%LOCALAPPDATA%\apexshot\updates` on Windows, the platform cache directory
/// elsewhere.
fn update_directory() -> Option<PathBuf> {
    dirs::cache_dir().map(|directory| directory.join("apexshot").join("updates"))
}

fn partial_path(destination: &Path) -> PathBuf {
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("update");
    destination.with_file_name(format!("{name}.part"))
}

enum DownloadError {
    Cancelled,
    Failed(String),
}

/// What a download has on disk so far, carried across retries and sources.
#[derive(Default)]
struct Progress {
    received: u64,
    total: Option<u64>,
    bytes_per_second: f64,
}

/// How one attempt ended.
enum Attempt {
    Cancelled,
    /// Worth trying again: the connection dropped, stalled, or the server hiccuped.
    Retry(String),
    /// Trying this source again will not help (missing file, disk error).
    Fatal(String),
}

fn download_with_retries(
    url: &str,
    partial: &Path,
    cancel: &AtomicBool,
    sender: &Sender<Event>,
    progress: &mut Progress,
) -> Result<(), DownloadError> {
    let mut last_error = String::new();
    for attempt in 0..ATTEMPTS_PER_SOURCE {
        if attempt > 0 && !wait_before_retry(attempt, cancel) {
            return Err(DownloadError::Cancelled);
        }
        match download_attempt(url, partial, cancel, sender, progress) {
            Ok(()) => return Ok(()),
            Err(Attempt::Cancelled) => return Err(DownloadError::Cancelled),
            Err(Attempt::Retry(message)) => last_error = message,
            Err(Attempt::Fatal(message)) => return Err(DownloadError::Failed(message)),
        }
    }
    Err(DownloadError::Failed(last_error))
}

/// Back off 1, 2, 4, 8 seconds, checking for a cancel every 100 ms. Returns
/// false when the user cancelled while waiting.
fn wait_before_retry(attempt: u32, cancel: &AtomicBool) -> bool {
    let wait = Duration::from_secs(1u64 << (attempt - 1).min(3));
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !cancel.load(Ordering::Relaxed)
}

/// 408, 429 and 5xx are passing server trouble; any other status is final for
/// this source.
fn is_retryable_status(code: u16) -> bool {
    code == 408 || code == 429 || code >= 500
}

/// The full size from a `Content-Range: bytes 100-199/200` header.
fn content_range_total(header: Option<&str>) -> Option<u64> {
    header?
        .rsplit('/')
        .next()?
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|total| *total > 0)
}

/// An exponential average whose weight follows the time since the last
/// sample, so the shown speed settles over about `SPEED_WINDOW`.
fn smooth_rate(previous: f64, instant: f64, elapsed: Duration) -> f64 {
    if previous <= 0.0 {
        return instant;
    }
    let weight = 1.0 - (-elapsed.as_secs_f64() / SPEED_WINDOW.as_secs_f64()).exp();
    previous + (instant - previous) * weight
}

fn download_attempt(
    url: &str,
    partial: &Path,
    cancel: &AtomicBool,
    sender: &Sender<Event>,
    progress: &mut Progress,
) -> Result<(), Attempt> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Attempt::Cancelled);
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .user_agent(USER_AGENT)
        .build();

    let mut request = agent.get(url);
    if progress.received > 0 {
        request = request.set("Range", &format!("bytes={}-", progress.received));
    }
    let response = match request.call() {
        Ok(response) => response,
        // Everything was already on disk when the last attempt dropped.
        Err(ureq::Error::Status(416, _)) if progress.total == Some(progress.received) => {
            return Ok(());
        }
        Err(error @ ureq::Error::Status(code, _)) => {
            let message = describe(error);
            return Err(if is_retryable_status(code) {
                Attempt::Retry(message)
            } else {
                Attempt::Fatal(message)
            });
        }
        Err(error) => return Err(Attempt::Retry(describe(error))),
    };

    let resuming = progress.received > 0 && response.status() == 206;
    let total = if resuming {
        content_range_total(response.header("Content-Range"))
    } else {
        response
            .header("Content-Length")
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
    };
    if resuming && total.is_some() && progress.total.is_some() && total != progress.total {
        // A different file under the same name: start it from the beginning.
        progress.received = 0;
        progress.total = None;
        return Err(Attempt::Retry(
            "The download changed on the server.".to_string(),
        ));
    }
    if !resuming {
        // The server ignored the range or this is the first try.
        progress.received = 0;
    }
    if total.is_some() {
        progress.total = total;
    }

    let file = if resuming {
        OpenOptions::new().append(true).open(partial)
    } else {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(partial)
    }
    .map_err(|error| Attempt::Fatal(format!("Could not save the update: {error}")))?;
    let mut file = BufWriter::with_capacity(BUFFER_SIZE, file);
    let mut reader = response.into_reader();
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let mut last_report = Instant::now();
    let mut last_report_bytes = progress.received;

    // The first event carries the total as soon as the headers arrive, so the
    // card can show the file size before the first chunk lands.
    let _ = sender.try_send(Event::Progress {
        received: progress.received,
        total: progress.total,
        bytes_per_second: progress.bytes_per_second,
    });

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Attempt::Cancelled);
        }
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(error) => {
                // Keep what arrived; the next attempt resumes from the bytes
                // actually on disk.
                drop(file);
                progress.received = fs::metadata(partial).map_or(0, |metadata| metadata.len());
                return Err(Attempt::Retry(format!("The download stopped: {error}")));
            }
        };
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|error| Attempt::Fatal(format!("Could not save the update: {error}")))?;
        progress.received += read as u64;

        let now = Instant::now();
        let elapsed = now.duration_since(last_report);
        if elapsed >= REPORT_INTERVAL {
            let instant =
                (progress.received - last_report_bytes) as f64 / elapsed.as_secs_f64().max(0.001);
            progress.bytes_per_second = smooth_rate(progress.bytes_per_second, instant, elapsed);
            last_report = now;
            last_report_bytes = progress.received;
            let _ = sender.try_send(Event::Progress {
                received: progress.received,
                total: progress.total,
                bytes_per_second: progress.bytes_per_second,
            });
        }
    }

    file.flush()
        .map_err(|error| Attempt::Fatal(format!("Could not save the update: {error}")))?;
    drop(file);

    if let Some(total) = progress.total {
        if progress.received < total {
            return Err(Attempt::Retry("The download ended early.".to_string()));
        }
    }
    Ok(())
}

/// Move the complete download into place once its size checks out.
fn finish(partial: &Path, destination: &Path, progress: &Progress) -> Result<(), DownloadError> {
    let size = fs::metadata(partial).map_or(0, |metadata| metadata.len());
    if size == 0 {
        return Err(DownloadError::Failed(
            "The server sent an empty download.".to_string(),
        ));
    }
    if progress.total.is_some_and(|total| total != size) {
        return Err(DownloadError::Failed(
            "The download is incomplete.".to_string(),
        ));
    }
    // `rename` replaces on Unix; Windows wants the destination gone first.
    let _ = fs::remove_file(destination);
    fs::rename(partial, destination)
        .map_err(|error| DownloadError::Failed(format!("Could not finish the update: {error}")))
}

/// `ureq`'s error text already names the stage; keep it on one line and free
/// of the request URL's query string for the window's error label.
fn describe(error: ureq::Error) -> String {
    let text = error.to_string();
    let text = text.replace('\n', " ");
    if let Some((head, _)) = text.split_once(": https://") {
        format!("{head}.")
    } else {
        text
    }
}

/// The asset name at the end of a URL, with any query or fragment removed and
/// Windows' reserved characters replaced.
pub fn file_name_from_url(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.rsplit('/').next().unwrap_or_default();
    sanitize_file_name(name)
}

fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        "ApexShot-update-setup.exe".to_string()
    } else {
        cleaned
    }
}

/// Decimal units, matching the release API's byte counts and the installer's
/// own size display.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

/// A transfer rate in the same units as the file sizes.
pub fn format_rate(bytes_per_second: f64) -> String {
    format!(
        "{}/s",
        format_bytes(bytes_per_second.max(0.0).round() as u64)
    )
}

/// `16.8%` once the total is known, or the bytes received when it is not.
pub fn progress_text(received: u64, total: Option<u64>) -> String {
    match total.filter(|total| *total > 0) {
        Some(total) => format!(
            "{:.1}%",
            (received as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
        ),
        None => format_bytes(received),
    }
}

/// `24.36 MB / 175.39 MB`, or the received size when the server sends none.
pub fn size_text(received: u64, total: Option<u64>) -> String {
    match total.filter(|total| *total > 0) {
        Some(total) => format!("{} / {}", format_bytes(received), format_bytes(total)),
        None => format_bytes(received),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_assets_are_fetched_from_the_mirror_first() {
        assert_eq!(
            mirror_url(
                "https://github.com/apex-shot/apexshot/releases/download/v0.3.1/ApexShot-0.3.1-setup.exe"
            )
            .as_deref(),
            Some("https://dl.apexshot.org/v0.3.1/ApexShot-0.3.1-setup.exe")
        );
        assert_eq!(
            mirror_url("https://example.test/ApexShot-0.3.1-setup.exe"),
            None
        );
        assert_eq!(
            mirror_url("https://github.com/apex-shot/apexshot/releases/download/"),
            None
        );
    }

    #[test]
    fn sizes_use_decimal_units_and_two_decimals() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_000), "1.00 KB");
        assert_eq!(format_bytes(24_360_000), "24.36 MB");
        assert_eq!(format_bytes(1_500_000_000), "1.50 GB");
    }

    #[test]
    fn rates_and_progress_match_the_size_units() {
        assert_eq!(format_rate(2_990_000.0), "2.99 MB/s");
        assert_eq!(format_rate(0.0), "0 B/s");
        assert_eq!(progress_text(1_680, Some(10_000)), "16.8%");
        assert_eq!(progress_text(500, None), "500 B");
        assert_eq!(progress_text(10_000, Some(10_000)), "100.0%");
        assert_eq!(
            size_text(24_360_000, Some(145_390_000)),
            "24.36 MB / 145.39 MB"
        );
        assert_eq!(size_text(512, None), "512 B");
    }

    #[test]
    fn asset_names_come_from_the_url_path() {
        assert_eq!(
            file_name_from_url("https://github.test/ApexShot-0.2.37-setup.exe"),
            "ApexShot-0.2.37-setup.exe"
        );
        assert_eq!(
            file_name_from_url("https://github.test/a/ApexShot-0.2.37-setup.exe?token=1#notes"),
            "ApexShot-0.2.37-setup.exe"
        );
        assert_eq!(
            file_name_from_url("https://github.test/release/"),
            "ApexShot-update-setup.exe"
        );
        assert_eq!(
            file_name_from_url("https://github.test/a/.."),
            "ApexShot-update-setup.exe"
        );
    }

    #[test]
    fn asset_names_drop_characters_windows_reserves() {
        assert_eq!(
            file_name_from_url("https://github.test/a/ApexShot:0.2.37*.exe"),
            "ApexShot_0.2.37_.exe"
        );
    }

    #[test]
    fn the_partial_file_sits_beside_the_final_name() {
        let destination = Path::new(r"C:\Updates\ApexShot-0.2.37-setup.exe");
        assert_eq!(
            partial_path(destination),
            Path::new(r"C:\Updates\ApexShot-0.2.37-setup.exe.part")
        );
    }

    #[test]
    fn content_range_gives_the_full_size() {
        assert_eq!(content_range_total(Some("bytes 100-199/200")), Some(200));
        assert_eq!(
            content_range_total(Some("bytes 0-0/181576975")),
            Some(181_576_975)
        );
        assert_eq!(content_range_total(Some("bytes 100-199/*")), None);
        assert_eq!(content_range_total(None), None);
    }

    #[test]
    fn passing_server_trouble_is_retried_and_a_missing_file_is_not() {
        assert!(is_retryable_status(503));
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(408));
        assert!(!is_retryable_status(404));
        assert!(!is_retryable_status(403));
    }

    #[test]
    fn the_shown_speed_settles_instead_of_jumping() {
        let tick = Duration::from_millis(100);
        assert_eq!(smooth_rate(0.0, 2_000_000.0, tick), 2_000_000.0);
        // One 100 ms dip to zero barely moves a steady 2 MB/s reading.
        let dipped = smooth_rate(2_000_000.0, 0.0, tick);
        assert!(dipped > 1_850_000.0 && dipped < 2_000_000.0);
        // A sustained change is followed within a few seconds.
        let mut rate = 2_000_000.0;
        for _ in 0..60 {
            rate = smooth_rate(rate, 1_000_000.0, tick);
        }
        assert!((rate - 1_000_000.0).abs() < 60_000.0);
    }
}
