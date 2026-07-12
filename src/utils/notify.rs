//! Desktop notifications via the freedesktop Notifications API.
//!
//! Prefer a direct session-bus `Notify` call (reliable on GNOME and KDE Plasma,
//! survives parent-process exit). Fall back to `notify-send` when D-Bus is
//! unavailable so Ubuntu and other desktops keep working without a session bus
//! edge case.

use std::collections::HashMap;
use std::process::Command;

use zbus::zvariant::Value;

/// Default popup lifetime in milliseconds. `-1` would mean "server default",
/// but Plasma and GNOME both honor an explicit timeout cleanly.
const DEFAULT_TIMEOUT_MS: i32 = 8_000;

/// Show a desktop notification with ApexShot branding.
///
/// Safe to call from any thread. Errors are logged; this never panics.
pub fn desktop_notification(summary: &str, body: &str) {
    if summary.is_empty() && body.is_empty() {
        return;
    }

    if let Err(dbus_err) = notify_via_dbus(summary, body) {
        if let Err(cli_err) = notify_via_notify_send(summary, body) {
            eprintln!(
                "[notify] Failed to send desktop notification via D-Bus ({dbus_err}) and notify-send ({cli_err})"
            );
        }
    }
}

fn app_name() -> &'static str {
    crate::app_identity::app_name()
}

fn app_icon() -> &'static str {
    // Prefer the short icon name used in the installed .desktop file so theme
    // lookup works even when the reverse-DNS name is not in the icon theme.
    let short_name_available = crate::app_identity::is_dev()
        || std::path::Path::new("/usr/share/icons/hicolor/scalable/apps/apexshot.svg").exists()
        || std::path::Path::new("/usr/share/pixmaps/apexshot.svg").exists();
    if short_name_available {
        "apexshot"
    } else {
        crate::app_identity::icon_name()
    }
}

/// Desktop file basenames for the `desktop-entry` hint (no `.desktop` suffix).
fn desktop_entry() -> &'static str {
    crate::app_identity::app_id()
}

fn notify_via_dbus(summary: &str, body: &str) -> Result<(), String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;

    // Hints that matter on Plasma 6 / GNOME:
    // - desktop-entry: groups the notification under ApexShot settings and
    //   ensures popups are not stuck in a disabled "other applications" bucket
    // - urgency: 1 = normal
    let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
    hints.insert("desktop-entry", Value::from(desktop_entry()));
    hints.insert("urgency", Value::U8(1));
    // Suppress sound-only suppression paths that treat missing icon poorly.
    hints.insert("suppress-sound", Value::Bool(false));

    let actions: &[&str] = &[];

    let reply = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                app_name(),
                0u32, // replaces_id
                app_icon(),
                summary,
                body,
                actions,
                hints,
                DEFAULT_TIMEOUT_MS,
            ),
        )
        .map_err(|e| e.to_string())?;

    // Ensure the server accepted the notification (returns uint32 id).
    let _id: u32 = reply.body().deserialize().map_err(|e| e.to_string())?;
    Ok(())
}

fn notify_via_notify_send(summary: &str, body: &str) -> Result<(), String> {
    let mut cmd = Command::new("notify-send");
    cmd.arg("--app-name")
        .arg(app_name())
        .arg("--icon")
        .arg(app_icon())
        .arg(format!("--hint=string:desktop-entry:{}", desktop_entry()))
        .arg(format!("--hint=byte:urgency:{}", 1u8))
        .arg(format!("--expire-time={DEFAULT_TIMEOUT_MS}"))
        .arg("--")
        .arg(summary);

    if !body.is_empty() {
        cmd.arg(body);
    }

    // Wait for completion so the notification is registered with the daemon
    // before a short-lived preview process exits (spawn-and-forget races on
    // KDE/Plasma were dropping upload-complete toasts).
    let status = cmd
        .status()
        .map_err(|e| format!("failed to run notify-send: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("notify-send exited with {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_matches_app_id() {
        assert_eq!(desktop_entry(), crate::app_identity::app_id());
        assert!(!desktop_entry().ends_with(".desktop"));
    }

    #[test]
    fn app_name_is_nonempty() {
        assert!(!app_name().is_empty());
    }

    #[test]
    fn empty_notification_is_noop() {
        // Must not panic or attempt I/O for a completely empty message.
        desktop_notification("", "");
    }

    /// Live smoke test: only runs when APEXSHOT_NOTIFY_SMOKE=1 so CI stays silent.
    #[test]
    fn smoke_send_notification_when_requested() {
        if std::env::var_os("APEXSHOT_NOTIFY_SMOKE").is_none() {
            return;
        }
        desktop_notification(
            "ApexShot notification test",
            "If you see this popup, desktop notifications work on this session.",
        );
        // Give the compositor a moment if someone is watching.
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            notify_via_dbus(
                "ApexShot D-Bus path",
                "Direct org.freedesktop.Notifications.Notify call."
            )
            .is_ok(),
            "D-Bus Notify should succeed when a notification daemon is running"
        );
    }
}
