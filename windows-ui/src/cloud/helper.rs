//! The upload helper the C++ capture shell runs after a capture.
//!
//! The shell owns the capture and the result card but has no Cloud transport.
//! It starts this binary with `--upload <path> --progress` (an explicit upload
//! from the result card, which draws its own progress and result) or
//! `--auto-upload <path>` (the same, but only when Settings has
//! auto-upload enabled and the destination is configured). The helper finishes
//! the request and waits for its notification before exiting, because the
//! notification is drawn from this process's own hidden window.
//!
//! `--cloud-nudge <path>` is the same pattern for the Quick Access Cloud
//! nudge: the shell counts the show itself, then starts this binary detached
//! to draw the card, and the helper records only the card's click, dismissal
//! and the shell's dismissal counter.

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
    /// Show the Quick Access Cloud nudge card for `path`.
    CloudNudge(PathBuf),
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
            Some("--cloud-nudge") => {
                return args
                    .next()
                    .map(PathBuf::from)
                    .map(UploadCommand::CloudNudge)
            }
            _ => {}
        }
    }
    None
}

/// Read the standalone-card arguments the notification helper starts.
///
/// The tray shell only knows how to start this executable, so a limit card is
/// another argument mode of the same binary, mirroring Linux's `cloud-limit`.
pub fn cloud_limit_from_args(
    args: impl IntoIterator<Item = OsString>,
) -> Option<(crate::cloud::upload::CloudLimit, bool)> {
    let args: Vec<String> = args
        .into_iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    let position = args
        .iter()
        .position(|argument| argument == "--cloud-limit")?;
    crate::cloud::upgrade::parse_cli_limit(&args[position + 1..]).ok()
}

/// Run one helper command; the exit code is 0 when nothing needed to happen
/// or the work completed, and 1 when a requested upload failed.
pub fn run(command: UploadCommand) -> i32 {
    match command {
        UploadCommand::Upload(path) => upload_now(&path, false, false),
        UploadCommand::UploadWithProgress(path) => upload_now(&path, false, true),
        UploadCommand::AutoUpload(path) => upload_now(&path, true, false),
        UploadCommand::CloudNudge(path) => crate::cloud::sign_in::run_standalone_cloud_nudge(
            path,
            crate::usage_telemetry::persist_cloud_nudge_dismissal,
        ),
    }
}

fn upload_now(path: &Path, automatic: bool, report_progress: bool) -> i32 {
    let mut config = load_config().sanitized();
    if automatic && !upload::should_auto_upload_after_capture(&config) {
        return 0;
    }
    // The capture shell starts the explicit upload detached while signed out,
    // so this process owns the sign-in and the upload that follows it.
    if !automatic && crate::cloud::sign_in::needs_apexshot_sign_in(&config) {
        if !crate::cloud::sign_in::run_standalone_for_upload() {
            return 1;
        }
        config = load_config().sanitized();
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
        let percent = (sent * 100).checked_div(total).unwrap_or(100);
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
                // The result card reads this line and takes over immediately;
                // the account read for the heads-up must not delay it.
                let mut out = std::io::stdout().lock();
                let _ = writeln!(out, "done");
                let _ = out.flush();
                if let Some(heads_up) = crate::cloud::limit_prompt::usage_heads_up(&config) {
                    for limit in &heads_up.cards {
                        crate::cloud::limit_prompt::spawn_card(limit, true);
                    }
                }
                return 0;
            }
            let heads_up = crate::cloud::limit_prompt::usage_heads_up(&config);
            let mut body = match copied {
                Ok(()) => tfmt("Copied to clipboard\n{url}", &[("url", &result.share_url)]),
                Err(_) => result.share_url.clone(),
            };
            if let Some(heads_up) = &heads_up {
                body = heads_up.append_to(&body);
            }
            notify::desktop_notification_and_wait(&t("Upload complete"), &body, false);
            if let Some(heads_up) = &heads_up {
                for limit in &heads_up.cards {
                    crate::cloud::limit_prompt::spawn_card(limit, true);
                }
            }
            0
        }
        Err(UploadError::UpgradeRequired(limit)) => {
            eprintln!("[cloud] Upload failed: {limit}");
            let opened = crate::cloud::limit_prompt::allow_upgrade_prompt(limit.kind)
                && crate::cloud::limit_prompt::spawn_card(&limit, false);
            // The result card draws its own failure state; the notification is
            // the fallback when the limit card was already shown recently or
            // could not open at all.
            if !report_progress || !opened {
                notify::desktop_notification_and_wait(
                    &t("Not uploaded"),
                    &crate::cloud::upgrade::headline(&limit),
                    true,
                );
            }
            1
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
    fn parses_a_cloud_nudge_and_its_path() {
        assert_eq!(
            command_from_args(args(&["--cloud-nudge", r"C:\Shots\shot.png"])),
            Some(UploadCommand::CloudNudge(PathBuf::from(
                r"C:\Shots\shot.png"
            )))
        );
    }

    #[test]
    fn an_upload_flag_without_a_path_is_not_a_command() {
        assert_eq!(command_from_args(args(&["--upload"])), None);
        assert_eq!(command_from_args(args(&["--auto-upload"])), None);
        assert_eq!(command_from_args(args(&["--cloud-nudge"])), None);
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

    #[test]
    fn the_cloud_limit_arguments_build_a_storage_card() {
        let (limit, almost) = cloud_limit_from_args(args(&[
            "--cloud-limit",
            "storage",
            "--used",
            "2147483648",
            "--max",
            "2147483648",
            "--almost",
        ]))
        .expect("a storage limit");

        assert!(almost);
        assert_eq!(limit.kind, crate::cloud::upload::CloudLimitKind::Storage);
        assert_eq!(limit.used, Some(2 * 1024 * 1024 * 1024));
        assert_eq!(limit.max, Some(2 * 1024 * 1024 * 1024));
    }

    #[test]
    fn an_upload_command_does_not_also_read_as_a_cloud_limit() {
        assert_eq!(
            cloud_limit_from_args(args(&["--upload", r"C:\Shots\shot.png"])),
            None
        );
        assert_eq!(
            cloud_limit_from_args(args(&["--cloud-nudge", r"C:\Shots\shot.png"])),
            None
        );
    }
}
