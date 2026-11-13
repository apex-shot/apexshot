//! The upload helper the C++ capture shell runs after a capture.
//!
//! The shell owns the capture and the result card but has no Cloud transport.
//! It starts this binary with `--upload <path> --progress` (an explicit upload
//! from the result card, which draws its own progress and result) or
//! `--auto-upload <path>` (the same, but only when Settings has
//! auto-upload enabled and the destination is configured). The helper finishes
//! the request and waits for its notification before exiting, because the
//! notification is drawn from this process's own hidden window.

use std::cell::Cell;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::load_config;
use crate::i18n::{t, tfmt};
use crate::utils::{clipboard, notify};

use super::upload::{self, UploadError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadCommand {
    /// Upload regardless of the auto-upload setting.
    Upload(PathBuf),
    /// Upload for the result card: progress goes to stdout as
    /// `progress <sent> <total>` lines, and the card shows the success itself.
    UploadWithProgress(PathBuf),
    /// Upload only when Settings enables auto-upload after capture.
    AutoUpload(PathBuf),
}

/// Read an upload command from the process arguments, ignoring anything else.
pub fn command_from_args(args: impl IntoIterator<Item = OsString>) -> Option<UploadCommand> {
    let args: Vec<OsString> = args.into_iter().collect();
    let report_progress = args.iter().any(|argument| argument == "--progress");
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        match argument.to_str() {
            Some("--upload") => {
                return args.next().map(PathBuf::from).map(|path| {
                    if report_progress {
                        UploadCommand::UploadWithProgress(path)
                    } else {
                        UploadCommand::Upload(path)
                    }
                })
            }
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
        UploadCommand::Upload(path) => upload_now(&path, false, false),
        UploadCommand::UploadWithProgress(path) => upload_now(&path, false, true),
        UploadCommand::AutoUpload(path) => upload_now(&path, true, false),
    }
}

fn upload_now(path: &Path, automatic: bool, report_progress: bool) -> i32 {
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

    let last_percent = Cell::new(u64::MAX);
    let report = |sent: u64, total: u64| {
        if !report_progress {
            return;
        }
        let percent = if total == 0 { 100 } else { sent * 100 / total };
        if percent == last_percent.get() {
            return;
        }
        last_percent.set(percent);
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "progress {sent} {total}");
        let _ = out.flush();
    };

    match upload::upload_file_with_progress(&config, path, &report) {
        Ok(result) => {
            eprintln!("[cloud] Upload complete: {}", result.share_url);
            let copied = clipboard::copy_text_via_windows_clipboard(&result.share_url);
            if let Err(error) = &copied {
                eprintln!("[cloud] Failed to copy the share link: {error}");
            }
            if report_progress {
                let mut out = std::io::stdout().lock();
                let _ = writeln!(out, "done");
                let _ = out.flush();
                return 0;
            }
            let body = match copied {
                Ok(()) => tfmt("Copied to clipboard\n{url}", &[("url", &result.share_url)]),
                Err(_) => result.share_url.clone(),
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

    #[test]
    fn the_progress_flag_asks_for_a_reported_upload() {
        assert_eq!(
            command_from_args(args(&["--upload", r"C:\Shots\shot.png", "--progress"])),
            Some(UploadCommand::UploadWithProgress(PathBuf::from(
                r"C:\Shots\shot.png"
            )))
        );
    }
}
