//! Streaming update download with progress, cancellation, and verification.
//!
//! The worker is a plain thread because `ureq` blocks. The window drains its
//! events on the GTK main loop and never waits on the network itself. The
//! package drives the progress numbers; the checksum manifest and the small
//! extension archive are fetched after it and only switch the status line.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use sha2::{Digest, Sha256};

use super::release::{self, AssetError, ReleaseAsset};
use crate::i18n::{t, tfmt};

const USER_AGENT: &str = concat!("ApexShot/", env!("CARGO_PKG_VERSION"));
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const REPORT_INTERVAL: Duration = Duration::from_millis(100);
const BUFFER_SIZE: usize = 64 * 1024;
/// A checksum manifest is a few hundred bytes; refuse anything that is not.
const MAX_CHECKSUMS_BYTES: u64 = 64 * 1024;

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
    /// The package is on disk and the checksum manifest is being applied.
    Verifying,
    /// Everything the install needs is downloaded and verified.
    Finished(Downloaded),
    /// The user asked to stop; any partial file has been removed.
    Cancelled,
    /// The download could not finish. The message is safe to show.
    Failed(String),
}

/// The files a successful download leaves behind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Downloaded {
    /// The verified package.
    pub package: PathBuf,
    /// Its file name, as shown in the fact card.
    pub package_name: String,
    /// The verified GNOME extension archive, when one was needed.
    pub extension: Option<PathBuf>,
}

/// The files this download has to fetch.
#[derive(Clone, Debug)]
pub struct DownloadRequest {
    pub package: ReleaseAsset,
    pub checksums: ReleaseAsset,
    pub extension: Option<ReleaseAsset>,
}

/// Start downloading a release into the app's update cache.
pub fn start(request: DownloadRequest, cancel: Arc<AtomicBool>) -> Result<Receiver<Event>, String> {
    let directory = update_directory()
        .ok_or_else(|| t("Could not find a local cache folder for the update."))?;
    fs::create_dir_all(&directory).map_err(|error| {
        tfmt(
            "Could not create the update folder: {error}",
            &[("error", &error.to_string())],
        )
    })?;

    let (sender, receiver) = async_channel::bounded(64);
    std::thread::spawn(move || match run(&request, &directory, &cancel, &sender) {
        Ok(downloaded) => {
            let _ = sender.send_blocking(Event::Finished(downloaded));
        }
        Err(DownloadError::Cancelled) => {
            let _ = sender.send_blocking(Event::Cancelled);
        }
        Err(DownloadError::Failed(message)) => {
            let _ = sender.send_blocking(Event::Failed(message));
        }
    });

    Ok(receiver)
}

/// `~/.cache/apexshot/updates`; the package replaces the installed binary in
/// place, so the download outlives the window that started it.
pub(super) fn update_directory() -> Option<PathBuf> {
    Some(
        gtk4::glib::user_cache_dir()
            .join("apexshot")
            .join("updates"),
    )
}

fn run(
    request: &DownloadRequest,
    directory: &Path,
    cancel: &AtomicBool,
    sender: &Sender<Event>,
) -> Result<Downloaded, DownloadError> {
    let package_name = if request.package.name.trim().is_empty() {
        file_name_from_url(&request.package.browser_download_url)
    } else {
        sanitize_file_name(&request.package.name)
    };
    let package_path = directory.join(&package_name);
    let partial = partial_path(&package_path);
    let package_digest = download_file(
        &request.package.browser_download_url,
        &package_path,
        &partial,
        cancel,
        Some(sender),
    )?;

    let _ = sender.send_blocking(Event::Verifying);
    let checksums_text = fetch_text(&request.checksums.browser_download_url, MAX_CHECKSUMS_BYTES)?;
    let sums = release::parse_sha256sums(&checksums_text);
    if let Err(error) = release::verify_digest(&package_digest, &request.package.name, &sums) {
        let _ = fs::remove_file(&package_path);
        return Err(DownloadError::Failed(error.message()));
    }

    // The extension archive only matters when the user already has the
    // extension: refreshing must not install it for someone who skipped it.
    let extension = match &request.extension {
        Some(extension) if crate::gnome_extension::is_installed() => {
            if cancel.load(Ordering::Relaxed) {
                return Err(DownloadError::Cancelled);
            }
            let extension_name = sanitize_file_name(&extension.name);
            let extension_path = directory.join(extension_name);
            let extension_partial = partial_path(&extension_path);
            let extension_digest = download_file(
                &extension.browser_download_url,
                &extension_path,
                &extension_partial,
                cancel,
                None,
            )?;
            if let Err(error) = release::verify_digest(&extension_digest, &extension.name, &sums) {
                let _ = fs::remove_file(&extension_path);
                return Err(DownloadError::Failed(error.message()));
            }
            Some(extension_path)
        }
        _ => None,
    };

    Ok(Downloaded {
        package: package_path,
        package_name,
        extension,
    })
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

/// A user-facing failure whose text keeps the operating system's own reason.
fn failed_with_error(message: &str, error: impl std::fmt::Display) -> DownloadError {
    DownloadError::Failed(tfmt(message, &[("error", &error.to_string())]))
}

fn download_file(
    url: &str,
    destination: &Path,
    partial: &Path,
    cancel: &AtomicBool,
    sender: Option<&Sender<Event>>,
) -> Result<String, DownloadError> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .user_agent(USER_AGENT)
        .build();

    let response = agent
        .get(url)
        .call()
        .map_err(|error| DownloadError::Failed(release::describe_request_error(error)))?;
    let total = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0);

    let file = File::create(partial)
        .map_err(|error| failed_with_error("Could not save the update: {error}", error))?;
    let mut file = BufWriter::with_capacity(BUFFER_SIZE, file);
    let mut reader = response.into_reader();
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let mut received = 0u64;
    let mut last_report = Instant::now();
    let mut last_report_bytes = 0u64;
    // Hash while writing so the checksum is ready the moment the transfer
    // ends, instead of reading the whole package back from disk.
    let mut hasher = Sha256::new();

    // The first event carries the total as soon as the headers arrive, so the
    // card can show the file size before the first chunk lands.
    if let Some(sender) = sender {
        let _ = sender.try_send(Event::Progress {
            received: 0,
            total,
            bytes_per_second: 0.0,
        });
    }

    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = fs::remove_file(partial);
            return Err(DownloadError::Cancelled);
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|error| failed_with_error("The download stopped: {error}", error))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|error| failed_with_error("Could not save the update: {error}", error))?;
        hasher.update(&buffer[..read]);
        received += read as u64;

        let now = Instant::now();
        let elapsed = now.duration_since(last_report);
        if elapsed >= REPORT_INTERVAL {
            let bytes_per_second =
                (received - last_report_bytes) as f64 / elapsed.as_secs_f64().max(0.001);
            last_report = now;
            last_report_bytes = received;
            if let Some(sender) = sender {
                let _ = sender.try_send(Event::Progress {
                    received,
                    total,
                    bytes_per_second,
                });
            }
        }
    }

    file.flush()
        .map_err(|error| failed_with_error("Could not save the update: {error}", error))?;
    drop(file);

    // Reports are throttled, so the last one can land short of the end. Close
    // on the true count so the card reads 100% before verification starts.
    if let Some(sender) = sender {
        let _ = sender.send_blocking(Event::Progress {
            received,
            total: total.or(Some(received)),
            bytes_per_second: 0.0,
        });
    }

    if received == 0 {
        let _ = fs::remove_file(partial);
        return Err(DownloadError::Failed(t(
            "The server sent an empty download.",
        )));
    }

    let _ = fs::remove_file(destination);
    fs::rename(partial, destination)
        .map_err(|error| failed_with_error("Could not finish the update: {error}", error))?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn fetch_text(url: &str, max_bytes: u64) -> Result<String, DownloadError> {
    let response = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .get(url)
        .call()
        .map_err(|error| DownloadError::Failed(release::describe_request_error(error)))?;
    let mut text = String::new();
    response
        .into_reader()
        .take(max_bytes + 1)
        .read_to_string(&mut text)
        .map_err(|error| failed_with_error("Could not read the checksums: {error}", error))?;
    if text.len() as u64 > max_bytes {
        return Err(DownloadError::Failed(AssetError::NoChecksums.message()));
    }
    Ok(text)
}

/// The asset name at the end of a URL, with any query or fragment removed.
pub fn file_name_from_url(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.rsplit('/').next().unwrap_or_default();
    sanitize_file_name(name)
}

/// Keep a release asset name usable as a file name. Release names are plain
/// already; this only guards against path separators and control characters.
fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|character| {
            if character.is_control() || matches!(character, '/' | '\\' | ':') {
                '_'
            } else {
                character
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        "apexshot-update".to_string()
    } else {
        cleaned
    }
}

/// Decimal units, matching the release API's byte counts and the file sizes
/// the package manager reports.
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
            file_name_from_url("https://github.test/apexshot_0.2.36_amd64.deb"),
            "apexshot_0.2.36_amd64.deb"
        );
        assert_eq!(
            file_name_from_url("https://github.test/a/apexshot_0.2.36_amd64.deb?token=1#notes"),
            "apexshot_0.2.36_amd64.deb"
        );
        assert_eq!(
            file_name_from_url("https://github.test/release/"),
            "apexshot-update"
        );
        assert_eq!(
            file_name_from_url("https://github.test/a/.."),
            "apexshot-update"
        );
    }

    #[test]
    fn asset_names_drop_path_separators() {
        assert_eq!(
            file_name_from_url("https://github.test/a/apexshot:0.2.36*.deb"),
            "apexshot_0.2.36*.deb"
        );
        assert_eq!(sanitize_file_name("a/b\\c"), "a_b_c");
    }

    #[test]
    fn the_partial_file_sits_beside_the_final_name() {
        let destination = Path::new("/home/user/.cache/apexshot/updates/apexshot_0.2.36_amd64.deb");
        assert_eq!(
            partial_path(destination),
            Path::new("/home/user/.cache/apexshot/updates/apexshot_0.2.36_amd64.deb.part")
        );
    }

    #[test]
    fn the_update_folder_is_in_the_app_cache() {
        // `g_get_user_cache_dir` degrades to a relative path on a host with no
        // home directory; the suffix is what this test cares about.
        if let Some(directory) = update_directory() {
            assert!(directory.ends_with("apexshot/updates"));
        }
    }
}
