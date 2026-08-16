use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SIDECAR_VERSION: u32 = 1;
pub const MAX_CLICKS: usize = 500;
pub const CLICK_PULSE_WINDOW_SECONDS: f64 = 0.08;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CursorKind {
    #[default]
    Default,
    Hand,
    Text,
    Crosshair,
    Wait,
    Resize,
}

impl CursorKind {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "hand" => Self::Hand,
            "text" => Self::Text,
            "crosshair" => Self::Crosshair,
            "wait" => Self::Wait,
            "resize" => Self::Resize,
            _ => Self::Default,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Hand => "hand",
            Self::Text => "text",
            Self::Crosshair => "crosshair",
            Self::Wait => "wait",
            Self::Resize => "resize",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CaptureRegion {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl CaptureRegion {
    pub fn from_capture(x: Option<i32>, y: Option<i32>, w: Option<u32>, h: Option<u32>) -> Self {
        match (x, y, w, h) {
            (Some(x), Some(y), Some(w), Some(h)) if w > 0 && h > 0 => Self { x, y, w, h },
            _ => Self {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
        }
    }

    pub fn is_area(self) -> bool {
        self.w > 0 && self.h > 0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointerSample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub kind: CursorKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClickSample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub button: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointerSidecar {
    pub version: u32,
    pub t0_monotonic_us: i64,
    pub region: CaptureRegion,
    pub pointer: Vec<PointerSample>,
    pub clicks: Vec<ClickSample>,
}

impl PointerSidecar {
    pub fn new(t0_monotonic_us: i64, region: CaptureRegion) -> Self {
        Self {
            version: SIDECAR_VERSION,
            t0_monotonic_us,
            region,
            pointer: Vec::new(),
            clicks: Vec::new(),
        }
    }

    pub fn sidecar_path(video_path: &Path) -> PathBuf {
        let parent = video_path.parent().unwrap_or_else(|| Path::new(""));
        let stem = video_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("recording");
        parent.join(format!("{stem}.apexshot.json"))
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)?;
        let sidecar: Self = serde_json::from_slice(&bytes)?;
        Ok(sidecar)
    }

    pub fn load_next_to_video(video_path: &Path) -> Option<Self> {
        let path = Self::sidecar_path(video_path);
        if !path.is_file() {
            return None;
        }
        match Self::load(&path) {
            Ok(sidecar) => Some(sidecar),
            Err(err) => {
                eprintln!(
                    "[recording] failed to load pointer sidecar {}: {err}",
                    path.display()
                );
                None
            }
        }
    }

    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn write_next_to_video(&self, video_path: &Path) -> anyhow::Result<PathBuf> {
        let path = Self::sidecar_path(video_path);
        self.write(&path)?;
        Ok(path)
    }

    pub fn delete_next_to_video(video_path: &Path) {
        let path = Self::sidecar_path(video_path);
        if path.exists() {
            if let Err(err) = std::fs::remove_file(&path) {
                eprintln!(
                    "[recording] failed to delete pointer sidecar {}: {err}",
                    path.display()
                );
            }
        }
    }

    pub fn subtract_region(&mut self) {
        if !self.region.is_area() {
            return;
        }
        let origin_x = self.region.x as f64;
        let origin_y = self.region.y as f64;
        for sample in &mut self.pointer {
            sample.x -= origin_x;
            sample.y -= origin_y;
        }
        for click in &mut self.clicks {
            click.x -= origin_x;
            click.y -= origin_y;
        }
    }

    pub fn sample_at(&self, t: f64) -> Option<&PointerSample> {
        if self.pointer.is_empty() {
            return None;
        }
        let idx = self.pointer.partition_point(|sample| sample.t <= t);
        if idx == 0 {
            return self.pointer.first();
        }
        if idx >= self.pointer.len() {
            return self.pointer.last();
        }
        let prev = &self.pointer[idx - 1];
        let next = &self.pointer[idx];
        if (t - prev.t).abs() <= (next.t - t).abs() {
            Some(prev)
        } else {
            Some(next)
        }
    }

    pub fn interpolated_at(&self, t: f64) -> Option<(f64, f64, CursorKind)> {
        if self.pointer.is_empty() {
            return None;
        }
        let idx = self.pointer.partition_point(|sample| sample.t <= t);
        if idx == 0 {
            let first = &self.pointer[0];
            return Some((first.x, first.y, first.kind));
        }
        if idx >= self.pointer.len() {
            let last = self.pointer.last()?;
            return Some((last.x, last.y, last.kind));
        }
        let prev = &self.pointer[idx - 1];
        let next = &self.pointer[idx];
        let span = (next.t - prev.t).max(f64::EPSILON);
        let alpha = ((t - prev.t) / span).clamp(0.0, 1.0);
        Some((
            prev.x + (next.x - prev.x) * alpha,
            prev.y + (next.y - prev.y) * alpha,
            prev.kind,
        ))
    }

    pub fn click_pulse_at(&self, t: f64) -> f64 {
        let mut peak: f64 = 0.0;
        for click in &self.clicks {
            let distance = (t - click.t).abs();
            if distance <= CLICK_PULSE_WINDOW_SECONDS {
                let amount = 1.0 - (distance / CLICK_PULSE_WINDOW_SECONDS);
                peak = peak.max(amount);
            }
        }
        1.0 + peak * 0.35
    }
}

pub fn delete_recording_outputs(video_path: &Path) {
    if video_path.exists() {
        let _ = std::fs::remove_file(video_path);
    }
    PointerSidecar::delete_next_to_video(video_path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_path_uses_apexshot_json_stem() {
        let video = PathBuf::from("/tmp/ApexShot Recording 2026-08-15 at 12-00-00.mp4");
        assert_eq!(
            PointerSidecar::sidecar_path(&video),
            PathBuf::from("/tmp/ApexShot Recording 2026-08-15 at 12-00-00.apexshot.json")
        );
    }

    #[test]
    fn roundtrip_preserves_clock_and_samples() {
        let dir =
            std::env::temp_dir().join(format!("apexshot-sidecar-roundtrip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("clip.mp4");
        let mut sidecar = PointerSidecar::new(
            1_700_000,
            CaptureRegion {
                x: 10,
                y: 20,
                w: 800,
                h: 600,
            },
        );
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 100.0,
            y: 200.0,
            kind: CursorKind::Default,
        });
        sidecar.clicks.push(ClickSample {
            t: 1.2,
            x: 100.0,
            y: 200.0,
            button: 1,
        });
        sidecar.write_next_to_video(&video).unwrap();

        let loaded = PointerSidecar::load_next_to_video(&video).unwrap();
        assert_eq!(loaded, sidecar);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_sidecar_file_fails_parse() {
        let dir =
            std::env::temp_dir().join(format!("apexshot-sidecar-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.apexshot.json");
        std::fs::write(&path, b"").unwrap();
        assert!(PointerSidecar::load(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn region_subtract_moves_desktop_coords_into_capture_space() {
        let mut sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 200,
                y: 100,
                w: 640,
                h: 480,
            },
        );
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 250.0,
            y: 180.0,
            kind: CursorKind::Hand,
        });
        sidecar.clicks.push(ClickSample {
            t: 0.4,
            x: 200.0,
            y: 100.0,
            button: 1,
        });
        sidecar.subtract_region();
        assert_eq!(sidecar.pointer[0].x, 50.0);
        assert_eq!(sidecar.pointer[0].y, 80.0);
        assert_eq!(sidecar.clicks[0].x, 0.0);
        assert_eq!(sidecar.clicks[0].y, 0.0);
    }

    #[test]
    fn fullscreen_region_does_not_subtract() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 250.0,
            y: 180.0,
            kind: CursorKind::Default,
        });
        sidecar.subtract_region();
        assert_eq!(sidecar.pointer[0].x, 250.0);
        assert_eq!(sidecar.pointer[0].y, 180.0);
    }

    #[test]
    fn interpolated_sample_lerps_between_neighbors() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 0.0,
            y: 0.0,
            kind: CursorKind::Default,
        });
        sidecar.pointer.push(PointerSample {
            t: 1.0,
            x: 100.0,
            y: 50.0,
            kind: CursorKind::Default,
        });
        let (x, y, kind) = sidecar.interpolated_at(0.5).unwrap();
        assert!((x - 50.0).abs() < 1e-9);
        assert!((y - 25.0).abs() < 1e-9);
        assert_eq!(kind, CursorKind::Default);
    }

    #[test]
    fn click_pulse_peaks_at_click_time() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.clicks.push(ClickSample {
            t: 1.0,
            x: 0.0,
            y: 0.0,
            button: 1,
        });
        assert!((sidecar.click_pulse_at(1.0) - 1.35).abs() < 1e-9);
        assert!((sidecar.click_pulse_at(0.0) - 1.0).abs() < 1e-9);
        assert!(sidecar.click_pulse_at(1.04) > 1.0);
    }

    #[test]
    fn discard_deletes_video_and_sidecar() {
        let dir =
            std::env::temp_dir().join(format!("apexshot-sidecar-discard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("take.mp4");
        std::fs::write(&video, b"mp4").unwrap();
        let sidecar = PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.write_next_to_video(&video).unwrap();
        assert!(PointerSidecar::sidecar_path(&video).exists());
        delete_recording_outputs(&video);
        assert!(!video.exists());
        assert!(!PointerSidecar::sidecar_path(&video).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
