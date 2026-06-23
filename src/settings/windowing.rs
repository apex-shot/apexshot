use gtk4::{gdk, prelude::*, EventControllerMotion, GestureClick};

#[allow(dead_code)]
pub const SETTINGS_WINDOW_MIN_WIDTH: i32 = 920;
#[allow(dead_code)]
pub const SETTINGS_WINDOW_MIN_HEIGHT: i32 = 760;
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

pub fn prefers_dark_glass_theme() -> bool {
    if let Some(settings) = gtk4::Settings::default() {
        if settings.property::<bool>("gtk-application-prefer-dark-theme") {
            return true;
        }

        let theme_name = settings
            .property::<Option<String>>("gtk-theme-name")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if theme_name.contains("dark") {
            return true;
        }
        if theme_name.contains("light") {
            return false;
        }
    }

    if let Some(color_scheme) = read_gsettings("org.gnome.desktop.interface", "color-scheme") {
        if color_scheme.contains("prefer-dark") {
            return true;
        }
        if color_scheme.contains("prefer-light") || color_scheme == "default" {
            return false;
        }
    }

    true
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

#[allow(dead_code)]
fn autostart_dir() -> anyhow::Result<std::path::PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        })
        .ok_or_else(|| anyhow::anyhow!("Unable to resolve XDG config directory"))?;
    Ok(config_home.join("autostart"))
}

#[allow(dead_code)]
pub fn install_autostart_entry_for_current_exe() -> anyhow::Result<std::path::PathBuf> {
    let autostart_dir = autostart_dir()?;
    std::fs::create_dir_all(&autostart_dir)?;

    let binary_path = crate::app_identity::preferred_command_path()
        .display()
        .to_string();

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
         X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n",
        crate::app_identity::daemon_name(),
        crate::app_identity::icon_name(),
    );

    let desktop_path = autostart_dir.join("apexshot-daemon.desktop");
    std::fs::write(&desktop_path, desktop_content)?;
    Ok(desktop_path)
}

#[allow(dead_code)]
pub fn install_autostart_entry_smart() -> anyhow::Result<std::path::PathBuf> {
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
         X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n",
        crate::app_identity::daemon_name(),
        crate::app_identity::icon_name(),
    );

    let desktop_path = autostart_dir.join("apexshot.desktop");
    std::fs::write(&desktop_path, desktop_content)?;
    Ok(desktop_path)
}

#[allow(dead_code)]
pub fn uninstall_autostart_entry() -> anyhow::Result<()> {
    let autostart_dir = autostart_dir()?;
    // Remove both possible autostart files
    for name in ["apexshot-daemon.desktop", "apexshot.desktop"] {
        let desktop_path = autostart_dir.join(name);
        match std::fs::remove_file(&desktop_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
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
