use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Emit `TrackedWindowOpened(tracked_id, pid, title, role, namespace, opened_at_ms)` on
/// the session D-Bus. The extension uses `tracked_id` as the logical key and `pid`
/// as the primary Wayland matching key.
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
        let _ = Command::new("dbus-send")
            .args([
                "--session",
                "--type=signal",
                "/org/apexshot/TrackedWindow",
                "org.apexshot.TrackedWindow.TrackedWindowClosed",
                &format!("string:{}", tracked_id),
            ])
            .spawn();
    });
}
