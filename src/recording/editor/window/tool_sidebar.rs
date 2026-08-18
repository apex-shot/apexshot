use crate::recording::editor::model::{
    nearest_zoom_preset, VideoEditState, ZoomMode, MAX_ZOOM_BLUR_SAMPLES, MIN_ZOOM_BLUR_SAMPLES,
    ZOOM_SCALE_PRESETS,
};
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, Grid, Image, Label, Orientation, PolicyType, Scale,
    ScrolledWindow, Switch, ToggleButton,
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

    let zoom_panel = build_zoom_panel(state.clone(), on_change);
    root.append(&zoom_panel.widget);
    root.set_visible(false);

    let refresh = {
        let root = root.clone();
        let state = state.clone();
        let refresh_zoom = zoom_panel.refresh;
        Rc::new(move || {
            let selected = state.lock().unwrap().selected_zoom.is_some();
            root.set_visible(selected);
            if selected {
                refresh_zoom();
            }
        }) as Rc<dyn Fn()>
    };

    ToolSidebar {
        widget: root,
        refresh,
    }
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
    let auto_btn = ToggleButton::with_label("Auto");
    auto_btn.add_css_class("recording-editor-zoom-mode-btn");
    auto_btn.set_has_frame(false);
    auto_btn.set_hexpand(false);
    auto_btn.set_active(true);
    let manual_btn = ToggleButton::with_label("Manual");
    manual_btn.add_css_class("recording-editor-zoom-mode-btn");
    manual_btn.set_has_frame(false);
    manual_btn.set_hexpand(false);
    manual_btn.set_group(Some(&auto_btn));
    mode_row.append(&auto_btn);
    mode_row.append(&manual_btn);

    let mode_hint = Label::new(Some(
        "Camera recenters when the cursor nears the edge of the zoomed view",
    ));
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

    let presets_hint = Label::new(Some("Zoom motion presets are available in Settings."));
    presets_hint.add_css_class("recording-editor-zoom-hint");
    presets_hint.set_wrap(true);
    presets_hint.set_xalign(0.0);
    presets_hint.set_max_width_chars(34);

    let blur = GtkBox::new(Orientation::Vertical, 10);
    blur.add_css_class("recording-editor-zoom-blur");
    blur.set_hexpand(true);
    let blur_kicker = Label::new(Some("Blur"));
    blur_kicker.add_css_class("recording-editor-zoom-kicker");
    blur_kicker.set_xalign(0.0);
    let (samples_block, samples_scale, samples_value) = labeled_slider(
        "Samples",
        MIN_ZOOM_BLUR_SAMPLES as f64,
        MAX_ZOOM_BLUR_SAMPLES as f64,
        2.0,
        state.lock().unwrap().zoom_blur_samples as f64,
    );
    let (shutter_block, shutter_scale, shutter_value) = labeled_slider(
        "Shutter",
        0.0,
        100.0,
        1.0,
        state.lock().unwrap().zoom_blur_shutter * 100.0,
    );
    samples_value.set_text(&state.lock().unwrap().zoom_blur_samples.to_string());
    shutter_value.set_text(&format!(
        "{:.0}%",
        state.lock().unwrap().zoom_blur_shutter * 100.0
    ));
    blur.append(&blur_kicker);
    blur.append(&samples_block);
    blur.append(&shutter_block);

    let footer_delete = delete_zoom_button();

    body.append(&mode_row);
    body.append(&mode_hint);
    body.append(&chips);
    body.append(&animation_header);
    body.append(&classic_row);
    body.append(&presets_hint);
    body.append(&blur);

    let scroll = ScrolledWindow::new();
    scroll.add_css_class("recording-editor-zoom-scroll");
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_child(Some(&body));

    let footer = GtkBox::new(Orientation::Vertical, 0);
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
    samples_scale.connect_value_changed({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        let samples_value = samples_value.clone();
        move |scale| {
            if syncing.get() {
                return;
            }
            let samples = scale.value().round() as u32;
            samples_value.set_text(&samples.to_string());
            state.lock().unwrap().set_zoom_blur_samples(samples);
            on_change();
        }
    });
    shutter_scale.connect_value_changed({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        let shutter_value = shutter_value.clone();
        move |scale| {
            if syncing.get() {
                return;
            }
            let percent = scale.value().round();
            shutter_value.set_text(&format!("{percent:.0}%"));
            state.lock().unwrap().set_zoom_blur_shutter(percent / 100.0);
            on_change();
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
        let presets_hint = presets_hint.clone();
        let chip_buttons = chip_buttons.clone();
        let syncing = syncing.clone();
        let footer_delete = footer_delete.clone();
        let reset = reset.clone();
        let samples_scale = samples_scale.clone();
        let shutter_scale = shutter_scale.clone();
        let samples_value = samples_value.clone();
        let shutter_value = shutter_value.clone();
        Rc::new(move || {
            let guard = state.lock().unwrap();
            let Some(clip) = guard.selected_zoom_clip() else {
                panel.set_visible(false);
                return;
            };
            panel.set_visible(true);
            mode_hint.set_text(match clip.mode {
                ZoomMode::Manual => "Set a fixed focus point for this zoom",
                ZoomMode::Auto => {
                    "Camera recenters when the cursor nears the edge of the zoomed view"
                }
            });
            presets_hint.set_visible(!guard.zoom_classic);
            let locked = guard.zoom_locked;
            let selected_preset = nearest_zoom_preset(clip.scale);
            syncing.set(true);
            match clip.mode {
                ZoomMode::Auto => auto_btn.set_active(true),
                ZoomMode::Manual => manual_btn.set_active(true),
            }
            classic.set_active(guard.zoom_classic);
            samples_scale.set_value(guard.zoom_blur_samples as f64);
            shutter_scale.set_value(guard.zoom_blur_shutter * 100.0);
            samples_value.set_text(&guard.zoom_blur_samples.to_string());
            shutter_value.set_text(&format!("{:.0}%", guard.zoom_blur_shutter * 100.0));
            for (chip, &(_, scale)) in chip_buttons.iter().zip(ZOOM_SCALE_PRESETS.iter()) {
                let active = (scale - selected_preset).abs() < 1e-6;
                if active {
                    chip.add_css_class("recording-editor-zoom-chip-active");
                } else {
                    chip.remove_css_class("recording-editor-zoom-chip-active");
                }
                chip.set_sensitive(!locked);
            }
            auto_btn.set_sensitive(!locked);
            manual_btn.set_sensitive(!locked);
            classic.set_sensitive(!locked);
            reset.set_sensitive(!locked);
            samples_scale.set_sensitive(!locked);
            shutter_scale.set_sensitive(!locked);
            footer_delete.set_sensitive(!locked);
            syncing.set(false);
        }) as Rc<dyn Fn()>
    };

    ZoomPanel {
        widget: panel,
        refresh,
    }
}

fn labeled_slider(
    name: &str,
    min: f64,
    max: f64,
    step: f64,
    value: f64,
) -> (GtkBox, Scale, Label) {
    let block = GtkBox::new(Orientation::Vertical, 2);
    block.set_hexpand(true);
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.set_hexpand(true);
    let label = Label::new(Some(name));
    label.add_css_class("recording-editor-zoom-classic-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    let value_label = Label::new(Some(""));
    value_label.add_css_class("recording-editor-zoom-blur-value");
    value_label.set_halign(Align::End);
    row.append(&label);
    row.append(&value_label);
    let scale = Scale::with_range(Orientation::Horizontal, min, max, step);
    scale.add_css_class("recording-editor-zoom-slider");
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale.set_value(value);
    block.append(&row);
    block.append(&scale);
    (block, scale, value_label)
}

fn delete_zoom_button() -> Button {
    let button = Button::new();
    button.add_css_class("recording-editor-zoom-delete");
    button.set_has_frame(false);
    button.set_halign(Align::Start);
    let row = GtkBox::new(Orientation::Horizontal, 6);
    row.set_halign(Align::Start);
    let icon = Image::from_icon_name("user-trash-symbolic");
    icon.set_pixel_size(13);
    let label = Label::new(Some("Delete zoom"));
    row.append(&icon);
    row.append(&label);
    button.set_child(Some(&row));
    button
}
