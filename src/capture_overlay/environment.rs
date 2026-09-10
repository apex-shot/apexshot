fn desktop_value_contains(desktop: Option<&str>, needle: &str) -> bool {
    desktop
        .unwrap_or_default()
        .split([':', ';', ','])
        .any(|part| part.trim().eq_ignore_ascii_case(needle))
}

#[cfg(test)]
fn is_gnome_wayland_session_from_env(
    wayland_display: Option<&str>,
    desktop: Option<&str>,
    gnome_setup_display: Option<&str>,
) -> bool {
    let is_wayland = wayland_display.is_some_and(|value| !value.trim().is_empty());
    let is_gnome = desktop_value_contains(desktop, "GNOME")
        || gnome_setup_display.is_some_and(|value| !value.trim().is_empty());
    is_wayland && is_gnome
}

fn should_use_gtk_layer_shell_selector_from_env(
    wayland_display: Option<&str>,
    desktop: Option<&str>,
    gnome_setup_display: Option<&str>,
    hyprland_instance_signature: Option<&str>,
    sway_socket: Option<&str>,
    distro_is_arch: bool,
) -> bool {
    // Always use the Rust GTK LayerShell selector only on Arch-based wlroots
    // compositors where that path is known to work well (e.g. Hyprland/Sway).
    // Arch GNOME, Arch KDE, Fedora, Ubuntu, openSUSE, etc. should stay on the
    // C++ overlay path.
    if distro_is_arch
        && (hyprland_instance_signature.is_some_and(|value| !value.trim().is_empty())
            || sway_socket.is_some_and(|value| !value.trim().is_empty())
            || desktop_value_contains(desktop, "Hyprland")
            || desktop_value_contains(desktop, "sway"))
    {
        return true;
    }

    // Fedora/Ubuntu/openSUSE and other non-Arch paths should stay on the C++
    // overlay path unless a compositor-specific native flow is explicitly
    // required elsewhere.
    let _ = (wayland_display, desktop, gnome_setup_display);
    false
}

fn should_use_gtk_layer_shell_selector() -> bool {
    // Development-only route for visual parity testing on a desktop that would
    // normally take the Qt path (for example GNOME Wayland).
    if std::env::var_os("APEXSHOT_PREVIEW_RUST_CAPTURE_MENU").is_some() {
        return true;
    }

    let distro_is_arch = crate::distro::DistroInfo::detect()
        .map(|info| info.is_arch())
        .unwrap_or(false);
    should_use_gtk_layer_shell_selector_from_env(
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        std::env::var("GNOME_SETUP_DISPLAY").ok().as_deref(),
        std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok().as_deref(),
        std::env::var("SWAYSOCK").ok().as_deref(),
        distro_is_arch,
    )
}

fn force_wayland_gdk_for_layer_shell() {
    // On Hyprland/sway, GDK may default to X11 backend (via XWayland) because
    // DISPLAY=:0 is set. Layer-shell requires the Wayland GDK backend.
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("GDK_BACKEND").is_none() {
        // SAFETY: This must be set before any GTK calls. Callers invoke this
        // before entering the GTK layer-shell selector.
        unsafe { std::env::set_var("GDK_BACKEND", "wayland") };
    }
}

fn is_gnome_session() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .map(|desktop| {
            desktop
                .split(':')
                .any(|part| part.trim().eq_ignore_ascii_case("gnome"))
        })
        .unwrap_or(false)
}

fn execute_builtin_overlay_query<F>(query: F) -> bool
where
    F: FnOnce() -> bool + Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        return std::thread::spawn(query).join().unwrap_or(false);
    }

    query()
}

pub fn builtin_screenshot_overlay_active() -> bool {
    if !is_gnome_session() {
        return false;
    }

    execute_builtin_overlay_query(|| {
        let Ok(conn) = zbus::blocking::Connection::session() else {
            return false;
        };
        let Ok(proxy) = zbus::blocking::Proxy::new(
            &conn,
            "org.gnome.Shell",
            "/org/gnome/Shell",
            "org.gnome.Shell",
        ) else {
            return false;
        };

        let script = "(() => { try { const Main = imports.ui.main; return !!(Main.screenshotUI && Main.screenshotUI.visible); } catch (e) { return false; } })()";
        let Ok((success, value)) = proxy.call::<_, _, (bool, String)>("Eval", &(script)) else {
            return false;
        };
        if !success {
            return false;
        }

        let normalized = value.trim().trim_matches('"');
        matches!(normalized, "true" | "1")
    })
}
