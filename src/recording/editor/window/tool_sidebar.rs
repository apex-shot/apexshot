use crate::recording::editor::cursor_sprite;
use crate::recording::editor::model::{
    nearest_zoom_preset, ClickEffect, CursorTheme, EditorTool, VideoEditState, ZoomMode,
    CLIP_SPEED_PRESETS, MAX_CURSOR_SIZE, MIN_CURSOR_SIZE, ZOOM_SCALE_PRESETS,
};
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, DrawingArea, Grid, Image, Label, Orientation,
    PolicyType, Scale, ScrolledWindow, Switch, ToggleButton,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub(super) const TOOL_SIDEBAR_WIDTH: i32 = 288;

pub(super) struct ToolSidebar {
    pub widget: GtkBox,
    pub refresh: Rc<dyn Fn()>,
}

pub(super) fn build_tool_sidebar(
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) -> ToolSidebar {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("recording-editor-tool-sidebar");
    root.set_hexpand(false);
    root.set_vexpand(true);
    root.set_size_request(TOOL_SIDEBAR_WIDTH, -1);

    let cursor_panel = build_cursor_panel(state.clone(), on_change.clone());
    let zoom_panel = build_zoom_panel(state.clone(), on_change.clone());
    let clip_panel = build_clip_panel(state.clone(), on_change);
    root.append(&cursor_panel.widget);
    root.append(&zoom_panel.widget);
    root.append(&clip_panel.widget);
    root.set_visible(true);
    let last_zoom = Rc::new(Cell::new(false));

    let refresh = {
        let state = state.clone();
        let refresh_cursor = cursor_panel.refresh;
        let refresh_zoom = zoom_panel.refresh;
        let refresh_clip = clip_panel.refresh;
        let last_zoom = last_zoom.clone();
        Rc::new(move || {
            let guard = state.lock().unwrap();
            let tool = guard.selected_tool;
            let zoom = guard.selected_zoom.is_some();
            let clip = guard.selected_segment.is_some();
            drop(guard);
            if zoom {
                last_zoom.set(true);
            } else if clip {
                last_zoom.set(false);
            }
            let show_cursor = tool == EditorTool::Cursor;
            let show_zoom = !show_cursor && (zoom || (!clip && last_zoom.get()));
            cursor_panel.widget.set_visible(show_cursor);
            zoom_panel.widget.set_visible(show_zoom);
            clip_panel.widget.set_visible(!show_cursor && !show_zoom);
            if show_cursor {
                refresh_cursor();
            } else if show_zoom {
                refresh_zoom();
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

struct CursorPanel {
    widget: GtkBox,
    refresh: Rc<dyn Fn()>,
}

fn build_cursor_panel(state: Arc<Mutex<VideoEditState>>, on_change: Rc<dyn Fn()>) -> CursorPanel {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("recording-editor-zoom-panel");
    panel.set_hexpand(true);
    panel.set_vexpand(true);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.add_css_class("recording-editor-zoom-header");
    header.set_hexpand(true);
    let title = Label::new(Some("Cursor"));
    title.add_css_class("recording-editor-zoom-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);

    let body = GtkBox::new(Orientation::Vertical, 8);
    body.add_css_class("recording-editor-zoom-body");
    body.set_hexpand(true);
    let hint = Label::new(Some("Shown over the recording in preview and export"));
    hint.add_css_class("recording-editor-zoom-hint");
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    body.append(&hint);

    let grid = Grid::new();
    grid.add_css_class("recording-editor-cursor-grid");
    grid.set_column_spacing(8);
    grid.set_row_spacing(8);
    grid.set_column_homogeneous(true);
    grid.set_hexpand(true);

    let cards: Vec<(CursorTheme, ToggleButton, DrawingArea)> = CursorTheme::ALL
        .iter()
        .enumerate()
        .map(|(index, &theme)| {
            let card = ToggleButton::new();
            card.add_css_class("recording-editor-cursor-card");
            card.set_has_frame(false);
            card.set_hexpand(true);
            let column = GtkBox::new(Orientation::Vertical, 6);
            column.set_halign(Align::Fill);
            let preview = DrawingArea::new();
            preview.add_css_class("recording-editor-cursor-preview");
            preview.set_content_width(72);
            preview.set_content_height(56);
            preview.set_hexpand(true);
            preview.set_draw_func(move |_, cr, width, height| {
                cursor_sprite::draw_centered(cr, width as f64, height as f64, theme);
            });
            let name = Label::new(Some(theme.label()));
            name.add_css_class("recording-editor-cursor-card-label");
            name.set_xalign(0.5);
            column.append(&preview);
            column.append(&name);
            card.set_child(Some(&column));
            card.connect_clicked({
                let state = state.clone();
                let on_change = on_change.clone();
                move |button| {
                    if !button.is_active() {
                        return;
                    }
                    state.lock().unwrap().cursor.theme = theme;
                    on_change();
                }
            });
            grid.attach(&card, (index % 2) as i32, (index / 2) as i32, 1, 1);
            (theme, card, preview)
        })
        .collect();

    let first = cards[0].1.clone();
    for (index, (_, card, _)) in cards.iter().enumerate() {
        if index > 0 {
            card.set_group(Some(&first));
        }
    }

    body.append(&grid);

    let size_row = cursor_slider_row("Size");
    let shadow_row = cursor_slider_row("Shadow");
    let smooth_row = cursor_slider_row("Smoothing");
    size_row.scale.set_range(MIN_CURSOR_SIZE, MAX_CURSOR_SIZE);
    size_row.scale.set_increments(0.05, 0.25);
    shadow_row.scale.set_range(0.0, 1.0);
    shadow_row.scale.set_increments(0.05, 0.1);
    smooth_row.scale.set_range(0.0, 1.0);
    smooth_row.scale.set_increments(0.05, 0.1);
    body.append(&size_row.widget);
    body.append(&shadow_row.widget);
    body.append(&smooth_row.widget);

    let idle_row = GtkBox::new(Orientation::Horizontal, 8);
    idle_row.add_css_class("recording-editor-zoom-classic");
    idle_row.set_hexpand(true);
    let idle_label = Label::new(Some("Hide when idle"));
    idle_label.add_css_class("recording-editor-zoom-classic-label");
    idle_label.set_xalign(0.0);
    idle_label.set_hexpand(true);
    let idle_switch = Switch::new();
    idle_switch.add_css_class("recording-editor-zoom-switch");
    idle_switch.set_valign(Align::Center);
    idle_switch.set_halign(Align::End);
    idle_row.append(&idle_label);
    idle_row.append(&idle_switch);
    body.append(&idle_row);

    let click_label = Label::new(Some("Click effect"));
    click_label.add_css_class("recording-editor-zoom-kicker");
    click_label.set_xalign(0.0);
    body.append(&click_label);
    let click_row = GtkBox::new(Orientation::Horizontal, 12);
    click_row.add_css_class("recording-editor-zoom-mode");
    click_row.set_hexpand(true);
    let none_btn = ToggleButton::with_label("None");
    let pulse_btn = ToggleButton::with_label("Pulse");
    let ripple_btn = ToggleButton::with_label("Ripple");
    for btn in [&none_btn, &pulse_btn, &ripple_btn] {
        btn.add_css_class("recording-editor-zoom-mode-btn");
        btn.set_has_frame(false);
        click_row.append(btn);
    }
    pulse_btn.set_group(Some(&none_btn));
    ripple_btn.set_group(Some(&none_btn));
    body.append(&click_row);
    let intensity_row = cursor_slider_row("Intensity");
    intensity_row.scale.set_range(0.0, 1.0);
    intensity_row.scale.set_increments(0.05, 0.1);
    body.append(&intensity_row.widget);

    let scroll = ScrolledWindow::new();
    scroll.add_css_class("recording-editor-zoom-scroll");
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_child(Some(&body));
    panel.append(&header);
    panel.append(&scroll);

    let syncing = Rc::new(Cell::new(false));
    size_row.scale.connect_value_changed({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |scale| {
            if syncing.get() {
                return;
            }
            state.lock().unwrap().cursor.size = scale.value();
            on_change();
        }
    });
    shadow_row.scale.connect_value_changed({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |scale| {
            if syncing.get() {
                return;
            }
            state.lock().unwrap().cursor.shadow = scale.value();
            on_change();
        }
    });
    smooth_row.scale.connect_value_changed({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |scale| {
            if syncing.get() {
                return;
            }
            state.lock().unwrap().cursor.smooth = scale.value();
            on_change();
        }
    });
    idle_switch.connect_state_set({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |_, active| {
            if !syncing.get() {
                state.lock().unwrap().cursor.hide_idle = active;
                on_change();
            }
            gtk4::glib::Propagation::Proceed
        }
    });
    none_btn.connect_toggled({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() || !button.is_active() {
                return;
            }
            state.lock().unwrap().cursor.click_effect = ClickEffect::None;
            on_change();
        }
    });
    pulse_btn.connect_toggled({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() || !button.is_active() {
                return;
            }
            state.lock().unwrap().cursor.click_effect = ClickEffect::Pulse;
            on_change();
        }
    });
    ripple_btn.connect_toggled({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() || !button.is_active() {
                return;
            }
            state.lock().unwrap().cursor.click_effect = ClickEffect::Ripple;
            on_change();
        }
    });
    intensity_row.scale.connect_value_changed({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |scale| {
            if syncing.get() {
                return;
            }
            state.lock().unwrap().cursor.click_intensity = scale.value();
            on_change();
        }
    });

    let refresh = {
        let cards = cards;
        let size_scale = size_row.scale.clone();
        let shadow_scale = shadow_row.scale.clone();
        let smooth_scale = smooth_row.scale.clone();
        let idle_switch = idle_switch.clone();
        let none_btn = none_btn.clone();
        let pulse_btn = pulse_btn.clone();
        let ripple_btn = ripple_btn.clone();
        let intensity_scale = intensity_row.scale.clone();
        let syncing = syncing.clone();
        Rc::new(move || {
            let cursor = state.lock().unwrap().cursor;
            for (item, card, preview) in &cards {
                card.set_active(*item == cursor.theme);
                preview.queue_draw();
            }
            syncing.set(true);
            size_scale.set_value(cursor.size);
            shadow_scale.set_value(cursor.shadow);
            smooth_scale.set_value(cursor.smooth);
            idle_switch.set_active(cursor.hide_idle);
            match cursor.click_effect {
                ClickEffect::None => none_btn.set_active(true),
                ClickEffect::Pulse => pulse_btn.set_active(true),
                ClickEffect::Ripple => ripple_btn.set_active(true),
            }
            intensity_scale.set_value(cursor.click_intensity);
            intensity_scale.set_sensitive(cursor.click_effect != ClickEffect::None);
            syncing.set(false);
        }) as Rc<dyn Fn()>
    };

    CursorPanel {
        widget: panel,
        refresh,
    }
}

struct CursorSliderRow {
    widget: GtkBox,
    scale: Scale,
}

fn cursor_slider_row(label: &str) -> CursorSliderRow {
    let widget = GtkBox::new(Orientation::Vertical, 4);
    widget.add_css_class("recording-editor-cursor-slider-row");
    widget.set_hexpand(true);
    let caption = Label::new(Some(label));
    caption.add_css_class("recording-editor-zoom-kicker");
    caption.set_xalign(0.0);
    let scale = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.05);
    scale.add_css_class("recording-editor-cursor-slider");
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    widget.append(&caption);
    widget.append(&scale);
    CursorSliderRow { widget, scale }
}

struct ZoomPanel {
    widget: GtkBox,
    refresh: Rc<dyn Fn()>,
}

fn build_zoom_panel(state: Arc<Mutex<VideoEditState>>, on_change: Rc<dyn Fn()>) -> ZoomPanel {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("recording-editor-zoom-panel");
    panel.set_hexpand(true);
    panel.set_vexpand(true);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.add_css_class("recording-editor-zoom-header");
    header.set_hexpand(true);
    let title = Label::new(Some("Zoom"));
    title.add_css_class("recording-editor-zoom-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("recording-editor-zoom-body");
    body.set_hexpand(true);

    let mode_row = GtkBox::new(Orientation::Horizontal, 12);
    mode_row.add_css_class("recording-editor-zoom-mode");
    mode_row.set_hexpand(true);
    let auto_available = state.lock().unwrap().supports_auto_zoom();
    let auto_btn = ToggleButton::with_label("Auto");
    auto_btn.add_css_class("recording-editor-zoom-mode-btn");
    auto_btn.set_has_frame(false);
    auto_btn.set_hexpand(false);
    auto_btn.set_sensitive(auto_available);
    auto_btn.set_active(auto_available);
    let manual_btn = ToggleButton::with_label("Manual");
    manual_btn.add_css_class("recording-editor-zoom-mode-btn");
    manual_btn.set_has_frame(false);
    manual_btn.set_hexpand(false);
    manual_btn.set_group(Some(&auto_btn));
    if !auto_available {
        manual_btn.set_active(true);
    }
    mode_row.append(&auto_btn);
    mode_row.append(&manual_btn);

    let mode_hint = Label::new(Some(if auto_available {
        "Camera recenters when the cursor nears the edge of the zoomed view"
    } else {
        "Set a fixed focus point for this zoom"
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
    let animation_label = Label::new(Some("Animation"));
    animation_label.add_css_class("recording-editor-zoom-kicker");
    animation_label.set_xalign(0.0);
    animation_label.set_hexpand(true);
    let reset = Button::with_label("Reset");
    reset.add_css_class("recording-editor-zoom-reset");
    reset.set_has_frame(false);
    reset.set_halign(Align::End);
    animation_header.append(&animation_label);
    animation_header.append(&reset);

    let classic_row = GtkBox::new(Orientation::Horizontal, 8);
    classic_row.add_css_class("recording-editor-zoom-classic");
    classic_row.set_hexpand(true);
    let classic_label = Label::new(Some("Classic Animation"));
    classic_label.add_css_class("recording-editor-zoom-classic-label");
    classic_label.set_xalign(0.0);
    classic_label.set_hexpand(true);
    let classic = Switch::new();
    classic.add_css_class("recording-editor-zoom-switch");
    classic.set_valign(Align::Center);
    classic.set_halign(Align::End);
    classic_row.append(&classic_label);
    classic_row.append(&classic);

    let footer_delete = delete_tool_button("Delete zoom");

    body.append(&mode_row);
    body.append(&mode_hint);
    body.append(&chips);
    body.append(&animation_header);
    body.append(&classic_row);

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
                state.lock().unwrap().zoom_classic = active;
                on_change();
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
        let syncing = syncing.clone();
        Rc::new(move || {
            let guard = state.lock().unwrap();
            panel.set_visible(true);
            let auto_available = guard.supports_auto_zoom();
            let selected = guard.selected_zoom_clip().cloned();
            syncing.set(true);
            auto_btn.set_sensitive(auto_available);
            if let Some(clip) = &selected {
                let mode = if clip.mode == ZoomMode::Auto && auto_available {
                    ZoomMode::Auto
                } else {
                    ZoomMode::Manual
                };
                mode_hint.set_text(match mode {
                    ZoomMode::Manual => "Set a fixed focus point for this zoom",
                    ZoomMode::Auto => {
                        "Camera recenters when the cursor nears the edge of the zoomed view"
                    }
                });
                match mode {
                    ZoomMode::Auto => auto_btn.set_active(true),
                    ZoomMode::Manual => manual_btn.set_active(true),
                }
                classic_row.set_visible(mode == ZoomMode::Auto);
            } else {
                if !auto_available {
                    manual_btn.set_active(true);
                    mode_hint.set_text("Set a fixed focus point for this zoom");
                }
                classic_row.set_visible(false);
            }
            classic.set_active(guard.zoom_classic);
            let selected_preset = selected
                .as_ref()
                .map(|clip| nearest_zoom_preset(clip.scale));
            for (chip, &(_, scale)) in chip_buttons.iter().zip(ZOOM_SCALE_PRESETS.iter()) {
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

fn build_clip_panel(state: Arc<Mutex<VideoEditState>>, on_change: Rc<dyn Fn()>) -> ClipPanel {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("recording-editor-zoom-panel");
    panel.set_hexpand(true);
    panel.set_vexpand(true);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.add_css_class("recording-editor-zoom-header");
    header.set_hexpand(true);
    let title = Label::new(Some("Clip"));
    title.add_css_class("recording-editor-zoom-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("recording-editor-zoom-body");
    body.set_hexpand(true);

    let speed_label = Label::new(Some("Speed"));
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
    let audio_label = Label::new(Some("Audio"));
    audio_label.add_css_class("recording-editor-zoom-kicker");
    audio_label.set_xalign(0.0);
    audio_header.append(&audio_label);

    let mute_row = GtkBox::new(Orientation::Horizontal, 8);
    mute_row.add_css_class("recording-editor-zoom-classic");
    mute_row.set_hexpand(true);
    let mute_label = Label::new(Some("Mute"));
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

    let footer_delete = delete_tool_button("Delete clip");
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
        move |_, active| {
            if !syncing.get() {
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
        let syncing = syncing.clone();
        Rc::new(move || {
            let guard = state.lock().unwrap();
            panel.set_visible(true);
            let speed = guard.selected_clip_speed();
            let muted = guard.selected_clip_muted().unwrap_or(false);
            let can_mute = speed.is_some() && guard.has_audio_track();
            syncing.set(true);
            mute.set_active(muted);
            for (chip, &(_, preset)) in chip_buttons.iter().zip(CLIP_SPEED_PRESETS.iter()) {
                let active = speed.is_some_and(|value| (value - preset).abs() < 1e-6);
                if active {
                    chip.add_css_class("recording-editor-zoom-chip-active");
                } else {
                    chip.remove_css_class("recording-editor-zoom-chip-active");
                }
            }
            mute.set_sensitive(can_mute);
            syncing.set(false);
        }) as Rc<dyn Fn()>
    };

    ClipPanel {
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
