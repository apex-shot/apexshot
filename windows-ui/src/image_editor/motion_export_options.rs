//! The Motion export sheet: the video editor's export options laid over the
//! image editor, with the choices a rendered Motion video can use.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, Align, Box as GtkBox, Button, Image, Label, Orientation, Overlay};

use crate::capture::editor::window::icon_names::custom;
use crate::i18n::t;
use crate::settings::ui_support::{window_control_button, WindowControl};
use crate::video_editor::export::rate;
use crate::video_editor::export_options::{
    effective_crf, fps_text, section, segmented, summary_row, QUALITIES, RESOLUTIONS,
};

use super::motion::{MotionExportOptions, MotionMode};

/// Motion renders every frame itself, so there is no source rate to keep.
const FRAME_RATES: [u32; 3] = [60, 30, 24];
/// 30 FPS stays the default the export always had.
const DEFAULT_FPS_INDEX: usize = 1;

/// Names the sheet's overlay so a second request finds the open one.
const SHEET_NAME: &str = "motion-export-sheet";

type OnExport = Box<dyn FnOnce(MotionExportOptions)>;

/// Lay the export sheet over the editor; `on_export` runs with the choices
/// once Export is pressed, and never when the sheet is closed.
pub(super) fn show(
    window: &gtk4::ApplicationWindow,
    motion: &MotionMode,
    on_export: impl FnOnce(MotionExportOptions) + 'static,
) {
    let Some(shell) = window.child().and_downcast::<Overlay>() else {
        on_export(MotionExportOptions::default());
        return;
    };
    // Ctrl+S reaches the window while the sheet is up; one sheet at a time.
    let mut child = shell.first_child();
    while let Some(widget) = child {
        if widget.widget_name() == SHEET_NAME {
            return;
        }
        child = widget.next_sibling();
    }
    crate::video_editor::export_card::install_css();
    let scale = crate::platform::ui_scale();
    let px = |n: f64| (n * scale).round() as i32;
    let dark = crate::settings::windowing::prefers_dark_glass_theme();
    let themed_icon = |dark_icon, light_icon| if dark { dark_icon } else { light_icon };
    let defaults = MotionExportOptions::default();
    let initial_quality = (0..QUALITIES.len())
        .min_by_key(|index| QUALITIES[*index].0.abs_diff(defaults.crf))
        .unwrap_or(0);
    let resolution_index = Rc::new(Cell::new(0usize));
    let fps_index = Rc::new(Cell::new(DEFAULT_FPS_INDEX));
    let quality = Rc::new(Cell::new(initial_quality));

    // Every choice's frame and length are resolved once, up front.
    let dimensions: Vec<(u32, u32)> = RESOLUTIONS
        .iter()
        .map(|(edge, _, _)| {
            motion
                .export_dimensions(*edge)
                .map_or((0, 0), |(width, height)| {
                    (width.max(0) as u32, height.max(0) as u32)
                })
        })
        .collect();
    let seconds: Vec<f64> = FRAME_RATES
        .iter()
        .map(|fps| motion.export_seconds(*fps))
        .collect();

    // The sheet's CSS reads the theme from an ancestor, and the window's root
    // overlay does not inherit the editor root's theme class.
    let host = Overlay::new();
    host.set_widget_name(SHEET_NAME);
    host.add_css_class(if dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    });
    host.set_hexpand(true);
    host.set_vexpand(true);
    let scrim = GtkBox::new(Orientation::Vertical, 0);
    scrim.add_css_class("video-export-scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    host.set_child(Some(&scrim));

    let card = GtkBox::new(Orientation::Vertical, px(20.0));
    card.add_css_class("video-export-options");
    card.set_halign(Align::Center);
    card.set_valign(Align::Center);
    card.set_width_request(px(580.0));
    host.add_overlay(&card);

    let header = GtkBox::new(Orientation::Horizontal, px(12.0));
    let icon = Image::from_icon_name(themed_icon(custom::VIDEO, custom::VIDEO_LIGHT));
    icon.set_pixel_size(px(24.0));
    icon.set_valign(Align::Center);
    let title = Label::new(Some(&t("Export Motion video")));
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

    let original_edge = dimensions[0].0.min(dimensions[0].1);
    let resolution_options: Vec<(String, String)> = RESOLUTIONS
        .iter()
        .map(|(edge, title, detail)| match edge {
            None => (t(title), format!("{original_edge}p")),
            Some(_) => ((*title).to_string(), (*detail).to_string()),
        })
        .collect();
    let (resolution_track, resolution_buttons) = segmented(&resolution_options, 0, scale);
    for (index, button) in resolution_buttons.iter().enumerate() {
        let resolution_index = resolution_index.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                resolution_index.set(index);
            }
        });
    }
    let resolution_note = Label::new(Some(&t(
        "Export cannot recover detail missing from the screenshot.",
    )));
    resolution_note.add_css_class("video-export-muted");
    resolution_note.set_xalign(0.0);
    resolution_note.set_wrap(true);
    let resolution_section = section("Resolution", scale);
    resolution_section.append(&resolution_track);
    resolution_section.append(&resolution_note);

    let fps_options: Vec<(String, String)> = FRAME_RATES
        .iter()
        .map(|fps| (fps.to_string(), String::new()))
        .collect();
    let (fps_track, fps_buttons) = segmented(&fps_options, DEFAULT_FPS_INDEX, scale);
    for (index, button) in fps_buttons.iter().enumerate() {
        let fps_index = fps_index.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                fps_index.set(index);
            }
        });
    }
    let fps_note = Label::new(Some(&t(
        "60 FPS makes animation smoother and takes longer to export.",
    )));
    fps_note.add_css_class("video-export-muted");
    fps_note.set_xalign(0.0);
    fps_note.set_wrap(true);
    let fps_section = section("Frame rate", scale);
    fps_section.append(&fps_track);
    fps_section.append(&fps_note);

    let quality_options: Vec<(String, String)> = QUALITIES
        .iter()
        .map(|(_, name, _)| (t(name), String::new()))
        .collect();
    let (quality_track, quality_buttons) = segmented(&quality_options, initial_quality, scale);
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

    let summary = GtkBox::new(Orientation::Vertical, px(8.0));
    summary.add_css_class("video-export-summary");
    let output_value = summary_row(&summary, &t("Output"), scale);
    let format_value = summary_row(&summary, &t("Format"), scale);
    let size_value = summary_row(&summary, &t("Estimated size"), scale);
    format_value.set_text("MP4 · H.264");

    let actions = GtkBox::new(Orientation::Horizontal, px(16.0));
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let save = Button::with_label(&t("Export to file"));
    save.add_css_class("video-export-action");
    save.add_css_class("video-export-primary");
    save.set_width_request(px(170.0));
    actions.append(&spacer);
    actions.append(&save);

    card.append(&header);
    card.append(&resolution_section);
    card.append(&fps_section);
    card.append(&quality_section);
    card.append(&summary);
    card.append(&actions);

    let update: Rc<dyn Fn()> = Rc::new({
        let resolution_index = resolution_index.clone();
        let fps_index = fps_index.clone();
        let quality = quality.clone();
        move || {
            let output = dimensions[resolution_index.get()];
            let fps = FRAME_RATES[fps_index.get()];
            output_value.set_text(&format!(
                "{} × {} · {}",
                output.0,
                output.1,
                fps_text(f64::from(fps))
            ));
            description.set_text(&t(QUALITIES[quality.get()].2));
            let crf = effective_crf(defaults.crf, initial_quality, quality.get());
            let bps = rate::h264_bits_per_second(crf, output, f64::from(fps));
            let (low, _, high) = rate::bytes(bps, 0.0, seconds[fps_index.get()], rate::H264_SPREAD);
            let size = if high / low.max(1.0) < 1.5 {
                rate::format_bytes((low + high) / 2.0)
            } else {
                format!("{} – {}", rate::format_bytes(low), rate::format_bytes(high))
            };
            size_value.set_text(&size);
        }
    });
    update();
    for button in resolution_buttons
        .iter()
        .chain(&fps_buttons)
        .chain(&quality_buttons)
    {
        let update = update.clone();
        button.connect_toggled(move |_| update());
    }

    if let Some(editor) = shell.child() {
        editor.set_sensitive(false);
    }
    shell.add_overlay(&host);
    let dismiss: Rc<dyn Fn()> = Rc::new({
        let shell = shell.downgrade();
        let host = host.downgrade();
        move || {
            if let (Some(shell), Some(host)) = (shell.upgrade(), host.upgrade()) {
                shell.remove_overlay(&host);
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
    let on_export: Rc<RefCell<Option<OnExport>>> = Rc::new(RefCell::new(Some(Box::new(on_export))));
    save.connect_clicked({
        let dismiss = dismiss.clone();
        move |_| {
            dismiss();
            let options = MotionExportOptions {
                resolution: RESOLUTIONS[resolution_index.get()].0,
                fps: FRAME_RATES[fps_index.get()],
                crf: effective_crf(defaults.crf, initial_quality, quality.get()),
            };
            let callback = on_export.borrow_mut().take();
            if let Some(callback) = callback {
                callback(options);
            }
        }
    });
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
