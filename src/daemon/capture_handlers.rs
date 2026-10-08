use std::sync::{Arc, Mutex};

use crate::{
    capture::{
        save_capture, save_existing_png,
        unsaved::{UnsavedCaptureStore, UNSAVED_CAPTURE_MAX_AGE},
        ImageFormat, SaveConfig,
    },
    capture_overlay::{
        begin_capture_session, capture_area_file_via_cpp, capture_crosshair_file_via_cpp,
        capture_screen_file_via_cpp, capture_still_via_portal, is_launch_blocked_error,
        open_quick_capture_via_cpp, request_existing_overlay_focus,
        user_facing_capture_failure_message, AreaCapturePathResult, CaptureDisplay,
        CaptureOverlayGuard, LaunchBlockedReason,
    },
    config::load_config,
    ocr::{extract_text, OcrConfig},
    recording::run_overlay_recording_request_with_gtk,
};
use anyhow::Context;

use super::*;

pub(super) fn screenshot_image_format(value: &str) -> ImageFormat {
    ImageFormat::from_setting(value)
}

pub(super) fn screenshot_save_config_from(app_config: &crate::config::AppConfig) -> SaveConfig {
    let mut save_config = SaveConfig::default()
        .with_format(screenshot_image_format(&app_config.screenshot_format))
        .with_cursor(app_config.screenshot_show_cursor);

    if !app_config.screenshot_export_location.is_empty() {
        save_config = save_config.with_output_dir(&app_config.screenshot_export_location);
    }

    save_config
}

pub(super) fn apply_screenshot_after_capture_actions(
    saved_path: std::path::PathBuf,
    state: Arc<Mutex<DaemonState>>,
    target_display: Option<CaptureDisplay>,
    unsaved_capture: bool,
) {
    let config = load_config().sanitized();
    state.lock().unwrap().last_capture_path = Some(saved_path.clone());

    let open_annotate = config.after_capture_open_annotate;
    let show_quick_access = config.after_capture_show_quick_access;

    // When auto-upload will put a share URL on the text clipboard, only copy the
    // image (not file:// URI). Otherwise the URI wins for a few seconds and
    // looks like "upload did nothing" when the user pastes.
    let defer_text_clipboard =
        crate::cloud::upload::should_defer_text_clipboard_to_share_url(&config);
    let configured_mode = crate::utils::clipboard::ScreenshotClipboardMode::from_config_value(
        &config.adv_clipboard_mode,
    );
    let mode = if unsaved_capture {
        configured_mode.for_unsaved_capture()
    } else {
        configured_mode
    };
    let unsaved_bitmap_only =
        unsaved_capture && mode == crate::utils::clipboard::ScreenshotClipboardMode::ImageOnly;
    let clip_path = saved_path.clone();
    let clip_config = config.clone();
    std::thread::spawn(move || {
        if !clip_config.after_capture_copy_file_to_clipboard {
            return;
        }
        if defer_text_clipboard {
            if let Err(e) = crate::utils::clipboard::copy_image_to_clipboard(&clip_path) {
                eprintln!("[daemon] Failed to copy screenshot image to clipboard: {e}");
            }
        } else if unsaved_bitmap_only {
            if let Err(e) = crate::utils::clipboard::copy_image_only_to_clipboard(&clip_path) {
                eprintln!("[daemon] Failed to copy screenshot image to clipboard: {e}");
            }
        } else {
            copy_screenshot_to_clipboard(&clip_path, &clip_config);
        }
    });

    // Upload when Cloud is configured and auto-upload is enabled (manual
    // Upload button on Quick Access still works for re-uploads).
    crate::cloud::upload::spawn_auto_upload_after_capture(saved_path.clone());

    if open_annotate {
        // Spawn editor as subprocess to avoid tokio runtime conflicts
        // The editor runs its own GTK main loop which doesn't work inside tokio
        spawn_editor_subprocess(saved_path.clone());
    }

    if show_quick_access {
        let child = show_preview_subprocess(saved_path, target_display);
        replace_preview_child(&state, child);
    }
}

pub fn copy_screenshot_to_clipboard(path: &std::path::Path, config: &crate::config::AppConfig) {
    if !config.after_capture_copy_file_to_clipboard {
        return;
    }
    let mode = crate::utils::clipboard::ScreenshotClipboardMode::from_config_value(
        &config.adv_clipboard_mode,
    );
    if let Err(e) = crate::utils::clipboard::copy_screenshot_with_mode(path, mode) {
        eprintln!("[daemon] Failed to copy screenshot to clipboard: {e}");
    }
}

pub(super) fn save_and_open(
    capture: crate::backend::CaptureData,
    state: Arc<Mutex<DaemonState>>,
) -> bool {
    let config = load_config().sanitized();

    if !config.after_capture_save {
        let store = unsaved_capture_store(&state);
        match save_capture(
            &capture,
            &unsaved_capture_save_config(screenshot_save_config_from(&config), store.dir()),
        ) {
            Ok(path) => {
                eprintln!(
                    "[daemon] Save is disabled; keeping capture at {}",
                    path.display()
                );
                run_unsaved_capture_actions(path, state, None);
                true
            }
            Err(e) => {
                notify_screenshot_capture_failed("Unsaved", &e);
                false
            }
        }
    } else {
        match save_capture(&capture, &screenshot_save_config_from(&config)) {
            Ok(path) => {
                let path: std::path::PathBuf = path;
                eprintln!("[daemon] Saved: {}", path.display());
                crate::usage_telemetry::record_screenshot();
                apply_screenshot_after_capture_actions(path, state, None, false);
                true
            }
            Err(e) => {
                eprintln!("[daemon] Save error: {e}");
                false
            }
        }
    }
}

/// Save config for a capture that lands in app-owned storage because the user
/// turned auto-save off: always lossless PNG, never the export folder.
pub(super) fn unsaved_capture_save_config(base: SaveConfig, dir: &std::path::Path) -> SaveConfig {
    base.with_format(ImageFormat::Png)
        .with_output_dir(dir)
        .with_prefix("unsaved")
}

fn unsaved_capture_store(state: &Arc<Mutex<DaemonState>>) -> UnsavedCaptureStore {
    let store = UnsavedCaptureStore::app_owned();
    store.clean_stale(last_capture_path(state).as_deref(), UNSAVED_CAPTURE_MAX_AGE);
    store
}

/// Telemetry and the normal after-capture actions for a capture that auto-save
/// did not write to the export folder, so Quick Access, annotate, the
/// clipboard, drag-and-drop and an auto-upload all work before the user
/// decides to save.
fn run_unsaved_capture_actions(
    path: std::path::PathBuf,
    state: Arc<Mutex<DaemonState>>,
    target_display: Option<CaptureDisplay>,
) {
    crate::usage_telemetry::record_screenshot();
    apply_screenshot_after_capture_actions(path, state, target_display, true);
}

pub(super) fn save_existing_png_and_open(path: std::path::PathBuf, state: Arc<Mutex<DaemonState>>) {
    save_existing_png_and_open_on_display(path, state, None);
}

fn save_existing_png_and_open_on_display(
    path: std::path::PathBuf,
    state: Arc<Mutex<DaemonState>>,
    target_display: Option<CaptureDisplay>,
) {
    let config = load_config().sanitized();
    if !config.after_capture_save {
        let store = unsaved_capture_store(&state);
        match store.keep(&path) {
            Ok(unsaved_path) => {
                eprintln!(
                    "[daemon] Save is disabled; keeping capture at {}",
                    unsaved_path.display()
                );
                run_unsaved_capture_actions(unsaved_path, state, target_display);
            }
            Err(err) => {
                eprintln!("[daemon] Could not store the unsaved capture: {err}");
                run_unsaved_capture_actions(path, state, target_display);
            }
        }
        return;
    }

    match save_existing_png(&path, &screenshot_save_config_from(&config)) {
        Ok(saved_path) => {
            eprintln!("[daemon] Saved: {}", saved_path.display());
            crate::usage_telemetry::record_screenshot();
            apply_screenshot_after_capture_actions(saved_path, state, target_display, false);
        }
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            eprintln!("[daemon] Save error: {e}");
        }
    }
}

pub(super) fn handle_import_web_scroll_capture(
    png_base64: String,
    page_url: String,
    page_title: String,
    state: Arc<Mutex<DaemonState>>,
) -> bool {
    use crate::backend::{CaptureData, PixelFormat};
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let decoded = match STANDARD.decode(png_base64.as_bytes()) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("[daemon] Web scroll import failed: invalid base64 payload: {e}");
            return false;
        }
    };

    let dyn_image = match image::load_from_memory(&decoded) {
        Ok(img) => img,
        Err(e) => {
            eprintln!("[daemon] Web scroll import failed: invalid image payload: {e}");
            return false;
        }
    };

    let rgba = dyn_image.to_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    let pixels = rgba.into_raw();

    if width == 0 || height == 0 {
        eprintln!("[daemon] Web scroll import failed: empty image");
        return false;
    }

    eprintln!(
        "[daemon] Importing web scroll capture ({}x{}, url={}, title={})",
        width, height, page_url, page_title
    );

    let capture = CaptureData::new(pixels, width, height, PixelFormat::RGBA32);
    save_and_open(capture, state)
}

pub(super) fn run_ocr_and_report(capture: crate::backend::CaptureData) {
    eprintln!("[daemon] OCR tool selected — extracting text from selected area...");

    let config = load_config().sanitized();
    let ocr_config = OcrConfig::default().with_language(&config.adv_ocr_language);

    match extract_text(&capture, &ocr_config) {
        Ok(result) => match &result.source {
            crate::ocr::ContentSource::QrCode => {
                eprintln!("[daemon] QR code decoded");
                crate::usage_telemetry::record_ocr();
                if result.copied_to_clipboard {
                    send_desktop_notification(
                        &crate::i18n::t("QR code decoded"),
                        &crate::i18n::t("URL copied to clipboard"),
                    );
                } else {
                    send_desktop_notification(
                        &crate::i18n::t("QR code decoded"),
                        &crate::i18n::t("Content extracted, but clipboard copy was unavailable"),
                    );
                }
            }
            crate::ocr::ContentSource::Ocr { confidence } => {
                eprintln!("[daemon] OCR successful (confidence: {}%)", confidence);
                crate::usage_telemetry::record_ocr();
                if result.copied_to_clipboard {
                    send_desktop_notification(
                        &crate::i18n::t("OCR complete"),
                        &crate::i18n::t("Text copied to clipboard"),
                    );
                } else {
                    send_desktop_notification(
                        &crate::i18n::t("OCR complete"),
                        &crate::i18n::t("Text extracted, but clipboard copy was unavailable"),
                    );
                }
            }
        },
        Err(err) => {
            eprintln!("[daemon] OCR failed: {err}");
            send_desktop_notification(&crate::i18n::t("OCR failed"), &err.to_string());
        }
    }
}

pub(super) fn last_capture_target(path: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    path.map(std::path::Path::to_path_buf)
}

pub(super) fn clipboard_missing_image_notification() -> (String, String) {
    (
        crate::i18n::t("Clipboard image unavailable"),
        crate::i18n::t("Clipboard does not contain an image to open"),
    )
}

pub(super) fn restore_recently_closed_target(
    path: Option<&std::path::Path>,
) -> Option<std::path::PathBuf> {
    last_capture_target(path)
}

pub(super) fn should_show_preview_after_toggle(
    preview_visible: bool,
    has_last_capture: bool,
) -> bool {
    !preview_visible && has_last_capture
}

pub(super) fn refresh_preview_child_state(state: &mut DaemonState) -> bool {
    match state.preview_child.as_mut() {
        Some(child) => match child.try_wait() {
            Ok(Some(_)) => {
                state.preview_child = None;
                false
            }
            Ok(None) => true,
            Err(_) => {
                state.preview_child = None;
                false
            }
        },
        None => false,
    }
}

pub(super) fn replace_preview_child(
    state: &Arc<Mutex<DaemonState>>,
    child: Option<std::process::Child>,
) {
    if let Ok(mut guard) = state.lock() {
        if let Some(mut existing) = guard.preview_child.take() {
            let _ = existing.kill();
            let _ = existing.wait();
        }
        guard.preview_child = child;
    }
}

pub(super) fn preview_visible(state: &Arc<Mutex<DaemonState>>) -> bool {
    state
        .lock()
        .map(|mut guard| refresh_preview_child_state(&mut guard))
        .unwrap_or(false)
}

pub(super) fn last_capture_path(state: &Arc<Mutex<DaemonState>>) -> Option<std::path::PathBuf> {
    state
        .lock()
        .ok()
        .and_then(|guard| guard.last_capture_path.clone())
}

pub(super) fn stop_preview_overlay(state: &Arc<Mutex<DaemonState>>) -> bool {
    let mut guard = match state.lock() {
        Ok(guard) => guard,
        Err(_) => return false,
    };

    if !refresh_preview_child_state(&mut guard) {
        return false;
    }

    if let Some(mut child) = guard.preview_child.take() {
        let _ = child.kill();
        let _ = child.wait();
        return true;
    }

    false
}

/// Ask for a destination for the last capture and write it there.
///
/// The chooser runs in its own GTK process so the daemon keeps its own context;
/// a second request while one is still open is ignored.
pub(super) fn handle_save_last_capture(state: Arc<Mutex<DaemonState>>) {
    let Some(path) = last_capture_path(&state) else {
        eprintln!("[daemon] No screenshot available to save.");
        notify_no_screenshot_to_save();
        return;
    };

    if !crate::capture::save_dialog::is_supported_screenshot(&path) || !path.exists() {
        eprintln!(
            "[daemon] Cannot save {}: not an available screenshot file",
            path.display()
        );
        notify_no_screenshot_to_save();
        return;
    }

    let mut guard = match state.lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    if let Some(child) = guard.save_capture_child.as_mut() {
        if child.try_wait().ok().flatten().is_none() {
            eprintln!("[daemon] A save dialog is already open.");
            return;
        }
        guard.save_capture_child = None;
    }

    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));
    match std::process::Command::new(&exe)
        .arg("save-capture-internal")
        .arg(&path)
        .spawn()
    {
        Ok(child) => guard.save_capture_child = Some(child),
        Err(error) => {
            guard.save_capture_child = None;
            eprintln!("[daemon] Failed to open the save dialog: {error}");
            send_desktop_notification(
                &crate::i18n::t("Screenshot not saved"),
                &crate::i18n::tfmt("Save failed: {message}", &[("message", &error.to_string())]),
            );
        }
    }
}

fn notify_no_screenshot_to_save() {
    send_desktop_notification(
        &crate::i18n::t("Screenshot not saved"),
        &crate::i18n::t("No screenshot is available to save"),
    );
}

pub(super) fn show_preview_for_path(
    path: std::path::PathBuf,
    state: &Arc<Mutex<DaemonState>>,
) -> bool {
    let child = show_preview_subprocess(path, None);
    let shown = child.is_some();
    replace_preview_child(state, child);
    shown
}

pub(super) fn toggle_preview_overlay(state: &Arc<Mutex<DaemonState>>) -> bool {
    let visible = preview_visible(state);
    let target = last_capture_path(state);

    if !should_show_preview_after_toggle(visible, target.is_some()) {
        return stop_preview_overlay(state);
    }

    target
        .map(|path| show_preview_for_path(path, state))
        .unwrap_or(false)
}

pub(super) fn import_clipboard_image_to_temp_png() -> anyhow::Result<std::path::PathBuf> {
    let mut clipboard = arboard::Clipboard::new().context("Failed to access clipboard")?;
    let image = clipboard
        .get_image()
        .context("Clipboard does not contain an image")?;

    let width = u32::try_from(image.width).context("Clipboard image width out of range")?;
    let height = u32::try_from(image.height).context("Clipboard image height out of range")?;
    let bytes = image.bytes.into_owned();
    let rgba = image::RgbaImage::from_raw(width, height, bytes)
        .context("Clipboard image has invalid RGBA data")?;

    let path = std::env::temp_dir().join(format!(
        "apexshot_clipboard_{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    rgba.save(&path)
        .with_context(|| format!("Failed to save clipboard image to {}", path.display()))?;
    Ok(path)
}

pub(super) fn send_desktop_notification(summary: &str, body: &str) {
    crate::utils::notify::desktop_notification(summary, body);
}

/// Surface a capture failure to the user. Failures are never silent.
/// Cancel / launch-blocked paths should not call this.
pub(super) fn notify_screenshot_capture_failed(context: &str, err: &impl std::fmt::Display) {
    let technical = err.to_string();
    eprintln!("[daemon] {context} capture failed: {technical}");
    let body = user_facing_capture_failure_message(&technical);
    send_desktop_notification(&crate::i18n::t("Screenshot failed"), &body);
}

/// Spawn `apexshot preview <path>` as a subprocess so it gets its own GTK context.
pub(super) fn show_preview_subprocess(
    path: std::path::PathBuf,
    target_display: Option<CaptureDisplay>,
) -> Option<std::process::Child> {
    match crate::preview_launch::spawn_preview_subprocess_on_display(&path, target_display) {
        Ok(child) => Some(child),
        Err(e) => {
            eprintln!(
                "[daemon] Failed to spawn preview subprocess: {e}, falling back to in-app preview"
            );
            crate::preview_launch::show_preview_direct_on_display(path, target_display);
            None
        }
    }
}

pub(super) fn show_settings_subprocess() {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));

    if let Err(e) = std::process::Command::new(&exe).arg("settings").spawn() {
        eprintln!("[daemon] Failed to spawn settings window: {e}");
    }
}

pub(super) fn spawn_history_subprocess() {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));

    if let Err(e) = std::process::Command::new(&exe).arg("history").spawn() {
        eprintln!("[daemon] Failed to spawn history window: {e}");
    }
}

/// Spawn the recording editor without an initial video.
pub(super) fn spawn_empty_video_editor_subprocess() {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));

    if let Err(e) = std::process::Command::new(&exe).arg("video-editor").spawn() {
        eprintln!("[daemon] Failed to spawn video editor: {e}");
    }
}

pub(super) fn spawn_empty_image_editor_subprocess() {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));

    if let Err(e) = std::process::Command::new(&exe).arg("image-editor").spawn() {
        eprintln!("[daemon] Failed to spawn image editor: {e}");
    }
}

/// Spawn `apexshot edit <path>` as a subprocess on desktops that need process
/// isolation. KDE Wayland opens the editor directly to avoid extra taskbar /
/// loading artifacts in Plasma.
pub(super) fn spawn_editor_subprocess(path: std::path::PathBuf) {
    if crate::preview_launch::should_use_direct_editor_launch() {
        if let Err(e) = crate::capture::open_image_editor(path) {
            eprintln!("[daemon] Failed to open editor directly: {e}");
        }
        return;
    }

    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));

    if let Err(e) = std::process::Command::new(&exe)
        .arg("edit")
        .arg(&path)
        .spawn()
    {
        eprintln!("[daemon] Failed to spawn editor subprocess: {e}");
    }
}

/// Hand a file to the desktop's default application.
///
/// Shared with the History window so "open in default app" behaves identically
/// whether it comes from the tray or from a history card.
pub fn open_file(path: std::path::PathBuf) -> Result<(), String> {
    crate::utils::open::open_path(&path)
}

pub(super) fn screenshot_timer_delay_duration(seconds: u32) -> Option<std::time::Duration> {
    if seconds == 0 {
        None
    } else {
        Some(std::time::Duration::from_secs(seconds as u64))
    }
}

pub(super) fn screenshot_timer_supported(action: &str) -> bool {
    matches!(action, "screen" | "window")
}

pub(super) fn apply_screenshot_timer_if_needed(
    action: &str,
    app_config: &crate::config::AppConfig,
) {
    if !screenshot_timer_supported(action) {
        return;
    }
    if let Some(delay) = screenshot_timer_delay_duration(app_config.screenshot_timer_interval) {
        std::thread::sleep(delay);
    }
}

pub(super) fn handle_capture_area(state: Arc<Mutex<DaemonState>>) {
    let Some(_session_guard) = acquire_capture_session_guard("area") else {
        return;
    };
    // Close any existing preview before starting capture (single-instance behavior)
    let _ = stop_preview_overlay(&state);
    handle_capture_area_with_active_session(state);
}

pub(super) fn handle_quick_capture(state: Arc<Mutex<DaemonState>>) {
    let Some(_session_guard) = acquire_capture_session_guard("quick-capture") else {
        return;
    };
    let _ = stop_preview_overlay(&state);
    let gtk_tx = state.lock().unwrap().gtk_tx.clone();
    let result = open_quick_capture_via_cpp().map_err(anyhow::Error::from);
    handle_interactive_capture_result(result, state, gtk_tx, "Quick Capture");
}

pub(super) fn handle_capture_crosshair(state: Arc<Mutex<DaemonState>>) {
    let Some(_session_guard) = acquire_capture_session_guard("crosshair") else {
        return;
    };
    // Close any existing preview before starting capture
    let _ = stop_preview_overlay(&state);
    handle_capture_crosshair_with_active_session(state);
}

/// Flatpak / portal-only still capture: Screenshot portal → save/open pipeline.
/// Does not invoke `apexshot-capture` (omitted from portal-only builds).
fn handle_portal_only_still_capture(
    state: Arc<Mutex<DaemonState>>,
    interactive: bool,
    label: &str,
) {
    match capture_still_via_portal(interactive) {
        Ok(capture) => {
            let _ = save_and_open(capture, state);
        }
        Err(crate::overlay::SelectionError::Cancelled) => {
            eprintln!("[daemon] {label} capture cancelled.");
        }
        Err(err) if is_launch_blocked_error(&err) => {
            eprintln!("[daemon] {label} capture blocked: {err}");
        }
        Err(err) => {
            notify_screenshot_capture_failed(label, &err);
        }
    }
}

pub(super) fn handle_capture_area_with_active_session(state: Arc<Mutex<DaemonState>>) {
    let app_config = load_config().sanitized();
    apply_screenshot_timer_if_needed("area", &app_config);

    if crate::capture_overlay::requires_portal_still_capture() {
        handle_portal_only_still_capture(state, true, "Area");
        return;
    }

    let gtk_tx = state.lock().unwrap().gtk_tx.clone();

    let cpp_area_init = if let Some(gtk_tx) = gtk_tx.clone() {
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(0);
        match gtk_tx.send(super::GtkWork::CaptureAreaInit { reply: reply_tx }) {
            Ok(()) => match reply_rx.recv() {
                Ok(result) => result.map_err(anyhow::Error::msg),
                Err(err) => Err(anyhow::anyhow!(
                    "GTK main-thread area-init reply failed: {err}"
                )),
            },
            Err(err) => Err(anyhow::anyhow!(
                "GTK main-thread area-init dispatch failed: {err}"
            )),
        }
    } else {
        capture_area_file_via_cpp().map_err(anyhow::Error::from)
    };

    handle_interactive_capture_result(cpp_area_init, state, gtk_tx, "Area");
}

fn handle_interactive_capture_result(
    result: anyhow::Result<AreaCapturePathResult>,
    state: Arc<Mutex<DaemonState>>,
    gtk_tx: Option<std::sync::mpsc::Sender<GtkWork>>,
    label: &str,
) {
    match result {
        Ok(AreaCapturePathResult::Captured(path)) => {
            save_existing_png_and_open(path, state);
        }
        Ok(AreaCapturePathResult::CapturedOnDisplay(path, display)) => {
            save_existing_png_and_open_on_display(path, state, Some(display));
        }
        Ok(AreaCapturePathResult::ScrollCaptured(path)) => {
            save_existing_png_and_open(path, state);
        }
        Ok(AreaCapturePathResult::OcrRequested(capture)) => {
            run_ocr_and_report(capture);
        }
        Ok(AreaCapturePathResult::Cancelled) => {
            eprintln!("[daemon] {label} cancelled.");
        }
        Ok(AreaCapturePathResult::RecordingConfigUpdated) => {
            eprintln!("[daemon] Recording overlay state updated.");
        }
        Ok(AreaCapturePathResult::RecordingRequested(request)) => {
            if let Err(err) = run_overlay_recording_request_with_gtk(request, gtk_tx.clone()) {
                if err
                    .downcast_ref::<crate::recording::RecordError>()
                    .is_some_and(|e| matches!(e, crate::recording::RecordError::Cancelled))
                {
                    eprintln!("[daemon] Recording cancelled by user.");
                    return;
                }
                eprintln!("[daemon] Recording failed: {err}");
                send_desktop_notification(&crate::i18n::t("Recording failed"), &err.to_string());
                // Show notification for GNOME extension not installed
                if err
                    .to_string()
                    .contains("GNOME Shell extension is not installed")
                {
                    send_desktop_notification(
                        &crate::i18n::t("Recording failed"),
                        &crate::i18n::t(
                            "GNOME Shell extension is not installed. Please install the ApexShot GNOME extension first.",
                        ),
                    );
                }
            }
        }
        Err(err) => {
            if err
                .downcast_ref::<crate::overlay::SelectionError>()
                .is_some_and(is_launch_blocked_error)
            {
                eprintln!("[daemon] {label} blocked: {err}");
                return;
            }
            notify_screenshot_capture_failed(label, &err);
        }
    }
}

pub(super) fn handle_capture_crosshair_with_active_session(state: Arc<Mutex<DaemonState>>) {
    let app_config = load_config().sanitized();
    apply_screenshot_timer_if_needed("crosshair", &app_config);

    if crate::capture_overlay::requires_portal_still_capture() {
        handle_portal_only_still_capture(state, true, "Crosshair");
        return;
    }

    let gtk_tx = state.lock().unwrap().gtk_tx.clone();

    let crosshair = if let Some(gtk_tx) = gtk_tx {
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(0);
        match gtk_tx.send(super::GtkWork::CaptureCrosshair { reply: reply_tx }) {
            Ok(()) => match reply_rx.recv() {
                Ok(result) => result.map_err(|err| match err.as_str() {
                    "cancelled" => crate::overlay::SelectionError::Cancelled,
                    other => crate::overlay::SelectionError::InitError(other.to_string()),
                }),
                Err(err) => Err(crate::overlay::SelectionError::InitError(format!(
                    "GTK main-thread crosshair reply failed: {err}"
                ))),
            },
            Err(err) => Err(crate::overlay::SelectionError::InitError(format!(
                "GTK main-thread crosshair dispatch failed: {err}"
            ))),
        }
    } else {
        capture_crosshair_file_via_cpp()
    };

    match crosshair {
        Ok(path) => {
            save_existing_png_and_open(path, state);
        }
        Err(err) if is_launch_blocked_error(&err) => {
            eprintln!("[daemon] Crosshair capture blocked: {err}");
        }
        Err(crate::overlay::SelectionError::Cancelled) => {
            eprintln!("[daemon] Crosshair capture cancelled.");
        }
        Err(err) => {
            notify_screenshot_capture_failed("Crosshair", &err);
        }
    }
}

pub(super) fn handle_capture_screen(state: Arc<Mutex<DaemonState>>) {
    let Some(_session_guard) = acquire_capture_session_guard("screen") else {
        return;
    };
    // Close any existing preview before starting capture
    let _ = stop_preview_overlay(&state);
    handle_capture_screen_with_active_session(state);
}

pub(super) fn handle_capture_screen_with_active_session(state: Arc<Mutex<DaemonState>>) {
    let app_config = load_config().sanitized();
    apply_screenshot_timer_if_needed("screen", &app_config);

    if crate::capture_overlay::requires_portal_still_capture() {
        handle_portal_only_still_capture(state, false, "Fullscreen");
        return;
    }

    let gtk_tx = state.lock().unwrap().gtk_tx.clone();

    let screen = if let Some(gtk_tx) = gtk_tx {
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(0);
        match gtk_tx.send(super::GtkWork::CaptureScreen { reply: reply_tx }) {
            Ok(()) => match reply_rx.recv() {
                Ok(result) => result.map_err(|err| match err.as_str() {
                    "cancelled" => crate::overlay::SelectionError::Cancelled,
                    other => crate::overlay::SelectionError::InitError(other.to_string()),
                }),
                Err(err) => Err(crate::overlay::SelectionError::InitError(format!(
                    "GTK main-thread screen reply failed: {err}"
                ))),
            },
            Err(err) => Err(crate::overlay::SelectionError::InitError(format!(
                "GTK main-thread screen dispatch failed: {err}"
            ))),
        }
    } else {
        capture_screen_file_via_cpp()
    };

    match screen {
        Ok(path) => {
            save_existing_png_and_open(path, state);
        }
        Err(err) if is_launch_blocked_error(&err) => {
            eprintln!("[daemon] Fullscreen capture blocked: {err}");
        }
        Err(crate::overlay::SelectionError::Cancelled) => {
            eprintln!("[daemon] Fullscreen capture cancelled.");
        }
        Err(err) => {
            notify_screenshot_capture_failed("Fullscreen", &err);
        }
    }
}

pub(super) fn handle_capture_window(_state: Arc<Mutex<DaemonState>>) {
    // Keep legacy direct window actions from silently turning into area capture.
    // Quick Capture > Shot > Window remains available through the GUI.
    eprintln!(
        "[daemon] Legacy direct window capture is disabled. Use Quick Capture > Shot > Window."
    );
}

pub(super) fn acquire_capture_session_guard(context: &str) -> Option<CaptureOverlayGuard<'static>> {
    match begin_capture_session() {
        Ok(guard) => Some(guard),
        Err(LaunchBlockedReason::ApexOverlayAlreadyActive) => {
            let refocused = request_existing_overlay_focus();
            eprintln!(
                "[daemon] Ignoring duplicate {context} request while ApexShot overlay is active (refocused={refocused})."
            );
            None
        }
        Err(LaunchBlockedReason::BuiltinOverlayActive) => {
            eprintln!(
                "[daemon] Refusing {context} request because the GNOME screenshot UI is active."
            );
            None
        }
    }
}
