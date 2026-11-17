//! The windows an update closes, and the commands that bring them back.
//!
//! Each window process keeps one small record under `%LOCALAPPDATA%` while its
//! window is up. Before starting the installer, the update window reads those
//! records, asks each process to close through its own close handler (so the
//! editors persist their sessions), waits for the processes to exit, and
//! reopens the same documents once the installer finishes. A record whose
//! process is gone is pruned the next time the list is read, so a crash leaves
//! nothing behind and the update window does not depend on an IPC service.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::i18n::{t, tfmt};

/// Which surface a recorded process owns, for the list the update window
/// shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    ImageEditor,
    VideoEditor,
    Settings,
    History,
}

impl WindowKind {
    fn rank(self) -> u8 {
        match self {
            Self::ImageEditor => 0,
            Self::VideoEditor => 1,
            Self::Settings => 2,
            Self::History => 3,
        }
    }

    /// The name the close-work page lists for this kind.
    pub fn label(self) -> String {
        match self {
            Self::ImageEditor => t("Image editor"),
            Self::VideoEditor => t("Video editor"),
            Self::Settings => t("Settings"),
            Self::History => t("History"),
        }
    }
}

/// One open window, as the updater sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenWindow {
    pub pid: u32,
    pub kind: WindowKind,
    /// The sibling executable and the arguments that reopen this window.
    pub reopen: Vec<String>,
    /// Set while the window is exporting, so an install waits instead of
    /// closing the window and losing the encode. Old records omit it.
    #[serde(default)]
    pub busy: bool,
}

const IMAGE_EDITOR_EXE: &str = "apexshot-image-editor.exe";
const VIDEO_EDITOR_EXE: &str = "apexshot-video-editor.exe";
const SETTINGS_EXE: &str = "apexshot-settings.exe";
const HISTORY_EXE: &str = "apexshot-history.exe";

fn record_directory() -> Option<PathBuf> {
    let mut directory = dirs::data_local_dir()?;
    directory.push("apexshot");
    directory.push("open-windows");
    Some(directory)
}

fn record_path() -> Option<PathBuf> {
    record_directory().map(|directory| directory.join(format!("{}.json", std::process::id())))
}

fn write_record(kind: WindowKind, reopen: Vec<String>) {
    let Some(path) = record_path() else {
        return;
    };
    let record = OpenWindow {
        pid: std::process::id(),
        kind,
        reopen,
        busy: false,
    };
    let _ = write_record_atomically(&path, &record);
}

/// Write the record to a temporary file beside it and rename it into place, so
/// the update window never reads a half-written record.
fn write_record_atomically(path: &Path, record: &OpenWindow) -> Option<()> {
    let parent = path.parent()?;
    fs::create_dir_all(parent).ok()?;
    let raw = serde_json::to_string(record).ok()?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, raw).ok()?;
    if fs::rename(&temporary, path).is_err() {
        let _ = fs::remove_file(&temporary);
        return None;
    }
    Some(())
}

/// Mark this process's window busy while an export runs. The update window
/// reads the flag before it closes anything, so an install waits for the
/// export instead of ending it.
pub fn set_busy(busy: bool) {
    let Some(path) = record_path() else {
        return;
    };
    let Some(mut record) = fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<OpenWindow>(&raw).ok())
    else {
        return;
    };
    if record.busy == busy {
        return;
    }
    record.busy = busy;
    let _ = write_record_atomically(&path, &record);
}

/// Record an image-editor window and the file it has open, if any.
pub fn register_image_editor(path: Option<&Path>) {
    write_record(
        WindowKind::ImageEditor,
        reopen_command(IMAGE_EDITOR_EXE, path),
    );
}

/// Record a video-editor window and the recording it has open, if any.
pub fn register_video_editor(path: Option<&Path>) {
    write_record(
        WindowKind::VideoEditor,
        reopen_command(VIDEO_EDITOR_EXE, path),
    );
}

/// Record the Settings window.
pub fn register_settings() {
    write_record(WindowKind::Settings, vec![SETTINGS_EXE.to_string()]);
}

/// Record the History window.
pub fn register_history() {
    write_record(WindowKind::History, vec![HISTORY_EXE.to_string()]);
}

/// Drop this process's record when its window closes.
pub fn unregister() {
    if let Some(path) = record_path() {
        let _ = fs::remove_file(path);
    }
}

fn reopen_command(executable: &str, path: Option<&Path>) -> Vec<String> {
    let mut command = vec![executable.to_string()];
    if let Some(path) = path {
        command.push(path.to_string_lossy().into_owned());
    }
    command
}

/// Every recorded window whose process is still running, one per process,
/// sorted by kind.
pub fn list() -> Vec<OpenWindow> {
    let Some(directory) = record_directory() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&directory) else {
        return Vec::new();
    };
    let mut open = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // The atomic writer's temporary file is not a record.
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let record = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<OpenWindow>(&raw).ok());
        let Some(record) = record else {
            let _ = fs::remove_file(&path);
            continue;
        };
        if record.pid == std::process::id() || !process_alive(record.pid) {
            let _ = fs::remove_file(&path);
            continue;
        }
        open.push(record);
    }
    open.sort_by_key(|window| (window.kind.rank(), window.pid));
    open
}

/// Whether any of these windows is exporting, so an install has to wait
/// rather than close it and lose the encode.
pub fn any_busy(windows: &[OpenWindow]) -> bool {
    windows.iter().any(|window| window.busy)
}

/// One line per kind of open window, with a count when more than one is open.
pub fn summary(windows: &[OpenWindow]) -> Vec<String> {
    let mut counts: Vec<(WindowKind, usize)> = Vec::new();
    for window in windows {
        match counts.iter_mut().find(|(kind, _)| *kind == window.kind) {
            Some((_, count)) => *count += 1,
            None => counts.push((window.kind, 1)),
        }
    }
    counts
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

/// Ask every window to close through its own close handler.
pub fn request_close(windows: &[OpenWindow]) {
    for window in windows {
        close_process_windows(window.pid);
    }
}

/// Wait until every pid has exited or `timeout` passes, polling every 100 ms.
/// Returns the pids that were still running.
pub fn wait_for_close(pids: &[u32], timeout: Duration) -> Vec<u32> {
    let started = Instant::now();
    loop {
        let running: Vec<u32> = pids
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

/// Start each remembered window again, from the executable beside this one.
pub fn reopen(commands: &[Vec<String>]) {
    for command in commands {
        let Some(name) = command.first() else {
            continue;
        };
        let executable = sibling_executable(name);
        let mut process = std::process::Command::new(&executable);
        process
            .args(&command[1..])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            process.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        if let Err(error) = process.spawn() {
            eprintln!(
                "[update] Could not reopen {}: {error}",
                executable.display()
            );
        }
    }
}

/// Wait for the installer to finish, then reopen the windows it closed.
///
/// The update window cannot do this itself: it closes as soon as the installer
/// starts so the installer can replace its own executable. A short PowerShell
/// watcher outlives it, waits for the installer process, and launches the same
/// documents with the new binaries.
pub fn reopen_after_installer(installer_pid: u32, commands: Vec<Vec<String>>) {
    if commands.is_empty() {
        return;
    }
    #[cfg(target_os = "windows")]
    spawn_reopen_watcher(installer_pid, &commands);
    #[cfg(not(target_os = "windows"))]
    let _ = installer_pid;
}

fn sibling_executable(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(|directory| directory.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

#[cfg(target_os = "windows")]
fn spawn_reopen_watcher(installer_pid: u32, commands: &[Vec<String>]) {
    use std::os::windows::process::CommandExt;

    let mut script = String::new();
    script.push_str("$ErrorActionPreference = 'SilentlyContinue'\r\n");
    script.push_str(&format!("Wait-Process -Id {installer_pid}\r\n"));
    script.push_str("Start-Sleep -Milliseconds 500\r\n");
    for command in commands {
        let Some(name) = command.first() else {
            continue;
        };
        let executable = sibling_executable(name);
        script.push_str(&format!(
            "Start-Process -FilePath {}",
            powershell_literal(&executable.to_string_lossy())
        ));
        if command.len() > 1 {
            let arguments = command[1..]
                .iter()
                .map(|argument| format!("\"{}\"", argument.replace('"', "")))
                .collect::<Vec<_>>()
                .join(" ");
            script.push_str(&format!(
                " -ArgumentList {}",
                powershell_literal(&arguments)
            ));
        }
        script.push_str("\r\n");
    }
    script.push_str("Remove-Item -LiteralPath $PSCommandPath -Force\r\n");

    let path = std::env::temp_dir().join(format!("apexshot-reopen-{}.ps1", std::process::id()));
    if fs::write(&path, script).is_err() {
        return;
    }
    let mut process = std::process::Command::new("powershell.exe");
    process
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    if process.spawn().is_err() {
        let _ = fs::remove_file(&path);
    }
}

/// A single-quoted PowerShell string; an apostrophe is doubled.
#[cfg(target_os = "windows")]
fn powershell_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(target_os = "windows")]
fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return false;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let mut code = 0u32;
    let ok = unsafe { GetExitCodeProcess(handle, &mut code) };
    let _ = unsafe { CloseHandle(handle) };
    ok != 0 && code == STILL_ACTIVE as u32
}

#[cfg(not(target_os = "windows"))]
fn process_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(not(target_os = "windows"))]
fn close_process_windows(_pid: u32) {}

#[cfg(target_os = "windows")]
fn close_process_windows(pid: u32) {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, TRUE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
    };

    struct Search {
        pid: u32,
        windows: Vec<HWND>,
    }

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> windows_sys::core::BOOL {
        let search = &mut *(lparam as *mut Search);
        let mut owner = 0u32;
        GetWindowThreadProcessId(hwnd, &mut owner);
        if owner == search.pid && IsWindowVisible(hwnd) != 0 {
            search.windows.push(hwnd);
        }
        TRUE
    }

    let mut search = Search {
        pid,
        windows: Vec::new(),
    };
    unsafe {
        EnumWindows(Some(collect), &mut search as *mut Search as LPARAM);
        for hwnd in &search.windows {
            PostMessageW(*hwnd, WM_CLOSE, 0, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(kind: WindowKind, pid: u32) -> OpenWindow {
        OpenWindow {
            pid,
            kind,
            reopen: vec!["apexshot-image-editor.exe".to_string()],
            busy: false,
        }
    }

    #[test]
    fn the_summary_counts_each_kind_once_with_its_count() {
        let lines = summary(&[
            window(WindowKind::ImageEditor, 10),
            window(WindowKind::ImageEditor, 11),
            window(WindowKind::History, 12),
            window(WindowKind::History, 13),
            window(WindowKind::History, 14),
        ]);
        assert_eq!(
            lines,
            vec![
                format!("{} (2)", t("Image editor")),
                format!("{} (3)", t("History")),
            ]
        );
    }

    #[test]
    fn a_reopen_command_keeps_its_executable_and_path() {
        let command = reopen_command(IMAGE_EDITOR_EXE, Some(Path::new(r"C:\Shots\a.png")));
        assert_eq!(
            command,
            vec![
                "apexshot-image-editor.exe".to_string(),
                r"C:\Shots\a.png".to_string(),
            ]
        );
        assert_eq!(
            reopen_command(VIDEO_EDITOR_EXE, None),
            vec!["apexshot-video-editor.exe".to_string()]
        );
    }

    #[test]
    fn a_wait_that_finds_no_process_returns_immediately() {
        assert!(wait_for_close(&[], Duration::from_millis(1)).is_empty());
    }

    #[test]
    fn a_record_round_trips_with_its_busy_flag() {
        let record = OpenWindow {
            pid: 42,
            kind: WindowKind::VideoEditor,
            reopen: vec!["apexshot-video-editor.exe".to_string()],
            busy: true,
        };
        let raw = serde_json::to_string(&record).expect("the record serialises");
        assert_eq!(
            serde_json::from_str::<OpenWindow>(&raw).expect("the record parses"),
            record
        );
        assert!(raw.contains("\"busy\":true"));
    }

    #[test]
    fn an_old_record_without_the_busy_field_is_idle() {
        // The shape written before the flag existed.
        let raw = r#"{
            "pid": 42,
            "kind": "video_editor",
            "reopen": ["apexshot-video-editor.exe"]
        }"#;
        let record: OpenWindow = serde_json::from_str(raw).expect("the old record parses");
        assert!(!record.busy);
    }

    #[test]
    fn an_exporting_window_holds_the_install() {
        assert!(!any_busy(&[]));
        assert!(!any_busy(&[
            window(WindowKind::ImageEditor, 10),
            window(WindowKind::VideoEditor, 11),
        ]));
        let mut exporting = window(WindowKind::VideoEditor, 12);
        exporting.busy = true;
        assert!(any_busy(&[window(WindowKind::ImageEditor, 10), exporting,]));
    }
}
