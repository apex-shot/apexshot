//! The export options dialog the Motion page's Export button opens first:
//! frame size, frame rate and quality for the MP4, then the platform file
//! chooser asks where it lands.

use std::cell::Cell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, ComboBoxText, Label, Orientation};

use crate::i18n::{t, tfmt};

use super::motion::{MotionExportFps, MotionExportQuality, MotionExportSize};
use super::ui_support::px;

/// Ask for the MP4's size, frame rate and quality, then hand the answer to
/// `on_export`. The live summary reads the even frame each size lands on,
/// so the three sizes are resolved once while the dialog is built.
pub(super) fn show(
    parent: &gtk4::ApplicationWindow,
    motion: &super::motion::MotionMode,
    on_export: impl FnOnce(super::motion::MotionExportOptions) + 'static,
) {
    let scale = crate::platform::ui_scale();
    let dimensions = [
        motion.export_dimensions(MotionExportSize::Canvas),
        motion.export_dimensions(MotionExportSize::Medium),
        motion.export_dimensions(MotionExportSize::Small),
    ];

    let dialog = gtk4::Dialog::new();
    dialog.set_title(Some(&t("Export Motion video")));
    dialog.set_modal(true);
    dialog.set_transient_for(Some(parent));
    dialog.set_destroy_with_parent(true);
    dialog.set_resizable(false);
    dialog.add_button(&t("Cancel"), gtk4::ResponseType::Cancel);
    let accept = dialog.add_button(&t("Export"), gtk4::ResponseType::Accept);
    accept.add_css_class("suggested-action");
    dialog.set_default_response(gtk4::ResponseType::Accept);

    let size_combo = combo(&[
        ("canvas", "Canvas size"),
        ("medium", "75%"),
        ("small", "50%"),
    ]);
    let fps_combo = combo(&[("30", "30 FPS"), ("60", "60 FPS")]);
    let quality_combo = combo(&[("high", "High quality"), ("smaller", "Smaller file")]);

    let content = GtkBox::new(Orientation::Vertical, px(14.0, scale));
    content.set_margin_top(px(18.0, scale));
    content.set_margin_bottom(px(18.0, scale));
    content.set_margin_start(px(20.0, scale));
    content.set_margin_end(px(20.0, scale));
    for (title, combo) in [
        ("Size", &size_combo),
        ("Frame rate", &fps_combo),
        ("Quality", &quality_combo),
    ] {
        let row = GtkBox::new(Orientation::Horizontal, px(12.0, scale));
        let label = Label::new(Some(&t(title)));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        row.append(&label);
        row.append(combo);
        combo.update_property(&[gtk4::accessible::Property::Label(&t(title))]);
        content.append(&row);
    }

    let summary = Label::new(None);
    summary.set_xalign(0.0);
    content.append(&summary);
    let explanation = Label::new(Some(&t(
        "60 FPS makes animation smoother. Export cannot recover detail missing from the screenshot.",
    )));
    explanation.set_xalign(0.0);
    explanation.set_wrap(true);
    explanation.set_max_width_chars(52);
    content.append(&explanation);

    let refresh = {
        let summary = summary.clone();
        let size_combo = size_combo.downgrade();
        let fps_combo = fps_combo.downgrade();
        Rc::new(move || {
            let (Some(size_combo), Some(fps_combo)) = (size_combo.upgrade(), fps_combo.upgrade())
            else {
                return;
            };
            let (width, height) = match selected_size(&size_combo) {
                MotionExportSize::Canvas => dimensions[0],
                MotionExportSize::Medium => dimensions[1],
                MotionExportSize::Small => dimensions[2],
            }
            .unwrap_or((0, 0));
            let width = width.to_string();
            let height = height.to_string();
            let fps = selected_fps(&fps_combo).value().to_string();
            summary.set_text(&tfmt(
                "{width} × {height} · {fps} FPS · MP4",
                &[("width", &width), ("height", &height), ("fps", &fps)],
            ));
        })
    };
    refresh();
    size_combo.connect_changed({
        let refresh = refresh.clone();
        move |_| refresh()
    });
    fps_combo.connect_changed(move |_| refresh());

    dialog.content_area().append(&content);
    let on_export = Cell::new(Some(on_export));
    dialog.connect_response({
        let size_combo = size_combo.clone();
        let fps_combo = fps_combo.clone();
        let quality_combo = quality_combo.clone();
        move |dialog, response| {
            if response == gtk4::ResponseType::Accept {
                let options = super::motion::MotionExportOptions {
                    size: selected_size(&size_combo),
                    fps: selected_fps(&fps_combo),
                    quality: match quality_combo.active_id().as_deref() {
                        Some("smaller") => MotionExportQuality::Smaller,
                        _ => MotionExportQuality::High,
                    },
                };
                let on_export = on_export.take();
                dialog.destroy();
                if let Some(on_export) = on_export {
                    on_export(options);
                }
            } else {
                dialog.destroy();
            }
        }
    });
    dialog.present();
}

/// A labelled dropdown with its first entry selected.
fn combo(entries: &[(&str, &str)]) -> ComboBoxText {
    let combo = ComboBoxText::new();
    for &(id, label) in entries {
        combo.append(Some(id), &t(label));
    }
    if let Some(&(id, _)) = entries.first() {
        combo.set_active_id(Some(id));
    }
    combo
}

fn selected_size(size_combo: &ComboBoxText) -> MotionExportSize {
    match size_combo.active_id().as_deref() {
        Some("medium") => MotionExportSize::Medium,
        Some("small") => MotionExportSize::Small,
        _ => MotionExportSize::Canvas,
    }
}

fn selected_fps(fps_combo: &ComboBoxText) -> MotionExportFps {
    match fps_combo.active_id().as_deref() {
        Some("60") => MotionExportFps::Fps60,
        _ => MotionExportFps::Fps30,
    }
}
