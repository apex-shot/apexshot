use anyhow::Context;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

mod config;
mod gnome;
mod kde;
mod portal;
mod wlroots;

pub use config::{
    accel_to_gnome, accel_to_portal, hotkey_config_from_app_config, load_hotkey_config,
    reset_hotkey_config, HotkeyBinding, HotkeyConfig,
};
pub use gnome::{
    run_gnome_hotkey_daemon, sync_gnome_hotkeys_for_current_desktop, GnomeHotkeySyncResult,
};
pub use portal::run_portal_hotkey_daemon;
pub use wlroots::{
    export_configured_hotkeys_for_hyprland, export_hotkeys_for_hyprland, export_hotkeys_for_niri,
    export_hotkeys_for_river, export_hotkeys_for_sway,
};

pub(super) fn desktop_exec_value() -> String {
    let exe = crate::app_identity::preferred_command_path()
        .to_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "apexshot".to_string());

    // Desktop entry Exec is not shell-parsed; spaces must be escaped per spec.
    let escaped_exe = exe.replace('\\', "\\\\").replace(' ', "\\ ");
    format!("{escaped_exe} daemon")
}

pub(super) fn default_daemon_log_path() -> Option<PathBuf> {
    let mut dir = dirs::cache_dir()?;
    dir.push("apexshot");
    dir.push("hotkey-daemon.log");
    Some(dir)
}

pub(super) fn open_daemon_log_if_needed() -> Option<(PathBuf, std::fs::File)> {
    let path = if let Ok(p) = std::env::var("APEXSHOT_HOTKEY_LOG") {
        Some(PathBuf::from(p))
    } else if !std::io::stderr().is_terminal() {
        default_daemon_log_path()
    } else {
        None
    }?;

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;

    Some((path, file))
}

pub(super) fn log_line(log: &mut Option<std::fs::File>, msg: &str) {
    eprintln!("{msg}");
    if let Some(file) = log.as_mut() {
        let _ = writeln!(file, "{msg}");
    }
}

pub(super) fn hotkey_debug_enabled() -> bool {
    std::env::var_os("APEXSHOT_HOTKEY_DEBUG").is_some()
}

pub(super) fn resolve_action_exe() -> anyhow::Result<PathBuf> {
    let preferred = crate::app_identity::preferred_command_path();
    if preferred.exists() {
        return Ok(preferred);
    }

    if let Some(arg0) = std::env::args_os().next() {
        let p = config::strip_deleted_suffix(std::path::Path::new(&arg0));
        if p.is_absolute() && p.exists() {
            return Ok(p);
        }
        if let Ok(canon) = std::fs::canonicalize(&p) {
            return Ok(canon);
        }
    }

    let p = std::env::current_exe().context("Failed to get current executable")?;
    let cleaned = config::strip_deleted_suffix(&p);
    if cleaned.exists() {
        return Ok(cleaned);
    }
    Ok(p)
}

pub(super) fn daemon_pid_file_path() -> anyhow::Result<PathBuf> {
    let mut dir =
        dirs::cache_dir().ok_or_else(|| anyhow::anyhow!("Failed to resolve cache dir"))?;
    dir.push("apexshot");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create daemon state dir {}", dir.display()))?;
    dir.push("hotkey-daemon.pid");
    Ok(dir)
}

pub(super) fn is_pid_running(pid: u32) -> bool {
    PathBuf::from(format!("/proc/{pid}")).exists()
}

pub(super) fn existing_daemon_pid() -> Option<u32> {
    let path = daemon_pid_file_path().ok()?;
    let pid = std::fs::read_to_string(path)
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()?;
    if pid != std::process::id() && is_pid_running(pid) {
        Some(pid)
    } else {
        None
    }
}

pub(super) struct DaemonPidGuard {
    path: PathBuf,
}

impl Drop for DaemonPidGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(super) fn acquire_daemon_pid_guard() -> anyhow::Result<DaemonPidGuard> {
    use std::io::ErrorKind;

    let path = daemon_pid_file_path()?;
    let current_pid = std::process::id();

    for _ in 0..2 {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                writeln!(file, "{current_pid}")
                    .with_context(|| format!("Failed to write pid file {}", path.display()))?;
                return Ok(DaemonPidGuard { path });
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                let existing_pid = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|s| s.trim().parse::<u32>().ok());

                if let Some(pid) = existing_pid {
                    if pid != current_pid && is_pid_running(pid) {
                        anyhow::bail!(
                            "Hotkey daemon already running (pid {pid}). Stop it first (e.g. `pkill -f \"apexshot daemon\"`) and retry"
                        );
                    }
                }

                let _ = std::fs::remove_file(&path);
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Failed to create daemon pid file {}: {}",
                    path.display(),
                    e
                ));
            }
        }
    }

    anyhow::bail!(
        "Failed to acquire hotkey daemon pid file lock at {}",
        path.display()
    )
}

pub(super) fn spawn_hotkey_action(
    preferred_exe: Option<&PathBuf>,
    args: &[String],
) -> anyhow::Result<(std::process::Child, PathBuf)> {
    use std::io::ErrorKind;
    use std::process::Stdio;

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(exe) = preferred_exe {
        candidates.push(exe.clone());
    }
    if let Ok(exe) = resolve_action_exe() {
        candidates.push(exe);
    }
    candidates.push(PathBuf::from("/proc/self/exe"));
    if let Ok(exe) = std::env::current_exe() {
        candidates.push(config::strip_deleted_suffix(&exe));
    }

    let mut seen = HashSet::new();
    candidates.retain(|p| seen.insert(p.clone()));

    let mut not_found: Vec<PathBuf> = Vec::new();
    for exe in candidates {
        let mut cmd = std::process::Command::new(&exe);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match cmd.spawn() {
            Ok(child) => return Ok((child, exe)),
            Err(e) if e.kind() == ErrorKind::NotFound => not_found.push(exe),
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "spawn failed via {} for args {:?}: {}",
                    exe.display(),
                    args,
                    e
                ));
            }
        }
    }

    if not_found.is_empty() {
        anyhow::bail!("spawn failed for args {:?}: no executable candidates", args);
    }

    anyhow::bail!(
        "spawn failed for args {:?}: executable not found (tried: {})",
        args,
        not_found
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub(super) fn ensure_desktop_entry(app_id: &str) -> anyhow::Result<PathBuf> {
    let mut dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    dir.push("applications");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create applications dir {}", dir.display()))?;

    let mut path = dir;
    path.push(format!("{app_id}.desktop"));

    // Minimal desktop entry: GlobalShortcuts uses this to associate the app_id with the caller.
    // Exec must reference a resolvable binary; otherwise GLib may ignore the app info.
    let is_daemon = app_id.ends_with(".daemon");
    let content = if is_daemon {
        // desktop_exec_value() already includes "daemon" suffix
        format!(
            "[Desktop Entry]\nType=Application\nName={}\nExec={}\nIcon={}\nTerminal=false\nCategories=Utility;\nNoDisplay=true\n",
            crate::app_identity::daemon_name(),
            desktop_exec_value(),
            crate::app_identity::icon_name()
        )
    } else {
        let exe = crate::app_identity::preferred_command_path()
            .to_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "apexshot".to_string());
        let escaped_exe = exe.replace('\\', "\\\\").replace(' ', "\\ ");
        format!(
            "[Desktop Entry]\nType=Application\nName={}\nExec={}\nIcon={}\nTerminal=false\nCategories=Utility;\n",
            crate::app_identity::app_name(),
            escaped_exe,
            crate::app_identity::icon_name()
        )
    };

    if let Ok(existing) = std::fs::read_to_string(&path) {
        if existing == content {
            return Ok(path);
        }
    }

    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write desktop entry {}", path.display()))?;

    Ok(path)
}

pub(super) fn apply_gio_desktop_launch_env(desktop_path: &PathBuf) {
    // GNOME/portal backends often rely on these variables to associate an unsandboxed
    // process with its .desktop file (and thus its application id).
    // If we were launched from a terminal, they are typically unset.
    if std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE").is_none() {
        std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", desktop_path);
    }
    if std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE_PID").is_none() {
        std::env::set_var(
            "GIO_LAUNCHED_DESKTOP_FILE_PID",
            std::process::id().to_string(),
        );
    }
}

pub(super) fn try_relaunch_via_desktop(
    app_id: &str,
    config_path: &PathBuf,
    configure: bool,
) -> anyhow::Result<()> {
    if let Some(msg) = crate::app_identity::host_escape_blocked("gtk-launch") {
        anyhow::bail!(msg);
    }
    if std::env::var_os("APEXSHOT_DESKTOP_RELAUNCHED").is_some() {
        return Ok(());
    }

    let mut cmd = std::process::Command::new("gtk-launch");
    cmd.arg(app_id);
    cmd.env("APEXSHOT_DESKTOP_RELAUNCHED", "1");
    cmd.env("APEXSHOT_HOTKEY_CONFIG", config_path);
    if configure {
        cmd.env("APEXSHOT_HOTKEY_CONFIGURE", "1");
    }

    // Ensure the desktop-launched daemon writes logs somewhere discoverable.
    if std::env::var_os("APEXSHOT_HOTKEY_LOG").is_none() {
        if let Some(p) = default_daemon_log_path() {
            cmd.env("APEXSHOT_HOTKEY_LOG", p);
        }
    }

    cmd.spawn()
        .map(|_| ())
        .with_context(|| format!("Failed to relaunch via desktop (gtk-launch {app_id})"))
}

/// Public wrapper so the daemon can ensure the desktop entry exists and get its path.
/// Used to set GIO_LAUNCHED_DESKTOP_FILE so GNOME trusts the daemon process.
pub fn ensure_desktop_entry_pub(app_id: &str) -> anyhow::Result<std::path::PathBuf> {
    ensure_desktop_entry(app_id)
}

pub fn sync_hotkeys_from_app_config(app_config: &crate::config::AppConfig) -> anyhow::Result<()> {
    if let Some(msg) = crate::app_identity::host_escape_blocked("compositor hotkey install") {
        anyhow::bail!(msg);
    }
    let path = config::default_config_path();
    let cfg = config::hotkey_config_from_app_config(app_config);
    config::save_hotkey_config(&path, &cfg)?;

    // For compositors that don't provide a GlobalShortcuts portal, generate
    // compositor-native bind-snippet files so the user's configured shortcuts
    // are picked up by the WM. The daemon portal listener is best-effort and
    // may fail on compositors without portal support.
    if let Some(comp) = crate::compositor::detect_compositor() {
        match comp.name() {
            "Hyprland" => {
                wlroots::write_hyprland_hotkey_snippet(&cfg)?;
            }
            "Sway/i3" => {
                wlroots::write_sway_hotkey_snippet(&cfg)?;
            }
            "Niri" => {
                wlroots::write_niri_hotkey_snippet(&cfg)?;
            }
            "River" => {
                wlroots::write_river_hotkey_snippet(&cfg)?;
            }
            _ => {}
        }
    }

    kde::sync_kde_hotkeys_if_applicable(&cfg)?;

    Ok(())
}

pub(super) fn prompt_line(prompt: &str) -> anyhow::Result<String> {
    print!("{prompt}");
    std::io::stdout()
        .flush()
        .context("Failed to flush stdout")?;

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .context("Failed to read input")?;
    Ok(input.trim().to_string())
}

pub(super) fn pretty_action_name(binding: &config::HotkeyBinding, idx: usize) -> String {
    let name = binding
        .name
        .as_deref()
        .unwrap_or("binding")
        .replace('_', " ")
        .split_whitespace()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) => format!("{}{}", c.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    if name.trim().is_empty() {
        format!("Binding {}", idx + 1)
    } else {
        name
    }
}

pub(super) fn shell_quote(arg: &str) -> String {
    let escaped = arg
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");
    format!("\"{escaped}\"")
}

pub fn install_hotkeys_for_current_desktop(config_path: Option<PathBuf>) -> anyhow::Result<()> {
    if let Some(msg) = crate::app_identity::host_escape_blocked("host hotkey install") {
        anyhow::bail!(msg);
    }
    let (_config_path, cfg) = config::load_or_create_config(config_path)?;

    if gnome::is_gnome_desktop() {
        gnome::install_gnome_custom_keybindings(&cfg)?;
        println!("Installed GNOME custom keybindings for ApexShot (no daemon required).");
        return Ok(());
    }

    anyhow::bail!(
        "No-daemon hotkey install is currently supported on GNOME only (current desktop: {}).",
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".into())
    )
}

pub fn uninstall_hotkeys_for_current_desktop() -> anyhow::Result<()> {
    if let Some(msg) = crate::app_identity::host_escape_blocked("host hotkey uninstall") {
        anyhow::bail!(msg);
    }
    if gnome::is_gnome_desktop() {
        gnome::uninstall_gnome_custom_keybindings()?;
        println!("Removed ApexShot GNOME custom keybindings.");
        return Ok(());
    }

    anyhow::bail!(
        "No-daemon hotkey uninstall is currently supported on GNOME only (current desktop: {}).",
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".into())
    )
}

pub fn setup_hotkeys_for_current_desktop(config_path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let (path, mut cfg) = config::load_or_create_config(config_path)?;

    println!(
        "\nApexShot hotkey setup wizard (current desktop: {})",
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".into())
    );
    println!("Press Enter to keep current binding. Type 'none' to clear a binding.\n");

    for (idx, binding) in cfg.bindings.iter_mut().enumerate() {
        let action = pretty_action_name(binding, idx);
        let command_preview = binding.args.join(" ");
        let prompt = format!("{action} ({command_preview}) [{}]: ", binding.accelerator);
        let entered = prompt_line(&prompt)?;
        if entered.is_empty() {
            continue;
        }
        if entered.eq_ignore_ascii_case("none") {
            binding.accelerator.clear();
            continue;
        }
        binding.accelerator = config::as_portal_trigger(&entered);
    }

    cfg.bindings.retain(|b| !b.accelerator.trim().is_empty());

    if cfg.bindings.is_empty() {
        anyhow::bail!("No bindings left after setup; aborting to avoid disabling all shortcuts");
    }

    config::save_hotkey_config(&path, &cfg)?;
    println!("Saved hotkey config: {}", path.display());

    if gnome::is_gnome_desktop() {
        gnome::install_gnome_custom_keybindings(&cfg)?;
        println!("Installed GNOME custom keybindings. Hotkeys now work without running daemon.");
    } else if let Some(comp) = crate::compositor::detect_compositor() {
        match comp.name() {
            "Hyprland" => {
                if let Ok(hypr_path) = wlroots::write_hyprland_hotkey_snippet(&cfg) {
                    println!("\n[Hyprland detected]");
                    println!("1. Saved bindings to: {}", hypr_path.display());
                    println!("2. Add this line to your hyprland.conf:");
                    println!("   source = {}", hypr_path.display());
                }
            }
            "Sway/i3" => {
                if let Ok(output) = wlroots::export_hotkeys_for_sway_config(&cfg.bindings) {
                    println!("\n[Sway/i3 detected]");
                    println!("Add these lines to your config file:\n");
                    println!("{}", output);
                }
            }
            "Niri" => {
                if let Ok(output) = wlroots::export_hotkeys_for_niri_config(&cfg.bindings) {
                    println!("\n[Niri detected]");
                    println!("Add these lines to your binds {{ ... }} block:\n");
                    println!("{}", output);
                }
            }
            "River" => {
                if let Ok(output) = wlroots::export_hotkeys_for_river_config(&cfg.bindings) {
                    println!("\n[River detected]");
                    println!("Add these lines to your river init script:\n");
                    println!("{}", output);
                }
            }
            "COSMIC" => {
                // COSMIC ships its own GlobalShortcuts portal backend;
                // shortcuts work through the daemon portal listener.
                println!("\n[COSMIC detected]");
                println!("Shortcuts will work through the daemon and xdg-desktop-portal-cosmic.");
                println!("If you don't see shortcut prompts, install xdg-desktop-portal-cosmic.");
            }
            _ => {
                println!(
                    "Config saved, but automatic installation for {} is not yet implemented.",
                    comp.name()
                );
            }
        }
    } else {
        println!("Config saved, but no-daemon install is currently GNOME-only.");
    }

    Ok(path)
}

pub(super) fn print_trigger_count(cfg: &config::HotkeyConfig) -> usize {
    cfg.bindings
        .iter()
        .filter(|binding| {
            config::as_portal_trigger(&binding.accelerator)
                .to_ascii_uppercase()
                .ends_with("PRINT")
        })
        .count()
}

pub(super) fn is_print_trigger(trigger: &str) -> bool {
    trigger.to_ascii_uppercase().ends_with("PRINT")
}

/// Default daemon entrypoint: prefer the portal GlobalShortcuts API (works on Wayland with consent),
/// and fall back to GNOME Shell if the portal is unavailable.
pub async fn run_hotkey_daemon(config_path: Option<PathBuf>) -> anyhow::Result<()> {
    run_hotkey_daemon_with_options(config_path, false, true).await
}

pub async fn run_hotkey_daemon_with_options(
    config_path: Option<PathBuf>,
    configure: bool,
    allow_desktop_relaunch: bool,
) -> anyhow::Result<()> {
    match portal::run_portal_hotkey_daemon(config_path.clone(), configure, allow_desktop_relaunch)
        .await
    {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("Portal hotkeys unavailable/failed:\n{e:#}");
            // On Wayland, GNOME Shell accelerator grabbing is typically forbidden, so falling back
            // just produces confusing AccessDenied errors.
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                return Err(e);
            }

            gnome::run_gnome_hotkey_daemon(config_path).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::config::*;
    use super::gnome::*;
    use super::kde::*;
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;

    fn sample_hotkey_config() -> HotkeyConfig {
        HotkeyConfig {
            bindings: vec![HotkeyBinding {
                accelerator: "CTRL+ALT+A".into(),
                args: vec!["capture".into(), "area".into()],
                name: Some("capture_area".into()),
            }],
        }
    }

    #[test]
    fn gnome_binding_snapshot_matches_expected_values() {
        let cfg = sample_hotkey_config();
        let expected = expected_gnome_bindings(&cfg, Path::new("/tmp/apexshot"));
        let binding = &expected[0];
        let snapshot = GnomeBindingSnapshot {
            paths: HashSet::from([binding.path.clone()]),
            commands: HashMap::from([(binding.path.clone(), binding.command_raw.clone())]),
            bindings: HashMap::from([(binding.path.clone(), binding.binding_raw.clone())]),
        };

        assert!(gnome_binding_issues_from_snapshot(&expected, &snapshot).is_empty());
    }

    #[test]
    fn gnome_binding_snapshot_detects_stale_command_path() {
        let cfg = sample_hotkey_config();
        let expected = expected_gnome_bindings(&cfg, Path::new("/new/location/apexshot"));
        let binding = &expected[0];
        let stale_command = gsettings_string(&gnome_binding_command(
            Path::new("/old/location/apexshot"),
            &cfg.bindings[0].args,
        ));
        let snapshot = GnomeBindingSnapshot {
            paths: HashSet::from([binding.path.clone()]),
            commands: HashMap::from([(binding.path.clone(), stale_command)]),
            bindings: HashMap::from([(binding.path.clone(), binding.binding_raw.clone())]),
        };

        let issues = gnome_binding_issues_from_snapshot(&expected, &snapshot);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("stale GNOME command"));
        assert!(issues[0].contains("/old/location/apexshot"));
    }

    #[test]
    fn default_hotkeys_include_recording_stop_binding() {
        let cfg = HotkeyConfig::default();
        let names = cfg
            .bindings
            .iter()
            .map(|binding| binding.name.clone().unwrap_or_default())
            .collect::<Vec<_>>();

        assert!(names.contains(&"recording_stop_save".to_string()));
        assert!(!names.contains(&"recording_pause_resume".to_string()));
        assert!(!names.contains(&"recording_restart".to_string()));
        assert!(!names.contains(&"recording_discard".to_string()));
    }

    #[test]
    fn default_hotkeys_include_crosshair_capture_binding() {
        let cfg = HotkeyConfig::default();
        let crosshair = cfg
            .bindings
            .iter()
            .find(|binding| binding.name.as_deref() == Some("capture_crosshair"))
            .expect("crosshair binding should exist");

        assert_eq!(crosshair.accelerator, "CTRL+ALT+X");
        assert_eq!(crosshair.args, vec!["capture", "crosshair"]);
    }

    #[test]
    fn default_hotkeys_expose_configurable_record_actions() {
        let cfg = crate::config::AppConfig::default();
        let hotkeys = hotkey_config_from_app_config(&cfg);

        // `record_screen` defaults to an empty accelerator, so it stays opt-in.
        assert!(!hotkeys
            .bindings
            .iter()
            .any(|binding| { binding.name.as_deref() == Some("record_screen") }));

        // `record_area`, `open_recording_ui`, and `show_last_preview` ship with
        // working defaults so the user can rebind them in the Shortcuts settings.
        assert!(hotkeys
            .bindings
            .iter()
            .any(|binding| { binding.name.as_deref() == Some("open_recording_ui") }));

        let show_last_preview = hotkeys
            .bindings
            .iter()
            .find(|binding| binding.name.as_deref() == Some("show_last_preview"))
            .expect("show_last_preview binding should exist by default");
        assert_eq!(show_last_preview.accelerator, "CTRL+ALT+P");
        assert_eq!(show_last_preview.args, vec!["show-last-preview"]);
    }

    #[test]
    fn app_config_can_expose_record_screen_separately_from_open_recording_ui() {
        let cfg = crate::config::AppConfig {
            shortcut_open_recording_ui: "Ctrl+Alt+R".into(),
            shortcut_record_screen: "Ctrl+Shift+R".into(),
            ..crate::config::AppConfig::default()
        };

        let hotkeys = hotkey_config_from_app_config(&cfg);

        assert!(hotkeys.bindings.iter().any(|binding| {
            binding.name.as_deref() == Some("open_recording_ui")
                && binding.accelerator == "CTRL+ALT+R"
        }));
        assert!(hotkeys.bindings.iter().any(|binding| {
            binding.name.as_deref() == Some("record_screen")
                && binding.accelerator == "CTRL+SHIFT+R"
                && binding.args == vec!["record", "screen", "--overlay-stop"]
        }));
    }

    #[test]
    fn app_config_shortcuts_map_to_runtime_hotkeys() {
        let cfg = crate::config::AppConfig {
            shortcut_open_file: "Ctrl+Alt+O".into(),
            shortcut_open_from_clipboard: "Ctrl+Alt+V".into(),
            shortcut_restore_recently_closed: "Ctrl+Alt+Z".into(),
            shortcut_toggle_overlays: "Ctrl+Alt+H".into(),
            shortcut_capture_area: "Shift+Super+4".into(),
            shortcut_capture_crosshair: "Ctrl+Alt+X".into(),
            shortcut_capture_fullscreen: "Shift+Super+3".into(),
            shortcut_capture_window: "Shift+Super+5".into(),
            shortcut_open_recording_ui: "Ctrl+Alt+R".into(),
            shortcut_recording_stop_save: "Ctrl+Alt+Shift+S".into(),
            ..crate::config::AppConfig::default()
        };

        let hotkeys = hotkey_config_from_app_config(&cfg);

        assert!(hotkeys.bindings.iter().any(|binding| {
            binding.name.as_deref() == Some("open_file")
                && binding.accelerator == "CTRL+ALT+O"
                && binding.args == vec!["open-file".to_string()]
        }));
        assert!(hotkeys.bindings.iter().any(|binding| {
            binding.name.as_deref() == Some("open_from_clipboard")
                && binding.accelerator == "CTRL+ALT+V"
                && binding.args == vec!["open-from-clipboard".to_string()]
        }));
        assert!(hotkeys.bindings.iter().any(|binding| {
            binding.name.as_deref() == Some("restore_recently_closed")
                && binding.accelerator == "CTRL+ALT+Z"
                && binding.args == vec!["restore-recently-closed".to_string()]
        }));
        assert!(hotkeys.bindings.iter().any(|binding| {
            binding.name.as_deref() == Some("toggle_overlays")
                && binding.accelerator == "CTRL+ALT+H"
                && binding.args == vec!["toggle-overlays".to_string()]
        }));
        assert!(hotkeys.bindings.iter().any(|binding| {
            binding.name.as_deref() == Some("open_recording_ui")
                && binding.accelerator == "CTRL+ALT+R"
                && binding.args == vec!["record".to_string(), "ui".to_string()]
        }));
    }

    #[test]
    fn blank_shortcuts_are_omitted_from_runtime_hotkeys() {
        let cfg = crate::config::AppConfig {
            shortcut_open_recording_ui: String::new(),
            ..crate::config::AppConfig::default()
        };

        let hotkeys = hotkey_config_from_app_config(&cfg);

        assert!(!hotkeys
            .bindings
            .iter()
            .any(|binding| binding.name.as_deref() == Some("open_recording_ui")));
    }

    #[test]
    fn kde_component_action_is_string_array_of_four() {
        // Must be Vec (D-Bus `as`), not a fixed array (which zbus encodes as struct).
        let binding = HotkeyBinding {
            accelerator: "CTRL+ALT+F".into(),
            args: vec!["capture".into(), "area".into()],
            name: Some("capture_area".into()),
        };
        let action = kde_component_action(&binding, 0);
        assert_eq!(action.len(), 4);
        assert_eq!(action[0], crate::app_identity::app_id());
        assert_eq!(action[1], "capture_area");
        assert_eq!(action[2], crate::app_identity::app_name());
        assert_eq!(action[3], "capture area");
    }
}
