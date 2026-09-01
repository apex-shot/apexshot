use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const SIDECAR_VERSION: u32 = 1;
pub const MAX_POINTER_SAMPLES: usize = 8_000;
pub const MAX_CLICKS: usize = 500;
pub const CLICK_PULSE_WINDOW_SECONDS: f64 = 0.08;
pub const CLICK_RIPPLE_WINDOW_SECONDS: f64 = 0.32;

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorMotion {
    pub smooth: f64,
    pub hide_idle: bool,
    pub idle_ms: f64,
    pub trail: f64,
    pub tilt: f64,
    pub sway: f64,
    pub speed: f64,
}

impl Default for CursorMotion {
    fn default() -> Self {
        Self {
            smooth: 0.0,
            hide_idle: false,
            idle_ms: 800.0,
            trail: 0.0,
            tilt: 0.0,
            sway: 0.0,
            speed: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CursorFrame {
    pub x: f64,
    pub y: f64,
    pub kind: CursorKind,
    pub alpha: f64,
    pub tilt: f64,
    pub trail: Vec<(f64, f64, f64)>,
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
        pointer_sidecars_directory().join(sidecar_file_name(video_path))
    }

    pub fn legacy_sidecar_path(video_path: &Path) -> PathBuf {
        let parent = video_path.parent().unwrap_or_else(|| Path::new(""));
        let stem = video_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("recording");
        parent.join(format!("{stem}.apexshot.json"))
    }

    pub fn exists_next_to_video(video_path: &Path) -> bool {
        Self::sidecar_path(video_path).is_file() || Self::legacy_sidecar_path(video_path).is_file()
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)?;
        let sidecar: Self = serde_json::from_slice(&bytes)?;
        Ok(sidecar)
    }

    pub fn load_next_to_video(video_path: &Path) -> Option<Self> {
        let path = Self::sidecar_path(video_path);
        if !path.is_file() {
            return Self::load_migrate_legacy_sidecar(video_path);
        }
        Self::load_logged(&path)
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
        let current_path = Self::sidecar_path(video_path);
        if current_path.is_file() {
            remove_sidecar(&current_path);
        }

        let legacy_path = Self::legacy_sidecar_path(video_path);
        if legacy_path.is_file() {
            remove_sidecar(&legacy_path);
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

    fn load_logged(path: &Path) -> Option<Self> {
        match Self::load(path) {
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

    fn load_migrate_legacy_sidecar(video_path: &Path) -> Option<Self> {
        let legacy_path = Self::legacy_sidecar_path(video_path);
        if !legacy_path.is_file() {
            return None;
        }

        let sidecar = Self::load_logged(&legacy_path)?;
        if sidecar.write(&Self::sidecar_path(video_path)).is_ok() {
            // The MP4 folder should contain media files only. Keep cursor data
            // in the private app-data directory and migrate old recordings too.
            remove_sidecar(&legacy_path);
        }
        Some(sidecar)
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

    pub fn presented_at(&self, t: f64, motion: CursorMotion) -> Option<CursorFrame> {
        let (_, _, kind) = self.interpolated_at(t)?;
        let (mut x, mut y) = if motion.smooth <= 0.01 {
            self.interpolated_at(t).map(|(x, y, _)| (x, y))?
        } else {
            self.smoothed_at(t, motion.smooth, motion.speed)
        };
        let (vx, vy) = self.velocity_at(t, motion.smooth, motion.speed);
        let travel = vx.hypot(vy);
        let still = (1.0 - (travel / 90.0).clamp(0.0, 1.0)).powi(2);
        if motion.sway > 0.01 {
            x += (t * 5.1).sin() * 2.6 * motion.sway * still;
            y += (t * 3.7).cos() * 1.8 * motion.sway * still;
        }
        let tilt = if motion.tilt <= 0.01 || travel < 28.0 {
            0.0
        } else {
            let angle = vy.atan2(vx) + std::f64::consts::FRAC_PI_2;
            (angle * motion.tilt * 0.28 * (travel / 220.0).clamp(0.0, 1.0)).clamp(-0.42, 0.42)
        };
        let alpha = if motion.hide_idle {
            self.idle_alpha(t, motion.idle_ms)
        } else {
            1.0
        };
        let trail = self.trail_at(t, motion);
        Some(CursorFrame {
            x,
            y,
            kind,
            alpha,
            tilt,
            trail,
        })
    }

    fn velocity_at(&self, t: f64, smooth: f64, speed: f64) -> (f64, f64) {
        let dt = 0.04;
        let a = if smooth <= 0.01 {
            self.interpolated_at((t - dt).max(0.0))
                .map(|(x, y, _)| (x, y))
        } else {
            Some(self.smoothed_at((t - dt).max(0.0), smooth, speed))
        };
        let b = if smooth <= 0.01 {
            self.interpolated_at(t).map(|(x, y, _)| (x, y))
        } else {
            Some(self.smoothed_at(t, smooth, speed))
        };
        match (a, b) {
            (Some((ax, ay)), Some((bx, by))) => ((bx - ax) / dt, (by - ay) / dt),
            _ => (0.0, 0.0),
        }
    }

    fn trail_at(&self, t: f64, motion: CursorMotion) -> Vec<(f64, f64, f64)> {
        if motion.trail <= 0.02 || self.pointer.is_empty() {
            return Vec::new();
        }
        let count = ((motion.trail * 8.0).round() as usize).clamp(1, 8);
        let mut trail = Vec::with_capacity(count);
        for index in 1..=count {
            let back = t - 0.024 * index as f64 * (0.55 + motion.trail);
            if back < 0.0 {
                break;
            }
            let (x, y) = if motion.smooth <= 0.01 {
                match self.interpolated_at(back) {
                    Some((x, y, _)) => (x, y),
                    None => continue,
                }
            } else {
                self.smoothed_at(back, motion.smooth, motion.speed)
            };
            let alpha =
                motion.trail * (1.0 - index as f64 / (count as f64 + 0.35)).powf(1.35) * 0.42;
            if alpha >= 0.03 {
                trail.push((x, y, alpha));
            }
        }
        trail
    }

    fn smoothed_at(&self, t: f64, smooth: f64, speed: f64) -> (f64, f64) {
        let Some(first) = self.pointer.first() else {
            return (0.0, 0.0);
        };
        let tau = (0.01 + smooth.clamp(0.0, 1.0).powi(2) * 0.26) / speed.clamp(0.25, 3.0);
        let mut px = first.x;
        let mut py = first.y;
        let mut pt = first.t;
        for sample in self.pointer.iter().skip(1) {
            let target_t = sample.t.min(t);
            let dt = (target_t - pt).max(0.0);
            let k = 1.0 - (-dt / tau).exp();
            if sample.t <= t {
                px += (sample.x - px) * k;
                py += (sample.y - py) * k;
                pt = sample.t;
                continue;
            }
            if let Some((ix, iy, _)) = self.interpolated_at(t) {
                px += (ix - px) * k;
                py += (iy - py) * k;
            }
            break;
        }
        (px, py)
    }

    fn idle_alpha(&self, t: f64, idle_ms: f64) -> f64 {
        let window = (idle_ms / 1000.0).clamp(0.12, 4.0);
        let Some((cx, cy, _)) = self.interpolated_at(t) else {
            return 1.0;
        };
        let mut last_move = self.pointer.first().map(|sample| sample.t).unwrap_or(0.0);
        for sample in &self.pointer {
            if sample.t > t {
                break;
            }
            if (sample.x - cx).hypot(sample.y - cy) > 8.0 {
                last_move = sample.t;
            }
        }
        let still = t - last_move;
        if still <= window {
            return 1.0;
        }
        (1.0 - (still - window) / 0.18).clamp(0.0, 1.0)
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

    pub fn click_ripples_at(&self, t: f64, window: f64) -> Vec<(f64, f64, f64)> {
        let window = window.max(1e-6);
        let mut ripples = Vec::new();
        for click in &self.clicks {
            let age = t - click.t;
            if (0.0..=window).contains(&age) {
                let progress = (age / window).clamp(0.0, 1.0);
                ripples.push((click.x, click.y, progress));
            }
        }
        ripples
    }
}

fn pointer_sidecars_directory() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("apexshot")
        .join("pointer-sidecars")
}

fn sidecar_file_name(video_path: &Path) -> PathBuf {
    let canonical = video_path
        .canonicalize()
        .unwrap_or_else(|_| video_path.to_path_buf());
    let path_str = canonical.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    PathBuf::from(format!("{:x}", hasher.finalize()))
}

fn remove_sidecar(path: &Path) {
    if let Err(err) = std::fs::remove_file(path) {
        eprintln!(
            "[recording] failed to delete pointer sidecar {}: {err}",
            path.display()
        );
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
    fn sidecar_path_is_app_local_and_legacy_path_stays_local() {
        let video = PathBuf::from("/tmp/ApexShot Recording 2026-08-15 at 12-00-00.mp4");
        assert_eq!(
            PointerSidecar::sidecar_path(&video),
            pointer_sidecars_directory().join(sidecar_file_name(&video))
        );
        assert_ne!(
            PointerSidecar::sidecar_path(&video),
            PointerSidecar::legacy_sidecar_path(&video)
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
        PointerSidecar::delete_next_to_video(&video);
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
    fn legacy_sidecar_is_migrated_out_of_media_folder() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "apexshot-sidecar-legacy-migration-{}-{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("legacy.mp4");
        let mut sidecar =
            PointerSidecar::new(42, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 5.0,
            y: 6.0,
            kind: CursorKind::Default,
        });
        sidecar
            .write(&PointerSidecar::legacy_sidecar_path(&video))
            .unwrap();

        let loaded = PointerSidecar::load_next_to_video(&video).unwrap();
        assert_eq!(loaded, sidecar);
        assert!(PointerSidecar::sidecar_path(&video).is_file());
        assert!(!PointerSidecar::legacy_sidecar_path(&video).exists());

        PointerSidecar::delete_next_to_video(&video);
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
    fn presented_at_smooths_behind_raw_sample() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 0.0,
            y: 0.0,
            kind: CursorKind::Default,
        });
        sidecar.pointer.push(PointerSample {
            t: 0.2,
            x: 200.0,
            y: 0.0,
            kind: CursorKind::Default,
        });
        let raw = sidecar.interpolated_at(0.2).unwrap();
        let presented = sidecar
            .presented_at(
                0.2,
                CursorMotion {
                    smooth: 0.8,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        assert!(presented.x < raw.0);
        assert!(presented.x > 0.0);
        assert!((presented.alpha - 1.0).abs() < 1e-9);
    }

    #[test]
    fn presented_at_hides_idle_cursor() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 10.0,
            kind: CursorKind::Default,
        });
        sidecar.pointer.push(PointerSample {
            t: 0.05,
            x: 12.0,
            y: 10.0,
            kind: CursorKind::Default,
        });
        sidecar.pointer.push(PointerSample {
            t: 2.0,
            x: 12.0,
            y: 10.0,
            kind: CursorKind::Default,
        });
        let shown = sidecar
            .presented_at(
                0.05,
                CursorMotion {
                    hide_idle: true,
                    idle_ms: 400.0,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        let hidden = sidecar
            .presented_at(
                2.0,
                CursorMotion {
                    hide_idle: true,
                    idle_ms: 400.0,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        assert!((shown.alpha - 1.0).abs() < 1e-9);
        assert!(hidden.alpha < 0.05);
    }

    #[test]
    fn idle_delay_controls_when_cursor_fades() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 10.0,
            kind: CursorKind::Default,
        });
        let short_delay = sidecar
            .presented_at(
                1.0,
                CursorMotion {
                    hide_idle: true,
                    idle_ms: 200.0,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        let long_delay = sidecar
            .presented_at(
                1.0,
                CursorMotion {
                    hide_idle: true,
                    idle_ms: 2000.0,
                    ..CursorMotion::default()
                },
            )
            .unwrap();

        assert!(short_delay.alpha < 0.05);
        assert!((long_delay.alpha - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sway_offsets_an_idle_cursor() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 10.0,
            kind: CursorKind::Default,
        });
        let still = sidecar.presented_at(1.0, CursorMotion::default()).unwrap();
        let swaying = sidecar
            .presented_at(
                1.0,
                CursorMotion {
                    sway: 1.0,
                    ..CursorMotion::default()
                },
            )
            .unwrap();

        assert!((swaying.x - still.x).abs() > 0.1);
        assert!((swaying.y - still.y).abs() > 0.1);
    }

    #[test]
    fn presented_at_adds_trail_and_tilt_when_moving() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 0.0,
            y: 0.0,
            kind: CursorKind::Default,
        });
        sidecar.pointer.push(PointerSample {
            t: 0.2,
            x: 240.0,
            y: 0.0,
            kind: CursorKind::Default,
        });
        let frame = sidecar
            .presented_at(
                0.2,
                CursorMotion {
                    trail: 0.8,
                    tilt: 1.0,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        assert!(!frame.trail.is_empty());
        assert!(frame.tilt.abs() > 0.02);
    }

    #[test]
    fn presented_at_speed_keeps_cursor_on_the_recorded_timeline() {
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
            y: 10.0,
            kind: CursorKind::Default,
        });
        sidecar.pointer.push(PointerSample {
            t: 2.0,
            x: 200.0,
            y: 20.0,
            kind: CursorKind::Default,
        });
        sidecar.pointer.push(PointerSample {
            t: 3.0,
            x: 300.0,
            y: 30.0,
            kind: CursorKind::Default,
        });
        sidecar.clicks.push(ClickSample {
            t: 1.0,
            x: 100.0,
            y: 10.0,
            button: 1,
        });
        let expected = sidecar.interpolated_at(1.0).unwrap();
        let presented = sidecar
            .presented_at(
                1.0,
                CursorMotion {
                    speed: 2.0,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        assert!((presented.x - expected.0).abs() < 1e-9);
        assert!((presented.y - expected.1).abs() < 1e-9);
        assert!((presented.x - 100.0).abs() < 1e-9);
        let ripples = sidecar.click_ripples_at(1.0, CLICK_RIPPLE_WINDOW_SECONDS);
        assert_eq!(ripples.len(), 1);
        assert!((presented.x - ripples[0].0).abs() < 1e-9);
        assert!((presented.y - ripples[0].1).abs() < 1e-9);
        assert!(sidecar.click_pulse_at(1.0) > 1.0);
    }

    #[test]
    fn speed_controls_how_quickly_smoothing_catches_up() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 0.0,
            y: 0.0,
            kind: CursorKind::Default,
        });
        sidecar.pointer.push(PointerSample {
            t: 0.2,
            x: 200.0,
            y: 0.0,
            kind: CursorKind::Default,
        });
        let slow = sidecar
            .presented_at(
                0.2,
                CursorMotion {
                    smooth: 0.8,
                    speed: 0.5,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        let fast = sidecar
            .presented_at(
                0.2,
                CursorMotion {
                    smooth: 0.8,
                    speed: 2.0,
                    ..CursorMotion::default()
                },
            )
            .unwrap();

        assert!(fast.x > slow.x);
        assert!(fast.x <= 200.0);
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
    fn click_ripples_expand_after_click() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.clicks.push(ClickSample {
            t: 1.0,
            x: 40.0,
            y: 80.0,
            button: 1,
        });
        assert!(sidecar
            .click_ripples_at(0.5, CLICK_RIPPLE_WINDOW_SECONDS)
            .is_empty());
        let ripples = sidecar.click_ripples_at(1.08, CLICK_RIPPLE_WINDOW_SECONDS);
        assert_eq!(ripples.len(), 1);
        assert!((ripples[0].0 - 40.0).abs() < 1e-9);
        assert!(ripples[0].2 > 0.0 && ripples[0].2 < 1.0);
        assert!(sidecar
            .click_ripples_at(1.5, CLICK_RIPPLE_WINDOW_SECONDS)
            .is_empty());
    }

    #[test]
    fn click_ripples_honor_effect_window() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.clicks.push(ClickSample {
            t: 1.0,
            x: 40.0,
            y: 80.0,
            button: 1,
        });
        assert!(sidecar.click_ripples_at(1.4, 0.32).is_empty());
        let long = sidecar.click_ripples_at(1.4, 0.5);
        assert_eq!(long.len(), 1);
        assert!((long[0].2 - 0.8).abs() < 1e-9);
        let short = sidecar.click_ripples_at(1.16, 0.32);
        assert_eq!(short.len(), 1);
        assert!((short[0].2 - 0.5).abs() < 1e-9);
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
