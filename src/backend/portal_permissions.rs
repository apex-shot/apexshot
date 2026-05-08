//! Persist XDG desktop portal permissions so the user doesn't have to
//! re-approve screenshot/screencast access after every reboot.
//!
//! On GNOME Wayland, the portal shows a "Allow…?" dialog the first time an app
//! requests a screenshot or screencast.  The portal backend stores the answer
//! in the **PermissionStore** D-Bus service (`org.freedesktop.impl.portal.PermissionStore`).
//!
//! For sandboxed (Flatpak) apps the store is populated automatically, but
//! host-installed binaries like ApexShot need to write the entry themselves.
//!
//! This module provides `ensure_portal_permissions()` which:
//!   1. Checks whether the `screenshot` and `screencast` permissions are already
//!      granted for our application ID.
//!   2. If not, grants them via `SetPermission` so the portal remembers the
//!      choice across reboots.

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

/// Grant the `screenshot` and `screencast` portal permissions for our app ID
/// via the D-Bus PermissionStore.  This is a best-effort operation — errors
/// are logged but never propagated.
///
/// Call this from:
///   - `apexshot install` (so permissions are set up at install time)
///   - Daemon startup (so a fresh session still has permissions after reboot)
pub fn ensure_portal_permissions() {
    let app_id = crate::app_identity::app_id();
    for (table, id) in [
        (SCREENSHOT_TABLE, SCREENSHOT_ID),
        (SCREENCAST_TABLE, SCREENCAST_ID),
    ] {
        let status = grant_permission(table, id);
        match status {
            PermStatus::AlreadyGranted => {
                eprintln!("[portal-perm] {table}/{id}: already granted for {app_id}");
            }
            PermStatus::Granted => {
                eprintln!("[portal-perm] {table}/{id}: granted permission for {app_id}");
            }
            PermStatus::Failed(ref reason) => {
                // Not fatal — the user will simply see the portal dialog as before.
                eprintln!("[portal-perm] {table}/{id}: could not grant permission ({reason})");
            }
        }
    }
}

enum PermStatus {
    AlreadyGranted,
    Granted,
    Failed(String),
}

fn grant_permission(table: &str, id: &str) -> PermStatus {
    let app_id = crate::app_identity::app_id();
    // We use `dbus-send` (available on every GNOME system) rather than
    // pulling in zbus synchronously, because this function is called from
    // both the sync `install` path and the async daemon path.
    //
    // Step 1: Check if permission is already present.
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
        }
        _ => {
            // Lookup failed — try to grant anyway.
        }
    }

    // Step 2: Grant the permission.
    let result = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply=literal",
            "--dest=org.freedesktop.impl.portal.PermissionStore",
            "/org/freedesktop/impl/portal/PermissionStore",
            "org.freedesktop.impl.portal.PermissionStore.SetPermission",
            &format!("string:{table}"),
            "boolean:true",
            &format!("string:{id}"),
            &format!("string:{app_id}"),
            "array:string:yes",
        ])
        .output();

    match result {
        Ok(output) if output.status.success() => PermStatus::Granted,
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
