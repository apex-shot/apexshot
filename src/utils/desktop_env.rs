use std::{ffi::OsString, path::PathBuf};

const GIO_DESKTOP_FILE_ENV: &str = "GIO_LAUNCHED_DESKTOP_FILE";
const GIO_DESKTOP_FILE_PID_ENV: &str = "GIO_LAUNCHED_DESKTOP_FILE_PID";

pub struct ScopedPortalCaptureIdentity {
    previous_desktop_file: Option<OsString>,
    previous_pid: Option<OsString>,
    changed: bool,
}

impl Drop for ScopedPortalCaptureIdentity {
    fn drop(&mut self) {
        if !self.changed {
            return;
        }

        match &self.previous_desktop_file {
            Some(value) => std::env::set_var(GIO_DESKTOP_FILE_ENV, value),
            None => std::env::remove_var(GIO_DESKTOP_FILE_ENV),
        }
        match &self.previous_pid {
            Some(value) => std::env::set_var(GIO_DESKTOP_FILE_PID_ENV, value),
            None => std::env::remove_var(GIO_DESKTOP_FILE_PID_ENV),
        }
    }
}

pub fn scoped_portal_capture_identity() -> ScopedPortalCaptureIdentity {
    scoped_portal_capture_identity_for_path(resolve_main_app_desktop_file())
}

fn scoped_portal_capture_identity_for_path(path: Option<PathBuf>) -> ScopedPortalCaptureIdentity {
    let previous_desktop_file = std::env::var_os(GIO_DESKTOP_FILE_ENV);
    let previous_pid = std::env::var_os(GIO_DESKTOP_FILE_PID_ENV);

    let Some(path) = path else {
        return ScopedPortalCaptureIdentity {
            previous_desktop_file,
            previous_pid,
            changed: false,
        };
    };

    std::env::set_var(GIO_DESKTOP_FILE_ENV, path);
    std::env::set_var(GIO_DESKTOP_FILE_PID_ENV, std::process::id().to_string());

    ScopedPortalCaptureIdentity {
        previous_desktop_file,
        previous_pid,
        changed: true,
    }
}

fn resolve_main_app_desktop_file() -> Option<PathBuf> {
    if let Some(desktop_file) = crate::app_identity::desktop_file_for_portal() {
        return Some(desktop_file);
    }

    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return None;
    }

    crate::hotkeys::ensure_desktop_entry_pub(crate::app_identity::app_id()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_test_env<T>(f: impl FnOnce() -> T) -> T {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let old_desktop = std::env::var_os(GIO_DESKTOP_FILE_ENV);
        let old_pid = std::env::var_os(GIO_DESKTOP_FILE_PID_ENV);
        std::env::remove_var(GIO_DESKTOP_FILE_ENV);
        std::env::remove_var(GIO_DESKTOP_FILE_PID_ENV);
        let result = f();
        match old_desktop {
            Some(value) => std::env::set_var(GIO_DESKTOP_FILE_ENV, value),
            None => std::env::remove_var(GIO_DESKTOP_FILE_ENV),
        }
        match old_pid {
            Some(value) => std::env::set_var(GIO_DESKTOP_FILE_PID_ENV, value),
            None => std::env::remove_var(GIO_DESKTOP_FILE_PID_ENV),
        }
        result
    }

    #[test]
    fn scoped_portal_capture_identity_sets_main_app_desktop_file() {
        with_test_env(|| {
            let _identity = super::scoped_portal_capture_identity_for_path(Some(PathBuf::from(
                "/tmp/io.github.codegoddy.apexshot.desktop",
            )));

            assert_eq!(
                std::env::var(GIO_DESKTOP_FILE_ENV).ok().as_deref(),
                Some("/tmp/io.github.codegoddy.apexshot.desktop")
            );
            let expected_pid = std::process::id().to_string();
            assert_eq!(
                std::env::var(GIO_DESKTOP_FILE_PID_ENV).ok().as_deref(),
                Some(expected_pid.as_str())
            );
        });
    }

    #[test]
    fn scoped_portal_capture_identity_restores_previous_values_on_drop() {
        with_test_env(|| {
            std::env::set_var(GIO_DESKTOP_FILE_ENV, "/etc/xdg/autostart/apexshot.desktop");
            std::env::set_var(GIO_DESKTOP_FILE_PID_ENV, "12345");

            {
                let _identity = super::scoped_portal_capture_identity_for_path(Some(
                    PathBuf::from("/tmp/io.github.codegoddy.apexshot.desktop"),
                ));
                assert_eq!(
                    std::env::var(GIO_DESKTOP_FILE_ENV).ok().as_deref(),
                    Some("/tmp/io.github.codegoddy.apexshot.desktop")
                );
            }

            assert_eq!(
                std::env::var(GIO_DESKTOP_FILE_ENV).ok().as_deref(),
                Some("/etc/xdg/autostart/apexshot.desktop")
            );
            assert_eq!(
                std::env::var(GIO_DESKTOP_FILE_PID_ENV).ok().as_deref(),
                Some("12345")
            );
        });
    }

    #[test]
    fn scoped_portal_capture_identity_leaves_env_untouched_when_no_path_available() {
        with_test_env(|| {
            std::env::set_var(GIO_DESKTOP_FILE_ENV, "/etc/xdg/autostart/apexshot.desktop");
            std::env::set_var(GIO_DESKTOP_FILE_PID_ENV, "12345");

            let _identity = super::scoped_portal_capture_identity_for_path(None);

            assert_eq!(
                std::env::var(GIO_DESKTOP_FILE_ENV).ok().as_deref(),
                Some("/etc/xdg/autostart/apexshot.desktop")
            );
            assert_eq!(
                std::env::var(GIO_DESKTOP_FILE_PID_ENV).ok().as_deref(),
                Some("12345")
            );
        });
    }
}
