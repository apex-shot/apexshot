//! Pointer tracking backend selection.
//!
//! The GNOME shell extension is used when it is running; X11 sessions use the
//! in-process poller instead. Callers only see this facade, the same result
//! shape, and the same start/stop lifecycle.

use crate::gnome_shell::PointerTrackResult;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tracker {
    None,
    Gnome,
    X11,
}

static ACTIVE_TRACKER: Mutex<Tracker> = Mutex::new(Tracker::None);

fn gnome_available() -> bool {
    crate::gnome_shell::should_use_pointer_track()
}

/// Whether the GNOME extension is the tracker for this session. Outside an
/// X11 session the in-process poller cannot see the compositor's pointer, so
/// the GNOME path stays the default.
fn gnome_path() -> bool {
    gnome_available() || !crate::backend::is_x11_session()
}

/// Whether this session can record pointer motion at all.
pub(super) fn should_use() -> bool {
    gnome_available() || crate::backend::is_x11_session()
}

/// Whether the in-process X11 tracker is the one in use for this session.
pub(super) fn uses_x11_tracker() -> bool {
    !gnome_path()
}

/// Whether a tracker is either running now or could be harvesting a session.
pub(super) fn service_available() -> bool {
    crate::gnome_shell::is_shell_overlay_service_available()
        || super::x11_pointer_track::is_active()
}

fn take_active() -> Tracker {
    ACTIVE_TRACKER
        .lock()
        .map(|mut guard| std::mem::replace(&mut *guard, Tracker::None))
        .unwrap_or(Tracker::None)
}

pub(super) fn start() -> anyhow::Result<()> {
    if gnome_path() {
        crate::gnome_shell::start_pointer_track()?;
        set_active(Tracker::Gnome);
        Ok(())
    } else {
        super::x11_pointer_track::start()?;
        set_active(Tracker::X11);
        Ok(())
    }
}

pub(super) fn stop() -> anyhow::Result<PointerTrackResult> {
    match take_active() {
        Tracker::Gnome => crate::gnome_shell::stop_pointer_track(),
        Tracker::X11 => super::x11_pointer_track::stop(),
        // Preserve the hook for callers that harvest without a recorded start.
        Tracker::None if gnome_path() => crate::gnome_shell::stop_pointer_track(),
        Tracker::None => Ok(PointerTrackResult {
            t0_monotonic_us: 0,
            samples: Vec::new(),
            clicks: Vec::new(),
            presses: Vec::new(),
        }),
    }
}

fn set_active(tracker: Tracker) {
    if let Ok(mut guard) = ACTIVE_TRACKER.lock() {
        *guard = tracker;
    }
}
