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
};

use anyhow::Context;
use ashpd::desktop::{
    remote_desktop::{Axis, DeviceType, KeyState, RemoteDesktop},
    screencast::{CursorMode, Screencast, SourceType},
    PersistMode, Session,
};

use crate::{
    backend::DisplayBackend,
    capture::{
        copy_capture_uri_to_clipboard, open_image_editor, save_capture, save_existing_png,
        SaveConfig,
    },
    capture_overlay::{
        capture_area_file_via_cpp, capture_screen_file_via_cpp, capture_window_file_via_cpp,
        AreaCapturePathResult,
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

#[derive(Debug, Clone)]
pub enum DaemonAction {
    CaptureArea,
    CaptureScreen,
    CaptureWindow,
    RecordScreen,
    RecordArea,
    ShowLastPreview,
    OpenLastCapture,
    OpenSettings,
    SetTrayVisible(bool),
    SetHotkeySuppressed(bool),
    ImportWebScrollCapture {
        png_base64: String,
        page_url: String,
        page_title: String,
    },
    Quit,
}

impl From<TrayAction> for DaemonAction {
    fn from(a: TrayAction) -> Self {
        match a {
            TrayAction::CaptureArea => DaemonAction::CaptureArea,
            TrayAction::CaptureScreen => DaemonAction::CaptureScreen,
            TrayAction::CaptureWindow => DaemonAction::CaptureWindow,
            TrayAction::RecordScreen => DaemonAction::RecordScreen,
            TrayAction::RecordArea => DaemonAction::RecordArea,
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
    /// Channel to send GTK work to the main OS thread. `None` when the daemon
    /// owns the main thread itself (legacy / test mode).
    gtk_tx: Option<std::sync::mpsc::Sender<GtkWork>>,
    /// Whether the Wayland compositor supports the Layer Shell protocol.
    /// Detected once on the GTK main thread (where GTK is initialized) and
    /// stored here so worker threads can read it without calling GTK APIs.
    layer_shell_supported: bool,
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
/// `layer_shell_supported` must be detected on the GTK main thread before calling this.
pub async fn run_daemon_with_gtk_channel(
    gtk_tx: std::sync::mpsc::Sender<GtkWork>,
    layer_shell_supported: bool,
) -> anyhow::Result<()> {
    run_daemon_inner(Some(gtk_tx), layer_shell_supported).await
}

pub async fn run_daemon() -> anyhow::Result<()> {
    run_daemon_inner(None, false).await
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

async fn run_daemon_inner(
    gtk_tx: Option<std::sync::mpsc::Sender<GtkWork>>,
    layer_shell_supported: bool,
) -> anyhow::Result<()> {
    eprintln!("[daemon] ApexShot daemon starting…");

    if should_autostart_ydotoold() {
        ensure_ydotoold_running();
    }

    if maybe_relaunch_via_desktop() {
        return Ok(());
    }

    let state = Arc::new(Mutex::new(DaemonState {
        last_capture_path: None,
        gtk_tx,
        layer_shell_supported,
    }));

    // Ensure GNOME Shell can associate this process with our desktop entry
    // even when the daemon is launched from a terminal.
    ensure_gio_desktop_env();

    // Main action channel — both tray and hotkeys send here.
    let (action_tx, action_rx) = std::sync::mpsc::channel::<DaemonAction>();

    // ── Tray icon ────────────────────────────────────────────────────────────
    let tray_enabled = load_config().sanitized().show_menu_bar_icon;
    let mut tray_handle = if tray_enabled {
        let handle = spawn_daemon_tray(&action_tx)?;
        eprintln!("[daemon] Tray icon active.");
        Some(handle)
    } else {
        eprintln!("[daemon] Tray icon disabled by settings.");
        None
    };

    // ── D-Bus IPC server ─────────────────────────────────────────────────────
    let dbus_tx = action_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = run_dbus_server(dbus_tx).await {
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
            DaemonAction::CaptureScreen => {
                tokio::task::spawn_blocking(move || handle_capture_screen(state_clone));
            }
            DaemonAction::CaptureWindow => {
                tokio::task::spawn_blocking(move || handle_capture_window(state_clone));
            }
            DaemonAction::RecordScreen => {
                tokio::spawn(handle_record_screen(action_tx_clone));
            }
            DaemonAction::RecordArea => {
                tokio::spawn(handle_record_area(action_tx_clone));
            }
            DaemonAction::ShowLastPreview => {
                let path = state.lock().unwrap().last_capture_path.clone();
                if let Some(p) = path {
                    tokio::task::spawn_blocking(move || show_preview_subprocess(p));
                } else {
                    eprintln!("[daemon] No capture yet.");
                }
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
                if visible {
                    if tray_handle.is_none() {
                        match spawn_daemon_tray(&action_tx) {
                            Ok(handle) => {
                                tray_handle = Some(handle);
                                eprintln!("[daemon] Tray icon enabled live.");
                            }
                            Err(e) => {
                                eprintln!("[daemon] Failed to enable tray icon live: {e}");
                            }
                        }
                    }
                } else if let Some(handle) = tray_handle.take() {
                    handle.shutdown();
                    eprintln!("[daemon] Tray icon disabled live.");
                }
            }
            DaemonAction::SetHotkeySuppressed(suppressed) => {
                HOTKEY_SUPPRESSED.store(suppressed, std::sync::atomic::Ordering::Relaxed);
                eprintln!(
                    "[daemon] Hotkey suppression {}.",
                    if suppressed { "enabled" } else { "disabled" }
                );
            }
            DaemonAction::ImportWebScrollCapture {
                png_base64,
                page_url,
                page_title,
            } => {
                tokio::task::spawn_blocking(move || {
                    handle_import_web_scroll_capture(png_base64, page_url, page_title, state_clone)
                });
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
            "capture_screen" => DaemonAction::CaptureScreen,
            "capture_window" => DaemonAction::CaptureWindow,
            "record_screen" => DaemonAction::RecordScreen,
            "record_area" => DaemonAction::RecordArea,
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

    fn import_web_scroll_capture(
        &self,
        png_base64: String,
        page_url: String,
        page_title: String,
    ) -> zbus::fdo::Result<bool> {
        eprintln!(
            "[daemon] D-Bus import_web_scroll_capture called (url={})",
            page_url
        );
        self.tx
            .send(DaemonAction::ImportWebScrollCapture {
                png_base64,
                page_url,
                page_title,
            })
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Daemon action channel unavailable: {e}"))
            })?;
        Ok(true)
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

async fn run_dbus_server(tx: std::sync::mpsc::Sender<DaemonAction>) -> anyhow::Result<()> {
    use zbus::connection::Builder;

    let ipc = DaemonIpc {
        tx,
        scroll_injector: tokio::sync::Mutex::new(ScrollInjector::default()),
    };

    let _conn = Builder::session()
        .context("Failed to get session D-Bus")?
        .name(DAEMON_BUS_NAME)
        .context("Failed to request D-Bus name")?
        .serve_at(DAEMON_OBJECT_PATH, ipc)
        .context("Failed to serve D-Bus object")?
        .build()
        .await
        .context("Failed to build D-Bus connection")?;

    // Mic: explicitly target physical input device to avoid picking up system audio
    // Falls back to default input if specific device not found
    start_audio_level_stream(
        "mic",
        "apexshot-mic-monitor",
        Some("alsa_input.pci-0000_00_1f.3.analog-stereo"),
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

/// Set GIO_LAUNCHED_DESKTOP_FILE env vars if not already set, so GNOME Shell
/// treats this process as a trusted desktop-launched application.
fn ensure_gio_desktop_env() {
    let app_id = std::env::var("APEXSHOT_APP_ID")
        .unwrap_or_else(|_| "io.github.codegoddy.apexshot".to_string());

    if let Ok(desktop_path) = ensure_desktop_entry_pub(&app_id) {
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
        .unwrap_or_else(|_| "io.github.codegoddy.apexshot".to_string());

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
        .unwrap_or_else(|_| "io.github.codegoddy.apexshot".to_string());

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
            "capture_screen" | "capture-screen" => return Some(DaemonAction::CaptureScreen),
            "capture_window" | "capture-window" => return Some(DaemonAction::CaptureWindow),
            "show_last_preview" | "show-last-preview" => {
                return Some(DaemonAction::ShowLastPreview);
            }
            "record_screen" | "record-screen" => return Some(DaemonAction::RecordScreen),
            "record_area" | "record-area" => return Some(DaemonAction::RecordArea),
            _ => {}
        }
    }

    // Fallback: derive action from the args list.
    match binding.args.get(0).map(|s| s.as_str()) {
        Some("capture") => match binding.args.get(1).map(|s| s.as_str()) {
            Some("area") => Some(DaemonAction::CaptureArea),
            Some("screen") => Some(DaemonAction::CaptureScreen),
            Some("window") => Some(DaemonAction::CaptureWindow),
            _ => None,
        },
        Some("show-last-preview") => Some(DaemonAction::ShowLastPreview),
        Some("record") => match binding.args.get(1).map(|s| s.as_str()) {
            Some("screen") => Some(DaemonAction::RecordScreen),
            Some("area") => Some(DaemonAction::RecordArea),
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

fn capture_full_screen_for_area_selector() -> Option<crate::backend::CaptureData> {
    eprintln!(
        "[capture] capture_full_screen_for_area_selector: starting background capture for selector"
    );
    if let Some(b) = make_wayland_backend() {
        eprintln!("[capture] capture_full_screen_for_area_selector: using Wayland backend (capture_screen_for_selection_impl)");
        let result = b.capture_screen_for_selection_impl().ok();
        match &result {
            Some(d) => eprintln!("[capture] capture_full_screen_for_area_selector: Wayland capture succeeded ({}x{})", d.width, d.height),
            None => eprintln!("[capture] capture_full_screen_for_area_selector: Wayland capture failed (returned None)"),
        }
        result
    } else if let Some(b) = make_x11_backend() {
        eprintln!("[capture] capture_full_screen_for_area_selector: using X11 backend");
        let result = <crate::backend::X11Backend as DisplayBackend>::capture_screen(&b).ok();
        match &result {
            Some(d) => eprintln!("[capture] capture_full_screen_for_area_selector: X11 capture succeeded ({}x{})", d.width, d.height),
            None => eprintln!("[capture] capture_full_screen_for_area_selector: X11 capture failed (returned None)"),
        }
        result
    } else {
        eprintln!("[capture] capture_full_screen_for_area_selector: no backend available");
        None
    }
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

fn screenshot_save_config() -> SaveConfig {
    let app_config = load_config().sanitized();
    let mut save_config = SaveConfig::default();
    if !app_config.export_location.is_empty() {
        save_config = save_config.with_output_dir(&app_config.export_location);
    }
    save_config
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
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets/sounds")
            .join(file_name),
        std::env::current_exe()
            .ok()
            .and_then(|exe| {
                exe.parent()
                    .map(|dir| dir.join("assets/sounds").join(file_name))
            })
            .unwrap_or_default(),
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

    if config.after_capture_copy_file_to_clipboard {
        if let Err(e) = copy_capture_uri_to_clipboard(&saved_path) {
            eprintln!("[daemon] Failed to copy screenshot URI to clipboard: {e}");
        }
    }

    if config.after_capture_open_annotate {
        if let Err(e) = open_image_editor(saved_path.clone()) {
            eprintln!("[daemon] Failed to open annotate editor: {e}");
        }
    }

    if config.after_capture_show_quick_access {
        show_preview_subprocess(saved_path);
    }
}

fn save_and_open(capture: crate::backend::CaptureData, state: Arc<Mutex<DaemonState>>) {
    let config = load_config().sanitized();
    if !config.after_capture_save {
        eprintln!(
            "[daemon] Screenshot discarded because Save is disabled in after-capture settings"
        );
        send_desktop_notification(
            "Screenshot not saved",
            "Save is disabled in After capture settings",
        );
        return;
    }

    match save_capture(&capture, &screenshot_save_config()) {
        Ok(path) => {
            let path: std::path::PathBuf = path;
            eprintln!("[daemon] Saved: {}", path.display());
            play_shutter_sound_if_enabled();
            apply_screenshot_after_capture_actions(path, state);
        }
        Err(e) => eprintln!("[daemon] Save error: {e}"),
    }
}

fn save_existing_png_and_open(path: std::path::PathBuf, state: Arc<Mutex<DaemonState>>) {
    let config = load_config().sanitized();
    if !config.after_capture_save {
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
) {
    use crate::backend::{CaptureData, PixelFormat};
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let decoded = match STANDARD.decode(png_base64.as_bytes()) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("[daemon] Web scroll import failed: invalid base64 payload: {e}");
            return;
        }
    };

    let dyn_image = match image::load_from_memory(&decoded) {
        Ok(img) => img,
        Err(e) => {
            eprintln!("[daemon] Web scroll import failed: invalid image payload: {e}");
            return;
        }
    };

    let rgba = dyn_image.to_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    let pixels = rgba.into_raw();

    if width == 0 || height == 0 {
        eprintln!("[daemon] Web scroll import failed: empty image");
        return;
    }

    eprintln!(
        "[daemon] Importing web scroll capture ({}x{}, url={}, title={})",
        width, height, page_url, page_title
    );

    let capture = CaptureData::new(pixels, width, height, PixelFormat::RGBA32);
    save_and_open(capture, state);
}

fn run_ocr_and_report(capture: crate::backend::CaptureData) {
    eprintln!("[daemon] OCR tool selected — extracting text from selected area...");
    match extract_text(&capture, &OcrConfig::default()) {
        Ok(result) => {
            eprintln!(
                "[daemon] OCR successful (confidence: {}%)",
                result.confidence
            );
            if result.copied_to_clipboard {
                send_desktop_notification("OCR complete", "Text copied to clipboard");
            } else {
                send_desktop_notification(
                    "OCR complete",
                    "Text extracted, but clipboard copy was unavailable",
                );
            }
        }
        Err(err) => {
            eprintln!("[daemon] OCR failed: {err}");
            send_desktop_notification("OCR failed", &err.to_string());
        }
    }
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
fn show_preview_subprocess(path: std::path::PathBuf) {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));
    match std::process::Command::new(&exe)
        .arg("preview")
        .arg(&path)
        .spawn()
    {
        Ok(_) => {}
        Err(e) => {
            eprintln!("[daemon] Failed to spawn preview subprocess: {e}, falling back to xdg-open");
            open_file(path);
        }
    }
}

fn show_settings_subprocess() {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));

    if let Err(e) = std::process::Command::new(&exe).arg("settings").spawn() {
        eprintln!("[daemon] Failed to spawn settings window: {e}");
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

fn handle_capture_area(state: Arc<Mutex<DaemonState>>) {
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
            return;
        }
        Ok(AreaCapturePathResult::ScrollCaptured(path)) => {
            save_existing_png_and_open(path, state);
            return;
        }
        Ok(AreaCapturePathResult::OcrRequested(capture)) => {
            run_ocr_and_report(capture);
            return;
        }
        Ok(AreaCapturePathResult::Cancelled) => {
            eprintln!("[daemon] Area selection cancelled.");
            return;
        }
        Ok(AreaCapturePathResult::RecordingRequested(request)) => {
            if let Err(err) = run_overlay_recording_request_with_gtk(request, gtk_tx.clone()) {
                eprintln!("[daemon] Recording failed: {err}");
            }
            return;
        }
        Err(err) => {
            eprintln!(
                "[daemon] C++ area-init capture path failed ({err}); falling back to Rust backend."
            );
        }
    }

    eprintln!(
        "[daemon] handle_capture_area: START — thread={:?}",
        std::thread::current().id()
    );
    let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    eprintln!("[daemon] handle_capture_area: is_wayland={is_wayland}");
    eprintln!(
        "[daemon] handle_capture_area: gtk_tx available={}",
        gtk_tx.is_some()
    );

    if is_wayland {
        let Some(backend) = make_wayland_backend() else {
            eprintln!("[daemon] No Wayland backend available for area capture.");
            return;
        };

        // On Wayland compositors that don't support the Layer Shell protocol
        // (e.g. GNOME Shell), a transparent live overlay is impossible.
        // This was detected on the GTK main thread at startup and stored in state.
        let layer_shell_supported = state.lock().unwrap().layer_shell_supported;
        eprintln!("[daemon] handle_capture_area: layer_shell_supported={layer_shell_supported}");

        // Use the C++ overlay with screenshot background for area selection.
        // Capture full screen first, save as temp PNG, pass to C++ overlay.
        let state_for_window = state.clone();
        let run_cpp_selector_fn =
            |bg: Option<&std::path::Path>| -> crate::overlay::SelectionResult {
                let result = crate::capture_overlay::run_capture_overlay_with_window(bg);
                // Handle sentinels from window picker toolbar
                if let Ok(Some(ref area)) = result {
                    if area.x == i32::MIN {
                        eprintln!("[daemon] Window capture sentinel");
                        match capture_window_file_via_cpp() {
                            Ok(path) => save_existing_png_and_open(path, state_for_window.clone()),
                            Err(e) => eprintln!("[daemon] Window capture failed: {e}"),
                        }
                        return Ok(None);
                    } else if area.x == i32::MIN + 1 {
                        eprintln!("[daemon] Switching to area mode from window picker");
                        match capture_area_file_via_cpp() {
                            Ok(AreaCapturePathResult::Captured(path)) => {
                                save_existing_png_and_open(path, state_for_window.clone())
                            }
                            Ok(AreaCapturePathResult::ScrollCaptured(path)) => {
                                save_existing_png_and_open(path, state_for_window.clone())
                            }
                            Ok(AreaCapturePathResult::OcrRequested(capture)) => {
                                run_ocr_and_report(capture)
                            }
                            Ok(AreaCapturePathResult::Cancelled) => {
                                eprintln!("[daemon] Area capture cancelled")
                            }
                            Ok(AreaCapturePathResult::RecordingRequested(request)) => {
                                if let Err(err) =
                                    run_overlay_recording_request_with_gtk(request, gtk_tx.clone())
                                {
                                    eprintln!("[daemon] Recording failed: {err}");
                                }
                            }
                            Err(e) => eprintln!("[daemon] Area capture failed: {e}"),
                        }
                        return Ok(None);
                    } else if area.x == i32::MIN + 2 {
                        eprintln!("[daemon] Switching to fullscreen from window picker");
                        match capture_screen_file_via_cpp() {
                            Ok(path) => save_existing_png_and_open(path, state_for_window.clone()),
                            Err(e) => eprintln!("[daemon] Fullscreen capture failed: {e}"),
                        }
                        return Ok(None);
                    }
                }
                result
            };

        let run_live_selector = || -> crate::overlay::SelectionResult {
            let bg_capture = backend.capture_screen_for_selection_impl().ok();
            let tmp_bg = bg_capture.as_ref().and_then(|c| save_temp_png_daemon(c));
            let result = run_cpp_selector_fn(tmp_bg.as_deref());
            if let Some(ref p) = tmp_bg {
                let _ = std::fs::remove_file(p);
            }
            result
        };

        // Fast path: show transparent live overlay immediately.
        // Skip on compositors that don't support Layer Shell (e.g. GNOME Wayland) —
        // we use the capture-after-selection (Option B) path below instead.
        if !layer_shell_supported {
            eprintln!(
                "[daemon] Wayland compositor does not support Layer Shell; \
                 will use dark-overlay selector + capture-after-selection."
            );
        }

        let live_selector_result = if layer_shell_supported {
            Some(run_live_selector())
        } else {
            None
        };

        // If we ran the live selector, process its result.
        // None means we skipped it (no layer-shell), so fall through to screenshot-backed path.
        match live_selector_result {
            Some(Ok(Some(area))) => {
                match backend.capture_area_direct_impl(area.x, area.y, area.width, area.height) {
                    Ok(capture) => {
                        save_and_open(capture, state.clone());
                        return;
                    }
                    Err(err) => {
                        eprintln!(
                            "[daemon] Wayland area capture failed ({err}); trying full-screen crop fallback."
                        );

                        match backend.capture_screen_impl() {
                            Ok(full) => {
                                let x = area.x.max(0) as u32;
                                let y = area.y.max(0) as u32;
                                let w = area.width.max(1) as u32;
                                let h = area.height.max(1) as u32;
                                if let Some(cropped) = crop_capture_data(&full, x, y, w, h) {
                                    save_and_open(cropped, state.clone());
                                    return;
                                }
                                eprintln!("[daemon] Full-screen crop fallback was out of bounds.");
                            }
                            Err(full_err) => {
                                eprintln!(
                                    "[daemon] Full-screen fallback capture failed: {full_err}"
                                );
                            }
                        }
                    }
                }
            }
            Some(Ok(None)) => {
                eprintln!("[daemon] Area selection cancelled.");
                return;
            }
            Some(Err(err)) => {
                eprintln!(
                    "[daemon] Live selector unavailable ({err}); falling back to screenshot-backed selector."
                );
            }
            None => {
                // Layer shell not supported — go straight to screenshot-backed selector.
            }
        }

        // Option B path: on compositors without Layer Shell (e.g. GNOME Wayland),
        // show the dark-overlay selector UI FIRST (no pre-capture, no flash/sound),
        // then capture only the selected region AFTER the user confirms.
        // The flash/sound from the portal fires after selection — feels like a shutter.
        eprintln!("[daemon] No Layer Shell — using capture-after-selection (Option B): showing dark overlay selector first.");
        let selector_start = std::time::Instant::now();
        let area_result = run_live_selector();
        eprintln!(
            "[daemon] Dark overlay selector returned after {:.0}ms: {:?}",
            selector_start.elapsed().as_millis(),
            area_result.as_ref().map(|o| o.as_ref().map(|_| "area"))
        );
        match area_result {
            Ok(Some(area)) => {
                eprintln!("[daemon] Selection confirmed: ({}, {}, {}x{}); now capturing selected region...", area.x, area.y, area.width, area.height);
                let capture_start = std::time::Instant::now();
                match backend.capture_area_direct_impl(area.x, area.y, area.width, area.height) {
                    Ok(capture) => {
                        eprintln!(
                            "[daemon] Region capture succeeded in {:.0}ms ({}x{})",
                            capture_start.elapsed().as_millis(),
                            capture.width,
                            capture.height
                        );
                        save_and_open(capture, state.clone());
                    }
                    Err(err) => {
                        eprintln!("[daemon] Region capture failed ({:.0}ms): {err}; falling back to full-screen capture + crop.", capture_start.elapsed().as_millis());
                        // Fallback: full-screen capture then crop
                        match backend.capture_screen_impl() {
                            Ok(full) => {
                                let x = area.x.max(0) as u32;
                                let y = area.y.max(0) as u32;
                                let w = area.width.max(1) as u32;
                                let h = area.height.max(1) as u32;
                                if let Some(cropped) = crop_capture_data(&full, x, y, w, h) {
                                    save_and_open(cropped, state.clone());
                                } else {
                                    eprintln!("[daemon] Crop out of bounds.");
                                }
                            }
                            Err(full_err) => eprintln!(
                                "[daemon] Full-screen fallback capture failed: {full_err}"
                            ),
                        }
                    }
                }
            }
            Ok(None) => eprintln!("[daemon] Area selection cancelled."),
            Err(err) => eprintln!("[daemon] Dark overlay selector failed: {err}"),
        }
        return;
    }

    // X11 path: capture full screen, save as temp PNG, pass to C++ overlay.
    let Some(full) = capture_full_screen_for_area_selector() else {
        eprintln!("[daemon] Could not capture screen for area selector.");
        return;
    };

    let tmp_bg = save_temp_png_daemon(&full);
    let area_opt = match crate::capture_overlay::run_capture_overlay(tmp_bg.as_deref()) {
        Ok(area) => area,
        Err(e) => {
            eprintln!("[daemon] C++ overlay failed: {e}");
            None
        }
    };
    if let Some(ref p) = tmp_bg {
        let _ = std::fs::remove_file(p);
    }

    match area_opt {
        Some(area) => {
            // Crop from the already-captured frame.
            let x = area.x.max(0) as u32;
            let y = area.y.max(0) as u32;
            let w = area.width.max(1) as u32;
            let h = area.height.max(1) as u32;

            if let Some(cropped) = crop_capture_data(&full, x, y, w, h) {
                save_and_open(cropped, state);
            } else {
                eprintln!("[daemon] Crop out of bounds.");
            }
        }
        None => eprintln!("[daemon] Area selection cancelled."),
    }
}

fn handle_capture_screen(state: Arc<Mutex<DaemonState>>) {
    match capture_screen_file_via_cpp() {
        Ok(path) => {
            save_existing_png_and_open(path, state);
            return;
        }
        Err(err) => {
            eprintln!(
                "[daemon] C++ fullscreen capture failed ({err}); falling back to Rust backend."
            );
        }
    }

    match capture_full_screen() {
        Some(c) => save_and_open(c, state),
        None => eprintln!("[daemon] No backend available for screen capture."),
    }
}

fn handle_capture_window(state: Arc<Mutex<DaemonState>>) {
    eprintln!("[daemon] Window capture requested — using the shared window capture flow");
    match capture_window_file_via_cpp() {
        Ok(path) => {
            save_existing_png_and_open(path, state);
        }
        Err(e) => {
            eprintln!("[daemon] Window capture failed: {e}; falling back to area capture.");
            handle_capture_area(state);
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

async fn handle_record_area(_tx: std::sync::mpsc::Sender<DaemonAction>) {
    use crate::capture_overlay::run_capture_overlay;
    use crate::recording::{
        run_recording_with_controls, RecordingConfig, RecordingControlsParams, StopAction,
    };

    eprintln!("[daemon] Selecting area for recording…");

    // Show C++ overlay on a blocking thread.
    let selection = tokio::task::spawn_blocking(|| run_capture_overlay(None)).await;

    let cpp_area = match selection {
        Ok(Ok(Some(a))) => a,
        Ok(Ok(None)) => {
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

    let mut config = RecordingConfig::default();
    config.x = Some(area.x);
    config.y = Some(area.y);
    config.width = Some(area.width as u32);
    config.height = Some(area.height as u32);

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
    use super::should_autostart_ydotoold;

    #[test]
    fn daemon_does_not_autostart_ydotoold_by_default() {
        assert!(!should_autostart_ydotoold());
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
}
