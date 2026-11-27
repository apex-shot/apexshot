use gtk4::{gdk, prelude::*, EventControllerMotion, GestureClick};

const SETTINGS_WINDOW_EDGE_RESIZE_MARGIN: f64 = 8.0;

fn parse_env_bool(name: &str) -> Option<bool> {
    let value = std::env::var(name).ok()?.trim().to_ascii_lowercase();
    match value.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn read_gsettings(schema: &str, key: &str) -> Option<String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let raw = String::from_utf8(output.stdout).ok()?;
    Some(
        raw.trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_ascii_lowercase(),
    )
}

fn read_gsettings_bool(schema: &str, key: &str) -> Option<bool> {
    match read_gsettings(schema, key)?.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// App-wide dark-theme resolution. The canonical implementation lives in the
/// editor UI support module so every window agrees; it honours the Settings →
/// General → Theme choice before falling back to the desktop preference.
pub fn prefers_dark_glass_theme() -> bool {
    crate::capture::editor::ui_support::prefers_dark_glass_theme()
}

pub fn prefers_reduced_transparency() -> bool {
    if let Some(value) = parse_env_bool("APEXSHOT_REDUCED_TRANSPARENCY") {
        return value;
    }

    if let Some(settings) = gtk4::Settings::default() {
        let theme_name = settings
            .property::<Option<String>>("gtk-theme-name")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if theme_name.contains("highcontrast") {
            return true;
        }

        if !settings.property::<bool>("gtk-enable-animations") {
            return true;
        }
    }

    if read_gsettings_bool("org.gnome.desktop.a11y.interface", "high-contrast").unwrap_or(false) {
        return true;
    }

    if let Some(animations_enabled) =
        read_gsettings_bool("org.gnome.desktop.interface", "enable-animations")
    {
        return !animations_enabled;
    }

    false
}

fn autostart_dir() -> anyhow::Result<std::path::PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        })
        .ok_or_else(|| anyhow::anyhow!("Unable to resolve XDG config directory"))?;
    Ok(config_home.join("autostart"))
}

/// The file name every Linux package installs under `/etc/xdg/autostart`. A
/// user file with the same name overrides it, which is how login startup is
/// turned off for a packaged install.
const SYSTEM_AUTOSTART_FILE: &str = "apexshot.desktop";

/// A user override that turns the packaged autostart entry off.
const AUTOSTART_OPT_OUT: &str =
    "[Desktop Entry]\nType=Application\nName=ApexShot Daemon\nHidden=true\n";

fn system_autostart_dirs() -> Vec<std::path::PathBuf> {
    let dirs = std::env::var("XDG_CONFIG_DIRS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "/etc/xdg".to_string());
    dirs.split(':')
        .filter(|dir| !dir.is_empty())
        .map(|dir| std::path::PathBuf::from(dir).join("autostart"))
        .collect()
}

fn system_autostart_entry(system_dirs: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    system_dirs
        .iter()
        .map(|dir| dir.join(SYSTEM_AUTOSTART_FILE))
        .find(|path| path.is_file())
}

/// Whether a desktop entry's text turns the entry off.
fn desktop_entry_disabled(content: &str) -> bool {
    content.lines().map(str::trim).any(|line| {
        line.eq_ignore_ascii_case("Hidden=true")
            || line.eq_ignore_ascii_case("X-GNOME-Autostart-enabled=false")
    })
}

/// Whether ApexShot starts at login, given the user and system autostart
/// directories: a user `apexshot.desktop` decides on its own, a user
/// `apexshot-daemon.desktop` turns it on, and otherwise a packaged system
/// entry does.
fn autostart_enabled_in(user_dir: &std::path::Path, system_dirs: &[std::path::PathBuf]) -> bool {
    let user_entry = user_dir.join(SYSTEM_AUTOSTART_FILE);
    if user_entry.is_file() {
        return std::fs::read_to_string(&user_entry)
            .map(|content| !desktop_entry_disabled(&content))
            .unwrap_or(true);
    }
    if user_dir.join("apexshot-daemon.desktop").is_file() {
        return true;
    }
    system_autostart_entry(system_dirs).is_some()
}

/// The login-startup state the Settings toggle shows. A portal-only build
/// cannot read the portal's state back, so it keeps the saved setting.
pub fn effective_start_at_login(configured: bool) -> bool {
    if crate::app_identity::portal_only() {
        return configured;
    }
    match autostart_dir() {
        Ok(user_dir) => autostart_enabled_in(&user_dir, &system_autostart_dirs()),
        Err(_) => configured,
    }
}

/// Flatpak autostart via XDG Background portal (no ~/.config/autostart write).
fn request_background_autostart(enable: bool) -> anyhow::Result<std::path::PathBuf> {
    let reason = if enable {
        "Start ApexShot on login for tray icon and global hotkeys"
    } else {
        "Disable ApexShot login autostart"
    };

    let result = block_on_async(async {
        ashpd::desktop::background::Background::request()
            .reason(reason)
            .auto_start(enable)
            .command(["apexshot", "daemon"])
            .dbus_activatable(false)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Background portal request failed: {e}"))?
            .response()
            .map_err(|e| anyhow::anyhow!("Background portal response failed: {e}"))
    })?;

    if enable && !result.auto_start() {
        anyhow::bail!("Background portal did not grant autostart");
    }

    // No desktop file is written; return a marker path for callers that log it.
    Ok(std::path::PathBuf::from(if enable {
        "portal:background-autostart"
    } else {
        "portal:background-autostart-disabled"
    }))
}

fn block_on_async<F, T>(future: F) -> anyhow::Result<T>
where
    F: std::future::Future<Output = anyhow::Result<T>>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(future)),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(future)
        }
    }
}

pub fn install_autostart_entry_smart() -> anyhow::Result<std::path::PathBuf> {
    if crate::app_identity::portal_only() {
        return request_background_autostart(true);
    }
    // A packaged install already starts from its system entry. Clear any user
    // override (including an opt-out) so that entry runs, instead of writing a
    // second entry that would start a second daemon.
    if let Some(system_entry) = system_autostart_entry(&system_autostart_dirs()) {
        let user_dir = autostart_dir()?;
        for name in ["apexshot-daemon.desktop", SYSTEM_AUTOSTART_FILE] {
            match std::fs::remove_file(user_dir.join(name)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        return Ok(system_entry);
    }
    let autostart_dir = autostart_dir()?;
    std::fs::create_dir_all(&autostart_dir)?;

    let binary_path = crate::app_identity::preferred_command_path()
        .display()
        .to_string();

    // The daemon itself reads config and decides whether to show the tray icon.
    // We always start the daemon — it exits immediately if show_menu_bar_icon is false.
    let desktop_content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={}\n\
         Comment=ApexShot screenshot daemon - tray icon and hotkey listener\n\
         Exec={binary_path} daemon\n\
         Icon={}\n\
         Categories=Utility;\n\
         Keywords=screenshot;capture;record;\n\
         StartupNotify=false\n\
         X-GNOME-Autostart-enabled=true\n\
         X-GNOME-Autostart-Delay=2\n\
         Hidden=false\n\
         NoDisplay=true\n\
         X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n\
         X-KDE-Wayland-Interfaces=zkde_screencast_unstable_v1\n",
        crate::app_identity::daemon_name(),
        crate::app_identity::icon_name(),
    );

    let desktop_path = autostart_dir.join("apexshot.desktop");
    std::fs::write(&desktop_path, desktop_content)?;
    Ok(desktop_path)
}

pub fn uninstall_autostart_entry() -> anyhow::Result<()> {
    if crate::app_identity::portal_only() {
        request_background_autostart(false)?;
        return Ok(());
    }
    let autostart_dir = autostart_dir()?;
    match std::fs::remove_file(autostart_dir.join("apexshot-daemon.desktop")) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let user_entry = autostart_dir.join(SYSTEM_AUTOSTART_FILE);
    if system_autostart_entry(&system_autostart_dirs()).is_some() {
        // The packaged entry cannot be removed by the user; an override with
        // the same name and Hidden=true turns it off.
        std::fs::create_dir_all(&autostart_dir)?;
        std::fs::write(&user_entry, AUTOSTART_OPT_OUT)?;
        return Ok(());
    }
    match std::fs::remove_file(&user_entry) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

fn edge_cursor_name(x: f64, y: f64, width: f64, height: f64) -> Option<&'static str> {
    let left = x <= SETTINGS_WINDOW_EDGE_RESIZE_MARGIN;
    let right = x >= width - SETTINGS_WINDOW_EDGE_RESIZE_MARGIN;
    let top = y <= SETTINGS_WINDOW_EDGE_RESIZE_MARGIN;
    let bottom = y >= height - SETTINGS_WINDOW_EDGE_RESIZE_MARGIN;

    match (left, right, top, bottom) {
        (true, false, true, false) => Some("nw-resize"),
        (false, true, true, false) => Some("ne-resize"),
        (true, false, false, true) => Some("sw-resize"),
        (false, true, false, true) => Some("se-resize"),
        (false, false, true, false) => Some("n-resize"),
        (false, false, false, true) => Some("s-resize"),
        (true, false, false, false) => Some("w-resize"),
        (false, true, false, false) => Some("e-resize"),
        _ => None,
    }
}

fn edge_for_resize(x: f64, y: f64, width: f64, height: f64) -> Option<gdk::SurfaceEdge> {
    let left = x <= SETTINGS_WINDOW_EDGE_RESIZE_MARGIN;
    let right = x >= width - SETTINGS_WINDOW_EDGE_RESIZE_MARGIN;
    let top = y <= SETTINGS_WINDOW_EDGE_RESIZE_MARGIN;
    let bottom = y >= height - SETTINGS_WINDOW_EDGE_RESIZE_MARGIN;

    match (left, right, top, bottom) {
        (true, false, true, false) => Some(gdk::SurfaceEdge::NorthWest),
        (false, true, true, false) => Some(gdk::SurfaceEdge::NorthEast),
        (true, false, false, true) => Some(gdk::SurfaceEdge::SouthWest),
        (false, true, false, true) => Some(gdk::SurfaceEdge::SouthEast),
        (false, false, true, false) => Some(gdk::SurfaceEdge::North),
        (false, false, false, true) => Some(gdk::SurfaceEdge::South),
        (true, false, false, false) => Some(gdk::SurfaceEdge::West),
        (false, true, false, false) => Some(gdk::SurfaceEdge::East),
        _ => None,
    }
}

pub fn install_window_drag(toolbar: &impl IsA<gtk4::Widget>, window: &gtk4::ApplicationWindow) {
    let drag_window_gesture = GestureClick::new();
    drag_window_gesture.set_button(1);
    let window_drag = window.downgrade();
    drag_window_gesture.connect_pressed(move |gesture, _, x, y| {
        let Some(window) = window_drag.upgrade() else {
            return;
        };
        let Some(event) = gesture.current_event() else {
            return;
        };
        let Some(device) = event.device() else {
            return;
        };
        let Some(surface) = window.surface() else {
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            return;
        };
        toplevel.begin_move(&device, gesture.current_button() as i32, x, y, event.time());
    });
    toolbar.add_controller(drag_window_gesture);
}

pub fn install_edge_resize(root: &impl IsA<gtk4::Widget>, window: &gtk4::ApplicationWindow) {
    let resize_motion = EventControllerMotion::new();
    let window_resize_motion = window.downgrade();
    resize_motion.connect_motion(move |controller, x, y| {
        let Some(widget) = controller.widget() else {
            return;
        };
        let width = widget.allocated_width() as f64;
        let height = widget.allocated_height() as f64;
        let cursor = edge_cursor_name(x, y, width, height)
            .and_then(|name| gdk::Cursor::from_name(name, None));
        widget.set_cursor(cursor.as_ref());
        if cursor.is_none() {
            if let Some(window) = window_resize_motion.upgrade() {
                window.set_cursor(None);
            }
        }
    });

    let resize_motion_leave = EventControllerMotion::new();
    resize_motion_leave.connect_leave(move |controller| {
        if let Some(widget) = controller.widget() {
            widget.set_cursor(None);
        }
    });

    let resize_click = GestureClick::new();
    resize_click.set_button(1);
    let window_resize = window.downgrade();
    resize_click.connect_pressed(move |gesture, _, x, y| {
        let Some(window) = window_resize.upgrade() else {
            return;
        };
        let Some(event) = gesture.current_event() else {
            return;
        };
        let Some(device) = event.device() else {
            return;
        };
        let width = window.allocated_width() as f64;
        let height = window.allocated_height() as f64;
        let Some(edge) = edge_for_resize(x, y, width, height) else {
            return;
        };
        let Some(surface) = window.surface() else {
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            return;
        };
        toplevel.begin_resize(
            edge,
            Some(&device),
            gesture.current_button() as i32,
            x,
            y,
            event.time(),
        );
    });

    root.add_controller(resize_motion);
    root.add_controller(resize_motion_leave);
    root.add_controller(resize_click);
}

#[cfg(test)]
mod autostart_state_tests {
    use super::*;

    fn scratch(case: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("apexshot-autostart-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("user")).unwrap();
        std::fs::create_dir_all(dir.join("system")).unwrap();
        dir
    }

    #[test]
    fn a_packaged_entry_starts_at_login_until_the_user_opts_out() {
        let dir = scratch("packaged");
        let user = dir.join("user");
        let system = vec![dir.join("system")];
        assert!(!autostart_enabled_in(&user, &system));
        std::fs::write(
            dir.join("system").join(SYSTEM_AUTOSTART_FILE),
            "[Desktop Entry]\n",
        )
        .unwrap();
        assert!(autostart_enabled_in(&user, &system));
        std::fs::write(user.join(SYSTEM_AUTOSTART_FILE), AUTOSTART_OPT_OUT).unwrap();
        assert!(!autostart_enabled_in(&user, &system));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_user_entry_decides_without_a_packaged_one() {
        let dir = scratch("user-only");
        let user = dir.join("user");
        let system = vec![dir.join("system")];
        std::fs::write(user.join("apexshot-daemon.desktop"), "[Desktop Entry]\n").unwrap();
        assert!(autostart_enabled_in(&user, &system));
        std::fs::write(
            user.join(SYSTEM_AUTOSTART_FILE),
            "[Desktop Entry]\nX-GNOME-Autostart-enabled=false\n",
        )
        .unwrap();
        assert!(!autostart_enabled_in(&user, &system));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
