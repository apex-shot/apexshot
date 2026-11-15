use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Original,
    H264,
    Hevc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwarePreference {
    Environment,
    Software,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportSettings {
    pub resolution: Option<u32>,
    pub fps: Option<u32>,
    pub codec: Codec,
    pub crf: Option<u32>,
    pub hardware: HardwarePreference,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            resolution: None,
            fps: None,
            codec: Codec::Original,
            crf: None,
            hardware: HardwarePreference::Environment,
        }
    }
}

impl ExportSettings {
    pub(crate) const MIN_FPS: u32 = 1;
    pub(crate) const MAX_FPS: u32 = 240;
    pub(crate) const MAX_CRF: u32 = 51;
    pub(crate) const MIN_EDGE: u32 = 2;

    pub(crate) fn sanitized(self) -> Self {
        Self {
            resolution: self
                .resolution
                .filter(|edge| *edge >= Self::MIN_EDGE && edge.is_multiple_of(2)),
            fps: self
                .fps
                .filter(|fps| (Self::MIN_FPS..=Self::MAX_FPS).contains(fps)),
            codec: self.codec,
            crf: self.crf.map(|crf| crf.min(Self::MAX_CRF)),
            hardware: self.hardware,
        }
    }

    pub(crate) fn forces_reencode(self) -> bool {
        self.resolution.is_some()
            || self.fps.is_some()
            || self.codec != Codec::Original
            || self.crf.is_some()
    }

    pub(crate) fn effective_codec(self) -> Codec {
        match self.codec {
            Codec::Original => Codec::H264,
            codec => codec,
        }
    }

    pub(crate) fn crf_or(self, fallback: u32) -> u32 {
        self.crf.unwrap_or(fallback).min(Self::MAX_CRF)
    }

    pub(crate) fn dimensions(self, canvas: (u32, u32)) -> (u32, u32) {
        let Some(edge) = self.resolution else {
            return (
                super::model::even_dimension(canvas.0.max(2)),
                super::model::even_dimension(canvas.1.max(2)),
            );
        };
        let short = canvas.0.min(canvas.1).max(1);
        let ratio = f64::from(edge) / f64::from(short);
        (
            super::model::even_dimension((f64::from(canvas.0) * ratio).round().max(2.0) as u32),
            super::model::even_dimension((f64::from(canvas.1) * ratio).round().max(2.0) as u32),
        )
    }
}

const PHASE_PREPARING: u64 = 0;
const PHASE_ENCODING: u64 = 1;
const PHASE_FINISHING: u64 = 2;
const PHASE_FINISHED: u64 = 3;
const PHASE_RETRYING_SOFTWARE: u64 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportPhase {
    Preparing,
    Encoding,
    Finishing,
    Finished,
    RetryingSoftware,
}

#[derive(Debug)]
pub struct ExportProgress {
    encoded_us: AtomicU64,
    total_us: AtomicU64,
    phase: AtomicU64,
    retried: AtomicBool,
}

impl ExportProgress {
    pub(crate) fn new(total_us: u64) -> Self {
        Self {
            encoded_us: AtomicU64::new(0),
            total_us: AtomicU64::new(total_us),
            phase: AtomicU64::new(PHASE_PREPARING),
            retried: AtomicBool::new(false),
        }
    }

    pub(crate) fn reset(&self) {
        self.encoded_us.store(0, Ordering::Relaxed);
        self.phase.store(PHASE_PREPARING, Ordering::Relaxed);
    }

    pub(crate) fn set_total_us(&self, total_us: u64) {
        self.total_us.store(total_us, Ordering::Relaxed);
    }

    pub(crate) fn set_phase(&self, phase: ExportPhase) {
        let marker = match phase {
            ExportPhase::Preparing => PHASE_PREPARING,
            ExportPhase::Encoding => PHASE_ENCODING,
            ExportPhase::Finishing => PHASE_FINISHING,
            ExportPhase::Finished => PHASE_FINISHED,
            ExportPhase::RetryingSoftware => PHASE_RETRYING_SOFTWARE,
        };
        self.phase.store(marker, Ordering::Relaxed);
    }

    pub(crate) fn phase(&self) -> ExportPhase {
        match self.phase.load(Ordering::Relaxed) {
            PHASE_ENCODING => ExportPhase::Encoding,
            PHASE_FINISHING => ExportPhase::Finishing,
            PHASE_FINISHED => ExportPhase::Finished,
            PHASE_RETRYING_SOFTWARE => ExportPhase::RetryingSoftware,
            _ => ExportPhase::Preparing,
        }
    }

    pub(crate) fn mark_retried(&self) {
        self.retried.store(true, Ordering::Relaxed);
    }

    pub(crate) fn retried(&self) -> bool {
        self.retried.load(Ordering::Relaxed)
    }

    pub(crate) fn note_encoded(&self, base_us: u64, local_us: u64) {
        self.encoded_us
            .fetch_max(base_us.saturating_add(local_us), Ordering::Relaxed);
    }

    pub(crate) fn encoded_us(&self) -> u64 {
        self.encoded_us.load(Ordering::Relaxed)
    }

    pub(crate) fn total_us(&self) -> u64 {
        self.total_us.load(Ordering::Relaxed)
    }

    pub(crate) fn fraction(&self) -> f64 {
        if self.phase() == ExportPhase::Finished {
            return 1.0;
        }
        let total = self.total_us();
        if total == 0 {
            return 0.0;
        }
        (self.encoded_us() as f64 / total as f64).clamp(0.0, 0.999)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_keep_the_legacy_stream_copy_path() {
        let settings = ExportSettings::default();
        assert!(!settings.forces_reencode());
        assert_eq!(settings, ExportSettings::default());
        assert!(!settings.sanitized().forces_reencode());
    }

    #[test]
    fn explicit_choices_force_a_reencode() {
        for settings in [
            ExportSettings {
                resolution: Some(720),
                ..Default::default()
            },
            ExportSettings {
                fps: Some(60),
                ..Default::default()
            },
            ExportSettings {
                codec: Codec::H264,
                ..Default::default()
            },
            ExportSettings {
                codec: Codec::Hevc,
                ..Default::default()
            },
            ExportSettings {
                crf: Some(24),
                ..Default::default()
            },
        ] {
            assert!(settings.forces_reencode(), "{settings:?}");
        }
        assert!(!ExportSettings {
            hardware: HardwarePreference::Software,
            ..Default::default()
        }
        .forces_reencode());
    }

    #[test]
    fn short_edge_scales_landscape_portrait_and_custom_even() {
        let settings = ExportSettings {
            resolution: Some(720),
            ..Default::default()
        };
        assert_eq!(settings.dimensions((1920, 1080)), (1280, 720));
        assert_eq!(settings.dimensions((1080, 1920)), (720, 1280));
        assert_eq!(settings.dimensions((1000, 300)), (2400, 720));
        let (w, h) = settings.dimensions((1919, 1079));
        assert!(w % 2 == 0 && h % 2 == 0);
        assert_eq!(
            ExportSettings {
                resolution: Some(2160),
                ..Default::default()
            }
            .dimensions((1920, 1080)),
            (3840, 2160)
        );
        assert_eq!(settings.dimensions((2, 2)), (720, 720));
    }

    #[test]
    fn original_resolution_keeps_the_canvas() {
        assert_eq!(
            ExportSettings::default().dimensions((1920, 1080)),
            (1920, 1080)
        );
        assert_eq!(
            ExportSettings::default().dimensions((1919, 1079)),
            (1918, 1078)
        );
    }

    #[test]
    fn invalid_values_are_sanitized_before_use() {
        let settings = ExportSettings {
            resolution: Some(0),
            fps: Some(0),
            codec: Codec::H264,
            crf: Some(90),
            hardware: HardwarePreference::Auto,
        }
        .sanitized();
        assert_eq!(settings.resolution, None);
        assert_eq!(settings.fps, None);
        assert_eq!(settings.crf, Some(ExportSettings::MAX_CRF));
        assert_eq!(
            ExportSettings {
                fps: Some(500),
                ..Default::default()
            }
            .sanitized()
            .fps,
            None
        );
        assert_eq!(
            ExportSettings {
                fps: Some(240),
                ..Default::default()
            }
            .sanitized()
            .fps,
            Some(240)
        );
    }

    #[test]
    fn effective_codec_defaults_to_h264() {
        assert_eq!(ExportSettings::default().effective_codec(), Codec::H264);
        assert_eq!(
            ExportSettings {
                codec: Codec::Hevc,
                ..Default::default()
            }
            .effective_codec(),
            Codec::Hevc
        );
    }

    #[test]
    fn progress_monotonic_and_clamped_until_finished() {
        let progress = ExportProgress::new(1_000_000);
        progress.set_phase(ExportPhase::Encoding);
        progress.note_encoded(0, 400_000);
        assert!((progress.fraction() - 0.4).abs() < 1e-9);
        progress.note_encoded(0, 200_000);
        assert!((progress.fraction() - 0.4).abs() < 1e-9);
        progress.note_encoded(600_000, 700_000);
        assert!(progress.fraction() <= 0.999);
        assert!(progress.fraction() < 1.0);
        progress.reset();
        assert_eq!(progress.encoded_us(), 0);
        assert_eq!(progress.phase(), ExportPhase::Preparing);
        progress.set_phase(ExportPhase::Finished);
        assert_eq!(progress.fraction(), 1.0);
    }

    #[test]
    fn a_software_retry_is_marked_and_phase_tracks_it() {
        let progress = ExportProgress::new(1_000_000);
        assert!(!progress.retried());
        progress.mark_retried();
        progress.set_phase(ExportPhase::RetryingSoftware);
        assert_eq!(progress.phase(), ExportPhase::RetryingSoftware);
        assert!(progress.retried());
        progress.reset();
        assert!(progress.retried());
    }
}
