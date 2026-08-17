use super::dialogs;
use super::panels::EditorControls;
use crate::recording::editor::ffmpeg;
use crate::recording::editor::model::{format_size, VideoEditState};
use gtk4::{
    glib, prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, Image, Label, Orientation,
    Spinner,
};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(super) fn build_inspector_actions(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    _estimate_label: Label,
    controls: EditorControls,
    exporting: Rc<Cell<bool>>,
) -> GtkBox {
    let footer = GtkBox::new(Orientation::Horizontal, 6);
    footer.add_css_class("recording-editor-footer");
    footer.add_css_class("recording-editor-inspector-toolbar");
    footer.add_css_class("editor-sidebar-actions");
    footer.set_hexpand(true);

    let copy = icon_action_button(
        crate::capture::editor::window::icon_names::custom::COPY_SYMBOLIC,
        "Copy the recording path to the clipboard",
    );
    let upload = icon_action_button(
        crate::capture::editor::window::icon_names::custom::CLOUD_OUTLINE_THIN_SYMBOLIC,
        "Export with your current settings, then upload",
    );
    let done = Button::with_label("Done");
    done.set_has_frame(false);
    done.add_css_class("recording-editor-primary-button");
    done.set_halign(Align::End);
    done.set_tooltip_text(Some("Export the edited MP4"));
    let spinner = Spinner::new();
    spinner.set_visible(false);

    let export_controls = vec![
        copy.clone().upcast::<gtk4::Widget>(),
        upload.clone().upcast::<gtk4::Widget>(),
        done.clone().upcast::<gtk4::Widget>(),
        controls.dimension_button.clone().upcast::<gtk4::Widget>(),
        controls.width_entry.clone().upcast::<gtk4::Widget>(),
        controls.height_entry.clone().upcast::<gtk4::Widget>(),
        controls.quality_scale.clone().upcast::<gtk4::Widget>(),
        controls.audio_unchanged.clone().upcast::<gtk4::Widget>(),
        controls.audio_mono.clone().upcast::<gtk4::Widget>(),
        controls.audio_muted.clone().upcast::<gtk4::Widget>(),
    ];

    copy.connect_clicked({
        let state = state.clone();
        move |_| {
            let path = state.lock().unwrap().metadata.path.clone();
            if let Err(err) = crate::utils::clipboard::copy_uri_to_clipboard(&path) {
                crate::utils::notify::desktop_notification("Copy failed", &err.to_string());
            } else {
                crate::utils::notify::desktop_notification("Copied", "Recording path copied");
            }
        }
    });

    wire_upload_button(
        &upload,
        state.clone(),
        export_controls.clone(),
        spinner.clone(),
        exporting.clone(),
    );
    wire_export_button(
        &done,
        window,
        state,
        export_controls,
        spinner.clone(),
        exporting,
    );

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    footer.append(&copy);
    footer.append(&upload);
    footer.append(&spinner);
    footer.append(&spacer);
    footer.append(&done);
    footer
}

fn icon_action_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-inspector-icon");
    button.set_tooltip_text(Some(tooltip));
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(16);
    button.set_child(Some(&icon));
    button
}

pub(super) fn update_estimate(label: &Label, state: &Arc<Mutex<VideoEditState>>, _trim_only: bool) {
    let state = state.lock().unwrap();
    let trim_only = !state.needs_reencode();
    label.set_text(&format!(
        "~{}",
        format_size(state.estimated_size_bytes(trim_only)),
    ));
    label.set_tooltip_text(Some("Estimated export size"));
}

fn wire_upload_button(
    button: &Button,
    state: Arc<Mutex<VideoEditState>>,
    controls: Vec<gtk4::Widget>,
    spinner: Spinner,
    exporting: Rc<Cell<bool>>,
) {
    button.connect_clicked(move |_| {
        if exporting.get() {
            return;
        }

        let config = crate::config::load_config();
        if !crate::cloud::upload::is_configured(&config) {
            let (title, body) = crate::cloud::upload::not_configured_notification(&config);
            crate::utils::notify::desktop_notification(title, body);
            return;
        }

        exporting.set(true);
        spinner.set_visible(true);
        spinner.start();
        for control in &controls {
            control.set_sensitive(false);
        }

        // Export with current editor settings first, then upload the result.
        let state_snapshot = state.lock().unwrap().clone();
        let (sender, receiver) = std::sync::mpsc::channel::<Result<String, String>>();
        std::thread::spawn(move || {
            let result = (|| {
                let path = ffmpeg::export_edited(&state_snapshot)
                    .map_err(|err| format!("Export before upload failed: {err}"))?;
                crate::cloud::upload::upload_file(&config, &path)
                    .map(|result| result.share_url)
                    .map_err(|err| err.to_string())
            })();
            let _ = sender.send(result);
        });

        let controls = controls.clone();
        let spinner = spinner.clone();
        let exporting = exporting.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            match receiver.try_recv() {
                Ok(result) => {
                    exporting.set(false);
                    spinner.stop();
                    spinner.set_visible(false);
                    for control in &controls {
                        control.set_sensitive(true);
                    }
                    match result {
                        Ok(share_url) => {
                            if let Err(e) =
                                crate::utils::clipboard::copy_text_to_clipboard(&share_url)
                            {
                                eprintln!("Failed to copy share link to clipboard: {e}");
                                crate::utils::notify::desktop_notification(
                                    "Upload complete",
                                    &format!("Share link: {share_url}"),
                                );
                            } else {
                                crate::utils::notify::desktop_notification(
                                    "Upload complete",
                                    "Share link copied to clipboard",
                                );
                            }
                        }
                        Err(err) => {
                            crate::utils::notify::desktop_notification("Upload failed", &err);
                        }
                    }
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    exporting.set(false);
                    spinner.stop();
                    spinner.set_visible(false);
                    for control in &controls {
                        control.set_sensitive(true);
                    }
                    crate::utils::notify::desktop_notification(
                        "Upload failed",
                        "ApexShot lost contact with the upload worker.",
                    );
                    glib::ControlFlow::Break
                }
            }
        });
    });
}

fn wire_export_button(
    button: &Button,
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    controls: Vec<gtk4::Widget>,
    spinner: Spinner,
    exporting: Rc<Cell<bool>>,
) {
    let window = window.clone();
    button.connect_clicked(move |_| {
        if exporting.get() {
            return;
        }
        exporting.set(true);
        spinner.set_visible(true);
        spinner.start();
        for control in &controls {
            control.set_sensitive(false);
        }

        let state_snapshot = state.lock().unwrap().clone();
        let (sender, receiver) = std::sync::mpsc::channel::<Result<PathBuf, String>>();
        std::thread::spawn(move || {
            let result = ffmpeg::export_edited(&state_snapshot);
            let _ = sender.send(result.map_err(|err| err.to_string()));
        });

        let controls = controls.clone();
        let spinner = spinner.clone();
        let exporting = exporting.clone();
        let window = window.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            match receiver.try_recv() {
                Ok(result) => {
                    exporting.set(false);
                    spinner.stop();
                    spinner.set_visible(false);
                    for control in &controls {
                        control.set_sensitive(true);
                    }
                    match result {
                        Ok(path) => dialogs::show_success(&window, path),
                        Err(err) => dialogs::show_error(
                            &window,
                            "Export failed",
                            "ApexShot could not export this recording.",
                            Some(&err),
                        ),
                    }
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    exporting.set(false);
                    spinner.stop();
                    spinner.set_visible(false);
                    for control in &controls {
                        control.set_sensitive(true);
                    }
                    dialogs::show_error(
                        &window,
                        "Export failed",
                        "ApexShot lost contact with the export worker.",
                        None,
                    );
                    glib::ControlFlow::Break
                }
            }
        });
    });
}
