use gtk4::{glib, prelude::*, Application, ApplicationWindow, Button};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::annotations::{save_annotations, AnnotationError};
use crate::capture::editor::{
    io_ops::{save_clipboard_image, save_edited_image},
    state::EditorState,
};

pub fn persist_image_session(path: &Path, state: &EditorState) -> Result<(), AnnotationError> {
    save_annotations(
        path,
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
    )
}

pub(super) fn wire_output_lifecycle(
    app: &Application,
    window: &ApplicationWindow,
    path: &Path,
    state: &Arc<Mutex<EditorState>>,
    copy_btn: &Button,
    upload_btn: &Button,
    save_btn: &Button,
    traffic_close: &Button,
    in_motion: Rc<Cell<bool>>,
    export_motion: Rc<dyn Fn() -> Result<PathBuf, String>>,
) {
    let state_copy = state.clone();
    copy_btn.connect_clicked(move |_| {
        let config = crate::config::load_config().sanitized();
        let mode = crate::utils::clipboard::ScreenshotClipboardMode::from_config_value(
            &config.adv_clipboard_mode,
        );
        let snapshot = {
            let state = state_copy.lock().unwrap();
            save_clipboard_image(
                crate::capture::unsaved::UnsavedCaptureStore::app_owned().dir(),
                &state,
            )
        };
        let result = snapshot
            .map_err(|error| error.to_string())
            .and_then(|snapshot| {
                let result = crate::utils::clipboard::copy_screenshot_with_mode(&snapshot, mode);
                if result.is_err() {
                    let _ = std::fs::remove_file(snapshot);
                }
                result
            });
        if let Err(error) = result {
            eprintln!("Copy failed: {error}");
            crate::utils::notify::desktop_notification_important(
                &crate::i18n::t("Copy failed"),
                &error,
            );
        }
    });

    let path_upload = path.to_path_buf();
    let state_upload = state.clone();
    // Worker completion returns to GTK's main loop so the !Send button can be re-enabled.
    let uploading = Rc::new(Cell::new(false));
    let (upload_done_tx, upload_done_rx) = std::sync::mpsc::channel::<Option<String>>();
    let upload_btn_poll = upload_btn.clone();
    let uploading_poll = uploading.clone();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        match upload_done_rx.try_recv() {
            Ok(share_url) => {
                uploading_poll.set(false);
                upload_btn_poll.set_sensitive(true);
                if let Some(share_url) = share_url {
                    if let Err(error) =
                        crate::utils::clipboard::copy_text_to_gtk_clipboard(&share_url)
                    {
                        eprintln!("[editor] Failed to copy share link: {error}");
                    }
                }
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
            crate::utils::notify::desktop_notification_important(&title, &body);
            return;
        }

        {
            let state = state_upload.lock().unwrap();
            if let Err(error) = save_edited_image(&path_upload, &state) {
                eprintln!("[editor] Failed to save edits before upload: {error}");
                crate::utils::notify::desktop_notification_important(
                    &crate::i18n::t("Upload failed"),
                    &crate::i18n::tfmt(
                        "Could not save edits: {error}",
                        &[("error", &error.to_string())],
                    ),
                );
                return;
            }
        }

        uploading_click.set(true);
        upload_btn_click.set_sensitive(false);

        let path = path_upload.clone();
        let upload_done_tx = upload_done_tx.clone();
        std::thread::spawn(move || {
            let share_url = crate::cloud::upload::upload_file_with_notifications_without_clipboard(
                &config, &path,
            )
            .ok()
            .map(|result| result.share_url);
            let _ = upload_done_tx.send(share_url);
        });
    });

    let state_save = state.clone();
    let path_save = path.to_path_buf();
    let window_save = window.downgrade();
    let app_save = app.downgrade();
    let save_chooser: Rc<RefCell<Option<gtk4::FileChooserNative>>> = Rc::new(RefCell::new(None));
    let save_chooser_open = Rc::new(Cell::new(false));
    save_btn.connect_clicked(move |_| {
        let state_save = state_save.clone();
        let path_save = path_save.clone();
        let window_save = window_save.clone();
        let app_save = app_save.clone();
        let in_motion = in_motion.clone();
        let export_motion = export_motion.clone();
        let save_chooser = save_chooser.clone();
        let save_chooser_open = save_chooser_open.clone();
        glib::idle_add_local_once(move || {
            if in_motion.get() {
                if let Some(window) = window_save.upgrade() {
                    window.set_visible(false);
                }
                match export_motion() {
                    Ok(video_path) => {
                        let _ = persist_image_session(&path_save, &state_save.lock().unwrap());
                        crate::utils::notify::desktop_notification(
                            &crate::i18n::t("Export complete"),
                            &video_path.display().to_string(),
                        );
                        if let Some(window) = window_save.upgrade() {
                            window.close();
                        }
                        if let Some(app) = app_save.upgrade() {
                            app.quit();
                        }
                    }
                    Err(error) => {
                        eprintln!("Failed to export Motion video: {error}");
                        crate::utils::notify::desktop_notification_important(
                            &crate::i18n::t("Export failed"),
                            &error,
                        );
                        if let Some(window) = window_save.upgrade() {
                            window.set_visible(true);
                        }
                    }
                }
                return;
            }

            if save_chooser_open.get() {
                return;
            }

            if !done_needs_save_chooser(&path_save) {
                if let Some(window) = window_save.upgrade() {
                    window.set_visible(false);
                }
                let save_result = {
                    let state = state_save.lock().unwrap();
                    save_edited_image(&path_save, &state)
                };
                match save_result {
                    Ok(()) => {
                        finish_image_session(&state_save, &path_save, &window_save, &app_save)
                    }
                    Err(error) => {
                        eprintln!("Failed to save edited image: {error}");
                        if let Some(window) = window_save.upgrade() {
                            window.set_visible(true);
                        }
                    }
                }
                return;
            }

            if let Err(error) = save_edited_image(&path_save, &state_save.lock().unwrap()) {
                eprintln!("Failed to save edited image: {error}");
                return;
            }

            save_chooser_open.set(true);
            let save_chooser_open_done = save_chooser_open.clone();
            let save_chooser_done = save_chooser.clone();
            let state_done = state_save.clone();
            let window_done = window_save.clone();
            let app_done = app_save.clone();
            let chooser = crate::capture::save_dialog::show_save_dialog(
                window_save
                    .upgrade()
                    .as_ref()
                    .map(|window| window.upcast_ref()),
                path_save.clone(),
                move |destination| {
                    save_chooser_open_done.set(false);
                    *save_chooser_done.borrow_mut() = None;

                    let Some(destination) = destination else {
                        if let Some(window) = window_done.upgrade() {
                            window.set_visible(true);
                        }
                        return;
                    };

                    finish_image_session(&state_done, &destination, &window_done, &app_done);
                },
            );
            *save_chooser.borrow_mut() = Some(chooser);
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

/// Done writes back to the file the editor opened; a capture the user has not
/// saved yet needs a destination from the user instead.
pub(super) fn done_needs_save_chooser(path: &Path) -> bool {
    crate::capture::unsaved::UnsavedCaptureStore::app_owned().owns(path)
}

/// Persist the session at `path`, honor the after-capture clipboard setting and
/// close the editor the way Done always has.
fn finish_image_session(
    state: &Arc<Mutex<EditorState>>,
    path: &Path,
    window: &gtk4::glib::WeakRef<ApplicationWindow>,
    app: &gtk4::glib::WeakRef<Application>,
) {
    if let Err(error) = persist_image_session(path, &state.lock().unwrap()) {
        eprintln!("[editor] Warning: Failed to save annotations: {error}");
    }

    let config = crate::config::load_config().sanitized();
    crate::daemon::copy_screenshot_to_clipboard(path, &config);
    if let Some(window) = window.upgrade() {
        window.close();
    }
    if let Some(app) = app.upgrade() {
        app.quit();
    }
    if config.after_capture_show_quick_access && !crate::daemon::show_preview_via_daemon(path) {
        let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
        if let Err(error) = Command::new(&executable).arg("preview").arg(path).spawn() {
            eprintln!("[editor] Failed to open preview: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::done_needs_save_chooser;

    #[test]
    fn editor_copy_exports_current_state_without_saving_the_source_or_opening_a_chooser() {
        let source = include_str!("output.rs");
        let handler = source
            .split("copy_btn.connect_clicked(move |_| {")
            .nth(1)
            .unwrap()
            .split("let path_upload")
            .next()
            .unwrap();
        assert!(handler.contains("state_copy.lock()"));
        assert!(handler.contains("save_clipboard_image"));
        assert!(handler.contains("copy_screenshot_with_mode(&snapshot, mode)"));
        assert!(!handler.contains("save_edited_image"));
        assert!(!handler.contains("show_save_dialog"));
    }

    #[test]
    fn done_routes_unsaved_captures_through_the_save_chooser() {
        let store = crate::capture::unsaved::UnsavedCaptureStore::app_owned();

        assert!(done_needs_save_chooser(&store.dir().join("unsaved-1.png")));
        assert!(!done_needs_save_chooser(std::path::Path::new(
            "/tmp/shot.png"
        )));
    }

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

    #[test]
    fn done_still_flattens_and_persists_session() {
        let source = include_str!("output.rs");
        let start = source
            .find("save_btn.connect_clicked(move |_| {")
            .expect("Done click handler");
        let handler = &source[start..];
        let flatten = handler.find("save_edited_image").expect("flatten PNG");
        let persist = handler
            .rfind("persist_image_session")
            .expect("persist session");
        assert!(
            flatten < persist,
            "Done must flatten before writing the sidecar"
        );
        assert!(
            handler.contains("if in_motion.get()")
                && handler.contains("export_motion()")
                && handler.contains("save_edited_image"),
            "Done in Motion exports MP4; Done in Static still flattens the PNG"
        );
    }

    #[test]
    fn done_releases_the_save_guard_before_finishing_the_session() {
        let source = include_str!("output.rs");
        let direct_save = source
            .split("if !done_needs_save_chooser(&path_save) {")
            .nth(1)
            .expect("direct save branch")
            .split("if let Err(error) = save_edited_image")
            .next()
            .expect("direct save handler");
        let scoped_save = direct_save
            .find("let save_result = {")
            .expect("scoped save");
        let guard_end = scoped_save
            + direct_save[scoped_save..]
                .find("};")
                .expect("save guard scope ends");
        let dispatch = direct_save
            .find("match save_result {")
            .expect("result dispatch");
        let finish = direct_save
            .find("finish_image_session(")
            .expect("finish session");
        assert!(direct_save[scoped_save..guard_end].contains("state_save.lock().unwrap()"));
        assert!(
            direct_save[scoped_save..guard_end].contains("save_edited_image(&path_save, &state)")
        );
        assert!(guard_end < dispatch && dispatch < finish);
        assert!(!direct_save.contains("match save_edited_image("));
    }
}
