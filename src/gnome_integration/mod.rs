use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const TRACKED_WINDOW_PATH: &str = "/org/apexshot/TrackedWindow";
const TRACKED_WINDOW_INTERFACE: &str = "org.apexshot.TrackedWindow";

/// Emit a tracked-window event on the session D-Bus. Flatpak sends the app ID
/// directly to the extension; native builds retain the existing signal payload.
pub fn emit_tracked_window_opened(
    tracked_id: &str,
    pid: u32,
    title: &str,
    role: &str,
    namespace: &str,
) {
    let opened_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let tracked_id = tracked_id.to_owned();
    let title = title.to_owned();
    let role = role.to_owned();
    let namespace = namespace.to_owned();
    let app_id = crate::app_identity::app_id().to_owned();

    if crate::app_identity::portal_only() {
        std::thread::spawn(move || {
            let Ok(connection) =
                zbus::blocking::connection::Builder::session().and_then(|builder| builder.build())
            else {
                return;
            };

            let _ = connection.emit_signal(
                Some(crate::gnome_shell::shell_overlay_bus_name()),
                TRACKED_WINDOW_PATH,
                TRACKED_WINDOW_INTERFACE,
                "TrackedWindowOpened",
                &(
                    tracked_id,
                    pid,
                    title,
                    role,
                    namespace,
                    app_id,
                    opened_at_ms,
                ),
            );
        });
        return;
    }

    std::thread::spawn(move || {
        let _ = Command::new("dbus-send")
            .args([
                "--session",
                "--type=signal",
                "/org/apexshot/TrackedWindow",
                "org.apexshot.TrackedWindow.TrackedWindowOpened",
                &format!("string:{}", tracked_id),
                &format!("uint32:{}", pid),
                &format!("string:{}", title),
                &format!("string:{}", role),
                &format!("string:{}", namespace),
                &format!("uint64:{}", opened_at_ms),
            ])
            .spawn();
    });
}

/// Emit `TrackedWindowClosed(tracked_id)` on the session D-Bus.
pub fn emit_tracked_window_closed(tracked_id: &str) {
    let tracked_id = tracked_id.to_owned();

    std::thread::spawn(move || {
        if crate::app_identity::portal_only() {
            let Ok(connection) =
                zbus::blocking::connection::Builder::session().and_then(|builder| builder.build())
            else {
                return;
            };

            let _ = connection.emit_signal(
                Some(crate::gnome_shell::shell_overlay_bus_name()),
                TRACKED_WINDOW_PATH,
                TRACKED_WINDOW_INTERFACE,
                "TrackedWindowClosed",
                &(tracked_id,),
            );
            return;
        }

        let _ = Command::new("dbus-send")
            .args([
                "--session",
                "--type=signal",
                TRACKED_WINDOW_PATH,
                "org.apexshot.TrackedWindow.TrackedWindowClosed",
                &format!("string:{}", tracked_id),
            ])
            .spawn();
    });
}
