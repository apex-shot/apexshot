//! Persistent tray daemon for ApexShot.
//!
//! A single long-running process that:
//!   1. Shows a system tray icon (via ksni / StatusNotifierItem)
//!   2. Listens for global hotkeys via GNOME Shell GrabAccelerators
//!   3. On hotkey or tray-menu trigger → runs capture + overlay IN-PROCESS
//!      (no subprocess spawn, no GTK cold-start delay)
//!
//! Because the daemon is launched once via its .desktop entry, GNOME Shell
//! trusts it — so `org.gnome.Shell.Screenshot` D-Bus calls succeed (~50 ms),
//! giving instant popup-free captures.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::Context;
use ashpd::desktop::{
    remote_desktop::{Axis, DeviceType, KeyState, RemoteDesktop},
    screencast::{CursorMode, Screencast, SourceType},
    PersistMode, Session,
};

use crate::{
    backend::DisplayBackend,
    capture::{copy_capture_uri_to_clipboard, save_capture, save_existing_png, SaveConfig},
    capture_overlay::{
        begin_capture_session, capture_area_file_via_cpp, capture_crosshair_file_via_cpp,
        capture_screen_file_via_cpp, capture_window_file_via_cpp, is_launch_blocked_error,
        open_recording_ui_via_cpp, request_existing_overlay_focus, AreaCapturePathResult,
        CaptureOverlayGuard, LaunchBlockedReason,
    },
    config::load_config,
    hotkeys::{
        accel_to_gnome, ensure_desktop_entry_pub, load_hotkey_config,
        sync_gnome_hotkeys_for_current_desktop, HotkeyBinding,
    },
    ocr::{extract_text, OcrConfig},
    recording::run_overlay_recording_request_with_gtk,
    tray::{spawn_tray, ApexShotTray, TrayAction},
};

// ─────────────────────────────────────────────────────────────────────────────
// Daemon action
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum DaemonAction {
    CaptureArea,
    CaptureCrosshair,
    CaptureScreen,
    CaptureWindow,
    OpenFile,
    OpenFromClipboard,
    RestoreRecentlyClosed,
    ToggleOverlays,
    RecordScreen,
    RecordArea,
    OpenRecordingUi,
    OpenVideoEditor,
    ToggleRecordingPause,
    StopRecordingSave,
    RestartRecording,
    DiscardRecording,
    ShowLastPreview,
    ShowPreviewForPath(std::path::PathBuf),
    OpenLastCapture,
    OpenSettings,
    SetTrayVisible(bool),
    RecordingSessionStarted,
    RecordingSessionPaused,
    RecordingSessionResumed,
    RecordingSessionRestarted,
    RecordingSessionEnded,
    RecordingTimerTick,
    RecordingWebcamMoved(f64, f64),
    SetHotkeySuppressed(bool),
    Quit,
}

impl From<TrayAction> for DaemonAction {
    fn from(a: TrayAction) -> Self {
        match a {
            TrayAction::CaptureArea => DaemonAction::CaptureArea,
            TrayAction::CaptureCrosshair => DaemonAction::CaptureCrosshair,
            TrayAction::CaptureScreen => DaemonAction::CaptureScreen,
            TrayAction::CaptureWindow => DaemonAction::CaptureWindow,
            TrayAction::OpenRecordingUi => DaemonAction::OpenRecordingUi,
            TrayAction::OpenVideoEditor => DaemonAction::OpenVideoEditor,
            TrayAction::RecordScreen => DaemonAction::RecordScreen,
            TrayAction::StopRecordingSave => DaemonAction::StopRecordingSave,
            TrayAction::ShowLastPreview => DaemonAction::ShowLastPreview,
            TrayAction::OpenLastCapture => DaemonAction::OpenLastCapture,
            TrayAction::OpenSettings => DaemonAction::OpenSettings,
            TrayAction::Quit => DaemonAction::Quit,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared state
// ─────────────────────────────────────────────────────────────────────────────

struct DaemonState {
    last_capture_path: Option<std::path::PathBuf>,
    preview_child: Option<std::process::Child>,
    /// Channel to send GTK work to the main OS thread. `None` when the daemon
    /// owns the main thread itself (legacy / test mode).
    gtk_tx: Option<std::sync::mpsc::Sender<GtkWork>>,
}

#[derive(Debug, Clone)]
struct RecordingTrayState {
    started_at: Instant,
    paused_total: Duration,
    paused_at: Option<Instant>,
}

impl RecordingTrayState {
    fn started() -> Self {
        Self {
            started_at: Instant::now(),
            paused_total: Duration::ZERO,
            paused_at: None,
        }
    }

    fn pause(&mut self) {
        if self.paused_at.is_none() {
            self.paused_at = Some(Instant::now());
        }
    }

    fn resume(&mut self) {
        if let Some(paused_at) = self.paused_at.take() {
            self.paused_total += paused_at.elapsed();
        }
    }

    fn restart(&mut self) {
        *self = Self::started();
    }

    fn elapsed(&self) -> Duration {
        let end = self.paused_at.unwrap_or_else(Instant::now);
        end.saturating_duration_since(self.started_at)
            .saturating_sub(self.paused_total)
    }

    fn elapsed_text(&self) -> String {
        let total_seconds = self.elapsed().as_secs();
        format!("{}:{:02}", total_seconds / 60, total_seconds % 60)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Main daemon entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Well-known D-Bus name that the daemon registers.
pub const DAEMON_BUS_NAME: &str = "org.apexshot.Daemon";
/// D-Bus object path.
pub const DAEMON_OBJECT_PATH: &str = "/org/apexshot/Daemon";
/// D-Bus interface.
pub const DAEMON_INTERFACE: &str = "org.apexshot.Daemon";

/// Current mic level (f64 bits stored as u64), updated by mic monitoring thread.
static MIC_LEVEL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0); // 0.0f64.to_bits()
/// Current system audio level (f64 bits stored as u64), updated by speaker monitoring thread.
static SPEAKER_LEVEL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// When true, hotkey activations are suppressed (e.g. during shortcut editing).
static HOTKEY_SUPPRESSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Returns true if hotkey activations are currently suppressed.
pub fn is_hotkey_suppressed() -> bool {
    HOTKEY_SUPPRESSED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Set hotkey suppression via D-Bus. Returns `true` if the daemon was found.
pub fn set_daemon_hotkey_suppressed(suppressed: bool) -> bool {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        return true;
    };
    let proxy = match zbus::blocking::Proxy::new(
        &conn,
        DAEMON_BUS_NAME,
        DAEMON_OBJECT_PATH,
        DAEMON_INTERFACE,
    ) {
        Ok(proxy) => proxy,
        Err(_) => return false,
    };

    proxy
        .call::<_, _, ()>("SetHotkeySuppressed", &(suppressed,))
        .is_ok()
}

/// Try to trigger an action on an already-running daemon via D-Bus.
/// Returns `true` if the daemon was found and the call succeeded.
pub async fn trigger_daemon_action(action: &str) -> bool {
    let Ok(conn) = zbus::Connection::session().await else {
        return false;
    };
    let proxy = match zbus::Proxy::new(&conn, DAEMON_BUS_NAME, DAEMON_OBJECT_PATH, DAEMON_INTERFACE)
        .await
    {
        Ok(p) => p,
        Err(_) => return false,
    };
    proxy
        .call::<_, _, ()>("Trigger", &(action.to_string(),))
        .await
        .is_ok()
}

pub async fn import_web_scroll_capture(
    png_base64: String,
    page_url: String,
    page_title: String,
) -> bool {
    let Ok(conn) = zbus::Connection::session().await else {
        return false;
    };
    let proxy = match zbus::Proxy::new(&conn, DAEMON_BUS_NAME, DAEMON_OBJECT_PATH, DAEMON_INTERFACE)
        .await
    {
        Ok(p) => p,
        Err(_) => return false,
    };

    proxy
        .call::<_, _, bool>(
            "ImportWebScrollCapture",
            &(png_base64, page_url, page_title),
        )
        .await
        .unwrap_or(false)
}

pub fn set_daemon_tray_visibility(visible: bool) -> bool {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        return false;
    };
    let proxy = match zbus::blocking::Proxy::new(
        &conn,
        DAEMON_BUS_NAME,
        DAEMON_OBJECT_PATH,
        DAEMON_INTERFACE,
    ) {
        Ok(proxy) => proxy,
        Err(_) => return false,
    };

    proxy
        .call::<_, _, ()>("SetTrayVisible", &(visible,))
        .is_ok()
}

fn trigger_daemon_action_blocking(action: &str) -> bool {
    if tokio::runtime::Handle::try_current().is_ok() {
        let action = action.to_string();
        return std::thread::spawn(move || trigger_daemon_action_blocking(&action))
            .join()
            .unwrap_or(false);
    }

    let Ok(conn) = zbus::blocking::Connection::session() else {
        return false;
    };
    let proxy = match zbus::blocking::Proxy::new(
        &conn,
        DAEMON_BUS_NAME,
        DAEMON_OBJECT_PATH,
        DAEMON_INTERFACE,
    ) {
        Ok(proxy) => proxy,
        Err(_) => return false,
    };

    proxy
        .call::<_, _, ()>("Trigger", &(action.to_string(),))
        .is_ok()
}

pub fn notify_daemon_recording_started() -> bool {
    trigger_daemon_action_blocking("recording_session_started")
}

pub fn notify_daemon_recording_paused() -> bool {
    trigger_daemon_action_blocking("recording_session_paused")
}

pub fn notify_daemon_recording_resumed() -> bool {
    trigger_daemon_action_blocking("recording_session_resumed")
}

pub fn notify_daemon_recording_restarted() -> bool {
    trigger_daemon_action_blocking("recording_session_restarted")
}

pub fn notify_daemon_recording_ended() -> bool {
    trigger_daemon_action_blocking("recording_session_ended")
}

/// Tell the daemon to show preview for a specific path.
/// This ensures single-instance behavior (daemon will close existing preview first).
/// Returns true if the daemon was successfully notified.
pub fn show_preview_via_daemon(path: &std::path::Path) -> bool {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        return false;
    };
    let proxy = match zbus::blocking::Proxy::new(
        &conn,
        DAEMON_BUS_NAME,
        DAEMON_OBJECT_PATH,
        DAEMON_INTERFACE,
    ) {
        Ok(proxy) => proxy,
        Err(_) => return false,
    };

    let path_str = path.to_string_lossy().to_string();
    proxy
        .call::<_, _, ()>("show_preview_for_path", &(path_str,))
        .is_ok()
}

pub fn stop_daemon_via_dbus() -> bool {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        return false;
    };
    let proxy = match zbus::blocking::Proxy::new(
        &conn,
        DAEMON_BUS_NAME,
        DAEMON_OBJECT_PATH,
        DAEMON_INTERFACE,
    ) {
        Ok(proxy) => proxy,
        Err(_) => return false,
    };

    proxy
        .call::<_, _, ()>("Trigger", &("quit".to_string(),))
        .is_ok()
}

pub fn start_daemon_subprocess() -> anyhow::Result<()> {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));
    std::process::Command::new(&exe)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("Failed to spawn daemon subprocess via {}", exe.display()))?;
    Ok(())
}

fn spawn_daemon_tray(
    action_tx: &std::sync::mpsc::Sender<DaemonAction>,
) -> anyhow::Result<ksni::Handle<ApexShotTray>> {
    let (tray_tx, tray_rx) = std::sync::mpsc::channel::<TrayAction>();
    let tray_action_tx = action_tx.clone();
    std::thread::spawn(move || {
        while let Ok(action) = tray_rx.recv() {
            let _ = tray_action_tx.send(DaemonAction::from(action));
        }
    });
    spawn_tray(tray_tx).context("Failed to spawn tray icon")
}

fn update_tray_recording_state(
    tray_handle: &Option<ksni::Handle<ApexShotTray>>,
    recording_state: Option<&RecordingTrayState>,
) {
    let Some(handle) = tray_handle else {
        return;
    };

    handle.update(|tray| {
        if let Some(state) = recording_state {
            tray.show_recording_timer(state.elapsed_text());
        } else {
            tray.show_idle();
        }
    });
}

/// A request for GTK work that must run on the main OS thread.
/// The daemon sends these through a channel; the main thread executes them.
pub enum GtkWork {
    SelectAreaLive {
        reply: std::sync::mpsc::SyncSender<crate::overlay::SelectionResult>,
    },
    CaptureAreaInit {
        reply: std::sync::mpsc::SyncSender<Result<AreaCapturePathResult, String>>,
    },
    RunRecordingControls {
        params: crate::recording::RecordingControlsParams,
        stop_tx: tokio::sync::oneshot::Sender<crate::recording::StopAction>,
    },
    RunCountdown {
        seconds: u32,
        params: Option<crate::recording::RecordingControlsParams>,
        reply: std::sync::mpsc::SyncSender<()>,
    },
    SelectArea {
        capture: crate::backend::CaptureData,
        reply: std::sync::mpsc::SyncSender<Option<crate::overlay::SelectionArea>>,
    },
}

/// Entry point used when GTK runs on the main OS thread.
/// The daemon sends `GtkWork` items through `gtk_tx`; the caller's main thread
/// executes them and sends results back via the embedded reply channels.
pub async fn run_daemon_with_gtk_channel(
    gtk_tx: std::sync::mpsc::Sender<GtkWork>,
    _layer_shell_supported: bool,
) -> anyhow::Result<()> {
    run_daemon_inner(Some(gtk_tx)).await
}

pub async fn run_daemon() -> anyhow::Result<()> {
    run_daemon_inner(None).await
}

/// Ensure ydotoold daemon is running for scroll capture on Wayland.
/// This is called at startup to ensure scroll functionality works.
fn ensure_ydotoold_running() {
    use std::process::Command;

    // Check if ydotoold is already running
    let output = Command::new("pgrep").arg("-x").arg("ydotoold").output();

    if let Ok(output) = output {
        if output.status.success() {
            eprintln!("[daemon] ydotoold is already running");
            return;
        }
    }

    // Try to start ydotoold daemon
    eprintln!("[daemon] Starting ydotoold daemon for scroll capture...");

    let result = Command::new("ydotoold")
        .arg("--socket-path=/tmp/.ydotool_socket")
        .arg("--socket-own=1000:1000")
        .spawn();

    match result {
        Ok(_) => {
            // Give it a moment to start
            std::thread::sleep(std::time::Duration::from_millis(500));
            eprintln!("[daemon] ydotoold started successfully");
        }
        Err(e) => {
            eprintln!("[daemon] Warning: Could not start ydotoold: {}", e);
            eprintln!("[daemon] Scroll capture may not work on Wayland without ydotoold");
        }
    }
}

fn should_autostart_ydotoold() -> bool {
    false
}

fn should_quit_on_sigint(recording_active: bool) -> bool {
    !recording_active
}

async fn run_daemon_inner(gtk_tx: Option<std::sync::mpsc::Sender<GtkWork>>) -> anyhow::Result<()> {
    eprintln!("[daemon] ApexShot daemon starting…");

    // Ensure XDG portal permissions are persisted so the user doesn't have
    // to re-approve screenshot/screencast access after reboot.
    crate::backend::portal_permissions::ensure_portal_permissions();

    if should_autostart_ydotoold() {
        ensure_ydotoold_running();
    }

    if maybe_relaunch_via_desktop() {
        return Ok(());
    }

    // ── SINGLE-INSTANCE CHECK ─────────────────────────────────────────────────
    // Try to register D-Bus name BEFORE any other initialization.
    // This prevents multiple daemons from running simultaneously.
    let dbus_conn = match zbus::Connection::session().await {
        Ok(conn) => conn,
        Err(e) => {
            anyhow::bail!("Failed to connect to session bus: {}", e);
        }
    };

    // Try to request the name - if another daemon is running, this will fail
    let name_result = dbus_conn
        .request_name_with_flags(
            DAEMON_BUS_NAME,
            zbus::fdo::RequestNameFlags::DoNotQueue | zbus::fdo::RequestNameFlags::ReplaceExisting,
        )
        .await;
    match name_result {
        Ok(_) => eprintln!("[daemon] D-Bus name '{}' registered.", DAEMON_BUS_NAME),
        Err(e) => {
            eprintln!(
                "[daemon] Another daemon is already running (D-Bus name taken): {}",
                e
            );
            return Ok(()); // Exit gracefully, another instance is running
        }
    }

    let state = Arc::new(Mutex::new(DaemonState {
        last_capture_path: None,
        preview_child: None,
        gtk_tx,
    }));

    // ── Early exit if tray icon is disabled ─────────────────────────────────
    // The daemon is primarily needed for the tray icon and hotkey listening.
    // If the user has disabled the tray icon, exit early to avoid wasting resources.
    let initial_config = load_config().sanitized();
    if !initial_config.show_menu_bar_icon {
        eprintln!("[daemon] Tray icon disabled by settings — exiting.");
        return Ok(());
    }

    // Main action channel — both tray and hotkeys send here.
    let (action_tx, action_rx) = std::sync::mpsc::channel::<DaemonAction>();

    // ── Tray icon ────────────────────────────────────────────────────────────
    let mut tray_requested_visible = load_config().sanitized().show_menu_bar_icon;
    let mut recording_tray_state: Option<RecordingTrayState> = None;
    let mut tray_handle = if tray_requested_visible {
        let handle = spawn_daemon_tray(&action_tx)?;
        eprintln!("[daemon] Tray icon active.");
        Some(handle)
    } else {
        eprintln!("[daemon] Tray icon disabled by settings.");
        None
    };

    {
        let tick_tx = action_tx.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            if tick_tx.send(DaemonAction::RecordingTimerTick).is_err() {
                break;
            }
        });
    }

    {
        let signal_tx = action_tx.clone();
        tokio::spawn(async move {
            loop {
                if tokio::signal::ctrl_c().await.is_err() {
                    break;
                }

                let recording_active = crate::recording::has_active_recording_control();
                if should_quit_on_sigint(recording_active) {
                    eprintln!("[daemon] SIGINT received while idle; quitting daemon.");
                    if signal_tx.send(DaemonAction::Quit).is_err() {
                        break;
                    }
                } else {
                    eprintln!("[daemon] SIGINT ignored while recording is active.");
                }
            }
        });
    }

    // ── D-Bus IPC server ─────────────────────────────────────────────────────
    let dbus_conn_clone = dbus_conn.clone();
    let dbus_state = state.clone();
    let dbus_tx = action_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = run_dbus_server(dbus_conn_clone, dbus_tx, dbus_state).await {
            eprintln!("[daemon] D-Bus server error: {e}");
        }
    });

    // ── Hotkey listener ──────────────────────────────────────────────────────
    // On GNOME, hotkeys are handled via gsettings custom keybindings that
    // spawn `apexshot capture area` etc., which relay to us via D-Bus IPC.
    // We do NOT use the portal GlobalShortcuts here because it grabs keys
    // exclusively and prevents gsd-media-keys from grabbing the same keys.
    // Portal listener is only started as a last resort on non-GNOME desktops.
    let gnome_session = std::env::var_os("GNOME_SETUP_DISPLAY").is_some()
        || std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_ascii_uppercase()
            .contains("GNOME");

    if gnome_session {
        eprintln!("[daemon] GNOME detected — validating custom keybindings for D-Bus hotkeys.");
        match sync_gnome_hotkeys_for_current_desktop(None) {
            Ok(result) if result.updated => {
                eprintln!("[daemon] GNOME hotkeys refreshed for the current executable path.");
                for issue in result.issues {
                    eprintln!("[daemon]   repaired: {issue}");
                }
            }
            Ok(_) => {
                eprintln!("[daemon] GNOME hotkeys already point at the current executable.");
            }
            Err(e) => {
                eprintln!(
                    "[daemon] GNOME hotkeys are not active: {e}. Run `cargo run -- hotkeys install` to repair them."
                );
            }
        }
    } else {
        let hotkey_tx = action_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = run_hotkey_listener(hotkey_tx).await {
                eprintln!("[daemon] Hotkey listener error: {e}");
            }
        });
    }

    if gnome_session {
        eprintln!("[daemon] Ready. Tray active; GNOME hotkeys use custom keybindings + D-Bus IPC.");
    } else {
        eprintln!("[daemon] Ready. Listening for hotkeys and tray events.");
    }

    // ── Action loop ──────────────────────────────────────────────────────────
    while let Ok(action) = action_rx.recv() {
        let state_clone = state.clone();
        let action_tx_clone = action_tx.clone();
        match action {
            DaemonAction::CaptureArea => {
                tokio::task::spawn_blocking(move || handle_capture_area(state_clone));
            }
            DaemonAction::CaptureCrosshair => {
                tokio::task::spawn_blocking(move || handle_capture_crosshair(state_clone));
            }
            DaemonAction::CaptureScreen => {
                tokio::task::spawn_blocking(move || handle_capture_screen(state_clone));
            }
            DaemonAction::CaptureWindow => {
                tokio::task::spawn_blocking(move || handle_capture_window(state_clone));
            }
            DaemonAction::OpenFile => {
                if let Some(path) = last_capture_target(last_capture_path(&state_clone).as_deref())
                {
                    tokio::task::spawn_blocking(move || open_file(path));
                } else {
                    eprintln!("[daemon] No capture yet.");
                }
            }
            DaemonAction::OpenFromClipboard => {
                tokio::task::spawn_blocking(move || match import_clipboard_image_to_temp_png() {
                    Ok(path) => save_existing_png_and_open(path, state_clone),
                    Err(err) => {
                        eprintln!("[daemon] Clipboard import failed: {err}");
                        let (summary, body) = clipboard_missing_image_notification();
                        send_desktop_notification(summary, body);
                    }
                });
            }
            DaemonAction::RestoreRecentlyClosed => {
                if let Some(path) =
                    restore_recently_closed_target(last_capture_path(&state_clone).as_deref())
                {
                    tokio::task::spawn_blocking(move || {
                        let _ = show_preview_for_path(path, &state_clone);
                    });
                } else {
                    eprintln!("[daemon] No capture available to restore.");
                }
            }
            DaemonAction::ToggleOverlays => {
                tokio::task::spawn_blocking(move || {
                    if !toggle_preview_overlay(&state_clone) {
                        eprintln!("[daemon] No preview overlay available to toggle.");
                    }
                });
            }
            DaemonAction::RecordScreen => {
                tokio::spawn(handle_record_screen(action_tx_clone));
            }
            DaemonAction::RecordArea => {
                tokio::spawn(handle_record_area(action_tx_clone));
            }
            DaemonAction::OpenRecordingUi => {
                tokio::spawn(handle_open_recording_ui(action_tx_clone));
            }
            DaemonAction::OpenVideoEditor => {
                tokio::task::spawn_blocking(spawn_empty_video_editor_subprocess);
            }
            DaemonAction::ToggleRecordingPause => {
                if !crate::recording::toggle_active_recording_pause() {
                    eprintln!("[daemon] No active recording available for pause/resume.");
                }
            }
            DaemonAction::StopRecordingSave => {
                if !crate::recording::send_active_recording_command(
                    crate::recording::RecordingControlCommand::StopSave,
                ) {
                    eprintln!("[daemon] No active recording available for stop/save.");
                }
            }
            DaemonAction::RestartRecording => {
                if !crate::recording::send_active_recording_command(
                    crate::recording::RecordingControlCommand::Restart,
                ) {
                    eprintln!("[daemon] No active recording available for restart.");
                }
            }
            DaemonAction::DiscardRecording => {
                if !crate::recording::send_active_recording_command(
                    crate::recording::RecordingControlCommand::StopDiscard,
                ) {
                    eprintln!("[daemon] No active recording available for discard.");
                }
            }

            DaemonAction::ShowLastPreview => {
                let path = state.lock().unwrap().last_capture_path.clone();
                if let Some(p) = path {
                    tokio::task::spawn_blocking(move || {
                        let _ = show_preview_for_path(p, &state_clone);
                    });
                } else {
                    eprintln!("[daemon] No capture yet.");
                }
            }
            DaemonAction::ShowPreviewForPath(path) => {
                tokio::task::spawn_blocking(move || {
                    let _ = show_preview_for_path(path, &state_clone);
                });
            }
            DaemonAction::OpenLastCapture => {
                let path = state.lock().unwrap().last_capture_path.clone();
                if let Some(p) = path {
                    tokio::task::spawn_blocking(move || open_file(p));
                } else {
                    eprintln!("[daemon] No capture yet.");
                }
            }
            DaemonAction::OpenSettings => {
                tokio::task::spawn_blocking(show_settings_subprocess);
            }
            DaemonAction::SetTrayVisible(visible) => {
                tray_requested_visible = visible;
                if visible {
                    if tray_handle.is_none() {
                        match spawn_daemon_tray(&action_tx) {
                            Ok(handle) => {
                                tray_handle = Some(handle);
                                update_tray_recording_state(
                                    &tray_handle,
                                    recording_tray_state.as_ref(),
                                );
                                eprintln!("[daemon] Tray icon enabled live.");
                            }
                            Err(e) => {
                                eprintln!("[daemon] Failed to enable tray icon live: {e}");
                            }
                        }
                    }
                } else if recording_tray_state.is_none() {
                    if let Some(handle) = tray_handle.take() {
                        handle.shutdown();
                        eprintln!("[daemon] Tray icon disabled live.");
                    }
                }
            }
            DaemonAction::RecordingSessionStarted => {
                recording_tray_state = Some(RecordingTrayState::started());
                if tray_handle.is_none() {
                    match spawn_daemon_tray(&action_tx) {
                        Ok(handle) => tray_handle = Some(handle),
                        Err(e) => {
                            eprintln!("[daemon] Failed to show recording tray: {e}");
                        }
                    }
                }
                update_tray_recording_state(&tray_handle, recording_tray_state.as_ref());
            }
            DaemonAction::RecordingSessionPaused => {
                if let Some(state) = recording_tray_state.as_mut() {
                    state.pause();
                    update_tray_recording_state(&tray_handle, Some(state));
                }
            }
            DaemonAction::RecordingSessionResumed => {
                if let Some(state) = recording_tray_state.as_mut() {
                    state.resume();
                    update_tray_recording_state(&tray_handle, Some(state));
                }
            }
            DaemonAction::RecordingSessionRestarted => {
                if let Some(state) = recording_tray_state.as_mut() {
                    state.restart();
                    update_tray_recording_state(&tray_handle, Some(state));
                }
            }
            DaemonAction::RecordingSessionEnded => {
                recording_tray_state = None;
                if tray_requested_visible {
                    update_tray_recording_state(&tray_handle, None);
                } else if let Some(handle) = tray_handle.take() {
                    handle.shutdown();
                    eprintln!("[daemon] Recording tray removed.");
                }
            }
            DaemonAction::RecordingTimerTick => {
                if let Some(state) = recording_tray_state.as_ref() {
                    update_tray_recording_state(&tray_handle, Some(state));
                }
            }
            DaemonAction::RecordingWebcamMoved(x, y) => {
                let mut config = crate::config::load_config();
                config.rec_webcam_rel_x = x;
                config.rec_webcam_rel_y = y;
                let _ = crate::config::save_config(&config);
            }
            DaemonAction::SetHotkeySuppressed(suppressed) => {
                HOTKEY_SUPPRESSED.store(suppressed, std::sync::atomic::Ordering::Relaxed);
                eprintln!(
                    "[daemon] Hotkey suppression {}.",
                    if suppressed { "enabled" } else { "disabled" }
                );
            }
            DaemonAction::Quit => {
                eprintln!("[daemon] Quit requested.");
                break;
            }
        }
    }

    eprintln!("[daemon] Exiting.");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// D-Bus IPC server — exposes org.apexshot.Daemon with Trigger(action) method
// ─────────────────────────────────────────────────────────────────────────────

/// The D-Bus interface object. Holds a channel sender to forward actions to
/// the daemon's main loop.
struct PortalScrollSession {
    remote: RemoteDesktop<'static>,
    session: Session<'static, RemoteDesktop<'static>>,
    stream_node_id: Option<u32>,
    stream_pos: (i32, i32),
    stream_size: Option<(i32, i32)>,
}

#[derive(Default)]
struct ScrollInjector {
    portal: Option<PortalScrollSession>,
    focused: bool,
}

impl ScrollInjector {
    async fn begin(&mut self) -> Result<bool, String> {
        if self.portal.is_some() {
            return Ok(true);
        }

        let remote: RemoteDesktop<'static> = RemoteDesktop::new()
            .await
            .map_err(|e| format!("RemoteDesktop proxy init failed: {e}"))?;

        let screencast = Screencast::new()
            .await
            .map_err(|e| format!("Screencast proxy init failed: {e}"))?;

        let session: Session<'static, RemoteDesktop<'static>> = remote
            .create_session()
            .await
            .map_err(|e| format!("RemoteDesktop create_session failed: {e}"))?;

        remote
            .select_devices(
                &session,
                DeviceType::Pointer | DeviceType::Keyboard,
                None,
                PersistMode::DoNot,
            )
            .await
            .map_err(|e| format!("RemoteDesktop select_devices failed: {e}"))?;

        screencast
            .select_sources(
                &session,
                CursorMode::Hidden,
                SourceType::Monitor.into(),
                false,
                None,
                PersistMode::DoNot,
            )
            .await
            .map_err(|e| format!("Screencast select_sources failed: {e}"))?;

        let selected = remote
            .start(&session, None)
            .await
            .map_err(|e| format!("RemoteDesktop start request failed: {e}"))?
            .response()
            .map_err(|e| format!("RemoteDesktop start denied: {e}"))?;

        let (stream_node_id, stream_pos, stream_size) =
            if let Some(stream) = selected.streams().and_then(|streams| streams.first()) {
                (
                    Some(stream.pipe_wire_node_id()),
                    stream.position().unwrap_or((0, 0)),
                    stream.size(),
                )
            } else {
                (None, (0, 0), None)
            };

        self.portal = Some(PortalScrollSession {
            remote,
            session,
            stream_node_id,
            stream_pos,
            stream_size,
        });
        self.focused = false;
        eprintln!(
            "[daemon] RemoteDesktop scroll session started (stream={:?})",
            stream_node_id
        );
        Ok(true)
    }

    async fn step(&mut self, target_x: i32, target_y: i32, steps: i32) -> bool {
        if self.begin().await != Ok(true) {
            return false;
        }

        let Some(portal) = self.portal.as_ref() else {
            return false;
        };

        let mut ok = false;

        if let Some(stream_id) = portal.stream_node_id {
            let (sx, sy) = portal.stream_pos;
            let mut local_x = (target_x - sx).max(0) as f64;
            let mut local_y = (target_y - sy).max(0) as f64;
            if let Some((w, h)) = portal.stream_size {
                local_x = local_x.min((w.saturating_sub(1)) as f64);
                local_y = local_y.min((h.saturating_sub(1)) as f64);
            }

            if portal
                .remote
                .notify_pointer_motion_absolute(&portal.session, stream_id, local_x, local_y)
                .await
                .is_ok()
            {
                ok = true;
                let press_ok = portal
                    .remote
                    .notify_pointer_button(&portal.session, 272, KeyState::Pressed)
                    .await
                    .is_ok();
                let release_ok = portal
                    .remote
                    .notify_pointer_button(&portal.session, 272, KeyState::Released)
                    .await
                    .is_ok();
                self.focused = press_ok && release_ok;
                ok = ok || self.focused;
            }
        }

        let count = std::cmp::max(1, steps);
        for _ in 0..count {
            let axis_ok = portal
                .remote
                .notify_pointer_axis_discrete(&portal.session, Axis::Vertical, -1)
                .await
                .is_ok();

            let smooth_axis_ok = portal
                .remote
                .notify_pointer_axis(&portal.session, 0.0, 36.0, true)
                .await
                .is_ok();

            let keysym_ok = portal
                .remote
                .notify_keyboard_keysym(&portal.session, 0xFF56, KeyState::Pressed)
                .await
                .is_ok()
                && portal
                    .remote
                    .notify_keyboard_keysym(&portal.session, 0xFF56, KeyState::Released)
                    .await
                    .is_ok();

            let keycode_ok = portal
                .remote
                .notify_keyboard_keycode(&portal.session, 109, KeyState::Pressed)
                .await
                .is_ok()
                && portal
                    .remote
                    .notify_keyboard_keycode(&portal.session, 109, KeyState::Released)
                    .await
                    .is_ok();

            let down_keycode_ok = portal
                .remote
                .notify_keyboard_keycode(&portal.session, 108, KeyState::Pressed)
                .await
                .is_ok()
                && portal
                    .remote
                    .notify_keyboard_keycode(&portal.session, 108, KeyState::Released)
                    .await
                    .is_ok();

            eprintln!(
                "[daemon] portal scroll step: axis_ok={}, smooth_axis_ok={}, keysym_ok={}, keycode_ok={}, down_keycode_ok={}, focused={}, target=({}, {})",
                axis_ok,
                smooth_axis_ok,
                keysym_ok,
                keycode_ok,
                down_keycode_ok,
                self.focused,
                target_x,
                target_y
            );

            ok = ok || axis_ok || smooth_axis_ok || keysym_ok || keycode_ok || down_keycode_ok;
        }

        if !ok {
            self.end().await;
        }

        ok
    }

    async fn end(&mut self) {
        if let Some(portal) = self.portal.take() {
            let _ = portal.session.close().await;
            eprintln!("[daemon] RemoteDesktop scroll session ended");
        }
        self.focused = false;
    }
}

struct DaemonIpc {
    tx: std::sync::mpsc::Sender<DaemonAction>,
    state: Arc<Mutex<DaemonState>>,
    scroll_injector: tokio::sync::Mutex<ScrollInjector>,
}
async fn try_gnome_shell_capture_area(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<String, String> {
    let conn = zbus::Connection::session()
        .await
        .map_err(|e| format!("Session bus error: {e}"))?;

    let proxy = zbus::Proxy::new(
        &conn,
        "org.gnome.Shell",
        "/org/gnome/Shell/Screenshot",
        "org.gnome.Shell.Screenshot",
    )
    .await
    .map_err(|e| format!("GNOME Shell proxy error: {e}"))?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0);
    let requested_path = format!("/tmp/apexshot_gnome_{timestamp}.png");

    let (success, filename_used): (bool, String) = proxy
        .call(
            "ScreenshotArea",
            &(x, y, width, height, false, requested_path.clone()),
        )
        .await
        .map_err(|e| format!("GNOME Shell screenshot call failed: {e}"))?;

    if !success {
        return Err("ScreenshotArea returned success=false".into());
    }

    let resolved_path = if filename_used.trim().is_empty() {
        requested_path
    } else {
        filename_used
    };

    if !std::path::Path::new(&resolved_path).exists() {
        return Err(format!(
            "GNOME Shell screenshot output file missing: {resolved_path}"
        ));
    }

    Ok(resolved_path)
}

async fn try_gnome_shell_capture_fullscreen() -> Result<String, String> {
    let conn = zbus::Connection::session()
        .await
        .map_err(|e| format!("Session bus error: {e}"))?;

    let proxy = zbus::Proxy::new(
        &conn,
        "org.gnome.Shell",
        "/org/gnome/Shell/Screenshot",
        "org.gnome.Shell.Screenshot",
    )
    .await
    .map_err(|e| format!("GNOME Shell proxy error: {e}"))?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0);
    let requested_path = format!("/tmp/apexshot_gnome_fs_{timestamp}.png");

    let (success, filename_used): (bool, String) = proxy
        .call("Screenshot", &(false, false, requested_path.clone()))
        .await
        .map_err(|e| format!("GNOME Shell screenshot call failed: {e}"))?;

    if !success {
        return Err("Screenshot returned success=false".into());
    }

    let resolved_path = if filename_used.trim().is_empty() {
        requested_path
    } else {
        filename_used
    };

    if !std::path::Path::new(&resolved_path).exists() {
        return Err(format!(
            "GNOME Shell screenshot output file missing: {resolved_path}"
        ));
    }

    Ok(resolved_path)
}

#[zbus::interface(name = "org.apexshot.Daemon")]
impl DaemonIpc {
    fn trigger(&self, action: String) -> zbus::fdo::Result<()> {
        eprintln!("[daemon] D-Bus Trigger: {action}");
        let daemon_action = match action.as_str() {
            "capture_area" => DaemonAction::CaptureArea,
            "capture_crosshair" => DaemonAction::CaptureCrosshair,
            "capture_screen" => DaemonAction::CaptureScreen,
            "capture_window" => DaemonAction::CaptureWindow,
            "record_screen" => DaemonAction::RecordScreen,
            "record_area" => DaemonAction::RecordArea,
            "open_recording_ui" => DaemonAction::OpenRecordingUi,
            "open_video_editor" => DaemonAction::OpenVideoEditor,
            "recording_pause_resume" => DaemonAction::ToggleRecordingPause,
            "recording_stop_save" => DaemonAction::StopRecordingSave,
            "recording_restart" => DaemonAction::RestartRecording,
            "recording_discard" => DaemonAction::DiscardRecording,
            "recording_session_started" => DaemonAction::RecordingSessionStarted,
            "recording_session_paused" => DaemonAction::RecordingSessionPaused,
            "recording_session_resumed" => DaemonAction::RecordingSessionResumed,
            "recording_session_restarted" => DaemonAction::RecordingSessionRestarted,
            "recording_session_ended" => DaemonAction::RecordingSessionEnded,
            "open_file" => DaemonAction::OpenFile,
            "open_from_clipboard" => DaemonAction::OpenFromClipboard,
            "restore_recently_closed" => DaemonAction::RestoreRecentlyClosed,
            "toggle_overlays" => DaemonAction::ToggleOverlays,
            "show_last_preview" => DaemonAction::ShowLastPreview,
            "open_last" => DaemonAction::OpenLastCapture,
            "settings" => DaemonAction::OpenSettings,
            "quit" => DaemonAction::Quit,
            other => {
                eprintln!("[daemon] D-Bus Trigger: unknown action '{other}'");
                return Err(zbus::fdo::Error::InvalidArgs(format!(
                    "Unknown action: {other}"
                )));
            }
        };
        self.tx.send(daemon_action).map_err(|e| {
            zbus::fdo::Error::Failed(format!("Daemon action channel unavailable: {e}"))
        })?;
        Ok(())
    }

    fn move_webcam(&self, x: f64, y: f64) -> zbus::fdo::Result<()> {
        self.tx
            .send(DaemonAction::RecordingWebcamMoved(x, y))
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Daemon action channel unavailable: {e}"))
            })
    }

    /// Returns the current mic level as a normalized f64 (0.0 to 1.0).
    fn get_mic_level(&self) -> f64 {
        f64::from_bits(MIC_LEVEL.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Returns the current system audio level as a normalized f64 (0.0 to 1.0).
    fn get_speaker_level(&self) -> f64 {
        f64::from_bits(SPEAKER_LEVEL.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Allows the untrusted C++ overlay process to request an area screenshot
    /// via the trusted daemon on GNOME Wayland.
    async fn capture_area_gnome(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> zbus::fdo::Result<String> {
        eprintln!("[daemon] D-Bus capture_area_gnome called: {x} {y} {width} {height}");
        if width <= 0 || height <= 0 {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "Invalid capture area: {width}x{height}"
            )));
        }

        match try_gnome_shell_capture_area(x, y, width, height).await {
            Ok(path) => Ok(path),
            Err(shell_err) => {
                eprintln!(
                    "[daemon] GNOME Shell area screenshot failed ({shell_err}); trying backend fallback."
                );

                let fallback = tokio::task::spawn_blocking(move || {
                    capture_area_to_temp_png_path(x, y, width, height)
                })
                .await
                .map_err(|e| {
                    zbus::fdo::Error::Failed(format!("Area fallback task join error: {e}"))
                })?;

                fallback.map_err(|fallback_err| {
                    zbus::fdo::Error::Failed(format!(
                        "GNOME Shell area screenshot failed: {shell_err}; backend fallback failed: {fallback_err}"
                    ))
                })
            }
        }
    }

    /// Allows the untrusted C++ overlay process to request a fullscreen screenshot
    /// via the trusted daemon on GNOME Wayland.
    async fn capture_fullscreen_gnome(&self) -> zbus::fdo::Result<String> {
        eprintln!("[daemon] D-Bus capture_fullscreen_gnome called");
        match try_gnome_shell_capture_fullscreen().await {
            Ok(path) => Ok(path),
            Err(shell_err) => {
                eprintln!(
                    "[daemon] GNOME Shell fullscreen screenshot failed ({shell_err}); trying backend fallback."
                );

                let fallback = tokio::task::spawn_blocking(capture_fullscreen_to_temp_png_path)
                    .await
                    .map_err(|e| {
                        zbus::fdo::Error::Failed(format!(
                            "Fullscreen fallback task join error: {e}"
                        ))
                    })?;

                fallback.map_err(|fallback_err| {
                    zbus::fdo::Error::Failed(format!(
                        "GNOME Shell fullscreen screenshot failed: {shell_err}; backend fallback failed: {fallback_err}"
                    ))
                })
            }
        }
    }

    async fn scroll_begin_gnome(&self) -> zbus::fdo::Result<bool> {
        eprintln!("[daemon] D-Bus scroll_begin_gnome called");
        let mut injector = self.scroll_injector.lock().await;
        injector.begin().await.map_err(zbus::fdo::Error::Failed)
    }

    async fn scroll_step_gnome(&self, x: i32, y: i32, steps: i32) -> zbus::fdo::Result<bool> {
        let mut injector = self.scroll_injector.lock().await;
        Ok(injector.step(x, y, steps).await)
    }

    async fn scroll_end_gnome(&self) -> zbus::fdo::Result<()> {
        eprintln!("[daemon] D-Bus scroll_end_gnome called");
        let mut injector = self.scroll_injector.lock().await;
        injector.end().await;
        Ok(())
    }

    fn set_tray_visible(&self, visible: bool) -> zbus::fdo::Result<()> {
        eprintln!("[daemon] D-Bus SetTrayVisible: {visible}");
        self.tx
            .send(DaemonAction::SetTrayVisible(visible))
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Daemon action channel unavailable: {e}"))
            })?;
        Ok(())
    }

    fn set_hotkey_suppressed(&self, suppressed: bool) -> zbus::fdo::Result<()> {
        eprintln!("[daemon] D-Bus SetHotkeySuppressed: {suppressed}");
        self.tx
            .send(DaemonAction::SetHotkeySuppressed(suppressed))
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Daemon action channel unavailable: {e}"))
            })?;
        Ok(())
    }

    async fn import_web_scroll_capture(
        &self,
        png_base64: String,
        page_url: String,
        page_title: String,
    ) -> zbus::fdo::Result<bool> {
        eprintln!(
            "[daemon] D-Bus import_web_scroll_capture called (url={})",
            page_url
        );
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            handle_import_web_scroll_capture(png_base64, page_url, page_title, state)
        })
        .await
        .map_err(|e| zbus::fdo::Error::Failed(format!("Web capture import task failed: {e}")))
    }

    /// Show preview for a specific path (used by editor to coordinate single-instance)
    fn show_preview_for_path(&self, path: String) -> zbus::fdo::Result<()> {
        eprintln!("[daemon] D-Bus show_preview_for_path: {}", path);
        let path = std::path::PathBuf::from(path);
        self.tx
            .send(DaemonAction::ShowPreviewForPath(path))
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Daemon action channel unavailable: {e}"))
            })?;
        Ok(())
    }
}

fn start_audio_level_stream(
    label: &'static str,
    stream_name: &'static str,
    target: Option<&str>,
    capture_sink: bool,
    level: &'static std::sync::atomic::AtomicU64,
) {
    let target_owned = target.map(String::from);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));

        use pipewire as pw;
        use pw::{properties::properties, spa};
        use spa::param::format::{MediaSubtype, MediaType};
        use spa::param::format_utils;

        struct UserData {
            format: spa::param::audio::AudioInfoRaw,
        }

        pw::init();

        let mainloop = match pw::main_loop::MainLoopRc::new(None) {
            Ok(ml) => ml,
            Err(e) => {
                eprintln!("[daemon] PipeWire ({label}): failed to create main loop: {e}");
                return;
            }
        };

        let context = match pw::context::ContextRc::new(&mainloop, None) {
            Ok(ctx) => ctx,
            Err(e) => {
                eprintln!("[daemon] PipeWire ({label}): failed to create context: {e}");
                return;
            }
        };

        let core = match context.connect_rc(None) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[daemon] PipeWire ({label}): failed to connect core: {e}");
                return;
            }
        };

        let data = UserData {
            format: Default::default(),
        };

        let mut props = properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Production",
        };
        if let Some(ref target_name) = target_owned {
            props.insert("target.object", target_name.as_str());
        }
        if capture_sink {
            props.insert("stream.capture.sink", "true");
        }

        let stream = match pw::stream::StreamBox::new(&core, stream_name, props) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[daemon] PipeWire ({label}): failed to create stream: {e}");
                return;
            }
        };

        let _listener = stream
            .add_local_listener_with_user_data(data)
            .param_changed(move |_, user_data, id, param| {
                let Some(param) = param else { return };
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }
                let (media_type, media_subtype) = match format_utils::parse_format(param) {
                    Ok(v) => v,
                    Err(_) => return,
                };
                if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                    return;
                }
                user_data.format.parse(param).ok();
                eprintln!(
                    "[daemon] PipeWire ({label}): capturing rate={} channels={}",
                    user_data.format.rate(),
                    user_data.format.channels(),
                );
            })
            .process(move |stream, _user_data| {
                let mut buf = match stream.dequeue_buffer() {
                    Some(b) => b,
                    None => return,
                };
                let datas = buf.datas_mut();
                if datas.is_empty() {
                    return;
                }

                let mut peak: f32 = 0.0;
                for data in datas.iter_mut() {
                    let n_bytes = data.chunk().size() as usize;
                    if let Some(slice) = data.data() {
                        let ptr = slice.as_ptr() as *const f32;
                        if n_bytes >= std::mem::size_of::<f32>() {
                            let n_samples = n_bytes / std::mem::size_of::<f32>();
                            for j in 0..n_samples {
                                let s = unsafe { *ptr.add(j) }.abs();
                                if s > peak {
                                    peak = s;
                                }
                            }
                        }
                    }
                }

                let raw_level = if capture_sink {
                    // RMS averaging for system audio — gives natural, varied levels
                    let mut sum_sq: f64 = 0.0;
                    let mut count: u64 = 0;
                    for data in datas.iter_mut() {
                        let n_bytes = data.chunk().size() as usize;
                        if let Some(slice) = data.data() {
                            let ptr = slice.as_ptr() as *const f32;
                            if n_bytes >= std::mem::size_of::<f32>() {
                                let n_samples = n_bytes / std::mem::size_of::<f32>();
                                for j in 0..n_samples {
                                    let s = unsafe { *ptr.add(j) };
                                    sum_sq += (s * s) as f64;
                                    count += 1;
                                }
                            }
                        }
                    }
                    if count > 0 {
                        (sum_sq / count as f64).sqrt().clamp(0.0, 1.0) * 3.0
                    } else {
                        0.0
                    }
                } else {
                    // Peak detection for mic — responsive to voice
                    (peak * 2.0).clamp(0.0, 1.0) as f64
                };

                // Noise gate: ignore quiet audio to avoid picking up speaker bleed
                // Only applies to mic stream (not speaker/sink monitor)
                let gated = if !capture_sink {
                    if raw_level < 0.15 {
                        0.0
                    } else {
                        raw_level
                    }
                } else {
                    raw_level
                };

                level.store(gated.to_bits(), std::sync::atomic::Ordering::Relaxed);
            })
            .register();

        // Build audio format pod: F32LE, 44100Hz, mono
        let mut params: Vec<Vec<u8>> = Vec::new();
        {
            let mut audio_info = spa::param::audio::AudioInfoRaw::new();
            audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
            audio_info.set_rate(44100);
            audio_info.set_channels(1);

            let obj = spa::pod::Object {
                type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
                id: spa::param::ParamType::EnumFormat.as_raw(),
                properties: audio_info.into(),
            };

            let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
                std::io::Cursor::new(Vec::new()),
                &spa::pod::Value::Object(obj),
            )
            .unwrap()
            .0
            .into_inner();

            if spa::pod::Pod::from_bytes(&values).is_some() {
                params.push(values);
            }
        }

        let mut param_refs: Vec<&spa::pod::Pod> = params
            .iter()
            .filter_map(|bytes| spa::pod::Pod::from_bytes(bytes))
            .collect();

        match stream.connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut param_refs,
        ) {
            Ok(_) => eprintln!("[daemon] PipeWire ({label}) monitoring started."),
            Err(e) => {
                eprintln!("[daemon] PipeWire ({label}): failed to connect stream: {e}");
                return;
            }
        }

        mainloop.run();
    });
}

/// Detect the first physical (non-monitor) audio input device via pactl.
/// Returns `None` if no suitable device is found, letting PipeWire fall back
/// to the default input.
fn find_physical_input_device() -> Option<String> {
    let output = std::process::Command::new("pactl")
        .args(["list", "sources", "short"])
        .output()
        .ok()?;
    let stdout = String::from_utf8(output.stdout).ok()?;

    for line in stdout.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 2 {
            continue;
        }
        let name = fields[1].trim();
        // Monitor sources end with `.monitor` — skip them to avoid picking up
        // system audio loopback (which the speaker stream already captures).
        if name.ends_with(".monitor") {
            continue;
        }
        eprintln!("[daemon] PipeWire (mic): detected physical input device '{name}'");
        return Some(name.to_string());
    }
    eprintln!("[daemon] PipeWire (mic): no physical input device found; falling back to default");
    None
}

async fn run_dbus_server(
    conn: zbus::Connection,
    tx: std::sync::mpsc::Sender<DaemonAction>,
    state: Arc<Mutex<DaemonState>>,
) -> anyhow::Result<()> {
    let ipc = DaemonIpc {
        tx,
        state,
        scroll_injector: tokio::sync::Mutex::new(ScrollInjector::default()),
    };

    // Serve the IPC object on the existing connection (name already registered)
    conn.object_server().at(DAEMON_OBJECT_PATH, ipc).await?;

    // Mic: detect physical input device at runtime to avoid picking up system audio
    // Falls back to PipeWire default if no physical device is found
    let mic_target = find_physical_input_device();
    start_audio_level_stream(
        "mic",
        "apexshot-mic-monitor",
        mic_target.as_deref(),
        false,
        &MIC_LEVEL,
    );
    // Speaker: capture from sink monitor (digital tap of system audio output)
    start_audio_level_stream(
        "speaker",
        "apexshot-speaker-monitor",
        None,
        true,
        &SPEAKER_LEVEL,
    );

    eprintln!("[daemon] D-Bus IPC ready on {DAEMON_BUS_NAME}");
    std::future::pending::<()>().await;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Hotkey listener — tries GNOME Shell GrabAccelerators, then portal fallback
// ─────────────────────────────────────────────────────────────────────────────

async fn run_hotkey_listener(tx: std::sync::mpsc::Sender<DaemonAction>) -> anyhow::Result<()> {
    let (_config_path, cfg) = load_hotkey_config(None)?;
    if cfg.bindings.is_empty() {
        eprintln!("[daemon] No hotkey bindings configured.");
        return Ok(());
    }

    // Ensure GIO_LAUNCHED_DESKTOP_FILE is set so GNOME trusts us even when
    // launched from a terminal. The hotkeys module's ensure_desktop_entry()
    // writes the file; we just need to export the env vars.
    ensure_gio_desktop_env();

    // Tier 1: GNOME Shell GrabAccelerators (fast, no dialog, works on GNOME).
    match run_hotkey_listener_gnome_shell(&cfg, tx.clone()).await {
        Ok(()) => return Ok(()),
        Err(e) => {
            eprintln!("[daemon] GNOME Shell hotkeys unavailable ({e}), trying portal…");
        }
    }

    // Tier 2: XDG GlobalShortcuts portal (works on KDE, GNOME with portal, etc.)
    run_hotkey_listener_portal(&cfg, tx).await
}

/// Set GIO_LAUNCHED_DESKTOP_FILE env vars so the portal can identify us.
///
/// We point to the main app's desktop file so that xdg-desktop-portal
/// associates the daemon with `io.github.codegoddy.apexshot` — the same
/// app ID used in the PermissionStore by `ensure_portal_permissions()`.
/// Without this, the portal derives the app ID from the autostart desktop
/// file name (`apexshot`) which never matches, so permissions are never
/// found and the user is asked to approve every time.
///
/// The autostart desktop file has NoDisplay=true, so GNOME Shell won't
/// show a duplicate dock entry regardless of which desktop file we point to.
fn ensure_gio_desktop_env() {
    let desktop_path = if let Some(desktop_path) = crate::app_identity::desktop_file_for_portal() {
        desktop_path
    } else {
        let app_id = crate::app_identity::app_id();
        match ensure_desktop_entry_pub(app_id) {
            Ok(path) => path,
            Err(_) => return,
        }
    };

    if std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE").is_none() {
        std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", &desktop_path);
    }
    if std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE_PID").is_none() {
        std::env::set_var(
            "GIO_LAUNCHED_DESKTOP_FILE_PID",
            std::process::id().to_string(),
        );
    }
    eprintln!("[daemon] GIO desktop env set ({})", desktop_path.display());
}

fn is_gnome_desktop() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains("gnome")
}

fn maybe_relaunch_via_desktop() -> bool {
    if !is_gnome_desktop() {
        return false;
    }

    if std::env::var_os("APEXSHOT_DAEMON_DESKTOP_RELAUNCHED").is_some() {
        return false;
    }

    // Already desktop-launched (trusted GNOME context).
    if std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE").is_some() {
        return false;
    }

    let app_id = std::env::var("APEXSHOT_APP_ID")
        .unwrap_or_else(|_| crate::app_identity::app_id().to_string());

    let desktop_path = match ensure_desktop_entry_pub(&app_id) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("[daemon] Desktop relaunch skipped: could not ensure desktop entry: {err}");
            return false;
        }
    };

    let mut cmd = std::process::Command::new("gtk-launch");
    cmd.arg(&app_id)
        .env("APEXSHOT_DAEMON_DESKTOP_RELAUNCHED", "1");

    match cmd.spawn() {
        Ok(_) => {
            eprintln!(
                "[daemon] GNOME terminal launch detected; relaunched via desktop entry {} (app_id={app_id}).",
                desktop_path.display()
            );
            true
        }
        Err(err) => {
            eprintln!("[daemon] Desktop relaunch failed (continuing in terminal mode): {err}");
            false
        }
    }
}

/// Tier 1: GNOME Shell `GrabAccelerators` / `AcceleratorActivated`.
async fn run_hotkey_listener_gnome_shell(
    cfg: &crate::hotkeys::HotkeyConfig,
    tx: std::sync::mpsc::Sender<DaemonAction>,
) -> anyhow::Result<()> {
    use futures_util::StreamExt;
    use std::collections::HashMap;
    use zbus::zvariant::OwnedValue;

    let conn = zbus::Connection::session().await?;

    let shell = zbus::Proxy::new(
        &conn,
        "org.gnome.Shell",
        "/org/gnome/Shell",
        "org.gnome.Shell",
    )
    .await?;

    let grab_args: Vec<(String, u32, u32)> = cfg
        .bindings
        .iter()
        .map(|b| (accel_to_gnome(&b.accelerator), 15u32, 0u32))
        .collect();

    let action_ids: Vec<u32> = shell
        .call("GrabAccelerators", &(grab_args,))
        .await
        .context("GrabAccelerators call failed")?;

    let mut action_map: HashMap<u32, HotkeyBinding> = HashMap::new();
    for (idx, action_id) in action_ids.into_iter().enumerate() {
        if action_id != 0 {
            if let Some(binding) = cfg.bindings.get(idx) {
                action_map.insert(action_id, binding.clone());
            }
        }
    }

    if action_map.is_empty() {
        anyhow::bail!("GrabAccelerators returned no valid action IDs (all conflicts or refused)");
    }

    eprintln!(
        "[daemon] {} hotkey(s) registered via GNOME Shell.",
        action_map.len()
    );

    let match_rule = "type='signal',interface='org.gnome.Shell',member='AcceleratorActivated',path='/org/gnome/Shell'";
    let rule: zbus::MatchRule = match_rule.try_into()?;
    let mut stream = zbus::MessageStream::for_match_rule(rule, &conn, None).await?;

    while let Some(Ok(msg)) = stream.next().await {
        let Ok((action_id, _params)) = msg
            .body()
            .deserialize::<(u32, HashMap<String, OwnedValue>)>()
        else {
            continue;
        };

        if is_hotkey_suppressed() {
            eprintln!("[daemon] Hotkey suppressed (shortcut edit active)");
            continue;
        }

        if let Some(binding) = action_map.get(&action_id) {
            if let Some(act) = binding_to_daemon_action(binding) {
                eprintln!("[daemon] Hotkey fired: {:?}", act);
                let _ = tx.send(act);
            }
        }
    }

    Ok(())
}

/// Tier 2: XDG GlobalShortcuts portal.
/// Mirrors the working `run_portal_hotkey_daemon` in src/hotkeys/mod.rs exactly.
async fn run_hotkey_listener_portal(
    cfg: &crate::hotkeys::HotkeyConfig,
    tx: std::sync::mpsc::Sender<DaemonAction>,
) -> anyhow::Result<()> {
    use crate::hotkeys::{accel_to_portal, ensure_desktop_entry_pub};
    use futures_util::StreamExt;
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

    let app_id = std::env::var("APEXSHOT_APP_ID")
        .unwrap_or_else(|_| crate::app_identity::app_id().to_string());

    let conn = zbus::Connection::session()
        .await
        .context("Failed to connect to session D-Bus")?;

    // Register app_id with the portal so it can associate us with our .desktop file.
    let _ = ensure_desktop_entry_pub(&app_id);
    if let Err(e) = portal_register_app_id(&conn, &app_id).await {
        eprintln!("[daemon] Portal Registry.Register failed (continuing): {e}");
    }

    let portal = zbus::Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.GlobalShortcuts",
    )
    .await
    .context("GlobalShortcuts portal not available")?;

    // Helpers shared with hotkeys/mod.rs pattern.
    let sender_id = conn
        .unique_name()
        .ok_or_else(|| anyhow::anyhow!("No D-Bus unique name"))?
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_")
        .to_string();

    let mk_token = || {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        // Portal token charset: [A-Za-z0-9_] only.
        format!("apexshot_{pid}_{nanos}")
    };

    let mk_request_path = |tok: &str| -> anyhow::Result<OwnedObjectPath> {
        format!("/org/freedesktop/portal/desktop/request/{sender_id}/{tok}")
            .try_into()
            .context("Invalid portal request path")
    };

    // ── CreateSession ─────────────────────────────────────────────────────────
    let create_tok = mk_token();
    let session_tok = mk_token();
    let mut create_opts: HashMap<String, Value> = HashMap::new();
    create_opts.insert("handle_token".into(), Value::from(create_tok.clone()));
    create_opts.insert("session_handle_token".into(), Value::from(session_tok));

    let create_req_path = mk_request_path(&create_tok)?;
    // Subscribe BEFORE the call to avoid a race condition.
    let create_rule_str = format!(
        "type='signal',interface='org.freedesktop.portal.Request',member='Response',path='{}'",
        create_req_path.as_str()
    );
    let create_rule: zbus::MatchRule = create_rule_str.as_str().try_into()?;
    let mut create_stream =
        zbus::MessageStream::for_match_rule(create_rule, &conn, Some(1)).await?;

    let _req: OwnedObjectPath = portal
        .call("CreateSession", &(create_opts))
        .await
        .context("GlobalShortcuts.CreateSession failed")?;

    let (create_status, create_results) = {
        let msg = create_stream
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("No CreateSession response"))??;
        msg.body()
            .deserialize::<(u32, HashMap<String, OwnedValue>)>()
            .context("Failed to deserialize CreateSession response")?
    };
    if create_status != 0 {
        anyhow::bail!("CreateSession response={create_status}");
    }

    let session_handle_str: String = create_results
        .get("session_handle")
        .ok_or_else(|| anyhow::anyhow!("Missing session_handle in CreateSession response"))?
        .try_clone()
        .context("clone session_handle")?
        .try_into()
        .context("session_handle not a string")?;

    let session_handle: OwnedObjectPath = session_handle_str
        .try_into()
        .context("Invalid session_handle object path")?;

    eprintln!("[daemon] Portal session created.");

    // ── BindShortcuts ─────────────────────────────────────────────────────────
    let mut id_to_binding: HashMap<String, HotkeyBinding> = HashMap::new();
    let mut shortcuts: Vec<(String, HashMap<String, Value>)> = Vec::new();

    for (idx, binding) in cfg.bindings.iter().enumerate() {
        let id = binding
            .name
            .clone()
            .unwrap_or_else(|| format!("binding_{idx}"));
        let preferred_trigger = accel_to_portal(&binding.accelerator);
        let mut props: HashMap<String, Value> = HashMap::new();
        props.insert("description".into(), Value::from(id.replace('_', " ")));
        // Skip Print-based triggers — they're often reserved on desktops.
        if !preferred_trigger.to_ascii_uppercase().ends_with("PRINT") {
            props.insert("preferred_trigger".into(), Value::from(preferred_trigger));
        }
        shortcuts.push((id.clone(), props));
        id_to_binding.insert(id, binding.clone());
    }

    let bind_tok = mk_token();
    let mut bind_opts: HashMap<String, Value> = HashMap::new();
    bind_opts.insert("handle_token".into(), Value::from(bind_tok.clone()));

    let bind_req_path = mk_request_path(&bind_tok)?;
    let bind_rule_str = format!(
        "type='signal',interface='org.freedesktop.portal.Request',member='Response',path='{}'",
        bind_req_path.as_str()
    );
    let bind_rule: zbus::MatchRule = bind_rule_str.as_str().try_into()?;
    let mut bind_stream = zbus::MessageStream::for_match_rule(bind_rule, &conn, Some(1)).await?;

    let _bind_req: OwnedObjectPath = portal
        .call(
            "BindShortcuts",
            &(session_handle.clone(), shortcuts, "".to_string(), bind_opts),
        )
        .await
        .context("GlobalShortcuts.BindShortcuts failed")?;

    eprintln!("[daemon] Registering shortcuts with portal…");

    let (bind_status, _bind_results) = {
        let msg = bind_stream
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("No BindShortcuts response"))??;
        msg.body()
            .deserialize::<(u32, HashMap<String, OwnedValue>)>()
            .context("Failed to deserialize BindShortcuts response")?
    };
    match bind_status {
        0 => eprintln!(
            "[daemon] Portal shortcuts bound ({} shortcut(s)).",
            id_to_binding.len()
        ),
        1 => anyhow::bail!("BindShortcuts cancelled by user"),
        s => {
            // Status 2 can mean "shortcuts set but user may need to confirm in Settings".
            // This is non-fatal — activations will still be delivered.
            eprintln!("[daemon] BindShortcuts response={s} (non-fatal, continuing to listen).");
        }
    }

    // ── Listen for Activated signals ─────────────────────────────────────────
    let activated_rule = format!(
        "type='signal',interface='org.freedesktop.portal.GlobalShortcuts',member='Activated',path='{}'",
        session_handle.as_str()
    );
    let rule: zbus::MatchRule = activated_rule.as_str().try_into()?;
    let mut activated_stream = zbus::MessageStream::for_match_rule(rule, &conn, None).await?;

    eprintln!("[daemon] Listening for portal hotkey activations…");

    while let Some(Ok(msg)) = activated_stream.next().await {
        // Signal body: (o session_handle, s shortcut_id, u timestamp, a{sv} options)
        if let Ok((_session, shortcut_id, _ts, _opts)) =
            msg.body()
                .deserialize::<(OwnedObjectPath, String, u32, HashMap<String, OwnedValue>)>()
        {
            if is_hotkey_suppressed() {
                eprintln!("[daemon] Hotkey suppressed (shortcut edit active)");
                continue;
            }
            if let Some(binding) = id_to_binding.get(&shortcut_id) {
                if let Some(act) = binding_to_daemon_action(binding) {
                    eprintln!("[daemon] Portal hotkey fired: {:?}", act);
                    let _ = tx.send(act);
                }
            }
        }
    }

    Ok(())
}

/// Register this process's D-Bus peer with the portal's host Registry so it can
/// be associated with our app_id / .desktop file.
async fn portal_register_app_id(conn: &zbus::Connection, app_id: &str) -> anyhow::Result<()> {
    use std::collections::HashMap;
    use zbus::zvariant::Value;

    let registry = zbus::Proxy::new(
        conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.host.portal.Registry",
    )
    .await
    .context("Failed to create host Registry proxy")?;

    let opts: HashMap<String, Value> = HashMap::new();
    for attempt in 0..2u8 {
        let call: Result<(), zbus::Error> = registry
            .call("Register", &(app_id.to_string(), opts.clone()))
            .await;
        match call {
            Ok(()) => {
                eprintln!("[daemon] Portal: registered app_id={app_id}");
                return Ok(());
            }
            Err(e) if attempt == 0 && e.to_string().contains("App info not found") => {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            Err(e) => return Err(anyhow::anyhow!("Registry.Register failed: {e}")),
        }
    }
    anyhow::bail!("Registry.Register failed after retries")
}

fn binding_to_daemon_action(binding: &HotkeyBinding) -> Option<DaemonAction> {
    // First try matching by the binding's name field.
    if let Some(name) = binding.name.as_deref() {
        match name {
            "capture_area" | "capture-area" => return Some(DaemonAction::CaptureArea),
            "capture_crosshair" | "capture-crosshair" => {
                return Some(DaemonAction::CaptureCrosshair);
            }
            "capture_screen" | "capture-screen" => return Some(DaemonAction::CaptureScreen),
            "capture_window" | "capture-window" => return Some(DaemonAction::CaptureWindow),
            "open_file" | "open-file" => return Some(DaemonAction::OpenFile),
            "open_from_clipboard" | "open-from-clipboard" => {
                return Some(DaemonAction::OpenFromClipboard);
            }
            "restore_recently_closed" | "restore-recently-closed" => {
                return Some(DaemonAction::RestoreRecentlyClosed);
            }
            "toggle_overlays" | "toggle-overlays" => {
                return Some(DaemonAction::ToggleOverlays);
            }
            "show_last_preview" | "show-last-preview" => {
                return Some(DaemonAction::ShowLastPreview);
            }
            "record_screen" | "record-screen" => return Some(DaemonAction::RecordScreen),
            "record_area" | "record-area" => return Some(DaemonAction::RecordArea),
            "open_recording_ui" | "open-recording-ui" => {
                return Some(DaemonAction::OpenRecordingUi);
            }
            "open_video_editor" | "open-video-editor" => {
                return Some(DaemonAction::OpenVideoEditor);
            }
            "recording_pause_resume" | "recording-pause-resume" => {
                return Some(DaemonAction::ToggleRecordingPause);
            }
            "recording_stop_save" | "recording-stop-save" => {
                return Some(DaemonAction::StopRecordingSave);
            }
            "recording_restart" | "recording-restart" => {
                return Some(DaemonAction::RestartRecording);
            }
            "recording_discard" | "recording-discard" => {
                return Some(DaemonAction::DiscardRecording);
            }
            _ => {}
        }
    }

    // Fallback: derive action from the args list.
    match binding.args.first().map(|s| s.as_str()) {
        Some("capture") => match binding.args.get(1).map(|s| s.as_str()) {
            Some("area") => Some(DaemonAction::CaptureArea),
            Some("crosshair") => Some(DaemonAction::CaptureCrosshair),
            Some("screen") => Some(DaemonAction::CaptureScreen),
            Some("window") => Some(DaemonAction::CaptureWindow),
            _ => None,
        },
        Some("open-file") => Some(DaemonAction::OpenFile),
        Some("open-from-clipboard") => Some(DaemonAction::OpenFromClipboard),
        Some("restore-recently-closed") => Some(DaemonAction::RestoreRecentlyClosed),
        Some("toggle-overlays") => Some(DaemonAction::ToggleOverlays),
        Some("show-last-preview") => Some(DaemonAction::ShowLastPreview),
        Some("record") => match binding.args.get(1).map(|s| s.as_str()) {
            Some("ui") => Some(DaemonAction::OpenRecordingUi),
            Some("screen") => Some(DaemonAction::RecordScreen),
            Some("area") => Some(DaemonAction::RecordArea),
            _ => None,
        },
        Some("recording-control") => match binding.args.get(1).map(|s| s.as_str()) {
            Some("pause-resume") => Some(DaemonAction::ToggleRecordingPause),
            Some("stop-save") => Some(DaemonAction::StopRecordingSave),
            Some("restart") => Some(DaemonAction::RestartRecording),
            Some("discard") => Some(DaemonAction::DiscardRecording),
            _ => None,
        },
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Capture helpers — run on blocking thread via spawn_blocking
// ─────────────────────────────────────────────────────────────────────────────

fn make_wayland_backend() -> Option<crate::backend::WaylandBackend> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        <crate::backend::WaylandBackend as DisplayBackend>::new().ok()
    } else {
        None
    }
}

fn make_x11_backend() -> Option<crate::backend::X11Backend> {
    if std::env::var_os("DISPLAY").is_some() {
        <crate::backend::X11Backend as DisplayBackend>::new().ok()
    } else {
        None
    }
}

fn capture_full_screen() -> Option<crate::backend::CaptureData> {
    if let Some(b) = make_wayland_backend() {
        b.capture_screen_impl().ok()
    } else if let Some(b) = make_x11_backend() {
        <crate::backend::X11Backend as DisplayBackend>::capture_screen(&b).ok()
    } else {
        None
    }
}

fn capture_to_temp_png_path(capture: crate::backend::CaptureData) -> Result<String, String> {
    let path =
        save_temp_png_daemon(&capture).ok_or_else(|| "Failed to save temporary PNG".to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

fn capture_fullscreen_to_temp_png_path() -> Result<String, String> {
    let capture = capture_full_screen()
        .ok_or_else(|| "No display backend available for fullscreen capture".to_string())?;
    capture_to_temp_png_path(capture)
}

fn capture_area_to_temp_png_path(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<String, String> {
    if width <= 0 || height <= 0 {
        return Err(format!("Invalid capture area: {width}x{height}"));
    }

    if let Some(backend) = make_wayland_backend() {
        match backend.capture_area_direct_impl(x, y, width, height) {
            Ok(capture) => return capture_to_temp_png_path(capture),
            Err(area_err) => {
                let full = backend.capture_screen_impl().map_err(|e| {
                    format!(
                        "Wayland area capture failed ({area_err}); fullscreen fallback failed: {e}"
                    )
                })?;
                let crop_x = x.max(0) as u32;
                let crop_y = y.max(0) as u32;
                let crop_w = width.max(1) as u32;
                let crop_h = height.max(1) as u32;
                let cropped = crop_capture_data(&full, crop_x, crop_y, crop_w, crop_h)
                    .ok_or_else(|| "Wayland crop fallback was out of bounds".to_string())?;
                return capture_to_temp_png_path(cropped);
            }
        }
    }

    if let Some(backend) = make_x11_backend() {
        match <crate::backend::X11Backend as DisplayBackend>::capture_area(
            &backend, x, y, width, height,
        ) {
            Ok(capture) => return capture_to_temp_png_path(capture),
            Err(area_err) => {
                let full = <crate::backend::X11Backend as DisplayBackend>::capture_screen(&backend)
                    .map_err(|e| {
                        format!(
                            "X11 area capture failed ({area_err}); fullscreen fallback failed: {e}"
                        )
                    })?;
                let crop_x = x.max(0) as u32;
                let crop_y = y.max(0) as u32;
                let crop_w = width.max(1) as u32;
                let crop_h = height.max(1) as u32;
                let cropped = crop_capture_data(&full, crop_x, crop_y, crop_w, crop_h)
                    .ok_or_else(|| "X11 crop fallback was out of bounds".to_string())?;
                return capture_to_temp_png_path(cropped);
            }
        }
    }

    Err("No display backend available for area capture".into())
}

fn crop_capture_data(
    capture: &crate::backend::CaptureData,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> Option<crate::backend::CaptureData> {
    use crate::backend::CaptureData;
    let bpp = capture.format.bytes_per_pixel as usize;
    let stride = capture.stride as usize;
    let row_len = w as usize * bpp;

    if x + w > capture.width || y + h > capture.height || w == 0 || h == 0 {
        return None;
    }

    let mut pixels = Vec::with_capacity(row_len * h as usize);
    for row in 0..h as usize {
        let src_y = y as usize + row;
        let src_offset = src_y * stride + x as usize * bpp;
        pixels.extend_from_slice(&capture.pixels[src_offset..src_offset + row_len]);
    }

    Some(CaptureData::new(pixels, w, h, capture.format))
}

fn screenshot_image_format(value: &str) -> crate::capture::ImageFormat {
    match value {
        "JPEG" => crate::capture::ImageFormat::Jpeg { quality: 85 },
        "WebP" => crate::capture::ImageFormat::WebP,
        _ => crate::capture::ImageFormat::Png,
    }
}

fn screenshot_save_config_from(app_config: &crate::config::AppConfig) -> SaveConfig {
    let mut save_config = SaveConfig::default()
        .with_format(screenshot_image_format(&app_config.screenshot_format))
        .with_cursor(app_config.screenshot_show_cursor);

    if !app_config.screenshot_export_location.is_empty() {
        save_config = save_config.with_output_dir(&app_config.screenshot_export_location);
    }

    save_config
}

fn screenshot_save_config() -> SaveConfig {
    let app_config = load_config().sanitized();
    screenshot_save_config_from(&app_config)
}

fn shutter_sound_asset_path(sound_name: &str) -> Option<PathBuf> {
    let file_name = match sound_name {
        "Camera" => "camera.ogg",
        "Classic" => "classic.ogg",
        "Pop" => "pop.ogg",
        "None" => return None,
        _ => return None,
    };

    let asset_paths = [
        // Development: relative to the current project directory.
        std::env::current_dir()
            .unwrap_or_default()
            .join("assets/sounds")
            .join(file_name),
        // Installed: relative to binary location
        std::env::current_exe()
            .ok()
            .and_then(|exe| {
                exe.parent()
                    .map(|dir| dir.join("assets/sounds").join(file_name))
            })
            .unwrap_or_default(),
        // System-wide install
        PathBuf::from("/usr/share/apexshot/sounds").join(file_name),
        PathBuf::from("/usr/local/share/apexshot/sounds").join(file_name),
    ];

    asset_paths
        .into_iter()
        .find(|path| !path.as_os_str().is_empty() && path.exists())
}

fn play_shutter_sound_if_enabled() {
    let config = load_config().sanitized();
    if !config.play_sounds || config.shutter_sound == "None" {
        return;
    }

    let Some(sound_path) = shutter_sound_asset_path(&config.shutter_sound) else {
        eprintln!(
            "[daemon] Shutter sound '{}' selected but asset file is not available yet",
            config.shutter_sound
        );
        return;
    };

    let playback = std::process::Command::new("sh")
        .arg("-c")
        .arg(
            "if command -v pw-play >/dev/null 2>&1; then pw-play \"$1\"; \
             elif command -v paplay >/dev/null 2>&1; then paplay \"$1\"; \
             elif command -v aplay >/dev/null 2>&1; then aplay \"$1\"; \
             else exit 127; fi",
        )
        .arg("sh")
        .arg(&sound_path)
        .spawn();

    if let Err(e) = playback {
        eprintln!(
            "[daemon] Failed to start shutter sound playback for {}: {e}",
            sound_path.display()
        );
    }
}

fn apply_screenshot_after_capture_actions(
    saved_path: std::path::PathBuf,
    state: Arc<Mutex<DaemonState>>,
) {
    let config = load_config().sanitized();
    state.lock().unwrap().last_capture_path = Some(saved_path.clone());

    copy_screenshot_to_clipboard(&saved_path, &config);

    if config.after_capture_open_annotate {
        // Spawn editor as subprocess to avoid tokio runtime conflicts
        // The editor runs its own GTK main loop which doesn't work inside tokio
        spawn_editor_subprocess(saved_path.clone());
    }

    if config.after_capture_show_quick_access {
        let child = show_preview_subprocess(saved_path);
        replace_preview_child(&state, child);
    }
}

fn copy_screenshot_to_clipboard(path: &std::path::Path, config: &crate::config::AppConfig) {
    if !config.after_capture_copy_file_to_clipboard {
        return;
    }
    if config.adv_clipboard_mode == "Image Only" {
        if let Err(e) = crate::utils::clipboard::copy_image_to_clipboard(path) {
            eprintln!("[daemon] Failed to copy screenshot image to clipboard: {e}");
        }
    } else {
        // "File & Image (default)" — copy both image and URI
        if let Err(e) = crate::utils::clipboard::copy_image_to_clipboard(path) {
            eprintln!("[daemon] Failed to copy screenshot image to clipboard: {e}");
        }
        if let Err(e) = copy_capture_uri_to_clipboard(path) {
            eprintln!("[daemon] Failed to copy screenshot URI to clipboard: {e}");
        }
    }
}

fn save_and_open(capture: crate::backend::CaptureData, state: Arc<Mutex<DaemonState>>) -> bool {
    let config = load_config().sanitized();

    if !config.after_capture_save {
        // Even if not saving, copy to clipboard if enabled (using temp capture data)
        if config.after_capture_copy_file_to_clipboard {
            // Save to a temp file first for clipboard copy
            if let Ok(temp_path) = save_capture(&capture, &screenshot_save_config()) {
                copy_screenshot_to_clipboard(&temp_path, &config);
                let _ = std::fs::remove_file(&temp_path);
            }
        }

        eprintln!(
            "[daemon] Screenshot discarded because Save is disabled in after-capture settings"
        );
        send_desktop_notification(
            "Screenshot not saved",
            "Save is disabled in After capture settings",
        );
        return true;
    }

    match save_capture(&capture, &screenshot_save_config()) {
        Ok(path) => {
            let path: std::path::PathBuf = path;
            eprintln!("[daemon] Saved: {}", path.display());
            play_shutter_sound_if_enabled();
            apply_screenshot_after_capture_actions(path, state);
            true
        }
        Err(e) => {
            eprintln!("[daemon] Save error: {e}");
            false
        }
    }
}

fn save_existing_png_and_open(path: std::path::PathBuf, state: Arc<Mutex<DaemonState>>) {
    let config = load_config().sanitized();
    if !config.after_capture_save {
        // Even if not saving, copy to clipboard if enabled
        copy_screenshot_to_clipboard(&path, &config);
        let _ = std::fs::remove_file(&path);
        eprintln!(
            "[daemon] Screenshot discarded because Save is disabled in after-capture settings"
        );
        send_desktop_notification(
            "Screenshot not saved",
            "Save is disabled in After capture settings",
        );
        return;
    }

    match save_existing_png(&path, &screenshot_save_config()) {
        Ok(saved_path) => {
            eprintln!("[daemon] Saved: {}", saved_path.display());
            play_shutter_sound_if_enabled();
            apply_screenshot_after_capture_actions(saved_path, state);
        }
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            eprintln!("[daemon] Save error: {e}");
        }
    }
}

fn handle_import_web_scroll_capture(
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

fn run_ocr_and_report(capture: crate::backend::CaptureData) {
    eprintln!("[daemon] OCR tool selected — extracting text from selected area...");

    let config = load_config().sanitized();
    let ocr_config = OcrConfig::default().with_language(&config.adv_ocr_language);

    match extract_text(&capture, &ocr_config) {
        Ok(result) => match &result.source {
            crate::ocr::ContentSource::QrCode => {
                eprintln!("[daemon] QR code decoded");
                if result.copied_to_clipboard {
                    send_desktop_notification("QR code decoded", "URL copied to clipboard");
                } else {
                    send_desktop_notification(
                        "QR code decoded",
                        "Content extracted, but clipboard copy was unavailable",
                    );
                }
            }
            crate::ocr::ContentSource::Ocr { confidence } => {
                eprintln!("[daemon] OCR successful (confidence: {}%)", confidence);
                if result.copied_to_clipboard {
                    send_desktop_notification("OCR complete", "Text copied to clipboard");
                } else {
                    send_desktop_notification(
                        "OCR complete",
                        "Text extracted, but clipboard copy was unavailable",
                    );
                }
            }
        },
        Err(err) => {
            eprintln!("[daemon] OCR failed: {err}");
            send_desktop_notification("OCR failed", &err.to_string());
        }
    }
}

fn last_capture_target(path: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    path.map(std::path::Path::to_path_buf)
}

fn clipboard_missing_image_notification() -> (&'static str, &'static str) {
    (
        "Clipboard image unavailable",
        "Clipboard does not contain an image to open",
    )
}

fn restore_recently_closed_target(path: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    last_capture_target(path)
}

fn should_show_preview_after_toggle(preview_visible: bool, has_last_capture: bool) -> bool {
    !preview_visible && has_last_capture
}

fn refresh_preview_child_state(state: &mut DaemonState) -> bool {
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

fn replace_preview_child(state: &Arc<Mutex<DaemonState>>, child: Option<std::process::Child>) {
    if let Ok(mut guard) = state.lock() {
        if let Some(mut existing) = guard.preview_child.take() {
            let _ = existing.kill();
            let _ = existing.wait();
        }
        guard.preview_child = child;
    }
}

fn preview_visible(state: &Arc<Mutex<DaemonState>>) -> bool {
    state
        .lock()
        .map(|mut guard| refresh_preview_child_state(&mut guard))
        .unwrap_or(false)
}

fn last_capture_path(state: &Arc<Mutex<DaemonState>>) -> Option<std::path::PathBuf> {
    state
        .lock()
        .ok()
        .and_then(|guard| guard.last_capture_path.clone())
}

fn stop_preview_overlay(state: &Arc<Mutex<DaemonState>>) -> bool {
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

fn show_preview_for_path(path: std::path::PathBuf, state: &Arc<Mutex<DaemonState>>) -> bool {
    let child = show_preview_subprocess(path);
    let shown = child.is_some();
    replace_preview_child(state, child);
    shown
}

fn toggle_preview_overlay(state: &Arc<Mutex<DaemonState>>) -> bool {
    let visible = preview_visible(state);
    let target = last_capture_path(state);

    if !should_show_preview_after_toggle(visible, target.is_some()) {
        return stop_preview_overlay(state);
    }

    target
        .map(|path| show_preview_for_path(path, state))
        .unwrap_or(false)
}

fn import_clipboard_image_to_temp_png() -> anyhow::Result<std::path::PathBuf> {
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

fn send_desktop_notification(summary: &str, body: &str) {
    let mut cmd = std::process::Command::new("notify-send");
    cmd.arg("-a").arg("ApexShot").arg(summary);
    if !body.is_empty() {
        cmd.arg(body);
    }

    if let Err(e) = cmd.spawn() {
        eprintln!("[daemon] Failed to send desktop notification: {e}");
    }
}

/// Spawn `apexshot preview <path>` as a subprocess so it gets its own GTK context.
fn show_preview_subprocess(path: std::path::PathBuf) -> Option<std::process::Child> {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));
    match std::process::Command::new(&exe)
        .arg("preview")
        .arg(&path)
        .spawn()
    {
        Ok(child) => Some(child),
        Err(e) => {
            eprintln!("[daemon] Failed to spawn preview subprocess: {e}, falling back to xdg-open");
            open_file(path);
            None
        }
    }
}

fn show_settings_subprocess() {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));

    if let Err(e) = std::process::Command::new(&exe).arg("settings").spawn() {
        eprintln!("[daemon] Failed to spawn settings window: {e}");
    }
}

/// Spawn the recording editor without an initial video.
fn spawn_empty_video_editor_subprocess() {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));

    if let Err(e) = std::process::Command::new(&exe).arg("video-editor").spawn() {
        eprintln!("[daemon] Failed to spawn video editor: {e}");
    }
}

/// Spawn `apexshot edit <path>` as a subprocess so it gets its own process
/// and doesn't conflict with the tokio runtime in the daemon.
fn spawn_editor_subprocess(path: std::path::PathBuf) {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));

    if let Err(e) = std::process::Command::new(&exe)
        .arg("edit")
        .arg(&path)
        .spawn()
    {
        eprintln!("[daemon] Failed to spawn editor subprocess: {e}");
    }
}

fn open_file(path: std::path::PathBuf) {
    let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
}

/// Save CaptureData as a temp PNG for the C++ overlay background.
fn save_temp_png_daemon(capture: &crate::backend::CaptureData) -> Option<std::path::PathBuf> {
    use image::{ImageBuffer, Rgba};

    let tmp = std::env::temp_dir().join(format!(
        "apexshot_bg_{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));

    let bytes_per_pixel = capture.format.bytes_per_pixel as usize;
    let stride = capture.stride as usize;
    let w = capture.width;
    let h = capture.height;

    use crate::backend::PixelFormat;
    let is_bgr = capture.format == PixelFormat::BGR24
        || capture.format == PixelFormat::BGR32
        || capture.format == PixelFormat::BGRA32;

    let mut rgba: Vec<u8> = Vec::with_capacity((w * h * 4) as usize);
    for row in 0..h as usize {
        let row_start = row * stride;
        let row_end = (row_start + w as usize * bytes_per_pixel).min(capture.pixels.len());
        let row_data = &capture.pixels[row_start..row_end];
        for px in row_data.chunks(bytes_per_pixel) {
            if px.len() >= 4 {
                if is_bgr {
                    rgba.push(px[2]); // R (from BGR byte[2])
                    rgba.push(px[1]); // G
                    rgba.push(px[0]); // B (from BGR byte[0])
                    rgba.push(px[3]); // A
                } else {
                    rgba.push(px[0]); // R
                    rgba.push(px[1]); // G
                    rgba.push(px[2]); // B
                    rgba.push(px[3]); // A
                }
            } else if px.len() == 3 {
                if is_bgr {
                    rgba.push(px[2]);
                    rgba.push(px[1]);
                    rgba.push(px[0]);
                } else {
                    rgba.push(px[0]);
                    rgba.push(px[1]);
                    rgba.push(px[2]);
                }
                rgba.push(255);
            }
        }
    }

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(w, h, rgba)?;
    img.save(&tmp).ok()?;
    Some(tmp)
}

fn screenshot_timer_delay_duration(seconds: u32) -> Option<std::time::Duration> {
    if seconds == 0 {
        None
    } else {
        Some(std::time::Duration::from_secs(seconds as u64))
    }
}

fn screenshot_timer_supported(action: &str) -> bool {
    matches!(action, "screen" | "window")
}

fn apply_screenshot_timer_if_needed(action: &str, app_config: &crate::config::AppConfig) {
    if !screenshot_timer_supported(action) {
        return;
    }
    if let Some(delay) = screenshot_timer_delay_duration(app_config.screenshot_timer_interval) {
        std::thread::sleep(delay);
    }
}

fn handle_capture_area(state: Arc<Mutex<DaemonState>>) {
    let Some(_session_guard) = acquire_capture_session_guard("area") else {
        return;
    };
    // Close any existing preview before starting capture (single-instance behavior)
    let _ = stop_preview_overlay(&state);
    handle_capture_area_with_active_session(state);
}

fn handle_capture_crosshair(state: Arc<Mutex<DaemonState>>) {
    let Some(_session_guard) = acquire_capture_session_guard("crosshair") else {
        return;
    };
    // Close any existing preview before starting capture
    let _ = stop_preview_overlay(&state);
    handle_capture_crosshair_with_active_session(state);
}

fn handle_capture_area_with_active_session(state: Arc<Mutex<DaemonState>>) {
    let app_config = load_config().sanitized();
    apply_screenshot_timer_if_needed("area", &app_config);

    let gtk_tx = state.lock().unwrap().gtk_tx.clone();

    let cpp_area_init = if let Some(gtk_tx) = gtk_tx.clone() {
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(0);
        match gtk_tx.send(GtkWork::CaptureAreaInit { reply: reply_tx }) {
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

    match cpp_area_init {
        Ok(AreaCapturePathResult::Captured(path)) => {
            save_existing_png_and_open(path, state);
        }
        Ok(AreaCapturePathResult::ScrollCaptured(path)) => {
            save_existing_png_and_open(path, state);
        }
        Ok(AreaCapturePathResult::OcrRequested(capture)) => {
            run_ocr_and_report(capture);
        }
        Ok(AreaCapturePathResult::Cancelled) => {
            eprintln!("[daemon] Area selection cancelled.");
        }
        Ok(AreaCapturePathResult::RecordingConfigUpdated) => {
            eprintln!("[daemon] Recording overlay state updated.");
        }
        Ok(AreaCapturePathResult::RecordingRequested(request)) => {
            if let Err(err) = run_overlay_recording_request_with_gtk(request, gtk_tx.clone()) {
                eprintln!("[daemon] Recording failed: {err}");
                // Show notification for GNOME extension not installed
                if err
                    .to_string()
                    .contains("GNOME Shell extension is not installed")
                {
                    send_desktop_notification(
                        "Recording failed",
                        "GNOME Shell extension is not installed. Please install the ApexShot GNOME extension first.",
                    );
                }
            }
        }
        Err(err) => {
            if err
                .downcast_ref::<crate::overlay::SelectionError>()
                .is_some_and(is_launch_blocked_error)
            {
                eprintln!("[daemon] Area capture blocked: {err}");
                return;
            }
            eprintln!("[daemon] C++ area-init capture path failed: {err}");
        }
    }
}

fn handle_capture_crosshair_with_active_session(state: Arc<Mutex<DaemonState>>) {
    let app_config = load_config().sanitized();
    apply_screenshot_timer_if_needed("crosshair", &app_config);

    match capture_crosshair_file_via_cpp() {
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
            eprintln!("[daemon] Crosshair capture failed: {err}");
        }
    }
}

fn handle_capture_screen(state: Arc<Mutex<DaemonState>>) {
    let Some(_session_guard) = acquire_capture_session_guard("screen") else {
        return;
    };
    // Close any existing preview before starting capture
    let _ = stop_preview_overlay(&state);
    handle_capture_screen_with_active_session(state);
}

fn handle_capture_screen_with_active_session(state: Arc<Mutex<DaemonState>>) {
    let app_config = load_config().sanitized();
    apply_screenshot_timer_if_needed("screen", &app_config);

    match capture_screen_file_via_cpp() {
        Ok(path) => {
            save_existing_png_and_open(path, state);
        }
        Err(err) if is_launch_blocked_error(&err) => {
            eprintln!("[daemon] Fullscreen capture blocked: {err}");
        }
        Err(err) => {
            eprintln!("[daemon] C++ fullscreen capture failed: {err}");
        }
    }
}

fn handle_capture_window(state: Arc<Mutex<DaemonState>>) {
    let Some(_session_guard) = acquire_capture_session_guard("window") else {
        return;
    };
    // Close any existing preview before starting capture
    let _ = stop_preview_overlay(&state);
    handle_capture_window_with_active_session(state);
}

fn handle_capture_window_with_active_session(state: Arc<Mutex<DaemonState>>) {
    let app_config = load_config().sanitized();
    apply_screenshot_timer_if_needed("window", &app_config);

    eprintln!("[daemon] Window capture requested — using the shared window capture flow");
    match capture_window_file_via_cpp() {
        Ok(path) => {
            save_existing_png_and_open(path, state);
        }
        Err(err) if is_launch_blocked_error(&err) => {
            eprintln!("[daemon] Window capture blocked: {err}");
        }
        Err(e) => {
            eprintln!("[daemon] Window capture failed: {e}; falling back to area capture.");
            handle_capture_area_with_active_session(state);
        }
    }
}

fn acquire_capture_session_guard(context: &str) -> Option<CaptureOverlayGuard<'static>> {
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

async fn handle_record_screen(_tx: std::sync::mpsc::Sender<DaemonAction>) {
    use crate::recording::{
        run_recording_with_controls, RecordingConfig, RecordingControlsParams, StopAction,
    };

    eprintln!("[daemon] Starting screen recording…");

    let config = RecordingConfig::default();
    let params = RecordingControlsParams {
        capture_x: 0,
        capture_y: 0,
        capture_w: 0,
        capture_h: 0,
        is_fullscreen: true,
        show_timer: true,
        use_shell_mask: false,
        show_webcam: false,
        webcam_device: -1,
        webcam_size: 1,
        webcam_shape: 0,
        webcam_rel_x: 0.0,
        webcam_rel_y: 0.0,
        webcam_flip: false,
        show_clicks: false,
        click_size: 1.0,
        click_color: 0,
        click_style: 0,
        click_animate: false,
        show_keys: false,
        key_size: 1.0,
        key_position: 0,
        countdown_enabled: false,
        countdown_seconds: 3,
        session_id: None,
    };

    match run_recording_with_controls(config, params).await {
        Ok((path, StopAction::Discard)) => {
            let _ = std::fs::remove_file(&path);
            eprintln!("[daemon] Recording discarded.");
        }
        Ok((path, StopAction::Save)) => {
            eprintln!("[daemon] Recording saved: {}", path.display())
        }
        Err(e) => eprintln!("[daemon] Recording error: {e}"),
    }
}

async fn handle_open_recording_ui(_tx: std::sync::mpsc::Sender<DaemonAction>) {
    match tokio::task::spawn_blocking(open_recording_ui_via_cpp).await {
        Ok(Ok(AreaCapturePathResult::RecordingRequested(request))) => {
            if let Err(err) = run_overlay_recording_request_with_gtk(request, None) {
                eprintln!("[daemon] Recording UI failed: {err}");
                // Show notification for GNOME extension not installed
                if err
                    .to_string()
                    .contains("GNOME Shell extension is not installed")
                {
                    send_desktop_notification(
                        "Recording failed",
                        "GNOME Shell extension is not installed. Please install the ApexShot GNOME extension first.",
                    );
                }
            }
        }
        Ok(Ok(AreaCapturePathResult::RecordingConfigUpdated)) => {
            eprintln!("[daemon] Recording UI updated settings only.");
        }
        Ok(Ok(AreaCapturePathResult::Cancelled)) => {
            eprintln!("[daemon] Recording UI cancelled.");
        }
        Ok(Ok(other)) => {
            eprintln!("[daemon] Unexpected recording UI result: {:?}", other);
        }
        Ok(Err(err)) => eprintln!("[daemon] Failed to open recording UI: {err}"),
        Err(err) => eprintln!("[daemon] Recording UI task panicked: {err}"),
    }
}

async fn handle_record_area(_tx: std::sync::mpsc::Sender<DaemonAction>) {
    use crate::capture_overlay::run_capture_overlay;
    use crate::overlay::OverlaySelection;
    use crate::recording::{
        run_recording_with_controls, RecordingConfig, RecordingControlsParams, StopAction,
    };

    eprintln!("[daemon] Selecting area for recording…");

    // Show C++ overlay on a blocking thread.
    let selection = tokio::task::spawn_blocking(|| run_capture_overlay(None)).await;

    let cpp_area = match selection {
        Ok(Ok(OverlaySelection::Area(Some(a)))) => a,
        Ok(Ok(OverlaySelection::Area(None))) | Ok(Ok(OverlaySelection::Recording(_))) => {
            eprintln!("[daemon] Area selection cancelled.");
            return;
        }
        Ok(Err(e)) => {
            eprintln!("[daemon] Area selection error: {e}");
            return;
        }
        Err(e) => {
            eprintln!("[daemon] Area selection task panicked: {e}");
            return;
        }
    };
    let area = crate::overlay::SelectionArea {
        x: cpp_area.x,
        y: cpp_area.y,
        width: cpp_area.width,
        height: cpp_area.height,
    };

    let config = RecordingConfig {
        x: Some(area.x),
        y: Some(area.y),
        width: Some(area.width as u32),
        height: Some(area.height as u32),
        ..Default::default()
    };

    eprintln!(
        "[daemon] Starting area recording ({},{} {}x{})…",
        area.x, area.y, area.width, area.height
    );

    let params = RecordingControlsParams {
        capture_x: area.x,
        capture_y: area.y,
        capture_w: area.width,
        capture_h: area.height,
        is_fullscreen: false,
        show_timer: true,
        use_shell_mask: false,
        show_webcam: false,
        webcam_device: -1,
        webcam_size: 1,
        webcam_shape: 0,
        webcam_rel_x: 0.0,
        webcam_rel_y: 0.0,
        webcam_flip: false,
        show_clicks: false,
        click_size: 1.0,
        click_color: 0,
        click_style: 0,
        click_animate: false,
        show_keys: false,
        key_size: 1.0,
        key_position: 0,
        countdown_enabled: false,
        countdown_seconds: 3,
        session_id: None,
    };
    match run_recording_with_controls(config, params).await {
        Ok((path, StopAction::Discard)) => {
            let _ = std::fs::remove_file(&path);
            eprintln!("[daemon] Recording discarded.");
        }
        Ok((path, StopAction::Save)) => {
            eprintln!("[daemon] Recording saved: {}", path.display())
        }
        Err(e) => eprintln!("[daemon] Recording error: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clipboard_missing_image_notification, last_capture_target, restore_recently_closed_target,
        should_autostart_ydotoold, should_quit_on_sigint, should_show_preview_after_toggle,
        RecordingTrayState,
    };
    use std::{
        path::Path,
        time::{Duration, Instant},
    };

    #[test]
    fn daemon_does_not_autostart_ydotoold_by_default() {
        assert!(!should_autostart_ydotoold());
    }

    #[test]
    fn screenshot_save_config_uses_format_and_cursor_settings() {
        let app_config = crate::config::AppConfig {
            screenshot_export_location: "/tmp/screens".into(),
            screenshot_format: "JPEG".into(),
            screenshot_show_cursor: false,
            ..crate::config::AppConfig::default()
        }
        .sanitized();

        let save_config = super::screenshot_save_config_from(&app_config);

        assert_eq!(
            save_config.output_dir.as_deref(),
            Some(std::path::Path::new("/tmp/screens"))
        );
        assert_eq!(
            save_config.format,
            crate::capture::ImageFormat::Jpeg { quality: 85 }
        );
        assert!(!save_config.include_cursor);
    }

    #[test]
    fn screenshot_timer_delay_duration_respects_config() {
        assert_eq!(super::screenshot_timer_delay_duration(0), None);
        assert_eq!(
            super::screenshot_timer_delay_duration(3),
            Some(std::time::Duration::from_secs(3))
        );
    }

    #[test]
    fn screenshot_timer_only_delays_non_interactive_capture_actions() {
        assert!(!super::screenshot_timer_supported("area"));
        assert!(!super::screenshot_timer_supported("crosshair"));
        assert!(super::screenshot_timer_supported("screen"));
        assert!(super::screenshot_timer_supported("window"));
        assert!(!super::screenshot_timer_supported("import_web_scroll"));
    }

    #[test]
    fn daemon_ignores_hotkeys_while_shortcut_edit_is_active() {
        use std::sync::atomic::Ordering;

        // Default: not suppressed
        assert!(!super::HOTKEY_SUPPRESSED.load(Ordering::Relaxed));
        assert!(!super::is_hotkey_suppressed());

        // Suppress
        super::HOTKEY_SUPPRESSED.store(true, Ordering::Relaxed);
        assert!(super::is_hotkey_suppressed());

        // Unsuppress
        super::HOTKEY_SUPPRESSED.store(false, Ordering::Relaxed);
        assert!(!super::is_hotkey_suppressed());
    }

    #[test]
    fn sigint_quits_only_when_no_recording_is_active() {
        assert!(should_quit_on_sigint(false));
        assert!(!should_quit_on_sigint(true));
    }

    #[test]
    fn binding_to_daemon_action_maps_open_recording_ui_hotkey() {
        let open_recording_ui = crate::hotkeys::HotkeyBinding {
            accelerator: "CTRL+ALT+R".into(),
            args: vec!["record".into(), "ui".into()],
            name: Some("open_recording_ui".into()),
        };

        assert_eq!(
            super::binding_to_daemon_action(&open_recording_ui),
            Some(super::DaemonAction::OpenRecordingUi)
        );
    }

    #[test]
    fn binding_to_daemon_action_maps_recording_control_hotkeys() {
        let pause_resume = crate::hotkeys::HotkeyBinding {
            accelerator: "CTRL+ALT+SHIFT+P".into(),
            args: vec!["recording-control".into(), "pause-resume".into()],
            name: Some("recording_pause_resume".into()),
        };
        let stop_save = crate::hotkeys::HotkeyBinding {
            accelerator: "CTRL+ALT+SHIFT+S".into(),
            args: vec!["recording-control".into(), "stop-save".into()],
            name: Some("recording_stop_save".into()),
        };
        let restart = crate::hotkeys::HotkeyBinding {
            accelerator: "CTRL+ALT+SHIFT+N".into(),
            args: vec!["recording-control".into(), "restart".into()],
            name: Some("recording_restart".into()),
        };
        let discard = crate::hotkeys::HotkeyBinding {
            accelerator: "CTRL+ALT+SHIFT+BackSpace".into(),
            args: vec!["recording-control".into(), "discard".into()],
            name: Some("recording_discard".into()),
        };

        assert!(matches!(
            super::binding_to_daemon_action(&pause_resume),
            Some(super::DaemonAction::ToggleRecordingPause)
        ));
        assert!(matches!(
            super::binding_to_daemon_action(&stop_save),
            Some(super::DaemonAction::StopRecordingSave)
        ));
        assert!(matches!(
            super::binding_to_daemon_action(&restart),
            Some(super::DaemonAction::RestartRecording)
        ));
        assert!(matches!(
            super::binding_to_daemon_action(&discard),
            Some(super::DaemonAction::DiscardRecording)
        ));
    }

    #[test]
    fn open_file_targets_last_capture_when_available() {
        let path = Path::new("/tmp/example.png");
        assert_eq!(last_capture_target(Some(path)).as_deref(), Some(path));
        assert_eq!(last_capture_target(None), None);
    }

    #[test]
    fn clipboard_missing_image_notification_matches_expected_copy() {
        assert_eq!(
            clipboard_missing_image_notification(),
            (
                "Clipboard image unavailable",
                "Clipboard does not contain an image to open"
            )
        );
    }

    #[test]
    fn restore_recently_closed_reuses_last_capture_target() {
        let path = Path::new("/tmp/example.png");
        assert_eq!(
            restore_recently_closed_target(Some(path)).as_deref(),
            Some(path)
        );
        assert_eq!(restore_recently_closed_target(None), None);
    }

    #[test]
    fn toggle_preview_policy_only_shows_when_hidden_and_capture_exists() {
        assert!(should_show_preview_after_toggle(false, true));
        assert!(!should_show_preview_after_toggle(true, true));
        assert!(!should_show_preview_after_toggle(false, false));
    }

    #[test]
    fn binding_to_daemon_action_maps_general_shortcuts() {
        let open_file = crate::hotkeys::HotkeyBinding {
            accelerator: "CTRL+ALT+O".into(),
            args: vec!["open-file".into()],
            name: Some("open_file".into()),
        };
        let open_from_clipboard = crate::hotkeys::HotkeyBinding {
            accelerator: "CTRL+ALT+V".into(),
            args: vec!["open-from-clipboard".into()],
            name: Some("open_from_clipboard".into()),
        };
        let restore_recently_closed = crate::hotkeys::HotkeyBinding {
            accelerator: "CTRL+ALT+Z".into(),
            args: vec!["restore-recently-closed".into()],
            name: Some("restore_recently_closed".into()),
        };
        let toggle_overlays = crate::hotkeys::HotkeyBinding {
            accelerator: "CTRL+ALT+H".into(),
            args: vec!["toggle-overlays".into()],
            name: Some("toggle_overlays".into()),
        };

        assert!(matches!(
            super::binding_to_daemon_action(&open_file),
            Some(super::DaemonAction::OpenFile)
        ));
        assert!(matches!(
            super::binding_to_daemon_action(&open_from_clipboard),
            Some(super::DaemonAction::OpenFromClipboard)
        ));
        assert!(matches!(
            super::binding_to_daemon_action(&restore_recently_closed),
            Some(super::DaemonAction::RestoreRecentlyClosed)
        ));
        assert!(matches!(
            super::binding_to_daemon_action(&toggle_overlays),
            Some(super::DaemonAction::ToggleOverlays)
        ));
    }

    #[test]
    fn binding_to_daemon_action_maps_crosshair_capture_hotkey() {
        let crosshair = crate::hotkeys::HotkeyBinding {
            accelerator: "CTRL+ALT+X".into(),
            args: vec!["capture".into(), "crosshair".into()],
            name: Some("capture_crosshair".into()),
        };

        assert!(matches!(
            super::binding_to_daemon_action(&crosshair),
            Some(super::DaemonAction::CaptureCrosshair)
        ));
    }

    #[test]
    fn tray_action_maps_crosshair_capture_to_daemon_action() {
        assert!(matches!(
            super::DaemonAction::from(crate::tray::TrayAction::CaptureCrosshair),
            super::DaemonAction::CaptureCrosshair
        ));
    }

    #[test]
    fn recording_tray_state_formats_elapsed_and_freezes_while_paused() {
        let mut state = RecordingTrayState::started();
        state.started_at -= Duration::from_secs(83);
        assert_eq!(state.elapsed_text(), "1:23");

        state.pause();
        state.paused_at = Some(Instant::now() - Duration::from_secs(5));
        assert_eq!(state.elapsed_text(), "1:18");

        state.resume();
        assert_eq!(state.elapsed_text(), "1:18");

        state.restart();
        assert_eq!(state.elapsed_text(), "0:00");
    }
}
