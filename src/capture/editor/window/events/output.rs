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
    types::EditorError,
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
        state.source_upscale,
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
    export_motion: super::super::motion_host::MotionExport,
) {
    let state_copy = state.clone();
    let window_copy = window.downgrade();
    let copy_retry = copy_btn.downgrade();
    let copying = Rc::new(Cell::new(false));
    let copy_btn_click = copy_btn.clone();
    copy_btn.connect_clicked(move |_| {
        if copying.get() {
            return;
        }
        let capabilities = {
            let state = state_copy.lock().unwrap();
            crate::studio_entitlement::capabilities_for_image_export(
                state.static_image_transform,
                state.source_upscale.factor() > 1,
            )
        };
        let Some(window) = window_copy.upgrade() else {
            return;
        };
        let retry = copy_retry.clone();
        if !crate::studio_upsell::gate(&window, capabilities, move || {
            if let Some(button) = retry.upgrade() {
                button.emit_clicked();
            }
        }) {
            return;
        }

        let config = crate::config::load_config().sanitized();
        let mode = crate::utils::clipboard::ScreenshotClipboardMode::from_config_value(
            &config.adv_clipboard_mode,
        );
        let snapshot = state_copy.lock().unwrap().export_snapshot();
        let directory = crate::capture::unsaved::UnsavedCaptureStore::app_owned()
            .dir()
            .to_path_buf();
        copying.set(true);
        copy_btn_click.set_sensitive(false);
        let copying_done = copying.clone();
        let copy_btn_done = copy_btn_click.clone();
        glib::MainContext::default().spawn_local(async move {
            let rendered =
                gio::spawn_blocking(move || save_clipboard_image(&directory, &snapshot()))
                    .await
                    .unwrap_or_else(|_| {
                        Err(EditorError::ImageSave(crate::i18n::t(
                            "ApexShot lost contact with the copy worker.",
                        )))
                    })
                    .map_err(|error| error.to_string());
            copying_done.set(false);
            copy_btn_done.set_sensitive(true);
            let result = rendered.and_then(|snapshot| {
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
    let window_upload_gate = window.downgrade();
    let upload_retry = upload_btn.downgrade();
    upload_btn.connect_clicked(move |_| {
        if uploading_click.get() {
            return;
        }

        let image_capabilities = {
            let state = state_upload.lock().unwrap();
            crate::studio_entitlement::capabilities_for_image_export(
                state.static_image_transform,
                state.source_upscale.factor() > 1,
            )
        };
        let Some(window) = window_upload_gate.upgrade() else {
            return;
        };
        let retry = upload_retry.clone();
        if !crate::studio_upsell::gate(&window, image_capabilities, move || {
            if let Some(button) = retry.upgrade() {
                button.emit_clicked();
            }
        }) {
            return;
        }

        let config = crate::config::load_config();
        if !crate::cloud::upload::is_configured(&config) {
            let (title, body) = crate::cloud::upload::not_configured_notification(&config);
            crate::utils::notify::desktop_notification_important(&title, &body);
            return;
        }

        let snapshot = state_upload.lock().unwrap().export_snapshot();

        uploading_click.set(true);
        upload_btn_click.set_sensitive(false);

        let path = path_upload.clone();
        let upload_done_tx = upload_done_tx.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let state = snapshot();
                let final_image = match state.to_final_image() {
                    Ok(image) => image,
                    Err(error) => {
                        eprintln!("[editor] Failed to save edits before upload: {error}");
                        return Err(crate::cloud::upload::UploadError::Server(format!(
                            "Could not save edits: {error}"
                        )));
                    }
                };
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
            match std::fs::create_dir_all(snapshot_root)
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
            }
            }))
            .unwrap_or_else(|_| {
                Err(crate::cloud::upload::UploadError::Server(
                    "Could not save edits: the upload worker stopped unexpectedly.".into(),
                ))
            });
            let _ = upload_done_tx.send(result);
        });
    });

    let state_save = state.clone();
    let path_save = path.to_path_buf();
    let window_save = window.downgrade();
    let app_save = app.downgrade();
    let save_chooser: Rc<RefCell<Option<gtk4::FileChooserNative>>> = Rc::new(RefCell::new(None));
    let save_chooser_open = Rc::new(Cell::new(false));
    let export_busy = Rc::new(Cell::new(false));
    let export_spinner = save_spinner.clone();
    let export_btn = save_btn.clone();
    save_btn.connect_clicked(move |_| {
        let state_save = state_save.clone();
        let path_save = path_save.clone();
        let window_save = window_save.clone();
        let app_save = app_save.clone();
        let in_motion = in_motion.clone();
        let export_motion = export_motion.clone();
        let save_chooser = save_chooser.clone();
        let save_chooser_open = save_chooser_open.clone();
        let export_busy = export_busy.clone();
        let export_spinner = export_spinner.clone();
        let export_btn = export_btn.clone();
        glib::idle_add_local_once(move || {
            if export_busy.get() || save_chooser_open.get() {
                return;
            }

            let capabilities = if in_motion.get() {
                crate::studio_entitlement::capabilities_for_motion_export()
            } else {
                let state = state_save.lock().unwrap();
                crate::studio_entitlement::capabilities_for_image_export(
                    state.static_image_transform,
                    state.source_upscale.factor() > 1,
                )
            };
            let Some(gate_window) = window_save.upgrade() else {
                return;
            };
            let retry = export_btn.clone();
            if !crate::studio_upsell::gate(&gate_window, capabilities, move || retry.emit_clicked())
            {
                return;
            }

            let config = crate::config::load_config().sanitized();
            let close_after_export = config.annotate_close_after_export;
            let overwrite_original = config.annotate_overwrite_original_image;

            if in_motion.get() {
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
                let export_busy_resp = export_busy.clone();
                let export_spinner_resp = export_spinner.clone();
                let export_btn_resp = export_btn.clone();
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

                    if export_busy_resp.get() {
                        return;
                    }
                    start_export_controls(&export_busy_resp, &export_spinner_resp, &export_btn_resp);

                    let state_resp = state_resp.clone();
                    let path_resp = path_resp.clone();
                    let window_resp = window_resp.clone();
                    let app_resp = app_resp.clone();
                    let export_motion = export_motion.clone();
                    let export_busy_inner = export_busy_resp.clone();
                    let export_spinner_inner = export_spinner_resp.clone();
                    let export_btn_inner = export_btn_resp.clone();
                    glib::MainContext::default().spawn_local(async move {
                        match export_motion(&destination).await {
                            Ok(video_path) => {
                                let mut config = crate::config::load_config();
                                if config.remember_video_export_dir(&video_path) {
                                    let _ = crate::config::save_config(&config);
                                }
                                match persist_image_session(
                                    &path_resp,
                                    &state_resp.lock().unwrap(),
                                ) {
                                    Ok(()) => {
                                        crate::utils::notify::desktop_notification(
                                            &crate::i18n::t("Export complete"),
                                            &video_path.display().to_string(),
                                        );
                                        if close_after_export {
                                            if let Some(window) = window_resp.upgrade() {
                                                window.close();
                                            }
                                            if let Some(app) = app_resp.upgrade() {
                                                app.quit();
                                            }
                                        } else {
                                            reset_export_controls(
                                                &export_busy_inner,
                                                &export_spinner_inner,
                                                &export_btn_inner,
                                            );
                                        }
                                    }
                                    Err(error) => {
                                        crate::utils::notify::desktop_notification_important(
                                            &crate::i18n::t("Export complete"),
                                            &crate::i18n::tfmt(
                                                "Export completed, but the editing session could not be saved: {error}",
                                                &[("error", &error.to_string())],
                                            ),
                                        );
                                        reset_export_controls(
                                            &export_busy_inner,
                                            &export_spinner_inner,
                                            &export_btn_inner,
                                        );
                                    }
                                }
                            }
                            Err(error) => {
                                eprintln!("Failed to export Motion video: {error}");
                                crate::utils::notify::desktop_notification_important(
                                    &crate::i18n::t("Export failed"),
                                    &error,
                                );
                                reset_export_controls(
                                    &export_busy_inner,
                                    &export_spinner_inner,
                                    &export_btn_inner,
                                );
                            }
                        }
                    });
                });
                *save_chooser.borrow_mut() = Some(chooser.clone());
                chooser.show();
                return;
            }

            if !static_export_needs_chooser(&path_save, overwrite_original) {
                start_static_export(
                    state_save,
                    path_save.clone(),
                    path_save,
                    overwrite_original,
                    close_after_export,
                    window_save,
                    app_save,
                    export_busy,
                    export_spinner,
                    export_btn,
                );
                return;
            }

            let Some(parent) = window_save.upgrade() else {
                return;
            };
            let suggestion = edited_image_suggestion(&path_save, &config);
            save_chooser_open.set(true);
            let save_chooser_open_done = save_chooser_open.clone();
            let save_chooser_done = save_chooser.clone();
            let state_done = state_save.clone();
            let path_done = path_save.clone();
            let window_done = window_save.clone();
            let app_done = app_save.clone();
            let busy_done = export_busy.clone();
            let spinner_done = export_spinner.clone();
            let button_done = export_btn.clone();
            let chooser = static_export_chooser(&parent, &suggestion);
            chooser.connect_response(move |dialog, response| {
                dialog.hide();
                save_chooser_open_done.set(false);
                *save_chooser_done.borrow_mut() = None;

                let destination = (response == ResponseType::Accept)
                    .then(|| dialog.file())
                    .flatten()
                    .and_then(|file| file.path());
                let Some(destination) = destination else {
                    return;
                };
                if let Err(error) = ensure_static_export_destination(
                    &path_done,
                    &destination,
                    overwrite_original,
                ) {
                    notify_static_export_error(error);
                    return;
                }

                start_static_export(
                    state_done.clone(),
                    path_done.clone(),
                    destination,
                    overwrite_original,
                    close_after_export,
                    window_done.clone(),
                    app_done.clone(),
                    busy_done.clone(),
                    spinner_done.clone(),
                    button_done.clone(),
                );
            });
            *save_chooser.borrow_mut() = Some(chooser.clone());
            chooser.show();
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

fn static_export_needs_chooser(path: &Path, overwrite_original: bool) -> bool {
    !overwrite_original || is_unsaved_capture(path)
}

fn is_unsaved_capture(path: &Path) -> bool {
    crate::capture::unsaved::UnsavedCaptureStore::app_owned().owns(path)
}

fn edited_image_suggestion(source: &Path, config: &crate::config::AppConfig) -> PathBuf {
    let suggestion = if is_unsaved_capture(source) {
        let store = crate::capture::unsaved::UnsavedCaptureStore::app_owned();
        let output_dir = crate::capture::save_dialog::save_dialog_folder(config)
            .filter(|directory| !directory.starts_with(store.dir()))
            .or_else(|| {
                crate::capture::SaveConfig::default()
                    .get_output_dir()
                    .ok()
                    .filter(|directory| !directory.starts_with(store.dir()))
            })
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(std::env::temp_dir));
        let format = crate::capture::ImageFormat::from_setting(&config.screenshot_format);
        output_dir.join(crate::capture::generate_filename(
            &crate::capture::SaveConfig::default().with_format(format),
        ))
    } else {
        let parent = source
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let stem = source
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("ApexShot");
        let extension = source
            .extension()
            .and_then(|extension| extension.to_str())
            .filter(|extension| !extension.is_empty())
            .unwrap_or("png");
        parent.join(format!("{stem}-edited.{extension}"))
    };

    collision_free_suggestion(suggestion)
}

fn collision_free_suggestion(suggestion: PathBuf) -> PathBuf {
    if !suggestion.exists() {
        return suggestion;
    }

    let parent = suggestion
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = suggestion
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("ApexShot");
    let extension = suggestion
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty());
    let mut suffix = 1u64;

    loop {
        let filename = match extension {
            Some(extension) => format!("{stem}-{suffix}.{extension}"),
            None => format!("{stem}-{suffix}"),
        };
        let candidate = parent.join(filename);
        if !candidate.exists() {
            return candidate;
        }
        suffix += 1;
    }
}

fn static_export_chooser(parent: &ApplicationWindow, suggestion: &Path) -> gtk4::FileChooserNative {
    let chooser = gtk4::FileChooserNative::new(
        Some(&crate::i18n::t("Export")),
        Some(parent),
        FileChooserAction::Save,
        Some(&crate::i18n::t("Export")),
        Some(&crate::i18n::t("Cancel")),
    );
    let filter = FileFilter::new();
    filter.set_name(Some(&crate::i18n::t("Image files")));
    for extension in ["png", "jpg", "jpeg", "webp"] {
        filter.add_pattern(&format!("*.{extension}"));
    }
    chooser.add_filter(&filter);
    if let Some(folder) = suggestion.parent() {
        let _ = chooser.set_current_folder(Some(&gio::File::for_path(folder)));
    }
    if let Some(name) = suggestion.file_name() {
        chooser.set_current_name(&name.to_string_lossy());
    }
    chooser
}

#[derive(Debug)]
enum StaticExportError {
    Image(String),
    Session(String),
}

fn ensure_static_export_destination(
    source: &Path,
    destination: &Path,
    overwrite_original: bool,
) -> Result<(), StaticExportError> {
    if !overwrite_original && crate::utils::paths_alias(source, destination) {
        return Err(StaticExportError::Image(crate::i18n::t(
            "Choose a different destination to keep the original image unchanged.",
        )));
    }
    Ok(())
}

fn export_static_image(
    source: &Path,
    destination: &Path,
    state: &EditorState,
    overwrite_original: bool,
) -> Result<(), StaticExportError> {
    ensure_static_export_destination(source, destination, overwrite_original)?;
    save_edited_image(destination, state)
        .map_err(|error| StaticExportError::Image(error.to_string()))?;
    persist_image_session(source, state)
        .map_err(|error| StaticExportError::Session(error.to_string()))?;
    if !crate::utils::paths_alias(source, destination) {
        persist_image_session(destination, state)
            .map_err(|error| StaticExportError::Session(error.to_string()))?;
    }
    Ok(())
}

fn notify_static_export_error(error: StaticExportError) {
    match error {
        StaticExportError::Image(error) => {
            eprintln!("Failed to export edited image: {error}");
            crate::utils::notify::desktop_notification_important(
                &crate::i18n::t("Export failed"),
                &error,
            );
        }
        StaticExportError::Session(error) => {
            eprintln!("Image exported, but the editing session could not be saved: {error}");
            crate::utils::notify::desktop_notification_important(
                &crate::i18n::t("Export complete"),
                &crate::i18n::tfmt(
                    "Export completed, but the editing session could not be saved: {error}",
                    &[("error", &error)],
                ),
            );
        }
    }
}

fn reset_export_controls(busy: &Cell<bool>, spinner: &Spinner, button: &Button) {
    busy.set(false);
    spinner.stop();
    spinner.set_visible(false);
    button.set_label(&crate::i18n::t("Export"));
    button.set_sensitive(true);
}

fn start_export_controls(busy: &Cell<bool>, spinner: &Spinner, button: &Button) {
    busy.set(true);
    button.set_label(&crate::i18n::t("Exporting…"));
    button.set_sensitive(false);
    spinner.set_visible(true);
    spinner.start();
}

fn start_static_export(
    state: Arc<Mutex<EditorState>>,
    source: PathBuf,
    destination: PathBuf,
    overwrite_original: bool,
    close_after_export: bool,
    window: gtk4::glib::WeakRef<ApplicationWindow>,
    app: gtk4::glib::WeakRef<Application>,
    busy: Rc<Cell<bool>>,
    spinner: Spinner,
    button: Button,
) {
    if busy.get() {
        return;
    }
    start_export_controls(&busy, &spinner, &button);
    let snapshot = state.lock().unwrap().export_snapshot();
    glib::MainContext::default().spawn_local(async move {
        let worker_destination = destination.clone();
        let result = gio::spawn_blocking(move || {
            export_static_image(
                &source,
                &worker_destination,
                &snapshot(),
                overwrite_original,
            )
        })
        .await
        .unwrap_or_else(|_| {
            Err(StaticExportError::Image(crate::i18n::t(
                "ApexShot lost contact with the export worker.",
            )))
        });
        match result {
            Ok(()) => {
                crate::utils::notify::desktop_notification(
                    &crate::i18n::t("Export complete"),
                    &destination.display().to_string(),
                );
                if !finish_image_export(&destination, close_after_export, &window, &app) {
                    reset_export_controls(&busy, &spinner, &button);
                }
            }
            Err(error) => {
                let session_failure = matches!(&error, StaticExportError::Session(_));
                notify_static_export_error(error);
                if session_failure {
                    let _ = finish_image_export(&destination, false, &window, &app);
                }
                reset_export_controls(&busy, &spinner, &button);
            }
        }
    });
}

fn finish_image_export(
    path: &Path,
    close_after_export: bool,
    window: &gtk4::glib::WeakRef<ApplicationWindow>,
    app: &gtk4::glib::WeakRef<Application>,
) -> bool {
    let config = crate::config::load_config().sanitized();
    crate::daemon::copy_screenshot_to_clipboard(path, &config);
    if !close_after_export {
        return false;
    }

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
    true
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
    use super::{
        edited_image_suggestion, export_static_image, is_unsaved_capture,
        static_export_needs_chooser, with_mp4_extension, StaticExportError,
    };
    use crate::capture::editor::{
        state::EditorState,
        types::{AnnotationAction, DrawColor, Point},
    };
    use crate::config::AppConfig;
    use image::{Rgba, RgbaImage};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "apexshot-image-export-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn image_and_motion_exports_show_busy_feedback_without_blocking_the_main_loop() {
        let source = include_str!("output.rs");
        let source = source.split("#[cfg(test)]").next().unwrap();
        let controls = source.split("fn start_export_controls(").nth(1).unwrap();
        assert!(controls.contains("busy.set(true)"));
        assert!(controls.contains("Exporting…"));
        assert!(controls.contains("button.set_sensitive(false)"));
        assert!(controls.contains("spinner.start()"));
        assert!(source.contains("match export_motion(&destination).await"));
        let image = source.split("fn start_static_export(").nth(1).unwrap();
        assert!(image.contains("if busy.get()"));
        assert!(image.contains("gio::spawn_blocking(move ||"));
        assert!(image.contains("reset_export_controls(&busy, &spinner, &button)"));
    }

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
    fn editor_copy_renders_on_a_worker_and_restores_the_button_after_any_outcome() {
        let source = include_str!("output.rs");
        let handler = source
            .split("copy_btn.connect_clicked(move |_| {")
            .nth(1)
            .unwrap()
            .split("let path_upload")
            .next()
            .unwrap();
        let (on_gtk_thread, worker) = handler
            .split_once("gio::spawn_blocking(")
            .expect("copy render worker");
        assert!(on_gtk_thread.contains("if copying.get()"));
        assert!(on_gtk_thread.contains("export_snapshot()"));
        assert!(on_gtk_thread.contains("copy_btn_click.set_sensitive(false)"));
        assert!(!on_gtk_thread.contains("save_clipboard_image("));
        assert!(worker.contains("save_clipboard_image(&directory, &snapshot())"));
        let restore = worker
            .find("copy_btn_done.set_sensitive(true)")
            .expect("copy button restore");
        assert!(worker.find(".await").unwrap() < restore);
    }

    #[test]
    fn editor_upload_renders_on_its_worker_and_cannot_leave_the_button_busy() {
        let source = include_str!("output.rs");
        let handler = source
            .split("upload_btn.connect_clicked(move |_| {")
            .nth(1)
            .unwrap()
            .split("let state_save")
            .next()
            .unwrap();
        let (on_gtk_thread, worker) = handler
            .split_once("std::thread::spawn(")
            .expect("upload worker");
        assert!(on_gtk_thread.contains("export_snapshot()"));
        assert!(!on_gtk_thread.contains("to_final_image"));
        assert!(on_gtk_thread.contains("uploading_click.set(true)"));
        assert!(on_gtk_thread.contains("upload_btn_click.set_sensitive(false)"));
        assert!(worker.contains("catch_unwind"));
        assert!(worker.contains("state.to_final_image()"));
        assert!(worker.contains("upload_done_tx.send(result)"));
    }

    #[test]
    fn image_exports_use_a_chooser_unless_overwrite_is_enabled_and_the_source_is_saved() {
        let store = crate::capture::unsaved::UnsavedCaptureStore::app_owned();
        let unsaved = store.dir().join("unsaved-1.png");
        let saved = Path::new("/tmp/shot.png");

        assert!(is_unsaved_capture(&unsaved));
        assert!(static_export_needs_chooser(saved, false));
        assert!(static_export_needs_chooser(&unsaved, true));
        assert!(!static_export_needs_chooser(saved, true));
    }

    #[test]
    fn edited_image_suggestion_uses_a_collision_free_source_based_name() {
        let dir = scratch_dir("suggestion");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("capture.png");
        let first = dir.join("capture-edited.png");
        let second = dir.join("capture-edited-1.png");
        std::fs::write(&first, b"existing").unwrap();
        assert_eq!(
            edited_image_suggestion(&source, &AppConfig::default()),
            second
        );
        std::fs::write(&second, b"existing").unwrap();
        assert_eq!(
            edited_image_suggestion(&source, &AppConfig::default()),
            dir.join("capture-edited-2.png")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unsaved_capture_suggestion_uses_the_configured_export_folder_and_format() {
        let dir = scratch_dir("unsaved-suggestion");
        std::fs::create_dir_all(&dir).unwrap();
        let config = AppConfig {
            screenshot_export_location: dir.to_string_lossy().into_owned(),
            screenshot_format: "WebP".into(),
            ..AppConfig::default()
        };
        let store = crate::capture::unsaved::UnsavedCaptureStore::app_owned();
        let source = store.dir().join("unsaved-1.png");

        let suggestion = edited_image_suggestion(&source, &config);

        assert_eq!(suggestion.parent(), Some(dir.as_path()));
        assert_ne!(suggestion.parent(), Some(store.dir()));
        assert!(suggestion
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("ApexShot-"));
        assert_eq!(
            suggestion
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("webp")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn default_unsaved_capture_suggestion_is_outside_the_app_owned_store() {
        let store = crate::capture::unsaved::UnsavedCaptureStore::app_owned();
        let source = store.dir().join("unsaved-default.png");

        let suggestion = edited_image_suggestion(&source, &AppConfig::default());

        assert_ne!(suggestion.parent(), Some(store.dir()));
        assert!(suggestion
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("ApexShot-"));
        assert_eq!(
            suggestion
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("png")
        );
    }

    #[test]
    fn exporting_a_copy_preserves_the_source_and_reopens_editable_sessions_for_both_files() {
        let dir = scratch_dir("copy-session");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.png");
        let destination = dir.join("source-edited.png");
        let original = RgbaImage::from_pixel(12, 8, Rgba([40, 90, 180, 255]));
        original.save(&source).unwrap();
        let source_bytes = std::fs::read(&source).unwrap();
        let mut state = EditorState::new(original.clone());
        state.push_action(AnnotationAction::Line {
            start: Point { x: 1.0, y: 1.0 },
            end: Point { x: 8.0, y: 6.0 },
            color: DrawColor::new(1.0, 0.0, 0.0, 1.0),
            stroke_size: 2.0,
            shadow: false,
        });

        export_static_image(&source, &destination, &state, false).unwrap();

        assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
        let source_session = crate::annotations::load_annotations(&source)
            .unwrap()
            .expect("source session");
        let exported_session = crate::annotations::load_annotations(&destination)
            .unwrap()
            .expect("exported image session");
        assert_eq!(source_session.annotations.len(), 1);
        assert_eq!(exported_session.annotations.len(), 1);
        assert_eq!(
            crate::annotations::load_original_image(&destination)
                .unwrap()
                .expect("export original image"),
            original
        );
        assert_eq!(
            image::open(&destination).unwrap().to_rgba8(),
            state.to_final_image().unwrap()
        );
        assert_eq!(state.actions.len(), 1);

        std::fs::write(&destination, b"changed externally").unwrap();
        assert!(matches!(
            crate::annotations::load_annotations(&destination),
            Err(crate::annotations::AnnotationError::HashMismatch)
        ));
        assert!(crate::annotations::load_annotations(&source)
            .unwrap()
            .is_some());

        let _ = crate::annotations::delete_annotations(&source);
        let _ = crate::annotations::delete_annotations(&destination);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn default_static_export_rejects_the_opened_image_without_modifying_it() {
        let dir = scratch_dir("source-alias");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.png");
        let original = RgbaImage::from_pixel(12, 8, Rgba([40, 90, 180, 255]));
        original.save(&source).unwrap();
        let source_bytes = std::fs::read(&source).unwrap();
        let state = EditorState::new(original);

        let result = export_static_image(&source, &source, &state, false);

        assert!(matches!(result, Err(StaticExportError::Image(_))));
        assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
        let _ = crate::annotations::delete_annotations(&source);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn extensionless_export_uses_the_confirmed_path_without_overwriting_a_suffixed_neighbor() {
        let dir = scratch_dir("extensionless-destination");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.png");
        let selected_destination = dir.join("copy");
        let suffixed_neighbor = dir.join("copy.png");
        let original = RgbaImage::from_pixel(12, 8, Rgba([40, 90, 180, 255]));
        original.save(&source).unwrap();
        let source_bytes = std::fs::read(&source).unwrap();
        std::fs::write(&suffixed_neighbor, b"existing neighbor").unwrap();
        let neighbor_bytes = std::fs::read(&suffixed_neighbor).unwrap();
        let state = EditorState::new(original);

        export_static_image(&source, &selected_destination, &state, false).unwrap();

        assert!(selected_destination.is_file());
        assert_eq!(
            image::load_from_memory(&std::fs::read(&selected_destination).unwrap())
                .unwrap()
                .to_rgba8(),
            state.to_final_image().unwrap()
        );
        assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
        assert_eq!(std::fs::read(&suffixed_neighbor).unwrap(), neighbor_bytes);

        let _ = crate::annotations::delete_annotations(&source);
        let _ = crate::annotations::delete_annotations(&selected_destination);
        std::fs::remove_dir_all(dir).unwrap();
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
        let entitlement_position = upload_handler
            .find("studio_upsell::gate")
            .expect("direct-upload entitlement guard");
        let render_position = upload_handler
            .find("state.to_final_image()")
            .expect("flatten before direct upload");
        assert!(
            entitlement_position < render_position
                && save_position < upload_position
                && render_position < upload_position,
            "Image editor Upload must guard, flatten edits, and save before uploading the file"
        );
    }

    #[test]
    fn static_export_chooses_a_destination_before_saving_and_persists_both_sessions() {
        let source = include_str!("output.rs");
        let start = source
            .find("save_btn.connect_clicked(move |_| {")
            .expect("Export click handler");
        let handler = source[start..]
            .split("let window_close")
            .next()
            .expect("export handler");
        let chooser = handler
            .find("static_export_chooser(&parent, &suggestion)")
            .expect("native image chooser");
        assert!(
            !handler.contains("save_edited_image"),
            "the click handler must not flatten the source before destination selection"
        );
        assert!(!handler.contains("with_image_extension"));
        assert!(handler.contains("if !static_export_needs_chooser(&path_save, overwrite_original)"));
        assert!(handler
            .contains("let Some(destination) = destination else {\n                    return;"));
        let export = source
            .split("fn export_static_image(")
            .nth(1)
            .expect("image export implementation");
        let flatten = export
            .find("save_edited_image(destination, state)")
            .unwrap();
        let persist_source = export.find("persist_image_session(source, state)").unwrap();
        let persist_destination = export
            .find("persist_image_session(destination, state)")
            .unwrap();
        let response = chooser + handler[chooser..].find("chooser.connect_response").unwrap();
        let destination = response
            + handler[response..]
                .find("let Some(destination) = destination else")
                .unwrap();
        let start_export = response + handler[response..].find("start_static_export(").unwrap();
        assert!(chooser < response && response < destination && destination < start_export);
        assert!(flatten < persist_source && persist_source < persist_destination);
    }

    #[test]
    fn successful_static_exports_keep_the_editor_open_by_default_and_use_the_exported_path() {
        let source = include_str!("output.rs");
        let finish = source
            .split("fn finish_image_export(")
            .nth(1)
            .expect("image export completion")
            .split("fn motion_export_suggestion")
            .next()
            .expect("finish helper");
        let keep_open = finish.find("if !close_after_export").unwrap();
        let quick_access = finish.find("show_preview_via_daemon(path)").unwrap();
        assert!(keep_open < quick_access);
        assert!(finish.contains("copy_screenshot_to_clipboard(path, &config)"));
        assert!(finish.contains("arg(\"preview\").arg(path)"));
        assert!(source.contains("reset_export_controls(&busy, &spinner, &button)"));
        assert!(source.contains("if close_after_export"));
    }

    #[test]
    fn motion_export_obeys_the_close_preference_and_resets_after_success_when_kept_open() {
        let source = include_str!("output.rs");
        let motion = source
            .split("match export_motion(&destination)")
            .nth(1)
            .expect("Motion export completion")
            .split("Err(error) => {")
            .next()
            .expect("Motion export success branch");

        assert!(motion.contains("if close_after_export"));
        assert!(motion.contains("reset_export_controls"));
        assert!(motion.contains("window.close()"));
        assert!(motion.contains("app.quit()"));
    }
}
