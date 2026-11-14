//! MP4 export choices, shown before asking for an output path.

use super::export::rate;
use super::export::{Codec, ExportModel, ExportSettings};
use crate::capture::editor::window::icon_names::custom;
use crate::i18n::t;
use crate::settings::ui_support::{window_control_button, WindowControl};
use gtk4::prelude::*;
use gtk4::{
    gdk, Align, Box as GtkBox, Button, Image, Label, Orientation, Overlay, Switch, ToggleButton,
};
use std::cell::Cell;
use std::rc::Rc;

const QUALITIES: [(u32, &str, &str); 4] = [
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

/// Short-edge targets; `None` keeps the frame's own size.
const RESOLUTIONS: [(Option<u32>, &str, &str); 4] = [
    (None, "Original", ""),
    (Some(720), "HD", "720p"),
    (Some(1080), "FHD", "1080p"),
    (Some(2160), "4K", "2160p"),
];

const FRAME_RATES: [Option<u32>; 4] = [None, Some(60), Some(30), Some(24)];

pub(super) fn show(
    shell: &Overlay,
    model: &ExportModel,
    then: Rc<dyn Fn(ExportSettings, u32, bool)>,
) {
    let scale = crate::platform::ui_scale();
    let px = |n: f64| (n * scale).round() as i32;
    let dark = crate::settings::windowing::prefers_dark_glass_theme();
    let themed_icon = |dark_icon, light_icon| if dark { dark_icon } else { light_icon };
    let settings = Rc::new(Cell::new(ExportSettings::default()));
    let project_crf = model.crf;
    let initial_quality = (0..QUALITIES.len())
        .min_by_key(|index| QUALITIES[*index].0.abs_diff(project_crf))
        .unwrap_or(1);
    let quality = Rc::new(Cell::new(initial_quality));
    let scrim = GtkBox::new(Orientation::Vertical, 0);
    scrim.add_css_class("video-export-scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    let card = GtkBox::new(Orientation::Vertical, px(20.0));
    card.add_css_class("video-export-options");
    card.set_halign(Align::Center);
    card.set_valign(Align::Center);
    card.set_width_request(px(580.0));

    let header = GtkBox::new(Orientation::Horizontal, px(12.0));
    let icon = Image::from_icon_name(themed_icon(custom::VIDEO, custom::VIDEO_LIGHT));
    icon.set_pixel_size(px(24.0));
    icon.set_valign(Align::Center);
    let title = Label::new(Some(&t("Export video")));
    title.add_css_class("video-export-dialog-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_valign(Align::Center);
    let close = window_control_button(WindowControl::Close, &t("Close export options"), scale);
    close.remove_css_class("caption-btn");
    close.remove_css_class("caption-btn-close");
    close.add_css_class("video-export-close");
    close.set_focusable(true);
    close.set_size_request(px(30.0), px(30.0));
    close.set_valign(Align::Center);
    header.append(&icon);
    header.append(&title);
    header.append(&close);

    let base = model
        .stage
        .filter(|_| model.webcam.is_some())
        .unwrap_or(model.source_size);
    let frame_size = model.canvas.as_ref().map_or(base, |canvas| canvas.size);

    let original_edge = frame_size.0.min(frame_size.1);
    let resolution_options: Vec<(String, String)> = RESOLUTIONS
        .iter()
        .map(|(edge, title, detail)| match edge {
            None => (t(title), format!("{original_edge}p")),
            Some(_) => ((*title).to_string(), (*detail).to_string()),
        })
        .collect();
    let (resolution_track, resolution_buttons) = segmented(&resolution_options, 0, scale);
    for (button, (edge, _, _)) in resolution_buttons.iter().zip(RESOLUTIONS) {
        let settings = settings.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                settings.set(ExportSettings {
                    resolution: edge,
                    ..settings.get()
                });
            }
        });
    }
    let resolution_section = section("Resolution", scale);
    resolution_section.append(&resolution_track);

    let source_fps = crate::recording::editor::ffmpeg::probe_frame_rate(&model.source)
        .filter(|fps| fps.is_finite() && *fps > 0.0)
        .unwrap_or(30.0);
    let fps_options: Vec<(String, String)> = FRAME_RATES
        .iter()
        .map(|rate| {
            (
                rate.map_or_else(|| t("Original"), |fps| fps.to_string()),
                String::new(),
            )
        })
        .collect();
    let (fps_track, fps_buttons) = segmented(&fps_options, 0, scale);
    fps_buttons[0].set_tooltip_text(Some(&fps_text(source_fps)));
    for (button, rate) in fps_buttons.iter().zip(FRAME_RATES) {
        let settings = settings.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                settings.set(ExportSettings {
                    fps: rate,
                    ..settings.get()
                });
            }
        });
    }
    let fps_section = section("Frame rate", scale);
    fps_section.set_hexpand(true);
    fps_section.append(&fps_track);

    let codec_options = [
        ("H.264".to_string(), String::new()),
        ("HEVC".to_string(), String::new()),
    ];
    let (codec_track, codec_buttons) = segmented(&codec_options, 0, scale);
    for (button, codec) in codec_buttons.iter().zip([Codec::H264, Codec::Hevc]) {
        let settings = settings.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                settings.set(ExportSettings {
                    codec,
                    ..settings.get()
                });
            }
        });
    }
    let codec_section = section("Codec", scale);
    codec_section.append(&codec_track);
    let pickers_row = GtkBox::new(Orientation::Horizontal, px(16.0));
    pickers_row.append(&fps_section);
    pickers_row.append(&codec_section);
    let codec_note = Label::new(Some(&t(
        "Smaller files at the same quality, but not every app or browser plays HEVC.",
    )));
    codec_note.add_css_class("video-export-muted");
    codec_note.set_xalign(0.0);
    codec_note.set_wrap(true);
    codec_note.set_visible(false);
    let pickers = GtkBox::new(Orientation::Vertical, px(8.0));
    pickers.append(&pickers_row);
    pickers.append(&codec_note);

    let quality_options: Vec<(String, String)> = QUALITIES
        .iter()
        .map(|(_, name, _)| (t(name), String::new()))
        .collect();
    let (quality_track, quality_buttons) = segmented(&quality_options, quality.get(), scale);
    for (index, button) in quality_buttons.iter().enumerate() {
        let quality = quality.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                quality.set(index);
            }
        });
    }
    let description = Label::new(None);
    description.add_css_class("video-export-muted");
    description.set_xalign(0.0);
    description.set_wrap(true);
    description.set_max_width_chars(70);
    let quality_section = section("Quality", scale);
    quality_section.append(&quality_track);
    quality_section.append(&description);

    let hardware_card = GtkBox::new(Orientation::Horizontal, px(14.0));
    hardware_card.add_css_class("video-export-toggle-card");
    hardware_card.set_tooltip_text(Some(&t(
        "Hardware encoding uses a bitrate, so file sizes follow the estimate; the software encoder uses a constant quality.",
    )));
    let hardware_text = GtkBox::new(Orientation::Vertical, px(3.0));
    hardware_text.set_hexpand(true);
    hardware_text.set_valign(Align::Center);
    let hardware_title = Label::new(Some(&t("Hardware acceleration")));
    hardware_title.add_css_class("video-export-toggle-title");
    hardware_title.set_xalign(0.0);
    let hardware_note = Label::new(Some(&t(
        "Use the GPU encoder when one is available. Falls back to the CPU automatically.",
    )));
    hardware_note.add_css_class("video-export-muted");
    hardware_note.set_xalign(0.0);
    hardware_note.set_wrap(true);
    hardware_text.append(&hardware_title);
    hardware_text.append(&hardware_note);
    let hardware = Switch::new();
    hardware.add_css_class("video-export-switch");
    hardware.set_valign(Align::Center);
    hardware.set_active(settings.get().hardware);
    hardware.update_property(&[gtk4::accessible::Property::Label(&t(
        "Hardware acceleration",
    ))]);
    hardware.connect_active_notify({
        let settings = settings.clone();
        move |switch| {
            settings.set(ExportSettings {
                hardware: switch.is_active(),
                ..settings.get()
            });
        }
    });
    hardware_card.append(&hardware_text);
    hardware_card.append(&hardware);

    let summary = GtkBox::new(Orientation::Vertical, px(8.0));
    summary.add_css_class("video-export-summary");
    let output_value = summary_row(&summary, &t("Output"), scale);
    let format_value = summary_row(&summary, &t("Format"), scale);
    let size_value = summary_row(&summary, &t("Estimated size"), scale);
    let time_value = summary_row(&summary, &t("Export time"), scale);

    let actions = GtkBox::new(Orientation::Horizontal, px(16.0));
    let save = Button::with_label(&t("Export to file"));
    save.add_css_class("video-export-action");
    save.add_css_class("video-export-primary");
    save.set_width_request(px(170.0));
    let copy = Button::with_label(&t("Copy to clipboard"));
    copy.add_css_class("video-export-action");
    copy.set_width_request(px(170.0));
    copy.set_tooltip_text(Some(&t(
        "Export a file and copy it to the clipboard for pasting into other apps.",
    )));
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    actions.append(&copy);
    actions.append(&spacer);
    actions.append(&save);
    card.append(&header);
    card.append(&resolution_section);
    card.append(&pickers);
    card.append(&quality_section);
    card.append(&hardware_card);
    card.append(&summary);
    card.append(&actions);

    // The estimate must describe the export's real length: cuts, speeds, moved
    // pieces, and the freeze tail all change it.
    let trim = (model.trim_start, model.trim_end);
    let cuts = model.cuts.clone();
    let speeds = model.segment_speeds.clone();
    let starts = model.segment_starts.clone();
    let freeze_tail = model.freeze_tail;
    let update: Rc<dyn Fn()> = Rc::new({
        let settings = settings.clone();
        let quality = quality.clone();
        move || {
            let selected = settings.get();
            let output = selected.dimensions(frame_size);
            let fps = selected.fps.map_or(source_fps, f64::from);
            output_value.set_text(&format!("{} × {} · {}", output.0, output.1, fps_text(fps)));
            format_value.set_text(match selected.codec {
                Codec::H264 => "MP4 · H.264",
                Codec::Hevc => "MP4 · HEVC",
            });
            codec_note.set_visible(selected.codec == Codec::Hevc);
            description.set_text(&t(QUALITIES[quality.get()].2));
            let crf = effective_crf(project_crf, initial_quality, quality.get());
            let spans =
                super::export::frame_plan_with_starts(trim.0, trim.1, &cuts, &speeds, &starts, fps);
            let mut frames = super::export::total_frames(&spans);
            if !spans.is_empty() && freeze_tail.is_finite() && freeze_tail > 0.0 {
                frames += (freeze_tail * fps).round() as u64;
            }
            let seconds = frames as f64 / fps;
            let (bps, spread) = match selected.codec {
                Codec::H264 => (
                    rate::h264_bits_per_second(crf, output, fps),
                    rate::H264_SPREAD,
                ),
                Codec::Hevc => (
                    rate::hevc_estimate_bits_per_second(crf, output, fps),
                    rate::BITRATE_SPREAD,
                ),
            };
            let (low, _, high) = rate::bytes(bps, rate::MP4_AUDIO_BPS, seconds, spread);
            let size = if high / low.max(1.0) < 1.5 {
                rate::format_bytes((low + high) / 2.0)
            } else {
                format!("{} – {}", rate::format_bytes(low), rate::format_bytes(high))
            };
            size_value.set_text(&size);
            time_value.set_text(&format!(
                "≈ {}",
                rate::format_duration(rate::export_seconds(output, fps, seconds))
            ));
        }
    });
    update();
    for button in resolution_buttons
        .iter()
        .chain(&fps_buttons)
        .chain(&codec_buttons)
        .chain(&quality_buttons)
    {
        let update = update.clone();
        button.connect_toggled(move |_| update());
    }

    if let Some(editor) = shell.child() {
        editor.set_sensitive(false);
    }
    shell.add_overlay(&scrim);
    shell.add_overlay(&card);
    let dismiss: Rc<dyn Fn()> = Rc::new({
        let shell = shell.downgrade();
        let scrim = scrim.downgrade();
        let card = card.downgrade();
        move || {
            if let (Some(shell), Some(scrim), Some(card)) =
                (shell.upgrade(), scrim.upgrade(), card.upgrade())
            {
                shell.remove_overlay(&card);
                shell.remove_overlay(&scrim);
                if let Some(editor) = shell.child() {
                    editor.set_sensitive(true);
                }
            }
        }
    });
    close.connect_clicked({
        let dismiss = dismiss.clone();
        move |_| dismiss()
    });
    for (button, clipboard) in [(&save, false), (&copy, true)] {
        button.connect_clicked({
            let dismiss = dismiss.clone();
            let then = then.clone();
            let settings = settings.clone();
            let quality = quality.clone();
            move |_| {
                dismiss();
                let crf = effective_crf(project_crf, initial_quality, quality.get());
                then(settings.get(), crf, clipboard);
            }
        });
    }
    let keys = gtk4::EventControllerKey::new();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            dismiss();
            gtk4::glib::Propagation::Stop
        } else {
            gtk4::glib::Propagation::Proceed
        }
    });
    card.add_controller(keys);
    save.grab_focus();
}

fn section(title: &str, scale: f64) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, (8.0 * scale).round() as i32);
    section.set_valign(Align::Start);
    let label = Label::new(Some(&t(title).to_uppercase()));
    label.add_css_class("video-export-eyebrow");
    label.set_xalign(0.0);
    section.append(&label);
    section
}

/// One row of mutually exclusive choices; a non-empty detail adds a second,
/// smaller line under the title.
fn segmented(
    options: &[(String, String)],
    active: usize,
    scale: f64,
) -> (GtkBox, Vec<ToggleButton>) {
    let track = GtkBox::new(Orientation::Horizontal, (4.0 * scale).round() as i32);
    track.add_css_class("video-export-segments");
    track.set_homogeneous(true);
    let mut buttons: Vec<ToggleButton> = Vec::with_capacity(options.len());
    for (index, (title, detail)) in options.iter().enumerate() {
        let button = ToggleButton::new();
        button.add_css_class("video-export-segment");
        if detail.is_empty() {
            button.set_label(title);
        } else {
            button.add_css_class("video-export-segment-tall");
            let lines = GtkBox::new(Orientation::Vertical, (1.0 * scale).round() as i32);
            lines.set_halign(Align::Center);
            lines.set_valign(Align::Center);
            let heading = Label::new(Some(title));
            heading.add_css_class("video-export-segment-title");
            let sub = Label::new(Some(detail));
            sub.add_css_class("video-export-segment-sub");
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

fn summary_row(summary: &GtkBox, label: &str, scale: f64) -> Label {
    let row = GtkBox::new(Orientation::Horizontal, (12.0 * scale).round() as i32);
    let name = Label::new(Some(label));
    name.add_css_class("video-export-summary-label");
    name.set_xalign(0.0);
    name.set_hexpand(true);
    let value = Label::new(None);
    value.add_css_class("video-export-summary-value");
    value.set_xalign(1.0);
    row.append(&name);
    row.append(&value);
    summary.append(&row);
    value
}

fn fps_text(fps: f64) -> String {
    if (fps - fps.round()).abs() < 0.01 {
        format!("{} FPS", fps.round() as u32)
    } else {
        format!("{fps:.2} FPS")
    }
}

/// The CRF an export writes: the project's own value while the quality segment
/// still shows its tier, so opening the dialog never changes a project's
/// quality; a tier the user picked uses that tier's CRF.
fn effective_crf(project: u32, initial: usize, chosen: usize) -> u32 {
    if chosen == initial {
        project
    } else {
        QUALITIES[chosen.min(QUALITIES.len() - 1)].0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn an_unchanged_quality_segment_keeps_the_projects_own_crf() {
        let initial = 1;
        assert_eq!(effective_crf(20, initial, initial), 20);
        assert_eq!(effective_crf(23, initial, initial), 23);
        assert_eq!(effective_crf(20, initial, 0), QUALITIES[0].0);
        assert_eq!(effective_crf(20, initial, 2), QUALITIES[2].0);
        assert_eq!(effective_crf(20, initial, 3), QUALITIES[3].0);
    }

    #[test]
    fn segmented_choices_forward_the_settings_to_both_export_actions() {
        let Some(()) = crate::test_support::with_gtk(|| {
            for clipboard in [false, true] {
                let shell = Overlay::new();
                let editor = GtkBox::new(Orientation::Vertical, 0);
                shell.set_child(Some(&editor));
                let model = ExportModel {
                    source: std::env::temp_dir().join("apexshot-export-options-missing.mp4"),
                    source_size: (1920, 1080),
                    trim_start: 0.0,
                    trim_end: 10.0,
                    cuts: Vec::new(),
                    segment_starts: Vec::new(),
                    segment_speeds: Vec::new(),
                    freeze_tail: 0.0,
                    zooms: Vec::new(),
                    zoom_camera: super::super::zoom::ZoomCameraSettings::default(),
                    cursor: crate::recording::editor::model::CursorSettings::default(),
                    sidecar: None,
                    webcam: None,
                    stage: None,
                    canvas: None,
                    crf: 20,
                    audio: Vec::new(),
                    effects: Default::default(),
                    cursor_hide_clips: Vec::new(),
                };
                let chosen = Rc::new(Cell::new(None));
                show(
                    &shell,
                    &model,
                    Rc::new({
                        let chosen = chosen.clone();
                        move |settings, crf, clipboard| chosen.set(Some((settings, crf, clipboard)))
                    }),
                );
                assert!(!editor.is_sensitive());
                let tracks = widgets_with_class(shell.upcast_ref(), "video-export-segments");
                assert_eq!(tracks.len(), 4);
                for (track, expected) in tracks.iter().zip([4, 4, 2, 4]) {
                    let segments = widgets_with_class(track, "video-export-segment");
                    assert_eq!(segments.len(), expected);
                    segments[1]
                        .clone()
                        .downcast::<ToggleButton>()
                        .unwrap()
                        .set_active(true);
                }
                let names: Vec<String> = widgets_with_class(&tracks[3], "video-export-segment")
                    .into_iter()
                    .map(|segment| {
                        segment
                            .downcast::<ToggleButton>()
                            .unwrap()
                            .label()
                            .unwrap_or_default()
                            .to_string()
                    })
                    .collect();
                assert_eq!(names, QUALITIES.map(|(_, name, _)| t(name)));
                let values: Vec<String> =
                    widgets_with_class(shell.upcast_ref(), "video-export-summary-value")
                        .into_iter()
                        .map(|value| value.downcast::<Label>().unwrap().text().to_string())
                        .collect();
                assert_eq!(values.len(), 4);
                assert_eq!(values[0], "1280 × 720 · 60 FPS");
                assert_eq!(values[1], "MP4 · HEVC");
                let hardware = widgets_with_class(shell.upcast_ref(), "video-export-switch")
                    .pop()
                    .unwrap()
                    .downcast::<Switch>()
                    .unwrap();
                assert!(hardware.is_active());
                hardware.set_active(false);
                let actions = widgets_with_class(shell.upcast_ref(), "video-export-action");
                assert_eq!(actions.len(), 2);
                assert!(!actions[0].has_css_class("video-export-primary"));
                assert!(actions[1].has_css_class("video-export-primary"));
                assert!(actions[0]
                    .next_sibling()
                    .is_some_and(|spacer| spacer.hexpands()));
                actions[usize::from(!clipboard)]
                    .clone()
                    .downcast::<Button>()
                    .unwrap()
                    .emit_clicked();
                let (settings, crf, copied) = chosen.get().expect("the export action must run");
                assert_eq!(settings.resolution, Some(720));
                assert_eq!(settings.fps, Some(60));
                assert_eq!(settings.codec, Codec::Hevc);
                assert!(!settings.hardware);
                // The quality segment still shows the project's own tier, so
                // its CRF is the one the export keeps.
                assert_eq!(crf, model.crf);
                assert_eq!(copied, clipboard);
                assert!(editor.is_sensitive());

                show(
                    &shell,
                    &model,
                    Rc::new(|_, _, _| panic!("closing must not export")),
                );
                let card = widgets_with_class(shell.upcast_ref(), "video-export-options")
                    .pop()
                    .unwrap();
                let close = widgets_with_class(&card, "video-export-close")
                    .pop()
                    .unwrap();
                assert_eq!(
                    card.first_child().unwrap().last_child(),
                    Some(close.clone())
                );
                close.downcast::<Button>().unwrap().emit_clicked();
                assert!(editor.is_sensitive());
                assert!(widgets_with_class(shell.upcast_ref(), "video-export-options").is_empty());
            }
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
