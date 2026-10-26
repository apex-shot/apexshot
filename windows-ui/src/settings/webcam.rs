//! The Webcam group of the Windows Recording page.
//!
//! The recorder writes the camera to its own track and the editor composites
//! it, so these are layer settings: which camera, how big, which shape, where
//! it sits and whether the picture is mirrored. The camera list is filled in
//! after the page is built, because listing DirectShow devices opens each one
//! and costs about a second.

use crate::config::AppConfig;
use crate::i18n::t;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, CheckButton, Label, Orientation};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

use super::select::SettingsSelect;

/// The corner presets, in the shared config's coordinates: `rel_x` runs left to
/// right and `rel_y` bottom to top, so bottom-right is x 1, y 0.
const POSITIONS: [(&str, f64, f64); 5] = [
    ("bottom_right", 1.0, 0.0),
    ("bottom_left", 0.0, 0.0),
    ("top_right", 1.0, 1.0),
    ("top_left", 0.0, 1.0),
    ("center", 0.5, 0.5),
];

#[allow(dead_code)]
pub(super) struct WebcamInputs {
    pub enabled: CheckButton,
    /// The chosen camera's DirectShow name, empty for the first camera.
    pub device: Rc<RefCell<String>>,
    pub size: SettingsSelect,
    pub shape: SettingsSelect,
    pub position: SettingsSelect,
    pub flip: CheckButton,
}

impl WebcamInputs {
    /// The stored position for the chosen preset.
    pub fn position(&self) -> (f64, f64) {
        let chosen = self.position.active_id().unwrap_or_default();
        POSITIONS
            .iter()
            .find(|(id, _, _)| *id == chosen)
            .map(|(_, x, y)| (*x, *y))
            .unwrap_or((1.0, 0.0))
    }
}

fn position_id(config: &AppConfig) -> &'static str {
    POSITIONS
        .iter()
        .find(|(_, x, y)| {
            (config.rec_webcam_rel_x - x).abs() < 0.01 && (config.rec_webcam_rel_y - y).abs() < 0.01
        })
        .map(|(id, _, _)| *id)
        .unwrap_or("bottom_right")
}

fn row(title: &str, description: Option<&str>, control: &impl IsA<gtk4::Widget>) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.add_css_class("settings-table-row");
    row.set_hexpand(true);

    let text = GtkBox::new(Orientation::Vertical, 4);
    text.set_hexpand(true);
    let title_label = Label::new(Some(&t(title)));
    title_label.set_xalign(0.0);
    text.append(&title_label);
    if let Some(description) = description {
        let description_label = Label::new(Some(&t(description)));
        description_label.set_xalign(0.0);
        description_label.set_wrap(true);
        description_label.add_css_class("settings-sub-option-hint");
        text.append(&description_label);
    }
    row.append(&text);
    row.append(control);
    row
}

/// Every camera DirectShow reports. ffmpeg's device listing goes to stderr and
/// exits non-zero on every release, so only the parsed names matter.
fn camera_devices() -> Vec<String> {
    let output = std::process::Command::new(crate::recording::editor::ffmpeg::ffmpeg_executable())
        .args([
            "-hide_banner",
            "-list_devices",
            "true",
            "-f",
            "dshow",
            "-i",
            "dummy",
        ])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();
    for line in text.lines() {
        let Some(start) = line.find('"') else {
            continue;
        };
        let Some(end) = line[start + 1..].find('"') else {
            continue;
        };
        if !line.contains("(video)") {
            continue;
        }
        let name = line[start + 1..start + 1 + end].to_string();
        if !devices.contains(&name) {
            devices.push(name);
        }
    }
    devices
}

pub(super) fn build_webcam_group(config: &AppConfig) -> (GtkBox, WebcamInputs) {
    let group = GtkBox::new(Orientation::Vertical, 14);
    group.set_halign(Align::Fill);
    group.set_valign(Align::Start);
    group.set_hexpand(true);

    let title = Label::new(Some(&t("Webcam")));
    title.add_css_class("settings-group-title");
    title.set_xalign(0.0);
    title.set_halign(Align::Start);
    title.set_margin_bottom(8);
    group.append(&title);

    let frame = GtkBox::new(Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.set_margin_bottom(24);
    frame.set_margin_start(4);
    frame.set_margin_end(4);

    let enabled = CheckButton::new();
    enabled.set_active(config.rec_webcam_enabled);
    let enabled_row = row(
        "Record a webcam track",
        Some("The camera is recorded beside the video and added by the editor as a layer you can keep or drop."),
        &enabled,
    );
    frame.append(&enabled_row);

    let device = Rc::new(RefCell::new(config.rec_webcam_device.clone()));
    let picker_slot = GtkBox::new(Orientation::Horizontal, 0);
    picker_slot.set_halign(Align::End);
    let picker = SettingsSelect::new([("", t("First available camera"))], "");
    picker_slot.append(picker.widget());
    {
        let device = device.clone();
        let picker_cb = picker.clone();
        picker.connect_changed(move || {
            if let Some(id) = picker_cb.active_id() {
                *device.borrow_mut() = id;
            }
        });
    }
    let device_row = row("Camera", None, &{
        let holder = GtkBox::new(Orientation::Horizontal, 0);
        holder.append(&picker_slot);
        holder
    });
    frame.append(&device_row);

    // The list arrives once the probe is done; until then the picker holds the
    // configured camera so the page never shows an empty control.
    let (sender, receiver) = mpsc::channel::<Vec<String>>();
    std::thread::spawn(move || {
        let _ = sender.send(camera_devices());
    });
    {
        let picker_slot = picker_slot.clone();
        let device = device.clone();
        let configured = config.rec_webcam_device.clone();
        glib::timeout_add_local(Duration::from_millis(150), move || {
            match receiver.try_recv() {
                Ok(devices) => {
                    let items: Vec<(String, String)> =
                        std::iter::once((String::new(), t("First available camera")))
                            .chain(devices.iter().map(|name| (name.clone(), name.clone())))
                            .collect();
                    let picker = SettingsSelect::new(items, &configured);
                    let device = device.clone();
                    let picker_cb = picker.clone();
                    picker.connect_changed(move || {
                        if let Some(id) = picker_cb.active_id() {
                            *device.borrow_mut() = id;
                        }
                    });
                    while let Some(child) = picker_slot.first_child() {
                        picker_slot.remove(&child);
                    }
                    picker_slot.append(picker.widget());
                    glib::ControlFlow::Break
                }
                Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(TryRecvError::Disconnected) => glib::ControlFlow::Break,
            }
        });
    }

    let size = SettingsSelect::new(
        [
            ("0", t("Small")),
            ("1", t("Medium")),
            ("2", t("Large")),
            ("3", t("Huge")),
        ],
        &config.rec_webcam_size.to_string(),
    );
    frame.append(&row("Size", None, size.widget()));

    let shape = SettingsSelect::new(
        [
            ("0", t("Circle")),
            ("1", t("Square")),
            ("2", t("Rectangle")),
            ("3", t("Vertical")),
        ],
        &config.rec_webcam_shape.to_string(),
    );
    frame.append(&row("Shape", None, shape.widget()));

    let position = SettingsSelect::new(
        [
            ("bottom_right", t("Bottom right")),
            ("bottom_left", t("Bottom left")),
            ("top_right", t("Top right")),
            ("top_left", t("Top left")),
            ("center", t("Center")),
        ],
        position_id(config),
    );
    frame.append(&row("Position", None, position.widget()));

    let flip = CheckButton::new();
    flip.set_active(config.rec_webcam_flip);
    frame.append(&row(
        "Mirror the picture",
        Some("Show the camera as a mirror image, the way a preview usually does."),
        &flip,
    ));

    group.append(&frame);

    (
        group,
        WebcamInputs {
            enabled,
            device,
            size,
            shape,
            position,
            flip,
        },
    )
}
