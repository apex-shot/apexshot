#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureSessionState {
    Idle,
    ApexOverlayActive,
    BuiltinOverlayActive,
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
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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

pub fn begin_capture_session() -> Result<CaptureOverlayGuard<'static>, LaunchBlockedReason> {
    capture_session_coordinator().begin_apex_overlay_session(builtin_screenshot_overlay_active())
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
