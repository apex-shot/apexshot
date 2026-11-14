//! Studio requirements for the Windows video export's rendered zooms.

use super::export::ExportModel;
use super::zoom::ZoomCameraSettings;
use crate::studio_entitlement::StudioCapabilities;

/// The composition length the export model alone describes: its kept spans end
/// to end at their speeds, plus the held tail. The editor's timeline also
/// knows about the gaps between pieces pulled apart, so its gate passes that
/// real length through [`capabilities_at`] instead.
pub(super) fn composition_seconds(model: &ExportModel) -> f64 {
    let start = model.trim_start.max(0.0);
    let end = model.trim_end.max(start);
    let mut cuts: Vec<f64> = model
        .cuts
        .iter()
        .copied()
        .filter(|cut| cut.is_finite() && *cut > start + 1e-9 && *cut < end - 1e-9)
        .collect();
    cuts.sort_by(f64::total_cmp);
    let mut total = 0.0;
    let mut previous = start;
    for (index, boundary) in cuts.iter().copied().chain(std::iter::once(end)).enumerate() {
        let speed = model
            .segment_speeds
            .get(index)
            .copied()
            .filter(|speed| speed.is_finite() && *speed > 0.0)
            .unwrap_or(1.0);
        total += (boundary - previous).max(0.0) / speed;
        previous = boundary;
    }
    total + model.freeze_tail.max(0.0)
}

/// Studio requirements for an export of `model`. The export's own last check
/// has only the model, so it measures the composition the file itself writes.
pub(super) fn capabilities(model: &ExportModel) -> StudioCapabilities {
    capabilities_at(model, composition_seconds(model))
}

/// Studio requirements measured against the timeline's real composition
/// length: the gaps between moved pieces and the held tail both count, so a
/// zoom is judged by when it actually renders.
pub(super) fn capabilities_at(
    model: &ExportModel,
    composition_seconds: f64,
) -> StudioCapabilities {
    let duration = composition_seconds.max(0.0);
    let settings = model.zoom_camera.clamped();
    let defaults = ZoomCameraSettings::default();
    let mut instant = false;
    let mut animated = false;
    let mut early = false;
    for clip in &model.zooms {
        if clip.hidden
            || !clip.start.is_finite()
            || !clip.end.is_finite()
            || clip.end <= clip.start
            || !clip.scale.is_finite()
            || clip.scale <= 1.0 + 1e-7
        {
            continue;
        }
        let visible = clip.start < duration && clip.end > 0.0;
        instant |= visible && clip.instant;
        animated |= visible && !clip.instant;
        early |= !clip.instant
            && settings.start_early
            && clip.start > 0.0
            && clip.start - settings.lead_in_seconds() < duration;
    }
    let changed_camera = (settings.speed - defaults.speed).abs() > 1e-7
        || (settings.smoothness - defaults.smoothness).abs() > 1e-7
        || (settings.motion_blur - defaults.motion_blur).abs() > 1e-7
        || early;
    StudioCapabilities {
        advanced_zoom: instant || ((animated || early) && changed_camera),
        ..StudioCapabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::model::CursorSettings;
    use crate::video_editor::timeline::{ZoomClip, ZoomMode};

    fn model() -> ExportModel {
        ExportModel {
            source: "recording.mp4".into(),
            source_size: (100, 100),
            trim_start: 0.0,
            trim_end: 10.0,
            cuts: vec![],
            segment_starts: Vec::new(),
            segment_speeds: Vec::new(),
            freeze_tail: 0.0,
            zooms: vec![ZoomClip {
                start: 2.0,
                end: 4.0,
                scale: 2.0,
                center: (50.0, 50.0),
                hidden: false,
                mode: ZoomMode::Manual,
                instant: false,
                anchor: None,
            }],
            zoom_camera: ZoomCameraSettings::default(),
            cursor: CursorSettings::default(),
            sidecar: None,
            webcam: None,
            stage: None,
            canvas: None,
            crf: 23,
            audio: Vec::new(),
            effects: Default::default(),
        }
    }

    #[test]
    fn only_visible_nondefault_camera_and_instant_zooms_need_studio() {
        let mut model = model();
        assert!(!capabilities(&model).requires_studio());
        model.zoom_camera.speed = 2.0;
        assert!(capabilities(&model).advanced_zoom);
        model.zooms[0].hidden = true;
        assert!(!capabilities(&model).requires_studio());
        model.zooms[0].hidden = false;
        model.zooms[0].start = 12.0;
        model.zooms[0].end = 14.0;
        assert!(!capabilities(&model).requires_studio());
        model.zooms[0].start = 2.0;
        model.zoom_camera = ZoomCameraSettings::default();
        model.zooms[0].instant = true;
        assert!(capabilities(&model).advanced_zoom);
        model.zooms[0].scale = 1.0;
        assert!(!capabilities(&model).requires_studio());
    }

    #[test]
    fn lead_in_at_end_of_composition_requires_studio() {
        let mut model = model();
        model.zooms[0].start = 10.2;
        model.zooms[0].end = 12.0;
        model.zoom_camera.start_early = true;
        assert!(capabilities(&model).advanced_zoom);
        model.zooms[0].start = 10.5;
        assert!(!capabilities(&model).requires_studio());
    }

    #[test]
    fn a_nondefault_motion_blur_counts_as_a_changed_camera() {
        let mut model = model();
        assert!(!capabilities(&model).requires_studio());
        model.zoom_camera.motion_blur = 0.5;
        assert!(capabilities(&model).advanced_zoom);
    }

    #[test]
    fn the_model_measures_sped_spans_and_the_held_tail() {
        let mut model = model();
        model.cuts = vec![5.0];
        model.segment_speeds = vec![1.0, 2.0];
        model.freeze_tail = 1.5;
        // 5s at 1x, 5s of source at 2x, then the held tail.
        assert!((composition_seconds(&model) - 9.0).abs() < 1e-9);
    }

    #[test]
    fn a_zoom_past_a_sped_up_composition_is_not_visible() {
        let mut model = model();
        model.cuts = vec![5.0];
        model.segment_speeds = vec![1.0, 2.0];
        model.zooms[0].start = 8.0;
        model.zooms[0].end = 9.0;
        model.zoom_camera.speed = 2.0;
        // The composition is 7.5s long, so the zoom never renders.
        assert!(!capabilities_at(&model, 7.5).advanced_zoom);
        // A longer composition would show it.
        assert!(capabilities_at(&model, 10.0).advanced_zoom);
    }

    #[test]
    fn a_gap_between_moved_pieces_extends_the_visible_window() {
        let mut model = model();
        model.cuts = vec![5.0];
        model.zooms[0].start = 10.5;
        model.zooms[0].end = 11.5;
        model.zoom_camera.speed = 2.0;
        // The model lays the pieces end to end, so it stops at 10s.
        assert!(!capabilities(&model).advanced_zoom);
        // The timeline moved the second piece to 7s, so the composition is 12s.
        assert!(capabilities_at(&model, 12.0).advanced_zoom);
    }
}
