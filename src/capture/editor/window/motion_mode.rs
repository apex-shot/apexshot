//! Static ↔ Motion chrome, still preview, and default cinematic move.
//!
//! Motion is a mode of the image editor window, not a second app. Annotate
//! tools stay on Static. Motion plays a composited snapshot; the PNG sidecar
//! is not flattened until Done.

mod appearance;
mod build;
mod controls;
mod parts;
mod position_pad;
mod preview;
mod session;
mod transition;
mod watermark;
mod widgets;

pub(super) use build::build_motion_mode;
pub(super) use controls::wire_motion_controls;
pub(super) use parts::MotionModeParts;
use preview::draw_motion_preview;
pub(super) use session::{MotionHoverTrack, MotionRuntime, MotionSession};
pub(super) use transition::{
    apply_editor_mode, install_confirm_overlay, request_enter_motion, request_leave_motion,
    MotionModeChrome,
};
pub(super) const MOTION_PAGE: &str = "motion";
pub(super) const STATIC_PAGE: &str = "static";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::state::EditorState;
    use crate::capture::editor::types::{AnnotationAction, Point, Rect};
    use crate::recording::editor::model::MotionState;
    use image::RgbaImage;

    fn blank_state() -> EditorState {
        let image = RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 255]));
        EditorState::new(image)
    }

    #[test]
    fn snapshot_is_required_when_annotations_or_crop_exist() {
        let mut state = blank_state();
        assert!(!transition::annotations_need_snapshot(&state));
        state.actions.push(AnnotationAction::Line {
            start: Point { x: 0.0, y: 0.0 },
            end: Point { x: 4.0, y: 4.0 },
            color: crate::capture::editor::types::DrawColor::new(1.0, 1.0, 1.0, 1.0),
            stroke_size: 2.0,
            shadow: false,
        });
        assert!(transition::annotations_need_snapshot(&state));
        state.actions.clear();
        state.crop_selection = Some(Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        });
        assert!(transition::annotations_need_snapshot(&state));
    }

    #[test]
    fn motion_page_names_match_chrome_stacks() {
        assert_eq!(MOTION_PAGE, "motion");
        assert_eq!(STATIC_PAGE, "static");
    }

    #[test]
    fn default_duration_matches_shotbase_still_length() {
        assert!((MotionState::default().duration - 6.0).abs() < f64::EPSILON);
    }

    #[test]
    fn entering_motion_starts_with_an_empty_track() {
        let state = blank_state();
        let session = MotionSession::new(true);
        session.capture_snapshot(&state);
        // Shotbase shows the hint and waits for a click or drag; nothing plays
        // until the user adds a clip.
        assert!(!session.has_segments());
        let identity = session.runtime.borrow().motion.sample(1.5);
        assert!((identity.scale - 1.0).abs() < 1e-6);
    }

    #[test]
    fn appearance_background_picker_keeps_choices_in_one_expanding_card() {
        let source = include_str!("motion_mode/appearance.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            source.contains("fill_section.add_css_class(\"editor-motion-background-picker\")")
                && source.contains("fill_section.append(&selection_stack)")
                && source.contains("selection_stack.set_visible_child_name(\"wallpapers\")")
        );
    }

    #[test]
    fn gradient_background_uses_one_shared_color_input() {
        let source = include_str!("motion_mode/appearance.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            source.contains(
                "motion_gradient_color_control(initial_gradient_start, initial_gradient_end"
            ) && !source.contains("let gradient_start = motion_color_control")
                && !source.contains("let gradient_end = motion_color_control"),
            "Motion gradients should use a shared hex input for both selectable stops"
        );
    }

    #[test]
    fn motion_scale_uses_a_multiplier_slider_instead_of_presets() {
        let source = include_str!("motion_mode/build.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            source.contains("FillSlider::new_with_value_text(&t(\"Scale\")")
                && source.contains("format!(\"{value:.1}x\")")
                && !source.contains("MOTION_SCALE_PRESETS"),
            "Motion Scale should be a multiplier slider rather than a preset grid"
        );
    }

    #[test]
    fn position_pad_and_sliders_share_a_position_section() {
        let build = include_str!("motion_mode/build.rs");
        let sync = include_str!("motion_mode/controls/sync.rs");
        let controls = include_str!("motion_mode/controls/transform.rs");
        let widgets = include_str!("motion_mode/widgets.rs");

        assert!(
            build.contains("let position_section")
                && build.contains("motion_settings_section(\"Position\")")
                && build.contains("position_section.append(&position_pad.widget())")
                && build.contains("position_slider_row(&t(\"X\"), 0.0)")
                && build.contains("position_slider_row(&t(\"Y\"), 0.0)")
                && widgets.contains("value * 1000.0")
                && sync.contains("position_pad.set_position(segment.to.pos_x, segment.to.pos_y)")
                && controls.contains("position_pad.connect_value_changed"),
            "Position should keep the direct pad and 3D-unit X/Y sliders visibly grouped and synchronized"
        );
    }
}
