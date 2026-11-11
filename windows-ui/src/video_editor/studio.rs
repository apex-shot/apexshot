//! Studio requirements for the Windows video export's rendered zooms.

use super::export::ExportModel;
use super::zoom::ZoomCameraSettings;
use crate::studio_entitlement::StudioCapabilities;

pub(super) fn capabilities(model: &ExportModel) -> StudioCapabilities {
    let duration = (model.trim_end - model.trim_start).max(0.0);
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
}
