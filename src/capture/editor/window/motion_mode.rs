//! Static ↔ Motion chrome, still preview, and default cinematic move.
//!
//! Motion is a mode of the image editor window, not a second app. Annotate
//! tools stay on Static. Motion plays a composited snapshot; the PNG sidecar
//! is not flattened until Done.

mod anchor_pad;
mod appearance;
mod build;
mod controls;
mod parts;
mod position_pad;
mod preview;
mod session;
mod text_pad;
mod transition;
mod watermark;
mod widgets;

pub(super) use appearance::build_motion_appearance_panel;
pub(super) use build::build_motion_mode;
pub(super) use controls::wire_motion_controls;
pub(super) use parts::MotionModeParts;
use preview::draw_motion_preview;
pub(super) use session::{MotionHoverTrack, MotionRuntime, MotionSession};
pub(super) use transition::{
    apply_editor_mode, install_confirm_overlay, request_enter_motion, request_leave_motion,
    show_motion_tool_page, MotionModeChrome,
};
pub(super) const MOTION_PAGE: &str = "motion";
pub(super) const STATIC_PAGE: &str = "static";
/// Inspector stack pages owned by the Motion tool notch, in notch order.
pub(super) const TEXT_PAGE: &str = "motion-text";
pub(super) const APPEARANCE_PAGE: &str = "motion-appearance";
pub(super) const WATERMARK_PAGE: &str = "motion-watermark";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::state::EditorState;
    use crate::capture::editor::types::{AnnotationAction, Point};
    use crate::recording::editor::model::{MotionBackgroundFillType, MotionState};
    use image::RgbaImage;

    fn blank_state() -> EditorState {
        let image = RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 255]));
        EditorState::new(image)
    }

    #[test]
    fn snapshot_is_required_when_annotations_exist() {
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
        assert!(!transition::annotations_need_snapshot(&state));
    }

    #[test]
    fn motion_page_names_match_chrome_stacks() {
        assert_eq!(MOTION_PAGE, "motion");
        assert_eq!(STATIC_PAGE, "static");
    }

    #[test]
    fn default_duration_matches_still_length() {
        assert!((MotionState::default().duration - 6.0).abs() < f64::EPSILON);
    }

    #[test]
    fn entering_motion_starts_with_an_empty_track() {
        let state = blank_state();
        let session = MotionSession::new(true, 0.0);
        session.capture_snapshot(&state);
        // Motion shows the hint and waits for a click or drag; nothing plays
        // until the user adds a clip.
        assert!(!session.has_segments());
        let identity = session.runtime.borrow().motion.sample(1.5);
        assert!((identity.scale - 1.0).abs() < 1e-6);
        // Empty track stays perfectly flat so the still matches Static:
        // no perspective warp, no rotation, no shadow lift.
        assert!(identity.perspective.abs() < 1e-9);
        assert!(identity.rotation_x.abs() < 1e-9);
        assert!(identity.rotation_y.abs() < 1e-9);
        assert!(identity.rotation_z.abs() < 1e-9);
    }

    #[test]
    fn background_tool_opens_at_zero_for_fresh_images() {
        let session = MotionSession::new(true, 0.0);
        let padding = {
            let runtime = session.runtime.borrow();
            runtime.motion.appearance.background_padding
        };
        assert!(
            padding.abs() < f64::EPSILON,
            "a fresh image should open with 0px padding"
        );
    }

    #[test]
    fn background_tool_restores_the_images_own_padding() {
        let session = MotionSession::new(true, 48.0);
        let padding = {
            let runtime = session.runtime.borrow();
            runtime.motion.appearance.background_padding
        };
        assert!(
            (padding - 48.0).abs() < f64::EPSILON,
            "reopening an edited image should restore its saved padding"
        );
    }

    #[test]
    fn entering_motion_starts_with_no_background_so_it_matches_static() {
        let session = MotionSession::new(true, 0.0);
        let runtime = session.runtime.borrow();
        // Single shared background: a fresh image with no Static fill must
        // not gain a wallpaper in Motion. Users pick a fill explicitly.
        assert_eq!(
            runtime.motion.appearance.background_fill_type,
            MotionBackgroundFillType::None
        );
    }

    #[test]
    fn motion_snapshot_is_background_free_so_no_second_layer_stacks() {
        use crate::capture::editor::types::{BackgroundStyle, DrawColor};
        let mut state = blank_state();
        state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.9, 0.1, 0.1, 1.0));
        state.background_padding = 48.0;
        let card = state.to_motion_card_image().expect("card renders");
        // Background-free: same pixels as the screenshot, not padded canvas.
        assert_eq!(card.dimensions(), state.working_image.dimensions());
        let snapshot_state = state.to_final_image().expect("final renders");
        assert!(
            snapshot_state.dimensions() != card.dimensions() || snapshot_state != card,
            "final image must contain the background the motion card excludes"
        );
    }

    #[test]
    fn entering_motion_preserves_the_static_background_instead_of_doubling() {
        use crate::capture::editor::types::{BackgroundStyle, DrawColor};
        let mut state = blank_state();
        state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.1, 0.2, 0.9, 1.0));
        state.background_padding = 32.0;
        let session = MotionSession::new(true, 0.0);
        // Startup seeds the shared runtime once (see setup_editor_window_full);
        // entry itself must preserve it, not reseed through lossy converters.
        {
            let mut runtime = session.runtime.borrow_mut();
            runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
            runtime.motion.appearance.background_color = [0.1, 0.2, 0.9, 1.0];
            runtime.motion.appearance.background_padding = 32.0;
        }
        session.capture_snapshot(&state);
        let runtime = session.runtime.borrow();
        // Single shared background: the seeded fill survives entry.
        assert_eq!(
            runtime.motion.appearance.background_fill_type,
            MotionBackgroundFillType::Color
        );
        assert!((runtime.motion.appearance.background_padding - 32.0).abs() < f64::EPSILON);
        // Card itself carries no background padding.
        let snapshot = runtime.snapshot.as_ref().expect("snapshot");
        assert_eq!(snapshot.dimensions(), state.working_image.dimensions());
    }

    #[test]
    fn entering_motion_keeps_motion_frame_edits_instead_of_reseeding() {
        use crate::recording::editor::model::MotionFramePreset;
        let state = blank_state();
        let session = MotionSession::new(true, 0.0);
        // User picks a Frame in Motion (e.g. 16:9); re-entering must keep it
        // instead of overwriting it from Static through lossy crop mapping
        // (Custom/social presets never round-trip).
        {
            let mut runtime = session.runtime.borrow_mut();
            runtime.motion.frame.preset = MotionFramePreset::SixteenNine;
            runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
        }
        session.capture_snapshot(&state);
        // Simulate a second entry without leaving: frame must survive.
        session.capture_snapshot(&state);
        let runtime = session.runtime.borrow();
        assert_eq!(runtime.motion.frame.preset, MotionFramePreset::SixteenNine);
        assert_eq!(
            runtime.motion.appearance.background_fill_type,
            MotionBackgroundFillType::Color
        );
    }

    #[test]
    fn entering_motion_with_no_static_background_adds_no_fill() {
        let state = blank_state();
        let session = MotionSession::new(true, 0.0);
        session.capture_snapshot(&state);
        let runtime = session.runtime.borrow();
        // Fresh images stay on None so Motion looks like Static instead of
        // gaining a wallpaper Static never had.
        assert_eq!(
            runtime.motion.appearance.background_fill_type,
            MotionBackgroundFillType::None
        );
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

    /// Text placement is a 2D pad, not two percentage sliders: the pad is the
    /// control the preview drag and the timeline selection both drive.
    #[test]
    fn text_placement_is_a_pad_rather_than_x_y_sliders() {
        let build = include_str!("motion_mode/build.rs");
        let controls = include_str!("motion_mode/controls/text.rs");
        let sync = include_str!("motion_mode/controls/sync.rs");

        assert!(
            build.contains("let text_pos_pad = MotionTextPad::new()")
                && build.contains("text_placement_section.append(&text_pos_pad.widget())")
                && build.contains("motion_text_pos_readout")
                && controls.contains("text_pos_pad.connect_value_changed")
                && controls.contains("text_pos_pad.set_text_pos(pos_x, pos_y)")
                && sync.contains("text_pos_pad.set_text_pos(segment.pos_x, segment.pos_y)")
                && !build.contains("span_slider_row(\n        &t(\"X\")")
                && !sync.contains("text_pos_x_slider"),
            "Text placement should be one 2D pad with a readout, not X/Y sliders"
        );
    }

    /// Text is its own Motion tool page. It used to be a section inside Move
    /// that appeared only while a text clip was selected, which is what made
    /// it hard to find.
    #[test]
    fn text_has_its_own_tool_page_beside_the_other_motion_tools() {
        let host = include_str!("motion_host.rs");
        let inspectors = include_str!("inspectors/mod.rs");
        let build = include_str!("motion_mode/build.rs");
        let transition = include_str!("motion_mode/transition.rs");

        assert!(
            inspectors.contains("let text_tab_btn = notch_btn(\"Text\",")
                && inspectors.contains("motion_tabs.append(&text_tab_btn);")
                && inspectors.contains("Some(\"motion-text\")")
                && host.contains("(text_tab_btn.clone(), \"motion-text\")")
                && build.contains("text_inspector: text_box.clone()")
                && transition.contains("TEXT_PAGE")
                && transition
                    .contains("pub(in crate::capture::editor::window) fn show_motion_tool_page"),
            "Text should be a notch page like Motion/Appearance/Watermark, not a Move sub-section"
        );
        // Reached the same way every other Motion tool is, so the highlighted
        // notch and the visible panel cannot drift apart.
        assert!(
            host.contains("motion_mode::show_motion_tool_page(&chrome, page);")
                || host.contains("show_motion_tool_page"),
            "notch clicks should route through the shared page/active-state helper"
        );
    }

    /// A text clip is its own tool page's content: selecting one reveals Text,
    /// and the page never hides — with nothing selected it offers the add
    /// action, which is why deselecting does not bounce the user elsewhere.
    #[test]
    fn selecting_a_text_clip_reveals_the_text_page_once() {
        let sync = include_str!("motion_mode/controls/sync.rs");
        assert!(
            sync.contains("last_had_text.replace(has_text)")
                && sync.contains("show_motion_tool_page(&chrome, super::super::TEXT_PAGE)")
                && sync.contains("text_empty_box.set_visible(!has_text)")
                && sync.contains("text_editor_box.set_visible(has_text)"),
            "a text selection should reveal the Text page on the transition only, and the page \
             should show an empty state instead of hiding"
        );
    }
}
