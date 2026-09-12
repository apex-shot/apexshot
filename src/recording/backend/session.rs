use std::time::Duration;

/// Holds overlay meters off for the lifetime of a capture so a second
/// pulsesrc cannot xrun the recording source on loud transients.
pub(super) struct RecordingAudioExclusiveGuard {
    active: bool,
}

impl RecordingAudioExclusiveGuard {
    pub(super) fn acquire(needed: bool) -> Self {
        if !needed {
            return Self { active: false };
        }
        if crate::daemon::recording_audio_is_exclusive() {
            return Self { active: false };
        }
        crate::daemon::release_audio_monitors_for_recording();
        Self { active: true }
    }
}

impl Drop for RecordingAudioExclusiveGuard {
    fn drop(&mut self) {
        if self.active {
            crate::daemon::end_recording_audio_exclusive();
        }
    }
}

pub(super) type RecordingPortalSession =
    ashpd::desktop::Session<ashpd::desktop::screencast::Screencast>;

/// Backend that owns the lifetime of a Wayland capture stream.
pub(super) enum WaylandCaptureSession {
    /// XDG ScreenCast portal session (GNOME and generic fallback).
    Portal(#[allow(dead_code)] OwnedPortalSession),
    /// KWin `zkde_screencast_unstable_v1` (no portal dialog).
    /// Boxed so the portal variant stays small (clippy `large_enum_variant`).
    KdeNative(Box<crate::backend::kde_screencast::KdeScreencastHandle>),
}

impl std::fmt::Debug for WaylandCaptureSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Portal(_) => f.write_str("Portal(..)"),
            Self::KdeNative(handle) => f
                .debug_struct("KdeNative")
                .field("node_id", &handle.node_id())
                .finish(),
        }
    }
}

/// A portal ScreenCast session bound to a recording-private D-Bus connection.
///
/// `Session.Close` wedges the zbus proxy machinery of the connection it
/// travels on (reproduced against xdg-desktop-portal on GNOME 50: after a
/// Close, every later portal call on that connection hangs without even
/// reaching the bus). A process-global portal connection would therefore
/// break every recording after the first. Giving each recording its own
/// connection contains the damage: the Close travels on the owning
/// connection (the only sender the portal accepts), and the connection is
/// dropped together with the recording.
pub(super) struct OwnedPortalSession {
    conn: zbus::Connection,
    session: Option<RecordingPortalSession>,
}

impl OwnedPortalSession {
    pub(super) fn new(conn: zbus::Connection, session: RecordingPortalSession) -> Self {
        Self {
            conn,
            session: Some(session),
        }
    }

    pub(super) fn as_session(&self) -> &RecordingPortalSession {
        self.session.as_ref().expect("portal session")
    }
}

impl Drop for OwnedPortalSession {
    fn drop(&mut self) {
        let Some(session) = self.session.take() else {
            return;
        };
        // Keep the owning connection alive until Close finishes, then let it
        // die with this recording. A wedged connection never outlives it.
        let conn = self.conn.clone();
        crate::utils::run_off_tokio(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    eprintln!("[recording] Portal session close runtime: {err}");
                    return;
                }
            };
            match rt.block_on(async move {
                tokio::time::timeout(Duration::from_secs(2), session.close()).await
            }) {
                Ok(Ok(())) => eprintln!("[recording] Closed portal session"),
                Ok(Err(err)) => eprintln!("[recording] Portal session close: {err}"),
                Err(_) => eprintln!(
                    "[recording] Portal session close timed out; dropping the recording portal connection"
                ),
            }
            drop(conn);
        });
    }
}
