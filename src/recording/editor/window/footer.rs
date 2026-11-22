use super::dialogs;
use super::export_flow;
use crate::recording::editor::ffmpeg;
use crate::recording::editor::model::{format_size, VideoEditState};
use gtk4::{
    gio, glib, prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, FileChooserAction,
    FileChooserNative, FileFilter, Image, Label, Orientation, ResponseType, Spinner,
};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::i18n::{t, tfmt};

pub(super) fn build_inspector_actions(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    exporting: Rc<Cell<bool>>,
    export_cancellation: Arc<AtomicBool>,
    export_flow_active: export_flow::FlowActive,
) -> GtkBox {
    let footer = GtkBox::new(Orientation::Horizontal, 6);
    footer.add_css_class("recording-editor-footer");
    footer.add_css_class("recording-editor-inspector-toolbar");
    footer.add_css_class("editor-sidebar-actions");
    footer.set_hexpand(true);

    let copy = icon_action_button(
        crate::capture::editor::window::icon_names::custom::COPY_SYMBOLIC,
        &t("Copy the recording path to the clipboard"),
    );
    let upload = icon_action_button(
        crate::capture::editor::window::icon_names::custom::CLOUD_OUTLINE_THIN_SYMBOLIC,
        &t("Export with your current settings, then upload"),
    );
    let done = Button::with_label(&t("Done"));
    done.set_has_frame(false);
    done.add_css_class("recording-editor-primary-button");
    done.set_halign(Align::End);
    done.set_tooltip_text(Some(&t("Export the edited MP4")));
    let spinner = Spinner::new();
    spinner.set_visible(false);

    let export_controls = vec![
        copy.clone().upcast::<gtk4::Widget>(),
        upload.clone().upcast::<gtk4::Widget>(),
        done.clone().upcast::<gtk4::Widget>(),
    ];

    copy.connect_clicked({
        let state = state.clone();
        move |_| {
            let path = state.lock().unwrap().metadata.path.clone();
            if let Err(err) = crate::utils::clipboard::copy_uri_to_clipboard(&path) {
                crate::utils::notify::desktop_notification(&t("Copy failed"), &err.to_string());
            } else {
                crate::utils::notify::desktop_notification(
                    &t("Copied"),
                    &t("Recording path copied"),
                );
            }
        }
    });

    wire_upload_button(
        &upload,
        window,
        state.clone(),
        export_controls.clone(),
        spinner.clone(),
        exporting.clone(),
        export_cancellation.clone(),
        export_flow_active.clone(),
    );
    wire_export_button(
        &done,
        window,
        state,
        export_controls,
        spinner.clone(),
        exporting,
        export_cancellation,
        export_flow_active,
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

pub(super) fn build_upload_action(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    exporting: Rc<Cell<bool>>,
    export_cancellation: Arc<AtomicBool>,
    export_flow_active: export_flow::FlowActive,
) -> (Button, Spinner) {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-title-upload");
    button.set_tooltip_text(Some(&t("Export with your current settings, then upload")));
    button.set_valign(Align::Center);
    let icon = Image::from_icon_name(
        crate::capture::editor::window::icon_names::custom::CLOUD_OUTLINE_THIN_SYMBOLIC,
    );
    icon.set_pixel_size(20);
    button.set_child(Some(&icon));

    let spinner = Spinner::new();
    spinner.set_size_request(12, 12);
    spinner.set_visible(false);
    spinner.set_valign(Align::Center);

    wire_upload_button(
        &button,
        window,
        state,
        vec![
            button.clone().upcast::<gtk4::Widget>(),
            spinner.clone().upcast(),
        ],
        spinner.clone(),
        exporting,
        export_cancellation,
        export_flow_active,
    );

    (button, spinner)
}

pub(super) fn build_export_action(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    exporting: Rc<Cell<bool>>,
    export_cancellation: Arc<AtomicBool>,
    export_flow_active: export_flow::FlowActive,
) -> (Button, Spinner) {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-primary-button");
    button.add_css_class("recording-editor-title-export");
    button.set_tooltip_text(Some(&t("Export the edited MP4")));
    button.set_valign(Align::Center);

    let label = Label::new(Some(&t("Export")));
    label.set_halign(Align::Center);
    label.set_valign(Align::Center);
    button.set_child(Some(&label));

    let spinner = Spinner::new();
    spinner.set_size_request(12, 12);
    spinner.set_visible(false);
    spinner.set_valign(Align::Center);

    wire_export_button(
        &button,
        window,
        state,
        vec![
            button.clone().upcast::<gtk4::Widget>(),
            spinner.clone().upcast(),
        ],
        spinner.clone(),
        exporting,
        export_cancellation,
        export_flow_active,
    );

    (button, spinner)
}

pub(super) fn update_estimate(label: &Label, state: &Arc<Mutex<VideoEditState>>, _trim_only: bool) {
    let state = state.lock().unwrap();
    let trim_only = !state.needs_reencode();
    label.set_text(&format!(
        "~{}",
        format_size(state.estimated_size_bytes(trim_only)),
    ));
    label.set_tooltip_text(Some(&t("Estimated export size")));
}

fn wire_upload_button(
    button: &Button,
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    controls: Vec<gtk4::Widget>,
    spinner: Spinner,
    exporting: Rc<Cell<bool>>,
    export_cancellation: Arc<AtomicBool>,
    export_flow_active: export_flow::FlowActive,
) {
    button.set_sensitive(state.lock().unwrap().has_source_video());
    let window = window.downgrade();
    button.connect_clicked(move |button| {
        if exporting.get() || export_flow_active.get() || !state.lock().unwrap().has_source_video()
        {
            return;
        }

        let state_snapshot = state.lock().unwrap().clone();
        let Some(gate_window) = window.upgrade() else {
            return;
        };
        let retry = button.clone();
        if !crate::studio_upsell::gate(
            &gate_window,
            crate::studio_entitlement::capabilities_for_video(&state_snapshot),
            move || retry.emit_clicked(),
        ) {
            return;
        }
        let was_trial = crate::studio_entitlement::studio_access(
            crate::studio_entitlement::capabilities_for_video(&state_snapshot),
        )
        .is_trial();

        let config = crate::config::load_config();
        if !crate::cloud::upload::is_configured(&config) {
            let (title, body) = crate::cloud::upload::not_configured_notification(&config);
            crate::utils::notify::desktop_notification(&title, &body);
            return;
        }

        exporting.set(true);
        export_cancellation.store(false, Ordering::Release);
        spinner.set_visible(true);
        spinner.start();
        for control in &controls {
            control.set_sensitive(false);
        }

        // Export with current editor settings first, then upload the result.
        let cancellation_worker = export_cancellation.clone();
        let (sender, receiver) =
            std::sync::mpsc::channel::<(Result<String, crate::cloud::upload::UploadError>, bool)>();
        std::thread::spawn(move || {
            let mut rendered = false;
            let result = (|| {
                if cancellation_worker.load(Ordering::Acquire) {
                    return Err(crate::cloud::upload::UploadError::Server(
                        "Export cancelled".into(),
                    ));
                }
                crate::studio_entitlement::ensure_export_allowed(
                    crate::studio_entitlement::capabilities_for_video(&state_snapshot),
                )
                .map_err(|error| {
                    crate::cloud::upload::UploadError::Server(format!(
                        "Export before upload failed: {error}"
                    ))
                })?;
                // Reuse the last export while every input that shaped it is
                // unchanged, so uploading costs no second encode.
                let cached_path = crate::recording::editor::export_cache::find(
                    &state_snapshot,
                    Some(&cancellation_worker),
                );
                if cancellation_worker.load(Ordering::Acquire) {
                    return Err(crate::cloud::upload::UploadError::Server(
                        "Export cancelled".into(),
                    ));
                }
                let path = if let Some(path) = cached_path {
                    path
                } else {
                    rendered = true;
                    ffmpeg::export_edited_to_cancellable(
                        &state_snapshot,
                        state_snapshot.export_path(),
                        Some(&cancellation_worker),
                    )
                    .map_err(|err| {
                        crate::cloud::upload::UploadError::Server(format!(
                            "Export before upload failed: {err}"
                        ))
                    })?
                };
                if cancellation_worker.load(Ordering::Acquire) {
                    return Err(crate::cloud::upload::UploadError::Server(
                        "Export cancelled".into(),
                    ));
                }
                crate::cloud::upload::upload_file(&config, &path).map(|result| result.share_url)
            })();
            let _ = sender.send((result, rendered));
        });

        let controls = controls.clone();
        let spinner = spinner.clone();
        let exporting = exporting.clone();
        let window = window.clone();
        let export_cancellation = export_cancellation.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            match receiver.try_recv() {
                Ok((result, rendered)) => {
                    exporting.set(false);
                    spinner.stop();
                    spinner.set_visible(false);
                    for control in &controls {
                        control.set_sensitive(true);
                    }
                    if export_cancellation.load(Ordering::Acquire) {
                        return glib::ControlFlow::Break;
                    }
                    match result {
                        Ok(share_url) => match window.upgrade() {
                            Some(window) => {
                                let copied_now =
                                    crate::utils::clipboard::copy_text_when_window_active(
                                        window.upcast_ref(),
                                        &share_url,
                                    );
                                let mut body = if copied_now {
                                    t("Share link copied to clipboard")
                                } else {
                                    t("Switch back to the editor to copy the link")
                                };
                                // A fresh export inside this upload already
                                // consumed one finished Studio output; a cached
                                // export was consumed when it was first written.
                                // The floating toast carries the trial count,
                                // so the notification only keeps it as a
                                // fallback when no overlay could host the card.
                                if rendered {
                                    let shown = was_trial
                                        && crate::studio_trial_toast::show(
                                            &window,
                                            crate::studio_trial::trial_remaining(),
                                        );
                                    if !shown {
                                        if let Some(text) =
                                            crate::studio_trial::finished_feedback(was_trial)
                                        {
                                            body = format!("{body}\n\n{text}");
                                        }
                                    }
                                }
                                crate::utils::notify::desktop_notification(
                                    &t("Upload complete"),
                                    &body,
                                );
                            }
                            None => crate::utils::notify::desktop_notification(
                                &t("Upload complete"),
                                &tfmt("Share link: {url}", &[("url", &share_url)]),
                            ),
                        },
                        Err(err) => {
                            if let crate::cloud::upload::UploadError::UpgradeRequired(message) =
                                &err
                            {
                                if let Some(window) = window.upgrade() {
                                    crate::cloud::upgrade::show_prompt(&window, message);
                                }
                            } else {
                                crate::utils::notify::desktop_notification(
                                    &t("Upload failed"),
                                    &err.to_string(),
                                );
                            }
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
                        &t("Upload failed"),
                        &t("ApexShot lost contact with the upload worker."),
                    );
                    glib::ControlFlow::Break
                }
            }
        });
    });
}

fn reserve_copy_export_path(state: &VideoEditState) -> Option<PathBuf> {
    let suggested = state.export_path();
    let dir = crate::config::load_config()
        .video_editor_export_dir(suggested.parent().unwrap_or_else(|| Path::new("")));
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!(
            "could not create the export folder {}: {err}",
            dir.display()
        );
        return None;
    }
    let stem = suggested
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "export".to_string());
    reserve_in_dir(&dir, &stem)
}

fn reserve_in_dir(dir: &Path, stem: &str) -> Option<PathBuf> {
    for index in 0..1000u32 {
        let name = if index == 0 {
            format!("{stem}.mp4")
        } else {
            format!("{stem}-{index}.mp4")
        };
        let candidate = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                drop(file);
                return Some(candidate);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                eprintln!("could not reserve {}: {err}", candidate.display());
                return None;
            }
        }
    }
    eprintln!("no free export name left under {}", dir.display());
    None
}

fn wire_export_button(
    button: &Button,
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    controls: Vec<gtk4::Widget>,
    spinner: Spinner,
    exporting: Rc<Cell<bool>>,
    export_cancellation: Arc<AtomicBool>,
    export_flow_active: export_flow::FlowActive,
) {
    let window = window.clone();
    let chooser_slot_click = Rc::new(RefCell::new(None::<FileChooserNative>));
    button.set_sensitive(state.lock().unwrap().has_source_video());
    button.connect_clicked(move |button| {
        if exporting.get()
            || export_flow_active.get()
            || chooser_slot_click.borrow().is_some()
            || !state.lock().unwrap().has_source_video()
        {
            return;
        }
        let state_snapshot = state.lock().unwrap().clone();
        let retry = button.clone();
        if !crate::studio_upsell::gate(
            &window,
            crate::studio_entitlement::capabilities_for_video(&state_snapshot),
            move || retry.emit_clicked(),
        ) {
            return;
        }

        let Some(flow) = export_flow::ActiveFlow::begin(&window, &export_flow_active) else {
            return;
        };
        export_cancellation.store(false, Ordering::Release);

        let state_actions = state.clone();
        let window_actions = window.clone();
        let controls_actions = controls.clone();
        let spinner_actions = spinner.clone();
        let exporting_actions = exporting.clone();
        let export_cancellation_actions = export_cancellation.clone();
        let chooser_slot_actions = chooser_slot_click.clone();
        let export_button_actions = button.clone();
        super::export_options::show(
            &window,
            state_snapshot,
            flow.clone(),
            Rc::new(move |export_state, action| match action {
                super::export_options::ExportAction::CopyToClipboard => {
                    let Some(path) = reserve_copy_export_path(&export_state) else {
                        export_flow::release(&flow);
                        dialogs::show_error(
                            &window_actions,
                            &t("Export failed"),
                            &t("ApexShot could not reserve a file for the export."),
                            None,
                        );
                        return;
                    };
                    start_card_export(
                        &window_actions,
                        &state_actions,
                        export_state,
                        path,
                        true,
                        flow.clone(),
                        &controls_actions,
                        &spinner_actions,
                        &exporting_actions,
                        &export_button_actions,
                        &export_cancellation_actions,
                    );
                }
                super::export_options::ExportAction::SaveToFile => {
                    choose_export_path(
                        &window_actions,
                        &state_actions,
                        export_state,
                        flow.clone(),
                        &controls_actions,
                        &spinner_actions,
                        &exporting_actions,
                        &export_button_actions,
                        &export_cancellation_actions,
                        &chooser_slot_actions,
                    );
                }
            }),
        );
    });
}

fn start_card_export(
    window: &ApplicationWindow,
    state: &Arc<Mutex<VideoEditState>>,
    export_state: VideoEditState,
    path: PathBuf,
    copy_to_clipboard: bool,
    flow: export_flow::FlowSlot,
    controls: &[gtk4::Widget],
    spinner: &Spinner,
    exporting: &Rc<Cell<bool>>,
    export_button: &Button,
    export_cancellation: &Arc<AtomicBool>,
) {
    state.lock().unwrap().export_settings = export_state.export_settings;
    let idle_label = export_button.label().unwrap_or_else(|| t("Export").into());
    exporting.set(true);
    export_button.set_label(&t("Exporting…"));
    spinner.set_visible(true);
    spinner.start();
    for control in controls {
        control.set_sensitive(false);
    }

    let controls = controls.to_vec();
    let spinner = spinner.clone();
    let exporting = exporting.clone();
    let export_button = export_button.clone();
    let restore = Rc::new(move || {
        export_button.set_label(&idle_label);
        spinner.stop();
        spinner.set_visible(false);
        for control in &controls {
            control.set_sensitive(true);
        }
    });

    super::export_card::start(
        window,
        export_state,
        path,
        copy_to_clipboard,
        flow,
        restore,
        exporting.clone(),
        export_cancellation.clone(),
    );
}

fn choose_export_path(
    window: &ApplicationWindow,
    state: &Arc<Mutex<VideoEditState>>,
    export_state: VideoEditState,
    flow: export_flow::FlowSlot,
    controls: &[gtk4::Widget],
    spinner: &Spinner,
    exporting: &Rc<Cell<bool>>,
    export_button: &Button,
    export_cancellation: &Arc<AtomicBool>,
    chooser_slot: &Rc<RefCell<Option<FileChooserNative>>>,
) {
    let suggested = export_state.export_path();
    let suggested_dir = crate::config::load_config()
        .video_editor_export_dir(suggested.parent().unwrap_or_else(|| Path::new("")));
    let title = t("Export video");
    let export = t("Export");
    let cancel = t("Cancel");
    let chooser = FileChooserNative::new(
        Some(&title),
        Some(window),
        FileChooserAction::Save,
        Some(&export),
        Some(&cancel),
    );
    let filter = FileFilter::new();
    filter.set_name(Some(&t("MP4 video")));
    filter.add_mime_type("video/mp4");
    filter.add_pattern("*.mp4");
    chooser.add_filter(&filter);
    if !suggested_dir.as_os_str().is_empty() {
        let _ = chooser.set_current_folder(Some(&gio::File::for_path(&suggested_dir)));
    }
    if let Some(name) = suggested.file_name() {
        chooser.set_current_name(&name.to_string_lossy());
    }

    let window = window.clone();
    let state = state.clone();
    let controls = controls.to_vec();
    let spinner = spinner.clone();
    let exporting = exporting.clone();
    let export_button = export_button.clone();
    let export_cancellation = export_cancellation.clone();
    let flow = flow.clone();
    let chooser_slot_response = Rc::downgrade(chooser_slot);
    let export_state = Rc::new(RefCell::new(Some(export_state)));
    chooser.connect_response(move |dialog, response| {
        dialog.hide();
        let selected_file = (response == ResponseType::Accept)
            .then(|| dialog.file())
            .flatten();
        if let Some(chooser_slot) = chooser_slot_response.upgrade() {
            chooser_slot.borrow_mut().take();
        }
        if response != ResponseType::Accept {
            export_flow::release(&flow);
            return;
        }
        let Some(mut path) = selected_file.and_then(|file| file.path()) else {
            export_flow::release(&flow);
            dialogs::show_error(
                &window,
                &t("Export failed"),
                &t("ApexShot could not export this recording."),
                Some("Choose a local file to export."),
            );
            return;
        };
        let is_mp4 = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"));
        if !is_mp4 {
            path.set_extension("mp4");
        }
        let Some(export_state) = export_state.borrow_mut().take() else {
            export_flow::release(&flow);
            return;
        };

        start_card_export(
            &window,
            &state,
            export_state,
            path,
            false,
            flow.clone(),
            &controls,
            &spinner,
            &exporting,
            &export_button,
            &export_cancellation,
        );
    });
    *chooser_slot.borrow_mut() = Some(chooser.clone());
    chooser.show();
}

#[cfg(test)]
mod tests {
    use super::reserve_in_dir;

    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        for index in 0u32..1000 {
            let candidate =
                std::env::temp_dir().join(format!("apexshot-{tag}-{}-{index}", std::process::id()));
            match std::fs::create_dir(&candidate) {
                Ok(()) => return candidate,
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => panic!("could not create {candidate:?}: {err}"),
            }
        }
        panic!("no free scratch dir for {tag}");
    }

    #[test]
    fn the_copy_reservation_never_overwrites_an_existing_file() {
        let dir = scratch_dir("reserve");
        std::fs::write(dir.join("clip.mp4"), b"keep me").unwrap();
        let first = reserve_in_dir(&dir, "clip").expect("a free name exists");
        assert_eq!(first, dir.join("clip-1.mp4"));
        assert!(first.exists());
        let second = reserve_in_dir(&dir, "clip").expect("another free name exists");
        assert_eq!(second, dir.join("clip-2.mp4"));
        assert_eq!(
            std::fs::read(dir.join("clip.mp4")).unwrap(),
            b"keep me",
            "the pre-existing export must be untouched",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_copy_reservation_fails_when_every_name_is_taken() {
        let dir = scratch_dir("reserve-full");
        for index in 0..1000u32 {
            let name = if index == 0 {
                "clip.mp4".to_string()
            } else {
                format!("clip-{index}.mp4")
            };
            std::fs::write(dir.join(name), b"").unwrap();
        }
        assert_eq!(reserve_in_dir(&dir, "clip"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn video_export_shows_busy_feedback_and_restores_it_on_completion_or_worker_failure() {
        let source = include_str!("footer.rs");
        let source = source.split("#[cfg(test)]").next().unwrap();
        let export = source.split("fn wire_export_button(").nth(1).unwrap();
        assert!(export.contains("if exporting.get()"));
        assert!(export.contains("export_flow_active.get()"));
        assert!(export.contains("export_button.set_label(&t(\"Exporting…\"))"));
        assert!(export.contains("export_button.set_label(&idle_label)"));
        assert!(export.contains("control.set_sensitive(false)"));
        let card = include_str!("export_card.rs");
        let hand_backs = card
            .matches("hand_back_to_editor(&handle, &restore, &flow)")
            .count();
        let releases = card.matches("export_flow::release(&flow)").count();
        let restores = card.matches("restore()").count();
        assert!(
            hand_backs >= 1 && releases >= 4 && restores >= 4,
            "success, failure, cancellation, disconnect, and Escape must all hand the editor back; found {hand_backs} hand-backs, {releases} releases, {restores} restores",
        );
        assert!(
            card.contains("connect_parent_notify")
                && card.contains(
                    "terminal.get() {
                hand_back_to_editor(&handle, &restore, &flow);"
                ),
            "an externally dismissed terminal screen still restores and releases",
        );
    }

    #[test]
    fn cloud_and_export_require_video_and_upload_applies_edits() {
        let source = include_str!("footer.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        for fn_name in ["fn wire_upload_button", "fn wire_export_button"] {
            let start = source.find(fn_name).expect(fn_name);
            assert!(
                source[start..].contains("has_source_video()"),
                "{fn_name} must no-op when no video is loaded",
            );
        }
        let upload = &source[source.find("fn wire_upload_button").expect("upload wire")..];
        let export_pos = upload.find("export_edited").expect("export current edits");
        let upload_pos = upload.find("upload_file").expect("cloud upload");
        assert!(
            export_pos < upload_pos,
            "Cloud must export current editor edits before uploading",
        );
        let export = &source[source.find("fn wire_export_button").expect("export wire")..];
        let options_pos = export
            .find("export_options::show")
            .expect("export options sheet");
        let save_pos = export.find("FileChooserAction::Save").expect("save dialog");
        assert!(
            options_pos < save_pos,
            "Export must show the options sheet before asking for a save location",
        );
        let chooser = &export[export
            .find("fn choose_export_path")
            .expect("chooser helper")..];
        let response_pos = chooser.find("connect_response").expect("chooser response");
        let card_pos = chooser
            .find("start_card_export")
            .expect("export card start");
        assert!(
            response_pos < card_pos,
            "Export must wait for the save chooser before starting the encode",
        );
        assert!(
            export.contains("chooser_slot.borrow_mut() = Some(chooser.clone())")
                && export.contains("Rc::downgrade(chooser_slot)")
                && export.contains("chooser_slot.borrow_mut().take()"),
            "The native save chooser must stay strongly owned until its response",
        );
        let card = include_str!("export_card.rs");
        assert!(
            card.contains("export_edited_to_cancellable") && card.contains("export_cancellation"),
            "The export worker must observe the editor cancellation token",
        );
    }

    /// Uploading reuses the render the editor already produced while every
    /// input that shaped it is unchanged, and must not delete that file when
    /// the upload is cancelled between the lookup and the transfer.
    #[test]
    fn upload_reuses_an_unchanged_render_without_deleting_it() {
        let source = include_str!("footer.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let upload = &source[source.find("fn wire_upload_button").expect("upload wire")..];
        let cache = upload
            .find("export_cache::find")
            .expect("cached export lookup");
        let entitlement = upload
            .find("ensure_export_allowed")
            .expect("Studio entitlement preflight");
        let render = upload
            .find("export_edited_to_cancellable")
            .expect("fresh render fallback");
        let network = upload.find("upload_file").expect("cloud upload");
        assert!(
            cache < render && render < network,
            "Upload must look for a reusable render before encoding, and only then upload",
        );
        assert!(
            entitlement < cache,
            "Upload must check the entitlement before reusing a rendered export",
        );
        assert!(
            !upload.contains("remove_file(&path)"),
            "A reused export must survive cancellation between lookup and upload",
        );
    }
}
