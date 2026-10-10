#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureSessionState {
    Idle,
    ApexOverlayActive,
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
    /// Reserve the shared capture slot without probing external state.
    pub fn reserve_apex_overlay_session(
        &self,
    ) -> Result<CaptureOverlayGuard<'_>, LaunchBlockedReason> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if matches!(*state, CaptureSessionState::ApexOverlayActive) {
            return Err(LaunchBlockedReason::ApexOverlayAlreadyActive);
        }
        *state = CaptureSessionState::ApexOverlayActive;
        drop(state);
        Ok(CaptureOverlayGuard { coordinator: self })
    }

    /// Reserve the shared capture slot unless either overlay is already active.
    pub fn begin_apex_overlay_session(
        &self,
        builtin_overlay_active: bool,
    ) -> Result<CaptureOverlayGuard<'_>, LaunchBlockedReason> {
        let guard = self.reserve_apex_overlay_session()?;
        if builtin_overlay_active {
            return Err(LaunchBlockedReason::BuiltinOverlayActive);
        }
        Ok(guard)
    }
}

impl Drop for CaptureOverlayGuard<'_> {
    fn drop(&mut self) {
        let mut state = self
            .coordinator
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *state = CaptureSessionState::Idle;
    }
}

fn capture_session_coordinator() -> &'static CaptureSessionCoordinator {
    static COORDINATOR: OnceLock<CaptureSessionCoordinator> = OnceLock::new();
    COORDINATOR.get_or_init(CaptureSessionCoordinator::default)
}

const OVERLAY_FOCUS_REQUEST: &str = "focus";
const OVERLAY_CANCEL_REQUEST: &str = "cancel";

fn overlay_socket_path() -> PathBuf {
    crate::app_identity::capture_runtime_dir().join("apexshot-capture-overlay.sock")
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

/// Reserve the shared capture slot without performing external checks.
pub fn reserve_capture_session() -> Result<CaptureOverlayGuard<'static>, LaunchBlockedReason> {
    capture_session_coordinator().reserve_apex_overlay_session()
}

/// Reserve the shared slot and reject capture while the built-in UI is active.
pub fn begin_capture_session() -> Result<CaptureOverlayGuard<'static>, LaunchBlockedReason> {
    let guard = reserve_capture_session()?;
    validate_capture_session(guard)
}

/// Check the built-in screenshot UI after reserving the shared capture slot.
pub fn validate_capture_session(
    guard: CaptureOverlayGuard<'static>,
) -> Result<CaptureOverlayGuard<'static>, LaunchBlockedReason> {
    validate_capture_session_with(guard, builtin_screenshot_overlay_active)
}

fn validate_capture_session_with<'a, F>(
    guard: CaptureOverlayGuard<'a>,
    builtin_overlay_active: F,
) -> Result<CaptureOverlayGuard<'a>, LaunchBlockedReason>
where
    F: FnOnce() -> bool,
{
    if builtin_overlay_active() {
        return Err(LaunchBlockedReason::BuiltinOverlayActive);
    }
    Ok(guard)
}

impl InteractiveOverlaySessionGuard {
    fn begin(extra_args: &[&str]) -> Self {
        if !should_request_screenshot_lock(extra_args) {
            return Self {
                tracked_overlay_id: None,
            };
        }
        if overlay_socket_is_listening() {
            eprintln!("[capture_overlay] Skipping GNOME window tracking: an interactive overlay is already active.");
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

        eprintln!("[capture_overlay] Registering GNOME capture overlay window (pid={pid}).");
        crate::gnome_integration::register_capture_overlay(crate::app_identity::app_id());
        emit_tracked_window_opened(
            tracked_id,
            pid,
            "ApexShot Capture Overlay",
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
