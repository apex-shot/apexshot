use crate::recording::editor::model::{
    nearest_zoom_preset, VideoEditState, ZoomMode, CLIP_SPEED_PRESETS, MAX_ZOOM_BLUR_SAMPLES,
    MIN_ZOOM_BLUR_SAMPLES, ZOOM_SCALE_PRESETS,
};
use gtk4::{
    gdk, prelude::*, Align, Box as GtkBox, Button, DrawingArea, GestureClick, GestureDrag, Grid,
    Image, Label, Orientation, Overlay, PolicyType, ScrolledWindow, Switch, ToggleButton,
};
use std::cell::{Cell, RefCell};
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

    let zoom_panel = build_zoom_panel(state.clone(), on_change.clone());
    let clip_panel = build_clip_panel(state.clone(), on_change);
    root.append(&zoom_panel.widget);
    root.append(&clip_panel.widget);
    root.set_visible(true);
    let last_zoom = Rc::new(Cell::new(false));

    let refresh = {
        let state = state.clone();
        let refresh_zoom = zoom_panel.refresh;
        let refresh_clip = clip_panel.refresh;
        let last_zoom = last_zoom.clone();
        Rc::new(move || {
            let guard = state.lock().unwrap();
            let zoom = guard.selected_zoom.is_some();
            let clip = guard.selected_segment.is_some();
            drop(guard);
            if zoom {
                last_zoom.set(true);
            } else if clip {
                last_zoom.set(false);
            }
            let show_zoom = zoom || (!clip && last_zoom.get());
            zoom_panel.widget.set_visible(show_zoom);
            clip_panel.widget.set_visible(!show_zoom);
            if show_zoom {
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

    let blur = GtkBox::new(Orientation::Vertical, 10);
    blur.add_css_class("recording-editor-zoom-blur");
    blur.set_hexpand(true);
    let blur_kicker = Label::new(Some("Blur"));
    blur_kicker.add_css_class("recording-editor-zoom-kicker");
    blur_kicker.set_xalign(0.0);
    let samples = labeled_slider(
        "Samples",
        MIN_ZOOM_BLUR_SAMPLES as f64,
        MAX_ZOOM_BLUR_SAMPLES as f64,
        2.0,
        state.lock().unwrap().zoom_blur_samples as f64,
    );
    let shutter = labeled_slider(
        "Shutter",
        0.0,
        100.0,
        1.0,
        state.lock().unwrap().zoom_blur_shutter * 100.0,
    );
    samples
        .value_label
        .set_text(&state.lock().unwrap().zoom_blur_samples.to_string());
    shutter.value_label.set_text(&format!(
        "{:.0}%",
        state.lock().unwrap().zoom_blur_shutter * 100.0
    ));
    blur.append(&blur_kicker);
    blur.append(&samples.widget);
    blur.append(&shutter.widget);
    blur.set_visible(false);

    let footer_delete = delete_tool_button("Delete zoom");

    body.append(&mode_row);
    body.append(&mode_hint);
    body.append(&chips);
    body.append(&animation_header);
    body.append(&classic_row);
    body.append(&blur);

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
    samples.connect_changed({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        let samples_value = samples.value_label.clone();
        move |value| {
            if syncing.get() {
                return;
            }
            let samples = value.round() as u32;
            samples_value.set_text(&samples.to_string());
            state.lock().unwrap().set_zoom_blur_samples(samples);
            on_change();
        }
    });
    shutter.connect_changed({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        let shutter_value = shutter.value_label.clone();
        move |value| {
            if syncing.get() {
                return;
            }
            let percent = value.round();
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
        let classic_row = classic_row.clone();
        let blur = blur.clone();
        let chip_buttons = chip_buttons.clone();
        let syncing = syncing.clone();
        let samples = samples.clone();
        let shutter = shutter.clone();
        let samples_value = samples.value_label.clone();
        let shutter_value = shutter.value_label.clone();
        Rc::new(move || {
            let guard = state.lock().unwrap();
            panel.set_visible(true);
            let selected = guard.selected_zoom_clip().cloned();
            syncing.set(true);
            if let Some(clip) = &selected {
                mode_hint.set_text(match clip.mode {
                    ZoomMode::Manual => "Set a fixed focus point for this zoom",
                    ZoomMode::Auto => {
                        "Camera recenters when the cursor nears the edge of the zoomed view"
                    }
                });
                match clip.mode {
                    ZoomMode::Auto => auto_btn.set_active(true),
                    ZoomMode::Manual => manual_btn.set_active(true),
                }
                classic_row.set_visible(clip.mode == ZoomMode::Auto);
                blur.set_visible(true);
            } else {
                classic_row.set_visible(false);
                blur.set_visible(false);
            }
            classic.set_active(guard.zoom_classic);
            samples.set_value(guard.zoom_blur_samples as f64);
            shutter.set_value(guard.zoom_blur_shutter * 100.0);
            samples_value.set_text(&guard.zoom_blur_samples.to_string());
            shutter_value.set_text(&format!("{:.0}%", guard.zoom_blur_shutter * 100.0));
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

#[derive(Clone)]
struct BarSlider {
    widget: GtkBox,
    value_label: Label,
    value: Rc<Cell<f64>>,
    track: DrawingArea,
    min: f64,
    max: f64,
    listeners: Rc<RefCell<Vec<Rc<dyn Fn(f64)>>>>,
}

impl BarSlider {
    fn set_value(&self, value: f64) {
        self.value.set(value.clamp(self.min, self.max));
        self.track.queue_draw();
    }

    fn connect_changed(&self, f: impl Fn(f64) + 'static) {
        self.listeners.borrow_mut().push(Rc::new(f));
    }
}

fn slider_value_at(x: f64, width: f64, min: f64, max: f64, step: f64) -> f64 {
    let t = (x / width.max(1.0)).clamp(0.0, 1.0);
    let raw = min + t * (max - min);
    let snapped = if step > 0.0 {
        (raw / step).round() * step
    } else {
        raw
    };
    snapped.clamp(min, max)
}

fn labeled_slider(name: &str, min: f64, max: f64, step: f64, value: f64) -> BarSlider {
    let value = Rc::new(Cell::new(value.clamp(min, max)));
    let listeners: Rc<RefCell<Vec<Rc<dyn Fn(f64)>>>> = Rc::new(RefCell::new(Vec::new()));
    let row = GtkBox::new(Orientation::Horizontal, 0);
    row.add_css_class("recording-editor-zoom-slider-row");
    row.set_hexpand(true);
    let overlay = Overlay::new();
    overlay.set_hexpand(true);
    overlay.set_valign(Align::Fill);
    overlay.set_size_request(-1, 36);
    let label = Label::new(Some(name));
    label.add_css_class("recording-editor-zoom-classic-label");
    label.set_xalign(0.0);
    label.set_valign(Align::Center);
    label.set_margin_start(12);
    label.set_can_target(false);
    let value_label = Label::new(Some(""));
    value_label.add_css_class("recording-editor-zoom-blur-value");
    value_label.set_halign(Align::End);
    value_label.set_valign(Align::Center);
    value_label.set_margin_end(12);
    value_label.set_can_target(false);
    let chrome = GtkBox::new(Orientation::Horizontal, 0);
    chrome.set_can_target(false);
    chrome.set_hexpand(true);
    chrome.set_valign(Align::Fill);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    spacer.set_can_target(false);
    chrome.append(&label);
    chrome.append(&spacer);
    chrome.append(&value_label);
    overlay.set_child(Some(&chrome));
    let track = DrawingArea::new();
    track.add_css_class("recording-editor-zoom-slider-track");
    track.set_hexpand(true);
    track.set_vexpand(true);
    track.set_halign(Align::Fill);
    track.set_valign(Align::Fill);
    track.set_can_target(true);
    track.set_content_height(36);
    track.set_cursor(gdk::Cursor::from_name("ew-resize", None).as_ref());
    track.set_draw_func({
        let value = value.clone();
        move |_, cr, width, height| {
            let t = (value.get() - min) / (max - min).max(1e-9);
            let w = 5.0;
            let pad = 6.0;
            let h = (height as f64 - pad * 2.0).max(2.0);
            let x = t * (width as f64 - w).max(0.0);
            let y = pad;
            let r = w / 2.0;
            cr.set_source_rgb(0.82, 0.82, 0.82);
            cr.new_sub_path();
            cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
            cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
            cr.arc(
                x + r,
                y + h - r,
                r,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::PI,
            );
            cr.arc(
                x + r,
                y + r,
                r,
                std::f64::consts::PI,
                3.0 * std::f64::consts::FRAC_PI_2,
            );
            cr.close_path();
            let _ = cr.fill();
        }
    });
    let apply = {
        let value = value.clone();
        let track = track.clone();
        let listeners = listeners.clone();
        Rc::new(move |x: f64| {
            let next = slider_value_at(x, track.allocated_width().max(1) as f64, min, max, step);
            if (next - value.get()).abs() < 1e-9 {
                return;
            }
            value.set(next);
            track.queue_draw();
            for cb in listeners.borrow().iter() {
                cb(next);
            }
        })
    };
    let click = GestureClick::new();
    click.set_button(1);
    click.connect_pressed({
        let apply = apply.clone();
        move |_, _, x, _| apply(x)
    });
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_update({
        let apply = apply.clone();
        move |gesture, dx, _| {
            let Some((sx, _)) = gesture.start_point() else {
                return;
            };
            apply(sx + dx);
        }
    });
    click.group_with(&drag);
    track.add_controller(click);
    track.add_controller(drag);
    overlay.add_overlay(&track);
    row.append(&overlay);
    BarSlider {
        widget: row,
        value_label,
        value,
        track,
        min,
        max,
        listeners,
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
    use super::slider_value_at;

    #[test]
    fn slider_maps_edges_and_snaps() {
        assert_eq!(slider_value_at(0.0, 100.0, 0.0, 100.0, 1.0), 0.0);
        assert_eq!(slider_value_at(100.0, 100.0, 0.0, 100.0, 1.0), 100.0);
        assert_eq!(slider_value_at(50.0, 100.0, 1.0, 21.0, 2.0), 12.0);
    }
}
