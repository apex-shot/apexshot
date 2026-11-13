//! The Clips panel: the selected piece's speed and sound, and the project's
//! sound.
//!
//! The panel is a view over the timeline. It reads the selected piece and the
//! project's sound settings, and every control writes straight back into them,
//! so an edit is one undo step and the preview and the export hear the same
//! thing.

use super::side_panel::{panel_body, panel_frame, switch_row, value_slider_control, webcam_kicker};
use super::sound::MicPreset;
use super::timeline::{ClipPanelState, Timeline};
use crate::i18n::{t, tfmt};
use gtk4::prelude::*;
use gtk4::{
    Align, ApplicationWindow, Box as GtkBox, Button, FileChooserAction, FileChooserNative,
    FileFilter, Grid, Label, Orientation, ResponseType, ToggleButton,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The speed chips, in the order the row lays them out.
const SPEED_CHIPS: [(f64, &str); 5] = [
    (0.5, "0.5×"),
    (1.0, "1×"),
    (1.5, "1.5×"),
    (2.0, "2×"),
    (4.0, "4×"),
];

/// Build the Clips panel: the selected piece's speed, mute and volumes, then
/// the project's master volume, microphone mode, system sound and background
/// music.
pub(super) fn build_clips_panel(
    scale: f64,
    window: &ApplicationWindow,
    timeline: Timeline,
) -> (GtkBox, Rc<dyn Fn()>) {
    let px = |value: f64| (value * scale).round() as i32;
    let body = panel_body();
    body.set_vexpand(true);
    body.set_valign(Align::Start);

    let syncing = Rc::new(Cell::new(false));

    body.append(&webcam_kicker(&t("Clip")));
    let clip_count = Label::new(None);
    clip_count.add_css_class("recording-editor-zoom-hint");
    clip_count.set_xalign(0.0);
    body.append(&clip_count);
    let clip_hint = Label::new(Some(&t(
        "Select a clip on the timeline to change its speed and sound.",
    )));
    clip_hint.add_css_class("recording-editor-zoom-hint");
    clip_hint.set_wrap(true);
    clip_hint.set_xalign(0.0);
    body.append(&clip_hint);

    let clip_controls = GtkBox::new(Orientation::Vertical, px(8.0));
    clip_controls.set_hexpand(true);

    let speed_row = GtkBox::new(Orientation::Horizontal, px(6.0));
    speed_row.add_css_class("recording-editor-click-effect-row");
    speed_row.set_hexpand(true);
    speed_row.set_homogeneous(true);
    let speed_card = |label: &str| {
        let card = ToggleButton::new();
        card.add_css_class("recording-editor-click-effect-card");
        card.set_has_frame(false);
        card.set_hexpand(true);
        let name = Label::new(Some(label));
        name.add_css_class("recording-editor-click-effect-label");
        name.set_xalign(0.5);
        card.set_child(Some(&name));
        card
    };
    let chips: Vec<ToggleButton> = SPEED_CHIPS
        .iter()
        .map(|(_, label)| speed_card(label))
        .collect();
    for (chip, (chip_speed, _)) in chips.iter().zip(SPEED_CHIPS) {
        speed_row.append(chip);
        chip.connect_toggled({
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            move |chip| {
                if !chip.is_active() || syncing.get() {
                    return;
                }
                timeline.set_selected_clip_speed(chip_speed);
            }
        });
    }
    clip_controls.append(&speed_row);

    let speed_slider = value_slider_control(&t("Speed"), 0.25, 4.0, 1.0, 0.05, 2, scale, {
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        move |value| {
            if !syncing.get() {
                timeline.set_selected_clip_speed(value);
            }
        }
    });
    clip_controls.append(&speed_slider.widget());

    let (mute_row, mute_switch) = switch_row(&t("Mute clip"));
    clip_controls.append(&mute_row);
    mute_switch.connect_state_set({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        move |_, active| {
            if !syncing.get() {
                timeline.update_selected_clip_sound(|clip| clip.muted = active);
            }
            gtk4::glib::Propagation::Proceed
        }
    });

    let mic_slider =
        value_slider_control(&t("Microphone volume"), 0.0, 100.0, 100.0, 1.0, 0, scale, {
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            move |value| {
                if !syncing.get() {
                    timeline.update_selected_clip_sound(|clip| clip.mic_volume = value / 100.0);
                }
            }
        });
    clip_controls.append(&mic_slider.widget());

    let system_slider = value_slider_control(
        &t("System sound volume"),
        0.0,
        100.0,
        100.0,
        1.0,
        0,
        scale,
        {
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            move |value| {
                if !syncing.get() {
                    timeline.update_selected_clip_sound(|clip| clip.system_volume = value / 100.0);
                }
            }
        },
    );
    clip_controls.append(&system_slider.widget());
    body.append(&clip_controls);

    let sound_section = GtkBox::new(Orientation::Vertical, px(8.0));
    sound_section.set_hexpand(true);
    sound_section.append(&webcam_kicker(&t("Sound")));

    let volume_slider = value_slider_control(&t("Volume"), 0.0, 100.0, 100.0, 1.0, 0, scale, {
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        move |value| {
            if !syncing.get() {
                timeline.update_sound(|sound| sound.volume = value / 100.0);
            }
        }
    });
    sound_section.append(&volume_slider.widget());

    let no_sound_hint = Label::new(Some(&t(
        "This recording has no sound. Turn on the microphone or system audio before recording.",
    )));
    no_sound_hint.add_css_class("recording-editor-zoom-hint");
    no_sound_hint.set_wrap(true);
    no_sound_hint.set_xalign(0.0);
    sound_section.append(&no_sound_hint);

    let mic_group = GtkBox::new(Orientation::Vertical, px(8.0));
    mic_group.set_hexpand(true);
    mic_group.append(&webcam_kicker(&t("Microphone")));
    let (mute_mic_row, mute_mic_switch) = switch_row(&t("Mute microphone"));
    mic_group.append(&mute_mic_row);
    mute_mic_switch.connect_state_set({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        move |_, active| {
            if !syncing.get() {
                timeline.update_sound(|sound| sound.mute_microphone = active);
            }
            gtk4::glib::Propagation::Proceed
        }
    });

    let preset_grid = Grid::new();
    preset_grid.set_column_spacing(px(6.0).max(0) as u32);
    preset_grid.set_row_spacing(px(6.0).max(0) as u32);
    preset_grid.set_column_homogeneous(true);
    preset_grid.set_hexpand(true);
    let mut tiles: Vec<(MicPreset, ToggleButton)> = Vec::new();
    for preset in MicPreset::ALL {
        let tile = ToggleButton::new();
        tile.add_css_class("recording-editor-click-effect-card");
        tile.set_has_frame(false);
        tile.set_hexpand(true);
        let inner = GtkBox::new(Orientation::Vertical, px(2.0));
        inner.set_valign(Align::Center);
        let name = Label::new(Some(&t(preset.label())));
        name.add_css_class("recording-editor-click-effect-label");
        name.set_xalign(0.5);
        inner.append(&name);
        tile.set_child(Some(&inner));
        tile.set_tooltip_text(Some(&t(preset.description())));
        tiles.push((preset, tile));
    }
    for (_, tile) in tiles.iter().skip(1) {
        tile.set_group(Some(&tiles[0].1));
    }
    for (index, (preset, tile)) in tiles.iter().enumerate() {
        preset_grid.attach(tile, (index % 2) as i32, (index / 2) as i32, 1, 1);
        let preset = *preset;
        tile.connect_toggled({
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            move |tile| {
                if !tile.is_active() || syncing.get() {
                    return;
                }
                timeline.update_sound(|sound| sound.microphone_preset = preset);
            }
        });
    }
    mic_group.append(&preset_grid);
    sound_section.append(&mic_group);

    let system_group = GtkBox::new(Orientation::Vertical, px(8.0));
    system_group.set_hexpand(true);
    system_group.append(&webcam_kicker(&t("System sound")));
    let (mute_system_row, mute_system_switch) = switch_row(&t("Mute system sound"));
    system_group.append(&mute_system_row);
    mute_system_switch.connect_state_set({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        move |_, active| {
            if !syncing.get() {
                timeline.update_sound(|sound| sound.mute_system = active);
            }
            gtk4::glib::Propagation::Proceed
        }
    });
    sound_section.append(&system_group);

    sound_section.append(&webcam_kicker(&t("Background music")));
    let music_row = GtkBox::new(Orientation::Horizontal, px(8.0));
    music_row.set_hexpand(true);
    let music_name = Label::new(Some(&t("No music")));
    music_name.add_css_class("recording-editor-zoom-hint");
    music_name.set_xalign(0.0);
    music_name.set_hexpand(true);
    music_name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    let choose = Button::with_label(&t("Choose…"));
    choose.add_css_class("recording-editor-secondary-button");
    let remove = Button::with_label(&t("Remove"));
    remove.add_css_class("recording-editor-secondary-button");
    music_row.append(&music_name);
    music_row.append(&choose);
    music_row.append(&remove);
    sound_section.append(&music_row);

    let chooser_slot: Rc<RefCell<Option<FileChooserNative>>> = Rc::new(RefCell::new(None));
    choose.connect_clicked({
        let window = window.clone();
        let timeline = timeline.clone();
        let chooser_slot = chooser_slot.clone();
        move |_| {
            if chooser_slot.borrow().is_some() {
                return;
            }
            let chooser = FileChooserNative::new(
                Some(&t("Choose background music")),
                Some(&window),
                FileChooserAction::Open,
                Some(&t("Choose")),
                Some(&t("Cancel")),
            );
            let filter = FileFilter::new();
            filter.set_name(Some(&t("Audio")));
            for pattern in [
                "*.mp3", "*.wav", "*.m4a", "*.aac", "*.ogg", "*.flac", "*.opus",
            ] {
                filter.add_pattern(pattern);
            }
            chooser.set_filter(&filter);
            let timeline = timeline.clone();
            let slot = Rc::downgrade(&chooser_slot);
            chooser.connect_response(move |dialog, response| {
                let path = (response == ResponseType::Accept)
                    .then(|| dialog.file().and_then(|file| file.path()))
                    .flatten();
                dialog.hide();
                if let Some(slot) = slot.upgrade() {
                    slot.borrow_mut().take();
                }
                let Some(path) = path else {
                    return;
                };
                timeline.update_sound(|sound| sound.background_file = Some(path));
            });
            *chooser_slot.borrow_mut() = Some(chooser.clone());
            chooser.show();
        }
    });
    remove.connect_clicked({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        move |_| {
            if !syncing.get() {
                timeline.update_sound(|sound| sound.background_file = None);
            }
        }
    });

    let music_controls = GtkBox::new(Orientation::Vertical, px(8.0));
    music_controls.set_hexpand(true);
    let music_volume = value_slider_control(&t("Music volume"), 0.0, 100.0, 5.0, 1.0, 0, scale, {
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        move |value| {
            if !syncing.get() {
                timeline.update_sound(|sound| sound.background_volume = value / 100.0);
            }
        }
    });
    music_controls.append(&music_volume.widget());
    let (mute_music_row, mute_music_switch) = switch_row(&t("Mute music"));
    music_controls.append(&mute_music_row);
    mute_music_switch.connect_state_set({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        move |_, active| {
            if !syncing.get() {
                timeline.update_sound(|sound| sound.mute_background = active);
            }
            gtk4::glib::Propagation::Proceed
        }
    });
    sound_section.append(&music_controls);
    body.append(&sound_section);

    let last: Rc<RefCell<Option<ClipPanelState>>> = Rc::new(RefCell::new(None));
    let refresh: Rc<dyn Fn()> = {
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        let clip_count = clip_count.clone();
        let clip_hint = clip_hint.clone();
        let clip_controls = clip_controls.clone();
        let chips = chips.clone();
        let speed_slider = speed_slider.clone();
        let mute_switch = mute_switch.clone();
        let mic_slider = mic_slider.clone();
        let system_slider = system_slider.clone();
        let sound_section = sound_section.clone();
        let volume_slider = volume_slider.clone();
        let no_sound_hint = no_sound_hint.clone();
        let mic_group = mic_group.clone();
        let mute_mic_switch = mute_mic_switch.clone();
        let preset_grid = preset_grid.clone();
        let tiles = tiles.clone();
        let system_group = system_group.clone();
        let mute_system_switch = mute_system_switch.clone();
        let music_name = music_name.clone();
        let remove = remove.clone();
        let music_controls = music_controls.clone();
        let music_volume = music_volume.clone();
        let mute_music_switch = mute_music_switch.clone();
        Rc::new(move || {
            let state = timeline.clip_panel_state();
            if last.borrow().as_ref() == Some(&state) {
                return;
            }
            *last.borrow_mut() = Some(state.clone());
            syncing.set(true);
            let current = state.segment.map_or(0, |index| index + 1).to_string();
            let total = state.segment_count.to_string();
            clip_count.set_text(&tfmt(
                "Clip {current} of {total}",
                &[("current", current.as_str()), ("total", total.as_str())],
            ));
            clip_count.set_visible(state.segment.is_some());
            clip_hint.set_visible(state.segment.is_none());
            clip_controls.set_visible(state.segment.is_some());
            for (chip, (chip_speed, _)) in chips.iter().zip(SPEED_CHIPS) {
                chip.set_active((state.speed - chip_speed).abs() < 1e-6);
            }
            speed_slider.set_value(state.speed);
            mute_switch.set_active(state.clip.muted);
            mic_slider.set_value(state.clip.mic_volume * 100.0);
            mic_slider.widget().set_visible(state.has_microphone);
            mic_slider.widget().set_sensitive(!state.clip.muted);
            system_slider.set_value(state.clip.system_volume * 100.0);
            system_slider.widget().set_visible(state.has_system);
            system_slider.widget().set_sensitive(!state.clip.muted);
            sound_section.set_visible(state.has_media);
            volume_slider.set_value(state.settings.volume * 100.0);
            no_sound_hint.set_visible(!state.has_microphone && !state.has_system);
            mic_group.set_visible(state.has_microphone);
            mute_mic_switch.set_active(state.settings.mute_microphone);
            for (preset, tile) in tiles.iter() {
                tile.set_active(state.settings.microphone_preset == *preset);
            }
            preset_grid.set_sensitive(!state.settings.mute_microphone);
            system_group.set_visible(state.has_system);
            mute_system_switch.set_active(state.settings.mute_system);
            let music = state.settings.background_file.as_ref();
            music_name.set_text(
                &music
                    .and_then(|path| path.file_name())
                    .map_or_else(|| t("No music"), |name| name.to_string_lossy().into_owned()),
            );
            remove.set_visible(music.is_some());
            music_controls.set_visible(music.is_some());
            music_volume.set_value(state.settings.background_volume * 100.0);
            mute_music_switch.set_active(state.settings.mute_background);
            syncing.set(false);
        })
    };

    (panel_frame(&t("Clips"), body), refresh)
}
