//! Persistent “recording in progress” desktop notification.
//!
//! While recording, ApexShot keeps a system notification with a red record
//! icon that blinks (by alternating the icon / title). Clicking the
//! notification (default action) or the **Stop** button sends `StopSave` to
//! the active in-process recording control session.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use futures_util::StreamExt;
use zbus::zvariant::Value;

use super::control_session::{send_active_recording_command, RecordingControlCommand};

const BLINK_INTERVAL: Duration = Duration::from_millis(800);
/// Never expire — notification stays until we close it or the user dismisses it.
const PERSISTENT_TIMEOUT_MS: i32 = 0;
const RECORD_ICON_ON: &str = "media-record";
const RECORD_ICON_OFF: &str = "media-record-symbolic";

struct IndicatorState {
    active: Arc<AtomicBool>,
    notification_id: Arc<AtomicU32>,
    paused: Arc<AtomicBool>,
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

/// Start (or re-assert) the persistent recording indicator.
pub fn show_recording_indicator() {
    let mut slot = match indicator_slot().lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    if let Some(existing) = slot.as_ref() {
        if existing.active.load(Ordering::Relaxed) {
            existing.paused.store(false, Ordering::Relaxed);
            let id = existing.notification_id.load(Ordering::Relaxed);
            let _ = post_notification_blocking(id, true, false);
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
    let paused = Arc::new(AtomicBool::new(false));

    let active_w = active.clone();
    let id_w = notification_id.clone();
    let paused_w = paused.clone();
    let join = thread::Builder::new()
        .name("apexshot-rec-indicator".into())
        .spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                rt.block_on(indicator_worker(active_w, id_w, paused_w));
            } else {
                eprintln!("[recording] indicator: failed to start tokio runtime");
            }
        })
        .ok();

    *slot = Some(IndicatorState {
        active,
        notification_id,
        paused,
        join,
    });
}

/// Reflect pause state on the indicator (stops blinking while paused).
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
    state.paused.store(is_paused, Ordering::Relaxed);
    let id = state.notification_id.load(Ordering::Relaxed);
    let _ = post_notification_blocking(id, true, is_paused);
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
    paused: Arc<AtomicBool>,
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

    let mut action_stream =
        match zbus::MessageStream::for_match_rule(action_rule, &conn, None).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[recording] indicator: ActionInvoked stream failed: {e}");
                return;
            }
        };

    let mut blink_on = true;
    match post_notification_async(&conn, 0, blink_on, false).await {
        Ok(id) => notification_id.store(id, Ordering::Relaxed),
        Err(e) => {
            eprintln!("[recording] indicator: Notify failed: {e}");
            return;
        }
    }

    let mut ticker = tokio::time::interval(BLINK_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await;

    while active.load(Ordering::Relaxed) {
        tokio::select! {
            _ = ticker.tick() => {
                if !active.load(Ordering::Relaxed) {
                    break;
                }
                if paused.load(Ordering::Relaxed) {
                    continue;
                }
                blink_on = !blink_on;
                let id = notification_id.load(Ordering::Relaxed);
                if id == 0 {
                    // User dismissed it — re-show so the stop affordance stays available.
                    if let Ok(new_id) = post_notification_async(&conn, 0, blink_on, false).await {
                        notification_id.store(new_id, Ordering::Relaxed);
                    }
                    continue;
                }
                match post_notification_async(&conn, id, blink_on, false).await {
                    Ok(new_id) => notification_id.store(new_id, Ordering::Relaxed),
                    Err(e) => eprintln!("[recording] indicator: blink update failed: {e}"),
                }
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
        }
    }

    let id = notification_id.load(Ordering::Relaxed);
    if id != 0 {
        let _ = close_notification_async(&conn, id).await;
    }
}

fn shortcut_hint() -> String {
    let cfg = crate::config::load_config().sanitized();
    let stop = if cfg.shortcut_recording_stop_save.trim().is_empty() {
        "not set".to_string()
    } else {
        cfg.shortcut_recording_stop_save
    };
    format!("Click the red circle or Stop to finish · Shortcut: {stop}")
}

fn notification_content(blink_on: bool, paused: bool) -> (String, String, &'static str) {
    let body = shortcut_hint();
    if paused {
        ("⏸ Recording paused".to_string(), body, RECORD_ICON_ON)
    } else if blink_on {
        ("● Recording".to_string(), body, RECORD_ICON_ON)
    } else {
        ("○ Recording".to_string(), body, RECORD_ICON_OFF)
    }
}

fn build_hints<'a>() -> HashMap<&'a str, Value<'a>> {
    let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
    hints.insert("desktop-entry", Value::from(desktop_entry()));
    hints.insert("urgency", Value::U8(2)); // critical — stays visible
    hints.insert("suppress-sound", Value::Bool(true));
    hints.insert("resident", Value::Bool(true));
    hints
}

async fn post_notification_async(
    conn: &zbus::Connection,
    replaces_id: u32,
    blink_on: bool,
    paused: bool,
) -> Result<u32, String> {
    let (summary, body, icon) = notification_content(blink_on, paused);
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

fn post_notification_blocking(
    replaces_id: u32,
    blink_on: bool,
    paused: bool,
) -> Result<u32, String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let (summary, body, icon) = notification_content(blink_on, paused);
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
    fn notification_content_blinks_and_pauses() {
        let (s_on, _, icon_on) = notification_content(true, false);
        let (s_off, _, icon_off) = notification_content(false, false);
        let (s_paused, _, _) = notification_content(true, true);
        assert!(s_on.contains('●'));
        assert!(s_off.contains('○'));
        assert!(s_paused.to_lowercase().contains("paused"));
        assert_eq!(icon_on, RECORD_ICON_ON);
        assert_eq!(icon_off, RECORD_ICON_OFF);
    }

    #[test]
    fn hide_without_show_is_safe() {
        hide_recording_indicator();
    }
}
