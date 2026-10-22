//! Windows windowing and login-start helpers for the settings window.

use gtk4::gdk;
use gtk4::gdk::prelude::*;
use gtk4::prelude::*;
use std::path::PathBuf;
use std::process::Command;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "ApexShot";

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
/// `APEXSHOT_SHELL` path (development).
fn shell_executable() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("APEXSHOT_SHELL") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }

    let exe = std::env::current_exe()?;
    let sibling = exe.with_file_name("apexshot-capture.exe");
    if sibling.is_file() {
        return Ok(sibling);
    }

    anyhow::bail!(
        "could not find apexshot-capture.exe next to {}; set APEXSHOT_SHELL",
        exe.display()
    )
}

pub fn install_autostart_entry_smart() -> anyhow::Result<PathBuf> {
    let shell = shell_executable()?;
    let value = format!("\"{}\"", shell.display());
    let status = Command::new("reg")
        .args([
            "add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d", &value, "/f",
        ])
        .status()?;
    if !status.success() {
        anyhow::bail!("reg add failed with exit code {:?}", status.code());
    }
    Ok(shell)
}

pub fn uninstall_autostart_entry() -> anyhow::Result<()> {
    let output = Command::new("reg")
        .args(["delete", RUN_KEY, "/v", RUN_VALUE, "/f"])
        .output()?;
    if !output.status.success() {
        // Deleting a value that is not there is a success for this setting.
        let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
        if !stderr.contains("unable to find") && !stderr.contains("not find") {
            anyhow::bail!(
                "reg delete failed with exit code {:?}",
                output.status.code()
            );
        }
    }
    Ok(())
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
