use crate::recording::editor::cursor_sprite;
use crate::recording::editor::model::{
    nearest_zoom_preset, ClickEffect, CursorMotionStyle, CursorTheme, EditorTool, VideoEditState,
    ZoomEasing, ZoomMode, CLIP_SPEED_PRESETS, DEFAULT_ZOOM_EASE_MS, MAX_CLICK_DURATION_MS,
    MAX_CLICK_SCALE, MAX_CURSOR_SIZE, MAX_CURSOR_SPEED, MAX_ZOOM_EASE_MS, MIN_CLICK_DURATION_MS,
    MIN_CLICK_SCALE, MIN_CURSOR_SIZE, MIN_CURSOR_SPEED, MIN_ZOOM_EASE_MS, ZOOM_SCALE_PRESETS,
};
use gtk4::{
    gdk, glib, prelude::*, Align, Box as GtkBox, Button, ColorChooserDialog, DrawingArea,
    GestureClick, GestureDrag, Grid, Image, Label, Orientation, Overlay, PolicyType,
    ScrolledWindow, Switch, ToggleButton, Widget, Window,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::i18n::t;

pub(super) const TOOL_SIDEBAR_WIDTH: i32 = 288;

pub(super) type PausePlayback = Rc<dyn Fn()>;

pub(super) struct ToolSidebar {
    pub widget: GtkBox,
    pub refresh: Rc<dyn Fn()>,
}

pub(super) fn build_tool_sidebar(
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
    pause_playback: PausePlayback,
) -> ToolSidebar {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("recording-editor-tool-sidebar");
    root.set_hexpand(false);
    root.set_vexpand(true);
    root.set_size_request(TOOL_SIDEBAR_WIDTH, -1);

    let cursor_panel = build_cursor_panel(state.clone(), on_change.clone(), pause_playback.clone());
    let zoom_panel = build_zoom_panel(state.clone(), on_change.clone(), pause_playback.clone());
    let hide_panel = build_hide_panel(state.clone(), on_change.clone());
    let clip_panel = build_clip_panel(state.clone(), on_change, pause_playback);
    root.append(&cursor_panel.widget);
    root.append(&zoom_panel.widget);
    root.append(&hide_panel.widget);
    root.append(&clip_panel.widget);
    root.set_visible(true);
    let last_zoom = Rc::new(Cell::new(false));

    let refresh = {
        let state = state.clone();
        let refresh_cursor = cursor_panel.refresh;
        let refresh_zoom = zoom_panel.refresh;
        let refresh_hide = hide_panel.refresh;
        let refresh_clip = clip_panel.refresh;
        let last_zoom = last_zoom.clone();
        Rc::new(move || {
            let guard = state.lock().unwrap();
            let tool = guard.selected_tool;
            let zoom = guard.selected_zoom.is_some();
            let hide = guard.selected_cursor_hide.is_some();
            let clip = guard.selected_segment.is_some();
            let pointer_data = guard.supports_auto_zoom();
            drop(guard);
            if zoom {
                last_zoom.set(true);
            } else if clip || hide {
                last_zoom.set(false);
            }
            let show_cursor = tool == EditorTool::Cursor;
            let show_zoom =
                !show_cursor && (zoom || (!clip && !hide && (last_zoom.get() || pointer_data)));
            let show_hide = !show_cursor && !show_zoom && hide;
            cursor_panel.widget.set_visible(show_cursor);
            zoom_panel.widget.set_visible(show_zoom);
            hide_panel.widget.set_visible(show_hide);
            clip_panel
                .widget
                .set_visible(!show_cursor && !show_zoom && !show_hide);
            if show_cursor {
                refresh_cursor();
            } else if show_zoom {
                refresh_zoom();
            } else if show_hide {
                refresh_hide();
            } else {
                refresh_clip();
            }
        }) as Rc<dyn Fn()>
    };

    ToolSidebar {
        widget: root,
        refresh,
    }
}

include!("tool_sidebar_cursor.rs");

struct ZoomPanel {
    widget: GtkBox,
    refresh: Rc<dyn Fn()>,
}

fn build_zoom_panel(
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
    pause_playback: PausePlayback,
) -> ZoomPanel {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("recording-editor-zoom-panel");
    panel.set_hexpand(true);
    panel.set_vexpand(true);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.add_css_class("recording-editor-zoom-header");
    header.set_hexpand(true);
    let title = Label::new(Some(&t("Zoom")));
    title.add_css_class("recording-editor-zoom-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("recording-editor-zoom-body");
    body.set_hexpand(true);

    let mode_row = GtkBox::new(Orientation::Horizontal, 0);
    mode_row.add_css_class("recording-editor-zoom-mode");
    mode_row.set_hexpand(true);
    mode_row.set_homogeneous(true);
    let auto_available = state.lock().unwrap().supports_auto_zoom();
    let auto_btn = ToggleButton::with_label(&t("Auto"));
    auto_btn.add_css_class("recording-editor-zoom-mode-btn");
    auto_btn.set_has_frame(false);
    auto_btn.set_hexpand(true);
    auto_btn.set_sensitive(auto_available);
    auto_btn.set_active(auto_available);
    let manual_btn = ToggleButton::with_label(&t("Manual"));
    manual_btn.add_css_class("recording-editor-zoom-mode-btn");
    manual_btn.set_has_frame(false);
    manual_btn.set_hexpand(true);
    manual_btn.set_group(Some(&auto_btn));
    if !auto_available {
        manual_btn.set_active(true);
    }
    mode_row.append(&auto_btn);
    mode_row.append(&manual_btn);

    let mode_hint = Label::new(Some(&if auto_available {
        t("Camera recenters when the cursor nears the edge of the zoomed view")
    } else {
        t("Set a fixed focus point for this zoom")
    }));
    mode_hint.add_css_class("recording-editor-zoom-hint");
    mode_hint.set_wrap(true);
    mode_hint.set_xalign(0.0);
    mode_hint.set_max_width_chars(34);

    let chips = Grid::new();
    chips.add_css_class("recording-editor-zoom-chips");
    chips.set_column_spacing(4);
    chips.set_row_spacing(4);
    chips.set_column_homogeneous(true);
    chips.set_hexpand(true);
    let syncing = Rc::new(Cell::new(false));
    let chip_buttons: Vec<Button> = ZOOM_SCALE_PRESETS
        .iter()
        .enumerate()
        .map(|(i, &(label, scale))| {
            let chip = Button::with_label(label);
            chip.add_css_class("recording-editor-zoom-chip");
            chip.set_hexpand(true);
            chip.set_has_frame(false);
            chip.connect_clicked({
                let state = state.clone();
                let on_change = on_change.clone();
                let syncing = syncing.clone();
                move |_| {
                    if syncing.get() {
                        return;
                    }
                    state.lock().unwrap().set_selected_zoom_scale(scale);
                    on_change();
                }
            });
            chips.attach(&chip, (i % 3) as i32, (i / 3) as i32, 1, 1);
            chip
        })
        .collect();

    let animation_header = GtkBox::new(Orientation::Horizontal, 8);
    animation_header.add_css_class("recording-editor-zoom-section-row");
    animation_header.set_hexpand(true);
    let animation_label = Label::new(Some(&t("Animation")));
    animation_label.add_css_class("recording-editor-zoom-kicker");
    animation_label.set_xalign(0.0);
    animation_label.set_hexpand(true);
    let reset = Button::with_label(&t("Reset"));
    reset.add_css_class("recording-editor-zoom-reset");
    reset.set_has_frame(false);
    reset.set_halign(Align::End);
    animation_header.append(&animation_label);
    animation_header.append(&reset);

    let classic_row = GtkBox::new(Orientation::Horizontal, 8);
    classic_row.add_css_class("recording-editor-zoom-classic");
    classic_row.set_hexpand(true);
    let classic_label = Label::new(Some(&t("Classic Animation")));
    classic_label.add_css_class("recording-editor-zoom-classic-label");
    classic_label.set_xalign(0.0);
    classic_label.set_hexpand(true);
    let classic = Switch::new();
    classic.add_css_class("recording-editor-zoom-switch");
    classic.set_valign(Align::Center);
    classic.set_halign(Align::End);
    classic_row.append(&classic_label);
    classic_row.append(&classic);

    let easing_label = Label::new(Some(&t("Easing")));
    easing_label.add_css_class("recording-editor-zoom-kicker");
    easing_label.add_css_class("recording-editor-zoom-easing-kicker");
    easing_label.set_xalign(0.0);
    let easing_row = GtkBox::new(Orientation::Horizontal, 6);
    easing_row.add_css_class("recording-editor-zoom-easing");
    easing_row.set_hexpand(true);
    easing_row.set_homogeneous(true);
    let easing_buttons: Vec<(ZoomEasing, ToggleButton)> = ZoomEasing::ALL
        .iter()
        .map(|&easing| {
            let button = ToggleButton::with_label(&t(easing.label()));
            button.add_css_class("recording-editor-zoom-easing-btn");
            button.set_has_frame(false);
            button.set_hexpand(true);
            button.connect_toggled({
                let state = state.clone();
                let on_change = on_change.clone();
                let syncing = syncing.clone();
                move |button| {
                    if syncing.get() || !button.is_active() {
                        return;
                    }
                    state.lock().unwrap().set_selected_zoom_easing(easing);
                    on_change();
                }
            });
            easing_row.append(&button);
            (easing, button)
        })
        .collect();
    let first_easing = easing_buttons[0].1.clone();
    for (index, (_, button)) in easing_buttons.iter().enumerate() {
        if index > 0 {
            button.set_group(Some(&first_easing));
        }
    }
    let ease_row = cursor_slider_row(&t("Ease"));
    ease_row.widget.add_css_class("recording-editor-zoom-ease");
    ease_row
        .scale
        .set_range(MIN_ZOOM_EASE_MS as f64, MAX_ZOOM_EASE_MS as f64);
    ease_row.scale.set_increments(20.0, 100.0);
    ease_row.scale.connect_value_changed({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |scale| {
            if syncing.get() {
                return;
            }
            state
                .lock()
                .unwrap()
                .set_selected_zoom_ease_ms(scale.value().round() as u32);
            on_change();
        }
    });

    let footer_delete = delete_tool_button(&t("Delete zoom"));

    body.append(&mode_row);
    body.append(&mode_hint);
    body.append(&chips);
    body.append(&animation_header);
    body.append(&classic_row);
    body.append(&easing_label);
    body.append(&easing_row);
    body.append(&ease_row.widget);

    let scroll = ScrolledWindow::new();
    scroll.add_css_class("recording-editor-zoom-scroll");
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_child(Some(&body));

    let footer = GtkBox::new(Orientation::Horizontal, 6);
    footer.add_css_class("recording-editor-zoom-footer");
    footer.set_hexpand(true);
    footer.append(&footer_delete);

    panel.append(&header);
    panel.append(&scroll);
    panel.append(&footer);

    auto_btn.connect_toggled({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() || !button.is_active() {
                return;
            }
            state.lock().unwrap().set_selected_zoom_mode(ZoomMode::Auto);
            on_change();
        }
    });
    manual_btn.connect_toggled({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() || !button.is_active() {
                return;
            }
            state
                .lock()
                .unwrap()
                .set_selected_zoom_mode(ZoomMode::Manual);
            on_change();
        }
    });
    classic.connect_state_set({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |_, active| {
            if !syncing.get() {
                pause_playback();
                let mut guard = state.lock().unwrap();
                if !guard.zoom_locked {
                    guard.zoom_classic = active;
                    drop(guard);
                    on_change();
                }
            }
            gtk4::glib::Propagation::Proceed
        }
    });
    reset.connect_clicked({
        let state = state.clone();
        let on_change = on_change.clone();
        move |_| {
            state.lock().unwrap().reset_zoom_animation();
            on_change();
        }
    });
    let delete = {
        let state = state.clone();
        let on_change = on_change.clone();
        Rc::new(move || {
            state.lock().unwrap().remove_selected_zoom();
            on_change();
        })
    };
    footer_delete.connect_clicked({
        let delete = delete.clone();
        move |_| delete()
    });

    let refresh = {
        let panel = panel.clone();
        let auto_btn = auto_btn.clone();
        let manual_btn = manual_btn.clone();
        let mode_hint = mode_hint.clone();
        let classic = classic.clone();
        let classic_row = classic_row.clone();
        let chip_buttons = chip_buttons.clone();
        let easing_buttons = easing_buttons.clone();
        let ease_scale = ease_row.scale.clone();
        let easing_row = easing_row.clone();
        let easing_label = easing_label.clone();
        let reset = reset.clone();
        let footer_delete = footer_delete.clone();
        let syncing = syncing.clone();
        Rc::new(move || {
            let guard = state.lock().unwrap();
            panel.set_visible(true);
            let auto_available = guard.supports_auto_zoom();
            let selected = guard.selected_zoom_clip().cloned();
            let has_clip = selected.is_some();
            let can_edit = has_clip && !guard.zoom_locked;
            syncing.set(true);
            auto_btn.set_sensitive(auto_available && can_edit);
            manual_btn.set_sensitive(can_edit);
            classic.set_sensitive(can_edit);
            reset.set_sensitive(can_edit);
            easing_row.set_sensitive(can_edit);
            easing_label.set_sensitive(can_edit);
            ease_scale.set_sensitive(can_edit);
            footer_delete.set_sensitive(can_edit);
            if let Some(clip) = &selected {
                let mode = if clip.mode == ZoomMode::Auto && auto_available {
                    ZoomMode::Auto
                } else {
                    ZoomMode::Manual
                };
                mode_hint.set_text(&match mode {
                    ZoomMode::Manual => t("Set a fixed focus point for this zoom"),
                    ZoomMode::Auto => {
                        t("Camera recenters when the cursor nears the edge of the zoomed view")
                    }
                });
                match mode {
                    ZoomMode::Auto => auto_btn.set_active(true),
                    ZoomMode::Manual => manual_btn.set_active(true),
                }
                classic_row.set_visible(mode == ZoomMode::Auto);
            } else {
                mode_hint.set_text(&if auto_available {
                    t("Select a zoom to adjust it; timeline detection uses clicks and pointer pauses")
                } else {
                    t("Add a Manual zoom, or analyze visible cursor motion from the timeline")
                });
                if !auto_available {
                    manual_btn.set_active(true);
                }
                classic_row.set_visible(false);
            }
            classic.set_active(guard.zoom_classic);
            let selected_easing = selected
                .as_ref()
                .map(|clip| clip.easing)
                .unwrap_or(ZoomEasing::Glide);
            for (easing, button) in &easing_buttons {
                button.set_active(*easing == selected_easing);
            }
            ease_scale.set_value(
                selected
                    .as_ref()
                    .map(|clip| clip.ease_ms as f64)
                    .unwrap_or(DEFAULT_ZOOM_EASE_MS as f64),
            );
            let selected_preset = selected
                .as_ref()
                .map(|clip| nearest_zoom_preset(clip.scale));
            for (chip, &(_, scale)) in chip_buttons.iter().zip(ZOOM_SCALE_PRESETS.iter()) {
                chip.set_sensitive(can_edit);
                let active = selected_preset.is_some_and(|preset| (scale - preset).abs() < 1e-6);
                if active {
                    chip.add_css_class("recording-editor-zoom-chip-active");
                } else {
                    chip.remove_css_class("recording-editor-zoom-chip-active");
                }
            }
            syncing.set(false);
        }) as Rc<dyn Fn()>
    };

    ZoomPanel {
        widget: panel,
        refresh,
    }
}

struct ClipPanel {
    widget: GtkBox,
    refresh: Rc<dyn Fn()>,
}

fn build_clip_panel(
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
    pause_playback: PausePlayback,
) -> ClipPanel {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("recording-editor-zoom-panel");
    panel.set_hexpand(true);
    panel.set_vexpand(true);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.add_css_class("recording-editor-zoom-header");
    header.set_hexpand(true);
    let title = Label::new(Some(&t("Clip")));
    title.add_css_class("recording-editor-zoom-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("recording-editor-zoom-body");
    body.set_hexpand(true);

    let speed_label = Label::new(Some(&t("Speed")));
    speed_label.add_css_class("recording-editor-zoom-kicker");
    speed_label.set_xalign(0.0);

    let chips = Grid::new();
    chips.add_css_class("recording-editor-zoom-chips");
    chips.set_column_spacing(4);
    chips.set_row_spacing(4);
    chips.set_column_homogeneous(true);
    chips.set_hexpand(true);
    let syncing = Rc::new(Cell::new(false));
    let chip_buttons: Vec<Button> = CLIP_SPEED_PRESETS
        .iter()
        .enumerate()
        .map(|(i, &(label, speed))| {
            let chip = Button::with_label(label);
            chip.add_css_class("recording-editor-zoom-chip");
            chip.set_hexpand(true);
            chip.set_has_frame(false);
            chip.connect_clicked({
                let state = state.clone();
                let on_change = on_change.clone();
                let syncing = syncing.clone();
                move |_| {
                    if syncing.get() {
                        return;
                    }
                    state.lock().unwrap().set_selected_clip_speed(speed);
                    on_change();
                }
            });
            chips.attach(&chip, (i % 4) as i32, (i / 4) as i32, 1, 1);
            chip
        })
        .collect();

    let audio_header = GtkBox::new(Orientation::Horizontal, 8);
    audio_header.add_css_class("recording-editor-zoom-section-row");
    audio_header.set_hexpand(true);
    let audio_label = Label::new(Some(&t("Audio")));
    audio_label.add_css_class("recording-editor-zoom-kicker");
    audio_label.set_xalign(0.0);
    audio_header.append(&audio_label);

    let mute_row = GtkBox::new(Orientation::Horizontal, 8);
    mute_row.add_css_class("recording-editor-zoom-classic");
    mute_row.set_hexpand(true);
    let mute_label = Label::new(Some(&t("Mute")));
    mute_label.add_css_class("recording-editor-zoom-classic-label");
    mute_label.set_xalign(0.0);
    mute_label.set_hexpand(true);
    mute_label.set_valign(Align::Center);
    let mute = Switch::new();
    mute.add_css_class("recording-editor-zoom-switch");
    mute.set_valign(Align::Center);
    mute.set_halign(Align::End);
    mute_row.append(&mute_label);
    mute_row.append(&mute);

    body.append(&speed_label);
    body.append(&chips);
    body.append(&audio_header);
    body.append(&mute_row);

    let scroll = ScrolledWindow::new();
    scroll.add_css_class("recording-editor-zoom-scroll");
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_child(Some(&body));

    let footer_delete = delete_tool_button(&t("Delete clip"));
    let footer = GtkBox::new(Orientation::Horizontal, 6);
    footer.add_css_class("recording-editor-zoom-footer");
    footer.set_hexpand(true);
    footer.append(&footer_delete);

    panel.append(&header);
    panel.append(&scroll);
    panel.append(&footer);

    mute.connect_state_set({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        let pause_playback = pause_playback.clone();
        move |_, active| {
            if !syncing.get() {
                pause_playback();
                state.lock().unwrap().set_selected_clip_muted(active);
                on_change();
            }
            gtk4::glib::Propagation::Proceed
        }
    });
    footer_delete.connect_clicked({
        let state = state.clone();
        let on_change = on_change.clone();
        move |_| {
            state.lock().unwrap().remove_selected_clip();
            on_change();
        }
    });

    let refresh = {
        let panel = panel.clone();
        let mute = mute.clone();
        let chip_buttons = chip_buttons.clone();
        let footer_delete = footer_delete.clone();
        let syncing = syncing.clone();
        Rc::new(move || {
            let guard = state.lock().unwrap();
            panel.set_visible(true);
            let speed = guard.selected_clip_speed();
            let muted = guard.selected_clip_muted().unwrap_or(false);
            let can_edit = speed.is_some() && !guard.video_locked;
            let can_mute = can_edit && guard.has_audio_track() && !guard.audio_locked;
            syncing.set(true);
            mute.set_active(muted);
            for (chip, &(_, preset)) in chip_buttons.iter().zip(CLIP_SPEED_PRESETS.iter()) {
                chip.set_sensitive(can_edit);
                let active = speed.is_some_and(|value| (value - preset).abs() < 1e-6);
                if active {
                    chip.add_css_class("recording-editor-zoom-chip-active");
                } else {
                    chip.remove_css_class("recording-editor-zoom-chip-active");
                }
            }
            mute.set_sensitive(can_mute);
            footer_delete.set_sensitive(guard.selected_segment.is_some() && !guard.video_locked);
            syncing.set(false);
        }) as Rc<dyn Fn()>
    };

    ClipPanel {
        widget: panel,
        refresh,
    }
}

struct HidePanel {
    widget: GtkBox,
    refresh: Rc<dyn Fn()>,
}

fn build_hide_panel(state: Arc<Mutex<VideoEditState>>, on_change: Rc<dyn Fn()>) -> HidePanel {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("recording-editor-zoom-panel");
    panel.set_hexpand(true);
    panel.set_vexpand(true);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.add_css_class("recording-editor-zoom-header");
    header.set_hexpand(true);
    let title = Label::new(Some(&t("Hide cursor")));
    title.add_css_class("recording-editor-zoom-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);

    let body = GtkBox::new(Orientation::Vertical, 8);
    body.add_css_class("recording-editor-zoom-body");
    body.set_hexpand(true);
    let hint = Label::new(Some(&t(
        "Cursor is hidden for this range in preview and export",
    )));
    hint.add_css_class("recording-editor-zoom-hint");
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    hint.set_max_width_chars(34);
    body.append(&hint);

    let scroll = ScrolledWindow::new();
    scroll.add_css_class("recording-editor-zoom-scroll");
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_child(Some(&body));

    let footer_delete = delete_tool_button(&t("Delete hide"));
    footer_delete.connect_clicked({
        let state = state.clone();
        let on_change = on_change.clone();
        move |_| {
            state.lock().unwrap().remove_selected_cursor_hide();
            on_change();
        }
    });
    let footer = GtkBox::new(Orientation::Horizontal, 6);
    footer.add_css_class("recording-editor-zoom-footer");
    footer.set_hexpand(true);
    footer.append(&footer_delete);

    panel.append(&header);
    panel.append(&scroll);
    panel.append(&footer);

    let refresh = {
        let panel = panel.clone();
        Rc::new(move || {
            panel.set_visible(true);
        }) as Rc<dyn Fn()>
    };

    HidePanel {
        widget: panel,
        refresh,
    }
}

fn delete_tool_button(label: &str) -> Button {
    let button = Button::new();
    button.add_css_class("recording-editor-zoom-delete");
    button.set_has_frame(false);
    button.set_halign(Align::Start);
    let row = GtkBox::new(Orientation::Horizontal, 6);
    row.set_halign(Align::Start);
    let icon = Image::from_icon_name("user-trash-symbolic");
    icon.set_pixel_size(13);
    let text = Label::new(Some(label));
    row.append(&icon);
    row.append(&text);
    button.set_child(Some(&row));
    button
}
