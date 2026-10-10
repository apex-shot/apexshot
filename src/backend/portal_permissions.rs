//! Read-only diagnostics for desktop-managed portal permissions.

/// Permission table used by the Screenshot portal backend (xdg-desktop-portal-gnome).
const SCREENSHOT_TABLE: &str = "screenshot";
/// Resource ID inside the screenshot table.
const SCREENSHOT_ID: &str = "screenshot";

/// Permission table used by the ScreenCast portal backend.
const SCREENCAST_TABLE: &str = "screencast";
/// Resource ID inside the screencast table.
const SCREENCAST_ID: &str = "screencast";

/// The permission value that means "allowed".
const PERM_YES: &str = "yes";

pub fn report_portal_permissions() {
    // Flatpak / portal-only: never touch PermissionStore directly.
    if crate::app_identity::portal_only() {
        return;
    }

    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        eprintln!("[portal-perm] skipped: no user session bus (run from the desktop session, not sudo/root)");
        return;
    }

    let app_id = crate::app_identity::app_id();
    for (table, id) in [
        (SCREENSHOT_TABLE, SCREENSHOT_ID),
        (SCREENCAST_TABLE, SCREENCAST_ID),
    ] {
        let status = permission_status(table, id);
        match status {
            PermStatus::AlreadyGranted => {
                eprintln!("[portal-perm] {table}/{id}: already granted for {app_id}");
            }
            PermStatus::ApprovalRequired => {
                eprintln!("[portal-perm] {table}/{id}: desktop approval required for {app_id}");
            }
            PermStatus::Failed(ref reason) => {
                eprintln!("[portal-perm] {table}/{id}: could not inspect permission ({reason})");
            }
        }
    }
}

enum PermStatus {
    AlreadyGranted,
    ApprovalRequired,
    Failed(String),
}

fn permission_status(table: &str, id: &str) -> PermStatus {
    let app_id = crate::app_identity::app_id();
    let check = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply=literal",
            "--dest=org.freedesktop.impl.portal.PermissionStore",
            "/org/freedesktop/impl/portal/PermissionStore",
            "org.freedesktop.impl.portal.PermissionStore.Lookup",
            &format!("string:{table}"),
            &format!("string:{id}"),
        ])
        .output();

    match check {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains(app_id) && stdout.contains(PERM_YES) {
                return PermStatus::AlreadyGranted;
            }
            PermStatus::ApprovalRequired
        }
        Ok(output) => PermStatus::Failed(format!(
            "dbus-send exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(e) => PermStatus::Failed(format!("dbus-send failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_sane() {
        assert!(!crate::app_identity::app_id().is_empty());
        assert!(!SCREENSHOT_TABLE.is_empty());
        assert!(!SCREENSHOT_ID.is_empty());
        assert!(!SCREENCAST_TABLE.is_empty());
        assert!(!SCREENCAST_ID.is_empty());
        assert_eq!(PERM_YES, "yes");
    }
}
