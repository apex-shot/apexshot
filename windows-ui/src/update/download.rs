//! Streaming installer download with progress and cancellation.
//!
//! The worker is a plain thread because `ureq` blocks. The window drains its
//! events on the GTK main loop and never waits on the network itself.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};

const USER_AGENT: &str = concat!("ApexShot/", env!("CARGO_PKG_VERSION"));
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const REPORT_INTERVAL: Duration = Duration::from_millis(100);
const BUFFER_SIZE: usize = 64 * 1024;

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

    std::thread::spawn(move || {
        match download(&url, &worker_destination, &partial, &cancel, &sender) {
            Ok(()) => {
                let _ = sender.send_blocking(Event::Finished(worker_destination));
            }
            Err(DownloadError::Cancelled) => {
                let _ = fs::remove_file(&partial);
                let _ = sender.send_blocking(Event::Cancelled);
            }
            Err(DownloadError::Failed(message)) => {
                let _ = fs::remove_file(&partial);
                let _ = sender.send_blocking(Event::Failed(message));
            }
        }
    });

    Ok((receiver, destination))
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

fn download(
    url: &str,
    destination: &Path,
    partial: &Path,
    cancel: &AtomicBool,
    sender: &Sender<Event>,
) -> Result<(), DownloadError> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .user_agent(USER_AGENT)
        .build();

    let response = agent
        .get(url)
        .call()
        .map_err(|error| DownloadError::Failed(describe(error)))?;
    let total = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0);

    let file = File::create(partial)
        .map_err(|error| DownloadError::Failed(format!("Could not save the update: {error}")))?;
    let mut file = BufWriter::with_capacity(BUFFER_SIZE, file);
    let mut reader = response.into_reader();
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let mut received = 0u64;
    let mut last_report = Instant::now();
    let mut last_report_bytes = 0u64;

    // The first event carries the total as soon as the headers arrive, so the
    // card can show the file size before the first chunk lands.
    let _ = sender.try_send(Event::Progress {
        received: 0,
        total,
        bytes_per_second: 0.0,
    });

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|error| DownloadError::Failed(format!("The download stopped: {error}")))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|error| {
            DownloadError::Failed(format!("Could not save the update: {error}"))
        })?;
        received += read as u64;

        let now = Instant::now();
        let elapsed = now.duration_since(last_report);
        if elapsed >= REPORT_INTERVAL {
            let bytes_per_second =
                (received - last_report_bytes) as f64 / elapsed.as_secs_f64().max(0.001);
            last_report = now;
            last_report_bytes = received;
            let _ = sender.try_send(Event::Progress {
                received,
                total,
                bytes_per_second,
            });
        }
    }

    file.flush()
        .map_err(|error| DownloadError::Failed(format!("Could not save the update: {error}")))?;
    drop(file);

    if received == 0 {
        return Err(DownloadError::Failed(
            "The server sent an empty download.".to_string(),
        ));
    }

    // `rename` replaces on Unix; Windows wants the destination gone first.
    let _ = fs::remove_file(destination);
    fs::rename(partial, destination)
        .map_err(|error| DownloadError::Failed(format!("Could not finish the update: {error}")))?;
    Ok(())
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
}
