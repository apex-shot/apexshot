//! Persistent tray daemon for ApexShot.
//!
//! A single long-running process that:
//!   1. Shows a system tray icon (via ksni / StatusNotifierItem)
//!   2. Listens for global hotkeys via GNOME Shell GrabAccelerators
//!   3. On hotkey or tray-menu trigger → runs capture + overlay IN-PROCESS
//!      (no subprocess spawn, no GTK cold-start delay)
//!
//! The daemon keeps hotkeys, tray actions, previews, and recording controls
//! warm so user-triggered actions avoid repeated process setup work.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{
    capture_overlay::{
        ensure_warm_capture_helper, shutdown_warm_capture_helper, AreaCapturePathResult,
    },
    config::load_config,
    hotkeys::{sync_gnome_hotkeys_for_current_desktop, sync_hotkeys_from_app_config},
    tray::{spawn_tray, ApexShotTray, TrayAction},
};
use anyhow::Context;

mod audio;
mod capture_handlers;
mod dispatch;
mod hotkey_listener;
mod recording_handlers;
mod scroll;

pub(crate) use audio::find_physical_input_device;
pub use capture_handlers::{copy_screenshot_to_clipboard, open_file};
pub use recording_handlers::{
    notify_daemon_recording_ended, notify_daemon_recording_paused,
    notify_daemon_recording_restarted, notify_daemon_recording_resumed,
    notify_daemon_recording_started,
};

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
    OpenImageEditor,
    StopRecordingSave,
    ShowLastPreview,
    ShowPreviewForPath(std::path::PathBuf),
    OpenLastCapture,
    OpenHistory,
    OpenSettings,
    SetTrayVisible(bool),
    RecordingSessionStarted,
    RecordingSessionPaused,
    RecordingSessionResumed,
    RecordingSessionRestarted,
    RecordingSessionEnded,
    SetHotkeySuppressed(bool),
    Quit,
}

impl From<TrayAction> for DaemonAction {
    fn from(action: TrayAction) -> Self {
        match action {
            TrayAction::CaptureArea => Self::CaptureArea,
            TrayAction::CaptureCrosshair => Self::CaptureCrosshair,
            TrayAction::CaptureScreen => Self::CaptureScreen,
            TrayAction::CaptureWindow => Self::CaptureWindow,
            TrayAction::OpenRecordingUi => Self::OpenRecordingUi,
            TrayAction::OpenVideoEditor => Self::OpenVideoEditor,
            TrayAction::OpenImageEditor => Self::OpenImageEditor,
            TrayAction::RecordScreen => Self::RecordScreen,
            TrayAction::StopRecordingSave => Self::StopRecordingSave,
            TrayAction::ShowLastPreview => Self::ShowLastPreview,
            TrayAction::OpenLastCapture => Self::OpenLastCapture,
            TrayAction::OpenHistory => Self::OpenHistory,
            TrayAction::OpenSettings => Self::OpenSettings,
            TrayAction::Quit => Self::Quit,
        }
    }
}

pub(super) struct DaemonState {
    pub(super) last_capture_path: Option<std::path::PathBuf>,
    pub(super) preview_child: Option<std::process::Child>,
    /// Channel to send GTK work to the main OS thread. `None` when the daemon
    /// owns the main thread itself (legacy / test mode).
    pub(super) gtk_tx: Option<std::sync::mpsc::Sender<GtkWork>>,
}

/// Well-known D-Bus name that the daemon registers.
pub const DAEMON_BUS_NAME: &str = "org.apexshot.Daemon";

/// D-Bus object path.
pub const DAEMON_OBJECT_PATH: &str = "/org/apexshot/Daemon";

/// D-Bus interface.
pub const DAEMON_INTERFACE: &str = "org.apexshot.Daemon";

/// When true, hotkey activations are suppressed (e.g. during shortcut editing).
pub(super) static HOTKEY_SUPPRESSED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

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

pub(super) fn trigger_daemon_action_blocking(action: &str) -> bool {
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
        .call::<_, _, ()>(SHOW_PREVIEW_FOR_PATH_MEMBER, &(path_str,))
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

/// Whether a daemon is reachable on the session bus.
pub fn is_daemon_running() -> bool {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        return false;
    };
    zbus::blocking::Proxy::new(&conn, DAEMON_BUS_NAME, DAEMON_OBJECT_PATH, DAEMON_INTERFACE).is_ok()
}

/// Start the tray/hotkey daemon if needed and wait briefly for D-Bus readiness.
///
/// Returns `true` when a daemon is reachable (already running or freshly started).
/// Fail-open: never panics; returns `false` if spawn/connect fails.
pub fn ensure_daemon_running() -> bool {
    if is_daemon_running() {
        return true;
    }

    if let Err(e) = start_daemon_subprocess() {
        eprintln!("[daemon] Failed to start daemon: {e}");
        return false;
    }

    // D-Bus name claim + tray setup usually finishes within a few hundred ms.
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(50));
        if is_daemon_running() {
            return true;
        }
    }

    is_daemon_running()
}

/// Blocking D-Bus Trigger for use outside async contexts (onboarding, settings).
pub fn trigger_daemon_action_sync(action: &str) -> bool {
    trigger_daemon_action_blocking(action)
}

pub(super) fn spawn_daemon_tray(
    action_tx: &std::sync::mpsc::Sender<DaemonAction>,
) -> anyhow::Result<ksni::Handle<ApexShotTray>> {
    let (tray_tx, tray_rx) = std::sync::mpsc::channel();
    let action_tx = action_tx.clone();
    std::thread::spawn(move || {
        while let Ok(action) = tray_rx.recv() {
            let _ = action_tx.send(DaemonAction::from(action));
        }
    });
    spawn_tray(tray_tx).context("Failed to spawn tray icon")
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
    CaptureCrosshair {
        reply: std::sync::mpsc::SyncSender<Result<std::path::PathBuf, String>>,
    },
    CaptureScreen {
        reply: std::sync::mpsc::SyncSender<Result<std::path::PathBuf, String>>,
    },
    CaptureWindow {
        reply: std::sync::mpsc::SyncSender<Result<std::path::PathBuf, String>>,
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

pub(super) fn should_quit_on_sigint(recording_active: bool) -> bool {
    !recording_active
}

pub(super) async fn run_daemon_inner(
    gtk_tx: Option<std::sync::mpsc::Sender<GtkWork>>,
) -> anyhow::Result<()> {
    eprintln!("[daemon] ApexShot daemon starting…");

    // Ensure XDG portal permissions are persisted so the user doesn't have
    // to re-approve screenshot/screencast access after reboot.
    // (no-op in portal_only / Flatpak — never mutates PermissionStore)
    crate::backend::portal_permissions::ensure_portal_permissions();

    // Pre-warm apexshot-capture so the first hotkey doesn't pay Qt cold start.
    // Flatpak builds do not ship the Qt helper.
    if !crate::app_identity::portal_only() {
        ensure_warm_capture_helper();
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
    if let Err(err) = sync_hotkeys_from_app_config(&initial_config) {
        eprintln!("[daemon] Failed to refresh configured hotkeys: {err}");
    }
    if !initial_config.show_menu_bar_icon {
        eprintln!("[daemon] Tray icon disabled by settings — exiting.");
        return Ok(());
    }

    // Anonymous daily heartbeat (opt-out via Settings / APEXSHOT_TELEMETRY=0).
    crate::usage_telemetry::spawn_daemon_telemetry_worker();

    // Main action channel — both tray and hotkeys send here.
    let (action_tx, action_rx) = std::sync::mpsc::channel::<DaemonAction>();

    // ── Tray icon ────────────────────────────────────────────────────────────
    let mut recording_tray_state: Option<recording_handlers::RecordingTrayState> = None;
    let mut tray_handle = if initial_config.show_menu_bar_icon {
        let handle = spawn_daemon_tray(&action_tx)?;
        eprintln!("[daemon] Tray icon active.");
        Some(handle)
    } else {
        eprintln!("[daemon] Tray icon disabled by settings.");
        None
    };

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
            if let Err(e) = hotkey_listener::run_hotkey_listener(hotkey_tx).await {
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
    // ── Action loop ──────────────────────────────────────────────────────────
    while let Ok(action) = action_rx.recv() {
        if !dispatch::dispatch_daemon_action(
            action,
            &state,
            &action_tx,
            &mut tray_handle,
            &mut recording_tray_state,
        ) {
            break;
        }
    }

    shutdown_warm_capture_helper();
    eprintln!("[daemon] Exiting.");
    Ok(())
}

pub(super) struct DaemonIpc {
    tx: std::sync::mpsc::Sender<DaemonAction>,
    state: Arc<Mutex<DaemonState>>,
    scroll_injector: tokio::sync::Mutex<scroll::ScrollInjector>,
}

#[zbus::interface(name = "org.apexshot.Daemon")]
impl DaemonIpc {
    fn trigger(&self, action: String) -> zbus::fdo::Result<()> {
        eprintln!("[daemon] D-Bus Trigger: {action}");
        let daemon_action = parse_trigger_action(&action).ok_or_else(|| {
            eprintln!("[daemon] D-Bus Trigger: unknown action '{action}'");
            zbus::fdo::Error::InvalidArgs(format!("Unknown action: {action}"))
        })?;
        self.tx.send(daemon_action).map_err(|e| {
            zbus::fdo::Error::Failed(format!("Daemon action channel unavailable: {e}"))
        })?;
        Ok(())
    }

    /// Returns the current mic level as a normalized f64 (0.0 to 1.0).
    ///
    /// Lazily starts the mic capture stream used for the recording UI meter.
    /// The stream is released after [`audio::AUDIO_MONITOR_IDLE`] without polls so a
    /// tray-only daemon does not keep Bluetooth headsets in HSP/HFP mode.
    fn get_mic_level(&self) -> f64 {
        audio::touch_mic_monitor();
        f64::from_bits(audio::MIC_LEVEL.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Returns the current system audio level as a normalized f64 (0.0 to 1.0).
    ///
    /// Lazily starts the speaker (sink-monitor) capture stream for the recording
    /// UI meter; released after idle like the mic stream.
    fn get_speaker_level(&self) -> f64 {
        audio::touch_speaker_monitor();
        f64::from_bits(audio::SPEAKER_LEVEL.load(std::sync::atomic::Ordering::Relaxed))
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
            capture_handlers::handle_import_web_scroll_capture(
                png_base64, page_url, page_title, state,
            )
        })
        .await
        .map_err(|e| zbus::fdo::Error::Failed(format!("Web capture import task failed: {e}")))
    }

    /// Show preview for a specific path (used by editor to coordinate single-instance)
    fn show_preview_for_path(&self, path: String) -> zbus::fdo::Result<()> {
        eprintln!("[daemon] D-Bus ShowPreviewForPath: {}", path);
        let path = std::path::PathBuf::from(path);
        self.tx
            .send(DaemonAction::ShowPreviewForPath(path))
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Daemon action channel unavailable: {e}"))
            })?;
        Ok(())
    }
}

/// Parse a stable D-Bus/CLI trigger action string into a [`DaemonAction`].
///
/// These strings are part of the external IPC protocol and must not track Rust
/// module paths. Keep names stable across internal refactors.
pub(super) fn parse_trigger_action(action: &str) -> Option<DaemonAction> {
    match action {
        "capture_area" => Some(DaemonAction::CaptureArea),
        "capture_crosshair" => Some(DaemonAction::CaptureCrosshair),
        "capture_screen" => Some(DaemonAction::CaptureScreen),
        "capture_window" => Some(DaemonAction::CaptureWindow),
        "record_screen" => Some(DaemonAction::RecordScreen),
        "record_area" => Some(DaemonAction::RecordArea),
        "open_recording_ui" => Some(DaemonAction::OpenRecordingUi),
        "open_video_editor" => Some(DaemonAction::OpenVideoEditor),
        "open_image_editor" => Some(DaemonAction::OpenImageEditor),
        // Canonical control action names (used by CLI + settings shortcuts).
        "recording_stop_save" | "stop_recording" | "stop_recording_save" => {
            Some(DaemonAction::StopRecordingSave)
        }
        "recording_session_started" => Some(DaemonAction::RecordingSessionStarted),
        "recording_session_paused" => Some(DaemonAction::RecordingSessionPaused),
        "recording_session_resumed" => Some(DaemonAction::RecordingSessionResumed),
        "recording_session_restarted" => Some(DaemonAction::RecordingSessionRestarted),
        "recording_session_ended" => Some(DaemonAction::RecordingSessionEnded),
        "open_file" => Some(DaemonAction::OpenFile),
        "open_from_clipboard" => Some(DaemonAction::OpenFromClipboard),
        "restore_recently_closed" => Some(DaemonAction::RestoreRecentlyClosed),
        "toggle_overlays" => Some(DaemonAction::ToggleOverlays),
        "show_last_preview" => Some(DaemonAction::ShowLastPreview),
        "open_last" => Some(DaemonAction::OpenLastCapture),
        "history" => Some(DaemonAction::OpenHistory),
        "settings" => Some(DaemonAction::OpenSettings),
        "quit" => Some(DaemonAction::Quit),
        _ => None,
    }
}

/// Stable D-Bus member used by editor/clients to request a path-backed preview.
pub(super) const SHOW_PREVIEW_FOR_PATH_MEMBER: &str = "show_preview_for_path";

pub(super) async fn run_dbus_server(
    conn: zbus::Connection,
    tx: std::sync::mpsc::Sender<DaemonAction>,
    state: Arc<Mutex<DaemonState>>,
) -> anyhow::Result<()> {
    let ipc = DaemonIpc {
        tx,
        state,
        scroll_injector: tokio::sync::Mutex::new(scroll::ScrollInjector::default()),
    };

    // Serve the IPC object on the existing connection (name already registered)
    conn.object_server().at(DAEMON_OBJECT_PATH, ipc).await?;

    // Do NOT open mic/speaker capture streams here. Holding a mic capture open
    // keeps Bluetooth headsets in low-quality HSP/HFP mode for the whole tray
    // lifetime (issue #41). Streams start lazily when GetMicLevel/GetSpeakerLevel
    // are polled by the recording UI, and stop after audio::AUDIO_MONITOR_IDLE.

    eprintln!("[daemon] D-Bus IPC ready on {DAEMON_BUS_NAME}");
    std::future::pending::<()>().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::audio::*;
    use super::capture_handlers::*;
    use super::hotkey_listener::*;
    use super::*;
    use std::{path::Path, time::Duration};

    #[test]
    fn audio_monitor_idle_detection() {
        let idle = Duration::from_secs(3);
        assert!(audio_monitor_is_idle(0, 10_000, idle));
        assert!(!audio_monitor_is_idle(10_000, 10_500, idle));
        assert!(!audio_monitor_is_idle(10_000, 12_999, idle));
        assert!(audio_monitor_is_idle(10_000, 13_000, idle));
        assert!(audio_monitor_is_idle(10_000, 20_000, idle));
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

        let save_config = screenshot_save_config_from(&app_config);

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
        assert_eq!(screenshot_timer_delay_duration(0), None);
        assert_eq!(
            screenshot_timer_delay_duration(3),
            Some(std::time::Duration::from_secs(3))
        );
    }

    #[test]
    fn screenshot_timer_only_delays_non_interactive_capture_actions() {
        assert!(!screenshot_timer_supported("area"));
        assert!(!screenshot_timer_supported("crosshair"));
        assert!(screenshot_timer_supported("screen"));
        assert!(screenshot_timer_supported("window"));
        assert!(!screenshot_timer_supported("import_web_scroll"));
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
            binding_to_daemon_action(&open_recording_ui),
            Some(super::DaemonAction::OpenRecordingUi)
        );
    }

    #[test]
    fn binding_to_daemon_action_maps_recording_control_hotkeys() {
        let stop_save = crate::hotkeys::HotkeyBinding {
            accelerator: "CTRL+ALT+SHIFT+S".into(),
            args: vec!["record".into(), "stop".into()],
            name: Some("recording_stop_save".into()),
        };
        assert!(matches!(
            binding_to_daemon_action(&stop_save),
            Some(super::DaemonAction::StopRecordingSave)
        ));

        // Legacy recording-control args and legacy binding names still map.
        let legacy_stop = crate::hotkeys::HotkeyBinding {
            accelerator: String::new(),
            args: vec!["recording-control".into(), "stop-save".into()],
            name: Some("stop_recording".into()),
        };
        assert!(matches!(
            binding_to_daemon_action(&legacy_stop),
            Some(super::DaemonAction::StopRecordingSave)
        ));
        let stop_by_args_only = crate::hotkeys::HotkeyBinding {
            accelerator: String::new(),
            args: vec!["record".into(), "stop".into()],
            name: None,
        };
        assert!(matches!(
            binding_to_daemon_action(&stop_by_args_only),
            Some(super::DaemonAction::StopRecordingSave)
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
            binding_to_daemon_action(&open_file),
            Some(super::DaemonAction::OpenFile)
        ));
        assert!(matches!(
            binding_to_daemon_action(&open_from_clipboard),
            Some(super::DaemonAction::OpenFromClipboard)
        ));
        assert!(matches!(
            binding_to_daemon_action(&restore_recently_closed),
            Some(super::DaemonAction::RestoreRecentlyClosed)
        ));
        assert!(matches!(
            binding_to_daemon_action(&toggle_overlays),
            Some(super::DaemonAction::ToggleOverlays)
        ));
    }

    #[test]
    fn binding_to_daemon_action_maps_open_file_by_name_without_args() {
        // Name-only bindings must resolve without relying on CLI args fallback.
        let open_file = crate::hotkeys::HotkeyBinding {
            accelerator: "CTRL+ALT+O".into(),
            args: vec![],
            name: Some("open_file".into()),
        };
        assert_eq!(
            binding_to_daemon_action(&open_file),
            Some(super::DaemonAction::OpenFile)
        );
    }

    #[test]
    fn parse_trigger_action_accepts_stable_open_file_protocol_name() {
        assert_eq!(
            parse_trigger_action("open_file"),
            Some(super::DaemonAction::OpenFile)
        );
        // Rust module paths must never become the external IPC protocol.
        assert_eq!(parse_trigger_action("capture_handlers::open_file"), None);
        assert_eq!(
            parse_trigger_action("super::capture_handlers::open_file"),
            None
        );
    }

    #[test]
    fn preview_dbus_member_is_stable_method_name() {
        // Client D-Bus member and zbus server method name must stay aligned and path-free.
        assert_eq!(SHOW_PREVIEW_FOR_PATH_MEMBER, "show_preview_for_path");
        assert!(!SHOW_PREVIEW_FOR_PATH_MEMBER.contains("::"));
        assert!(
            include_str!("mod.rs").contains("fn show_preview_for_path("),
            "D-Bus server method must remain show_preview_for_path"
        );
    }

    #[test]
    fn binding_to_daemon_action_maps_crosshair_capture_hotkey() {
        let crosshair = crate::hotkeys::HotkeyBinding {
            accelerator: "CTRL+ALT+X".into(),
            args: vec!["capture".into(), "crosshair".into()],
            name: Some("capture_crosshair".into()),
        };

        assert!(matches!(
            binding_to_daemon_action(&crosshair),
            Some(super::DaemonAction::CaptureCrosshair)
        ));
    }
}
