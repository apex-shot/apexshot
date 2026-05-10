use super::footer;
use crate::recording::editor::model::{AudioMode, DimensionPreset, VideoEditState};
use gtk4::{
    prelude::*, Box as GtkBox, Button, Entry, Label, MenuButton, Orientation, Popover, Scale,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(super) struct EditorControls {
    pub dimension_button: MenuButton,
    pub width_entry: Entry,
    pub height_entry: Entry,
    pub quality_scale: Scale,
    pub audio_unchanged: gtk4::CheckButton,
    pub audio_mono: gtk4::CheckButton,
    pub audio_muted: gtk4::CheckButton,
}

pub(super) fn build_panels(
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
) -> (GtkBox, EditorControls) {
    let panels = GtkBox::new(Orientation::Horizontal, 12);
    panels.add_css_class("recording-editor-panels");
    panels.set_hexpand(true);

    let dimensions = GtkBox::new(Orientation::Vertical, 0);
    dimensions.add_css_class("recording-editor-panel");
    dimensions.set_hexpand(true);

    let dimensions_title = Label::new(Some("Dimensions"));
    dimensions_title.add_css_class("recording-editor-panel-title");
    dimensions_title.set_xalign(0.0);
    dimensions.append(&dimensions_title);

    let dimensions_body = GtkBox::new(Orientation::Vertical, 8);
    dimensions_body.add_css_class("recording-editor-panel-body");

    let original_label = {
        let state = state.lock().unwrap();
        format!(
            "{} x {} (Original)",
            state.metadata.width, state.metadata.height
        )
    };
    let dimension_options: Vec<String> = std::iter::once(original_label.clone())
        .chain(
            ["1920 x 1080", "1280 x 720", "854 x 480", "Custom"]
                .into_iter()
                .map(|s| s.to_string()),
        )
        .collect();

    // Use MenuButton for native popover handling (no freeze)
    let dimension_button = MenuButton::new();
    dimension_button.set_has_frame(false);
    dimension_button.add_css_class("recording-editor-dropdown");
    dimension_button.set_hexpand(true);
    dimension_button.set_label(&original_label);

    let popover = Popover::new();
    popover.set_has_arrow(false);
    popover.add_css_class("recording-editor-dropdown-popover");
    let dimension_list = GtkBox::new(Orientation::Vertical, 0);
    dimension_list.add_css_class("recording-editor-dropdown-list");
    popover.set_child(Some(&dimension_list));
    dimension_button.set_popover(Some(&popover));

    dimensions_body.append(&dimension_button);

    let width_entry = Entry::new();
    let height_entry = Entry::new();
    width_entry.add_css_class("recording-editor-size-entry");
    height_entry.add_css_class("recording-editor-size-entry");
    {
        let state = state.lock().unwrap();
        width_entry.set_text(&state.metadata.width.to_string());
        height_entry.set_text(&state.metadata.height.to_string());
    }
    width_entry.set_sensitive(false);
    height_entry.set_sensitive(false);

    for option in dimension_options {
        let item = Button::with_label(&option);
        item.set_has_frame(false);
        item.add_css_class("recording-editor-dropdown-item");
        item.set_hexpand(true);
        let state_select = state.clone();
        let estimate_label_select = estimate_label.clone();
        let width_entry_select = width_entry.clone();
        let height_entry_select = height_entry.clone();
        let dimension_button_select = dimension_button.clone();
        let popover_select = popover.clone();
        let option_select = option.clone();
        item.connect_clicked(move |_| {
            let preset = DimensionPreset::from_label(&option_select);
            width_entry_select.set_sensitive(preset == DimensionPreset::Custom);
            height_entry_select.set_sensitive(preset == DimensionPreset::Custom);
            let (width, height) = {
                let mut state_guard = state_select.lock().unwrap();
                state_guard.dimension_preset = preset;
                state_guard.target_dimensions()
            };
            width_entry_select.set_text(&width.to_string());
            height_entry_select.set_text(&height.to_string());
            dimension_button_select.set_label(&option_select);
            popover_select.popdown();
            footer::update_estimate(&estimate_label_select, &state_select, false);
        });
        dimension_list.append(&item);
    }

    dimensions_body.append(&field_row("Width", &width_entry));
    dimensions_body.append(&field_row("Height", &height_entry));
    dimensions.append(&dimensions_body);

    let settings = GtkBox::new(Orientation::Vertical, 0);
    settings.add_css_class("recording-editor-panel");
    settings.set_hexpand(true);

    let quality_label = Label::new(Some("Quality"));
    quality_label.add_css_class("recording-editor-panel-title");
    quality_label.set_xalign(0.0);
    settings.append(&quality_label);

    let quality_body = GtkBox::new(Orientation::Vertical, 8);
    quality_body.add_css_class("recording-editor-panel-body");

    let quality_row = GtkBox::new(Orientation::Horizontal, 8);
    let low = Label::new(Some("Low"));
    low.add_css_class("recording-editor-label");
    let high = Label::new(Some("High"));
    high.add_css_class("recording-editor-label");
    let quality_scale = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    quality_scale.add_css_class("recording-editor-quality-slider");
    quality_scale.set_value(70.0);
    quality_scale.set_hexpand(true);
    quality_scale.set_draw_value(false);
    quality_row.append(&low);
    quality_row.append(&quality_scale);
    quality_row.append(&high);
    quality_body.append(&quality_row);

    let audio_label = Label::new(Some("Audio"));
    audio_label.add_css_class("recording-editor-panel-title");
    audio_label.set_xalign(0.0);
    settings.append(&audio_label);

    let audio_body = GtkBox::new(Orientation::Vertical, 4);
    audio_body.add_css_class("recording-editor-panel-body");

    let audio_unchanged = gtk4::CheckButton::with_label("Don't change");
    let audio_mono = gtk4::CheckButton::with_label("Convert to mono");
    let audio_muted = gtk4::CheckButton::with_label("Mute");
    for button in [&audio_unchanged, &audio_mono, &audio_muted] {
        button.add_css_class("recording-editor-audio-choice");
    }
    // No set_group — keeps square checkbox look; mutual exclusion handled in wire_controls
    audio_unchanged.set_active(true);
    audio_body.append(&audio_unchanged);
    audio_body.append(&audio_mono);
    audio_body.append(&audio_muted);
    settings.append(&quality_body);
    settings.append(&audio_body);

    panels.append(&dimensions);
    panels.append(&settings);

    let controls = EditorControls {
        dimension_button,
        width_entry,
        height_entry,
        quality_scale,
        audio_unchanged,
        audio_mono,
        audio_muted,
    };
    wire_controls(&controls, state, estimate_label);

    (panels, controls)
}

fn field_row(label: &str, entry: &Entry) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 10);
    let label = Label::new(Some(label));
    label.add_css_class("recording-editor-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);
    row.append(entry);
    row
}

fn wire_controls(
    controls: &EditorControls,
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
) {
    controls.width_entry.connect_changed({
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        let height_entry = controls.height_entry.clone();
        move |entry| {
            let width = entry.text().parse::<u32>().unwrap_or(64);
            let height = height_entry.text().parse::<u32>().unwrap_or(64);
            let mut state_guard = state.lock().unwrap();
            state_guard.custom_width = width;
            state_guard.custom_height = height;
            drop(state_guard);
            footer::update_estimate(&estimate_label, &state, false);
        }
    });
    controls.height_entry.connect_changed({
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        let width_entry = controls.width_entry.clone();
        move |entry| {
            let width = width_entry.text().parse::<u32>().unwrap_or(64);
            let height = entry.text().parse::<u32>().unwrap_or(64);
            let mut state_guard = state.lock().unwrap();
            state_guard.custom_width = width;
            state_guard.custom_height = height;
            drop(state_guard);
            footer::update_estimate(&estimate_label, &state, false);
        }
    });

    controls.quality_scale.connect_value_changed({
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        move |scale| {
            state.lock().unwrap().quality = scale.value().round().clamp(0.0, 100.0) as u8;
            footer::update_estimate(&estimate_label, &state, false);
        }
    });

    // Manual mutual exclusion for audio checkboxes (square checkbox style)
    let audio_buttons = [
        (controls.audio_unchanged.clone(), AudioMode::Unchanged),
        (controls.audio_mono.clone(), AudioMode::Mono),
        (controls.audio_muted.clone(), AudioMode::Muted),
    ];
    let updating_audio = Rc::new(Cell::new(false));
    for (button, mode) in &audio_buttons {
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        let all_buttons: Vec<gtk4::CheckButton> = audio_buttons.iter().map(|(b, _)| b.clone()).collect();
        let mode = *mode;
        let button_clone = button.clone();
        let updating = updating_audio.clone();
        button.connect_toggled(move |btn| {
            if updating.get() {
                return;
            }
            if btn.is_active() {
                updating.set(true);
                for other in &all_buttons {
                    if other != &button_clone {
                        other.set_active(false);
                    }
                }
                updating.set(false);
                state.lock().unwrap().audio_mode = mode;
                footer::update_estimate(&estimate_label, &state, false);
            } else {
                // Prevent unchecking the active one — force it back on
                updating.set(true);
                btn.set_active(true);
                updating.set(false);
            }
        });
    }
}
