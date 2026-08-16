use super::footer;
use super::panels::{self, EditorControls};
use super::toolbar;
use crate::recording::editor::model::{
    VideoBackground, VideoEditState, MAX_ZOOM_SCALE, MIN_ZOOM_SCALE,
};
use gtk4::{
    prelude::*, ApplicationWindow, Box as GtkBox, Button, Label, Orientation, Scale, ScrolledWindow,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub(super) const INSPECTOR_WIDTH: i32 = 210;

pub(super) struct InspectorParts {
    pub root: GtkBox,
    pub _controls: EditorControls,
}

pub(super) fn build_inspector(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
    exporting: Rc<Cell<bool>>,
) -> InspectorParts {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-right-inspector");
    root.add_css_class("recording-editor-inspector");
    root.set_width_request(INSPECTOR_WIDTH);
    root.set_hexpand(false);
    root.set_vexpand(true);

    let lights = toolbar::build_traffic_lights(window);
    lights.set_halign(gtk4::Align::End);
    lights.set_margin_top(8);
    lights.set_margin_end(8);
    lights.set_margin_start(8);
    root.append(&lights);
    crate::capture::editor::ui_support::install_window_drag(&lights, window);

    let scroll = ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(false);

    let content = GtkBox::new(Orientation::Vertical, 12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(8);
    content.set_margin_bottom(12);

    let (panels_widget, controls) = panels::build_panels(state.clone(), estimate_label.clone());
    content.append(&panels_widget);
    content.append(&build_background_section(
        state.clone(),
        estimate_label.clone(),
    ));
    content.append(&build_zoom_section(state.clone(), estimate_label.clone()));
    scroll.set_child(Some(&content));
    root.append(&scroll);

    let actions =
        footer::build_inspector_actions(window, state, estimate_label, controls.clone(), exporting);
    root.append(&actions);

    InspectorParts {
        root,
        _controls: controls,
    }
}

fn build_background_section(state: Arc<Mutex<VideoEditState>>, estimate_label: Label) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 8);
    section.add_css_class("editor-inspector-section");

    let title = Label::new(Some("Background"));
    title.add_css_class("recording-editor-panel-title");
    title.set_xalign(0.0);
    section.append(&title);

    let none = gtk4::CheckButton::with_label("None");
    let plain = gtk4::CheckButton::with_label("Plain");
    let gradient = gtk4::CheckButton::with_label("Gradient");
    for button in [&none, &plain, &gradient] {
        button.add_css_class("recording-editor-audio-choice");
        section.append(button);
    }

    {
        let current = state.lock().unwrap().background;
        none.set_active(matches!(current, VideoBackground::None));
        plain.set_active(matches!(current, VideoBackground::Plain { .. }));
        gradient.set_active(matches!(current, VideoBackground::Gradient(_)));
    }

    let updating = Rc::new(Cell::new(false));
    let apply = {
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        move |background: VideoBackground| {
            state.lock().unwrap().background = background;
            footer::update_estimate(&estimate_label, &state, false);
        }
    };

    wire_exclusive_choice(&none, &[&plain, &gradient], updating.clone(), {
        let apply = apply.clone();
        move || apply(VideoBackground::None)
    });
    wire_exclusive_choice(&plain, &[&none, &gradient], updating.clone(), {
        let apply = apply.clone();
        move || {
            apply(VideoBackground::Plain {
                r: 18,
                g: 18,
                b: 22,
            })
        }
    });
    wire_exclusive_choice(&gradient, &[&none, &plain], updating, {
        let apply = apply.clone();
        move || apply(VideoBackground::Gradient(0))
    });

    let pad = labeled_scale("Padding", 0.0, 80.0, {
        let value = state.lock().unwrap().background_padding;
        value
    });
    pad.connect_value_changed({
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        move |scale| {
            state.lock().unwrap().background_padding = scale.value();
            footer::update_estimate(&estimate_label, &state, false);
        }
    });
    section.append(&pad);

    let corners = labeled_scale("Corners", 0.0, 48.0, {
        state.lock().unwrap().background_corner_radius
    });
    corners.connect_value_changed({
        let state = state.clone();
        move |scale| {
            state.lock().unwrap().background_corner_radius = scale.value();
        }
    });
    section.append(&corners);

    let shadow = labeled_scale("Shadow", 0.0, 80.0, {
        state.lock().unwrap().background_shadow
    });
    shadow.connect_value_changed({
        let state = state.clone();
        move |scale| {
            state.lock().unwrap().background_shadow = scale.value();
        }
    });
    section.append(&shadow);

    section
}

fn build_zoom_section(state: Arc<Mutex<VideoEditState>>, estimate_label: Label) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 8);
    section.add_css_class("editor-inspector-section");
    section.add_css_class("recording-editor-zoom-inspector");

    let title = Label::new(Some("Zoom"));
    title.add_css_class("recording-editor-panel-title");
    title.set_xalign(0.0);
    section.append(&title);

    let hint = Label::new(Some("Select a zoom clip on the timeline"));
    hint.add_css_class("recording-editor-convert-hint");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    section.append(&hint);

    let scale = labeled_scale("Scale", MIN_ZOOM_SCALE, MAX_ZOOM_SCALE, DEFAULT_SCALE);
    let duration = labeled_scale("Duration", 0.4, 6.0, 1.8);
    let reset = Button::with_label("Reset center");
    reset.set_has_frame(false);
    reset.add_css_class("recording-editor-secondary-button");

    section.append(&scale);
    section.append(&duration);
    section.append(&reset);

    scale.connect_value_changed({
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        move |scale| {
            {
                let mut guard = state.lock().unwrap();
                if let Some(index) = guard.selected_zoom {
                    if let Some(clip) = guard.zoom_clips.get_mut(index) {
                        clip.scale = scale.value().clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE);
                    }
                }
            }
            footer::update_estimate(&estimate_label, &state, false);
        }
    });
    duration.connect_value_changed({
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        move |scale_widget| {
            {
                let mut guard = state.lock().unwrap();
                if let Some(index) = guard.selected_zoom {
                    let start = guard.zoom_clips.get(index).map(|clip| clip.start);
                    if let Some(start) = start {
                        let end = start + scale_widget.value();
                        guard.set_zoom_range(index, start, end);
                    }
                }
            }
            footer::update_estimate(&estimate_label, &state, false);
        }
    });
    reset.connect_clicked({
        let state = state.clone();
        move |_| {
            let mut state = state.lock().unwrap();
            if let Some(index) = state.selected_zoom {
                let start = state.zoom_clips.get(index).map(|clip| clip.start);
                if let Some(start) = start {
                    let center = state.default_zoom_center(start);
                    if let Some(clip) = state.zoom_clips.get_mut(index) {
                        clip.center = center;
                    }
                }
            }
        }
    });

    {
        let state = state.clone();
        let hint = hint.clone();
        let scale = scale.clone();
        let duration = duration.clone();
        let reset = reset.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            let selected = {
                let state = state.lock().unwrap();
                state
                    .selected_zoom
                    .and_then(|index| state.zoom_clips.get(index).cloned())
            };
            let enabled = selected.is_some();
            hint.set_visible(!enabled);
            scale.set_sensitive(enabled);
            duration.set_sensitive(enabled);
            reset.set_sensitive(enabled);
            if let Some(clip) = selected {
                if (scale.value() - clip.scale).abs() > 0.01 {
                    scale.set_value(clip.scale);
                }
                let clip_duration = clip.duration();
                if (duration.value() - clip_duration).abs() > 0.05 {
                    duration.set_value(clip_duration);
                }
            }
            gtk4::glib::ControlFlow::Continue
        });
    }

    section
}

const DEFAULT_SCALE: f64 = 1.8;

fn labeled_scale(label: &str, min: f64, max: f64, value: f64) -> Scale {
    let scale = Scale::with_range(Orientation::Horizontal, min, max, 0.05);
    scale.add_css_class("recording-editor-quality-slider");
    scale.set_value(value);
    scale.set_hexpand(true);
    scale.set_draw_value(true);
    scale.set_digits(1);
    scale.set_tooltip_text(Some(label));
    scale
}

fn wire_exclusive_choice(
    button: &gtk4::CheckButton,
    others: &[&gtk4::CheckButton],
    updating: Rc<Cell<bool>>,
    on_select: impl Fn() + 'static,
) {
    let others: Vec<gtk4::CheckButton> = others.iter().map(|button| (*button).clone()).collect();
    button.connect_toggled(move |btn| {
        if updating.get() {
            return;
        }
        if btn.is_active() {
            updating.set(true);
            for other in &others {
                other.set_active(false);
            }
            updating.set(false);
            on_select();
        } else {
            updating.set(true);
            btn.set_active(true);
            updating.set(false);
        }
    });
}
