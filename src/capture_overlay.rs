//! Bridge to the native C++ Qt5 capture overlay binary (`apexshot-capture`).
//!
//! The binary is compiled from `capture-overlay/` by `build.rs` and placed
//! next to the Rust binary. This module finds and runs it as a subprocess,
//! parses the JSON output, and returns selection/capture data.
//!
//! Protocol:
//!   overlay mode: exit 0 + `{"x":N,"y":N,"width":N,"height":N}`
//!   capture mode: exit 0 + `{"path":"/tmp/...png",...}`
//!   exit 1 → cancelled by user
//!   exit 2 → error

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::{
    io::Write,
    os::unix::{net::UnixStream, process::ExitStatusExt},
};

use crate::{
    backend::{CaptureData, DisplayBackend, PixelFormat, WaylandBackend},
    gnome_integration::{emit_tracked_window_closed, emit_tracked_window_opened},
    overlay::{OverlaySelection, SelectionArea, SelectionError, SelectionResult},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureSessionState {
    Idle,
    ApexOverlayActive,
    BuiltinOverlayActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchBlockedReason {
    ApexOverlayAlreadyActive,
    BuiltinOverlayActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum OverlayExitCode {
    Cancelled = 1,
    Error = 2,
    WindowCaptureRequested = 3,
    SwitchToArea = 4,
    SwitchToFullscreen = 5,
    RecordConfigUpdated = 6,
    ForwardedToExistingOverlay = 10,
    BlockedByBuiltinOverlay = 11,
}

fn current_desktop_contains(needle: &str) -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split([':', ';', ','])
        .any(|part| part.trim().eq_ignore_ascii_case(needle))
}

fn should_use_gtk_layer_shell_selector() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
        || std::env::var_os("SWAYSOCK").is_some()
        || current_desktop_contains("Hyprland")
        || current_desktop_contains("sway")
}

fn force_wayland_gdk_for_layer_shell() {
    // On Hyprland/sway, GDK may default to X11 backend (via XWayland) because
    // DISPLAY=:0 is set. Layer-shell requires the Wayland GDK backend.
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("GDK_BACKEND").is_none() {
        // SAFETY: This must be set before any GTK calls. Callers invoke this
        // before entering the GTK layer-shell selector.
        unsafe { std::env::set_var("GDK_BACKEND", "wayland") };
    }
}

fn wait_for_layer_shell_overlay_to_unmap() {
    // Layer-shell window destruction is acknowledged by the compositor just
    // after GTK returns. Give Hyprland/sway one frame to remove our overlay
    // before wlr-screencopy grabs the real screenshot, otherwise the capture
    // can include ApexShot's own UI.
    std::thread::sleep(std::time::Duration::from_millis(180));
}

fn capture_area_file_via_gtk_layer_shell_wlroots() -> Result<AreaCapturePathResult, SelectionError>
{
    force_wayland_gdk_for_layer_shell();

    let backend = WaylandBackend::new()
        .map_err(|err| SelectionError::InitError(format!("Wayland backend unavailable: {err}")))?;
    let full_capture = backend
        .capture_screen_for_selection_impl()
        .or_else(|_| backend.capture_screen())
        .map_err(|err| {
            SelectionError::InitError(format!("Wayland background capture failed: {err}"))
        })?;
    match crate::overlay::select_area_from_capture_with_gtk(&full_capture) {
        Ok(crate::overlay::OverlaySelection::Area(Some(area))) => {
            wait_for_layer_shell_overlay_to_unmap();
            let capture = backend
                .capture_area(area.x, area.y, area.width, area.height)
                .map_err(|err| {
                    SelectionError::InitError(format!("Wayland area capture failed: {err}"))
                })?;
            save_capture_to_temp_png(&capture).map(AreaCapturePathResult::Captured)
        }
        Ok(crate::overlay::OverlaySelection::Area(None)) => Err(SelectionError::Cancelled),
        Ok(crate::overlay::OverlaySelection::Recording(request)) => {
            wait_for_layer_shell_overlay_to_unmap();
            Ok(AreaCapturePathResult::RecordingRequested(request))
        }
        Err(SelectionError::WindowCaptureRequested) => {
            eprintln!("[capture] Window capture requested from GTK overlay — using Wayland portal");
            wait_for_layer_shell_overlay_to_unmap();
            let capture = backend.capture_window(0).map_err(|err| {
                SelectionError::InitError(format!("Wayland window capture failed: {err}"))
            })?;
            save_capture_to_temp_png(&capture).map(AreaCapturePathResult::Captured)
        }
        Err(SelectionError::OcrRequested(area)) => {
            eprintln!("[capture] OCR requested from GTK overlay — capturing area for OCR");
            wait_for_layer_shell_overlay_to_unmap();
            backend
                .capture_area(area.x, area.y, area.width, area.height)
                .map(AreaCapturePathResult::OcrRequested)
                .map_err(|err| {
                    SelectionError::InitError(format!("Wayland area capture for OCR failed: {err}"))
                })
        }
        Err(e) => Err(e),
    }
}

fn capture_area_via_gtk_layer_shell_wlroots() -> Result<AreaCaptureResult, SelectionError> {
    match capture_area_file_via_gtk_layer_shell_wlroots()? {
        AreaCapturePathResult::Captured(path) => {
            let capture = load_capture_data_from_path(&path);
            let _ = std::fs::remove_file(&path);
            capture.map(AreaCaptureResult::Captured)
        }
        AreaCapturePathResult::OcrRequested(capture) => {
            Ok(AreaCaptureResult::OcrRequested(capture))
        }
        AreaCapturePathResult::Cancelled => Ok(AreaCaptureResult::Cancelled),
        AreaCapturePathResult::ScrollCaptured(path) => {
            let capture = load_capture_data_from_path(&path);
            let _ = std::fs::remove_file(&path);
            capture.map(AreaCaptureResult::ScrollCaptured)
        }
        AreaCapturePathResult::RecordingRequested(request) => {
            Ok(AreaCaptureResult::RecordingRequested(request))
        }
        AreaCapturePathResult::RecordingConfigUpdated => Ok(AreaCaptureResult::Cancelled),
    }
}

fn capture_crosshair_file_via_gtk_layer_shell_wlroots() -> Result<PathBuf, SelectionError> {
    force_wayland_gdk_for_layer_shell();

    let backend = WaylandBackend::new()
        .map_err(|err| SelectionError::InitError(format!("Wayland backend unavailable: {err}")))?;
    let full_capture = backend
        .capture_screen_for_selection_impl()
        .or_else(|_| backend.capture_screen())
        .map_err(|err| {
            SelectionError::InitError(format!("Wayland background capture failed: {err}"))
        })?;
    let area = match crate::overlay::select_crosshair_from_capture_with_gtk(&full_capture)? {
        OverlaySelection::Area(Some(area)) => area,
        OverlaySelection::Area(None) => return Err(SelectionError::Cancelled),
        OverlaySelection::Recording(_) => return Err(SelectionError::Cancelled),
    };
    wait_for_layer_shell_overlay_to_unmap();
    let capture = backend
        .capture_area(area.x, area.y, area.width, area.height)
        .map_err(|err| SelectionError::InitError(format!("Wayland area capture failed: {err}")))?;

    save_capture_to_temp_png(&capture)
}

fn capture_crosshair_via_gtk_layer_shell_wlroots() -> Result<AreaCaptureResult, SelectionError> {
    let path = capture_crosshair_file_via_gtk_layer_shell_wlroots()?;
    let capture = load_capture_data_from_path(&path);
    let _ = std::fs::remove_file(&path);
    capture.map(AreaCaptureResult::Captured)
}

#[derive(Debug)]
pub struct CaptureSessionCoordinator {
    state: Mutex<CaptureSessionState>,
}

impl Default for CaptureSessionCoordinator {
    fn default() -> Self {
        Self {
            state: Mutex::new(CaptureSessionState::Idle),
        }
    }
}

#[must_use]
pub struct CaptureOverlayGuard<'a> {
    coordinator: &'a CaptureSessionCoordinator,
}

#[derive(Debug)]
struct InteractiveOverlaySessionGuard {
    tracked_overlay_id: Option<String>,
}

impl CaptureSessionCoordinator {
    pub fn begin_apex_overlay_session(
        &self,
        builtin_overlay_active: bool,
    ) -> Result<CaptureOverlayGuard<'_>, LaunchBlockedReason> {
        let mut state = self.state.lock().expect("capture session mutex poisoned");
        if matches!(*state, CaptureSessionState::ApexOverlayActive) {
            return Err(LaunchBlockedReason::ApexOverlayAlreadyActive);
        }
        if builtin_overlay_active {
            *state = CaptureSessionState::BuiltinOverlayActive;
            *state = CaptureSessionState::Idle;
            return Err(LaunchBlockedReason::BuiltinOverlayActive);
        }
        *state = CaptureSessionState::ApexOverlayActive;
        drop(state);
        Ok(CaptureOverlayGuard { coordinator: self })
    }
}

impl Drop for CaptureOverlayGuard<'_> {
    fn drop(&mut self) {
        let mut state = self
            .coordinator
            .state
            .lock()
            .expect("capture session mutex poisoned");
        *state = CaptureSessionState::Idle;
    }
}

fn capture_session_coordinator() -> &'static CaptureSessionCoordinator {
    static COORDINATOR: OnceLock<CaptureSessionCoordinator> = OnceLock::new();
    COORDINATOR.get_or_init(CaptureSessionCoordinator::default)
}

fn classify_overlay_exit_code(
    code: Option<i32>,
) -> Result<Option<&'static str>, LaunchBlockedReason> {
    match code {
        Some(code) if code == OverlayExitCode::ForwardedToExistingOverlay as i32 => {
            Ok(Some("forwarded"))
        }
        Some(code) if code == OverlayExitCode::BlockedByBuiltinOverlay as i32 => {
            Err(LaunchBlockedReason::BuiltinOverlayActive)
        }
        _ => Ok(None),
    }
}

const OVERLAY_FOCUS_REQUEST: &str = "focus";
const OVERLAY_CANCEL_REQUEST: &str = "cancel";

fn overlay_socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("apexshot-capture-overlay.sock")
}

pub fn request_existing_overlay_focus() -> bool {
    send_overlay_socket_request(OVERLAY_FOCUS_REQUEST)
}

pub fn request_existing_overlay_cancel() -> bool {
    send_overlay_socket_request(OVERLAY_CANCEL_REQUEST)
}

fn send_overlay_socket_request(request: &str) -> bool {
    #[cfg(unix)]
    {
        match UnixStream::connect(overlay_socket_path()) {
            Ok(mut stream) => {
                let _ = stream.write_all(request.as_bytes());
                let _ = stream.write_all(b"\n");
                true
            }
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn overlay_socket_is_listening() -> bool {
    #[cfg(unix)]
    {
        UnixStream::connect(overlay_socket_path()).is_ok()
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn is_gnome_session() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .map(|desktop| {
            desktop
                .split(':')
                .any(|part| part.trim().eq_ignore_ascii_case("gnome"))
        })
        .unwrap_or(false)
}

fn execute_builtin_overlay_query<F>(query: F) -> bool
where
    F: FnOnce() -> bool + Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        return std::thread::spawn(query).join().unwrap_or(false);
    }

    query()
}

pub fn builtin_screenshot_overlay_active() -> bool {
    if !is_gnome_session() {
        return false;
    }

    execute_builtin_overlay_query(|| {
        let Ok(conn) = zbus::blocking::Connection::session() else {
            return false;
        };
        let Ok(proxy) = zbus::blocking::Proxy::new(
            &conn,
            "org.gnome.Shell",
            "/org/gnome/Shell",
            "org.gnome.Shell",
        ) else {
            return false;
        };

        let script = "(() => { try { const Main = imports.ui.main; return !!(Main.screenshotUI && Main.screenshotUI.visible); } catch (e) { return false; } })()";
        let Ok((success, value)) = proxy.call::<_, _, (bool, String)>("Eval", &(script)) else {
            return false;
        };
        if !success {
            return false;
        }

        let normalized = value.trim().trim_matches('"');
        matches!(normalized, "true" | "1")
    })
}

pub fn begin_capture_session() -> Result<CaptureOverlayGuard<'static>, LaunchBlockedReason> {
    capture_session_coordinator().begin_apex_overlay_session(builtin_screenshot_overlay_active())
}

pub fn is_launch_blocked_error(err: &SelectionError) -> bool {
    matches!(err, SelectionError::Blocked(_))
}

fn blocked_selection_error(reason: LaunchBlockedReason) -> SelectionError {
    match reason {
        LaunchBlockedReason::ApexOverlayAlreadyActive => {
            SelectionError::Blocked("ApexShot capture overlay is already active".into())
        }
        LaunchBlockedReason::BuiltinOverlayActive => {
            SelectionError::Blocked("GNOME screenshot UI is already active".into())
        }
    }
}

#[cfg(unix)]
fn synthetic_output(status_code: i32) -> Output {
    Output {
        status: std::process::ExitStatus::from_raw(status_code << 8),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

/// Find the `apexshot-capture` binary.
///
/// Search order:
/// 1. `APEXSHOT_CAPTURE_BIN` env variable (manual override).
/// 2. Installed system paths (/usr/bin, /usr/local/bin).
/// 3. Same directory as the currently-running executable.
/// 4. Debug build output directory embedded by build.rs via `APEXSHOT_CAPTURE_BIN_DIR`.
/// 5. Common target profile directories relative to the exe (handles `cargo run` edge cases).
/// 6. PATH lookup.
fn find_capture_binary() -> Option<PathBuf> {
    // 1. Env override — highest priority for manual testing
    if let Some(p) = std::env::var_os("APEXSHOT_CAPTURE_BIN") {
        let path = PathBuf::from(p);
        if path.exists() {
            eprintln!(
                "[capture_overlay] Found apexshot-capture via env: {}",
                path.display()
            );
            return Some(path);
        }
    }

    // 2. Installed system paths — for .deb and manual installations
    if PathBuf::from("/usr/bin/apexshot-capture").exists() {
        eprintln!("[capture_overlay] Found apexshot-capture at /usr/bin/apexshot-capture");
        return Some(PathBuf::from("/usr/bin/apexshot-capture"));
    }
    if PathBuf::from("/usr/local/bin/apexshot-capture").exists() {
        eprintln!("[capture_overlay] Found apexshot-capture at /usr/local/bin/apexshot-capture");
        return Some(PathBuf::from("/usr/local/bin/apexshot-capture"));
    }

    // 3. Same directory as the running executable — useful for installed bundles.
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe.with_file_name("apexshot-capture");
        if candidate.exists() {
            eprintln!(
                "[capture_overlay] Found apexshot-capture next to exe: {}",
                candidate.display()
            );
            return Some(candidate);
        }
    }

    // 4. Debug build-time output directory embedded by build.rs.
    if let Some(dir) = option_env!("APEXSHOT_CAPTURE_BIN_DIR") {
        let candidate = PathBuf::from(dir).join("apexshot-capture");
        if candidate.exists() {
            eprintln!(
                "[capture_overlay] Found apexshot-capture via build dir: {}",
                candidate.display()
            );
            return Some(candidate);
        }
    }

    // 4. Walk up from exe dir to find target/release or target/debug
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        while let Some(d) = dir {
            for profile in &["release", "debug"] {
                let candidate = d.join(profile).join("apexshot-capture");
                if candidate.exists() {
                    eprintln!(
                        "[capture_overlay] Found apexshot-capture in target/{}: {}",
                        profile,
                        candidate.display()
                    );
                    return Some(candidate);
                }
            }
            let candidate = d.join("apexshot-capture");
            if candidate.exists() && candidate != exe.with_file_name("apexshot-capture") {
                eprintln!(
                    "[capture_overlay] Found apexshot-capture in parent dir: {}",
                    candidate.display()
                );
                return Some(candidate);
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }

    // 5. PATH
    eprintln!("[capture_overlay] Searching PATH for apexshot-capture");
    which_in_path("apexshot-capture")
}

fn which_in_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(name);
            if full.exists() {
                Some(full)
            } else {
                None
            }
        })
    })
}

fn run_capture_binary(
    extra_args: &[&str],
    background_png: Option<&Path>,
) -> Result<Output, SelectionError> {
    if builtin_screenshot_overlay_active() {
        return Err(blocked_selection_error(
            LaunchBlockedReason::BuiltinOverlayActive,
        ));
    }

    let binary = find_capture_binary().ok_or_else(|| {
        SelectionError::InitError(
            "apexshot-capture binary not found. \
             Re-run `cargo build --release` to compile it, or check your PATH."
                .into(),
        )
    })?;

    // Capture requests often originate from the autostart daemon, which uses a
    // different desktop identity for tray/hotkey purposes. Override that
    // identity while spawning the capture helper so xdg-desktop-portal stores
    // screenshot/screencast grants against the main ApexShot desktop file.
    let _portal_identity = crate::utils::desktop_env::scoped_portal_capture_identity();

    let mut interactive_session = InteractiveOverlaySessionGuard::begin(extra_args);

    let mut cmd = Command::new(&binary);
    cmd.env("QT_IM_MODULE", "compose")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    for arg in extra_args {
        cmd.arg(arg);
    }

    if let Some(bg) = background_png {
        cmd.arg("--background").arg(bg);
    }

    let child = cmd.spawn().map_err(|e| {
        SelectionError::InitError(format!(
            "Failed to launch apexshot-capture ({}): {}",
            binary.display(),
            e
        ))
    })?;
    interactive_session.attach_child_pid(child.id());
    let output = child.wait_with_output().map_err(|e| {
        SelectionError::InitError(format!(
            "Failed to wait for apexshot-capture ({}): {}",
            binary.display(),
            e
        ))
    })?;

    match classify_overlay_exit_code(output.status.code()) {
        Ok(Some("forwarded")) => {
            #[cfg(unix)]
            {
                return Ok(synthetic_output(
                    OverlayExitCode::ForwardedToExistingOverlay as i32,
                ));
            }
            #[cfg(not(unix))]
            {
                return Ok(output);
            }
        }
        Err(reason) => return Err(blocked_selection_error(reason)),
        _ => {}
    }

    Ok(output)
}

impl InteractiveOverlaySessionGuard {
    fn begin(extra_args: &[&str]) -> Self {
        if !should_request_screenshot_lock(extra_args) || overlay_socket_is_listening() {
            return Self {
                tracked_overlay_id: None,
            };
        }

        let session_id = next_screenshot_lock_session_id();

        Self {
            tracked_overlay_id: Some(tracked_overlay_id(&session_id)),
        }
    }

    fn attach_child_pid(&mut self, pid: u32) {
        let Some(tracked_id) = self.tracked_overlay_id.as_deref() else {
            return;
        };

        emit_tracked_window_opened(
            tracked_id,
            pid,
            "ApexShot Capture",
            "capture-overlay",
            "screenshot",
        );
    }
}

impl Drop for InteractiveOverlaySessionGuard {
    fn drop(&mut self) {
        if let Some(tracked_id) = self.tracked_overlay_id.take() {
            emit_tracked_window_closed(&tracked_id);
        }
    }
}

fn should_request_screenshot_lock(extra_args: &[&str]) -> bool {
    if extra_args.is_empty() {
        return true;
    }

    extra_args.iter().any(|arg| {
        matches!(
            *arg,
            "--area-init" | "--window-capture" | "--crosshair-capture"
        )
    })
}

fn next_screenshot_lock_session_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("screenshot-{}-{now}", std::process::id())
}

fn tracked_overlay_id(session_id: &str) -> String {
    format!("capture-overlay-{session_id}")
}

pub fn spawn_recording_controls_via_cpp(
    dbus_dest: &str,
    session_id: &str,
    params: crate::recording::RecordingControlsParams,
) -> anyhow::Result<Child> {
    let binary = find_capture_binary().ok_or_else(|| {
        anyhow::anyhow!(
            "apexshot-capture binary not found. Re-run `cargo build --release` to compile it, or check your PATH."
        )
    })?;

    let mut cmd = Command::new(&binary);
    cmd.env("QT_IM_MODULE", "compose")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .arg("--record-controls")
        .arg(format!("--dbus-dest={dbus_dest}"))
        .arg(format!("--session-id={session_id}"))
        .arg(format!("--capture-x={}", params.capture_x))
        .arg(format!("--capture-y={}", params.capture_y))
        .arg(format!("--capture-w={}", params.capture_w))
        .arg(format!("--capture-h={}", params.capture_h));

    if params.is_fullscreen {
        cmd.arg("--fullscreen");
    }
    if params.show_timer {
        cmd.arg("--show-timer");
    } else {
        cmd.arg("--hide-timer");
    }

    cmd.spawn().map_err(|e| {
        anyhow::anyhow!(
            "Failed to launch apexshot-capture record-controls ({}): {}",
            binary.display(),
            e
        )
    })
}

/// Result of running the capture overlay — either an area selection or a
/// full window capture (when user clicks the Window toolbar button).
pub enum OverlayResult {
    /// User selected an area — coordinates to crop from.
    Area(SelectionArea),
    /// User clicked Window tool — full window pixel data already captured.
    Window(CaptureData),
    /// User cancelled.
    Cancelled,
}

/// Result of area capture initiation through the C++ overlay.
#[derive(Debug)]
pub enum AreaCaptureResult {
    Captured(CaptureData),
    ScrollCaptured(CaptureData),
    OcrRequested(CaptureData),
    RecordingRequested(RecordingRequest),
    Cancelled,
}

/// Recording request from the capture overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingRequest {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub record_type: RecordingType,
    pub controls: bool,
    pub mic: bool,
    pub speaker: bool,
    pub clicks: bool,
    pub keystrokes: bool,
    // Runtime overlay settings
    pub webcam: bool,
    pub click_size: f64,
    pub click_color: u8,
    pub click_style: u8,
    pub click_animate: bool,
    pub key_size: f64,
    pub key_position: u8,
    pub key_appearance: u8,
    pub key_blur_bg: bool,
    pub key_filter: u8,
    pub webcam_size: u8,
    pub webcam_shape: u8,
    pub webcam_flip: bool,
    pub webcam_device: i32,
    pub webcam_rel_x: f64,
    pub webcam_rel_y: f64,
    // General tab settings
    pub display_rec_time: bool,
    pub hidpi: bool,
    pub notifications: bool,
    pub cursor: bool,
    pub remember_selection: bool,
    pub dim_screen: bool,
    pub countdown: bool,
    // Video tab settings
    pub video_format: u8,
    pub video_max_res: u8,
    pub video_fps: u8,
    pub record_mono: bool,
    pub open_editor: bool,
    // GIF tab settings
    pub gif_fps: u8,
    pub gif_quality: f64,
    pub gif_size_idx: u8,
    pub optimize_gif: bool,
    pub fullscreen: bool,
}

impl Default for RecordingRequest {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            record_type: RecordingType::Video,
            controls: false,
            mic: false,
            speaker: false,
            clicks: false,
            keystrokes: false,
            webcam: false,
            click_size: 0.3,
            click_color: 0,
            click_style: 0,
            click_animate: true,
            key_size: 0.32,
            key_position: 0,
            key_appearance: 0,
            key_blur_bg: true,
            key_filter: 0,
            webcam_size: 1,
            webcam_shape: 3,
            webcam_flip: false,
            webcam_device: -1,
            webcam_rel_x: 0.0,
            webcam_rel_y: 0.0,
            display_rec_time: false,
            hidpi: true,
            notifications: true,
            cursor: true,
            remember_selection: false,
            dim_screen: true,
            countdown: true,
            video_format: 0,
            video_max_res: 0,
            video_fps: 2,
            record_mono: false,
            open_editor: true,
            gif_fps: 50,
            gif_quality: 0.75,
            gif_size_idx: 0,
            optimize_gif: true,
            fullscreen: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordingType {
    Video,
    Gif,
}

#[derive(Debug)]
pub enum AreaCapturePathResult {
    Captured(PathBuf),
    ScrollCaptured(PathBuf),
    OcrRequested(CaptureData),
    RecordingRequested(RecordingRequest),
    RecordingConfigUpdated,
    Cancelled,
}

fn parse_area_capture_output(
    exit_code: Option<i32>,
    stdout: &str,
) -> Result<AreaCapturePathResult, SelectionError> {
    match exit_code {
        Some(0) => {
            let mode = extract_string(stdout.trim(), "mode");
            if matches!(mode.as_deref(), Some("record")) {
                let request = parse_recording_json(stdout.trim())?;
                Ok(AreaCapturePathResult::RecordingRequested(request))
            } else {
                let (path, mode) = parse_capture_screen_json_with_mode(stdout.trim())?;
                if matches!(mode.as_deref(), Some("ocr")) {
                    match load_capture_data_from_path(&path) {
                        Ok(capture) => {
                            let _ = std::fs::remove_file(&path);
                            Ok(AreaCapturePathResult::OcrRequested(capture))
                        }
                        Err(e) => {
                            let _ = std::fs::remove_file(&path);
                            Err(e)
                        }
                    }
                } else if matches!(mode.as_deref(), Some("scroll")) {
                    Ok(AreaCapturePathResult::ScrollCaptured(path))
                } else {
                    Ok(AreaCapturePathResult::Captured(path))
                }
            }
        }
        Some(code) if code == OverlayExitCode::RecordConfigUpdated as i32 => {
            let mode = extract_string(stdout.trim(), "mode");
            if matches!(mode.as_deref(), Some("record-config")) {
                let request = parse_recording_json(stdout.trim())?;
                crate::recording::persist_overlay_recording_request_state(&request).map_err(
                    |e| {
                        SelectionError::InitError(format!(
                            "Failed to persist recording overlay state: {e}"
                        ))
                    },
                )?;
                Ok(AreaCapturePathResult::RecordingConfigUpdated)
            } else {
                Err(SelectionError::InitError(format!(
                    "apexshot-capture --area-init exited with record-config code but stdout was not record-config: {stdout}"
                )))
            }
        }
        Some(1) | None => {
            eprintln!("[capture_overlay] capture_area_via_cpp: cancelled or no exit code");
            Ok(AreaCapturePathResult::Cancelled)
        }
        Some(code) if code == OverlayExitCode::ForwardedToExistingOverlay as i32 => {
            eprintln!(
                "[capture_overlay] capture_area_via_cpp: request forwarded to active overlay"
            );
            Ok(AreaCapturePathResult::Cancelled)
        }
        Some(3) => {
            eprintln!(
                "[capture_overlay] Window capture requested from area toolbar — launching portal"
            );
            capture_window_file_via_cpp().map(AreaCapturePathResult::Captured)
        }
        Some(code) => Err(SelectionError::InitError(format!(
            "apexshot-capture --area-init exited with code {code}"
        ))),
    }
}

/// Run the capture overlay and handle the Window toolbar button (exit code 3)
/// by immediately doing a window capture via the portal.
/// Returns `SelectionResult` — `Ok(None)` means "window capture was done and
/// the result should be retrieved from `capture_window_via_cpp()`".
pub fn run_capture_overlay_with_window(
    background_png: Option<&std::path::Path>,
) -> SelectionResult {
    let output = run_capture_binary(&[], background_png)?;

    match output.status.code() {
        Some(0) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_selection_json(stdout.trim())
        }
        Some(1) | None => Ok(OverlaySelection::Area(None)),
        Some(code) if code == OverlayExitCode::ForwardedToExistingOverlay as i32 => {
            Ok(OverlaySelection::Area(None))
        }
        Some(3) => Ok(OverlaySelection::Area(Some(SelectionArea {
            x: i32::MIN,
            y: i32::MIN,
            width: i32::MIN,
            height: i32::MIN,
        }))),
        Some(4) => {
            eprintln!("[capture_overlay] Window picker: switch to area mode requested");
            Ok(OverlaySelection::Area(Some(SelectionArea {
                x: i32::MIN + 1,
                y: i32::MIN,
                width: i32::MIN,
                height: i32::MIN,
            })))
        }
        Some(5) => {
            eprintln!("[capture_overlay] Window picker: switch to fullscreen mode requested");
            Ok(OverlaySelection::Area(Some(SelectionArea {
                x: i32::MIN + 2,
                y: i32::MIN,
                width: i32::MIN,
                height: i32::MIN,
            })))
        }
        Some(code) => Err(SelectionError::InitError(format!(
            "apexshot-capture exited with code {code}"
        ))),
    }
}

/// Run the native Qt capture overlay and return the selected area.
///
/// * `background_png` — optional path to a PNG screenshot to show as the
///   overlay background. If `None`, a dark semi-transparent overlay is used.
///
/// Exit code 3 means "window capture requested" — we then invoke
/// `--window-capture` to use GNOME Shell DBus.
pub fn run_capture_overlay(background_png: Option<&std::path::Path>) -> SelectionResult {
    let output = run_capture_binary(&[], background_png)?;

    match output.status.code() {
        Some(0) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_selection_json(stdout.trim())
        }
        Some(1) | None => Ok(OverlaySelection::Area(None)),
        Some(code) if code == OverlayExitCode::ForwardedToExistingOverlay as i32 => {
            Ok(OverlaySelection::Area(None))
        }
        Some(3) => {
            eprintln!(
                "[capture_overlay] Window capture requested — launching GNOME DBus window capture"
            );
            let _ = capture_window_via_cpp();
            Ok(OverlaySelection::Area(None))
        }
        Some(code) => Err(SelectionError::InitError(format!(
            "apexshot-capture exited with code {code}"
        ))),
    }
}

pub fn capture_window_file_via_cpp() -> Result<PathBuf, SelectionError> {
    if builtin_screenshot_overlay_active() {
        return Err(blocked_selection_error(
            LaunchBlockedReason::BuiltinOverlayActive,
        ));
    }

    if WaylandBackend::is_supported() {
        eprintln!(
            "[capture_overlay] capture_window_via_cpp: using Wayland ScreenCast portal backend"
        );
        let backend = WaylandBackend::new().map_err(|e| {
            SelectionError::InitError(format!("Failed to initialize Wayland backend: {e}"))
        })?;
        let capture = backend.capture_window(0).map_err(|e| {
            SelectionError::InitError(format!("Wayland window capture failed: {e}"))
        })?;
        return save_capture_to_temp_png(&capture);
    }

    // Non-Wayland path: use the native C++ window picker/capture flow.
    eprintln!("[capture_overlay] capture_window_via_cpp: launching --window-capture");
    let output = run_capture_binary(&["--window-capture"], None)?;
    let exit_code = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!(
        "[capture_overlay] capture_window_via_cpp: exit={:?} stdout={:?} stderr={:?}",
        exit_code,
        stdout.trim(),
        stderr.trim()
    );

    match exit_code {
        Some(0) => parse_capture_screen_json(stdout.trim()),
        Some(1) | None => Err(SelectionError::Cancelled),
        Some(code) if code == OverlayExitCode::ForwardedToExistingOverlay as i32 => {
            Err(SelectionError::Cancelled)
        }
        Some(code) => Err(SelectionError::InitError(format!(
            "apexshot-capture --window-capture exited with code {code}"
        ))),
    }
}

pub fn capture_window_via_cpp() -> Result<CaptureData, SelectionError> {
    let path = capture_window_file_via_cpp()?;
    let capture = load_capture_data_from_path(&path);
    let _ = std::fs::remove_file(&path);
    capture
}

pub fn capture_screen_file_via_cpp() -> Result<PathBuf, SelectionError> {
    let output = run_capture_binary(&["--capture-screen"], None)?;

    match output.status.code() {
        Some(0) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_capture_screen_json(stdout.trim())
        }
        Some(1) | None => Err(SelectionError::Cancelled),
        Some(code) if code == OverlayExitCode::ForwardedToExistingOverlay as i32 => {
            Err(SelectionError::Cancelled)
        }
        Some(code) => Err(SelectionError::InitError(format!(
            "apexshot-capture --capture-screen exited with code {code}"
        ))),
    }
}

pub fn capture_screen_via_cpp() -> Result<CaptureData, SelectionError> {
    let path = capture_screen_file_via_cpp()?;
    let capture = load_capture_data_from_path(&path);
    let _ = std::fs::remove_file(&path);
    capture
}

fn append_screenshot_timer_args(args: &mut Vec<String>, config: &crate::config::AppConfig) {
    if config.screenshot_timer_interval == 0 {
        args.push("--hide-timer".into());
    } else {
        args.push("--show-timer".into());
        args.push(format!(
            "--timer-seconds={}",
            config.screenshot_timer_interval
        ));
    }
}

fn build_area_init_args(config: &crate::config::AppConfig) -> Vec<String> {
    let mut extra_args: Vec<String> = vec!["--area-init".into()];

    if config.rec_remember_selection {
        if let (Some(x), Some(y), Some(w), Some(h)) = (
            config.last_selection_x,
            config.last_selection_y,
            config.last_selection_w,
            config.last_selection_h,
        ) {
            extra_args.push(format!("--restore-selection={x},{y},{w},{h}"));
        }
    }

    extra_args.push(format!(
        "--selection-cursor={}",
        config.screenshot_crosshair_mode
    ));
    extra_args.push(format!(
        "--show-zoom-preview={}",
        if config.screenshot_show_magnifier {
            1
        } else {
            0
        }
    ));
    extra_args.push(format!(
        "--freeze-selection-bg={}",
        if config.screenshot_freeze_screen {
            1
        } else {
            0
        }
    ));
    append_screenshot_timer_args(&mut extra_args, config);

    if config.rec_mic {
        extra_args.push("--rec-mic".into());
    }
    if config.rec_speaker {
        extra_args.push("--rec-speaker".into());
    }
    extra_args.push(if config.rec_controls {
        "--rec-controls".into()
    } else {
        "--no-rec-controls".into()
    });
    extra_args.push(if config.rec_display_time {
        "--display-rec-time".into()
    } else {
        "--no-display-rec-time".into()
    });
    extra_args.push(if config.rec_hidpi {
        "--hidpi".into()
    } else {
        "--no-hidpi".into()
    });
    extra_args.push(if config.rec_notifications {
        "--do-not-disturb".into()
    } else {
        "--no-do-not-disturb".into()
    });
    extra_args.push(if config.rec_cursor {
        "--show-cursor".into()
    } else {
        "--no-show-cursor".into()
    });
    extra_args.push(if config.rec_clicks {
        "--rec-clicks".into()
    } else {
        "--no-rec-clicks".into()
    });
    extra_args.push(if config.rec_keystrokes {
        "--rec-keystrokes".into()
    } else {
        "--no-rec-keystrokes".into()
    });
    extra_args.push(format!("--rec-click-size={:.4}", config.rec_click_size));
    extra_args.push(format!("--rec-click-color={}", config.rec_click_color));
    extra_args.push(format!("--rec-click-style={}", config.rec_click_style));
    if config.rec_click_animate {
        extra_args.push("--rec-click-animate".into());
    } else {
        extra_args.push("--no-rec-click-animate".into());
    }
    extra_args.push(format!("--rec-key-size={:.4}", config.rec_key_size));
    extra_args.push(format!("--rec-key-position={}", config.rec_key_position));
    extra_args.push(format!(
        "--rec-key-appearance={}",
        config.rec_key_appearance
    ));
    if config.rec_key_blur_bg {
        extra_args.push("--rec-key-blur-bg".into());
    } else {
        extra_args.push("--no-rec-key-blur-bg".into());
    }
    extra_args.push(format!("--rec-key-filter={}", config.rec_key_filter));
    if config.rec_webcam_enabled {
        extra_args.push("--rec-webcam".into());
    } else {
        extra_args.push("--no-rec-webcam".into());
    }
    extra_args.push(format!("--rec-webcam-size={}", config.rec_webcam_size));
    extra_args.push(format!("--rec-webcam-shape={}", config.rec_webcam_shape));
    if config.rec_webcam_flip {
        extra_args.push("--rec-webcam-flip".into());
    } else {
        extra_args.push("--no-rec-webcam-flip".into());
    }
    extra_args.push(format!("--rec-webcam-device={}", config.rec_webcam_device));
    extra_args.push(format!("--rec-webcam-rel-x={:.4}", config.rec_webcam_rel_x));
    extra_args.push(format!("--rec-webcam-rel-y={:.4}", config.rec_webcam_rel_y));
    extra_args.push(if config.rec_remember_selection {
        "--remember-selection".into()
    } else {
        "--no-remember-selection".into()
    });
    extra_args.push(if config.rec_dim_screen {
        "--dim-screen".into()
    } else {
        "--no-dim-screen".into()
    });
    extra_args.push(if config.rec_countdown {
        "--show-countdown".into()
    } else {
        "--no-show-countdown".into()
    });
    extra_args.push(format!("--video-max-res={}", config.rec_video_max_res));
    extra_args.push("--video-format=0".to_string());
    extra_args.push(format!("--video-fps={}", config.rec_video_fps));
    extra_args.push(if config.rec_video_mono {
        "--record-mono".into()
    } else {
        "--no-record-mono".into()
    });
    extra_args.push(if config.rec_video_open_editor {
        "--open-editor".into()
    } else {
        "--no-open-editor".into()
    });
    extra_args.push(format!("--gif-fps={}", config.rec_gif_fps));
    extra_args.push(format!("--gif-quality={:.4}", config.rec_gif_quality));
    extra_args.push(format!("--gif-size={}", config.rec_gif_size_idx));
    if config.rec_gif_optimize {
        extra_args.push("--gif-optimize".into());
    } else {
        extra_args.push("--no-gif-optimize".into());
    }

    extra_args
}

fn build_recording_ui_args(config: &crate::config::AppConfig) -> Vec<String> {
    let mut args = build_area_init_args(config);
    args.push("--open-recording-ui".into());
    args
}

pub fn open_recording_ui_via_cpp() -> Result<AreaCapturePathResult, SelectionError> {
    let config = crate::config::load_config();
    let extra_args = build_recording_ui_args(&config);
    let arg_refs: Vec<&str> = extra_args.iter().map(|s| s.as_str()).collect();
    let output = run_capture_binary(&arg_refs, None)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_area_capture_output(output.status.code(), stdout.trim())
}

pub fn capture_area_file_via_cpp() -> Result<AreaCapturePathResult, SelectionError> {
    if should_use_gtk_layer_shell_selector() {
        eprintln!(
            "[capture_overlay] Using ApexShot GTK layer-shell selector on wlroots compositor"
        );
        return capture_area_file_via_gtk_layer_shell_wlroots();
    }

    // Check config for remember selection
    let config = crate::config::load_config();
    let extra_args = build_area_init_args(&config);

    let arg_refs: Vec<&str> = extra_args.iter().map(|s| s.as_str()).collect();
    eprintln!(
        "[capture_overlay] capture_area_via_cpp: launching {:?}",
        arg_refs
    );
    let output = run_capture_binary(&arg_refs, None)?;
    let exit_code = output.status.code();
    eprintln!(
        "[capture_overlay] capture_area_via_cpp: --area-init exited with code {:?}",
        exit_code
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!(
        "[capture_overlay] capture_area_via_cpp: stdout = {:?}",
        stdout.trim()
    );

    parse_area_capture_output(exit_code, stdout.trim())
}

pub fn capture_area_via_cpp() -> Result<AreaCaptureResult, SelectionError> {
    if should_use_gtk_layer_shell_selector() {
        eprintln!(
            "[capture_overlay] Using ApexShot GTK layer-shell selector on wlroots compositor"
        );
        return capture_area_via_gtk_layer_shell_wlroots();
    }

    match capture_area_file_via_cpp()? {
        AreaCapturePathResult::Captured(path) => {
            let capture = load_capture_data_from_path(&path);
            let _ = std::fs::remove_file(&path);
            capture.map(AreaCaptureResult::Captured)
        }
        AreaCapturePathResult::ScrollCaptured(path) => {
            let capture = load_capture_data_from_path(&path);
            let _ = std::fs::remove_file(&path);
            capture.map(AreaCaptureResult::ScrollCaptured)
        }
        AreaCapturePathResult::OcrRequested(capture) => {
            Ok(AreaCaptureResult::OcrRequested(capture))
        }
        AreaCapturePathResult::RecordingRequested(request) => {
            Ok(AreaCaptureResult::RecordingRequested(request))
        }
        AreaCapturePathResult::RecordingConfigUpdated => Ok(AreaCaptureResult::Cancelled),
        AreaCapturePathResult::Cancelled => Ok(AreaCaptureResult::Cancelled),
    }
}

fn build_crosshair_args(config: &crate::config::AppConfig) -> Vec<String> {
    let mut args = vec!["--crosshair-capture".into()];
    append_screenshot_timer_args(&mut args, config);
    args
}

pub fn capture_crosshair_file_via_cpp() -> Result<PathBuf, SelectionError> {
    if should_use_gtk_layer_shell_selector() {
        eprintln!(
            "[capture_overlay] Using ApexShot GTK layer-shell crosshair selector on wlroots compositor"
        );
        return capture_crosshair_file_via_gtk_layer_shell_wlroots();
    }

    let config = crate::config::load_config();
    let args = build_crosshair_args(&config);
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let output = run_capture_binary(&arg_refs, None)?;

    match output.status.code() {
        Some(0) => parse_capture_screen_json(&String::from_utf8_lossy(&output.stdout)),
        Some(1) | None => Err(SelectionError::Cancelled),
        Some(code) if code == OverlayExitCode::ForwardedToExistingOverlay as i32 => {
            Err(SelectionError::Cancelled)
        }
        Some(code) => Err(SelectionError::InitError(format!(
            "apexshot-capture crosshair mode exited with code {code}"
        ))),
    }
}

pub fn capture_crosshair_via_cpp() -> Result<AreaCaptureResult, SelectionError> {
    if should_use_gtk_layer_shell_selector() {
        eprintln!(
            "[capture_overlay] Using ApexShot GTK layer-shell crosshair selector on wlroots compositor"
        );
        return capture_crosshair_via_gtk_layer_shell_wlroots();
    }

    let path = capture_crosshair_file_via_cpp()?;
    let capture = load_capture_data_from_path(&path);
    let _ = std::fs::remove_file(&path);
    capture.map(AreaCaptureResult::Captured)
}

fn save_capture_to_temp_png(capture: &CaptureData) -> Result<PathBuf, SelectionError> {
    use image::{ImageBuffer, Rgba};

    let tmp = std::env::temp_dir().join(format!(
        "apexshot_capture_{}.png",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));

    let bytes_per_pixel = capture.format.bytes_per_pixel as usize;
    let stride = capture.stride as usize;
    let width = capture.width;
    let height = capture.height;

    let is_bgr = capture.format == PixelFormat::BGR24
        || capture.format == PixelFormat::BGR32
        || capture.format == PixelFormat::BGRA32;

    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height as usize {
        let row_start = row * stride;
        let row_end = (row_start + width as usize * bytes_per_pixel).min(capture.pixels.len());
        let row_data = &capture.pixels[row_start..row_end];
        for px in row_data.chunks(bytes_per_pixel) {
            if px.len() >= 4 {
                if is_bgr {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                } else {
                    rgba.extend_from_slice(&[px[0], px[1], px[2], px[3]]);
                }
            } else if px.len() == 3 {
                if is_bgr {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                } else {
                    rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
        }
    }

    let image: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(width, height, rgba)
        .ok_or_else(|| SelectionError::InitError("Failed to build RGBA image buffer".into()))?;

    image.save(&tmp).map_err(|e| {
        SelectionError::InitError(format!(
            "Failed to save temporary window capture {}: {e}",
            tmp.display()
        ))
    })?;

    Ok(tmp)
}

fn load_capture_data_from_path(path: &Path) -> Result<CaptureData, SelectionError> {
    let image = image::open(path).map_err(|e| {
        SelectionError::InitError(format!(
            "Failed to load capture image from {}: {e}",
            path.display()
        ))
    })?;
    let rgba = image.into_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    Ok(CaptureData::new(
        rgba.into_raw(),
        width,
        height,
        PixelFormat::RGBA32,
    ))
}

fn parse_capture_screen_json(json: &str) -> Result<PathBuf, SelectionError> {
    let path = extract_string(json, "path").ok_or_else(|| {
        SelectionError::InitError(format!(
            "Failed to parse path from fullscreen capture output: '{json}'"
        ))
    })?;
    Ok(PathBuf::from(path))
}

fn parse_capture_screen_json_with_mode(
    json: &str,
) -> Result<(PathBuf, Option<String>), SelectionError> {
    let path = parse_capture_screen_json(json)?;
    let mode = extract_string(json, "mode");
    Ok((path, mode))
}

fn parse_recording_json(json: &str) -> Result<RecordingRequest, SelectionError> {
    let x = extract_int(json, "x").ok_or_else(|| SelectionError::InitError("Missing x".into()))?;
    let y = extract_int(json, "y").ok_or_else(|| SelectionError::InitError("Missing y".into()))?;
    let width = extract_int(json, "width")
        .ok_or_else(|| SelectionError::InitError("Missing width".into()))?;
    let height = extract_int(json, "height")
        .ok_or_else(|| SelectionError::InitError("Missing height".into()))?;

    let record_type_str = extract_string(json, "record_type").unwrap_or_else(|| "video".into());
    let record_type = match record_type_str.as_str() {
        "gif" => RecordingType::Gif,
        _ => RecordingType::Video,
    };

    let controls = extract_bool(json, "controls").unwrap_or(false);
    let mic = extract_bool(json, "mic").unwrap_or(false);
    let speaker = extract_bool(json, "speaker").unwrap_or(false);
    let clicks = extract_bool(json, "clicks").unwrap_or(false);
    let keystrokes = extract_bool(json, "keystrokes").unwrap_or(false);
    let webcam = extract_bool(json, "webcam").unwrap_or(false);
    let click_size = extract_float(json, "click_size").unwrap_or(0.3);
    let click_color = extract_int(json, "click_color").unwrap_or(0) as u8;
    let click_style = extract_int(json, "click_style").unwrap_or(0) as u8;
    let click_animate = extract_bool(json, "click_animate").unwrap_or(true);
    let key_size = extract_float(json, "key_size").unwrap_or(0.32);
    let key_position = extract_int(json, "key_position").unwrap_or(0) as u8;
    let key_appearance = extract_int(json, "key_appearance").unwrap_or(0) as u8;
    let key_blur_bg = extract_bool(json, "key_blur_bg").unwrap_or(true);
    let key_filter = extract_int(json, "key_filter").unwrap_or(0) as u8;
    let webcam_size = extract_int(json, "webcam_size").unwrap_or(1) as u8;
    let webcam_shape = extract_int(json, "webcam_shape").unwrap_or(3) as u8;
    let webcam_flip = extract_bool(json, "webcam_flip").unwrap_or(false);
    let webcam_device = extract_int(json, "webcam_device").unwrap_or(-1);
    let webcam_rel_x = extract_float(json, "webcam_rel_x").unwrap_or(0.0);
    let webcam_rel_y = extract_float(json, "webcam_rel_y").unwrap_or(0.0);

    // General tab settings
    let display_rec_time = extract_bool(json, "display_rec_time").unwrap_or(false);
    let hidpi = extract_bool(json, "hidpi").unwrap_or(false);
    let notifications = extract_bool(json, "notifications").unwrap_or(true);
    let cursor = extract_bool(json, "cursor").unwrap_or(true);
    let remember_selection = extract_bool(json, "remember_selection").unwrap_or(false);
    let dim_screen = extract_bool(json, "dim_screen").unwrap_or(true);
    let countdown = extract_bool(json, "countdown").unwrap_or(true);

    // Video tab settings
    let video_format = extract_int(json, "video_format").unwrap_or(0).clamp(0, 0) as u8;
    let video_max_res = extract_int(json, "video_max_res").unwrap_or(0) as u8;
    let video_fps = extract_int(json, "video_fps").unwrap_or(2) as u8; // Default matches the overlay constructor
    let record_mono = extract_bool(json, "record_mono").unwrap_or(false);
    let open_editor = extract_bool(json, "open_editor").unwrap_or(false);
    let gif_fps = extract_int(json, "gif_fps").unwrap_or(50).clamp(5, 60) as u8;
    let gif_quality = extract_float(json, "gif_quality")
        .unwrap_or(0.75)
        .clamp(0.0, 1.0);
    let gif_size_idx = extract_int(json, "gif_size_idx").unwrap_or(0).clamp(0, 3) as u8;
    let optimize_gif = extract_bool(json, "optimize_gif").unwrap_or(true);
    let fullscreen = extract_bool(json, "fullscreen").unwrap_or(false);

    Ok(RecordingRequest {
        x,
        y,
        width,
        height,
        record_type,
        controls,
        mic,
        speaker,
        clicks,
        keystrokes,
        webcam,
        click_size,
        click_color,
        click_style,
        click_animate,
        key_size,
        key_position,
        key_appearance,
        key_blur_bg,
        key_filter,
        webcam_size,
        webcam_shape,
        webcam_flip,
        webcam_device,
        webcam_rel_x,
        webcam_rel_y,
        display_rec_time,
        hidpi,
        notifications,
        cursor,
        remember_selection,
        dim_screen,
        countdown,
        video_format,
        video_max_res,
        video_fps,
        record_mono,
        open_editor,
        gif_fps,
        gif_quality,
        gif_size_idx,
        optimize_gif,
        fullscreen,
    })
}

fn extract_bool(json: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{}\":", key);
    let start = json.find(&needle)? + needle.len();
    let rest = json[start..].trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Parse `{"x":N,"y":N,"width":N,"height":N}` produced by the C++ binary.
fn parse_selection_json(json: &str) -> SelectionResult {
    let x = extract_int(json, "x");
    let y = extract_int(json, "y");
    let w = extract_int(json, "width");
    let h = extract_int(json, "height");

    match (x, y, w, h) {
        (Some(x), Some(y), Some(width), Some(height)) if width > 0 && height > 0 => {
            Ok(OverlaySelection::Area(Some(SelectionArea {
                x,
                y,
                width,
                height,
            })))
        }
        _ => Err(SelectionError::InitError(format!(
            "Failed to parse selection from apexshot-capture output: '{json}'"
        ))),
    }
}

fn extract_int(json: &str, key: &str) -> Option<i32> {
    let needle = format!("\"{}\":", key);
    let start = json.find(&needle)? + needle.len();
    let rest = json[start..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn extract_float(json: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{}\":", key);
    let start = json.find(&needle)? + needle.len();
    let rest = json[start..].trim_start();
    let end = rest
        .find(|c: char| {
            !c.is_ascii_digit() && c != '-' && c != '.' && c != 'e' && c != 'E' && c != '+'
        })
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn extract_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let start = json.find(&needle)? + needle.len();
    let mut out = String::new();
    let mut escaped = false;

    for ch in json[start..].chars() {
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => out.push(other),
            }
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == '"' {
            return Some(out);
        }

        out.push(ch);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        append_screenshot_timer_args, build_area_init_args, build_crosshair_args,
        build_recording_ui_args, classify_overlay_exit_code, execute_builtin_overlay_query,
        parse_area_capture_output, parse_capture_screen_json, parse_capture_screen_json_with_mode,
        parse_recording_json, parse_selection_json, save_capture_to_temp_png,
        should_request_screenshot_lock, tracked_overlay_id, CaptureSessionCoordinator,
        LaunchBlockedReason, OverlayExitCode, OverlaySelection, RecordingType,
    };
    use crate::{
        backend::{CaptureData, PixelFormat},
        config::AppConfig,
    };

    #[test]
    fn crosshair_capture_does_not_build_area_init_settings_args() {
        let config = AppConfig {
            screenshot_timer_interval: 0,
            ..AppConfig::default()
        };
        assert_eq!(
            build_crosshair_args(&config),
            vec!["--crosshair-capture", "--hide-timer"]
        );
    }

    #[test]
    fn screenshot_timer_args_follow_setting() {
        let mut off_args = Vec::new();
        append_screenshot_timer_args(
            &mut off_args,
            &AppConfig {
                screenshot_timer_interval: 0,
                ..AppConfig::default()
            },
        );
        assert_eq!(off_args, vec!["--hide-timer"]);

        let mut on_args = Vec::new();
        append_screenshot_timer_args(
            &mut on_args,
            &AppConfig {
                screenshot_timer_interval: 3,
                ..AppConfig::default()
            },
        );
        assert_eq!(on_args, vec!["--show-timer", "--timer-seconds=3"]);
    }

    #[test]
    fn test_parse_normal() {
        let result = parse_selection_json(r#"{"x":10,"y":20,"width":300,"height":200}"#).unwrap();
        let area = match result {
            OverlaySelection::Area(Some(area)) => area,
            other => panic!("unexpected selection: {other:?}"),
        };
        assert_eq!(area.x, 10);
        assert_eq!(area.y, 20);
        assert_eq!(area.width, 300);
        assert_eq!(area.height, 200);
    }

    #[test]
    fn test_parse_zero_size_is_error() {
        assert!(parse_selection_json(r#"{"x":0,"y":0,"width":0,"height":0}"#).is_err());
    }

    #[test]
    fn test_parse_capture_screen_path() {
        let path =
            parse_capture_screen_json(r#"{"path":"/tmp/demo.png","width":1920,"height":1080}"#)
                .unwrap();
        assert_eq!(path.to_string_lossy(), "/tmp/demo.png");
    }

    #[test]
    fn test_parse_capture_screen_path_with_mode() {
        let (path, mode) = parse_capture_screen_json_with_mode(
            r#"{"path":"/tmp/demo.png","width":1920,"height":1080,"mode":"ocr"}"#,
        )
        .unwrap();
        assert_eq!(path.to_string_lossy(), "/tmp/demo.png");
        assert_eq!(mode.as_deref(), Some("ocr"));
    }

    #[test]
    fn test_parse_selection_coords() {
        let parsed = parse_selection_json(r#"{"x":1,"y":2,"width":3,"height":4}"#).unwrap();
        let area = match parsed {
            OverlaySelection::Area(Some(area)) => area,
            other => panic!("unexpected selection: {other:?}"),
        };
        assert_eq!(area.x, 1);
        assert_eq!(area.y, 2);
        assert_eq!(area.width, 3);
        assert_eq!(area.height, 4);
    }

    #[test]
    fn parse_recording_json_reads_runtime_overlay_fields() {
        let request = parse_recording_json(
            r#"{
                "x":12,"y":34,"width":567,"height":890,
                "mode":"record","record_type":"video",
                "controls":true,"mic":true,"speaker":false,
                "clicks":true,"keystrokes":false,
                "display_rec_time":true,"hidpi":false,
                "notifications":true,"cursor":false,
                "remember_selection":true,"dim_screen":false,
                "countdown":true,
                "video_format":1,"video_max_res":2,"video_fps":1,
                "record_mono":true,"open_editor":false,
                "gif_fps":33,"gif_quality":0.8125,
                "gif_size_idx":2,"optimize_gif":false,"fullscreen":true,
                "webcam":true,"click_size":0.42,"click_color":4,"click_style":1,
                "click_animate":true,"key_size":0.33,"key_position":3,
                "key_appearance":1,"key_blur_bg":true,"key_filter":1,
                "webcam_size":2,"webcam_shape":1,"webcam_flip":true,
                "webcam_device":2,"webcam_rel_x":0.125,"webcam_rel_y":0.875
            }"#,
        )
        .unwrap();

        assert_eq!(request.x, 12);
        assert_eq!(request.y, 34);
        assert_eq!(request.width, 567);
        assert_eq!(request.height, 890);
        assert_eq!(request.record_type, RecordingType::Video);
        assert!(request.controls);
        assert!(request.mic);
        assert!(!request.speaker);
        assert!(request.clicks);
        assert!(!request.keystrokes);
        assert!(request.display_rec_time);
        assert!(!request.hidpi);
        assert!(request.notifications);
        assert!(!request.cursor);
        assert!(request.remember_selection);
        assert!(!request.dim_screen);
        assert!(request.countdown);
        assert_eq!(request.video_format, 0);
        assert_eq!(request.video_max_res, 2);
        assert_eq!(request.video_fps, 1);
        assert!(request.record_mono);
        assert!(!request.open_editor);
        assert_eq!(request.gif_fps, 33);
        assert_eq!(request.gif_quality, 0.8125);
        assert_eq!(request.gif_size_idx, 2);
        assert!(!request.optimize_gif);
        assert!(request.fullscreen);
        assert!(request.webcam);
        assert_eq!(request.click_size, 0.42);
        assert_eq!(request.click_color, 4);
        assert_eq!(request.click_style, 1);
        assert!(request.click_animate);
        assert_eq!(request.key_size, 0.33);
        assert_eq!(request.key_position, 3);
        assert_eq!(request.key_appearance, 1);
        assert!(request.key_blur_bg);
        assert_eq!(request.key_filter, 1);
        assert_eq!(request.webcam_size, 2);
        assert_eq!(request.webcam_shape, 1);
        assert!(request.webcam_flip);
        assert_eq!(request.webcam_device, 2);
        assert_eq!(request.webcam_rel_x, 0.125);
        assert_eq!(request.webcam_rel_y, 0.875);
    }

    #[test]
    fn build_area_init_args_includes_runtime_overlay_defaults() {
        let config = AppConfig {
            rec_video_format: 1,
            rec_click_size: 0.42,
            rec_click_color: 4,
            rec_click_style: 1,
            rec_click_animate: true,
            rec_key_size: 0.33,
            rec_key_position: 3,
            rec_key_appearance: 1,
            rec_key_blur_bg: true,
            rec_key_filter: 1,
            rec_webcam_enabled: true,
            rec_webcam_size: 2,
            rec_webcam_shape: 1,
            rec_webcam_flip: true,
            rec_webcam_device: 2,
            rec_webcam_rel_x: 0.125,
            rec_webcam_rel_y: 0.875,
            ..AppConfig::default()
        };

        let args = build_area_init_args(&config);

        assert!(args.contains(&"--video-format=0".to_string()));
        assert!(args.contains(&"--rec-click-size=0.4200".to_string()));
        assert!(args.contains(&"--rec-click-color=4".to_string()));
        assert!(args.contains(&"--rec-click-style=1".to_string()));
        assert!(args.contains(&"--rec-click-animate".to_string()));
        assert!(args.contains(&"--rec-key-size=0.3300".to_string()));
        assert!(args.contains(&"--rec-key-position=3".to_string()));
        assert!(args.contains(&"--rec-key-appearance=1".to_string()));
        assert!(args.contains(&"--rec-key-blur-bg".to_string()));
        assert!(args.contains(&"--rec-key-filter=1".to_string()));
        assert!(args.contains(&"--rec-webcam".to_string()));
        assert!(args.contains(&"--rec-webcam-size=2".to_string()));
        assert!(args.contains(&"--rec-webcam-shape=1".to_string()));
        assert!(args.contains(&"--rec-webcam-flip".to_string()));
        assert!(args.contains(&"--rec-webcam-device=2".to_string()));
        assert!(args.contains(&"--rec-webcam-rel-x=0.1250".to_string()));
        assert!(args.contains(&"--rec-webcam-rel-y=0.8750".to_string()));
    }

    #[test]
    fn forwarded_overlay_exit_code_is_classified_distinctly() {
        assert_eq!(
            classify_overlay_exit_code(Some(OverlayExitCode::ForwardedToExistingOverlay as i32)),
            Ok(Some("forwarded"))
        );
    }

    #[test]
    fn builtin_block_overlay_exit_code_is_classified_distinctly() {
        assert_eq!(
            classify_overlay_exit_code(Some(OverlayExitCode::BlockedByBuiltinOverlay as i32)),
            Err(LaunchBlockedReason::BuiltinOverlayActive)
        );
    }

    #[test]
    fn capture_session_coordinator_blocks_duplicate_apex_sessions() {
        let coordinator = CaptureSessionCoordinator::default();
        let _guard = coordinator
            .begin_apex_overlay_session(false)
            .expect("first session should acquire the guard");

        assert!(matches!(
            coordinator.begin_apex_overlay_session(false),
            Err(LaunchBlockedReason::ApexOverlayAlreadyActive)
        ));
    }

    #[test]
    fn capture_session_coordinator_blocks_builtin_overlay_without_latching() {
        let coordinator = CaptureSessionCoordinator::default();

        assert!(matches!(
            coordinator.begin_apex_overlay_session(true),
            Err(LaunchBlockedReason::BuiltinOverlayActive)
        ));

        assert!(
            coordinator.begin_apex_overlay_session(false).is_ok(),
            "builtin detection should not permanently wedge the coordinator"
        );
    }

    #[test]
    fn builtin_overlay_query_can_run_inside_tokio_runtime_without_panicking() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

        let result = runtime.block_on(async {
            std::panic::catch_unwind(|| execute_builtin_overlay_query(|| true))
        });

        assert!(result.expect("query should not panic"));
    }

    #[test]
    fn screenshot_lock_only_wraps_interactive_overlay_launches() {
        assert!(should_request_screenshot_lock(&[]));
        assert!(should_request_screenshot_lock(&["--area-init"]));
        assert!(should_request_screenshot_lock(&["--window-capture"]));
        assert!(should_request_screenshot_lock(&["--crosshair-capture"]));
        assert!(!should_request_screenshot_lock(&["--capture-screen"]));
    }

    #[test]
    fn tracked_overlay_id_matches_preview_helper_contract() {
        assert_eq!(
            tracked_overlay_id("session-123"),
            "capture-overlay-session-123"
        );
    }

    #[test]
    fn build_area_init_args_includes_screenshot_selection_settings() {
        let config = AppConfig {
            screenshot_freeze_screen: false,
            screenshot_crosshair_mode: "Crosshair".into(),
            screenshot_show_magnifier: true,
            ..AppConfig::default()
        };

        let args = build_area_init_args(&config);

        assert!(args.iter().any(|arg| arg == "--area-init"));
        assert!(args.iter().any(|arg| arg == "--selection-cursor=Crosshair"));
        assert!(args.iter().any(|arg| arg == "--show-zoom-preview=1"));
        assert!(args.iter().any(|arg| arg == "--freeze-selection-bg=0"));
    }

    #[test]
    fn build_recording_ui_args_adds_direct_recording_flag() {
        let args = build_recording_ui_args(&crate::config::AppConfig::default());
        assert!(args.iter().any(|arg| arg == "--area-init"));
        assert!(args.iter().any(|arg| arg == "--open-recording-ui"));
    }

    #[test]
    fn area_init_cancel_does_not_parse_record_config_payload() {
        let result = parse_area_capture_output(
            Some(OverlayExitCode::Cancelled as i32),
            r#"{"x":636,"y":177,"width":600,"height":744,"mode":"record-config","record_type":"video"}"#,
        )
        .expect("cancel should parse");

        assert!(matches!(result, super::AreaCapturePathResult::Cancelled));
    }

    #[test]
    fn explicit_record_config_exit_is_distinct_from_cancel() {
        let result = parse_area_capture_output(
            Some(OverlayExitCode::RecordConfigUpdated as i32),
            r#"{"x":636,"y":177,"width":600,"height":744,"mode":"record-config","record_type":"video","controls":true,"mic":false,"speaker":false,"clicks":true,"keystrokes":false,"webcam":false,"click_size":0.1464,"click_color":3,"click_style":0,"click_animate":true,"key_size":0.8929,"key_position":0,"key_appearance":0,"key_blur_bg":true,"key_filter":0,"webcam_size":1,"webcam_shape":1,"webcam_flip":false,"webcam_device":0,"webcam_rel_x":0.0000,"webcam_rel_y":0.0000,"display_rec_time":false,"hidpi":false,"notifications":true,"cursor":true,"remember_selection":false,"dim_screen":true,"countdown":true,"video_max_res":0,"video_fps":1,"record_mono":false,"open_editor":false,"gif_fps":60,"gif_quality":0.7500,"gif_size_idx":0,"optimize_gif":true,"fullscreen":false}"#,
        )
        .expect("record config should parse");

        assert!(matches!(
            result,
            super::AreaCapturePathResult::RecordingConfigUpdated
        ));
    }

    #[test]
    fn save_capture_to_temp_png_round_trips_rgba_capture() {
        let capture = CaptureData::new(
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
            ],
            2,
            2,
            PixelFormat::RGBA32,
        );

        let path = save_capture_to_temp_png(&capture).expect("temp png should save");
        let loaded = image::open(&path)
            .expect("temp png should load")
            .into_rgba8();
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded.width(), 2);
        assert_eq!(loaded.height(), 2);
    }
}
