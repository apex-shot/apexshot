use super::dialogs;
use super::export_flow;
use crate::i18n::t;
use crate::recording::editor::export_settings::{Codec, ExportSettings, HardwarePreference};
use crate::recording::editor::hardware_encode;
use crate::recording::editor::model::{format_size, VideoEditState};
use gtk4::{
    gdk, glib, prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, EventControllerKey,
    Image, Label, Orientation, ScrolledWindow, Switch, ToggleButton,
};
use std::cell::Cell;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExportAction {
    SaveToFile,
    CopyToClipboard,
}

const RESOLUTION_CHOICES: [Option<u32>; 4] = [None, Some(720), Some(1080), Some(2160)];
const FPS_CHOICES: [Option<u32>; 4] = [None, Some(60), Some(30), Some(24)];
const QUALITY_CHOICES: [(u32, &str, &str); 4] = [
    (
        14,
        "Maximum",
        "Near-lossless. Best for archiving or editing further, with the largest files.",
    ),
    (
        24,
        "High",
        "Sharp detail at a moderate size. A good default for sharing.",
    ),
    (
        30,
        "Balanced",
        "Smaller files for websites, docs, and chat, with little visible loss.",
    ),
    (
        42,
        "Compact",
        "The smallest files. Fine detail and small text may soften.",
    ),
];

fn nearest_quality_index(project_crf: u32) -> usize {
    QUALITY_CHOICES
        .iter()
        .enumerate()
        .min_by_key(|(_, (crf, _, _))| (*crf as i64 - project_crf as i64).abs())
        .map(|(index, _)| index)
        .unwrap_or(1)
}

fn selected_crf(project_initial: usize, selected: usize) -> Option<u32> {
    (selected != project_initial).then_some(QUALITY_CHOICES[selected].0)
}

fn fps_text(fps: f64) -> String {
    if (fps - fps.round()).abs() < 0.01 {
        format!("{} FPS", fps.round() as u32)
    } else {
        format!("{fps:.2} FPS")
    }
}

fn section(title: &str) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 8);
    section.set_valign(Align::Start);
    let label = Label::new(Some(&t(title).to_uppercase()));
    label.add_css_class("recording-editor-export-eyebrow");
    label.set_xalign(0.0);
    section.append(&label);
    section
}

fn segmented(options: &[(String, String)], active: usize) -> (GtkBox, Vec<ToggleButton>) {
    let track = GtkBox::new(Orientation::Horizontal, 4);
    track.add_css_class("recording-editor-export-segments");
    track.set_homogeneous(true);
    let mut buttons: Vec<ToggleButton> = Vec::with_capacity(options.len());
    for (index, (title, detail)) in options.iter().enumerate() {
        let button = ToggleButton::new();
        button.add_css_class("recording-editor-export-segment");
        if detail.is_empty() {
            button.set_label(title);
        } else {
            button.add_css_class("recording-editor-export-segment-tall");
            let lines = GtkBox::new(Orientation::Vertical, 1);
            lines.set_halign(Align::Center);
            lines.set_valign(Align::Center);
            let heading = Label::new(Some(title));
            heading.add_css_class("recording-editor-export-segment-title");
            let sub = Label::new(Some(detail));
            sub.add_css_class("recording-editor-export-segment-sub");
            lines.append(&heading);
            lines.append(&sub);
            button.set_child(Some(&lines));
            button.update_property(&[gtk4::accessible::Property::Label(&format!(
                "{title} {detail}"
            ))]);
        }
        if let Some(first) = buttons.first() {
            button.set_group(Some(first));
        }
        button.set_active(index == active);
        track.append(&button);
        buttons.push(button);
    }
    (track, buttons)
}

fn summary_row(summary: &GtkBox, label: &str) -> Label {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    let name = Label::new(Some(label));
    name.add_css_class("recording-editor-export-summary-label");
    name.set_xalign(0.0);
    name.set_hexpand(true);
    let value = Label::new(None);
    value.add_css_class("recording-editor-export-summary-value");
    value.set_xalign(1.0);
    value.set_wrap(true);
    value.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    value.set_justify(gtk4::Justification::Right);
    row.append(&name);
    row.append(&value);
    summary.append(&row);
    value
}

fn estimate_bytes(state: &VideoEditState, settings: &ExportSettings) -> u64 {
    let base = state.estimated_size_bytes(false) as f64;
    let (base_w, base_h) = state.output_dimensions();
    let (w, h) = settings.dimensions((base_w, base_h));
    let pixel_factor = (w as f64 * h as f64) / (base_w as f64 * base_h as f64).max(1.0);
    let source_fps = state.metadata.export_frame_rate().max(1.0);
    let fps_factor = settings
        .fps
        .map(|fps| (fps as f64 / source_fps).clamp(0.1, 4.0))
        .unwrap_or(1.0);
    let crf_factor = settings
        .crf
        .map(|crf| 2f64.powf((state.quality.crf() as f64 - crf as f64) / 6.0))
        .unwrap_or(1.0);
    (base * pixel_factor * fps_factor * crf_factor)
        .round()
        .max(0.0) as u64
}

#[derive(Debug, Clone, Copy)]
struct CodecAvailability {
    h264_software: bool,
    h264_auto: bool,
    hevc_software: bool,
    hevc_auto: bool,
}

impl CodecAvailability {
    fn allows(&self, codec: Codec, hardware: HardwarePreference) -> bool {
        match (codec, hardware) {
            (Codec::Hevc, HardwarePreference::Auto) => self.hevc_auto,
            (Codec::Hevc, _) => self.hevc_software,
            (_, HardwarePreference::Auto) => self.h264_auto,
            _ => self.h264_software,
        }
    }

    fn any_hevc(&self) -> bool {
        self.hevc_software || self.hevc_auto
    }
}

pub(super) fn show(
    window: &ApplicationWindow,
    snapshot: VideoEditState,
    flow: export_flow::FlowSlot,
    then: Rc<dyn Fn(VideoEditState, ExportAction)>,
) -> GtkBox {
    use crate::capture::editor::window::icon_names::custom;
    let dark = crate::settings::windowing::prefers_dark_glass_theme();
    let icon_name = if dark {
        custom::VIDEO
    } else {
        custom::VIDEO_LIGHT
    };

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("recording-editor-export-options-card");
    card.set_halign(Align::Center);
    card.set_valign(Align::Center);
    let measured_width = window.width();
    let window_width = if measured_width > 0 {
        measured_width
    } else {
        window.default_width()
    };
    let target_width = if window_width > 0 {
        (window_width - 48).clamp(360, 580)
    } else {
        580
    };
    card.set_size_request(target_width, -1);

    let header = GtkBox::new(Orientation::Horizontal, 12);
    header.add_css_class("recording-editor-export-header");
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(24);
    icon.set_valign(Align::Center);
    let title = Label::new(Some(&t("Export video")));
    title.add_css_class("recording-editor-export-dialog-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_valign(Align::Center);
    let close = Button::from_icon_name("window-close-symbolic");
    close.add_css_class("recording-editor-export-close");
    close.set_focusable(true);
    close.set_size_request(30, 30);
    close.set_valign(Align::Center);
    close.set_tooltip_text(Some(&t("Close export options")));
    header.append(&icon);
    header.append(&title);
    header.append(&close);
    card.append(&header);

    let scroll = ScrolledWindow::new();
    scroll.add_css_class("recording-editor-export-scroll");
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_hexpand(true);
    scroll.set_propagate_natural_height(true);
    let measured_height = window.height();
    let window_height = if measured_height > 0 {
        measured_height
    } else {
        window.default_height()
    };
    let body_budget = if window_height > 0 {
        (window_height - 48 - 54 - 48 - 52).max(280)
    } else {
        560
    };
    scroll.set_max_content_height(body_budget);
    let content = GtkBox::new(Orientation::Vertical, 20);
    scroll.set_child(Some(&content));
    card.append(&scroll);

    let frame_size = snapshot.output_dimensions();
    let original_edge = frame_size.0.min(frame_size.1);
    let source_fps = snapshot.metadata.export_frame_rate();

    let resolution_options: Vec<(String, String)> = RESOLUTION_CHOICES
        .iter()
        .map(|edge| match edge {
            None => (t("Original"), format!("{original_edge}p")),
            Some(720) => ("HD".to_string(), "720p".to_string()),
            Some(1080) => ("FHD".to_string(), "1080p".to_string()),
            _ => ("4K".to_string(), "2160p".to_string()),
        })
        .collect();
    let resolution_choice = Rc::new(Cell::new(0usize));
    let (resolution_track, resolution_buttons) = segmented(&resolution_options, 0);
    for (index, button) in resolution_buttons.iter().enumerate() {
        let choice = resolution_choice.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                choice.set(index);
            }
        });
    }
    let resolution_section = section("Resolution");
    resolution_section.append(&resolution_track);
    content.append(&resolution_section);

    let fps_choice = Rc::new(Cell::new(0usize));
    let fps_options: Vec<(String, String)> = FPS_CHOICES
        .iter()
        .map(|rate| {
            (
                rate.map_or_else(|| t("Original"), |fps| fps.to_string()),
                String::new(),
            )
        })
        .collect();
    let (fps_track, fps_buttons) = segmented(&fps_options, 0);
    fps_buttons[0].set_tooltip_text(Some(&fps_text(source_fps)));
    for (index, button) in fps_buttons.iter().enumerate() {
        let choice = fps_choice.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                choice.set(index);
            }
        });
    }
    let fps_section = section("Frame rate");
    fps_section.set_hexpand(true);
    fps_section.append(&fps_track);

    let codec_choice = Rc::new(Cell::new(Codec::H264));
    let codec_options = [
        ("H.264".to_string(), String::new()),
        ("HEVC".to_string(), String::new()),
    ];
    let (codec_track, codec_buttons) = segmented(&codec_options, 0);
    for (index, button) in codec_buttons.iter().enumerate() {
        let codec_choice = codec_choice.clone();
        let codec = if index == 1 { Codec::Hevc } else { Codec::H264 };
        button.connect_toggled(move |button| {
            if button.is_active() {
                codec_choice.set(codec);
            }
        });
    }
    let hevc_button = codec_buttons[1].clone();
    hevc_button.set_sensitive(false);
    let checking = t("Checking encoder support…");
    hevc_button.set_tooltip_text(Some(&checking));
    let codec_section = section("Codec");
    codec_section.append(&codec_track);
    let pickers_row = GtkBox::new(Orientation::Horizontal, 16);
    pickers_row.append(&fps_section);
    pickers_row.append(&codec_section);
    let codec_note = Label::new(Some(&t(
        "Smaller files at the same quality, but not every app or browser plays HEVC.",
    )));
    codec_note.add_css_class("recording-editor-export-muted");
    codec_note.set_xalign(0.0);
    codec_note.set_wrap(true);
    codec_note.set_visible(false);
    let pickers = GtkBox::new(Orientation::Vertical, 8);
    pickers.append(&pickers_row);
    pickers.append(&codec_note);
    content.append(&pickers);

    let quality_choice = Rc::new(Cell::new(nearest_quality_index(snapshot.quality.crf())));
    let quality_options: Vec<(String, String)> = QUALITY_CHOICES
        .iter()
        .map(|(_, name, _)| (t(name), String::new()))
        .collect();
    let (quality_track, quality_buttons) = segmented(&quality_options, quality_choice.get());
    for (index, button) in quality_buttons.iter().enumerate() {
        let choice = quality_choice.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                choice.set(index);
            }
        });
    }
    let description = Label::new(None);
    description.add_css_class("recording-editor-export-muted");
    description.set_xalign(0.0);
    description.set_wrap(true);
    description.set_max_width_chars(70);
    let quality_section = section("Quality");
    quality_section.append(&quality_track);
    quality_section.append(&description);
    content.append(&quality_section);

    let hardware_choice = Rc::new(Cell::new(false));
    let hardware_card = GtkBox::new(Orientation::Horizontal, 14);
    hardware_card.add_css_class("recording-editor-export-toggle-card");
    let hardware_note_text =
        t("Use the GPU encoder when one is available. Falls back to the CPU automatically.");
    hardware_card.set_tooltip_text(Some(&hardware_note_text));
    let hardware_text = GtkBox::new(Orientation::Vertical, 3);
    hardware_text.set_hexpand(true);
    hardware_text.set_valign(Align::Center);
    let hardware_title = Label::new(Some(&t("Hardware acceleration")));
    hardware_title.add_css_class("recording-editor-export-toggle-title");
    hardware_title.set_xalign(0.0);
    let hardware_note = Label::new(Some(&hardware_note_text));
    hardware_note.add_css_class("recording-editor-export-muted");
    hardware_note.set_xalign(0.0);
    hardware_note.set_wrap(true);
    hardware_text.append(&hardware_title);
    hardware_text.append(&hardware_note);
    let hardware_switch = Switch::new();
    hardware_switch.add_css_class("recording-editor-export-switch");
    hardware_switch.set_valign(Align::Center);
    hardware_switch.set_active(false);
    hardware_switch.update_property(&[gtk4::accessible::Property::Label(&t(
        "Hardware acceleration",
    ))]);
    hardware_switch.connect_state_set({
        let hardware_choice = hardware_choice.clone();
        move |_, state| {
            hardware_choice.set(state);
            glib::Propagation::Proceed
        }
    });
    hardware_card.append(&hardware_text);
    hardware_card.append(&hardware_switch);
    content.append(&hardware_card);

    let summary = GtkBox::new(Orientation::Vertical, 8);
    summary.add_css_class("recording-editor-export-summary");
    let output_value = summary_row(&summary, &t("Output"));
    let format_value = summary_row(&summary, &t("Format"));
    let size_value = summary_row(&summary, &t("Estimated size"));
    let time_value = summary_row(&summary, &t("Export time"));
    time_value.set_text(&t("Depends on hardware"));
    content.append(&summary);

    let availability_note = Label::new(None);
    availability_note.add_css_class("recording-editor-export-muted");
    availability_note.set_xalign(0.0);
    availability_note.set_wrap(true);
    availability_note.set_visible(false);
    content.append(&availability_note);

    let actions = GtkBox::new(Orientation::Horizontal, 16);
    actions.add_css_class("recording-editor-export-actions");
    let copy = Button::with_label(&t("Copy to clipboard"));
    copy.add_css_class("recording-editor-export-action");
    copy.set_width_request(170);
    copy.set_tooltip_text(Some(&t(
        "Export a file and copy it to the clipboard for pasting into other apps.",
    )));
    copy.set_sensitive(false);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let export = Button::with_label(&t("Export to file"));
    export.add_css_class("recording-editor-export-action");
    export.add_css_class("recording-editor-export-primary");
    export.set_width_request(170);
    export.set_sensitive(false);
    actions.append(&copy);
    actions.append(&spacer);
    actions.append(&export);
    card.append(&actions);

    let availability = Rc::new(Cell::new(None::<CodecAvailability>));

    let current_settings = {
        let resolution_choice = resolution_choice.clone();
        let fps_choice = fps_choice.clone();
        let codec_choice = codec_choice.clone();
        let quality_choice = quality_choice.clone();
        let hardware_choice = hardware_choice.clone();
        let initial = quality_choice.get();
        Rc::new(move || ExportSettings {
            resolution: RESOLUTION_CHOICES[resolution_choice.get()],
            fps: FPS_CHOICES[fps_choice.get()],
            codec: codec_choice.get(),
            crf: selected_crf(initial, quality_choice.get()),
            hardware: if hardware_choice.get() {
                HardwarePreference::Auto
            } else {
                HardwarePreference::Software
            },
        })
    };

    let update_summary = {
        let snapshot = snapshot.clone();
        let current_settings = current_settings.clone();
        let quality_choice = quality_choice.clone();
        let availability = availability.clone();
        let codec_note = codec_note.clone();
        let output_value = output_value.clone();
        let format_value = format_value.clone();
        let size_value = size_value.clone();
        let description = description.clone();
        let availability_note = availability_note.clone();
        let export_button = export.clone();
        let copy_button = copy.clone();
        let hevc_button = hevc_button.clone();
        move || {
            let settings = current_settings().sanitized();
            let (w, h) = settings.dimensions(frame_size);
            let fps = settings.fps.map_or(source_fps, f64::from);
            output_value.set_text(&format!("{w} × {h} · {}", fps_text(fps)));
            let hevc = settings.effective_codec() == Codec::Hevc;
            format_value.set_text(if hevc { "MP4 · HEVC" } else { "MP4 · H.264" });
            codec_note.set_visible(hevc);
            size_value.set_text(&format!(
                "≈ {}",
                format_size(estimate_bytes(&snapshot, &settings))
            ));
            let quality_index = quality_choice.get();
            let (_, _, quality_description) = QUALITY_CHOICES[quality_index];
            description.set_text(&t(quality_description));

            let allows = availability
                .get()
                .map(|state| state.allows(settings.effective_codec(), settings.hardware));
            match allows {
                None => {
                    availability_note.set_visible(false);
                    hevc_button.set_sensitive(false);
                    export_button.set_sensitive(!hevc);
                    copy_button.set_sensitive(!hevc);
                }
                Some(true) => {
                    availability_note.set_visible(false);
                    let any_hevc = availability.get().is_some_and(|state| state.any_hevc());
                    hevc_button.set_sensitive(any_hevc);
                    if any_hevc {
                        let ok = t("Encode with HEVC");
                        hevc_button.set_tooltip_text(Some(&ok));
                    } else {
                        let unavail = t("This ffmpeg build has no HEVC encoder");
                        hevc_button.set_tooltip_text(Some(&unavail));
                    }
                    export_button.set_sensitive(true);
                    copy_button.set_sensitive(true);
                }
                Some(false) => {
                    let message = t("The selected codec cannot encode on this machine");
                    availability_note.set_text(&message);
                    availability_note.set_visible(true);
                    export_button.set_sensitive(false);
                    copy_button.set_sensitive(false);
                }
            }
        }
    };
    for button in resolution_buttons
        .iter()
        .chain(fps_buttons.iter())
        .chain(codec_buttons.iter())
        .chain(quality_buttons.iter())
    {
        button.connect_toggled({
            let update_summary = update_summary.clone();
            move |_| update_summary()
        });
    }
    hardware_switch.connect_state_set({
        let update_summary = update_summary.clone();
        move |_, _| {
            update_summary();
            glib::Propagation::Proceed
        }
    });

    let (availability_sender, availability_receiver) =
        std::sync::mpsc::channel::<CodecAvailability>();
    std::thread::spawn(move || {
        let result = CodecAvailability {
            h264_software: hardware_encode::codec_export_available(
                Codec::H264,
                HardwarePreference::Software,
            ),
            h264_auto: hardware_encode::codec_export_available(
                Codec::H264,
                HardwarePreference::Auto,
            ),
            hevc_software: hardware_encode::codec_export_available(
                Codec::Hevc,
                HardwarePreference::Software,
            ),
            hevc_auto: hardware_encode::codec_export_available(
                Codec::Hevc,
                HardwarePreference::Auto,
            ),
        };
        let _ = availability_sender.send(result);
    });
    glib::timeout_add_local(std::time::Duration::from_millis(120), {
        let availability = availability.clone();
        let update_summary = update_summary.clone();
        let card = card.clone();
        move || match availability_receiver.try_recv() {
            Ok(result) => {
                availability.set(Some(result));
                update_summary();
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if card.parent().is_none() {
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                update_summary();
                glib::ControlFlow::Break
            }
        }
    });
    update_summary();

    let busy = Rc::new(Cell::new(false));
    let action_taken = Rc::new(Cell::new(false));

    let Some(overlay) = window
        .child()
        .and_then(|child| child.downcast::<gtk4::Overlay>().ok())
    else {
        export_flow::release(&flow);
        return card;
    };
    dialogs::set_editor_modal_blur(window, true);
    overlay.add_overlay(&card);

    let dismiss = {
        let card = card.downgrade();
        let overlay = overlay.downgrade();
        let window = window.downgrade();
        Rc::new(move || {
            if let (Some(overlay), Some(card)) = (overlay.upgrade(), card.upgrade()) {
                overlay.remove_overlay(&card);
            }
            if let Some(window) = window.upgrade() {
                dialogs::set_editor_modal_blur(&window, false);
            }
        })
    };

    let run = {
        let busy = busy.clone();
        let action_taken = action_taken.clone();
        let dismiss = dismiss.clone();
        let then = then.clone();
        let snapshot = snapshot.clone();
        move |action: ExportAction| {
            if busy.replace(true) {
                return;
            }
            action_taken.set(true);
            let mut export_state = snapshot.clone();
            export_state.export_settings = current_settings().sanitized();
            dismiss();
            then(export_state, action);
        }
    };
    export.connect_clicked({
        let run = run.clone();
        move |_| run(ExportAction::SaveToFile)
    });
    copy.connect_clicked({
        let run = run.clone();
        move |_| run(ExportAction::CopyToClipboard)
    });
    close.connect_clicked({
        let dismiss = dismiss.clone();
        move |_| dismiss()
    });
    card.connect_parent_notify({
        let action_taken = action_taken.clone();
        let flow = flow.clone();
        let window = window.downgrade();
        move |card| {
            if card.parent().is_some() {
                return;
            }
            if let Some(window) = window.upgrade() {
                dialogs::set_editor_modal_blur(&window, false);
            }
            if !action_taken.get() {
                export_flow::release(&flow);
            }
        }
    });
    let keys = EventControllerKey::new();
    keys.connect_key_pressed({
        let dismiss = dismiss.clone();
        move |_, key, _, _| {
            if key == gdk::Key::Escape {
                dismiss();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    });
    card.add_controller(keys);
    card.set_visible(true);
    if !export.grab_focus() {
        close.grab_focus();
    }
    card
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn the_nearest_quality_tier_tracks_the_project_crf() {
        assert_eq!(nearest_quality_index(14), 0);
        assert_eq!(nearest_quality_index(16), 0);
        assert_eq!(nearest_quality_index(24), 1);
        assert_eq!(nearest_quality_index(20), 1);
        assert_eq!(nearest_quality_index(30), 2);
        assert_eq!(nearest_quality_index(51), 3);
        assert_eq!(QUALITY_CHOICES.len(), 4);
    }

    #[test]
    fn the_initial_tier_forwards_the_project_crf_until_changed() {
        let initial = nearest_quality_index(24);
        assert_eq!(selected_crf(initial, initial), None);
        assert_eq!(selected_crf(initial, 0), Some(14));
        assert_eq!(selected_crf(initial, 3), Some(42));
    }

    fn editor_window_with_root() -> (gtk4::Application, ApplicationWindow, gtk4::Widget) {
        let app = crate::test_support::test_app();
        let window = ApplicationWindow::new(&app);
        let shell = gtk4::Overlay::new();
        let root = GtkBox::new(Orientation::Vertical, 0);
        root.add_css_class("recording-editor-root");
        shell.set_child(Some(&root));
        window.set_child(Some(&shell));
        window.present();
        (app, window, root.upcast())
    }

    fn blank_state() -> VideoEditState {
        VideoEditState::new(crate::recording::editor::model::VideoMetadata {
            path: std::path::PathBuf::from("/tmp/input.mp4"),
            duration_seconds: 10.0,
            width: 1920,
            height: 1080,
            file_size_bytes: 1024,
            has_audio: false,
            frame_rate: 30.0,
        })
    }

    fn find_button_by_label(root: &gtk4::Widget, label: &str) -> Option<Button> {
        if let Ok(button) = root.clone().downcast::<Button>() {
            if button.label().as_deref() == Some(label) {
                return Some(button);
            }
        }
        let mut child = root.first_child();
        while let Some(widget) = child {
            if let Some(found) = find_button_by_label(&widget, label) {
                return Some(found);
            }
            child = widget.next_sibling();
        }
        None
    }

    fn widgets_with_class(root: &gtk4::Widget, class: &str) -> Vec<gtk4::Widget> {
        let mut widgets = Vec::new();
        if root.has_css_class(class) {
            widgets.push(root.clone());
        }
        let mut child = root.first_child();
        while let Some(widget) = child {
            widgets.extend(widgets_with_class(&widget, class));
            child = widget.next_sibling();
        }
        widgets
    }

    fn find_button_by_icon(root: &gtk4::Widget, icon_name: &str) -> Option<Button> {
        if let Ok(button) = root.clone().downcast::<Button>() {
            if button.icon_name().as_deref() == Some(icon_name) {
                return Some(button);
            }
        }
        let mut child = root.first_child();
        while let Some(widget) = child {
            if let Some(found) = find_button_by_icon(&widget, icon_name) {
                return Some(found);
            }
            child = widget.next_sibling();
        }
        None
    }

    #[test]
    fn the_sheet_holds_the_flow_and_dismissal_releases_it() {
        let Some((active_before, sensitive_before, active_after, sensitive_after)) =
            crate::test_support::with_gtk(|| {
                use gtk4::prelude::*;
                let (_app, window, root) = editor_window_with_root();
                let active: export_flow::FlowActive = Rc::new(Cell::new(false));
                let flow = export_flow::ActiveFlow::begin(&window, &active).unwrap();
                let card = super::show(&window, blank_state(), flow, Rc::new(|_, _| {}))
                    .upcast::<gtk4::Widget>();
                window.present();
                let before = (active.get(), root.is_sensitive());
                let shell = window.child().unwrap().downcast::<gtk4::Overlay>().unwrap();
                shell.remove_overlay(&card);
                (before.0, before.1, active.get(), root.is_sensitive())
            })
        else {
            return;
        };
        assert!(active_before && !sensitive_before);
        assert!(!active_after && sensitive_after);
    }

    #[test]
    fn the_primary_action_forwards_an_immutable_snapshot() {
        let Some((codec, crf, hardware, fps, action, still_active)) =
            crate::test_support::with_gtk(|| {
                use gtk4::prelude::*;
                let (_app, window, _root) = editor_window_with_root();
                let active: export_flow::FlowActive = Rc::new(Cell::new(false));
                let flow = export_flow::ActiveFlow::begin(&window, &active).unwrap();
                let captured = Rc::new(RefCell::new(None::<(ExportSettings, ExportAction)>));
                let card = super::show(&window, blank_state(), flow, {
                    let captured = captured.clone();
                    Rc::new(move |state, action| {
                        captured.replace(Some((state.export_settings, action)));
                    })
                })
                .upcast::<gtk4::Widget>();
                let export = find_button_by_label(&card, &t("Export to file"))
                    .expect("the primary export button");
                export.emit_clicked();
                let (settings, action) = *captured.borrow().as_ref().expect("the action ran");
                (
                    settings.effective_codec(),
                    settings.crf,
                    settings.hardware,
                    settings.fps,
                    action,
                    active.get(),
                )
            })
        else {
            return;
        };
        assert_eq!(codec, Codec::H264);
        assert_eq!(crf, None);
        assert_eq!(hardware, HardwarePreference::Software);
        assert_eq!(fps, None);
        assert_eq!(action, ExportAction::SaveToFile);
        assert!(still_active, "the flow must stay held for the chooser");
    }

    #[test]
    fn the_copy_action_forwards_copy_and_a_chosen_resolution() {
        let Some((resolution, action)) = crate::test_support::with_gtk(|| {
            use gtk4::prelude::*;
            let (_app, window, _root) = editor_window_with_root();
            let active: export_flow::FlowActive = Rc::new(Cell::new(false));
            let flow = export_flow::ActiveFlow::begin(&window, &active).unwrap();
            let captured = Rc::new(RefCell::new(None::<(ExportSettings, ExportAction)>));
            let card = super::show(&window, blank_state(), flow, {
                let captured = captured.clone();
                Rc::new(move |state, action| {
                    captured.replace(Some((state.export_settings, action)));
                })
            })
            .upcast::<gtk4::Widget>();
            let segments = widgets_with_class(&card, "recording-editor-export-segment");
            let fhd = segments
                .into_iter()
                .find(|segment| {
                    widgets_with_class(segment, "recording-editor-export-segment-title")
                        .into_iter()
                        .any(|title| {
                            title
                                .downcast_ref::<Label>()
                                .is_some_and(|label| label.text() == "FHD")
                        })
                })
                .expect("the FHD segment exists");
            fhd.downcast::<ToggleButton>().unwrap().emit_clicked();
            let copy =
                find_button_by_label(&card, &t("Copy to clipboard")).expect("the copy action");
            copy.emit_clicked();
            let (settings, action) = *captured.borrow().as_ref().expect("the action ran");
            (settings.resolution, action)
        }) else {
            return;
        };
        assert_eq!(resolution, Some(1080));
        assert_eq!(action, ExportAction::CopyToClipboard);
    }

    #[test]
    fn the_close_button_releases_the_flow() {
        let Some((active_after, sensitive_after)) = crate::test_support::with_gtk(|| {
            use gtk4::prelude::*;
            let (_app, window, root) = editor_window_with_root();
            let active: export_flow::FlowActive = Rc::new(Cell::new(false));
            let flow = export_flow::ActiveFlow::begin(&window, &active).unwrap();
            let card = super::show(&window, blank_state(), flow, Rc::new(|_, _| {}))
                .upcast::<gtk4::Widget>();
            let close = find_button_by_icon(&card, "window-close-symbolic")
                .expect("the sheet must expose a Close button");
            close.emit_clicked();
            (active.get(), root.is_sensitive())
        }) else {
            return;
        };
        assert!(!active_after);
        assert!(sensitive_after);
    }

    #[test]
    fn each_segmented_track_offers_four_choices() {
        assert_eq!(RESOLUTION_CHOICES.len(), 4);
        assert_eq!(FPS_CHOICES.len(), 4);
        assert_eq!(QUALITY_CHOICES.len(), 4);
        let Some(count) = crate::test_support::with_gtk(|| {
            segmented(
                &[
                    ("A".to_string(), String::new()),
                    ("B".to_string(), String::new()),
                    ("C".to_string(), String::new()),
                    ("D".to_string(), String::new()),
                ],
                0,
            )
            .1
            .len()
        }) else {
            return;
        };
        assert_eq!(count, 4);
    }

    #[test]
    fn codec_availability_respects_the_requested_policy() {
        let all = CodecAvailability {
            h264_software: true,
            h264_auto: true,
            hevc_software: false,
            hevc_auto: true,
        };
        assert!(all.allows(Codec::H264, HardwarePreference::Software));
        assert!(!all.allows(Codec::Hevc, HardwarePreference::Software));
        assert!(all.allows(Codec::Hevc, HardwarePreference::Auto));
        assert!(!all.allows(Codec::Hevc, HardwarePreference::Environment));
    }

    fn pump_events() {
        let context = glib::MainContext::default();
        while context.pending() {
            context.iteration(false);
        }
    }

    fn fully_inside(widget: &gtk4::Widget, ancestor: &gtk4::Widget) -> bool {
        let Some(bounds) = widget.compute_bounds(ancestor) else {
            return false;
        };
        let (x, y) = (bounds.x(), bounds.y());
        let (w, h) = (bounds.width(), bounds.height());
        x >= 0.0
            && y >= 0.0
            && x + w <= ancestor.width() as f32
            && y + h <= ancestor.height() as f32
    }

    #[test]
    fn the_card_holds_focus_while_availability_is_pending() {
        let Some(focused_inside) = crate::test_support::with_gtk(|| {
            use gtk4::prelude::*;
            let (_app, window, _root) = editor_window_with_root();
            let active: export_flow::FlowActive = Rc::new(Cell::new(false));
            let flow = export_flow::ActiveFlow::begin(&window, &active).unwrap();
            let card = super::show(&window, blank_state(), flow, Rc::new(|_, _| {}))
                .upcast::<gtk4::Widget>();
            window.present();
            for _ in 0..10 {
                pump_events();
            }
            let focus = gtk4::prelude::GtkWindowExt::focus(&window);
            focus.is_some_and(|widget| {
                let mut current = Some(widget);
                while let Some(w) = current {
                    if w == card {
                        return true;
                    }
                    current = w.parent();
                }
                false
            })
        }) else {
            return;
        };
        assert!(
            focused_inside,
            "focus must land inside the card so Escape works"
        );
    }

    #[test]
    fn the_card_stays_centered_while_the_window_resizes() {
        let Some(()) = crate::test_support::with_gtk(|| {
            use gtk4::prelude::*;
            crate::recording::editor::ui_support::install_recording_editor_css();
            let (_app, window, _root) = editor_window_with_root();
            let shell = window.child().unwrap();
            window.set_default_size(1400, 900);
            let active: export_flow::FlowActive = Rc::new(Cell::new(false));
            let flow = export_flow::ActiveFlow::begin(&window, &active).unwrap();
            let card = super::show(&window, blank_state(), flow, Rc::new(|_, _| {}))
                .upcast::<gtk4::Widget>();
            window.present();
            for (width, height) in [(1400, 900), (1000, 700), (1920, 1080)] {
                window.set_default_size(width, height);
                for _ in 0..20 {
                    pump_events();
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                let card_bounds = card
                    .compute_bounds(&shell)
                    .expect("the card must stay inside the shell");
                let expected_x = (shell.width() as f32 - card_bounds.width()) / 2.0;
                let expected_y = (shell.height() as f32 - card_bounds.height()) / 2.0;
                assert!(
                    (card_bounds.x() - expected_x).abs() <= 2.0
                        && (card_bounds.y() - expected_y).abs() <= 2.0,
                    "the card must be centered in the shell at {width}x{height}: got {card_bounds:?}, expected center ({expected_x}, {expected_y})"
                );
                assert!(
                    fully_inside(&card, &shell),
                    "the card must fit the window at {width}x{height}"
                );
                let close = find_button_by_icon(&card, "window-close-symbolic")
                    .expect("the close button exists");
                assert!(
                    fully_inside(&close.upcast(), &shell),
                    "close must stay inside the window at {width}x{height}"
                );
                let actions: Vec<Button> =
                    widgets_with_class(&card, "recording-editor-export-action")
                        .into_iter()
                        .filter_map(|widget| widget.downcast::<Button>().ok())
                        .collect();
                assert_eq!(actions.len(), 2);
                for action in &actions {
                    assert!(
                        fully_inside(&action.clone().upcast(), &shell),
                        "action {:?} must fit inside the window at {width}x{height}",
                        action.label()
                    );
                }
                let segments = widgets_with_class(&card, "recording-editor-export-segment");
                assert_eq!(segments.len(), 4 + 4 + 2 + 4);
                for segment in &segments {
                    assert!(
                        fully_inside(segment, &card),
                        "every segment must fit inside the card at {width}x{height}"
                    );
                }
                let switch = widgets_with_class(&card, "recording-editor-export-switch")
                    .pop()
                    .expect("the hardware switch exists");
                assert!(
                    fully_inside(&switch, &card),
                    "the switch must fit inside the card at {width}x{height}"
                );
                let summary_rows =
                    widgets_with_class(&card, "recording-editor-export-summary-value");
                assert_eq!(summary_rows.len(), 4);
            }
        }) else {
            return;
        };
    }
}
