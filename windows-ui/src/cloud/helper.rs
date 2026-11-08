//! The upload helper the C++ capture shell runs after a capture.
//!
//! The shell owns the capture and the result card but has no Cloud transport.
//! It starts this binary with `--upload <path>` (an explicit upload from the
//! result card) or `--auto-upload <path>` (the same, but only when Settings has
//! auto-upload enabled and the destination is configured). The helper finishes
//! the request and waits for its notification before exiting, because the
//! notification is drawn from this process's own hidden window.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::config::load_config;
use crate::i18n::{t, tfmt};
use crate::utils::{clipboard, notify};

use super::upload::{self, UploadError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadCommand {
    /// Upload regardless of the auto-upload setting.
    Upload(PathBuf),
    /// Upload only when Settings enables auto-upload after capture.
    AutoUpload(PathBuf),
}

/// Read an upload command from the process arguments, ignoring anything else.
pub fn command_from_args(args: impl IntoIterator<Item = OsString>) -> Option<UploadCommand> {
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        match argument.to_str() {
            Some("--upload") => return args.next().map(PathBuf::from).map(UploadCommand::Upload),
            Some("--auto-upload") => {
                return args
                    .next()
                    .map(PathBuf::from)
                    .map(UploadCommand::AutoUpload)
            }
            _ => {}
        }
    }
    None
}

/// Run one upload; the exit code is 0 when nothing needed to happen or the
/// upload completed, and 1 when a requested upload failed.
pub fn run(command: UploadCommand) -> i32 {
    match command {
        UploadCommand::Upload(path) => upload_now(&path, false),
        UploadCommand::AutoUpload(path) => upload_now(&path, true),
    }
}

fn upload_now(path: &Path, automatic: bool) -> i32 {
    let config = load_config().sanitized();
    if automatic && !upload::should_auto_upload_after_capture(&config) {
        return 0;
    }
    if !upload::is_configured(&config) {
        if automatic {
            return 0;
        }
        let (title, body) = upload::not_configured_notification(&config);
        notify::desktop_notification_and_wait(&title, &body, true);
        return 1;
    }

    match upload::upload_file(&config, path) {
        Ok(result) => {
            eprintln!("[cloud] Upload complete: {}", result.share_url);
            let body = match clipboard::copy_text_via_windows_clipboard(&result.share_url) {
                Ok(()) => tfmt("Copied to clipboard\n{url}", &[("url", &result.share_url)]),
                Err(error) => {
                    eprintln!("[cloud] Failed to copy the share link: {error}");
                    result.share_url.clone()
                }
            };
            notify::desktop_notification_and_wait(&t("Upload complete"), &body, false);
            0
        }
        Err(error) => {
            eprintln!("[cloud] Upload failed: {error}");
            notify::desktop_notification_and_wait(&t("Upload failed"), &describe(&error), true);
            1
        }
    }
}

/// The user-facing failure text, matching what the in-process upload paths show.
fn describe(error: &UploadError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_an_explicit_upload_and_its_path() {
        assert_eq!(
            command_from_args(args(&["--upload", r"C:\Shots\shot.png"])),
            Some(UploadCommand::Upload(PathBuf::from(r"C:\Shots\shot.png")))
        );
    }

    #[test]
    fn parses_an_automatic_upload_and_its_path() {
        assert_eq!(
            command_from_args(args(&[
                "--verbose",
                "--auto-upload",
                r"C:\Videos\recording.mp4",
            ])),
            Some(UploadCommand::AutoUpload(PathBuf::from(
                r"C:\Videos\recording.mp4"
            )))
        );
    }

    #[test]
    fn an_upload_flag_without_a_path_is_not_a_command() {
        assert_eq!(command_from_args(args(&["--upload"])), None);
        assert_eq!(command_from_args(args(&["--auto-upload"])), None);
    }

    #[test]
    fn other_arguments_do_not_start_an_upload() {
        assert_eq!(command_from_args(args(&["--onboarding"])), None);
    }
}
