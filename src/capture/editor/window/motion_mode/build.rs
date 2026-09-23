use gtk4::{
    prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, DrawingArea, Entry, Grid, Label,
    Orientation, Overlay, ToggleButton,
};
use std::cell::Cell;
use std::rc::Rc;

use crate::capture::editor::ui_support::EDITOR_TOP_CHROME_HEIGHT;
use crate::i18n::t;
use crate::recording::editor::model::{
    MotionEffectTransformTiming, MotionTextAnimation, MotionTextScope, MotionTimingKind,
    DEFAULT_MOTION_DURATION_SECONDS, DEFAULT_MOTION_SPRING_BOUNCE, DEFAULT_MOTION_TEXT_POS_X,
    DEFAULT_MOTION_TEXT_POS_Y, DEFAULT_MOTION_TEXT_SIZE, DEFAULT_MOTION_ZOOM,
    MAX_MOTION_DURATION_SECONDS, MAX_MOTION_SPRING_BOUNCE, MAX_MOTION_TEXT_SIZE, MAX_MOTION_YAW,
    MAX_MOTION_ZOOM, MAX_ZOOM_EASE_MS, MIN_MOTION_DURATION_SECONDS, MIN_MOTION_SPRING_BOUNCE,
    MIN_MOTION_TEXT_SIZE, MIN_MOTION_YAW, MIN_MOTION_ZOOM, MIN_ZOOM_EASE_MS,
};
use crate::recording::editor::window::tool_sidebar::FillSlider;

use super::anchor_pad::MotionAnchorPad;
use super::appearance::build_motion_appearance_panel;
use super::parts::{
    MotionModeParts, MotionModeShellParts, MotionPanelParts, MotionSharedControlParts,
    MotionTextControlParts, MotionTimelineParts, MotionTransformControlParts,
};
use super::position_pad::MotionPositionPad;
use super::text_pad::MotionTextPad;
use super::watermark::build_motion_watermark_panel;
use super::widgets::{
    angle_slider_row, ease_preset_timing, format_duration_label, position_slider_row,
    span_slider_row, spring_preset_timing, timing_curve_icon,
};
use super::MotionSession;

pub(in crate::capture::editor::window) fn build_motion_mode(
    window: &ApplicationWindow,
    prefers_dark: bool,
    background_padding: f64,
) -> (MotionModeParts, MotionSession) {
    let session = MotionSession::new(prefers_dark, background_padding);

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
    // Zoom preview first: it shows the live thumbnail and the anchor the
    // Scale/Intensity values below act around, so the three read as one zoom
    // control instead of two disconnected ones.
    let anchor_pad = MotionAnchorPad::new();
    transform_section.append(&anchor_pad.widget());
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
    // Duration is the one control both families share, so it sits at the top.
    let ease_value = Label::new(Some("1200ms"));
    ease_value.set_visible(false);
    let ease_slider =
        FillSlider::new_with_value_text(&t("Duration"), |value, _, _| format!("{:.0}ms", value));
    ease_slider.set_range(MIN_ZOOM_EASE_MS as f64, MAX_ZOOM_EASE_MS as f64);
    ease_slider.set_increments(20.0, 100.0);
    ease_slider.set_value(1200.0);
    timing_section.append(&ease_value);
    timing_section.append(&ease_slider.widget());

    // Top row: the two families plus Custom. Each family button applies
    // the curve its icon previews; Custom just discloses the sliders for
    // whichever family is active.
    let timing_mode_row = GtkBox::new(Orientation::Horizontal, 6);
    timing_mode_row.add_css_class("recording-editor-zoom-easing");
    timing_mode_row.set_hexpand(true);
    timing_mode_row.set_homogeneous(true);
    let timing_kind_buttons: Vec<(MotionTimingKind, ToggleButton)> = MotionTimingKind::ALL
        .iter()
        .map(|&kind| {
            (
                kind,
                timing_mode_button(&t(kind.label()), timing_mode_preview(kind)),
            )
        })
        .collect();
    if let Some((_, first)) = timing_kind_buttons.first() {
        for (index, (_, button)) in timing_kind_buttons.iter().enumerate() {
            if index > 0 {
                button.set_group(Some(first));
            }
        }
    }
    for (_, button) in &timing_kind_buttons {
        timing_mode_row.append(button);
    }
    let custom_timing_btn = timing_custom_button(&t("Custom"));
    timing_mode_row.append(&custom_timing_btn);
    timing_section.append(&timing_mode_row);

    let custom_easing_rows = GtkBox::new(Orientation::Vertical, 0);
    let (easing_x1_header, easing_x1_value, easing_x1_slider) =
        span_slider_row(&t("Ease X1"), 0.25, 0.0, 1.0);
    custom_easing_rows.append(&easing_x1_header);
    custom_easing_rows.append(&easing_x1_slider.widget());
    let (easing_y1_header, easing_y1_value, easing_y1_slider) =
        span_slider_row(&t("Ease Y1"), 1.0, 0.0, 1.0);
    custom_easing_rows.append(&easing_y1_header);
    custom_easing_rows.append(&easing_y1_slider.widget());
    let (easing_x2_header, easing_x2_value, easing_x2_slider) =
        span_slider_row(&t("Ease X2"), 0.50, 0.0, 1.0);
    custom_easing_rows.append(&easing_x2_header);
    custom_easing_rows.append(&easing_x2_slider.widget());
    let (easing_y2_header, easing_y2_value, easing_y2_slider) =
        span_slider_row(&t("Ease Y2"), 1.0, 0.0, 1.0);
    custom_easing_rows.append(&easing_y2_header);
    custom_easing_rows.append(&easing_y2_slider.widget());
    custom_easing_rows.set_visible(false);
    timing_section.append(&custom_easing_rows);

    let custom_spring_rows = GtkBox::new(Orientation::Vertical, 0);
    let (spring_bounce_header, spring_bounce_value, spring_bounce_slider) = span_slider_row(
        &t("Bounce"),
        DEFAULT_MOTION_SPRING_BOUNCE,
        MIN_MOTION_SPRING_BOUNCE,
        MAX_MOTION_SPRING_BOUNCE,
    );
    custom_spring_rows.append(&spring_bounce_header);
    custom_spring_rows.append(&spring_bounce_slider.widget());
    custom_spring_rows.set_visible(false);
    timing_section.append(&custom_spring_rows);

    clip_box.append(&timing_section);

    inspector.append(&clip_box);

    // Text is its own tool page rather than a section that appears inside
    // Move: the panel used to swap between Move and Text depending on what
    // the timeline had selected, which made the controls hard to find. As a
    // sibling of Motion/Appearance/Watermark it is reachable whether or not
    // anything is selected, and it is the page a text clip reveals.
    let text_box = GtkBox::new(Orientation::Vertical, 10);
    text_box.add_css_class("editor-inspector-placeholder-shell");
    text_box.add_css_class("editor-motion-inspector");
    text_box.set_hexpand(false);
    text_box.set_vexpand(false);
    let text_title = Label::new(Some(&t("Text")));
    text_title.add_css_class("editor-inspector-title");
    text_title.set_xalign(0.0);
    text_box.append(&text_title);

    // Empty state: the page is reachable with nothing selected, so it offers
    // the same add action the timeline does instead of a blank panel.
    let text_empty_box = GtkBox::new(Orientation::Vertical, 8);
    let text_empty_hint = Label::new(Some(&t("Select a text clip, or add one at the playhead")));
    text_empty_hint.add_css_class("editor-select-inspector-hint");
    text_empty_hint.set_wrap(true);
    text_empty_hint.set_xalign(0.0);
    text_empty_hint.set_max_width_chars(22);
    text_empty_box.append(&text_empty_hint);
    let text_add_btn = Button::with_label(&t("Add Text"));
    text_add_btn.set_has_frame(false);
    text_add_btn.add_css_class("editor-sidebar-action-button");
    text_empty_box.append(&text_add_btn);
    text_box.append(&text_empty_box);

    let text_editor_box = GtkBox::new(Orientation::Vertical, 10);
    let text_content_section = motion_settings_section("Content");
    let text_entry = Entry::new();
    text_entry.set_placeholder_text(Some(&t("Title")));
    text_entry.set_hexpand(true);
    text_content_section.append(&text_entry);
    text_editor_box.append(&text_content_section);
    let text_placement_section = motion_settings_section("Placement");
    // The pad replaces the old X/Y sliders: dragging the puck is the same
    // gesture as dragging the title on the preview, so the panel and the
    // canvas teach each other. The readout keeps the old percentages
    // available for anyone who wants the exact value.
    let text_pos_pad = MotionTextPad::new();
    text_placement_section.append(&text_pos_pad.widget());
    let text_pos_readout = Label::new(Some(&motion_text_pos_readout(
        DEFAULT_MOTION_TEXT_POS_X,
        DEFAULT_MOTION_TEXT_POS_Y,
    )));
    text_pos_readout.add_css_class("recording-editor-zoom-kicker");
    text_pos_readout.set_xalign(0.0);
    text_placement_section.append(&text_pos_readout);
    let (text_size_header, text_size_value, text_size_slider) = span_slider_row(
        &t("Size"),
        DEFAULT_MOTION_TEXT_SIZE,
        MIN_MOTION_TEXT_SIZE,
        MAX_MOTION_TEXT_SIZE,
    );
    text_placement_section.append(&text_size_header);
    text_placement_section.append(&text_size_slider.widget());
    text_editor_box.append(&text_placement_section);
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
    text_editor_box.append(&text_animation_section);
    // The page owns its Delete: the shared button lives on Move, so deleting
    // the selected title must not require switching pages first.
    let text_delete_btn = Button::with_label(&t("Delete"));
    text_delete_btn.set_has_frame(false);
    text_delete_btn.add_css_class("editor-sidebar-action-button");
    text_editor_box.append(&text_delete_btn);
    text_editor_box.set_visible(false);
    text_box.append(&text_editor_box);

    let delete_btn = Button::with_label(&t("Delete"));
    delete_btn.set_has_frame(false);
    delete_btn.add_css_class("editor-sidebar-action-button");
    delete_btn.set_sensitive(false);
    inspector.append(&delete_btn);

    let appearance_inspector = build_motion_appearance_panel(window, &session, &preview, None);
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
                undo_btn: timeline.undo_btn,
                redo_btn: timeline.redo_btn,
                playhead_clock: timeline.playhead_clock,
                duration_clock: timeline.duration_clock,
                timeline_card: timeline.dock,
                ruler: timeline.ruler,
                source_track: timeline.source_track,
                motion_track: timeline.track,
                text_track: timeline.text_track,
                playhead_overlay: timeline.playhead,
                hover_playhead: timeline.hover_playhead,
                playhead_dragging: timeline.playhead_dragging,
                playhead_hovered: timeline.playhead_hovered,
            },
            panels: MotionPanelParts {
                inspector,
                appearance_inspector,
                watermark_inspector,
                text_inspector: text_box.clone(),
            },
            shared: MotionSharedControlParts {
                duration_slider,
                duration_value,
                blur_slider,
                blur_value,
                blur_shutter_slider,
                blur_shutter_value,
                clip_hint,
                delete_btn,
                inspector_syncing,
            },
            text: MotionTextControlParts {
                text_empty_box,
                text_editor_box,
                text_add_btn,
                text_delete_btn,
                text_entry,
                text_pos_pad,
                text_pos_readout,
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
                anchor_pad,
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
                timing_kind_buttons,
                custom_timing_btn,
                custom_easing_rows,
                custom_spring_rows,
                spring_bounce_slider,
                spring_bounce_value,
                easing_x1_slider,
                easing_x1_value,
                easing_y1_slider,
                easing_y1_value,
                easing_x2_slider,
                easing_x2_value,
                easing_y2_slider,
                easing_y2_value,
            },
        },
        session,
    )
}

/// Top-row button: curve icon left of its name, matching the family buttons.
fn timing_mode_button(label: &str, timing: MotionEffectTransformTiming) -> ToggleButton {
    let button = ToggleButton::new();
    button.add_css_class("recording-editor-zoom-easing-btn");
    button.set_has_frame(false);
    button.set_hexpand(true);
    let content = GtkBox::new(Orientation::Horizontal, 6);
    content.set_halign(gtk4::Align::Center);
    content.append(&timing_curve_icon(timing, 30, 20));
    content.append(&Label::new(Some(label)));
    button.set_child(Some(&content));
    button
}

/// Custom card: text only, stretched to the preset cards' size by the row.
fn timing_custom_button(label: &str) -> ToggleButton {
    let button = ToggleButton::new();
    button.add_css_class("recording-editor-zoom-easing-btn");
    button.set_has_frame(false);
    button.set_hexpand(true);
    button.set_child(Some(&Label::new(Some(label))));
    button
}

fn timing_mode_preview(kind: MotionTimingKind) -> MotionEffectTransformTiming {
    let base = MotionEffectTransformTiming::default();
    match kind {
        // Ease is the classic S (ease-in-out); Spring is Gentle. Each icon
        // is the curve its button applies.
        MotionTimingKind::Ease => ease_preset_timing(0, base),
        MotionTimingKind::Spring => spring_preset_timing(1, base),
    }
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

/// Placement read-out for the text pad. The pad is the control; this is the
/// exact value it lands on, in the same percentages the X/Y sliders showed
/// before they were replaced.
pub(super) fn motion_text_pos_readout(pos_x: f64, pos_y: f64) -> String {
    format!("X {:.0}%  ·  Y {:.0}%", pos_x * 100.0, pos_y * 100.0)
}
