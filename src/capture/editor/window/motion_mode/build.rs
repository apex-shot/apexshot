use gtk4::{
    prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, DrawingArea, Entry, Grid, Label,
    Orientation, Overlay, ToggleButton,
};
use std::cell::Cell;
use std::rc::Rc;

use crate::capture::editor::ui_support::EDITOR_TOP_CHROME_HEIGHT;
use crate::i18n::t;
use crate::recording::editor::model::{
    MotionTextAnimation, MotionTextScope, DEFAULT_MOTION_DURATION_SECONDS,
    DEFAULT_MOTION_TEXT_POS_X, DEFAULT_MOTION_TEXT_POS_Y, DEFAULT_MOTION_TEXT_SIZE,
    DEFAULT_MOTION_ZOOM, MAX_MOTION_DURATION_SECONDS, MAX_MOTION_TEXT_POS, MAX_MOTION_TEXT_SIZE,
    MAX_MOTION_YAW, MAX_MOTION_ZOOM, MAX_ZOOM_EASE_MS, MIN_MOTION_DURATION_SECONDS,
    MIN_MOTION_TEXT_POS, MIN_MOTION_TEXT_SIZE, MIN_MOTION_YAW, MIN_MOTION_ZOOM, MIN_ZOOM_EASE_MS,
};
use crate::recording::editor::window::tool_sidebar::FillSlider;

use super::appearance::build_motion_appearance_panel;
use super::parts::{
    MotionModeParts, MotionModeShellParts, MotionPanelParts, MotionSharedControlParts,
    MotionTextControlParts, MotionTimelineParts, MotionTransformControlParts,
};
use super::position_pad::MotionPositionPad;
use super::watermark::build_motion_watermark_panel;
use super::widgets::{
    angle_slider_row, format_duration_label, position_slider_row, span_slider_row,
};
use super::MotionSession;

pub(in crate::capture::editor::window) fn build_motion_mode(
    window: &ApplicationWindow,
    prefers_dark: bool,
) -> (MotionModeParts, MotionSession) {
    let session = MotionSession::new(prefers_dark);

    let static_toolbar = GtkBox::new(Orientation::Horizontal, 0);
    static_toolbar.add_css_class("editor-toolbar");
    static_toolbar.set_halign(Align::Center);
    static_toolbar.set_valign(Align::Start);

    let static_btn = Button::with_label(&t("Static"));
    static_btn.set_has_frame(false);
    static_btn.set_focusable(false);
    static_btn.add_css_class("editor-tool-button");
    static_btn.add_css_class("editor-mode-label-button");
    static_btn.set_tooltip_text(Some(&t("Switch to Static")));
    static_toolbar.append(&static_btn);

    let motion_btn = Button::with_label(&t("Motion"));
    motion_btn.set_has_frame(false);
    motion_btn.set_focusable(false);
    motion_btn.add_css_class("editor-footer-zoom-button");
    motion_btn.add_css_class("editor-mode-label-button");
    motion_btn.set_tooltip_text(Some(&t("Switch to Motion")));

    let preview = DrawingArea::new();
    preview.set_hexpand(true);
    preview.set_vexpand(true);
    preview.add_css_class("editor-canvas");
    preview.add_css_class("editor-motion-preview");
    // Reserve overlay chrome in GTK layout, not in renderer coordinates. This
    // keeps Position and Zoom Anchor math independent of the surrounding UI.
    preview.set_margin_top(EDITOR_TOP_CHROME_HEIGHT);
    preview.set_margin_end(64);

    let timeline = super::super::motion_timeline::build_motion_timeline(session.runtime.clone());

    let preview_backdrop = DrawingArea::new();
    preview_backdrop.set_hexpand(true);
    preview_backdrop.set_vexpand(true);
    preview_backdrop.set_can_target(false);
    preview_backdrop.set_draw_func(move |_, context, width, height| {
        crate::capture::editor::render::draw_canvas_checkerboard_background(
            context,
            width,
            height,
            None,
            !prefers_dark,
        );
    });

    let preview_shell = Overlay::new();
    preview_shell.set_hexpand(true);
    preview_shell.set_vexpand(true);
    preview_shell.set_child(Some(&preview_backdrop));
    preview_shell.add_overlay(&preview);
    preview_shell.set_clip_overlay(&preview, true);

    let page = GtkBox::new(Orientation::Vertical, 0);
    page.set_hexpand(true);
    page.set_vexpand(true);
    page.set_focusable(true);
    page.add_css_class("editor-motion-page");
    page.append(&preview_shell);
    page.append(&timeline.dock);

    let duration_value = Label::new(Some(&format_duration_label(
        DEFAULT_MOTION_DURATION_SECONDS,
    )));
    duration_value.add_css_class("editor-inspector-title");
    duration_value.set_xalign(0.0);

    let duration_slider =
        FillSlider::new_with_value_text(&t("Duration"), |value, _, _| format_duration_label(value));
    duration_slider.set_range(MIN_MOTION_DURATION_SECONDS, MAX_MOTION_DURATION_SECONDS);
    duration_slider.set_increments(0.5, 1.0);
    duration_slider.set_value(DEFAULT_MOTION_DURATION_SECONDS);

    let inspector = GtkBox::new(Orientation::Vertical, 12);
    inspector.add_css_class("editor-inspector-placeholder-shell");
    inspector.add_css_class("editor-motion-inspector");
    inspector.set_hexpand(false);
    inspector.set_vexpand(false);
    let composition_section = motion_settings_section("Composition");
    duration_value.set_visible(false);
    composition_section.append(&duration_value);
    composition_section.append(&duration_slider.widget());
    inspector.append(&composition_section);

    let motion_blur_section = motion_settings_section("Motion Blur");
    let blur_value = Label::new(Some("0%"));
    let blur_slider = FillSlider::new(&t("Motion blur"));
    blur_slider.set_range(0.0, 1.0);
    blur_slider.set_increments(0.01, 0.1);
    blur_slider.set_value(0.0);
    motion_blur_section.append(&blur_slider.widget());

    let blur_shutter_value = Label::new(Some("180°"));
    let blur_shutter_slider =
        FillSlider::new_with_value_text(&t("Blur shutter"), |value, _, _| format!("{value:.0}°"));
    blur_shutter_slider.set_range(0.0, 360.0);
    blur_shutter_slider.set_increments(5.0, 15.0);
    blur_shutter_slider.set_value(180.0);
    motion_blur_section.append(&blur_shutter_slider.widget());

    let blur_trail_value = Label::new(Some("28%"));
    let blur_trail_slider = FillSlider::new_with_value_text(&t("Blur trail"), |value, _, _| {
        format!("{:.0}%", value * 100.0)
    });
    blur_trail_slider.set_range(0.0, 0.4);
    blur_trail_slider.set_increments(0.01, 0.04);
    blur_trail_slider.set_value(0.28);
    motion_blur_section.append(&blur_trail_slider.widget());
    inspector.append(&motion_blur_section);

    let clip_hint = Label::new(Some(&t(
        "Click a clip, or double-click a track to add a move or text",
    )));
    clip_hint.add_css_class("editor-select-inspector-hint");
    clip_hint.set_wrap(true);
    clip_hint.set_xalign(0.0);
    clip_hint.set_max_width_chars(22);
    clip_hint.set_visible(false);
    inspector.append(&clip_hint);

    let clip_box = GtkBox::new(Orientation::Vertical, 10);
    clip_box.set_hexpand(true);
    let move_title = Label::new(Some(&t("Move")));
    move_title.add_css_class("editor-inspector-title");
    move_title.set_xalign(0.0);
    clip_box.append(&move_title);

    let transform_section = motion_settings_section("Transform");
    let scale_slider =
        FillSlider::new_with_value_text(&t("Scale"), |value, _, _| format!("{value:.1}x"));
    scale_slider.set_range(MIN_MOTION_ZOOM, MAX_MOTION_ZOOM);
    scale_slider.set_increments(0.1, 0.5);
    scale_slider.set_value(DEFAULT_MOTION_ZOOM);
    let inspector_syncing = Rc::new(Cell::new(false));
    transform_section.append(&scale_slider.widget());

    let (intensity_header, intensity_value, intensity_slider) =
        span_slider_row(&t("Intensity"), 1.0, 0.0, 1.0);
    transform_section.append(&intensity_header);
    transform_section.append(&intensity_slider.widget());
    clip_box.append(&transform_section);

    let zoom_anchor_section = motion_settings_section("Zoom Anchor");
    let (zoom_anchor_x_header, zoom_anchor_x_value, zoom_anchor_x_slider) =
        span_slider_row(&t("Anchor X"), 0.5, 0.0, 1.0);
    zoom_anchor_section.append(&zoom_anchor_x_header);
    zoom_anchor_section.append(&zoom_anchor_x_slider.widget());
    let (zoom_anchor_y_header, zoom_anchor_y_value, zoom_anchor_y_slider) =
        span_slider_row(&t("Anchor Y"), 0.5, 0.0, 1.0);
    zoom_anchor_section.append(&zoom_anchor_y_header);
    zoom_anchor_section.append(&zoom_anchor_y_slider.widget());
    clip_box.append(&zoom_anchor_section);

    let rotation_section = motion_settings_section("Rotation & Perspective");
    let yaw_value = Label::new(Some("8°"));
    yaw_value.set_visible(false);
    let yaw_slider =
        FillSlider::new_with_value_text(&t("Yaw"), |value, _, _| format!("{value:.0}°"));
    yaw_slider.set_range(MIN_MOTION_YAW, MAX_MOTION_YAW);
    yaw_slider.set_increments(1.0, 5.0);
    yaw_slider.set_value(8.0);
    rotation_section.append(&yaw_value);
    rotation_section.append(&yaw_slider.widget());

    let (pitch_header, pitch_value, pitch_slider) = angle_slider_row(&t("Pitch"), 0.0);
    rotation_section.append(&pitch_header);
    rotation_section.append(&pitch_slider.widget());
    let (roll_header, roll_value, roll_slider) = angle_slider_row(&t("Roll"), 0.0);
    rotation_section.append(&roll_header);
    rotation_section.append(&roll_slider.widget());

    let perspective_value = Label::new(Some("18%"));
    perspective_value.set_visible(false);
    let perspective_slider = FillSlider::new(&t("Perspective"));
    perspective_slider.set_range(0.0, 1.0);
    perspective_slider.set_increments(0.01, 0.1);
    perspective_slider.set_value(0.18);
    rotation_section.append(&perspective_value);
    rotation_section.append(&perspective_slider.widget());
    clip_box.append(&rotation_section);

    let position_section = motion_settings_section("Position");
    position_section.add_css_class("editor-motion-position-section");
    let position_pad = MotionPositionPad::new();
    position_section.append(&position_pad.widget());
    let (pos_x_header, pos_x_value, pos_x_slider) = position_slider_row(&t("X"), 0.0);
    position_section.append(&pos_x_header);
    position_section.append(&pos_x_slider.widget());
    let (pos_y_header, pos_y_value, pos_y_slider) = position_slider_row(&t("Y"), 0.0);
    position_section.append(&pos_y_header);
    position_section.append(&pos_y_slider.widget());
    clip_box.append(&position_section);

    let timing_section = motion_settings_section("Timing");
    let ease_value = Label::new(Some("1200ms"));
    ease_value.set_visible(false);
    let ease_slider =
        FillSlider::new_with_value_text(&t("Duration"), |value, _, _| format!("{:.0}ms", value));
    ease_slider.set_range(MIN_ZOOM_EASE_MS as f64, MAX_ZOOM_EASE_MS as f64);
    ease_slider.set_increments(20.0, 100.0);
    ease_slider.set_value(1200.0);
    timing_section.append(&ease_value);
    timing_section.append(&ease_slider.widget());

    let (easing_x1_header, easing_x1_value, easing_x1_slider) =
        span_slider_row(&t("Ease X1"), 0.25, 0.0, 1.0);
    timing_section.append(&easing_x1_header);
    timing_section.append(&easing_x1_slider.widget());
    let (easing_y1_header, easing_y1_value, easing_y1_slider) =
        span_slider_row(&t("Ease Y1"), 1.0, 0.0, 1.0);
    timing_section.append(&easing_y1_header);
    timing_section.append(&easing_y1_slider.widget());
    let (easing_x2_header, easing_x2_value, easing_x2_slider) =
        span_slider_row(&t("Ease X2"), 0.50, 0.0, 1.0);
    timing_section.append(&easing_x2_header);
    timing_section.append(&easing_x2_slider.widget());
    let (easing_y2_header, easing_y2_value, easing_y2_slider) =
        span_slider_row(&t("Ease Y2"), 1.0, 0.0, 1.0);
    timing_section.append(&easing_y2_header);
    timing_section.append(&easing_y2_slider.widget());
    let reset_timing_btn = Button::with_label(&t("Reset"));
    reset_timing_btn.add_css_class("recording-editor-zoom-easing-btn");
    reset_timing_btn.set_has_frame(false);
    reset_timing_btn.set_halign(gtk4::Align::Start);
    timing_section.append(&reset_timing_btn);
    clip_box.append(&timing_section);

    // Shotbase's Motion timing is a single global cubic-Bézier curve — there
    // are no named easing presets on the Motion track.
    inspector.append(&clip_box);

    let text_box = GtkBox::new(Orientation::Vertical, 10);
    text_box.set_hexpand(false);
    text_box.set_visible(false);
    let text_title = Label::new(Some(&t("Text")));
    text_title.add_css_class("editor-inspector-title");
    text_title.set_xalign(0.0);
    text_box.append(&text_title);
    let text_content_section = motion_settings_section("Content");
    let text_entry = Entry::new();
    text_entry.set_placeholder_text(Some(&t("Title")));
    text_entry.set_hexpand(true);
    text_content_section.append(&text_entry);
    text_box.append(&text_content_section);
    let text_placement_section = motion_settings_section("Placement");
    let (text_pos_x_header, text_pos_x_value, text_pos_x_slider) = span_slider_row(
        &t("X"),
        DEFAULT_MOTION_TEXT_POS_X,
        MIN_MOTION_TEXT_POS,
        MAX_MOTION_TEXT_POS,
    );
    text_placement_section.append(&text_pos_x_header);
    text_placement_section.append(&text_pos_x_slider.widget());
    let (text_pos_y_header, text_pos_y_value, text_pos_y_slider) = span_slider_row(
        &t("Y"),
        DEFAULT_MOTION_TEXT_POS_Y,
        MIN_MOTION_TEXT_POS,
        MAX_MOTION_TEXT_POS,
    );
    text_placement_section.append(&text_pos_y_header);
    text_placement_section.append(&text_pos_y_slider.widget());
    let (text_size_header, text_size_value, text_size_slider) = span_slider_row(
        &t("Size"),
        DEFAULT_MOTION_TEXT_SIZE,
        MIN_MOTION_TEXT_SIZE,
        MAX_MOTION_TEXT_SIZE,
    );
    text_placement_section.append(&text_size_header);
    text_placement_section.append(&text_size_slider.widget());
    text_box.append(&text_placement_section);
    let text_animation_section = motion_settings_section("Animation");
    let text_anim_grid = Grid::new();
    text_anim_grid.add_css_class("recording-editor-zoom-easing");
    text_anim_grid.set_hexpand(true);
    text_anim_grid.set_column_spacing(6);
    text_anim_grid.set_row_spacing(6);
    text_anim_grid.set_column_homogeneous(true);
    let text_anim_buttons: Vec<(MotionTextAnimation, ToggleButton)> = MotionTextAnimation::ALL
        .iter()
        .enumerate()
        .map(|(index, &animation)| {
            let button = ToggleButton::with_label(&t(animation.label()));
            button.add_css_class("recording-editor-zoom-easing-btn");
            button.set_has_frame(false);
            button.set_hexpand(true);
            text_anim_grid.attach(&button, (index % 3) as i32, (index / 3) as i32, 1, 1);
            (animation, button)
        })
        .collect();
    if let Some((_, first)) = text_anim_buttons.first() {
        for (index, (_, button)) in text_anim_buttons.iter().enumerate() {
            if index > 0 {
                button.set_group(Some(first));
            }
        }
    }
    text_animation_section.append(&text_anim_grid);
    let text_scope_label = Label::new(Some(&t("Scope")));
    text_scope_label.add_css_class("recording-editor-zoom-kicker");
    text_scope_label.set_xalign(0.0);
    text_animation_section.append(&text_scope_label);
    let text_scope_row = GtkBox::new(Orientation::Horizontal, 6);
    text_scope_row.add_css_class("recording-editor-zoom-easing");
    text_scope_row.set_hexpand(true);
    text_scope_row.set_homogeneous(true);
    let text_scope_buttons: Vec<(MotionTextScope, ToggleButton)> = MotionTextScope::ALL
        .iter()
        .map(|&scope| {
            let button = ToggleButton::with_label(&t(scope.label()));
            button.add_css_class("recording-editor-zoom-easing-btn");
            button.set_has_frame(false);
            button.set_hexpand(true);
            text_scope_row.append(&button);
            (scope, button)
        })
        .collect();
    if let Some((_, first)) = text_scope_buttons.first() {
        for (index, (_, button)) in text_scope_buttons.iter().enumerate() {
            if index > 0 {
                button.set_group(Some(first));
            }
        }
    }
    text_animation_section.append(&text_scope_row);
    text_box.append(&text_animation_section);
    inspector.append(&text_box);

    let delete_btn = Button::with_label(&t("Delete"));
    delete_btn.set_has_frame(false);
    delete_btn.add_css_class("editor-sidebar-action-button");
    delete_btn.set_sensitive(false);
    inspector.append(&delete_btn);

    let appearance_inspector = build_motion_appearance_panel(window, &session, &preview);
    let watermark_inspector = build_motion_watermark_panel(window, &session, &preview);

    let confirm_overlay = GtkBox::new(Orientation::Vertical, 0);
    confirm_overlay.add_css_class("editor-motion-confirm-scrim");
    confirm_overlay.set_halign(Align::Fill);
    confirm_overlay.set_valign(Align::Fill);
    confirm_overlay.set_hexpand(true);
    confirm_overlay.set_vexpand(true);
    confirm_overlay.set_visible(false);
    confirm_overlay.set_can_target(true);

    preview.set_draw_func({
        let session_runtime = session.runtime.clone();
        let prefers_dark = session.prefers_dark;
        move |_, context, width, height| {
            super::draw_motion_preview(context, width, height, &session_runtime, prefers_dark);
        }
    });

    (
        MotionModeParts {
            shell: MotionModeShellParts {
                static_toolbar,
                static_btn,
                motion_btn,
                preview,
                preview_shell,
                page,
                confirm_overlay,
            },
            timeline: MotionTimelineParts {
                play_btn: timeline.play_btn,
                skip_back: timeline.skip_back,
                skip_forward: timeline.skip_forward,
                add_btn: timeline.add_btn,
                add_text_btn: timeline.add_text_btn,
                playhead_clock: timeline.playhead_clock,
                duration_clock: timeline.duration_clock,
                timeline_card: timeline.dock,
                ruler: timeline.ruler,
                source_track: timeline.source_track,
                motion_track: timeline.track,
                text_track: timeline.text_track,
                playhead_overlay: timeline.playhead,
            },
            panels: MotionPanelParts {
                inspector,
                appearance_inspector,
                watermark_inspector,
            },
            shared: MotionSharedControlParts {
                duration_slider,
                duration_value,
                blur_slider,
                blur_value,
                blur_shutter_slider,
                blur_shutter_value,
                blur_trail_slider,
                blur_trail_value,
                clip_hint,
                delete_btn,
                inspector_syncing,
            },
            text: MotionTextControlParts {
                text_box,
                text_entry,
                text_pos_x_slider,
                text_pos_x_value,
                text_pos_y_slider,
                text_pos_y_value,
                text_size_slider,
                text_size_value,
                text_anim_buttons,
                text_scope_buttons,
            },
            transform: MotionTransformControlParts {
                clip_box,
                scale_slider,
                intensity_slider,
                intensity_value,
                zoom_anchor_x_slider,
                zoom_anchor_x_value,
                zoom_anchor_y_slider,
                zoom_anchor_y_value,
                yaw_slider,
                yaw_value,
                pitch_slider,
                pitch_value,
                roll_slider,
                roll_value,
                perspective_slider,
                perspective_value,
                position_pad,
                pos_x_slider,
                pos_x_value,
                pos_y_slider,
                pos_y_value,
                ease_slider,
                ease_value,
                easing_x1_slider,
                easing_x1_value,
                easing_y1_slider,
                easing_y1_value,
                easing_x2_slider,
                easing_x2_value,
                easing_y2_slider,
                easing_y2_value,
                reset_timing_btn,
            },
        },
        session,
    )
}

fn motion_settings_section(title: &str) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 8);
    section.add_css_class("editor-motion-settings-section");
    let heading = Label::new(Some(&t(title)));
    heading.add_css_class("editor-background-section-title");
    heading.set_xalign(0.0);
    section.append(&heading);
    section
}
