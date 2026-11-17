//! The recording manifest written beside a pointer sidecar.
//!
//! The shared sidecar reader takes the recording's device pixel ratio from
//! `<sidecar path>.recording.json`, so a Linux recording has to leave the same
//! document beside its sidecar that the Windows recorder writes. Only the
//! fields this recorder can fill truthfully are written; the reader ignores
//! the rest.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Appended to the pointer sidecar's path. Keep in step with
/// `PointerSidecar::load`, which reads this exact suffix.
pub const MANIFEST_SUFFIX: &str = ".recording.json";

/// Format version. The shared reader understands version 1.
pub const MANIFEST_VERSION: u32 = 1;

/// What a kept recording was, as far as this recorder knows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingManifest {
    pub version: u32,
    /// Recorded video pixels per logical pointer unit for the recorded
    /// monitor. `controls::recording_pixel_geometry` derives it; it stays 1.0
    /// when the monitor's scale cannot be resolved.
    pub device_pixel_ratio: f64,
    /// Frame rate the recorder asked the encoder for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_rate: Option<f64>,
    /// "screen", "area" or "window".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Width of the recorded region in video pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// Height of the recorded region in video pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

impl RecordingManifest {
    /// A version-1 manifest carrying only the ratio.
    pub fn new(device_pixel_ratio: f64) -> Self {
        Self {
            version: MANIFEST_VERSION,
            device_pixel_ratio: valid_ratio(device_pixel_ratio),
            frame_rate: None,
            target: None,
            width: None,
            height: None,
        }
    }

    /// The path the shared sidecar reader looks beside `sidecar_path`.
    pub fn path_for(sidecar_path: &Path) -> PathBuf {
        let mut path = sidecar_path.as_os_str().to_os_string();
        path.push(MANIFEST_SUFFIX);
        PathBuf::from(path)
    }

    /// Write this manifest beside the sidecar at `sidecar_path`. Returns the
    /// path written.
    pub fn write_beside(&self, sidecar_path: &Path) -> anyhow::Result<PathBuf> {
        let path = Self::path_for(sidecar_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_vec_pretty(self)?)?;
        Ok(path)
    }
}

fn valid_ratio(ratio: f64) -> f64 {
    if ratio.is_finite() && ratio > 0.0 {
        ratio
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::sidecar::{
        manifest_device_pixel_ratio, CaptureRegion, PointerSidecar,
    };

    fn scratch_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "apexshot-manifest-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn the_ratio_round_trips_through_the_shared_reader() {
        let mut manifest = RecordingManifest::new(1.5);
        manifest.frame_rate = Some(30.0);
        manifest.target = Some("screen".into());
        let json = serde_json::to_string(&manifest).unwrap();
        assert_eq!(manifest_device_pixel_ratio(&json), 1.5);
    }

    #[test]
    fn the_manifest_lands_where_the_sidecar_reader_looks() {
        let dir = scratch_dir("path");
        let sidecar_path = dir.join("pointer.json");
        let sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080,
            },
        );
        sidecar.write(&sidecar_path).unwrap();
        let manifest = RecordingManifest::new(2.0);
        let manifest_path = manifest.write_beside(&sidecar_path).unwrap();

        assert_eq!(manifest_path, RecordingManifest::path_for(&sidecar_path));
        assert!(manifest_path.is_file());
        assert_eq!(
            PointerSidecar::load(&sidecar_path)
                .unwrap()
                .device_pixel_ratio(),
            2.0
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_fields_are_omitted_rather_than_invented() {
        let json = serde_json::to_value(RecordingManifest::new(1.0)).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["devicePixelRatio"], 1.0);
        assert!(json.get("frameRate").is_none());
        assert!(json.get("target").is_none());
        assert!(json.get("width").is_none());
        assert!(json.get("height").is_none());
    }

    #[test]
    fn an_unusable_ratio_falls_back_to_one() {
        assert_eq!(RecordingManifest::new(0.0).device_pixel_ratio, 1.0);
        assert_eq!(RecordingManifest::new(f64::NAN).device_pixel_ratio, 1.0);
        assert_eq!(RecordingManifest::new(-2.0).device_pixel_ratio, 1.0);
    }
}
