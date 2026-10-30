//! Windows windowing and login-start helpers for the settings window.

use gtk4::gdk;
use gtk4::gdk::prelude::*;
use gtk4::prelude::*;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "ApexShot";
static LAUNCHER_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn parse_env_bool(name: &str) -> Option<bool> {
    let value = std::env::var(name).ok()?.trim().to_ascii_lowercase();
    match value.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub fn prefers_dark_glass_theme() -> bool {
    crate::capture::editor::ui_support::prefers_dark_glass_theme()
}

pub fn prefers_reduced_transparency() -> bool {
    if let Some(value) = parse_env_bool("APEXSHOT_REDUCED_TRANSPARENCY") {
        return value;
    }
    if let Some(settings) = gtk4::Settings::default() {
        let theme = settings
            .property::<Option<String>>("gtk-theme-name")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if theme.contains("highcontrast") {
            return true;
        }
    }
    false
}

/// Launch target for "Start at login".
///
/// The settings binary is not the tray shell, so prefer the sibling
/// `apexshot-capture.exe` (the packaged layout) or an explicit
/// `APEXSHOT_SHELL` path. Debug builds also recognize the local CMake build.
fn shell_executable() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("APEXSHOT_SHELL") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        anyhow::bail!(
            "APEXSHOT_SHELL does not point to a file: {}",
            path.display()
        );
    }

    let exe = std::env::current_exe()?;
    let sibling = exe.with_file_name("apexshot-capture.exe");
    if sibling.is_file() {
        return Ok(sibling);
    }

    if cfg!(debug_assertions) {
        if let Some(root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
            let local = root.join("capture-overlay/build/apexshot-capture.exe");
            if local.is_file() {
                return Ok(local);
            }
        }
    }

    anyhow::bail!(
        "could not find apexshot-capture.exe next to {}; set APEXSHOT_SHELL",
        exe.display()
    )
}

pub fn install_autostart_entry_smart() -> anyhow::Result<PathBuf> {
    let shell = shell_executable()?;
    let settings = std::env::current_exe()?;
    let value = if shell.parent() == settings.parent()
        && shell.with_file_name("Qt5Core.dll").is_file()
        && (settings.with_file_name("gtk-4-1.dll").is_file()
            || settings.with_file_name("libgtk-4-1.dll").is_file())
    {
        autostart_command(&shell)
    } else {
        let qt = shell
            .parent()
            .filter(|dir| dir.join("Qt5Core.dll").is_file())
            .map(PathBuf::from)
            .or_else(|| find_runtime_directory("Qt5Core.dll"))
            .ok_or_else(|| anyhow::anyhow!("Qt5Core.dll was not found on PATH"))?;
        let gtk = settings
            .parent()
            .filter(|dir| dir.join("gtk-4-1.dll").is_file() || dir.join("libgtk-4-1.dll").is_file())
            .map(PathBuf::from)
            .or_else(|| find_runtime_directory("gtk-4-1.dll"))
            .or_else(|| find_runtime_directory("libgtk-4-1.dll"))
            .ok_or_else(|| anyhow::anyhow!("the GTK runtime was not found on PATH"))?;
        let script = crate::config::config_path()
            .and_then(|path| path.parent().map(|parent| parent.join("start-capture.vbs")))
            .ok_or_else(|| anyhow::anyhow!("could not resolve the config directory"))?;
        let parent = script
            .parent()
            .ok_or_else(|| anyhow::anyhow!("login launcher has no parent directory"))?;
        std::fs::create_dir_all(parent)?;
        let content = encode_startup_script(&startup_script(&shell, &settings, &[qt, gtk]));
        if script.exists() {
            let previous = std::fs::read(&script)?;
            anyhow::ensure!(
                previous.starts_with(&encode_startup_script("' ApexShot login launcher\n")),
                "refusing to replace an unrecognized login launcher at {}",
                script.display()
            );
        }
        let sequence = LAUNCHER_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".start-capture-{}-{sequence}.tmp",
            std::process::id()
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let result = (|| -> anyhow::Result<()> {
            file.write_all(&content)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, &script)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result?;
        format!("wscript.exe //B //Nologo \"{}\"", script.display())
    };
    let mut command = Command::new("reg");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let status = command
        .args([
            "add",
            &format!(r"HKCU\{RUN_KEY}"),
            "/v",
            RUN_VALUE,
            "/t",
            "REG_SZ",
            "/d",
            &value,
            "/f",
        ])
        .status()?;
    if !status.success() {
        anyhow::bail!("reg add failed with exit code {:?}", status.code());
    }
    Ok(shell)
}

fn find_runtime_directory(dll: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).find(|dir| dir.join(dll).is_file())
}

fn startup_script(shell: &std::path::Path, settings: &std::path::Path, dirs: &[PathBuf]) -> String {
    let quote = |value: &str| value.replace('"', "\"\"");
    let paths = dirs
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(";");
    let mut script = format!(
        "' ApexShot login launcher\n\
         Set shell = CreateObject(\"WScript.Shell\")\n\
         Set env = shell.Environment(\"PROCESS\")\n\
         env(\"PATH\") = \"{};\" & env(\"PATH\")\n",
        quote(&paths)
    );
    for (name, path) in [
        ("APEXSHOT_SETTINGS", settings.to_path_buf()),
        (
            "APEXSHOT_HISTORY",
            settings.with_file_name("apexshot-history.exe"),
        ),
        (
            "APEXSHOT_VIDEO_EDITOR",
            settings.with_file_name("apexshot-video-editor.exe"),
        ),
    ] {
        if path.is_file() {
            script.push_str(&format!(
                "env(\"{name}\") = \"{}\"\n",
                quote(&path.to_string_lossy())
            ));
        }
    }
    script.push_str(&format!(
        "shell.Run \"\"\"{}\"\" --background\", 0, False\n",
        quote(&shell.to_string_lossy())
    ));
    script
}

fn encode_startup_script(script: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(script.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

fn autostart_command(shell: &std::path::Path) -> String {
    format!("\"{}\" --background", shell.display())
}

#[cfg(target_os = "windows")]
pub fn autostart_entry_exists() -> anyhow::Result<bool> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};

    let key: Vec<u16> = std::ffi::OsStr::new(RUN_KEY)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let value: Vec<u16> = std::ffi::OsStr::new(RUN_VALUE)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut size = 0;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    match result {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND => Ok(false),
        error => Err(std::io::Error::from_raw_os_error(error as i32).into()),
    }
}

#[cfg(not(target_os = "windows"))]
pub fn autostart_entry_exists() -> anyhow::Result<bool> {
    anyhow::bail!("Windows login startup is only available on Windows")
}

#[cfg(target_os = "windows")]
pub fn uninstall_autostart_entry() -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, HKEY_CURRENT_USER};

    let key: Vec<u16> = std::ffi::OsStr::new(RUN_KEY)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let value: Vec<u16> = std::ffi::OsStr::new(RUN_VALUE)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) };
    if result != ERROR_SUCCESS && result != ERROR_FILE_NOT_FOUND {
        return Err(std::io::Error::from_raw_os_error(result as i32).into());
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn uninstall_autostart_entry() -> anyhow::Result<()> {
    anyhow::bail!("Windows login startup is only available on Windows")
}

/// The Windows title bar is drawn by the app; drag the window from the custom
/// handle and toggle maximize on a double click.
pub fn install_window_drag(handle: &impl IsA<gtk4::Widget>, window: &gtk4::ApplicationWindow) {
    let drag = gtk4::GestureDrag::new();
    let window_clone = window.clone();
    drag.connect_drag_begin(move |gesture, start_x, start_y| {
        let Some(surface) = window_clone.surface() else {
            return;
        };
        let Some(toplevel) = surface.downcast_ref::<gdk::Toplevel>() else {
            return;
        };
        let Some(device) = gesture.current_event_device() else {
            return;
        };
        let Some(event) = gesture.current_event() else {
            return;
        };
        let button = event
            .downcast_ref::<gdk::ButtonEvent>()
            .map(|event| event.button())
            .unwrap_or(1);
        toplevel.begin_move(&device, button as i32, start_x, start_y, event.time());
    });
    handle.add_controller(drag);

    let click = gtk4::GestureClick::new();
    let window_clone = window.clone();
    click.connect_released(move |_, n_press, _, _| {
        if n_press == 2 {
            if window_clone.is_maximized() {
                window_clone.unmaximize();
            } else {
                window_clone.maximize();
            }
        }
    });
    handle.add_controller(click);
}

/// The Windows window manager already resizes the window.
pub fn install_edge_resize(_root: &impl IsA<gtk4::Widget>, _window: &gtk4::ApplicationWindow) {}

#[cfg(test)]
mod tests {
    use super::{autostart_command, encode_startup_script, startup_script};
    use std::path::{Path, PathBuf};

    #[test]
    fn login_start_quotes_the_shell_and_keeps_it_in_the_background() {
        assert_eq!(
            autostart_command(Path::new(r"C:\Program Files\ApexShot\apexshot-capture.exe")),
            r#""C:\Program Files\ApexShot\apexshot-capture.exe" --background"#
        );
    }

    #[test]
    fn development_launcher_sets_runtime_paths_and_starts_hidden() {
        let script = startup_script(
            Path::new(r"C:\Dev Tools\apexshot-capture.exe"),
            Path::new(r"C:\Dev Tools\apexshot-settings.exe"),
            &[PathBuf::from(r"C:\Qt\bin"), PathBuf::from(r"C:\gtk\bin")],
        );
        assert!(script.contains(r#"env("PATH") = "C:\Qt\bin;C:\gtk\bin;" & env("PATH")"#));
        assert!(script.contains(
            r#"shell.Run """C:\Dev Tools\apexshot-capture.exe"" --background", 0, False"#
        ));
    }

    #[test]
    fn development_launcher_is_utf16_for_windows_script_host() {
        assert_eq!(
            encode_startup_script("A\u{e9}"),
            [0xff, 0xfe, 0x41, 0, 0xe9, 0]
        );
    }
}
