use gtk4::{
    gio, glib, prelude::*, Application, ApplicationWindow, Button, FileChooserAction,
    FileChooserNative, FileFilter, ResponseType, Spinner,
};
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
        state.static_image_transform,
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
    save_spinner: &Spinner,
    traffic_close: &Button,
    in_motion: Rc<Cell<bool>>,
    export_motion: Rc<dyn Fn(&Path) -> Result<PathBuf, String>>,
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
    let (upload_done_tx, upload_done_rx) =
        std::sync::mpsc::channel::<Result<String, crate::cloud::upload::UploadError>>();
    let upload_btn_poll = upload_btn.clone();
    let uploading_poll = uploading.clone();
    let window_upload = window.downgrade();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        match upload_done_rx.try_recv() {
            Ok(result) => {
                uploading_poll.set(false);
                upload_btn_poll.set_sensitive(true);
                match result {
                    Ok(share_url) => {
                        if let Err(error) =
                            crate::utils::clipboard::copy_text_to_gtk_clipboard(&share_url)
                        {
                            eprintln!("[editor] Failed to copy share link: {error}");
                        }
                    }
                    Err(crate::cloud::upload::UploadError::UpgradeRequired(message)) => {
                        if let Some(window) = window_upload.upgrade() {
                            crate::cloud::upgrade::show_prompt(&window, &message);
                        }
                    }
                    Err(crate::cloud::upload::UploadError::Server(message))
                        if message.starts_with("Could not save edits: ") =>
                    {
                        let error = &message["Could not save edits: ".len()..];
                        crate::utils::notify::desktop_notification_important(
                            &crate::i18n::t("Upload failed"),
                            &crate::i18n::tfmt(
                                "Could not save edits: {error}",
                                &[("error", error)],
                            ),
                        );
                    }
                    Err(_) => {}
                }
                glib::ControlFlow::Continue
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                uploading_poll.set(false);
                upload_btn_poll.set_sensitive(true);
                glib::ControlFlow::Break
            }
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

        let final_image = {
            let state = state_upload.lock().unwrap();
            match state.to_final_image() {
                Ok(image) => image,
                Err(error) => {
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
        };

        uploading_click.set(true);
        upload_btn_click.set_sensitive(false);

        let path = path_upload.clone();
        let upload_done_tx = upload_done_tx.clone();
        std::thread::spawn(move || {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            let snapshot_dir = dirs::cache_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("apexshot")
                .join("upload-snapshots")
                .join(format!("{}-{stamp}", std::process::id()));
            let snapshot_root = snapshot_dir.parent().unwrap_or(&snapshot_dir);
            let result = match std::fs::create_dir_all(snapshot_root)
                .and_then(|()| std::fs::create_dir(&snapshot_dir))
            {
                Err(error) => Err(crate::cloud::upload::UploadError::Server(format!(
                    "Could not save edits: {error}"
                ))),
                Ok(()) => {
                    let filename = path
                        .file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new("upload.png"));
                    let snapshot_path = snapshot_dir.join(filename);
                    let result = match crate::capture::editor::io_ops::save_edited_image_snapshot(
                        &snapshot_path,
                        final_image,
                    ) {
                        Err(error) => Err(crate::cloud::upload::UploadError::Server(format!(
                            "Could not save edits: {error}"
                        ))),
                        Ok(()) => {
                            crate::cloud::upload::upload_file_with_notifications_without_clipboard(
                                &config,
                                &snapshot_path,
                            )
                            .map(|result| result.share_url)
                        }
                    };
                    let _ = std::fs::remove_dir_all(&snapshot_dir);
                    result
                }
            };
            let _ = upload_done_tx.send(result);
        });
    });

    let state_save = state.clone();
    let path_save = path.to_path_buf();
    let window_save = window.downgrade();
    let app_save = app.downgrade();
    let save_chooser: Rc<RefCell<Option<gtk4::FileChooserNative>>> = Rc::new(RefCell::new(None));
    let save_chooser_open = Rc::new(Cell::new(false));
    let done_busy = Rc::new(Cell::new(false));
    let done_spinner = save_spinner.clone();
    let save_btn_click = save_btn.clone();
    save_btn.connect_clicked(move |_| {
        let state_save = state_save.clone();
        let path_save = path_save.clone();
        let window_save = window_save.clone();
        let app_save = app_save.clone();
        let in_motion = in_motion.clone();
        let export_motion = export_motion.clone();
        let save_chooser = save_chooser.clone();
        let save_chooser_open = save_chooser_open.clone();
        let done_busy = done_busy.clone();
        let done_spinner = done_spinner.clone();
        let save_btn_click = save_btn_click.clone();
        glib::idle_add_local_once(move || {
            if done_busy.get() {
                return;
            }

            if in_motion.get() {
                if save_chooser_open.get() {
                    return;
                }
                let Some(parent) = window_save.upgrade() else {
                    return;
                };
                let suggestion = motion_export_suggestion(&path_save);
                save_chooser_open.set(true);

                let chooser = FileChooserNative::new(
                    Some(&crate::i18n::t("Export Motion video")),
                    Some(&parent),
                    FileChooserAction::Save,
                    Some(&crate::i18n::t("Export")),
                    Some(&crate::i18n::t("Cancel")),
                );
                let filter = FileFilter::new();
                filter.set_name(Some(&crate::i18n::t("MP4 video")));
                filter.add_mime_type("video/mp4");
                filter.add_pattern("*.mp4");
                chooser.add_filter(&filter);
                if let Some(folder) = suggestion.parent() {
                    let _ = chooser.set_current_folder(Some(&gio::File::for_path(folder)));
                }
                if let Some(name) = suggestion.file_name() {
                    chooser.set_current_name(&name.to_string_lossy());
                }

                let state_resp = state_save.clone();
                let path_resp = path_save.clone();
                let window_resp = window_save.clone();
                let app_resp = app_save.clone();
                let save_chooser_resp = save_chooser.clone();
                let save_chooser_open_resp = save_chooser_open.clone();
                let done_busy_resp = done_busy.clone();
                let done_spinner_resp = done_spinner.clone();
                let save_btn_resp = save_btn_click.clone();
                let export_motion = export_motion.clone();
                chooser.connect_response(move |dialog, response| {
                    dialog.hide();
                    save_chooser_open_resp.set(false);
                    *save_chooser_resp.borrow_mut() = None;

                    let destination = (response == ResponseType::Accept)
                        .then(|| dialog.file())
                        .flatten()
                        .and_then(|file| file.path());
                    let Some(destination) = destination else {
                        return;
                    };
                    let destination = with_mp4_extension(destination);

                    done_busy_resp.set(true);
                    save_btn_resp.set_sensitive(false);
                    done_spinner_resp.set_visible(true);
                    done_spinner_resp.start();

                    let state_resp = state_resp.clone();
                    let path_resp = path_resp.clone();
                    let window_resp = window_resp.clone();
                    let app_resp = app_resp.clone();
                    let export_motion = export_motion.clone();
                    let done_busy_inner = done_busy_resp.clone();
                    let done_spinner_inner = done_spinner_resp.clone();
                    let save_btn_inner = save_btn_resp.clone();
                    // One frame for the spinner before the export blocks the main
                    // loop; the window stays up so the spin is visible.
                    glib::timeout_add_local_once(Duration::from_millis(50), move || {
                        match export_motion(&destination) {
                            Ok(video_path) => {
                                let mut config = crate::config::load_config();
                                if config.remember_video_export_dir(&video_path) {
                                    let _ = crate::config::save_config(&config);
                                }
                                let _ =
                                    persist_image_session(&path_resp, &state_resp.lock().unwrap());
                                crate::utils::notify::desktop_notification(
                                    &crate::i18n::t("Export complete"),
                                    &video_path.display().to_string(),
                                );
                                if let Some(window) = window_resp.upgrade() {
                                    window.close();
                                }
                                if let Some(app) = app_resp.upgrade() {
                                    app.quit();
                                }
                            }
                            Err(error) => {
                                eprintln!("Failed to export Motion video: {error}");
                                crate::utils::notify::desktop_notification_important(
                                    &crate::i18n::t("Export failed"),
                                    &error,
                                );
                                done_busy_inner.set(false);
                                done_spinner_inner.stop();
                                done_spinner_inner.set_visible(false);
                                save_btn_inner.set_sensitive(true);
                            }
                        }
                    });
                });
                *save_chooser.borrow_mut() = Some(chooser.clone());
                chooser.show();
                return;
            }

            if save_chooser_open.get() {
                return;
            }

            if !done_needs_save_chooser(&path_save) {
                done_busy.set(true);
                save_btn_click.set_sensitive(false);
                done_spinner.set_visible(true);
                done_spinner.start();
                // One frame for the spinner before the flatten blocks the main
                // loop.
                glib::timeout_add_local_once(Duration::from_millis(50), move || {
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
                            done_busy.set(false);
                            done_spinner.stop();
                            done_spinner.set_visible(false);
                            save_btn_click.set_sensitive(true);
                        }
                    }
                });
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

/// Where the Motion export chooser opens and what it pre-fills: the same folder
/// the video editor starts in (remembered export folder, else the Recordings
/// folder, else XDG Videos), named from the still the editor opened.
fn motion_export_suggestion(source_image: &Path) -> PathBuf {
    let config = crate::config::load_config().sanitized();
    let fallback = crate::recording::recording_output_dir(&config);
    let dir = config.motion_export_dir(&fallback);
    let stem = source_image
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("ApexShot");
    dir.join(format!("{stem} Motion.mp4"))
}

/// Motion is always encoded as MP4, so an extension-less or mis-typed choice
/// from the Save chooser is corrected rather than written as-is.
fn with_mp4_extension(mut destination: PathBuf) -> PathBuf {
    let is_mp4 = destination
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"));
    if !is_mp4 {
        destination.set_extension("mp4");
    }
    destination
}

#[cfg(test)]
mod tests {
    use super::{done_needs_save_chooser, with_mp4_extension};
    use std::path::PathBuf;

    #[test]
    fn motion_export_forces_an_mp4_extension() {
        assert_eq!(
            with_mp4_extension(PathBuf::from("/tmp/clip")),
            PathBuf::from("/tmp/clip.mp4")
        );
        assert_eq!(
            with_mp4_extension(PathBuf::from("/tmp/clip.MOV")),
            PathBuf::from("/tmp/clip.mp4")
        );
        assert_eq!(
            with_mp4_extension(PathBuf::from("/tmp/clip.mp4")),
            PathBuf::from("/tmp/clip.mp4")
        );
    }

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
                && handler.contains("export_motion(&destination)")
                && handler.contains("save_edited_image"),
            "Done in Motion exports MP4 to the chosen path; Done in Static still flattens the PNG"
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
