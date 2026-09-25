use crate::recording::editor::cursor_sprite;
use crate::recording::editor::model::{
    nearest_zoom_preset, ClickEffect, CursorMotionStyle, CursorTheme, EditorTool, VideoBackground,
    VideoEditState, ZoomEasing, ZoomMode, CLIP_SPEED_PRESETS, MAX_CLICK_DURATION_MS,
    MAX_CLICK_SCALE, MAX_CURSOR_SIZE, MAX_CURSOR_SPEED, MIN_CLICK_DURATION_MS, MIN_CLICK_SCALE,
    MIN_CURSOR_SIZE, MIN_CURSOR_SPEED, ZOOM_SCALE_PRESETS,
};
use gtk4::{
    gdk, glib, prelude::*, Align, Box as GtkBox, Button, ColorChooserDialog, DrawingArea,
    EventControllerMotion, GestureClick, GestureDrag, Grid, Image, Label, Orientation, Overlay,
    PolicyType, ScrolledWindow, Switch, ToggleButton, Widget, Window,
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
    let background_panel = build_background_panel(state.clone(), on_change.clone());
    let zoom_panel = build_zoom_panel(state.clone(), on_change.clone(), pause_playback.clone());
    let hide_panel = build_hide_panel(state.clone(), on_change.clone());
    let clip_panel = build_clip_panel(state.clone(), on_change, pause_playback);
    root.append(&cursor_panel.widget);
    root.append(&background_panel.widget);
    root.append(&zoom_panel.widget);
    root.append(&hide_panel.widget);
    root.append(&clip_panel.widget);
    root.set_visible(true);
    let last_zoom = Rc::new(Cell::new(false));

    let refresh = {
        let state = state.clone();
        let refresh_cursor = cursor_panel.refresh;
        let refresh_background = background_panel.refresh;
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
            let show_background = tool == EditorTool::Background;
            let show_zoom = !show_cursor
                && !show_background
                && (zoom || (!clip && !hide && (last_zoom.get() || pointer_data)));
            let show_hide = !show_cursor && !show_background && !show_zoom && hide;
            cursor_panel.widget.set_visible(show_cursor);
            background_panel.widget.set_visible(show_background);
            zoom_panel.widget.set_visible(show_zoom);
            hide_panel.widget.set_visible(show_hide);
            clip_panel
                .widget
                .set_visible(!show_cursor && !show_background && !show_zoom && !show_hide);
            if show_cursor {
                refresh_cursor();
            } else if show_background {
                refresh_background();
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
include!("tool_sidebar_background.rs");

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
    let footer_delete = delete_tool_button(&t("Delete zoom"));

    body.append(&mode_row);
    body.append(&mode_hint);
    body.append(&chips);
    body.append(&animation_header);
    body.append(&classic_row);
    body.append(&easing_label);
    body.append(&easing_row);

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

#[cfg(test)]
mod tests {
    #[test]
    fn background_panel_opens_like_cursor() {
        let source = include_str!("tool_sidebar.rs");
        assert!(
            source.contains("build_background_panel"),
            "right sidebar must build a Background panel"
        );
        assert!(
            source.contains("EditorTool::Background"),
            "right sidebar must show Background when the rail selects it"
        );
        let panel = include_str!("tool_sidebar_background.rs");
        assert!(
            panel.contains("VideoBackground::Wallpaper"),
            "wallpaper picks must store VideoBackground::Wallpaper"
        );
        assert!(
            panel.contains("VideoBackground::Plain"),
            "Background panel must offer solid colors"
        );
        assert!(
            panel.contains("VideoBackground::Gradient"),
            "the Custom source must be able to hold a hand-drawn gradient"
        );
        assert!(
            !panel.contains("bg_placeholder_section"),
            "stroke and shadow were never rendered, so the panel must not advertise them"
        );
        assert!(
            panel.contains("editor-motion-wallpaper-thumbnail"),
            "wallpaper tiles must reuse the image editor chrome"
        );
        assert!(
            panel.contains("set_content_width(56)"),
            "wallpaper tiles must be fixed-size so the grid never stretches the sidebar"
        );
        // The redesigned panel picks a fill source up front rather than
        // offering a separate mode row.
        for tab in ["Wallpaper", "Custom", "Image"] {
            assert!(
                panel.contains(&format!("bg_tab_button(&t(\"{tab}\"))")),
                "Background panel must offer a {tab} source tab"
            );
        }
        assert!(
            panel.contains("background_corner_radius"),
            "Radius must be wired to the corner-radius state"
        );
    }

    #[test]
    fn the_wallpaper_grid_is_not_gated_behind_a_picked_fill() {
        // The grid used to show only once a wallpaper was already selected, so
        // the Wallpaper tab opened on an empty page. The open page is view
        // state and defaults to Wallpaper whatever the model's fill holds.
        assert!(
            include_str!("tool_sidebar_background.rs")
                .contains("Rc::new(Cell::new(BgPage::Wallpaper))"),
            "the panel must open on the wallpaper page by default"
        );
    }

    #[test]
    fn wallpaper_tiles_come_from_the_image_editor_assets() {
        let panel = include_str!("tool_sidebar_background.rs");
        assert!(
            panel.contains("MOTION_WALLPAPER_FILES"),
            "wallpaper tiles must reuse the image editor's bundled wallpapers"
        );
        assert!(
            panel.contains("editor-motion-wallpaper-thumbnail"),
            "wallpaper tiles must reuse the image editor's tile chrome"
        );
    }

    #[test]
    fn the_open_tab_is_not_overridden_by_the_current_fill() {
        // refresh used to infer the page from the fill, so switching to
        // Custom wrote a plain fill and then pinned the page there — going
        // back to Wallpaper snapped away again. The tab must be the only
        // thing that moves the page.
        let panel = include_str!("tool_sidebar_background.rs");
        let refresh_start = panel.find("Rc::new(move || {").unwrap();
        let refresh = &panel[refresh_start..];
        assert!(
            !refresh.contains("active_page.set(match (&background"),
            "refresh must not infer the page from the model's fill"
        );
    }

    #[test]
    fn the_value_rows_belong_to_the_custom_page() {
        // Padding, Radius and the custom-fill row were appended to the panel
        // body, so they rendered on the Wallpaper and Image tabs too and every
        // tab looked like Custom. They must be appended to the Custom page.
        let panel = include_str!("tool_sidebar_background.rs");
        assert!(
            panel.contains("custom_page.append(&padding_row.widget);"),
            "Padding must live inside the Custom page, not the panel frame"
        );
        assert!(
            panel.contains("custom_page.append(&radius_row.widget);"),
            "Radius must live inside the Custom page, not the panel frame"
        );
        assert!(
            !panel.contains("body.append(&padding_row.widget);"),
            "the panel frame must not show the value rows on every tab"
        );
    }

    #[test]
    fn the_custom_page_tunes_the_fill_before_offering_a_new_one() {
        // The Custom page reads as a sequence: set the fill's size, round its
        // corners, then pick a new fill. Pinning the order stops the rows from
        // drifting apart as the page is edited.
        let panel = include_str!("tool_sidebar_background.rs");
        let order = [
            "custom_page.append(&padding_row.widget);",
            "custom_page.append(&radius_row.widget);",
            "custom_page.append(&custom_row);",
        ];
        let mut cursor = 0;
        for step in order {
            let at = panel[cursor..]
                .find(step)
                .unwrap_or_else(|| panic!("Custom page must append {step}"));
            cursor += at + step.len();
        }
    }

    #[test]
    fn padding_does_not_hide_until_a_fill_is_picked() {
        // Padding used to hide itself with no fill, which left the Custom page
        // showing only Radius. Scoped to Custom it is a plain fill control, and
        // a value set before a fill exists has to stay visible to be adjusted.
        let panel = include_str!("tool_sidebar_background.rs");
        assert!(
            !panel.contains("set_visible(has_fill)"),
            "neither fill slider may gate itself on a fill being picked"
        );
        assert!(
            panel.contains("padding_row_value.sync_value(padding);"),
            "padding must still sync its stored value every refresh"
        );
    }

    #[test]
    fn the_custom_row_summarizes_the_fill_and_its_edit_opens_the_dialog() {
        // The row reads as a swatch, the fill's name, and an Edit pill. GTK
        // will not nest one button inside another, so the row itself must be a
        // plain box with the click living on the Edit button — a Button row
        // would have to be the whole click target again.
        let panel = include_str!("tool_sidebar_background.rs");
        assert!(
            panel.contains("let custom_row = GtkBox::new(Orientation::Horizontal, 10);"),
            "the custom row must be a box, not a button, to hold the Edit pill"
        );
        assert!(
            panel.contains("let custom_edit = Button::with_label(&t(\"Edit\"));"),
            "the row needs its own Edit button"
        );
        assert!(
            panel.contains("build_custom_wallpaper_popover(") && panel.contains("&custom_edit,"),
            "Edit, not the whole row, is what opens the Custom Wallpaper dialog"
        );
        assert!(
            !panel.contains("custom_row.connect_clicked("),
            "the row must not also be a button, or Edit would sit inside a button"
        );
        // The name tracks the fill so the row summarizes rather than commands.
        assert!(
            panel.contains("custom_label.set_text(&kind);"),
            "the row must name whichever custom fill is active"
        );
        for name in ["t(\"Color\")", "t(\"Gradient\")"] {
            assert!(
                panel.contains(name),
                "the custom row must be able to show {name}"
            );
        }
    }

    #[test]
    fn only_the_edit_pill_looks_clickable_on_the_custom_row() {
        // Nothing on the row is clickable but Edit, so nothing on it may look
        // clickable either. A 999px pill on a 40px row read as one, so the row
        // and the pill now share the FillSlider track's 8px radius instead —
        // they sit directly under Padding and Radius and have to match them.
        let css = include_str!("../ui_support_css/09.css");
        for class in [
            ".recording-editor-bg-custom-row {",
            "button.recording-editor-bg-custom-edit {",
        ] {
            let start = css
                .find(class)
                .unwrap_or_else(|| panic!("09.css must define {class}"));
            let block = &css[start..];
            let end = block.find('}').expect("the rule is closed");
            assert!(
                block[..end].contains("border-radius: 8px;"),
                "{class} must share the FillSlider track's 8px radius"
            );
            assert!(
                !block[..end].contains("999px"),
                "{class} must not be a pill — that reads as a button"
            );
        }
    }

    #[test]
    fn the_custom_wallpaper_picker_draws_its_own_surface() {
        // The app ships no libadwaita, so an unstyled popover falls through to
        // the host desktop's GTK theme and the card stops matching the editor.
        // Both halves of the strip-then-paint pattern have to stay: the popover
        // node gives up the theme's surface, the body draws ours. Dropping
        // either one silently reintroduces the mismatch, so pin both.
        let css = include_str!("../ui_support_css/09.css");
        let strip = css
            .find("popover.recording-editor-custom-popover,")
            .expect("09.css must strip the inherited GTK popover surface");
        let strip_end = css[strip..].find('}').expect("the rule is closed") + strip;
        for property in [
            "background: transparent;",
            "border: none;",
            "box-shadow: none;",
        ] {
            assert!(
                css[strip..strip_end].contains(property),
                "the popover must give up the desktop theme's surface ({property})"
            );
        }

        let body = css
            .find(".recording-editor-custom-body {")
            .expect("09.css must define the picker surface");
        let body_end = css[body..].find('}').expect("the rule is closed") + body;
        for property in [
            "background: #1d1d1d;",
            "border-radius: 12px;",
            "border: 1px solid alpha(white, 0.10);",
            "box-shadow: 0 12px 32px alpha(black, 0.45);",
        ] {
            assert!(
                css[body..body_end].contains(property),
                "the picker body must paint the app's own card ({property})"
            );
        }

        // The light theme needs its own surface or the dark card shows through.
        assert!(
            css.contains(".editor-theme-light .recording-editor-custom-body {"),
            "the picker surface needs a light-theme counterpart"
        );
    }

    #[test]
    fn the_picker_value_row_is_one_pill_not_a_box_in_a_box() {
        // The value row already carries the pill surface, so the entry inside
        // it has to be flat. `.recording-editor-root entry` in 01.css paints a
        // 6px radius, alpha(white, 0.06) fill, and a border, and it outranks a
        // bare `.recording-editor-custom-hex` — so the reset has to match that
        // specificity or the hex reads as a second box nested in the first.
        let css = include_str!("../ui_support_css/09.css");
        let class = ".recording-editor-custom-value-row .recording-editor-custom-hex {";
        let start = css
            .find(class)
            .unwrap_or_else(|| panic!("09.css must define {class}"));
        let block = &css[start..];
        let end = block.find('}').expect("the rule is closed");
        for property in [
            "background: transparent;",
            "background-image: none;",
            "border: none;",
            "box-shadow: none;",
        ] {
            assert!(
                block[..end].contains(property),
                "the hex entry must sit flat inside the value row ({property})"
            );
        }
    }

    #[test]
    fn the_custom_wallpaper_popover_opens_beside_the_sidebar() {
        // Placement is the point of this UI: the card is parented to the side
        // panel and points at the panel's left edge, so it floats over the
        // video stage instead of covering the panel it is editing or centering
        // on the window. The Edit pill only seats it level with the row.
        let source = include_str!("custom_wallpaper_popover.rs");
        assert!(
            source.contains("popover.set_position(gtk4::PositionType::Left)"),
            "the popover must open left of the sidebar, not centered on the window"
        );
        assert!(
            source.contains("popover.set_parent(sidebar)"),
            "the popover must be parented to the side panel"
        );
        assert!(
            source.contains("edit.compute_bounds(&sidebar)"),
            "the card must read the Edit row's bounds so it tracks the panel's scroll"
        );
        assert!(
            source.contains("popover.set_pointing_to("),
            "the card must point at the panel's left edge, not at the row inside the panel"
        );
        assert!(
            source.contains("-POPOVER_SIDEBAR_GAP"),
            "the card must be anchored clear of the panel's edge so it does not butt against the controls"
        );
        assert!(
            source.contains("popover.set_has_arrow(false)"),
            "the popover is a panel, not a tooltip"
        );
    }

    #[test]
    fn the_color_field_is_a_pickable_plane_not_a_flat_swatch() {
        // The reference's big field is a saturation/value plane with a handle
        // inside it — that red-to-dark wash. Drawing a flat fill there instead
        // produced an empty black box with nothing to pick, so both halves of
        // the picker have to exist and be draggable.
        let source = include_str!("custom_wallpaper_popover.rs");
        assert!(
            source.contains("fn draw_plane("),
            "the field must be a saturation/value plane, not a flat fill"
        );
        assert!(
            source.contains("fn attach_plane_drag("),
            "the plane must be draggable, or there is no way to pick saturation or value"
        );
        // The plane's handle has to be drawn, or it is a gradient with no
        // indication of where the live color sits on it.
        assert!(
            source.contains("let cx = saturation.clamp(0.0, 1.0) * w;"),
            "the plane must draw a handle at the live color's position"
        );
        // A flat fill of the current color is what produced the black box.
        let draw_plane = source.find("fn draw_plane(").expect("draw_plane exists");
        let end = source[draw_plane..]
            .find("\nfn ")
            .map(|at| draw_plane + at)
            .expect("draw_plane is closed");
        assert!(
            !source[draw_plane..end]
                .contains("fill_rounded(cr, 0.0, 0.0, w, h, FIELD_RADIUS, color)"),
            "the field must not be a flat fill of the current color"
        );
    }

    #[test]
    fn the_custom_wallpaper_popover_has_color_and_gradient_only() {
        // The reference's Image sub-tab is not built: the panel already has an
        // Image source tab, so a second one here would pick the same file twice.
        let source = include_str!("custom_wallpaper_popover.rs");
        for tab in ["t(\"Color\")", "t(\"Gradient\")"] {
            assert!(
                source.contains(tab),
                "the popover must offer a {tab} sub-tab"
            );
        }
        assert!(
            !source.contains("t(\"Image\")"),
            "the popover must not duplicate the panel's Image tab"
        );
    }

    #[test]
    fn switching_popover_tabs_does_not_write_a_fill() {
        // Opening Gradient must not replace a color the swatch is showing, so
        // the tab switch is a pure view change: it only sets visibility and
        // the active button, never the model.
        let source = include_str!("custom_wallpaper_popover.rs");
        let start = source
            .find("Rc::new(move |is_gradient: bool| {")
            .expect("the tab switch is a shared closure");
        let end = source[start..]
            .find("\n        })")
            .map(|at| start + at)
            .expect("the closure is closed");
        let body = &source[start..end];
        assert!(
            !body.contains("VideoBackground::"),
            "switching sub-tabs must not write to the background"
        );
        assert!(
            !body.contains("notify()"),
            "switching sub-tabs must not ping the editor"
        );
    }

    #[test]
    fn the_popover_opens_showing_only_the_active_page() {
        // Both pages are appended visible and the tab handlers only run once a
        // tab is toggled, so without an explicit initial sync the popover
        // opened with the Color and Gradient editors stacked on top of each
        // other until the user clicked a tab.
        let source = include_str!("custom_wallpaper_popover.rs");
        let handlers = source
            .find("color_tab.connect_toggled")
            .expect("the Color tab has a toggle handler");
        let initial_sync = source
            .find("set_page(false);")
            .expect("the popover syncs the pages to the active tab");
        assert!(
            initial_sync < handlers,
            "the active page must be selected before the tab handlers are wired, \
             or both pages show on open"
        );
    }

    #[test]
    fn the_gradient_editor_is_wired_end_to_end() {
        // Every control the reference gradient tab shows has to reach state.
        let source = include_str!("custom_wallpaper_popover.rs");
        for (needle, why) in [
            ("fn attach_stop_drag(", "stops must be draggable"),
            ("fn add_stop(", "the Steps + button must add a stop"),
            (
                "fn build_color_picker(",
                "a stop's color must be editable through the shared picker",
            ),
            (
                "recording-editor-gradient-picker",
                "the picker must be the stop editor's own mini card, not a dialog",
            ),
            (
                "GradientIcon::RotateCwSquare",
                "the angle must be rotatable, through Lucide's rotate-cw-square",
            ),
            (
                "fn draw_gradient_icon(",
                "the header glyphs must be drawn from the reference's own path data",
            ),
            (
                "GRADIENT_ROTATE_STEP",
                "the rotate control must step the angle",
            ),
            (
                "GradientIcon::ArrowLeftRight",
                "the flip control must use Lucide's arrow-left-right, not a theme icon",
            ),
            (
                "gradient.reversed = !gradient.reversed",
                "reverse must toggle",
            ),
            (
                "recording-editor-gradient-type",
                "the type picker must carry the reference's compact chip styling",
            ),
            (
                "recording-editor-dropdown-item",
                "the type picker must be the app's own dropdown pattern",
            ),
            (
                "object-select-symbolic",
                "the active type must be checked in the menu",
            ),
        ] {
            assert!(source.contains(needle), "gradient editor: {why}");
        }
        // Figma's four gradient shapes all have to be offered by the picker.
        for kind in ["Linear", "Radial", "Angular", "Diamond"] {
            assert!(
                source.contains(&format!("t(\"{kind}\")")),
                "the type picker must offer {kind}"
            );
        }
        // Stops are bounded, so the UI has to respect the same bounds the
        // model normalizes to.
        assert!(
            source.contains("MAX_GRADIENT_STOPS"),
            "adding a stop must stop at the model's maximum"
        );
    }

    #[test]
    fn a_stop_row_is_a_chip_and_a_hex_and_nothing_else() {
        // The row deliberately does not repeat position or opacity as
        // percentages. Position is the pin on the bar above, and a row that
        // also typed it would be a second, disagreeing source for the same
        // number; alpha is not part of this editor's per-stop surface.
        let source = include_str!("custom_wallpaper_popover.rs");
        for dropped in [
            "recording-editor-gradient-step-position",
            "recording-editor-gradient-step-opacity",
            "recording-editor-gradient-step-remove",
        ] {
            assert!(
                !source.contains(dropped),
                "{dropped} was dropped from the stop row and must not come back"
            );
        }
        let css = include_str!("../ui_support_css/09.css");
        for stale in [
            ".recording-editor-gradient-step-position",
            ".recording-editor-gradient-step-opacity",
            ".recording-editor-gradient-step-remove",
        ] {
            assert!(
                !css.contains(stale),
                "{stale} has no widget left and must not linger in the stylesheet"
            );
        }
        // The hex still has to be editable, or a stop's color could only be
        // changed through the picker card.
        assert!(
            source.contains("recording-editor-gradient-step-hex"),
            "the stop row must still carry an editable hex"
        );
    }

    #[test]
    fn a_stop_tile_opens_the_picker_card_for_that_stop() {
        // Editing a stop color used to open a modal chooser from inside the
        // popover, which hung the editor; then it became a second column
        // beside the ramp, which widened the whole popover. The tile now opens
        // the picker's own mini card beside it, and a tile click still moves
        // the selection the card edits.
        let source = include_str!("custom_wallpaper_popover.rs");
        assert!(
            !source.contains("ColorChooserDialog"),
            "a stop's color must be edited inline, never by a modal chooser"
        );
        let start = source
            .find("fn build_stop_row(")
            .expect("the stop row exists");
        let end = source[start..]
            .find("\nfn ")
            .map(|at| start + at)
            .unwrap_or(source.len());
        let body = &source[start..end];
        assert!(
            body.contains("selected.set(index);"),
            "clicking a stop's tile must select that stop"
        );
        assert!(
            body.contains("open_picker("),
            "clicking a stop's tile must open the picker card for it"
        );
        assert!(
            !body.contains("dialog") && !body.contains("RGBA"),
            "the row must not open a color chooser of its own"
        );
    }

    #[test]
    fn the_gradient_tab_keeps_the_popover_width() {
        // Opening the Gradient tab must not resize the popover. It used to
        // grow a picker column beside the stop editor, which widened the card
        // to roughly twice the Color tab's width. Nothing on the gradient page
        // may lay out horizontally against the editor now: the picker is a
        // separate mini card, opened from a stop tile.
        let source = include_str!("custom_wallpaper_popover.rs");
        // The popover file's own tests name the layout they forbid, so only the
        // production half of the file is inspected.
        let production = &source[..source
            .find("\n#[cfg(test)]")
            .expect("the popover has a tests module")];
        assert!(
            !production.contains("recording-editor-gradient-columns"),
            "the gradient page must not lay out as columns, or the tab widens the popover"
        );
        assert!(
            production.contains("recording-editor-gradient-picker-popover"),
            "the picker must be a popover of its own, not a column of the page"
        );
        assert!(
            production.contains("picker_popover.set_parent(card_body)"),
            "the mini card must hang off the popover's body, not the page"
        );
        let css = include_str!("../ui_support_css/09.css");
        assert!(
            !css.contains(".recording-editor-gradient-columns"),
            "the columns rule is dead and must not linger in the stylesheet"
        );
        assert!(
            css.contains("popover.recording-editor-gradient-picker-popover"),
            "the mini card needs its own surface, or the host theme paints it"
        );
    }

    #[test]
    fn the_picker_card_sits_clear_of_the_popover_and_can_close_itself() {
        // What the card's seat got wrong before it landed, in order. Seated on
        // the clicked tile, its right edge landed on the Gradient page's own
        // 12px inset — tucked under the popover — so its right-hand controls
        // were clipped. Then hand-built rects in the page's space and in the
        // popover's surface space each came out ~150px high, because GTK anchors
        // a left-positioned popover by the *corner* of the pointing rect rather
        // than the centre of its edge, and because a popover hung inside another
        // popover has its rect measured through that popover's surface. The seat
        // is now stated in the popover body's own space, with the card
        // top-aligned so the anchoring corner is the one being reasoned about.
        // The geometry itself is asserted by measurement in
        // `the_card_hangs_clear_of_the_popover_beside_it`; this test pins the
        // wiring that measurement depends on. And because the popover behind the
        // card stays open, the card needs a close of its own rather than relying
        // on a second click on the tile.
        let source = include_str!("custom_wallpaper_popover.rs");
        let production = &source[..source
            .find("\n#[cfg(test)]")
            .expect("the popover has a tests module")];
        assert!(
            production.contains("picker_popover.set_parent(card_body)"),
            "the card must hang off the popover's body: that body's space is the seat's space"
        );
        assert!(
            production.contains("picker_popover.set_valign(Align::Start)"),
            "GTK anchors this popover by the rect's corner, so the card must be top-aligned"
        );
        let start = production
            .find("let open_picker: Rc<dyn Fn(bool)>")
            .expect("the card has an opener");
        let end = production[start..]
            .find("\n    };")
            .map(|at| start + at)
            .expect("the opener is closed");
        let opener = &production[start..end];
        assert!(
            opener.contains("picker_popover.set_pointing_to"),
            "the card needs its seat, stated in the body's own coordinates"
        );
        assert!(
            opener.contains("card_body.height()"),
            "the seat must be taken from the popover's body, not from the page or a tile"
        );
        assert!(
            !opener.contains("compute_bounds"),
            "a tile-relative seat is what tucked the card under the popover"
        );
        assert!(
            !production.contains("fn host_popover("),
            "the host-popover walk is not needed once the seat is in the body's space"
        );
        assert!(
            production.contains("recording-editor-gradient-picker-close"),
            "the card must carry its own close control"
        );
        assert!(
            production.contains("card_close.connect_clicked"),
            "the card's close must actually pop the card down"
        );
        assert!(
            production.contains("window-close-symbolic"),
            "the close should be the app's own glyph, as the popover's is"
        );
        let css = include_str!("../ui_support_css/09.css");
        assert!(
            css.contains("button.recording-editor-gradient-picker-close"),
            "the close button needs chrome, or it shows the host theme's button"
        );
    }

    #[test]
    fn the_gradient_editor_draws_through_the_shared_rasterizer() {
        // Preview and export both call render_gradient, so a stop dragged off
        // even spacing cannot show one position in the popover and export
        // another. A Cairo gradient would not honor stop positions, so the
        // gradient drawing functions must not use one.
        //
        // The Color tab's saturation/value plane legitimately does use a
        // Cairo gradient — it is a hue wash, not a gradient fill, and it is
        // never exported. So the rule is scoped to the gradient editor's own
        // draw functions rather than the whole file.
        let source = include_str!("custom_wallpaper_popover.rs");
        assert!(
            source.contains("render_gradient(&flat,"),
            "the popover must render gradients with the shared rasterizer"
        );
        for draw in ["fn draw_stop_bar("] {
            let start = source
                .find(draw)
                .unwrap_or_else(|| panic!("the popover must define {draw}"));
            let end = source[start..]
                .find("\nfn ")
                .map(|at| start + at)
                .unwrap_or(source.len());
            assert!(
                !source[start..end].contains("LinearGradient"),
                "{draw} must use the shared rasterizer; a Cairo gradient would not \
                 honor stop positions and could drift from export"
            );
        }
    }

    #[test]
    fn the_gradient_bar_edits_stops_flat() {
        // The bar is where handles are dragged onto stops, so it always renders
        // left-to-right: the angle is a canvas/export property, and applying it
        // here would slide handles off the stops they belong to. Reversal does
        // apply, because it is a property of the stop order.
        let source = include_str!("custom_wallpaper_popover.rs");
        let start = source
            .find("fn draw_stop_bar(")
            .expect("the gradient bar has a draw function");
        let end = source[start..]
            .find("\nfn ")
            .map(|at| start + at)
            .unwrap_or(source.len());
        let body = &source[start..end];
        assert!(
            body.contains("angle_degrees = 0.0"),
            "the bar must flatten the angle so pins stay on their stops"
        );
        assert!(
            body.contains("flat.kind = GradientKind::Linear"),
            "the bar must flatten the kind too, or a radial's pins lose their stops"
        );
        assert!(
            body.contains("bar_handles(gradient)"),
            "the bar must place pins through the reversal-aware helper"
        );
    }

    #[test]
    fn only_the_stops_button_adds_a_stop() {
        // The bar is a drag surface for the pins already there. It used to
        // create a stop where the press landed, so any stray click on the
        // ramp added a stop the user never asked for. Adding belongs to the
        // Stops + button and nowhere else.
        let source = include_str!("custom_wallpaper_popover.rs");
        let start = source
            .find("fn attach_stop_drag(")
            .expect("the bar has a drag handler");
        let end = source[start..]
            .find("\n#[cfg(test)]")
            .map(|at| start + at)
            .unwrap_or(source.len());
        let body = &source[start..end];
        assert!(
            !body.contains("GradientStop::new("),
            "the bar's drag handler must not create stops; the + button does"
        );
        assert!(
            !body.contains("sample_color_at("),
            "the drag handler must not sample a new stop's color"
        );
        // A press that grabs no pin must leave nothing dragged, or the update
        // handler would move whatever stop happened to be at the stale index.
        assert!(
            body.contains("dragging.set(usize::MAX);"),
            "a press on empty track must clear the drag target"
        );
    }

    #[test]
    fn every_source_tab_refreshes_the_pages() {
        // The Wallpaper tab only recorded the open page and never asked for a
        // refresh, so going Custom and back left the custom page on screen.
        // Every tab must route through the same change + refresh path.
        let panel = include_str!("tool_sidebar_background.rs");
        assert!(
            panel.contains("Rc::new(move |page: BgPage| {"),
            "a tab switch must record the page and refresh"
        );
        for handler in [
            "set_page(BgPage::Wallpaper)",
            "set_page(BgPage::Custom)",
            "set_page(BgPage::Image)",
        ] {
            assert!(
                panel.contains(handler),
                "every source tab must set its own page ({handler})"
            );
        }
        // Browsing tabs must not quietly rewrite what gets exported.
        assert!(
            !panel.contains("guard.background = VideoBackground::Plain {\n                r: 17,"),
            "switching tabs must not write a fill"
        );
    }
}
