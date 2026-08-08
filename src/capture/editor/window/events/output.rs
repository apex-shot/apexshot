use gtk4::{glib, prelude::*, Application, ApplicationWindow, Button};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::annotations::save_annotations;
use crate::capture::editor::{
    io_ops::{copy_uri_to_clipboard, save_edited_image},
    state::EditorState,
};

pub(super) fn wire_output_lifecycle(
    app: &Application,
    window: &ApplicationWindow,
    path: &Path,
    state: &Arc<Mutex<EditorState>>,
    copy_btn: &Button,
    upload_btn: &Button,
    save_btn: &Button,
    traffic_close: &Button,
) {
    let path_copy = path.to_path_buf();
    copy_btn.connect_clicked(move |_| {
        if let Err(error) = copy_uri_to_clipboard(&path_copy) {
            eprintln!("Copy failed: {error}");
        }
    });

    let path_upload = path.to_path_buf();
    let state_upload = state.clone();
    // Worker completion returns to GTK's main loop so the !Send button can be re-enabled.
    let uploading = Rc::new(Cell::new(false));
    let (upload_done_tx, upload_done_rx) = std::sync::mpsc::channel::<()>();
    let upload_btn_poll = upload_btn.clone();
    let uploading_poll = uploading.clone();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        match upload_done_rx.try_recv() {
            Ok(()) => {
                uploading_poll.set(false);
                upload_btn_poll.set_sensitive(true);
                glib::ControlFlow::Continue
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
    let uploading_click = uploading.clone();
    let upload_btn_click = upload_btn.clone();
    upload_btn.connect_clicked(move |_| {
        if uploading_click.get() {
            return;
        }

        let config = crate::config::load_config();
        if !crate::cloud::upload::is_configured(&config) {
            let (title, body) = crate::cloud::upload::not_configured_notification(&config);
            crate::utils::notify::desktop_notification_important(title, body);
            return;
        }

        {
            let state = state_upload.lock().unwrap();
            if let Err(error) = save_edited_image(&path_upload, &state) {
                eprintln!("[editor] Failed to save edits before upload: {error}");
                crate::utils::notify::desktop_notification_important(
                    "Upload failed",
                    &format!("Could not save edits: {error}"),
                );
                return;
            }
        }

        uploading_click.set(true);
        upload_btn_click.set_sensitive(false);

        let path = path_upload.clone();
        let upload_done_tx = upload_done_tx.clone();
        std::thread::spawn(move || {
            let _ = crate::cloud::upload::upload_file_with_notifications(&config, &path);
            let _ = upload_done_tx.send(());
        });
    });

    let state_save = state.clone();
    let path_save = path.to_path_buf();
    let window_save = window.downgrade();
    let app_save = app.downgrade();
    save_btn.connect_clicked(move |_| {
        if let Some(window) = window_save.upgrade() {
            window.set_visible(false);
        }

        let state_save = state_save.clone();
        let path_save = path_save.clone();
        let window_save = window_save.clone();
        let app_save = app_save.clone();
        glib::idle_add_local_once(move || {
            let (image_result, annotation_data) = {
                let state = state_save.lock().unwrap();
                let save_result = save_edited_image(&path_save, &state);
                let annotation_result = save_annotations(
                    &path_save,
                    state.base_image.width(),
                    state.base_image.height(),
                    &state.actions,
                    &state.base_image,
                    &state.background_style,
                    state.background_padding,
                    state.background_shadow,
                    state.background_insert,
                    state.auto_balance,
                    state.background_alignment,
                    state.background_corner_radius,
                    state.background_aspect_ratio,
                );
                (save_result, annotation_result)
            };

            if let Err(error) = annotation_data {
                eprintln!("[editor] Warning: Failed to save annotations: {error}");
            }

            match image_result {
                Ok(()) => {
                    let config = crate::config::load_config().sanitized();
                    crate::daemon::copy_screenshot_to_clipboard(&path_save, &config);
                    if let Some(window) = window_save.upgrade() {
                        window.close();
                    }
                    if let Some(app) = app_save.upgrade() {
                        app.quit();
                    }
                    if config.after_capture_show_quick_access
                        && !crate::daemon::show_preview_via_daemon(&path_save)
                    {
                        let executable =
                            std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
                        if let Err(error) = Command::new(&executable)
                            .arg("preview")
                            .arg(&path_save)
                            .spawn()
                        {
                            eprintln!("[editor] Failed to open preview: {error}");
                        }
                    }
                }
                Err(error) => {
                    eprintln!("Failed to save edited image: {error}");
                    if let Some(window) = window_save.upgrade() {
                        window.set_visible(true);
                    }
                }
            }
        });
    });

    let window_close = window.downgrade();
    let app_close = app.downgrade();
    traffic_close.connect_clicked(move |_| {
        if let Some(window) = window_close.upgrade() {
            window.close();
        }
        if let Some(app) = app_close.upgrade() {
            app.quit();
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn editor_upload_saves_edits_before_uploading() {
        let source = include_str!("output.rs");
        let upload_start = source
            .find("upload_btn.connect_clicked(move |_| {")
            .expect("upload button click handler");
        let upload_handler = &source[upload_start..];
        let save_position = upload_handler
            .find("save_edited_image")
            .expect("save before upload");
        let upload_position = upload_handler
            .find("upload_file_with_notifications")
            .expect("upload request");
        assert!(
            save_position < upload_position,
            "Image editor Upload must persist canvas edits before uploading the file"
        );
    }
}
