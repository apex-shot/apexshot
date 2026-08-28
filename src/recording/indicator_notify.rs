//! One-shot “recording in progress” desktop notification.
//!
//! Posted once when recording starts. Clicking the notification (default
//! action) or the **Stop** button sends `StopSave` to the active in-process
//! recording control session.
//!
//! Do not replace the banner on a timer: GNOME/Ubuntu re-shows the banner on
//! every `Notify` replace, which looks like a blinking popup. Pause/resume
//! still updates the existing id.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use futures_util::StreamExt;
use zbus::zvariant::Value;

use super::control_session::{send_active_recording_command, RecordingControlCommand};

/// Poll `active` so `hide_recording_indicator` can join without a D-Bus wake.
const SHUTDOWN_POLL: Duration = Duration::from_millis(500);
/// Never expire — notification stays until we close it or the user dismisses it.
const PERSISTENT_TIMEOUT_MS: i32 = 0;
const RECORD_ICON: &str = "media-record";

struct IndicatorState {
    active: Arc<AtomicBool>,
    notification_id: Arc<AtomicU32>,
    /// Once the user dismisses the banner, do not recreate it for this session.
    user_dismissed: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

fn indicator_slot() -> &'static Mutex<Option<IndicatorState>> {
    static SLOT: OnceLock<Mutex<Option<IndicatorState>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn app_name() -> &'static str {
    crate::app_identity::app_name()
}

fn desktop_entry() -> &'static str {
    crate::app_identity::app_id()
}

/// Start the recording indicator. Posted once per session.
///
/// Every session gets this notification: it is the one stop affordance that
/// needs no tray host and no shell extension.
pub fn show_recording_indicator() {
    let mut slot = match indicator_slot().lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    if let Some(existing) = slot.as_ref() {
        if existing.active.load(Ordering::Relaxed) {
            // Already posted for this session. Replacing the banner on GNOME
            // pops it again; leave the original in place.
            return;
        }
    }

    if let Some(mut prev) = slot.take() {
        prev.active.store(false, Ordering::Relaxed);
        if let Some(join) = prev.join.take() {
            let _ = join.join();
        }
    }

    let active = Arc::new(AtomicBool::new(true));
    let notification_id = Arc::new(AtomicU32::new(0));
    let user_dismissed = Arc::new(AtomicBool::new(false));

    let active_w = active.clone();
    let id_w = notification_id.clone();
    let dismissed_w = user_dismissed.clone();
    let join = thread::Builder::new()
        .name("apexshot-rec-indicator".into())
        .spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                rt.block_on(indicator_worker(active_w, id_w, dismissed_w));
            } else {
                eprintln!("[recording] indicator: failed to start tokio runtime");
            }
        })
        .ok();

    *slot = Some(IndicatorState {
        active,
        notification_id,
        user_dismissed,
        join,
    });
}

/// Reflect pause state on the existing indicator (does not create a new banner).
pub fn set_recording_indicator_paused(is_paused: bool) {
    let slot = match indicator_slot().lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let Some(state) = slot.as_ref() else {
        return;
    };
    if !state.active.load(Ordering::Relaxed) {
        return;
    }
    if state.user_dismissed.load(Ordering::Relaxed) {
        return;
    }
    let id = state.notification_id.load(Ordering::Relaxed);
    if id != 0 {
        let _ = post_notification_blocking(id, is_paused);
    }
}

/// Close the indicator notification and stop the worker.
pub fn hide_recording_indicator() {
    let mut slot = match indicator_slot().lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let Some(mut state) = slot.take() else {
        return;
    };
    state.active.store(false, Ordering::Relaxed);
    let id = state.notification_id.load(Ordering::Relaxed);
    if id != 0 {
        let _ = close_notification_blocking(id);
    }
    if let Some(join) = state.join.take() {
        let _ = join.join();
    }
}

async fn indicator_worker(
    active: Arc<AtomicBool>,
    notification_id: Arc<AtomicU32>,
    user_dismissed: Arc<AtomicBool>,
) {
    let conn = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[recording] indicator: session bus failed: {e}");
            return;
        }
    };

    let action_rule: zbus::MatchRule<'_> =
        match "type='signal',interface='org.freedesktop.Notifications',member='ActionInvoked'"
            .try_into()
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[recording] indicator: bad ActionInvoked match: {e}");
                return;
            }
        };

    let closed_rule: zbus::MatchRule<'_> =
        match "type='signal',interface='org.freedesktop.Notifications',member='NotificationClosed'"
            .try_into()
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[recording] indicator: bad NotificationClosed match: {e}");
                return;
            }
        };

    let mut action_stream =
        match zbus::MessageStream::for_match_rule(action_rule, &conn, None).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[recording] indicator: ActionInvoked stream failed: {e}");
                return;
            }
        };

    let mut closed_stream =
        match zbus::MessageStream::for_match_rule(closed_rule, &conn, None).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[recording] indicator: NotificationClosed stream failed: {e}");
                return;
            }
        };

    match post_notification_async(&conn, 0, false).await {
        Ok(id) => notification_id.store(id, Ordering::Relaxed),
        Err(e) => {
            eprintln!("[recording] indicator: Notify failed: {e}");
            return;
        }
    }

    while active.load(Ordering::Relaxed) {
        tokio::select! {
            _ = tokio::time::sleep(SHUTDOWN_POLL) => {
                // Periodic wake so hide_recording_indicator can join even if
                // CloseNotification does not deliver NotificationClosed.
            }
            msg = action_stream.next() => {
                let Some(Ok(msg)) = msg else {
                    break;
                };
                let Ok((id, action)) = msg.body().deserialize::<(u32, String)>() else {
                    continue;
                };
                let our_id = notification_id.load(Ordering::Relaxed);
                if id != our_id {
                    continue;
                }
                if action == "default" || action == "stop" {
                    eprintln!(
                        "[recording] indicator: action '{action}' — stopping recording"
                    );
                    let _ = send_active_recording_command(RecordingControlCommand::StopSave);
                    active.store(false, Ordering::Relaxed);
                    break;
                }
            }
            msg = closed_stream.next() => {
                let Some(Ok(msg)) = msg else {
                    continue;
                };
                // NotificationClosed(u32 id, u32 reason)
                let Ok((id, _reason)) = msg.body().deserialize::<(u32, u32)>() else {
                    continue;
                };
                let our_id = notification_id.load(Ordering::Relaxed);
                if id != our_id || our_id == 0 {
                    continue;
                }
                // User or server closed our single banner — do not re-open.
                notification_id.store(0, Ordering::Relaxed);
                user_dismissed.store(true, Ordering::Relaxed);
            }
        }
    }

    let id = notification_id.load(Ordering::Relaxed);
    if id != 0 {
        let _ = close_notification_async(&conn, id).await;
        notification_id.store(0, Ordering::Relaxed);
    }
}

fn shortcut_hint() -> String {
    let cfg = crate::config::load_config().sanitized();
    let stop = cfg.shortcut_recording_stop_save.trim();
    if stop.is_empty() {
        "Click Stop on this notification to finish".to_string()
    } else {
        format!("Click Stop on this notification or press {stop} to finish")
    }
}

fn notification_content(paused: bool) -> (String, String, &'static str) {
    let body = shortcut_hint();
    if paused {
        ("Recording paused".to_string(), body, RECORD_ICON)
    } else {
        ("Recording".to_string(), body, RECORD_ICON)
    }
}

fn build_hints<'a>() -> HashMap<&'a str, Value<'a>> {
    let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
    hints.insert("desktop-entry", Value::from(desktop_entry()));
    // Normal urgency: critical banners on GNOME stay in the way and can stack.
    hints.insert("urgency", Value::U8(1));
    hints.insert("suppress-sound", Value::Bool(true));
    hints.insert("resident", Value::Bool(true));
    hints
}

async fn post_notification_async(
    conn: &zbus::Connection,
    replaces_id: u32,
    paused: bool,
) -> Result<u32, String> {
    let (summary, body, icon) = notification_content(paused);
    let hints = build_hints();
    // default = body/icon click; stop = explicit button
    let actions: &[&str] = &["default", "Stop recording", "stop", "Stop"];

    let reply = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                app_name(),
                replaces_id,
                icon,
                summary,
                body,
                actions,
                hints,
                PERSISTENT_TIMEOUT_MS,
            ),
        )
        .await
        .map_err(|e| e.to_string())?;

    reply.body().deserialize().map_err(|e| e.to_string())
}

fn post_notification_blocking(replaces_id: u32, paused: bool) -> Result<u32, String> {
    crate::utils::run_off_tokio(move || post_notification_on_thread(replaces_id, paused))
}

fn post_notification_on_thread(replaces_id: u32, paused: bool) -> Result<u32, String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let (summary, body, icon) = notification_content(paused);
    let hints = build_hints();
    let actions: &[&str] = &["default", "Stop recording", "stop", "Stop"];

    let reply = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                app_name(),
                replaces_id,
                icon,
                summary,
                body,
                actions,
                hints,
                PERSISTENT_TIMEOUT_MS,
            ),
        )
        .map_err(|e| e.to_string())?;

    reply.body().deserialize().map_err(|e| e.to_string())
}

async fn close_notification_async(conn: &zbus::Connection, id: u32) -> Result<(), String> {
    conn.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "CloseNotification",
        &(id,),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn close_notification_blocking(id: u32) -> Result<(), String> {
    crate::utils::run_off_tokio(move || close_notification_on_thread(id))
}

fn close_notification_on_thread(id: u32) -> Result<(), String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    conn.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "CloseNotification",
        &(id,),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_content_recording_and_paused() {
        let (s, body, icon) = notification_content(false);
        let (s_paused, _, _) = notification_content(true);
        assert_eq!(s, "Recording");
        assert!(s_paused.to_lowercase().contains("paused"));
        assert!(body.to_lowercase().contains("stop"));
        assert!(!body.to_lowercase().contains("red circle"));
        assert_eq!(icon, RECORD_ICON);
    }

    #[test]
    fn shortcut_hint_does_not_mention_red_circle() {
        let body = shortcut_hint();
        assert!(!body.to_lowercase().contains("red circle"));
        assert!(body.to_lowercase().contains("stop"));
    }

    #[test]
    fn hide_without_show_is_safe() {
        hide_recording_indicator();
    }

    #[test]
    fn hide_inside_tokio_runtime_does_not_panic() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async {
            hide_recording_indicator();
        });
    }

    #[test]
    fn close_notification_blocking_inside_tokio_does_not_panic() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async {
            let _ = close_notification_blocking(1);
        });
    }
}
