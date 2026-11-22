//! The three free Studio outputs every computer gets before activation.
//!
//! The count is stored beside the entitlement, so it shares that file's
//! private directory and never needs a licence to read. A missing or corrupt
//! file means nothing has been used; a trial never makes an output fail.
//!
//! Only a finished output consumes one: callers record an export after the
//! file or clipboard write succeeded, never when the user cancels or the
//! render fails. Licensed users never touch the counter.

use serde::Deserialize;
#[cfg(not(target_os = "windows"))]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::i18n::{t, tfmt};

/// The Studio outputs a computer may make before activation.
pub const STUDIO_FREE_EXPORTS: u32 = 3;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
std::thread_local! {
    static TEST_TRIAL_PATH_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrialState {
    used: u32,
}

/// The trial file, beside the entitlement on every platform.
fn trial_path() -> Option<PathBuf> {
    #[cfg(test)]
    {
        if let Some(path) = TEST_TRIAL_PATH_OVERRIDE.with(|path| path.borrow().clone()) {
            return Some(path);
        }
    }
    dirs::config_dir().map(|directory| directory.join("apexshot/studio/trial.json"))
}

/// Free Studio outputs left on this computer.
pub fn trial_remaining() -> u32 {
    #[cfg(test)]
    {
        // A test that wants a trial opts in with `test_trial_path_scope`; the
        // rest must never read or write the developer's real trial state.
        if TEST_TRIAL_PATH_OVERRIDE.with(|path| path.borrow().is_none()) {
            return 0;
        }
    }
    trial_path()
        .map(|path| trial_remaining_at_path(&path))
        .unwrap_or(STUDIO_FREE_EXPORTS)
}

/// Free Studio outputs left in the trial state stored at `path`.
pub fn trial_remaining_at_path(path: &Path) -> u32 {
    STUDIO_FREE_EXPORTS.saturating_sub(read_used(path).min(STUDIO_FREE_EXPORTS))
}

/// Record one finished Studio output and return how many remain.
///
/// A storage failure leaves the counter where it was; the caller's output has
/// already succeeded, so there is nothing useful to report and nothing to
/// retry.
pub fn record_trial_export() -> u32 {
    #[cfg(test)]
    {
        if TEST_TRIAL_PATH_OVERRIDE.with(|path| path.borrow().is_none()) {
            return 0;
        }
    }
    trial_path()
        .map(|path| record_trial_export_at_path(&path))
        .unwrap_or(STUDIO_FREE_EXPORTS)
}

/// Record one finished Studio output in the trial state stored at `path`.
///
/// Saturates at [`STUDIO_FREE_EXPORTS`], so a tampered or duplicated file can
/// never grant more free exports.
pub fn record_trial_export_at_path(path: &Path) -> u32 {
    let used = read_used(path).saturating_add(1).min(STUDIO_FREE_EXPORTS);
    let _ = write_state(path, used);
    STUDIO_FREE_EXPORTS.saturating_sub(used)
}

/// The label editors show near their Export action while a trial is active.
pub fn indicator_text(access: crate::studio_entitlement::StudioAccess) -> Option<String> {
    match access {
        crate::studio_entitlement::StudioAccess::Trial { remaining: 1 } => {
            Some(t("Studio · 1 free export left"))
        }
        crate::studio_entitlement::StudioAccess::Trial { remaining } if remaining > 1 => {
            let count = remaining.to_string();
            Some(tfmt(
                "Studio · {count} free exports left",
                &[("count", &count)],
            ))
        }
        _ => None,
    }
}

/// The line an editor adds after a trial export finished.
///
/// The last one tells the user why the next export will ask for activation.
pub fn finished_feedback(was_trial: bool) -> Option<String> {
    if !was_trial {
        return None;
    }
    match trial_remaining() {
        0 => Some(t(
            "That was your last free Studio export. Get Studio to keep these styles.",
        )),
        1 => Some(t("Exported with Studio. 1 free Studio export left.")),
        remaining => {
            let count = remaining.to_string();
            Some(tfmt(
                "Exported with Studio. {count} free Studio exports left.",
                &[("count", &count)],
            ))
        }
    }
}

/// Keep `label` showing the trial count for the capabilities `capabilities`
/// returns, hiding it whenever the output is free or licensed.
///
/// Editors change their Studio usage through many controls, so the label
/// follows them on a quiet timer instead of each control remembering to push
/// an update. The timer stops with the window.
pub fn attach_indicator<F>(label: &gtk4::Label, capabilities: F)
where
    F: Fn() -> crate::studio_entitlement::StudioCapabilities + 'static,
{
    use gtk4::prelude::*;

    let label = label.clone();
    let capabilities = std::rc::Rc::new(capabilities);
    let last: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let refresh = {
        let label = label.clone();
        let last = last.clone();
        std::rc::Rc::new(move || {
            let text = indicator_text(crate::studio_entitlement::studio_access(capabilities()));
            if *last.borrow() != text {
                match &text {
                    Some(text) => {
                        label.set_text(text);
                        label.set_visible(true);
                    }
                    None => label.set_visible(false),
                }
                *last.borrow_mut() = text;
            }
        })
    };
    refresh();

    let refresh = refresh.clone();
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        if label.root().is_none() {
            return gtk4::glib::ControlFlow::Break;
        }
        refresh();
        gtk4::glib::ControlFlow::Continue
    });
}

/// The used count, or zero when the file is missing or unreadable.
fn read_used(path: &Path) -> u32 {
    let Ok(text) = fs::read_to_string(path) else {
        return 0;
    };
    serde_json::from_str::<TrialState>(&text)
        .map(|state| state.used.min(STUDIO_FREE_EXPORTS))
        .unwrap_or(0)
}

fn write_state(path: &Path, used: u32) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "trial path has no parent")
    })?;
    fs::create_dir_all(parent)?;
    set_private_directory_permissions(parent)?;

    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".studio-trial-{}-{sequence}.tmp",
        std::process::id()
    ));
    let json = serde_json::json!({ "used": used }).to_string();
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        set_private_file_mode(&mut options);
        let mut file = options.open(&temporary)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        #[cfg(not(target_os = "windows"))]
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_private_file_mode(_options: &mut OpenOptions) {}

#[cfg(test)]
pub(crate) struct TestTrialGuard {
    previous_path: Option<PathBuf>,
    cleanup_directory: Option<PathBuf>,
}

#[cfg(test)]
impl Drop for TestTrialGuard {
    fn drop(&mut self) {
        TEST_TRIAL_PATH_OVERRIDE.with(|path| {
            *path.borrow_mut() = self.previous_path.take();
        });
        if let Some(directory) = self.cleanup_directory.take() {
            let _ = fs::remove_dir_all(directory);
        }
    }
}

#[cfg(test)]
fn new_test_trial_path() -> PathBuf {
    std::env::temp_dir()
        .join(format!(
            "apexshot-studio-trial-test-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
        .join("trial.json")
}

/// Point the trial at a fresh scratch file for one test.
#[cfg(test)]
pub(crate) fn test_trial_path_scope() -> TestTrialGuard {
    let path = new_test_trial_path();
    let cleanup_directory = path.parent().map(Path::to_path_buf);
    let previous_path = TEST_TRIAL_PATH_OVERRIDE.with(|current| current.replace(Some(path)));
    TestTrialGuard {
        previous_path,
        cleanup_directory,
    }
}

/// A scratch trial with every free export already used.
#[cfg(test)]
pub(crate) fn test_trial_used_up() -> TestTrialGuard {
    let guard = test_trial_path_scope();
    if let Some(path) = trial_path() {
        for _ in 0..STUDIO_FREE_EXPORTS {
            record_trial_export_at_path(&path);
        }
    }
    guard
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_path() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "apexshot-studio-trial-{}-{stamp}",
                std::process::id()
            ))
            .join("trial.json")
    }

    #[test]
    fn a_missing_or_corrupt_file_starts_at_three_free_exports() {
        let path = scratch_path();
        assert_eq!(trial_remaining_at_path(&path), STUDIO_FREE_EXPORTS);

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not-json").unwrap();
        assert_eq!(trial_remaining_at_path(&path), STUDIO_FREE_EXPORTS);

        fs::write(&path, r#"{"used": 99}"#).unwrap();
        assert_eq!(trial_remaining_at_path(&path), 0);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn three_finished_exports_use_the_trial_up_and_it_saturates() {
        let path = scratch_path();
        assert_eq!(record_trial_export_at_path(&path), 2);
        assert_eq!(record_trial_export_at_path(&path), 1);
        assert_eq!(record_trial_export_at_path(&path), 0);
        assert_eq!(record_trial_export_at_path(&path), 0);
        assert_eq!(trial_remaining_at_path(&path), 0);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_scratch_scope_points_the_public_counter_at_the_test_file() {
        let _trial = test_trial_path_scope();
        assert_eq!(trial_remaining(), STUDIO_FREE_EXPORTS);
        assert_eq!(record_trial_export(), 2);
        assert_eq!(trial_remaining(), 2);
    }

    #[test]
    fn the_finished_line_names_the_remaining_exports() {
        let _trial = test_trial_path_scope();
        assert_eq!(finished_feedback(false), None);

        assert_eq!(
            finished_feedback(true),
            Some(tfmt(
                "Exported with Studio. {count} free Studio exports left.",
                &[("count", "3")]
            ))
        );
        record_trial_export();
        record_trial_export();
        assert_eq!(
            finished_feedback(true),
            Some(t("Exported with Studio. 1 free Studio export left."))
        );
        record_trial_export();
        assert_eq!(
            finished_feedback(true),
            Some(t(
                "That was your last free Studio export. Get Studio to keep these styles."
            ))
        );
    }
}
