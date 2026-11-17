//! What a native install would close, how to close it gracefully, and whether
//! a recording is running.
//!
//! The packages' maintainer scripts stop every `apexshot` process before the
//! new files go in, and the outgoing package's script runs during an upgrade.
//! The window therefore asks each open window to close on SIGTERM, waits for
//! those processes to exit, and remembers the public command that reopens each
//! one afterwards. Windows and recordings are found through `/proc`, and a
//! recording hosted by the daemon is reported over its own bus interface.

use std::time::{Duration, Instant};

use crate::i18n::{t, tfmt};

/// A window whose unsaved state an install would discard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenWork {
    ImageEditor,
    VideoEditor,
    Settings,
    History,
    Onboarding,
    /// A running command that is not one of the named windows.
    Other,
}

impl OpenWork {
    /// The name the close-work page lists for this kind.
    fn label(self) -> String {
        match self {
            Self::ImageEditor => t("Image editor"),
            Self::VideoEditor => t("Video editor"),
            Self::Settings => t("Settings"),
            Self::History => t("History"),
            Self::Onboarding => t("Setup"),
            Self::Other => t("ApexShot window"),
        }
    }
}

/// A window process an install would close, with the `apexshot` arguments
/// that reopen it afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningWindow {
    /// The process that owns the GTK window: the `*-internal` child when a
    /// launcher is still waiting on it, otherwise the window process itself.
    pub pid: i32,
    pub kind: OpenWork,
    /// The public subcommand and its file; empty when the window has nothing
    /// worth reopening.
    pub reopen: Vec<String>,
}

/// The commands that run a recording session in their own process.
///
/// The daemon is the usual host, but a machine without a running daemon
/// records in the `record` command's process, and the overlay request runs in
/// its own process too.
fn is_recording_command(command: &str) -> bool {
    matches!(
        command,
        "record" | "record-from-overlay" | "recording-countdown-internal"
    )
}

/// Classify a process's arguments after the executable path.
///
/// `None` covers the processes that are not windows: the daemon and anything
/// that hosts a recording. An empty argument list is the desktop entry's
/// launch, which opens Settings (or first-run setup) in the main window.
pub fn classify(args: &[&str]) -> Option<OpenWork> {
    let command = args.first().copied().unwrap_or("");
    if command == "daemon" || is_recording_command(command) {
        return None;
    }
    Some(match command {
        "" | "settings" | "settings-internal" => OpenWork::Settings,
        "edit" | "edit-internal" | "image-editor" => OpenWork::ImageEditor,
        "video-editor" => OpenWork::VideoEditor,
        "history" | "history-internal" => OpenWork::History,
        "onboarding" | "onboarding-internal" => OpenWork::Onboarding,
        _ => OpenWork::Other,
    })
}

/// Whether a subcommand is the `*-internal` process that owns the GTK window.
/// The public launchers spawn that child and wait on it, so the child is the
/// process a signal has to reach.
fn owns_gtk_window(command: &str) -> bool {
    command.ends_with("-internal")
}

/// The public command that reopens this window with the same file.
///
/// The launchers (`edit`, `settings`, `history`) spawn an `*-internal` child
/// and wait on it, so the public subcommand is what a fresh process runs. An
/// editor with no file has no work to bring back: `edit` rejects a missing
/// path, and neither empty editor is worth reopening on its own.
fn reopen_commands(kind: OpenWork, args: &[&str]) -> Vec<String> {
    let path = args.get(1).copied().filter(|path| !path.is_empty());
    match kind {
        OpenWork::ImageEditor => path
            .map(|path| vec!["edit".to_string(), path.to_string()])
            .unwrap_or_default(),
        OpenWork::VideoEditor => path
            .map(|path| vec!["video-editor".to_string(), path.to_string()])
            .unwrap_or_default(),
        OpenWork::Settings => vec!["settings".to_string()],
        OpenWork::History => vec!["history".to_string()],
        OpenWork::Onboarding | OpenWork::Other => Vec::new(),
    }
}

/// The key that collapses a launcher and its `*-internal` child into one
/// window, while keeping two windows that edit different files apart.
fn duplicate_key(work: OpenWork, args: &[&str]) -> String {
    match work {
        OpenWork::ImageEditor | OpenWork::VideoEditor => {
            format!("{:?}:{}", work, args.get(1).copied().unwrap_or_default())
        }
        // A generic command is only known by its whole line; two different
        // commands are two different processes.
        OpenWork::Other => format!("{:?}:{}", work, args.join("\u{0}")),
        // The remaining windows take no file argument, and the application id
        // already keeps a second copy from opening.
        OpenWork::Settings | OpenWork::History | OpenWork::Onboarding => {
            format!("{:?}", work)
        }
    }
}

/// Group the kinds of open window, in a stable order, and count each one.
fn summary_counts(work: &[OpenWork]) -> Vec<(OpenWork, usize)> {
    const ORDER: [OpenWork; 6] = [
        OpenWork::ImageEditor,
        OpenWork::VideoEditor,
        OpenWork::Settings,
        OpenWork::History,
        OpenWork::Onboarding,
        OpenWork::Other,
    ];
    ORDER
        .into_iter()
        .filter_map(|kind| {
            let count = work.iter().filter(|open| **open == kind).count();
            (count > 0).then_some((kind, count))
        })
        .collect()
}

/// One line per kind of open window, with a count when more than one is open.
pub fn summary(work: &[OpenWork]) -> Vec<String> {
    summary_counts(work)
        .into_iter()
        .map(|(kind, count)| {
            let label = kind.label();
            if count > 1 {
                tfmt(
                    "{label} ({count})",
                    &[("label", &label), ("count", &count.to_string())],
                )
            } else {
                label
            }
        })
        .collect()
}

/// Every live `apexshot` process's pid and arguments, excluding this process.
fn apexshot_processes() -> Vec<(i32, Vec<String>)> {
    let self_pid = std::process::id();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut processes = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == self_pid {
            continue;
        }
        let directory = entry.path();
        // `comm` is the kernel's process name, which the update window changes
        // away from `apexshot` so the package scripts leave it running.
        let Ok(comm) = std::fs::read_to_string(directory.join("comm")) else {
            continue;
        };
        if comm.trim_end() != "apexshot" {
            continue;
        }
        let Ok(cmdline) = std::fs::read(directory.join("cmdline")) else {
            continue;
        };
        let args: Vec<String> = cmdline
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect();
        if args.is_empty() {
            continue;
        }
        let Ok(pid) = i32::try_from(pid) else {
            continue;
        };
        processes.push((pid, args));
    }
    processes
}

/// Collapse every launcher into its GTK-owning process, one entry per window.
fn windows_from_processes(processes: Vec<(i32, Vec<String>)>) -> Vec<RunningWindow> {
    struct Group {
        key: String,
        window: RunningWindow,
        gtk_owner: bool,
    }

    let mut groups: Vec<Group> = Vec::new();
    for (pid, process) in processes {
        let args: Vec<&str> = process.iter().map(String::as_str).collect();
        let subcommand = args.get(1..).unwrap_or_default();
        let Some(kind) = classify(subcommand) else {
            continue;
        };
        let key = duplicate_key(kind, subcommand);
        let gtk_owner = owns_gtk_window(subcommand.first().copied().unwrap_or_default());
        let window = RunningWindow {
            pid,
            kind,
            reopen: reopen_commands(kind, subcommand),
        };
        match groups.iter_mut().find(|group| group.key == key) {
            // The child replaced the launcher whenever it appears, in either
            // scan order; the launcher itself never replaces the child.
            Some(group) => {
                if gtk_owner && !group.gtk_owner {
                    group.window = window;
                    group.gtk_owner = true;
                }
            }
            None => groups.push(Group {
                key,
                window,
                gtk_owner,
            }),
        }
    }
    groups.into_iter().map(|group| group.window).collect()
}

/// The windows an install would close, one per launcher/child pair, each with
/// the command that reopens it.
pub fn running_windows() -> Vec<RunningWindow> {
    windows_from_processes(apexshot_processes())
}

/// The kinds of open window, with launcher and child collapsed.
pub fn open_work() -> Vec<OpenWork> {
    running_windows()
        .into_iter()
        .map(|window| window.kind)
        .collect()
}

/// Ask a window process to save and close through its own close handlers.
///
/// A process that cannot close yet (an export is encoding) leaves the signal
/// source in place, so a later SIGTERM is handled the same way.
pub fn request_graceful_close(pid: i32) {
    // SAFETY: `kill` only reads its arguments; an unknown pid returns ESRCH,
    // which a best-effort close can ignore.
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
}

/// Whether a process is still running. A zombie has already exited; only its
/// parent has yet to reap it.
fn process_alive(pid: i32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    // The kernel wraps `comm` in parentheses, and it may contain spaces or a
    // `)` of its own; the state is the first field after the last `)`.
    match stat.rsplit_once(')') {
        Some((_, rest)) => !rest.trim_start().starts_with('Z'),
        None => true,
    }
}

/// Wait until every pid has exited or `timeout` passes, polling `/proc` every
/// 100 ms. Returns the pids that were still running.
pub fn wait_for_windows_to_close(pids: &[i32], timeout: Duration) -> Vec<i32> {
    let started = Instant::now();
    loop {
        let running: Vec<i32> = pids
            .iter()
            .copied()
            .filter(|pid| process_alive(*pid))
            .collect();
        if running.is_empty() || started.elapsed() >= timeout {
            return running;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Whether a recording is running anywhere: in this session's processes, or in
/// a daemon that reports its own session state.
pub fn recording_active() -> bool {
    let recording_process = apexshot_processes().iter().any(|(_, process)| {
        process
            .get(1)
            .is_some_and(|command| is_recording_command(command))
    });
    recording_process || daemon_reports_recording()
}

/// Ask the daemon whether a recording session is open.
///
/// An unreachable daemon means no recording it hosts; an older daemon without
/// the method reports the same way.
fn daemon_reports_recording() -> bool {
    const TIMEOUT: Duration = Duration::from_secs(1);
    let Ok(connection) = zbus::blocking::connection::Builder::session()
        .and_then(|builder| builder.method_timeout(TIMEOUT).build())
    else {
        return false;
    };
    let Ok(reply) = connection.call_method(
        Some(crate::daemon::DAEMON_BUS_NAME),
        crate::daemon::DAEMON_OBJECT_PATH,
        Some(crate::daemon::DAEMON_INTERFACE),
        "IsRecording",
        &(),
    ) else {
        return false;
    };
    reply.body().deserialize::<bool>().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_window_subcommand_maps_to_its_window_kind() {
        assert_eq!(
            classify(&["edit", "/tmp/a.png"]),
            Some(OpenWork::ImageEditor)
        );
        assert_eq!(
            classify(&["edit-internal", "/tmp/a.png"]),
            Some(OpenWork::ImageEditor)
        );
        assert_eq!(classify(&["image-editor"]), Some(OpenWork::ImageEditor));
        assert_eq!(
            classify(&["video-editor", "/tmp/a.mp4"]),
            Some(OpenWork::VideoEditor)
        );
        assert_eq!(classify(&["settings"]), Some(OpenWork::Settings));
        assert_eq!(classify(&["settings-internal"]), Some(OpenWork::Settings));
        assert_eq!(classify(&["history"]), Some(OpenWork::History));
        assert_eq!(classify(&["history-internal"]), Some(OpenWork::History));
        assert_eq!(classify(&["onboarding"]), Some(OpenWork::Onboarding));
        // The desktop entry launches without arguments.
        assert_eq!(classify(&[]), Some(OpenWork::Settings));
    }

    #[test]
    fn the_daemon_is_not_open_work() {
        assert_eq!(classify(&["daemon"]), None);
    }

    #[test]
    fn recording_processes_are_not_open_work() {
        assert_eq!(classify(&["record", "screen"]), None);
        assert_eq!(classify(&["record-from-overlay", "{}"]), None);
        assert_eq!(classify(&["recording-countdown-internal", "{}", "3"]), None);
    }

    #[test]
    fn an_unknown_subcommand_is_a_generic_window() {
        assert_eq!(classify(&["capture", "area"]), Some(OpenWork::Other));
    }

    #[test]
    fn a_launcher_and_its_internal_child_are_one_window() {
        assert_eq!(
            duplicate_key(OpenWork::ImageEditor, &["edit", "/tmp/a.png"]),
            duplicate_key(OpenWork::ImageEditor, &["edit-internal", "/tmp/a.png"])
        );
        assert_eq!(
            duplicate_key(OpenWork::Settings, &["settings"]),
            duplicate_key(OpenWork::Settings, &["settings-internal"])
        );
        // Two editors on different files are two windows.
        assert_ne!(
            duplicate_key(OpenWork::ImageEditor, &["edit", "/tmp/a.png"]),
            duplicate_key(OpenWork::ImageEditor, &["edit", "/tmp/b.png"])
        );
    }

    #[test]
    fn the_summary_counts_windows_of_the_same_kind() {
        let lines = summary(&[
            OpenWork::ImageEditor,
            OpenWork::ImageEditor,
            OpenWork::Settings,
            OpenWork::History,
            OpenWork::History,
            OpenWork::History,
        ]);
        assert_eq!(
            lines,
            vec![
                format!("{} (2)", t("Image editor")),
                t("Settings"),
                format!("{} (3)", t("History")),
            ]
        );
    }

    fn command(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    fn process(pid: i32, args: &[&str]) -> (i32, Vec<String>) {
        (pid, args.iter().map(|arg| arg.to_string()).collect())
    }

    #[test]
    fn every_kind_maps_to_the_public_command_that_reopens_it() {
        assert_eq!(
            reopen_commands(OpenWork::ImageEditor, &["edit-internal", "/tmp/a.png"]),
            command(&["edit", "/tmp/a.png"])
        );
        // `edit` without a path is an error, so an empty editor reopens to
        // nothing.
        assert_eq!(
            reopen_commands(OpenWork::ImageEditor, &["image-editor"]),
            command(&[])
        );
        assert_eq!(
            reopen_commands(OpenWork::VideoEditor, &["video-editor", "/tmp/a.mp4"]),
            command(&["video-editor", "/tmp/a.mp4"])
        );
        assert_eq!(
            reopen_commands(OpenWork::VideoEditor, &["video-editor"]),
            command(&[])
        );
        assert_eq!(
            reopen_commands(OpenWork::Settings, &["settings-internal"]),
            command(&["settings"])
        );
        assert_eq!(
            reopen_commands(OpenWork::History, &["history-internal"]),
            command(&["history"])
        );
        assert_eq!(
            reopen_commands(OpenWork::Onboarding, &["onboarding"]),
            command(&[])
        );
        assert_eq!(
            reopen_commands(OpenWork::Other, &["capture", "area"]),
            command(&[])
        );
    }

    #[test]
    fn only_the_gtk_owning_process_of_a_launcher_is_reported() {
        // The scan order is /proc order, so cover the child before its
        // launcher and after it: the child wins either way.
        let windows = windows_from_processes(vec![
            process(41, &["apexshot", "edit-internal", "/tmp/a.png"]),
            process(40, &["apexshot", "edit", "/tmp/a.png"]),
            process(42, &["apexshot", "settings"]),
            process(43, &["apexshot", "settings-internal"]),
            process(44, &["apexshot", "video-editor", "/tmp/a.mp4"]),
        ]);
        assert_eq!(
            windows,
            vec![
                RunningWindow {
                    pid: 41,
                    kind: OpenWork::ImageEditor,
                    reopen: command(&["edit", "/tmp/a.png"]),
                },
                RunningWindow {
                    pid: 43,
                    kind: OpenWork::Settings,
                    reopen: command(&["settings"]),
                },
                RunningWindow {
                    pid: 44,
                    kind: OpenWork::VideoEditor,
                    reopen: command(&["video-editor", "/tmp/a.mp4"]),
                },
            ]
        );
    }

    #[test]
    fn the_daemon_and_recording_processes_are_left_out_of_the_scan() {
        let windows = windows_from_processes(vec![
            process(50, &["apexshot", "daemon"]),
            process(51, &["apexshot", "record", "screen"]),
            process(52, &["apexshot", "capture", "area"]),
        ]);
        assert_eq!(
            windows,
            vec![RunningWindow {
                pid: 52,
                kind: OpenWork::Other,
                reopen: command(&[]),
            }]
        );
    }

    #[test]
    fn a_live_process_is_alive() {
        assert!(process_alive(std::process::id() as i32));
        // No kernel reuses pid 0 for a user process.
        assert!(!process_alive(0));
        assert!(wait_for_windows_to_close(&[], Duration::from_millis(1)).is_empty());
    }
}
