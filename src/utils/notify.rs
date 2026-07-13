//! Desktop notifications.
//!
//! Ubuntu/GNOME keeps the pre-746a922 `notify-send` path because D-Bus `Notify`
//! can return success from background daemons without showing a banner. KDE /
//! Plasma keeps the Fedora fix: D-Bus first, then `notify-send` fallback.

use std::collections::HashMap;
use std::process::Command;

use zbus::zvariant::Value;

const DEFAULT_TIMEOUT_MS: i32 = 8_000;

/// Freedesktop notification urgency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

/// Show a desktop notification with ApexShot branding (normal urgency).
pub fn desktop_notification(summary: &str, body: &str) {
    let _ = desktop_notification_with_options(summary, body, Urgency::Normal, 0);
}

/// Important user-facing feedback (uploads, config missing, errors).
pub fn desktop_notification_important(summary: &str, body: &str) {
    let _ = desktop_notification_with_options(summary, body, Urgency::Critical, 0);
}

/// Replace a previous notification where the backend supports it.
pub fn desktop_notification_replace(replaces_id: u32, summary: &str, body: &str) -> u32 {
    desktop_notification_with_options(summary, body, Urgency::Critical, replaces_id).unwrap_or(0)
}

pub fn desktop_notification_with_options(
    summary: &str,
    body: &str,
    urgency: Urgency,
    replaces_id: u32,
) -> Option<u32> {
    if summary.is_empty() && body.is_empty() {
        return None;
    }

    if prefer_dbus_primary() {
        match notify_via_dbus(summary, body, urgency, replaces_id) {
            Ok(id) => return Some(id),
            Err(dbus_err) => match notify_via_notify_send(summary, body, urgency) {
                Ok(()) => return None,
                Err(cli_err) => {
                    eprintln!("[notify] Failed via D-Bus ({dbus_err}) and notify-send ({cli_err})");
                    return None;
                }
            },
        }
    }

    match notify_via_notify_send(summary, body, urgency) {
        Ok(()) => None,
        Err(cli_err) => match notify_via_dbus(summary, body, urgency, replaces_id) {
            Ok(id) => Some(id),
            Err(dbus_err) => {
                eprintln!("[notify] Failed via notify-send ({cli_err}) and D-Bus ({dbus_err})");
                None
            }
        },
    }
}

fn prefer_dbus_primary() -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let session = std::env::var("XDG_SESSION_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let desktop_session = std::env::var("DESKTOP_SESSION")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let combined = format!("{desktop};{session};{desktop_session}");
    combined.contains("kde") || combined.contains("plasma")
}

fn app_name() -> &'static str {
    crate::app_identity::app_name()
}

fn app_icon() -> &'static str {
    if crate::app_identity::is_dev()
        || std::path::Path::new("/usr/share/icons/hicolor/scalable/apps/apexshot.svg").exists()
        || std::path::Path::new("/usr/share/pixmaps/apexshot.svg").exists()
    {
        "apexshot"
    } else {
        crate::app_identity::icon_name()
    }
}

fn desktop_entry() -> &'static str {
    crate::app_identity::app_id()
}

fn urgency_byte(urgency: Urgency) -> u8 {
    match urgency {
        Urgency::Low => 0,
        Urgency::Normal => 1,
        Urgency::Critical => 2,
    }
}

fn urgency_flag(urgency: Urgency) -> &'static str {
    match urgency {
        Urgency::Low => "low",
        Urgency::Normal => "normal",
        Urgency::Critical => "critical",
    }
}

fn notify_via_dbus(
    summary: &str,
    body: &str,
    urgency: Urgency,
    replaces_id: u32,
) -> Result<u32, String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;

    let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
    hints.insert("desktop-entry", Value::from(desktop_entry()));
    hints.insert("urgency", Value::U8(urgency_byte(urgency)));
    hints.insert("suppress-sound", Value::Bool(false));

    let reply = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                app_name(),
                replaces_id,
                app_icon(),
                summary,
                body,
                &[] as &[&str],
                hints,
                DEFAULT_TIMEOUT_MS,
            ),
        )
        .map_err(|e| e.to_string())?;

    reply.body().deserialize().map_err(|e| e.to_string())
}

fn notify_via_notify_send(summary: &str, body: &str, urgency: Urgency) -> Result<(), String> {
    let mut cmd = Command::new("notify-send");
    cmd.arg("-a")
        .arg(app_name())
        .arg("-i")
        .arg(app_icon())
        .arg("-u")
        .arg(urgency_flag(urgency))
        .arg(summary);

    if !body.is_empty() {
        cmd.arg(body);
    }

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
    fn app_name_is_nonempty() {
        assert!(!app_name().is_empty());
    }

    #[test]
    fn desktop_entry_matches_app_id() {
        assert_eq!(desktop_entry(), crate::app_identity::app_id());
    }

    #[test]
    fn empty_notification_is_noop() {
        desktop_notification("", "");
        assert!(desktop_notification_with_options("", "", Urgency::Normal, 0).is_none());
    }

    #[test]
    fn urgency_values_match_freedesktop() {
        assert_eq!(urgency_byte(Urgency::Low), 0);
        assert_eq!(urgency_byte(Urgency::Normal), 1);
        assert_eq!(urgency_byte(Urgency::Critical), 2);
    }

    #[test]
    fn dbus_primary_detection_is_callable() {
        let _ = prefer_dbus_primary();
    }

    #[test]
    fn smoke_send_notification_when_requested() {
        if std::env::var_os("APEXSHOT_NOTIFY_SMOKE").is_none() {
            return;
        }
        assert!(
            notify_via_notify_send(
                "ApexShot notification test",
                "If you see this popup, notify-send works on this session.",
                Urgency::Critical,
            )
            .is_ok(),
            "notify-send should succeed when a notification daemon is running"
        );
    }
}
