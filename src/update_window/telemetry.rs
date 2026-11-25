//! Download telemetry for the update window, the same events the update
//! scripts send: `download_started`, then `download_completed` or
//! `download_failed`, on the `update` channel.
//!
//! It follows the app's telemetry switch (Settings → Advanced → Privacy, or
//! `APEXSHOT_TELEMETRY=0`), carries the shared anonymous install id, and never
//! delays or fails the download: each event is posted from its own thread with
//! a short timeout.

use std::time::Duration;

use serde::Serialize;

use crate::config::load_config;
use crate::usage_telemetry;

const DEFAULT_URL: &str = "https://apexshot.org/api/download-telemetry";
const SCRIPT: &str = "update-window";
const HTTP_TIMEOUT: Duration = Duration::from_secs(2);
const RELEASE_DOWNLOADS: &str = "https://github.com/apex-shot/apexshot/releases/download/";

/// One file the update window downloads, as the telemetry dashboard names it.
#[derive(Clone, Debug)]
pub(super) struct Asset {
    version: String,
    asset_type: &'static str,
    asset_name: String,
}

impl Asset {
    /// Describe a release asset from its GitHub download URL and file name.
    pub(super) fn new(url: &str, name: &str) -> Self {
        Self {
            version: release_tag(url).unwrap_or("unknown").to_string(),
            asset_type: asset_type(name),
            asset_name: name.to_string(),
        }
    }

    pub(super) fn started(&self) {
        self.send("download_started", "started", None);
    }

    pub(super) fn completed(&self, size_bytes: u64) {
        self.send("download_completed", "success", Some(size_bytes));
    }

    pub(super) fn failed(&self, status: &str) {
        self.send("download_failed", status, None);
    }

    fn send(&self, event: &'static str, status: &str, size_bytes: Option<u64>) {
        let config = load_config().sanitized();
        if !usage_telemetry::telemetry_enabled(&config) {
            return;
        }
        let payload = Payload {
            event,
            script: SCRIPT,
            distro: usage_telemetry::detect_distro_label(),
            channel: "update",
            version: self.version.clone(),
            asset_type: self.asset_type,
            asset_name: self.asset_name.clone(),
            status: status.to_string(),
            size_bytes: size_bytes.unwrap_or(0),
            install_id: usage_telemetry::ensure_install_id(),
        };
        std::thread::spawn(move || {
            let _ = ureq::AgentBuilder::new()
                .timeout(HTTP_TIMEOUT)
                .user_agent(&format!("ApexShotDownloadTelemetry/{SCRIPT}"))
                .build()
                .post(&endpoint())
                .send_json(&payload);
        });
    }
}

#[derive(Debug, Serialize)]
struct Payload {
    event: &'static str,
    script: &'static str,
    distro: String,
    channel: &'static str,
    version: String,
    asset_type: &'static str,
    asset_name: String,
    status: String,
    size_bytes: u64,
    install_id: Option<String>,
}

/// The install scripts read the same override.
fn endpoint() -> String {
    std::env::var("APEXSHOT_TELEMETRY_URL")
        .ok()
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty() && !url.contains("usage-telemetry"))
        .unwrap_or_else(|| DEFAULT_URL.to_string())
}

/// `v0.3.1` from `…/releases/download/v0.3.1/<name>`.
fn release_tag(url: &str) -> Option<&str> {
    let tag = url.strip_prefix(RELEASE_DOWNLOADS)?.split('/').next()?;
    (!tag.is_empty()).then_some(tag)
}

/// The `asset_type` values the install and update scripts report.
fn asset_type(name: &str) -> &'static str {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".deb") {
        "deb"
    } else if name.ends_with(".pkg.tar.zst") {
        "arch_package"
    } else if name.ends_with(".rpm") && name.contains(".fc") {
        "fedora_rpm"
    } else if name.ends_with(".rpm") {
        "opensuse_rpm"
    } else if name.ends_with(".zip") {
        "gnome_extension"
    } else {
        "package"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_the_release_tag_in_the_url() {
        assert_eq!(
            release_tag(
                "https://github.com/apex-shot/apexshot/releases/download/v0.3.1/apexshot_0.3.1-1_amd64.deb"
            ),
            Some("v0.3.1")
        );
        assert_eq!(release_tag("https://example.test/v0.3.1/a.deb"), None);
    }

    #[test]
    fn asset_types_match_the_update_scripts() {
        assert_eq!(asset_type("apexshot_0.3.1-1_amd64.deb"), "deb");
        assert_eq!(
            asset_type("apexshot-0.3.1-1-x86_64.pkg.tar.zst"),
            "arch_package"
        );
        assert_eq!(asset_type("apexshot-0.3.1-1.fc44.x86_64.rpm"), "fedora_rpm");
        assert_eq!(asset_type("apexshot-0.3.1-0.x86_64.rpm"), "opensuse_rpm");
        assert_eq!(
            asset_type("apexshot-gnome-integration.zip"),
            "gnome_extension"
        );
    }
}
