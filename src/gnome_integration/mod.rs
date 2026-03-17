use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Emit `PreviewOpened(preview_id, pid, title, namespace, opened_at_ms)` on
/// the session D-Bus.  The extension uses `preview_id` as the primary logical
/// key and `pid` as the primary Wayland matching key.
pub fn emit_preview_opened(preview_id: &str, pid: u32, title: &str, namespace: &str) {
    let opened_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let preview_id = preview_id.to_owned();
    let title = title.to_owned();
    let namespace = namespace.to_owned();

    std::thread::spawn(move || {
        let _ = Command::new("dbus-send")
            .args([
                "--session",
                "--type=signal",
                "/org/apexshot/Preview",
                "org.apexshot.Preview.PreviewOpened",
                &format!("string:{}", preview_id),
                &format!("uint32:{}", pid),
                &format!("string:{}", title),
                &format!("string:{}", namespace),
                &format!("uint64:{}", opened_at_ms),
            ])
            .spawn();
    });
}

/// Emit `PreviewClosed(preview_id)` on the session D-Bus.
pub fn emit_preview_closed(preview_id: &str) {
    let preview_id = preview_id.to_owned();

    std::thread::spawn(move || {
        let _ = Command::new("dbus-send")
            .args([
                "--session",
                "--type=signal",
                "/org/apexshot/Preview",
                "org.apexshot.Preview.PreviewClosed",
                &format!("string:{}", preview_id),
            ])
            .spawn();
    });
}
