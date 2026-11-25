//! Download telemetry for the update window: `download_started`, then
//! `download_completed` or `download_failed`, on the `update` channel, the
//! same events the Linux update window and update scripts send.
//!
//! Sent unless `APEXSHOT_TELEMETRY` turns it off. It carries the shared
//! anonymous install id and nothing else about the machine, and it never
//! delays or fails the download: each event is posted from its own thread
//! with a short timeout.

use std::time::Duration;

use serde::Serialize;

const DEFAULT_URL: &str = "https://apexshot.org/api/download-telemetry";
const SCRIPT: &str = "update-window";
const HTTP_TIMEOUT: Duration = Duration::from_secs(2);
const RELEASE_DOWNLOADS: &str = "https://github.com/apex-shot/apexshot/releases/download/";

/// The installer the update window downloads, as the dashboard names it.
#[derive(Clone, Debug)]
pub(super) struct Installer {
    version: String,
    asset_name: String,
}

impl Installer {
    pub(super) fn new(url: &str, name: &str) -> Self {
        Self {
            version: release_tag(url).unwrap_or("unknown").to_string(),
            asset_name: name.to_string(),
        }
    }

    pub(super) fn started(&self) {
        self.send("download_started", "started", 0);
    }

    pub(super) fn completed(&self, size_bytes: u64) {
        self.send("download_completed", "success", size_bytes);
    }

    pub(super) fn failed(&self, status: &str) {
        self.send("download_failed", status, 0);
    }

    fn send(&self, event: &'static str, status: &str, size_bytes: u64) {
        if !enabled() {
            return;
        }
        let payload = Payload {
            event,
            script: SCRIPT,
            distro: "windows",
            channel: "update",
            version: self.version.clone(),
            asset_type: "windows_installer",
            asset_name: self.asset_name.clone(),
            status: status.to_string(),
            size_bytes,
            install_id: crate::usage_telemetry::ensure_install_id(),
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
    distro: &'static str,
    channel: &'static str,
    version: String,
    asset_type: &'static str,
    asset_name: String,
    status: String,
    size_bytes: u64,
    install_id: Option<String>,
}

fn enabled() -> bool {
    !std::env::var("APEXSHOT_TELEMETRY").is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        )
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_the_release_tag_in_the_url() {
        assert_eq!(
            release_tag(
                "https://github.com/apex-shot/apexshot/releases/download/v0.3.1/ApexShot-0.3.1-setup.exe"
            ),
            Some("v0.3.1")
        );
        assert_eq!(release_tag("https://example.test/a.exe"), None);
    }
}
