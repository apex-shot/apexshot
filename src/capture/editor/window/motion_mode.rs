//! Static ↔ Motion chrome, still preview, and default cinematic move.
//!
//! Motion is a mode of the image editor window, not a second app. Annotate
//! tools stay on Static. Motion plays a composited snapshot; the PNG sidecar
//! is not flattened until Done.

use gtk4::cairo::Context;
use gtk4::{
    gdk, glib, prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, CheckButton,
    DrawingArea, Entry, EventControllerKey, EventControllerMotion, GestureClick, GestureDrag, Grid,
    Label, Orientation, Overlay, Stack, ToggleButton,
};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::capture::editor::render::rgba_image_to_surface;
use crate::capture::editor::state::EditorState;
use crate::config::{load_config, save_config};
use crate::i18n::t;
use crate::recording::editor::model::{
    MotionEffectTransformTiming, MotionState, MotionTextAnimation, MotionTextScope,
    DEFAULT_MOTION_DURATION_SECONDS, DEFAULT_MOTION_TEXT_POS_X, DEFAULT_MOTION_TEXT_POS_Y,
    DEFAULT_MOTION_TEXT_SIZE, MAX_MOTION_DURATION_SECONDS, MAX_MOTION_POS, MAX_MOTION_TEXT_POS,
    MAX_MOTION_TEXT_SIZE, MAX_MOTION_YAW, MAX_ZOOM_EASE_MS, MIN_MOTION_DURATION_SECONDS,
    MIN_MOTION_POS, MIN_MOTION_TEXT_POS, MIN_MOTION_TEXT_SIZE, MIN_MOTION_YAW, MIN_ZOOM_EASE_MS,
    MOTION_SCALE_PRESETS,
};
use crate::recording::editor::window::tool_sidebar::FillSlider;

pub(super) const MOTION_PAGE: &str = "motion";
pub(super) const STATIC_PAGE: &str = "static";

pub(super) struct MotionModeParts {
    pub static_toolbar: GtkBox,
    pub static_btn: Button,
    pub motion_btn: Button,
    pub play_btn: Button,
    pub skip_back: Button,
    pub skip_forward: Button,
    pub add_btn: Button,
    pub add_text_btn: Button,
    pub playhead_clock: Label,
    pub duration_clock: Label,
    pub preview: DrawingArea,
    #[allow(dead_code)]
    pub timeline_card: GtkBox,
    pub ruler: DrawingArea,
    pub source_track: DrawingArea,
    pub motion_track: DrawingArea,
    pub text_track: DrawingArea,
    pub playhead_overlay: DrawingArea,
    pub page: GtkBox,
    pub inspector: GtkBox,
    pub duration_slider: FillSlider,
    pub duration_value: Label,
    pub blur_slider: FillSlider,
    pub blur_value: Label,
    pub blur_shutter_slider: FillSlider,
    pub blur_shutter_value: Label,
    pub blur_trail_slider: FillSlider,
    pub blur_trail_value: Label,
    pub clip_box: GtkBox,
    pub text_box: GtkBox,
    pub text_entry: Entry,
    pub text_pos_x_slider: FillSlider,
    pub text_pos_x_value: Label,
    pub text_pos_y_slider: FillSlider,
    pub text_pos_y_value: Label,
    pub text_size_slider: FillSlider,
    pub text_size_value: Label,
    pub text_anim_buttons: Vec<(MotionTextAnimation, ToggleButton)>,
    pub text_scope_buttons: Vec<(MotionTextScope, ToggleButton)>,
    pub clip_hint: Label,
    pub scale_value: Label,
    pub scale_chips: Vec<Button>,
    pub intensity_slider: FillSlider,
    pub intensity_value: Label,
    pub zoom_anchor_x_slider: FillSlider,
    pub zoom_anchor_x_value: Label,
    pub zoom_anchor_y_slider: FillSlider,
    pub zoom_anchor_y_value: Label,
    pub yaw_slider: FillSlider,
    pub yaw_value: Label,
    pub pitch_slider: FillSlider,
    pub pitch_value: Label,
    pub roll_slider: FillSlider,
    pub roll_value: Label,
    pub perspective_slider: FillSlider,
    pub perspective_value: Label,
    pub pos_x_slider: FillSlider,
    pub pos_x_value: Label,
    pub pos_y_slider: FillSlider,
    pub pos_y_value: Label,
    pub ease_slider: FillSlider,
    pub ease_value: Label,
    pub easing_x1_slider: FillSlider,
    pub easing_x1_value: Label,
    pub easing_y1_slider: FillSlider,
    pub easing_y1_value: Label,
    pub easing_x2_slider: FillSlider,
    pub easing_x2_value: Label,
    pub easing_y2_slider: FillSlider,
    pub easing_y2_value: Label,
    pub reset_timing_btn: Button,
    pub delete_btn: Button,
    pub inspector_syncing: Rc<Cell<bool>>,
    pub confirm_overlay: GtkBox,
}

pub(super) struct MotionRuntime {
    pub(super) snapshot: Option<RgbaImage>,
    pub(super) card: Option<gtk4::cairo::ImageSurface>,
    pub(super) motion: MotionState,
    pub(super) playing: bool,
    pub(super) live_preview: bool,
    pub(super) last_tick: Option<Instant>,
    /// Playhead time at which an edit-triggered transition preview stops.
    pub(super) preview_end: Option<f64>,
}

impl MotionRuntime {
    fn new() -> Self {
        Self {
            snapshot: None,
            card: None,
            motion: MotionState::default(),
            playing: false,
            live_preview: false,
            last_tick: None,
            preview_end: None,
        }
    }
}

#[derive(Clone)]
pub(super) struct MotionSession {
    runtime: Rc<RefCell<MotionRuntime>>,
    prefers_dark: bool,
}

impl MotionSession {
    pub(super) fn new(prefers_dark: bool) -> Self {
        Self {
            runtime: Rc::new(RefCell::new(MotionRuntime::new())),
            prefers_dark,
        }
    }

    pub(super) fn has_segments(&self) -> bool {
        self.runtime.borrow().motion.has_segments()
    }

    pub(super) fn duration(&self) -> f64 {
        self.runtime.borrow().motion.duration
    }

    pub(super) fn capture_snapshot(&self, state: &EditorState) {
        let snapshot = state.to_final_image().ok();
        let mut runtime = self.runtime.borrow_mut();
        runtime.card = snapshot.as_ref().and_then(rgba_image_to_surface);
        runtime.snapshot = snapshot;
        runtime.motion.playhead = 0.0;
        runtime.playing = false;
        runtime.live_preview = false;
        runtime.last_tick = None;
        runtime.preview_end = None;
        // Shotbase enters Motion with an empty effects track; clips appear
        // when the user clicks or drags the timeline.
    }

    pub(super) fn export_mp4(&self, source_image: &std::path::Path) -> Result<PathBuf, String> {
        let runtime = self.runtime.borrow();
        let snapshot = runtime
            .snapshot
            .as_ref()
            .ok_or_else(|| "Motion has no still to export".to_string())?;
        super::motion_render::export_motion_mp4(
            snapshot,
            &runtime.motion,
            self.prefers_dark,
            source_image,
        )
    }

    pub(super) fn clear_snapshot(&self) {
        let mut runtime = self.runtime.borrow_mut();
        runtime.snapshot = None;
        runtime.card = None;
        runtime.playing = false;
        runtime.live_preview = false;
        runtime.last_tick = None;
        runtime.preview_end = None;
        runtime.motion.playhead = 0.0;
        runtime.motion.segments.clear();
        runtime.motion.text_segments.clear();
        runtime.motion.selected = None;
        runtime.motion.selected_text = None;
    }
}

pub(super) fn build_motion_mode(prefers_dark: bool) -> (MotionModeParts, MotionSession) {
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

    let timeline = super::motion_timeline::build_motion_timeline(session.runtime.clone());

    let page = GtkBox::new(Orientation::Vertical, 0);
    page.set_hexpand(true);
    page.set_vexpand(true);
    page.set_focusable(true);
    page.add_css_class("editor-motion-page");
    page.append(&preview);
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
    let duration_title = Label::new(Some(&t("Duration")));
    duration_title.add_css_class("editor-inspector-title");
    duration_title.set_xalign(0.0);
    duration_title.set_visible(false);
    duration_value.set_visible(false);
    inspector.append(&duration_title);
    inspector.append(&duration_value);
    inspector.append(&duration_slider.widget());

    let blur_value = Label::new(Some("0%"));
    let blur_slider = FillSlider::new(&t("Motion blur"));
    blur_slider.set_range(0.0, 1.0);
    blur_slider.set_increments(0.01, 0.1);
    blur_slider.set_value(0.0);
    inspector.append(&blur_slider.widget());

    let blur_shutter_value = Label::new(Some("180°"));
    let blur_shutter_slider =
        FillSlider::new_with_value_text(&t("Blur shutter"), |value, _, _| format!("{value:.0}°"));
    blur_shutter_slider.set_range(0.0, 360.0);
    blur_shutter_slider.set_increments(5.0, 15.0);
    blur_shutter_slider.set_value(180.0);
    inspector.append(&blur_shutter_slider.widget());

    let blur_trail_value = Label::new(Some("28%"));
    let blur_trail_slider = FillSlider::new_with_value_text(&t("Blur trail"), |value, _, _| {
        format!("{:.0}%", value * 100.0)
    });
    blur_trail_slider.set_range(0.0, 0.4);
    blur_trail_slider.set_increments(0.01, 0.04);
    blur_trail_slider.set_value(0.28);
    inspector.append(&blur_trail_slider.widget());

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

    let scale_header = GtkBox::new(Orientation::Horizontal, 8);
    let scale_label = Label::new(Some(&t("Scale")));
    scale_label.add_css_class("recording-editor-zoom-kicker");
    scale_label.set_xalign(0.0);
    scale_label.set_hexpand(true);
    let scale_value = Label::new(Some("200%"));
    scale_value.add_css_class("recording-editor-zoom-kicker");
    scale_value.set_xalign(1.0);
    scale_header.append(&scale_label);
    scale_header.append(&scale_value);
    clip_box.append(&scale_header);
    let chips = Grid::new();
    chips.add_css_class("recording-editor-zoom-chips");
    chips.set_column_spacing(4);
    chips.set_row_spacing(4);
    chips.set_column_homogeneous(true);
    chips.set_hexpand(true);
    let inspector_syncing = Rc::new(Cell::new(false));
    let scale_chips: Vec<Button> = MOTION_SCALE_PRESETS
        .iter()
        .enumerate()
        .map(|(i, &(label, _))| {
            let chip = Button::with_label(label);
            chip.add_css_class("recording-editor-zoom-chip");
            chip.set_hexpand(true);
            chip.set_has_frame(false);
            chips.attach(&chip, (i % 3) as i32, (i / 3) as i32, 1, 1);
            chip
        })
        .collect();
    clip_box.append(&chips);

    let (intensity_header, intensity_value, intensity_slider) =
        span_slider_row(&t("Intensity"), 1.0, 0.0, 1.0);
    clip_box.append(&intensity_header);
    clip_box.append(&intensity_slider.widget());

    let (zoom_anchor_x_header, zoom_anchor_x_value, zoom_anchor_x_slider) =
        span_slider_row(&t("Anchor X"), 0.5, 0.0, 1.0);
    clip_box.append(&zoom_anchor_x_header);
    clip_box.append(&zoom_anchor_x_slider.widget());
    let (zoom_anchor_y_header, zoom_anchor_y_value, zoom_anchor_y_slider) =
        span_slider_row(&t("Anchor Y"), 0.5, 0.0, 1.0);
    clip_box.append(&zoom_anchor_y_header);
    clip_box.append(&zoom_anchor_y_slider.widget());

    let yaw_value = Label::new(Some("8°"));
    yaw_value.set_visible(false);
    let yaw_slider =
        FillSlider::new_with_value_text(&t("Yaw"), |value, _, _| format!("{value:.0}°"));
    yaw_slider.set_range(MIN_MOTION_YAW, MAX_MOTION_YAW);
    yaw_slider.set_increments(1.0, 5.0);
    yaw_slider.set_value(8.0);
    clip_box.append(&yaw_value);
    clip_box.append(&yaw_slider.widget());

    let (pitch_header, pitch_value, pitch_slider) = angle_slider_row(&t("Pitch"), 0.0);
    clip_box.append(&pitch_header);
    clip_box.append(&pitch_slider.widget());
    let (roll_header, roll_value, roll_slider) = angle_slider_row(&t("Roll"), 0.0);
    clip_box.append(&roll_header);
    clip_box.append(&roll_slider.widget());

    let perspective_value = Label::new(Some("18%"));
    perspective_value.set_visible(false);
    let perspective_slider = FillSlider::new(&t("Perspective"));
    perspective_slider.set_range(0.0, 1.0);
    perspective_slider.set_increments(0.01, 0.1);
    perspective_slider.set_value(0.18);
    clip_box.append(&perspective_value);
    clip_box.append(&perspective_slider.widget());

    let (pos_x_header, pos_x_value, pos_x_slider) = percent_slider_row(&t("X"), 0.0);
    clip_box.append(&pos_x_header);
    clip_box.append(&pos_x_slider.widget());
    let (pos_y_header, pos_y_value, pos_y_slider) = percent_slider_row(&t("Y"), 0.0);
    clip_box.append(&pos_y_header);
    clip_box.append(&pos_y_slider.widget());

    let ease_value = Label::new(Some("1200ms"));
    ease_value.set_visible(false);
    let ease_slider =
        FillSlider::new_with_value_text(&t("Duration"), |value, _, _| format!("{:.0}ms", value));
    ease_slider.set_range(MIN_ZOOM_EASE_MS as f64, MAX_ZOOM_EASE_MS as f64);
    ease_slider.set_increments(20.0, 100.0);
    ease_slider.set_value(1200.0);
    clip_box.append(&ease_value);
    clip_box.append(&ease_slider.widget());

    let (easing_x1_header, easing_x1_value, easing_x1_slider) =
        span_slider_row(&t("Ease X1"), 0.25, 0.0, 1.0);
    clip_box.append(&easing_x1_header);
    clip_box.append(&easing_x1_slider.widget());
    let (easing_y1_header, easing_y1_value, easing_y1_slider) =
        span_slider_row(&t("Ease Y1"), 1.0, 0.0, 1.0);
    clip_box.append(&easing_y1_header);
    clip_box.append(&easing_y1_slider.widget());
    let (easing_x2_header, easing_x2_value, easing_x2_slider) =
        span_slider_row(&t("Ease X2"), 0.50, 0.0, 1.0);
    clip_box.append(&easing_x2_header);
    clip_box.append(&easing_x2_slider.widget());
    let (easing_y2_header, easing_y2_value, easing_y2_slider) =
        span_slider_row(&t("Ease Y2"), 1.0, 0.0, 1.0);
    clip_box.append(&easing_y2_header);
    clip_box.append(&easing_y2_slider.widget());
    let reset_timing_btn = Button::with_label(&t("Reset"));
    reset_timing_btn.add_css_class("recording-editor-zoom-easing-btn");
    reset_timing_btn.set_has_frame(false);
    reset_timing_btn.set_halign(gtk4::Align::Start);
    clip_box.append(&reset_timing_btn);

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
    let text_entry = Entry::new();
    text_entry.set_placeholder_text(Some(&t("Title")));
    text_entry.set_hexpand(true);
    text_box.append(&text_entry);
    let (text_pos_x_header, text_pos_x_value, text_pos_x_slider) = span_slider_row(
        &t("X"),
        DEFAULT_MOTION_TEXT_POS_X,
        MIN_MOTION_TEXT_POS,
        MAX_MOTION_TEXT_POS,
    );
    text_box.append(&text_pos_x_header);
    text_box.append(&text_pos_x_slider.widget());
    let (text_pos_y_header, text_pos_y_value, text_pos_y_slider) = span_slider_row(
        &t("Y"),
        DEFAULT_MOTION_TEXT_POS_Y,
        MIN_MOTION_TEXT_POS,
        MAX_MOTION_TEXT_POS,
    );
    text_box.append(&text_pos_y_header);
    text_box.append(&text_pos_y_slider.widget());
    let (text_size_header, text_size_value, text_size_slider) = span_slider_row(
        &t("Size"),
        DEFAULT_MOTION_TEXT_SIZE,
        MIN_MOTION_TEXT_SIZE,
        MAX_MOTION_TEXT_SIZE,
    );
    text_box.append(&text_size_header);
    text_box.append(&text_size_slider.widget());
    let text_anim_label = Label::new(Some(&t("Animation")));
    text_anim_label.add_css_class("recording-editor-zoom-kicker");
    text_anim_label.set_xalign(0.0);
    text_box.append(&text_anim_label);
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
    text_box.append(&text_anim_grid);
    let text_scope_label = Label::new(Some(&t("Scope")));
    text_scope_label.add_css_class("recording-editor-zoom-kicker");
    text_scope_label.set_xalign(0.0);
    text_box.append(&text_scope_label);
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
    text_box.append(&text_scope_row);
    inspector.append(&text_box);

    let delete_btn = Button::with_label(&t("Delete"));
    delete_btn.set_has_frame(false);
    delete_btn.add_css_class("editor-sidebar-action-button");
    delete_btn.set_sensitive(false);
    inspector.append(&delete_btn);

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
            draw_motion_preview(context, width, height, &session_runtime, prefers_dark);
        }
    });

    (
        MotionModeParts {
            static_toolbar,
            static_btn,
            motion_btn,
            play_btn: timeline.play_btn,
            skip_back: timeline.skip_back,
            skip_forward: timeline.skip_forward,
            add_btn: timeline.add_btn,
            add_text_btn: timeline.add_text_btn,
            playhead_clock: timeline.playhead_clock,
            duration_clock: timeline.duration_clock,
            preview,
            timeline_card: timeline.dock,
            ruler: timeline.ruler,
            source_track: timeline.source_track,
            motion_track: timeline.track,
            text_track: timeline.text_track,
            playhead_overlay: timeline.playhead,
            page,
            inspector,
            duration_slider,
            duration_value,
            blur_slider,
            blur_value,
            blur_shutter_slider,
            blur_shutter_value,
            blur_trail_slider,
            blur_trail_value,
            clip_box,
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
            clip_hint,
            scale_value,
            scale_chips,
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
            delete_btn,
            inspector_syncing,
            confirm_overlay,
        },
        session,
    )
}

pub(super) struct MotionModeChrome {
    pub mode_stack: Stack,
    pub canvas_stack: Stack,
    pub bottom_left_stack: Stack,
    pub motion_control: GtkBox,
    pub history_control: GtkBox,
    pub inspector_tabs: GtkBox,
    pub inspector_stack: Stack,
}

pub(super) fn apply_editor_mode(chrome: &MotionModeChrome, motion: bool, last_inspector: &str) {
    let page = if motion { MOTION_PAGE } else { STATIC_PAGE };
    chrome.mode_stack.set_visible_child_name(page);
    chrome.canvas_stack.set_visible_child_name(page);
    chrome.bottom_left_stack.set_visible(!motion);
    chrome.motion_control.set_visible(!motion);
    chrome.history_control.set_visible(!motion);
    chrome.inspector_tabs.set_visible(!motion);
    if motion {
        chrome.inspector_stack.set_visible_child_name(MOTION_PAGE);
    } else {
        chrome
            .inspector_stack
            .set_visible_child_name(last_inspector);
    }
}

pub(super) fn annotations_need_snapshot(state: &EditorState) -> bool {
    !state.actions.is_empty() || state.crop_selection.is_some()
}

#[derive(Clone, Copy)]
enum ConfirmKind {
    ToMotion,
    ToStatic,
}

#[derive(Clone, Copy)]
enum DragKind {
    Start(usize),
    End(usize),
    Body { index: usize, origin: f64 },
}

pub(super) fn request_enter_motion(
    window: &ApplicationWindow,
    confirm_overlay: &GtkBox,
    _session: &MotionSession,
    state: &Arc<Mutex<EditorState>>,
    empty_drop_zone: bool,
    enter: Rc<dyn Fn()>,
) {
    if empty_drop_zone {
        return;
    }
    let needs_confirm = {
        let guard = state.lock().unwrap();
        annotations_need_snapshot(&guard) && !load_config().skip_static_to_motion_confirm
    };
    if needs_confirm {
        show_confirm(
            window,
            confirm_overlay,
            ConfirmKind::ToMotion,
            Rc::new({
                let enter = enter.clone();
                move || enter()
            }),
        );
        return;
    }
    enter();
}

pub(super) fn request_leave_motion(
    window: &ApplicationWindow,
    confirm_overlay: &GtkBox,
    session: &MotionSession,
    leave: Rc<dyn Fn()>,
) {
    if session.has_segments() && !load_config().skip_motion_to_static_confirm {
        show_confirm(window, confirm_overlay, ConfirmKind::ToStatic, leave);
        return;
    }
    leave();
}

pub(super) fn wire_motion_controls(
    parts: &MotionModeParts,
    session: &MotionSession,
    chrome: Rc<MotionModeChrome>,
    last_inspector: Rc<RefCell<String>>,
    in_motion: Rc<Cell<bool>>,
) {
    let redraw = {
        let preview = parts.preview.clone();
        let ruler = parts.ruler.clone();
        let source_track = parts.source_track.clone();
        let motion_track = parts.motion_track.clone();
        let playhead_overlay = parts.playhead_overlay.clone();
        let playhead_clock = parts.playhead_clock.clone();
        let duration_clock = parts.duration_clock.clone();
        let play_btn = parts.play_btn.clone();
        let session = session.runtime.clone();
        let text_track = parts.text_track.clone();
        let blur_slider = parts.blur_slider.clone();
        let blur_value = parts.blur_value.clone();
        let blur_shutter_slider = parts.blur_shutter_slider.clone();
        let blur_shutter_value = parts.blur_shutter_value.clone();
        let blur_trail_slider = parts.blur_trail_slider.clone();
        let blur_trail_value = parts.blur_trail_value.clone();
        let clip_box = parts.clip_box.clone();
        let text_box = parts.text_box.clone();
        let text_entry = parts.text_entry.clone();
        let text_pos_x_slider = parts.text_pos_x_slider.clone();
        let text_pos_x_value = parts.text_pos_x_value.clone();
        let text_pos_y_slider = parts.text_pos_y_slider.clone();
        let text_pos_y_value = parts.text_pos_y_value.clone();
        let text_size_slider = parts.text_size_slider.clone();
        let text_size_value = parts.text_size_value.clone();
        let text_anim_buttons = parts.text_anim_buttons.clone();
        let text_scope_buttons = parts.text_scope_buttons.clone();
        let clip_hint = parts.clip_hint.clone();
        let scale_value = parts.scale_value.clone();
        let scale_chips = parts.scale_chips.clone();
        let intensity_slider = parts.intensity_slider.clone();
        let intensity_value = parts.intensity_value.clone();
        let zoom_anchor_x_slider = parts.zoom_anchor_x_slider.clone();
        let zoom_anchor_x_value = parts.zoom_anchor_x_value.clone();
        let zoom_anchor_y_slider = parts.zoom_anchor_y_slider.clone();
        let zoom_anchor_y_value = parts.zoom_anchor_y_value.clone();
        let yaw_slider = parts.yaw_slider.clone();
        let yaw_value = parts.yaw_value.clone();
        let pitch_slider = parts.pitch_slider.clone();
        let pitch_value = parts.pitch_value.clone();
        let roll_slider = parts.roll_slider.clone();
        let roll_value = parts.roll_value.clone();
        let perspective_slider = parts.perspective_slider.clone();
        let perspective_value = parts.perspective_value.clone();
        let pos_x_slider = parts.pos_x_slider.clone();
        let pos_x_value = parts.pos_x_value.clone();
        let pos_y_slider = parts.pos_y_slider.clone();
        let pos_y_value = parts.pos_y_value.clone();
        let ease_slider = parts.ease_slider.clone();
        let ease_value = parts.ease_value.clone();
        let easing_x1_slider = parts.easing_x1_slider.clone();
        let easing_x1_value = parts.easing_x1_value.clone();
        let easing_y1_slider = parts.easing_y1_slider.clone();
        let easing_y1_value = parts.easing_y1_value.clone();
        let easing_x2_slider = parts.easing_x2_slider.clone();
        let easing_x2_value = parts.easing_x2_value.clone();
        let easing_y2_slider = parts.easing_y2_slider.clone();
        let easing_y2_value = parts.easing_y2_value.clone();
        let reset_timing_btn = parts.reset_timing_btn.clone();
        let delete_btn = parts.delete_btn.clone();
        let syncing = parts.inspector_syncing.clone();
        Rc::new(move || {
            let runtime = session.borrow();
            playhead_clock.set_text(&super::motion_timeline::format_clock(
                runtime.motion.playhead,
            ));
            duration_clock.set_text(&super::motion_timeline::format_clock(
                runtime.motion.duration,
            ));
            play_btn.set_tooltip_text(Some(&if runtime.playing {
                t("Pause")
            } else {
                t("Play")
            }));
            if let Some(image) = play_btn
                .child()
                .and_then(|child| child.downcast::<gtk4::Image>().ok())
            {
                image.set_icon_name(Some(if runtime.playing {
                    "media-playback-pause-symbolic"
                } else {
                    "media-playback-start-symbolic"
                }));
            }
            let selected = runtime.motion.selected_segment().cloned();
            let selected_text = runtime.motion.selected_text_segment().cloned();
            let blur = runtime.motion.motion_blur;
            let blur_settings = runtime.motion.motion_blur_settings.clamped();
            let perspective_intensity = runtime.motion.perspective_intensity;
            let transform_timing = runtime.motion.transform_timing;
            drop(runtime);
            syncing.set(true);
            blur_slider.set_value(blur);
            blur_value.set_label(&format!("{:.0}%", blur * 100.0));
            blur_shutter_slider.set_value(blur_settings.shutter_angle);
            blur_shutter_value.set_label(&format!("{:.0}°", blur_settings.shutter_angle));
            blur_trail_slider.set_value(blur_settings.transform_trail_opacity);
            blur_trail_value.set_label(&format!(
                "{:.0}%",
                blur_settings.transform_trail_opacity * 100.0
            ));
            easing_x1_slider.set_value(transform_timing.easing_x1);
            easing_x1_value.set_label(&format!("{:.0}%", transform_timing.easing_x1 * 100.0));
            easing_y1_slider.set_value(transform_timing.easing_y1);
            easing_y1_value.set_label(&format!("{:.0}%", transform_timing.easing_y1 * 100.0));
            easing_x2_slider.set_value(transform_timing.easing_x2);
            easing_x2_value.set_label(&format!("{:.0}%", transform_timing.easing_x2 * 100.0));
            easing_y2_slider.set_value(transform_timing.easing_y2);
            easing_y2_value.set_label(&format!("{:.0}%", transform_timing.easing_y2 * 100.0));
            let has_clip = selected.is_some();
            let has_text = selected_text.is_some();
            reset_timing_btn.set_sensitive(has_clip);
            clip_box.set_visible(has_clip);
            text_box.set_visible(has_text);
            clip_hint.set_visible(!has_clip && !has_text);
            if let Some(segment) = selected_text {
                if !text_entry.has_focus() {
                    text_entry.set_text(&segment.text);
                }
                text_pos_x_slider.set_value(segment.pos_x);
                text_pos_x_value.set_label(&format!("{:.0}%", segment.pos_x * 100.0));
                text_pos_y_slider.set_value(segment.pos_y);
                text_pos_y_value.set_label(&format!("{:.0}%", segment.pos_y * 100.0));
                text_size_slider.set_value(segment.size);
                text_size_value.set_label(&format!("{:.0}%", segment.size * 100.0));
                for (animation, button) in &text_anim_buttons {
                    button.set_active(*animation == segment.animation);
                }
                for (scope, button) in &text_scope_buttons {
                    button.set_active(*scope == segment.scope);
                }
                preview.set_tooltip_text(Some(&t("Drag on the preview to place the title")));
            } else {
                preview.set_tooltip_text(None);
            }
            if let Some(segment) = selected {
                intensity_slider.set_value(segment.intensity);
                intensity_value.set_label(&format!("{:.0}%", segment.intensity * 100.0));
                zoom_anchor_x_slider.set_value(segment.zoom_anchor_x);
                zoom_anchor_x_value.set_label(&format!("{:.0}%", segment.zoom_anchor_x * 100.0));
                zoom_anchor_y_slider.set_value(segment.zoom_anchor_y);
                zoom_anchor_y_value.set_label(&format!("{:.0}%", segment.zoom_anchor_y * 100.0));
                yaw_slider.set_value(segment.to.rotation_y);
                yaw_value.set_label(&format!("{:.0}°", segment.to.rotation_y));
                pitch_slider.set_value(segment.to.rotation_x);
                pitch_value.set_label(&format!("{:.0}°", segment.to.rotation_x));
                roll_slider.set_value(segment.to.rotation_z);
                roll_value.set_label(&format!("{:.0}°", segment.to.rotation_z));
                perspective_slider.set_value(perspective_intensity);
                perspective_value.set_label(&format!("{:.0}%", perspective_intensity * 100.0));
                pos_x_slider.set_value(segment.to.pos_x);
                pos_x_value.set_label(&format!("{:.0}%", segment.to.pos_x * 100.0));
                pos_y_slider.set_value(segment.to.pos_y);
                pos_y_value.set_label(&format!("{:.0}%", segment.to.pos_y * 100.0));
                ease_slider.set_value(transform_timing.transition_duration * 1000.0);
                ease_value.set_label(&format!(
                    "{:.0}ms",
                    transform_timing.transition_duration * 1000.0
                ));
                for (chip, &(_, scale)) in scale_chips.iter().zip(MOTION_SCALE_PRESETS.iter()) {
                    if (segment.to.scale - scale).abs() < 0.03 {
                        chip.add_css_class("recording-editor-zoom-chip-active");
                    } else {
                        chip.remove_css_class("recording-editor-zoom-chip-active");
                    }
                }
                scale_value.set_label(&format!("{:.0}%", segment.to.scale * 100.0));
            }
            delete_btn.set_sensitive(has_clip || has_text);
            syncing.set(false);
            preview.queue_draw();
            ruler.queue_draw();
            source_track.queue_draw();
            motion_track.queue_draw();
            text_track.queue_draw();
            playhead_overlay.queue_draw();
        })
    };

    // Segment trimming and movement can generate far more pointer updates than
    // the expensive perspective preview can render. Keep the direct-manipulation
    // path limited to the lane being dragged; the full preview and inspector
    // catch up once the pointer is released.
    let redraw_motion_track = {
        let motion_track = parts.motion_track.clone();
        Rc::new(move || motion_track.queue_draw())
    };
    let redraw_text_track = {
        let text_track = parts.text_track.clone();
        Rc::new(move || text_track.queue_draw())
    };

    let request_live_preview = {
        let session = session.runtime.clone();
        let preview = parts.preview.clone();
        let gen = Rc::new(Cell::new(0u32));
        Rc::new(move || {
            session.borrow_mut().live_preview = true;
            preview.queue_draw();
            let token = gen.get().wrapping_add(1);
            gen.set(token);
            let session = session.clone();
            let preview = preview.clone();
            let gen = gen.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(90), move || {
                if gen.get() != token {
                    return glib::ControlFlow::Break;
                }
                session.borrow_mut().live_preview = false;
                preview.queue_draw();
                glib::ControlFlow::Break
            });
        })
    };

    // Editing a timed clip parameter has to be judged in motion: a static
    // frame at the playhead is identical before and after most edits. Like
    // Shotbase, play the affected transition once. If the playhead is already
    // inside the transition window the preview continues from there instead
    // of restarting.
    let request_transition_preview = {
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        Rc::new(move |segment_start: f64| {
            let mut runtime = session.borrow_mut();
            let transition = runtime
                .motion
                .transform_timing
                .clamped()
                .transition_duration;
            let start = segment_start.max(0.0).min(runtime.motion.duration);
            let end = (start + transition + 0.4).min(runtime.motion.duration);
            let inside = runtime.motion.playhead >= start && runtime.motion.playhead <= end;
            if !inside {
                runtime.motion.playhead = start;
            }
            runtime.playing = true;
            runtime.last_tick = Some(Instant::now());
            runtime.preview_end = Some(end);
            drop(runtime);
            redraw();
        })
    };

    parts.duration_slider.connect_value_changed({
        let session_runtime = session.runtime.clone();
        let duration_value = parts.duration_value.clone();
        let redraw = redraw.clone();
        move |slider| {
            let duration = MotionState::clamp_duration(slider.value());
            session_runtime.borrow_mut().motion.set_duration(duration);
            duration_value.set_label(&format_duration_label(duration));
            redraw();
        }
    });

    parts.blur_slider.connect_value_changed({
        let session = session.runtime.clone();
        let blur_value = parts.blur_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            session.borrow_mut().motion.set_motion_blur(value);
            blur_value.set_label(&format!("{:.0}%", value * 100.0));
            request_live_preview();
        }
    });

    parts.blur_shutter_slider.connect_value_changed({
        let session = session.runtime.clone();
        let value_label = parts.blur_shutter_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let shutter = slider.value().clamp(0.0, 360.0);
            session
                .borrow_mut()
                .motion
                .motion_blur_settings
                .shutter_angle = shutter;
            value_label.set_label(&format!("{shutter:.0}°"));
            request_live_preview();
        }
    });

    parts.blur_trail_slider.connect_value_changed({
        let session = session.runtime.clone();
        let value_label = parts.blur_trail_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let trail = slider.value().clamp(0.0, 1.0);
            session
                .borrow_mut()
                .motion
                .motion_blur_settings
                .transform_trail_opacity = trail;
            value_label.set_label(&format!("{:.0}%", trail * 100.0));
            request_live_preview();
        }
    });

    parts.play_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_| {
            {
                let mut runtime = session.borrow_mut();
                runtime.playing = !runtime.playing;
                runtime.last_tick = if runtime.playing {
                    Some(Instant::now())
                } else {
                    None
                };
                // Manual playback always runs the whole composition; only an
                // edit-triggered preview stops early.
                if runtime.playing {
                    runtime.preview_end = None;
                }
                if runtime.playing && runtime.motion.playhead >= runtime.motion.duration {
                    runtime.motion.playhead = 0.0;
                }
            }
            redraw();
        }
    });
    parts.skip_back.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_| {
            let mut runtime = session.borrow_mut();
            runtime.motion.playhead = (runtime.motion.playhead - 1.0).max(0.0);
            drop(runtime);
            redraw();
        }
    });
    let ruler_click = GestureClick::new();
    ruler_click.set_button(1);
    ruler_click.connect_pressed({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |gesture, _, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            runtime.motion.playhead = ((x / width) * duration).clamp(0.0, duration);
            drop(runtime);
            redraw();
        }
    });
    parts.ruler.add_controller(ruler_click);

    // The ruler is a scrub surface: dragging anywhere moves the playhead,
    // rather than requiring a pixel-perfect hit on the thin playhead line.
    let ruler_drag = GestureDrag::new();
    ruler_drag.set_button(1);
    ruler_drag.connect_drag_update({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |gesture, offset_x, _| {
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            runtime.motion.playhead =
                (((start_x + offset_x) / width) * duration).clamp(0.0, duration);
            drop(runtime);
            redraw();
        }
    });
    parts.ruler.add_controller(ruler_drag);

    // The source thumbnail lane is an actual scrub target, matching the
    // timeline's visible source segment instead of being decorative chrome.
    let source_click = GestureClick::new();
    source_click.set_button(1);
    source_click.connect_pressed({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |gesture, _, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            runtime.motion.playhead = ((x / width) * duration).clamp(0.0, duration);
            drop(runtime);
            redraw();
        }
    });
    parts.source_track.add_controller(source_click);

    let track_click = GestureClick::new();
    track_click.set_button(1);
    track_click.connect_pressed({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |gesture, n_press, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            let time = runtime.motion.snap_effect_time(
                ((x / width) * duration).clamp(0.0, duration),
                (10.0 / width) * duration,
                None,
            );
            if n_press >= 2 {
                if runtime.motion.add_segment_at(time).is_none() {
                    runtime.motion.selected = runtime.motion.segment_index_at(time);
                    runtime.motion.selected_text = None;
                    runtime.motion.playhead = time;
                }
            } else if let Some(index) = runtime.motion.segment_index_at(time) {
                runtime.motion.selected = Some(index);
                runtime.motion.selected_text = None;
            } else {
                runtime.motion.selected = None;
                runtime.motion.selected_text = None;
                runtime.motion.playhead = time;
            }
            drop(runtime);
            redraw();
        }
    });
    parts.motion_track.add_controller(track_click);

    let drag_kind = Rc::new(Cell::new(None::<DragKind>));
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let session = session.runtime.clone();
        let drag_kind = drag_kind.clone();
        move |gesture, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let runtime = session.borrow();
            let duration = runtime.motion.duration.max(0.001);
            let time = ((x / width) * duration).clamp(0.0, duration);
            let edge_seconds = (8.0 / width) * duration;
            let kind = runtime
                .motion
                .segments
                .iter()
                .enumerate()
                .find_map(|(index, segment)| {
                    if (segment.start - time).abs() <= edge_seconds {
                        Some(DragKind::Start(index))
                    } else if (segment.end - time).abs() <= edge_seconds {
                        Some(DragKind::End(index))
                    } else if time >= segment.start && time <= segment.end {
                        Some(DragKind::Body {
                            index,
                            origin: segment.start,
                        })
                    } else {
                        None
                    }
                });
            drag_kind.set(kind);
        }
    });
    drag.connect_drag_update({
        let session = session.runtime.clone();
        let drag_kind = drag_kind.clone();
        let redraw_motion_track = redraw_motion_track.clone();
        move |gesture, offset_x, _| {
            let Some(kind) = drag_kind.get() else {
                return;
            };
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            let raw_time = (((start_x + offset_x) / width) * duration).clamp(0.0, duration);
            let tolerance = (10.0 / width) * duration;
            let index = match kind {
                DragKind::Start(index) | DragKind::End(index) => index,
                DragKind::Body { index, .. } => index,
            };
            let before = runtime
                .motion
                .segments
                .get(index)
                .map(|segment| (segment.start, segment.end));
            match kind {
                DragKind::Start(index) => {
                    let time = runtime
                        .motion
                        .snap_effect_time(raw_time, tolerance, Some(index));
                    let end = runtime
                        .motion
                        .segments
                        .get(index)
                        .map(|segment| segment.end)
                        .unwrap_or(time);
                    runtime.motion.set_segment_range(index, time, end);
                }
                DragKind::End(index) => {
                    let time = runtime
                        .motion
                        .snap_effect_time(raw_time, tolerance, Some(index));
                    let start = runtime
                        .motion
                        .segments
                        .get(index)
                        .map(|segment| segment.start)
                        .unwrap_or(time);
                    runtime.motion.set_segment_range(index, start, time);
                }
                DragKind::Body { index, origin } => {
                    let delta = (offset_x / width) * duration;
                    let start =
                        runtime
                            .motion
                            .snap_effect_time(origin + delta, tolerance, Some(index));
                    runtime.motion.move_segment(index, start);
                }
            }
            let changed = before
                != runtime
                    .motion
                    .segments
                    .get(index)
                    .map(|segment| (segment.start, segment.end));
            drop(runtime);
            if changed {
                redraw_motion_track();
            }
        }
    });
    drag.connect_drag_end({
        let drag_kind = drag_kind.clone();
        let redraw = redraw.clone();
        move |_, _, _| {
            drag_kind.set(None);
            redraw();
        }
    });
    parts.motion_track.add_controller(drag);
    install_track_end_cursor(&parts.motion_track, session.runtime.clone(), false);

    let text_click = GestureClick::new();
    text_click.set_button(1);
    text_click.connect_pressed({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |gesture, n_press, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            let time = runtime.motion.snap_text_time(
                ((x / width) * duration).clamp(0.0, duration),
                (10.0 / width) * duration,
                None,
            );
            if n_press >= 2 {
                if runtime.motion.add_text_at(time).is_none() {
                    runtime.motion.selected_text = runtime.motion.text_index_at(time);
                    runtime.motion.selected = None;
                    runtime.motion.playhead = time;
                }
            } else if let Some(index) = runtime.motion.text_index_at(time) {
                runtime.motion.selected_text = Some(index);
                runtime.motion.selected = None;
            } else {
                runtime.motion.selected_text = None;
                runtime.motion.selected = None;
                runtime.motion.playhead = time;
            }
            drop(runtime);
            redraw();
        }
    });
    parts.text_track.add_controller(text_click);

    let text_drag_kind = Rc::new(Cell::new(None::<DragKind>));
    let text_drag = GestureDrag::new();
    text_drag.set_button(1);
    text_drag.connect_drag_begin({
        let session = session.runtime.clone();
        let text_drag_kind = text_drag_kind.clone();
        move |gesture, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let runtime = session.borrow();
            let duration = runtime.motion.duration.max(0.001);
            let time = ((x / width) * duration).clamp(0.0, duration);
            let edge_seconds = (8.0 / width) * duration;
            let kind =
                runtime
                    .motion
                    .text_segments
                    .iter()
                    .enumerate()
                    .find_map(|(index, segment)| {
                        if (segment.start - time).abs() <= edge_seconds {
                            Some(DragKind::Start(index))
                        } else if (segment.end - time).abs() <= edge_seconds {
                            Some(DragKind::End(index))
                        } else if time >= segment.start && time <= segment.end {
                            Some(DragKind::Body {
                                index,
                                origin: segment.start,
                            })
                        } else {
                            None
                        }
                    });
            text_drag_kind.set(kind);
        }
    });
    text_drag.connect_drag_update({
        let session = session.runtime.clone();
        let text_drag_kind = text_drag_kind.clone();
        let redraw_text_track = redraw_text_track.clone();
        move |gesture, offset_x, _| {
            let Some(kind) = text_drag_kind.get() else {
                return;
            };
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            let raw_time = (((start_x + offset_x) / width) * duration).clamp(0.0, duration);
            let tolerance = (10.0 / width) * duration;
            let index = match kind {
                DragKind::Start(index) | DragKind::End(index) => index,
                DragKind::Body { index, .. } => index,
            };
            let before = runtime
                .motion
                .text_segments
                .get(index)
                .map(|segment| (segment.start, segment.end));
            match kind {
                DragKind::Start(index) => {
                    let time = runtime
                        .motion
                        .snap_text_time(raw_time, tolerance, Some(index));
                    let end = runtime
                        .motion
                        .text_segments
                        .get(index)
                        .map(|segment| segment.end)
                        .unwrap_or(time);
                    runtime.motion.set_text_range(index, time, end);
                }
                DragKind::End(index) => {
                    let time = runtime
                        .motion
                        .snap_text_time(raw_time, tolerance, Some(index));
                    let start = runtime
                        .motion
                        .text_segments
                        .get(index)
                        .map(|segment| segment.start)
                        .unwrap_or(time);
                    runtime.motion.set_text_range(index, start, time);
                }
                DragKind::Body { index, origin } => {
                    let delta = (offset_x / width) * duration;
                    let start =
                        runtime
                            .motion
                            .snap_text_time(origin + delta, tolerance, Some(index));
                    runtime.motion.move_text(index, start);
                }
            }
            let changed = before
                != runtime
                    .motion
                    .text_segments
                    .get(index)
                    .map(|segment| (segment.start, segment.end));
            drop(runtime);
            if changed {
                redraw_text_track();
            }
        }
    });
    text_drag.connect_drag_end({
        let text_drag_kind = text_drag_kind.clone();
        let redraw = redraw.clone();
        move |_, _, _| {
            text_drag_kind.set(None);
            redraw();
        }
    });
    parts.text_track.add_controller(text_drag);
    install_track_end_cursor(&parts.text_track, session.runtime.clone(), true);

    parts.add_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_| {
            let mut runtime = session.borrow_mut();
            let playhead = runtime.motion.playhead;
            let _ = runtime.motion.add_segment_at(playhead);
            drop(runtime);
            redraw();
        }
    });

    parts.add_text_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_| {
            let mut runtime = session.borrow_mut();
            let playhead = runtime.motion.playhead;
            let _ = runtime.motion.add_text_at(playhead);
            drop(runtime);
            redraw();
        }
    });

    parts.text_entry.connect_changed({
        let session = session.runtime.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |entry| {
            if syncing.get() {
                return;
            }
            session
                .borrow_mut()
                .motion
                .set_selected_text_value(entry.text().to_string());
            request_live_preview();
        }
    });

    for (animation, button) in &parts.text_anim_buttons {
        let animation = *animation;
        button.connect_toggled({
            let session = session.runtime.clone();
            let request_live_preview = request_live_preview.clone();
            let syncing = parts.inspector_syncing.clone();
            move |button| {
                if syncing.get() || !button.is_active() {
                    return;
                }
                session
                    .borrow_mut()
                    .motion
                    .set_selected_text_animation(animation);
                request_live_preview();
            }
        });
    }

    for (scope, button) in &parts.text_scope_buttons {
        let scope = *scope;
        button.connect_toggled({
            let session = session.runtime.clone();
            let request_live_preview = request_live_preview.clone();
            let syncing = parts.inspector_syncing.clone();
            move |button| {
                if syncing.get() || !button.is_active() {
                    return;
                }
                session.borrow_mut().motion.set_selected_text_scope(scope);
                request_live_preview();
            }
        });
    }

    parts.text_pos_x_slider.connect_value_changed({
        let session = session.runtime.clone();
        let text_pos_x_value = parts.text_pos_x_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let pos_y = session
                .borrow()
                .motion
                .selected_text_segment()
                .map(|segment| segment.pos_y)
                .unwrap_or(DEFAULT_MOTION_TEXT_POS_Y);
            session
                .borrow_mut()
                .motion
                .set_selected_text_pos(value, pos_y);
            text_pos_x_value.set_label(&format!("{:.0}%", value * 100.0));
            request_live_preview();
        }
    });
    parts.text_pos_y_slider.connect_value_changed({
        let session = session.runtime.clone();
        let text_pos_y_value = parts.text_pos_y_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let pos_x = session
                .borrow()
                .motion
                .selected_text_segment()
                .map(|segment| segment.pos_x)
                .unwrap_or(DEFAULT_MOTION_TEXT_POS_X);
            session
                .borrow_mut()
                .motion
                .set_selected_text_pos(pos_x, value);
            text_pos_y_value.set_label(&format!("{:.0}%", value * 100.0));
            request_live_preview();
        }
    });
    parts.text_size_slider.connect_value_changed({
        let session = session.runtime.clone();
        let text_size_value = parts.text_size_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            session.borrow_mut().motion.set_selected_text_size(value);
            text_size_value.set_label(&format!("{:.0}%", value * 100.0));
            request_live_preview();
        }
    });

    let place_text = {
        let session = session.runtime.clone();
        let preview = parts.preview.clone();
        let text_pos_x_slider = parts.text_pos_x_slider.clone();
        let text_pos_x_value = parts.text_pos_x_value.clone();
        let text_pos_y_slider = parts.text_pos_y_slider.clone();
        let text_pos_y_value = parts.text_pos_y_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        Rc::new(move |x: f64, y: f64| {
            let runtime = session.borrow();
            if runtime.motion.selected_text.is_none() {
                return;
            }
            let width = preview.allocated_width().max(1) as f64;
            let height = preview.allocated_height().max(1) as f64;
            let Some(card) = runtime.card.as_ref() else {
                return;
            };
            let transform = runtime.motion.sample(runtime.motion.playhead);
            let zoom_anchor = runtime.motion.zoom_anchor_at(runtime.motion.playhead);
            let (pos_x, pos_y) = super::motion_render::view_point_to_motion_text_position(
                card,
                width,
                height,
                transform,
                zoom_anchor,
                x,
                y,
            );
            drop(runtime);
            session
                .borrow_mut()
                .motion
                .set_selected_text_pos(pos_x, pos_y);
            syncing.set(true);
            text_pos_x_slider.set_value(pos_x);
            text_pos_x_value.set_label(&format!("{:.0}%", pos_x * 100.0));
            text_pos_y_slider.set_value(pos_y);
            text_pos_y_value.set_label(&format!("{:.0}%", pos_y * 100.0));
            syncing.set(false);
            request_live_preview();
        })
    };
    let text_drag_armed = Rc::new(Cell::new(false));
    let preview_drag = GestureDrag::new();
    preview_drag.set_button(1);
    preview_drag.connect_drag_begin({
        let session = session.runtime.clone();
        let preview = parts.preview.clone();
        let text_drag_armed = text_drag_armed.clone();
        move |_, x, y| {
            let runtime = session.borrow();
            let hit = runtime
                .motion
                .selected_text_segment()
                .and_then(|segment| {
                    runtime.card.as_ref().map(|card| {
                        super::motion_render::motion_text_contains_view_point(
                            card,
                            preview.allocated_width().max(1) as f64,
                            preview.allocated_height().max(1) as f64,
                            runtime.motion.sample(runtime.motion.playhead),
                            runtime.motion.zoom_anchor_at(runtime.motion.playhead),
                            segment,
                            runtime.motion.playhead,
                            x,
                            y,
                        )
                    })
                })
                .unwrap_or(false);
            text_drag_armed.set(hit);
        }
    });
    preview_drag.connect_drag_update({
        let place_text = place_text.clone();
        let text_drag_armed = text_drag_armed.clone();
        move |gesture, offset_x, offset_y| {
            if !text_drag_armed.get() {
                return;
            }
            let Some((start_x, start_y)) = gesture.start_point() else {
                return;
            };
            place_text(start_x + offset_x, start_y + offset_y);
        }
    });
    preview_drag.connect_drag_end({
        let text_drag_armed = text_drag_armed.clone();
        move |_, _, _| text_drag_armed.set(false)
    });
    parts.preview.add_controller(preview_drag);

    for (i, (_, scale)) in MOTION_SCALE_PRESETS.iter().enumerate() {
        let chip = parts.scale_chips[i].clone();
        chip.connect_clicked({
            let session = session.runtime.clone();
            let redraw = redraw.clone();
            let request_transition_preview = request_transition_preview.clone();
            let syncing = parts.inspector_syncing.clone();
            let scale = *scale;
            move |_| {
                if syncing.get() {
                    return;
                }
                let segment_start = {
                    let runtime = session.borrow();
                    runtime
                        .motion
                        .selected_segment()
                        .map(|segment| segment.start)
                };
                session.borrow_mut().motion.set_selected_end_scale(scale);
                match segment_start {
                    Some(start) => request_transition_preview(start),
                    None => redraw(),
                }
            }
        });
    }

    parts.intensity_slider.connect_value_changed({
        let session = session.runtime.clone();
        let intensity_value = parts.intensity_value.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session.borrow_mut().motion.set_selected_intensity(value);
            intensity_value.set_label(&format!("{:.0}%", value * 100.0));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        }
    });

    for (axis, slider, value_label) in [
        (
            0_u8,
            parts.zoom_anchor_x_slider.clone(),
            parts.zoom_anchor_x_value.clone(),
        ),
        (
            1_u8,
            parts.zoom_anchor_y_slider.clone(),
            parts.zoom_anchor_y_value.clone(),
        ),
    ] {
        let session = session.runtime.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        slider.connect_value_changed(move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            let mut runtime = session.borrow_mut();
            let (mut x, mut y) = runtime
                .motion
                .selected_segment()
                .map_or((0.5, 0.5), |segment| {
                    (segment.zoom_anchor_x, segment.zoom_anchor_y)
                });
            if axis == 0 {
                x = value;
            } else {
                y = value;
            }
            runtime.motion.set_selected_zoom_anchor(x, y);
            drop(runtime);
            value_label.set_label(&format!("{:.0}%", value * 100.0));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        });
    }

    parts.yaw_slider.connect_value_changed({
        let session = session.runtime.clone();
        let yaw_value = parts.yaw_value.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session.borrow_mut().motion.set_selected_end_yaw(value);
            yaw_value.set_label(&format!("{:.0}°", value));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        }
    });
    parts.pitch_slider.connect_value_changed({
        let session = session.runtime.clone();
        let pitch_value = parts.pitch_value.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session.borrow_mut().motion.set_selected_end_pitch(value);
            pitch_value.set_label(&format!("{:.0}°", value));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        }
    });
    parts.roll_slider.connect_value_changed({
        let session = session.runtime.clone();
        let roll_value = parts.roll_value.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session.borrow_mut().motion.set_selected_end_roll(value);
            roll_value.set_label(&format!("{:.0}°", value));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        }
    });
    parts.perspective_slider.connect_value_changed({
        let session = session.runtime.clone();
        let perspective_value = parts.perspective_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            session.borrow_mut().motion.set_selected_perspective(value);
            perspective_value.set_label(&format!("{:.0}%", value * 100.0));
            request_live_preview();
        }
    });
    parts.pos_x_slider.connect_value_changed({
        let session = session.runtime.clone();
        let pos_x_value = parts.pos_x_value.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session.borrow_mut().motion.set_selected_end_pos_x(value);
            pos_x_value.set_label(&format!("{:.0}%", value * 100.0));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        }
    });
    parts.pos_y_slider.connect_value_changed({
        let session = session.runtime.clone();
        let pos_y_value = parts.pos_y_value.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session.borrow_mut().motion.set_selected_end_pos_y(value);
            pos_y_value.set_label(&format!("{:.0}%", value * 100.0));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        }
    });
    parts.ease_slider.connect_value_changed({
        let session = session.runtime.clone();
        let ease_value = parts.ease_value.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let transition_ms = slider.value().round() as u32;
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session
                .borrow_mut()
                .motion
                .set_selected_transition_ms(transition_ms);
            ease_value.set_label(&format!("{transition_ms}ms"));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        }
    });

    for (axis, slider, value_label) in [
        (
            0_u8,
            parts.easing_x1_slider.clone(),
            parts.easing_x1_value.clone(),
        ),
        (
            1_u8,
            parts.easing_y1_slider.clone(),
            parts.easing_y1_value.clone(),
        ),
        (
            2_u8,
            parts.easing_x2_slider.clone(),
            parts.easing_x2_value.clone(),
        ),
        (
            3_u8,
            parts.easing_y2_slider.clone(),
            parts.easing_y2_value.clone(),
        ),
    ] {
        let session = session.runtime.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.inspector_syncing.clone();
        slider.connect_value_changed(move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            let mut runtime = session.borrow_mut();
            let mut timing = runtime.motion.transform_timing;
            match axis {
                0 => timing.easing_x1 = value,
                1 => timing.easing_y1 = value,
                2 => timing.easing_x2 = value,
                _ => timing.easing_y2 = value,
            }
            runtime.motion.set_transform_timing(timing);
            drop(runtime);
            value_label.set_label(&format!("{:.0}%", value * 100.0));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        });
    }

    parts.reset_timing_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        let request_transition_preview = request_transition_preview.clone();
        move |_| {
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session
                .borrow_mut()
                .motion
                .set_transform_timing(MotionEffectTransformTiming::default());
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => redraw(),
            }
        }
    });

    parts.delete_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_| {
            session.borrow_mut().motion.remove_selected();
            redraw();
        }
    });

    let delete_keys = EventControllerKey::new();
    delete_keys.connect_key_pressed({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        let in_motion = in_motion.clone();
        move |_, key, _, _| {
            if !in_motion.get() {
                return glib::Propagation::Proceed;
            }
            if key != gdk::Key::Delete && key != gdk::Key::BackSpace {
                return glib::Propagation::Proceed;
            }
            if session.borrow_mut().motion.remove_selected() {
                redraw();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    });
    parts.page.add_controller(delete_keys);

    parts.skip_forward.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_| {
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration;
            runtime.motion.playhead = (runtime.motion.playhead + 1.0).min(duration);
            drop(runtime);
            redraw();
        }
    });

    glib::timeout_add_local(std::time::Duration::from_millis(33), {
        let session_runtime = session.runtime.clone();
        let redraw = redraw.clone();
        let preview = parts.preview.clone();
        let playhead_overlay = parts.playhead_overlay.clone();
        let playhead_clock = parts.playhead_clock.clone();
        let in_motion = in_motion.clone();
        move || {
            if !in_motion.get() {
                return glib::ControlFlow::Continue;
            }
            let playing = session_runtime.borrow().playing;
            if playing {
                let mut runtime = session_runtime.borrow_mut();
                if runtime.playing {
                    let now = Instant::now();
                    let dt = runtime
                        .last_tick
                        .map(|last| now.duration_since(last).as_secs_f64())
                        .unwrap_or(0.0);
                    runtime.last_tick = Some(now);
                    runtime.motion.playhead += dt;
                    let preview_done = runtime
                        .preview_end
                        .is_some_and(|end| runtime.motion.playhead >= end);
                    if preview_done {
                        runtime.motion.playhead = runtime.preview_end.take().unwrap_or(0.0);
                        runtime.playing = false;
                        runtime.last_tick = None;
                    } else if runtime.motion.playhead >= runtime.motion.duration {
                        runtime.motion.playhead = 0.0;
                        runtime.playing = false;
                        runtime.last_tick = None;
                        runtime.preview_end = None;
                    }
                }
                let playhead = runtime.motion.playhead;
                let stopped = !runtime.playing;
                drop(runtime);
                if stopped {
                    redraw();
                } else {
                    playhead_clock.set_text(&super::motion_timeline::format_clock(playhead));
                    preview.queue_draw();
                    playhead_overlay.queue_draw();
                }
            }
            glib::ControlFlow::Continue
        }
    });

    let _ = chrome;
    let _ = last_inspector;
}

/// Advertise the existing trim handles before a drag starts. Motion and Text
/// clips both accept edge drags, so the pointer must make those narrow targets
/// discoverable rather than looking like ordinary timeline space.
fn install_track_end_cursor(
    track: &DrawingArea,
    runtime: Rc<RefCell<MotionRuntime>>,
    text_track: bool,
) {
    let pointer = EventControllerMotion::new();
    pointer.connect_motion(move |controller, x, _| {
        let width = controller
            .widget()
            .map(|widget| widget.allocated_width().max(1) as f64)
            .unwrap_or(1.0);
        let cursor_name = {
            let runtime = runtime.borrow();
            let duration = runtime.motion.duration.max(0.001);
            let time = ((x / width) * duration).clamp(0.0, duration);
            let edge_seconds = (8.0 / width) * duration;
            let segments = if text_track {
                runtime
                    .motion
                    .text_segments
                    .iter()
                    .map(|segment| (segment.start, segment.end))
                    .collect::<Vec<_>>()
            } else {
                runtime
                    .motion
                    .segments
                    .iter()
                    .map(|segment| (segment.start, segment.end))
                    .collect::<Vec<_>>()
            };
            segments.iter().find_map(|(start, end)| {
                if (time - end).abs() <= edge_seconds {
                    Some("e-resize")
                } else if (time - start).abs() <= edge_seconds {
                    Some("w-resize")
                } else {
                    None
                }
            })
        };
        if let Some(widget) = controller.widget() {
            widget.set_cursor(
                gdk::Cursor::from_name(cursor_name.unwrap_or("default"), None).as_ref(),
            );
        }
    });
    pointer.connect_leave(|controller| {
        if let Some(widget) = controller.widget() {
            widget.set_cursor(None);
        }
    });
    track.add_controller(pointer);
}

fn show_confirm(
    _window: &ApplicationWindow,
    overlay: &GtkBox,
    kind: ConfirmKind,
    on_continue: Rc<dyn Fn()>,
) {
    while let Some(child) = overlay.first_child() {
        overlay.remove(&child);
    }

    let (title, body, skip_field) = match kind {
        ConfirmKind::ToMotion => (
            t("Annotations will be applied"),
            t("Motion will play a snapshot of your annotations. Returning to Static restores the live tools. Your original image is not flattened until Done."),
            "static_to_motion",
        ),
        ConfirmKind::ToStatic => (
            t("Motion effects will be cleared"),
            t("Motion effect segments can't be edited in Static mode and will be cleared if you continue."),
            "motion_to_static",
        ),
    };

    let card = GtkBox::new(Orientation::Vertical, 12);
    card.add_css_class("editor-motion-confirm-card");
    card.set_halign(Align::Center);
    card.set_valign(Align::Center);
    card.set_hexpand(false);

    let title_label = Label::new(Some(&title));
    title_label.add_css_class("editor-inspector-title");
    title_label.set_xalign(0.0);
    title_label.set_wrap(true);
    title_label.set_max_width_chars(44);

    let body_label = Label::new(Some(&body));
    body_label.add_css_class("editor-select-inspector-hint");
    body_label.set_xalign(0.0);
    body_label.set_wrap(true);
    body_label.set_max_width_chars(44);

    let skip = CheckButton::with_label(&t("Don't show this again"));
    skip.add_css_class("editor-motion-confirm-skip");

    let buttons = GtkBox::new(Orientation::Horizontal, 8);
    buttons.set_halign(Align::End);
    let cancel = Button::with_label(&t("Cancel"));
    cancel.set_has_frame(false);
    cancel.add_css_class("editor-sidebar-action-button");
    let cont = Button::with_label(&t("Continue"));
    cont.set_has_frame(false);
    cont.add_css_class("editor-done-button");
    buttons.append(&cancel);
    buttons.append(&cont);

    card.append(&title_label);
    card.append(&body_label);
    card.append(&skip);
    card.append(&buttons);
    overlay.append(&card);
    overlay.set_visible(true);

    cancel.connect_clicked({
        let overlay = overlay.clone();
        move |_| overlay.set_visible(false)
    });
    cont.connect_clicked({
        let overlay = overlay.clone();
        let skip = skip.clone();
        let skip_field = skip_field.to_string();
        move |_| {
            if skip.is_active() {
                let mut config = load_config();
                match skip_field.as_str() {
                    "static_to_motion" => config.skip_static_to_motion_confirm = true,
                    _ => config.skip_motion_to_static_confirm = true,
                }
                let _ = save_config(&config);
            }
            overlay.set_visible(false);
            on_continue();
        }
    });
}

fn format_duration_label(duration: f64) -> String {
    format!("{duration:.1}s")
}

fn span_slider_row(title: &str, initial: f64, min: f64, max: f64) -> (GtkBox, Label, FillSlider) {
    let header = GtkBox::new(Orientation::Horizontal, 8);
    let value = Label::new(Some(&format!("{:.0}%", initial * 100.0)));
    header.set_visible(false);
    let slider =
        FillSlider::new_with_value_text(title, |value, _, _| format!("{:.0}%", value * 100.0));
    slider.set_range(min, max);
    slider.set_increments(0.01, 0.1);
    slider.set_value(initial);
    (header, value, slider)
}

fn percent_slider_row(title: &str, initial: f64) -> (GtkBox, Label, FillSlider) {
    let header = GtkBox::new(Orientation::Horizontal, 8);
    let value = Label::new(Some(&format!("{:.0}%", initial * 100.0)));
    header.set_visible(false);
    let slider =
        FillSlider::new_with_value_text(title, |value, _, _| format!("{:.0}%", value * 100.0));
    slider.set_range(MIN_MOTION_POS, MAX_MOTION_POS);
    slider.set_increments(0.01, 0.1);
    slider.set_value(initial);
    (header, value, slider)
}

fn angle_slider_row(title: &str, initial: f64) -> (GtkBox, Label, FillSlider) {
    let header = GtkBox::new(Orientation::Horizontal, 8);
    let value = Label::new(Some(&format!("{initial:.0}°")));
    header.set_visible(false);
    let slider = FillSlider::new_with_value_text(title, |value, _, _| format!("{value:.0}°"));
    slider.set_range(MIN_MOTION_YAW, MAX_MOTION_YAW);
    slider.set_increments(1.0, 5.0);
    slider.set_value(initial);
    (header, value, slider)
}

fn draw_motion_preview(
    context: &Context,
    width: i32,
    height: i32,
    runtime: &Rc<RefCell<MotionRuntime>>,
    prefers_dark: bool,
) {
    let runtime = runtime.borrow();
    let Some(surface) = runtime.card.as_ref() else {
        crate::capture::editor::render::draw_canvas_checkerboard_background(
            context,
            width,
            height,
            None,
            !prefers_dark,
        );
        return;
    };
    super::motion_render::draw_motion_frame(
        context,
        width,
        height,
        surface,
        &runtime.motion,
        runtime.motion.playhead,
        true,
        prefers_dark,
        runtime.live_preview || runtime.playing,
    );
}

pub(super) fn install_confirm_overlay(root_overlay: &Overlay, confirm: &GtkBox) {
    root_overlay.add_overlay(confirm);
    root_overlay.set_clip_overlay(confirm, true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::state::EditorState;
    use crate::capture::editor::types::{AnnotationAction, Point, Rect};
    use image::RgbaImage;

    fn blank_state() -> EditorState {
        let image = RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 255]));
        EditorState::new(image)
    }

    #[test]
    fn snapshot_is_required_when_annotations_or_crop_exist() {
        let mut state = blank_state();
        assert!(!annotations_need_snapshot(&state));
        state.actions.push(AnnotationAction::Line {
            start: Point { x: 0.0, y: 0.0 },
            end: Point { x: 4.0, y: 4.0 },
            color: crate::capture::editor::types::DrawColor::new(1.0, 1.0, 1.0, 1.0),
            stroke_size: 2.0,
            shadow: false,
        });
        assert!(annotations_need_snapshot(&state));
        state.actions.clear();
        state.crop_selection = Some(Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        });
        assert!(annotations_need_snapshot(&state));
    }

    #[test]
    fn motion_page_names_match_chrome_stacks() {
        assert_eq!(MOTION_PAGE, "motion");
        assert_eq!(STATIC_PAGE, "static");
    }

    #[test]
    fn default_duration_matches_shotbase_still_length() {
        assert!((MotionState::default().duration - 3.0).abs() < f64::EPSILON);
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
}
