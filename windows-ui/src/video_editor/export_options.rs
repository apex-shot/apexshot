//! MP4 export choices, shown before asking for an output path.

use super::export::{ExportModel, ExportSettings};
use crate::capture::editor::window::icon_names::custom;
use crate::i18n::t;
use crate::settings::select::SettingsSelect;
use crate::settings::ui_support::{window_control_button, WindowControl};
use gtk4::prelude::*;
use gtk4::{
    gdk, Align, Box as GtkBox, Button, CheckButton, Grid, Image, Label, Orientation, Overlay,
    ToggleButton,
};
use std::cell::Cell;
use std::rc::Rc;

const QUALITIES: [(u32, &str, &str); 4] = [
    (
        16,
        "Studio",
        "Highest quality, best for further editing. Produces larger files.",
    ),
    (
        20,
        "Social Media",
        "High quality with a smaller file, ready to share.",
    ),
    (
        24,
        "Web",
        "A balanced file size for websites and everyday sharing.",
    ),
    (
        28,
        "Web (Low)",
        "The smallest file, with more visible compression.",
    ),
];

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
    let quality = Rc::new(Cell::new(
        (0..QUALITIES.len())
            .min_by_key(|index| QUALITIES[*index].0.abs_diff(model.crf))
            .unwrap_or(1),
    ));
    let scrim = GtkBox::new(Orientation::Vertical, 0);
    scrim.add_css_class("video-export-scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    let card = GtkBox::new(Orientation::Vertical, px(24.0));
    card.add_css_class("video-export-options");
    card.set_halign(Align::Center);
    card.set_valign(Align::Center);
    card.set_width_request(px(560.0));

    let header = GtkBox::new(Orientation::Horizontal, px(12.0));
    let icon = Image::from_icon_name(themed_icon(custom::VIDEO, custom::VIDEO_LIGHT));
    icon.set_pixel_size(px(24.0));
    icon.set_valign(Align::Center);
    let heading = GtkBox::new(Orientation::Vertical, px(4.0));
    heading.set_hexpand(true);
    let title = Label::new(Some(&t("Export video")));
    title.add_css_class("video-export-dialog-title");
    title.set_xalign(0.0);
    let format = Label::new(Some("MP4 · H.264"));
    format.add_css_class("video-export-muted");
    format.set_xalign(0.0);
    heading.append(&title);
    heading.append(&format);
    let close = window_control_button(WindowControl::Close, &t("Close export options"), scale);
    close.remove_css_class("caption-btn");
    close.remove_css_class("caption-btn-close");
    close.add_css_class("video-export-close");
    close.set_focusable(true);
    close.set_size_request(px(30.0), px(30.0));
    close.set_valign(Align::Start);
    header.append(&icon);
    header.append(&heading);
    header.append(&close);

    let fields = Grid::new();
    fields.set_column_homogeneous(true);
    fields.set_column_spacing(px(24.0) as u32);
    let size_group = group(
        "Output size",
        themed_icon(custom::PANELS_TOP_LEFT, custom::PANELS_TOP_LEFT_LIGHT),
        scale,
    );
    let sizes = SettingsSelect::new(
        [
            ("original", t("Original")),
            ("720", "720p".into()),
            ("1080", "1080p".into()),
            ("2160", "4K".into()),
        ],
        "original",
    );
    sizes.widget().set_hexpand(true);
    sizes.connect_changed({
        let sizes = sizes.clone();
        let settings = settings.clone();
        move || {
            let resolution = sizes.active_id().and_then(|id| id.parse().ok());
            settings.set(ExportSettings {
                resolution,
                ..settings.get()
            });
        }
    });
    let dimensions = Label::new(None);
    dimensions.add_css_class("video-export-muted");
    dimensions.set_xalign(0.0);
    size_group.append(sizes.widget());
    size_group.append(&dimensions);

    let fps_group = group(
        "Frame rate",
        themed_icon(custom::CLAPPERBOARD, custom::CLAPPERBOARD_LIGHT),
        scale,
    );
    let source_fps = crate::recording::editor::ffmpeg::probe_frame_rate(&model.source)
        .filter(|fps| fps.is_finite() && *fps > 0.0)
        .unwrap_or(30.0);
    let source_label = format!("{} ({source_fps:.2} FPS)", t("Original"));
    let fps = SettingsSelect::new(
        [
            ("original", source_label.as_str()),
            ("24", "24 FPS"),
            ("30", "30 FPS"),
            ("60", "60 FPS"),
        ],
        "original",
    );
    fps.widget().set_hexpand(true);
    fps.connect_changed({
        let fps = fps.clone();
        let settings = settings.clone();
        move || {
            let value = fps.active_id().and_then(|id| id.parse().ok());
            settings.set(ExportSettings {
                fps: value,
                ..settings.get()
            });
        }
    });
    fps_group.append(fps.widget());
    fields.attach(&size_group, 0, 0, 1, 1);
    fields.attach(&fps_group, 1, 0, 1, 1);

    let quality_group = group(
        "Quality",
        themed_icon(custom::SLIDERS_HORIZONTAL, custom::SLIDERS_HORIZONTAL_LIGHT),
        scale,
    );
    let presets = GtkBox::new(Orientation::Horizontal, px(6.0));
    presets.add_css_class("video-export-presets");
    presets.set_homogeneous(true);
    let description = Label::new(None);
    description.add_css_class("video-export-muted");
    description.set_xalign(0.0);
    description.set_yalign(0.0);
    description.set_wrap(true);
    description.set_height_request(px(28.0));
    description.set_max_width_chars(70);
    let mut first_quality = None;
    for (index, (_, name, _)) in QUALITIES.iter().enumerate() {
        let button = ToggleButton::with_label(&t(name));
        button.add_css_class("video-export-preset");
        if let Some(first) = &first_quality {
            button.set_group(Some(first));
        } else {
            first_quality = Some(button.clone());
        }
        button.set_active(index == quality.get());
        button.connect_toggled({
            let quality = quality.clone();
            move |button| {
                if button.is_active() {
                    quality.set(index);
                }
            }
        });
        presets.append(&button);
    }
    quality_group.append(&presets);
    quality_group.append(&description);

    let hardware = CheckButton::with_label(&t("Hardware acceleration"));
    hardware.set_active(settings.get().hardware);
    hardware.set_tooltip_text(Some(&t(
        "Use the GPU encoder when one is available. Falls back to the CPU automatically.",
    )));
    hardware.connect_toggled({
        let settings = settings.clone();
        move |button| {
            settings.set(ExportSettings {
                hardware: button.is_active(),
                ..settings.get()
            });
        }
    });
    quality_group.append(&hardware);

    let footer = GtkBox::new(Orientation::Vertical, px(16.0));
    let summary = GtkBox::new(Orientation::Vertical, px(4.0));
    let estimate = Label::new(None);
    estimate.add_css_class("video-export-muted");
    estimate.set_xalign(0.0);
    let timing = Label::new(Some(&t(
        "Export time depends on your hardware and effects.",
    )));
    timing.add_css_class("video-export-muted");
    timing.set_xalign(0.0);
    timing.set_wrap(true);
    summary.append(&estimate);
    summary.append(&timing);
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
    footer.append(&summary);
    footer.append(&actions);
    card.append(&header);
    card.append(&fields);
    card.append(&quality_group);
    card.append(&footer);

    let source_size = model.source_size;
    let source_bytes = std::fs::metadata(&model.source)
        .map(|m| m.len())
        .unwrap_or(0);
    let source_seconds =
        crate::recording::editor::ffmpeg::probe_duration(&model.source).unwrap_or(0.0);
    let seconds = (model.trim_end - model.trim_start).max(0.0);
    let update: Rc<dyn Fn()> = Rc::new({
        let settings = settings.clone();
        let quality = quality.clone();
        move || {
            let selected = settings.get();
            let (width, height) = selected.dimensions(source_size);
            dimensions.set_text(&format!("{width} × {height} px"));
            description.set_text(&t(QUALITIES[quality.get()].2));
            let size = estimate_bytes(
                source_bytes,
                source_seconds,
                seconds,
                source_size,
                selected,
                QUALITIES[quality.get()].0,
            );
            estimate.set_text(&format!(
                "{} — {:.1} MB",
                t("Estimated output size"),
                size / 1_000_000.0
            ));
        }
    });
    update();
    let mut child = presets.first_child();
    while let Some(widget) = child {
        if let Some(button) = widget.downcast_ref::<ToggleButton>() {
            let update = update.clone();
            button.connect_toggled(move |_| update());
        }
        child = widget.next_sibling();
    }
    sizes.connect_changed({
        let update = update.clone();
        move || update()
    });
    fps.connect_changed(move || update());

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
                then(settings.get(), QUALITIES[quality.get()].0, clipboard);
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

fn group(title: &str, icon: &str, scale: f64) -> GtkBox {
    let group = GtkBox::new(Orientation::Vertical, (8.0 * scale).round() as i32);
    group.set_valign(Align::Start);
    let heading = GtkBox::new(Orientation::Horizontal, (7.0 * scale).round() as i32);
    let image = Image::from_icon_name(icon);
    image.set_pixel_size((16.0 * scale).round() as i32);
    let label = Label::new(Some(&t(title)));
    label.add_css_class("video-export-heading");
    heading.append(&image);
    heading.append(&label);
    group.append(&heading);
    group
}

fn estimate_bytes(
    bytes: u64,
    source_seconds: f64,
    seconds: f64,
    source: (u32, u32),
    settings: ExportSettings,
    crf: u32,
) -> f64 {
    let (width, height) = settings.dimensions(source);
    let pixels = f64::from(source.0) * f64::from(source.1);
    if source_seconds <= 0.0 || pixels <= 0.0 {
        return 0.0;
    }
    bytes as f64
        * (seconds / source_seconds)
        * (f64::from(width) * f64::from(height) / pixels)
        * 2.0_f64.powf((23.0 - f64::from(crf)) / 6.0)
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
    fn settings_pickers_forward_the_choices_to_both_export_actions() {
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
                    zooms: Vec::new(),
                    zoom_camera: super::super::zoom::ZoomCameraSettings::default(),
                    cursor: crate::recording::editor::model::CursorSettings::default(),
                    sidecar: None,
                    webcam: None,
                    crf: 20,
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
                let pickers = widgets_with_class(shell.upcast_ref(), "settings-select");
                assert_eq!(pickers.len(), 2);
                for picker in &pickers {
                    let options = widgets_with_class(picker, "history-action-btn");
                    assert_eq!(options.len(), 4);
                    options[1]
                        .clone()
                        .downcast::<Button>()
                        .unwrap()
                        .emit_clicked();
                }
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
                assert_eq!(settings.fps, Some(24));
                assert_eq!(crf, 20);
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

    #[test]
    fn size_estimate_follows_duration_resolution_and_quality() {
        let settings = ExportSettings::default();
        let estimate = |seconds, settings, crf| {
            estimate_bytes(10_000_000, 10.0, seconds, (1920, 1080), settings, crf)
        };
        assert_eq!(estimate(5.0, settings, 23), 5_000_000.0);
        assert!(estimate(5.0, settings, 16) > estimate(5.0, settings, 28));
        assert!(
            estimate(
                5.0,
                ExportSettings {
                    resolution: Some(720),
                    ..settings
                },
                23
            ) < estimate(5.0, settings, 23)
        );
        assert_eq!(estimate_bytes(0, 0.0, 10.0, (0, 0), settings, 23), 0.0);
    }
}
