use anyhow::Context;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use zbus::zvariant::OwnedValue;
use zbus::zvariant::{OwnedObjectPath, Value};

fn portal_app_id() -> String {
    std::env::var("APEXSHOT_APP_ID").unwrap_or_else(|_| crate::app_identity::app_id().to_string())
}

fn desktop_exec_value() -> String {
    let exe = crate::app_identity::preferred_command_path()
        .to_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "apexshot".to_string());

    // Desktop entry Exec is not shell-parsed; spaces must be escaped per spec.
    let escaped_exe = exe.replace('\\', "\\\\").replace(' ', "\\ ");
    format!("{escaped_exe} daemon")
}

fn default_daemon_log_path() -> Option<PathBuf> {
    let mut dir = dirs::cache_dir()?;
    dir.push("apexshot");
    dir.push("hotkey-daemon.log");
    Some(dir)
}

fn open_daemon_log_if_needed() -> Option<(PathBuf, std::fs::File)> {
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

fn log_line(log: &mut Option<std::fs::File>, msg: &str) {
    eprintln!("{msg}");
    if let Some(file) = log.as_mut() {
        let _ = writeln!(file, "{msg}");
    }
}

fn hotkey_debug_enabled() -> bool {
    std::env::var_os("APEXSHOT_HOTKEY_DEBUG").is_some()
}

fn strip_deleted_suffix(path: &std::path::Path) -> PathBuf {
    // When a binary is replaced while running (e.g. `cargo run` rebuilds), /proc/self/exe can
    // resolve to a path ending with " (deleted)", which is not a real filesystem path.
    let Some(s) = path.to_str() else {
        return path.to_path_buf();
    };
    let Some(stripped) = s.strip_suffix(" (deleted)") else {
        return path.to_path_buf();
    };
    PathBuf::from(stripped)
}

fn resolve_action_exe() -> anyhow::Result<PathBuf> {
    let preferred = crate::app_identity::preferred_command_path();
    if preferred.exists() {
        return Ok(preferred);
    }

    if let Some(arg0) = std::env::args_os().next() {
        let p = strip_deleted_suffix(std::path::Path::new(&arg0));
        if p.is_absolute() && p.exists() {
            return Ok(p);
        }
        if let Ok(canon) = std::fs::canonicalize(&p) {
            return Ok(canon);
        }
    }

    let p = std::env::current_exe().context("Failed to get current executable")?;
    let cleaned = strip_deleted_suffix(&p);
    if cleaned.exists() {
        return Ok(cleaned);
    }
    Ok(p)
}

fn daemon_pid_file_path() -> anyhow::Result<PathBuf> {
    let mut dir =
        dirs::cache_dir().ok_or_else(|| anyhow::anyhow!("Failed to resolve cache dir"))?;
    dir.push("apexshot");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create daemon state dir {}", dir.display()))?;
    dir.push("hotkey-daemon.pid");
    Ok(dir)
}

fn is_pid_running(pid: u32) -> bool {
    PathBuf::from(format!("/proc/{pid}")).exists()
}

fn existing_daemon_pid() -> Option<u32> {
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

struct DaemonPidGuard {
    path: PathBuf,
}

impl Drop for DaemonPidGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn acquire_daemon_pid_guard() -> anyhow::Result<DaemonPidGuard> {
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

fn spawn_hotkey_action(
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
        candidates.push(strip_deleted_suffix(&exe));
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

fn ensure_desktop_entry(app_id: &str) -> anyhow::Result<PathBuf> {
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

fn is_gnome_desktop() -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    desktop.to_ascii_lowercase().contains("gnome")
}

fn apply_gio_desktop_launch_env(desktop_path: &PathBuf) {
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

fn try_relaunch_via_desktop(
    app_id: &str,
    config_path: &PathBuf,
    configure: bool,
) -> anyhow::Result<()> {
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

async fn register_portal_app_id(conn: &zbus::Connection, app_id: &str) -> anyhow::Result<()> {
    // For unsandboxed applications, portal implementations may require associating the DBus peer
    // with an app_id that matches a .desktop file basename.
    let registry = zbus::Proxy::new(
        conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.host.portal.Registry",
    )
    .await
    .context("Failed to create host Registry proxy")?;

    for attempt in 0..2 {
        let opts: HashMap<String, Value> = HashMap::new();
        let call: Result<(), zbus::Error> =
            registry.call("Register", &(app_id.to_string(), opts)).await;

        match call {
            Ok(()) => {
                eprintln!("Portal: registered app_id={}", app_id);
                return Ok(());
            }
            Err(e) if attempt == 0 && e.to_string().contains("App info not found") => {
                // Some portal backends may briefly fail to find a just-written desktop file.
                // Retry once after a short delay.
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Registry.Register failed for app_id={app_id}: {e}"
                ));
            }
        }
    }

    anyhow::bail!("Registry.Register failed for app_id={app_id}")
}

fn as_portal_trigger(input: &str) -> String {
    // Portal expects triggers in the XDG shortcuts spec format, e.g. CTRL+SHIFT+Print.
    // Accept legacy GNOME-style strings like <Ctrl><Shift>Print and convert.
    if input.contains('<') && input.contains('>') {
        let mut mods: Vec<String> = Vec::new();
        let mut rest = input;

        while let Some(stripped) = rest.strip_prefix('<') {
            let Some(end) = stripped.find('>') else {
                break;
            };
            let raw = &stripped[..end];
            let upper = raw.to_ascii_uppercase();
            let mapped = match upper.as_str() {
                "PRIMARY" => "CTRL",
                "CONTROL" | "CTRL" => "CTRL",
                "ALT" => "ALT",
                "SHIFT" => "SHIFT",
                "SUPER" => "SUPER",
                "META" => "META",
                _ => upper.as_str(),
            };
            mods.push(mapped.to_string());
            rest = &stripped[end + 1..];
        }

        let key = rest.trim();
        if mods.is_empty() {
            return key.to_string();
        }
        if key.is_empty() {
            return mods.join("+");
        }
        return format!("{}+{}", mods.join("+"), key);
    }

    // Already portal-style; normalize common modifier spellings.
    let parts: Vec<&str> = input.split('+').filter(|p| !p.is_empty()).collect();
    if parts.len() <= 1 {
        return input.trim().to_string();
    }
    let (mods, key) = parts.split_at(parts.len() - 1);
    let mods = mods
        .iter()
        .map(|m| m.trim().to_ascii_uppercase())
        .map(|m| match m.as_str() {
            "PRIMARY" => "CTRL".to_string(),
            "CONTROL" => "CTRL".to_string(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>();
    format!("{}+{}", mods.join("+"), key[0].trim())
}

fn as_gnome_accel(input: &str) -> String {
    // GNOME Shell expects accelerator strings like <Ctrl><Shift>Print.
    // Accept portal-style triggers like CTRL+SHIFT+Print and convert.
    if input.contains('<') && input.contains('>') {
        return input.trim().to_string();
    }

    let parts: Vec<&str> = input.split('+').filter(|p| !p.is_empty()).collect();
    if parts.len() <= 1 {
        return input.trim().to_string();
    }
    let (mods, key) = parts.split_at(parts.len() - 1);
    let mut out = String::new();
    for m in mods {
        let m = m.trim().to_ascii_uppercase();
        let tag = match m.as_str() {
            "CTRL" | "CONTROL" | "PRIMARY" => "<Ctrl>",
            "ALT" => "<Alt>",
            "SHIFT" => "<Shift>",
            "SUPER" => "<Super>",
            "META" => "<Meta>",
            _ => continue,
        };
        out.push_str(tag);
    }
    out.push_str(key[0].trim());
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyBinding {
    pub accelerator: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub bindings: Vec<HotkeyBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GnomeHotkeySyncResult {
    pub updated: bool,
    pub issues: Vec<String>,
}

fn default_hotkey_bindings() -> Vec<HotkeyBinding> {
    vec![
        HotkeyBinding {
            name: Some("capture_area".into()),
            accelerator: "CTRL+ALT+A".into(),
            args: vec!["capture".into(), "area".into()],
        },
        HotkeyBinding {
            name: Some("capture_crosshair".into()),
            accelerator: "CTRL+ALT+X".into(),
            args: vec!["capture".into(), "crosshair".into()],
        },
        HotkeyBinding {
            name: Some("capture_screen".into()),
            accelerator: "CTRL+ALT+S".into(),
            args: vec!["capture".into(), "screen".into()],
        },
        HotkeyBinding {
            name: Some("show_last_preview".into()),
            accelerator: "CTRL+ALT+P".into(),
            args: vec!["show-last-preview".into()],
        },
        HotkeyBinding {
            name: Some("record_screen".into()),
            accelerator: "CTRL+ALT+R".into(),
            args: vec!["record".into(), "screen".into(), "--overlay-stop".into()],
        },
        HotkeyBinding {
            name: Some("recording_pause_resume".into()),
            accelerator: "CTRL+ALT+SHIFT+P".into(),
            args: vec!["record".into(), "toggle-pause".into()],
        },
        HotkeyBinding {
            name: Some("recording_stop_save".into()),
            accelerator: "CTRL+ALT+SHIFT+S".into(),
            args: vec!["record".into(), "stop".into()],
        },
        HotkeyBinding {
            name: Some("recording_restart".into()),
            accelerator: "CTRL+ALT+SHIFT+N".into(),
            args: vec!["record".into(), "restart".into()],
        },
        HotkeyBinding {
            name: Some("recording_discard".into()),
            accelerator: "CTRL+ALT+SHIFT+BackSpace".into(),
            args: vec!["record".into(), "discard".into()],
        },
    ]
}

fn export_hotkeys_for_hyprland_config(bindings: &[HotkeyBinding]) -> anyhow::Result<String> {
    let exe = resolve_action_exe()?;
    let exe_str = exe.to_string_lossy();

    let mut output = String::new();
    output.push_str("# ApexShot Hotkeys for Hyprland\n");
    output.push_str("# Add these lines to your hyprland.conf\n\n");

    for binding in bindings {
        let parts: Vec<&str> = binding.accelerator.split('+').collect();
        let mut mods = Vec::new();
        let mut key = String::new();

        for part in parts {
            let upper = part.to_uppercase();
            match upper.as_str() {
                "CTRL" | "CONTROL" => mods.push("CTRL"),
                "ALT" => mods.push("ALT"),
                "SHIFT" => mods.push("SHIFT"),
                "SUPER" | "META" | "WIN" => mods.push("SUPER"),
                k => key = k.to_string(),
            }
        }

        let mods_joined = mods.join(" ");
        let mods_str = if mods.is_empty() { "" } else { &mods_joined };
        let name = binding.name.as_deref().unwrap_or("unknown");
        let args = binding.args.join(" ");

        output.push_str(&format!(
            "bind = {}, {}, exec, {} {} # {}\n",
            mods_str, key, exe_str, args, name
        ));
    }

    Ok(output)
}

pub fn export_hotkeys_for_hyprland() -> anyhow::Result<String> {
    export_hotkeys_for_hyprland_config(&default_hotkey_bindings())
}

pub fn export_configured_hotkeys_for_hyprland(
    config_path: Option<PathBuf>,
) -> anyhow::Result<String> {
    let (_path, cfg) = load_or_create_config(config_path)?;
    export_hotkeys_for_hyprland_config(&cfg.bindings)
}

fn export_hotkeys_for_sway_config(bindings: &[HotkeyBinding]) -> anyhow::Result<String> {
    let exe = resolve_action_exe()?;
    let exe_str = exe.to_string_lossy();

    let mut output = String::new();
    output.push_str("# ApexShot Hotkeys for Sway/i3\n");
    output.push_str("# Add these lines to your sway config (e.g. ~/.config/sway/config)\n\n");

    for binding in bindings {
        let name = binding.name.as_deref().unwrap_or("unknown");
        let args = binding.args.join(" ");

        output.push_str(&format!(
            "bindsym {} exec {} {} # {}\n",
            binding.accelerator, exe_str, args, name
        ));
    }

    Ok(output)
}

pub fn export_hotkeys_for_sway() -> anyhow::Result<String> {
    export_hotkeys_for_sway_config(&default_hotkey_bindings())
}

fn export_hotkeys_for_niri_config(bindings: &[HotkeyBinding]) -> anyhow::Result<String> {
    let exe = resolve_action_exe()?;
    let exe_str = exe.to_string_lossy();

    let mut output = String::new();
    output.push_str("// ApexShot Hotkeys for Niri\n");
    output.push_str("// Add these to your config.niri binds { ... } block\n\n");

    for binding in bindings {
        let parts: Vec<&str> = binding.accelerator.split('+').collect();
        let mut mods = Vec::new();
        let mut key = String::new();

        for part in parts {
            match part.to_uppercase().as_str() {
                "CTRL" | "CONTROL" => mods.push("Ctrl"),
                "ALT" => mods.push("Alt"),
                "SHIFT" => mods.push("Shift"),
                "SUPER" | "META" | "WIN" => mods.push("Super"),
                k => key = k.to_string(),
            }
        }

        let mods_str = if mods.is_empty() {
            "".to_string()
        } else {
            format!("{}+", mods.join("+"))
        };
        let name = binding.name.as_deref().unwrap_or("unknown");
        let args = binding.args.join(" ");

        output.push_str(&format!(
            "    \"{}{}\" {{ spawn \"{}\" \"{}\"; }} // {}\n",
            mods_str, key, exe_str, args, name
        ));
    }

    Ok(output)
}

pub fn export_hotkeys_for_niri() -> anyhow::Result<String> {
    export_hotkeys_for_niri_config(&default_hotkey_bindings())
}

fn export_hotkeys_for_river_config(bindings: &[HotkeyBinding]) -> anyhow::Result<String> {
    let exe = resolve_action_exe()?;
    let exe_str = exe.to_string_lossy();

    let mut output = String::new();
    output.push_str("# ApexShot Hotkeys for River\n");
    output.push_str("# Add these to your river init script\n\n");

    for binding in bindings {
        let parts: Vec<&str> = binding.accelerator.split('+').collect();
        let mut mods = Vec::new();
        let mut key = String::new();

        for part in parts {
            match part.to_uppercase().as_str() {
                "CTRL" | "CONTROL" => mods.push("Control"),
                "ALT" => mods.push("Alt"),
                "SHIFT" => mods.push("Shift"),
                "SUPER" | "META" | "WIN" => mods.push("Super"),
                k => key = k.to_string(),
            }
        }

        let mods_str = if mods.is_empty() {
            "None".to_string()
        } else {
            mods.join("+")
        };
        let name = binding.name.as_deref().unwrap_or("unknown");
        let args = binding.args.join(" ");

        output.push_str(&format!(
            "riverctl map normal {} {} spawn \"{} {}\" # {}\n",
            mods_str, key, exe_str, args, name
        ));
    }

    Ok(output)
}

pub fn export_hotkeys_for_river() -> anyhow::Result<String> {
    export_hotkeys_for_river_config(&default_hotkey_bindings())
}

fn merge_missing_default_hotkeys(cfg: &mut HotkeyConfig) -> bool {
    let mut changed = false;

    for default_binding in default_hotkey_bindings() {
        let already_present = cfg.bindings.iter().any(|binding| {
            binding.name == default_binding.name || binding.args == default_binding.args
        });

        if !already_present {
            cfg.bindings.push(default_binding);
            changed = true;
        }
    }

    changed
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        // Shortcut triggers are expressed using the XDG shortcuts specification
        // (e.g. CTRL+SHIFT+Print). The portal GlobalShortcuts API uses this format.
        Self {
            bindings: default_hotkey_bindings(),
        }
    }
}

/// Public wrapper so the daemon can load hotkey config without subprocess spawning.
pub fn load_hotkey_config(config_path: Option<PathBuf>) -> anyhow::Result<(PathBuf, HotkeyConfig)> {
    load_or_create_config(config_path)
}

/// Public wrapper so the daemon can convert accelerator strings to GNOME format.
pub fn accel_to_gnome(input: &str) -> String {
    as_gnome_accel(input)
}

/// Public wrapper so the daemon can convert accelerator strings to portal format.
pub fn accel_to_portal(input: &str) -> String {
    as_portal_trigger(input)
}

/// Public wrapper so the daemon can ensure the desktop entry exists and get its path.
/// Used to set GIO_LAUNCHED_DESKTOP_FILE so GNOME trusts the daemon process.
pub fn ensure_desktop_entry_pub(app_id: &str) -> anyhow::Result<std::path::PathBuf> {
    ensure_desktop_entry(app_id)
}

fn normalize_settings_accel(value: &str) -> String {
    as_portal_trigger(value)
}

pub fn hotkey_config_from_app_config(app_config: &crate::config::AppConfig) -> HotkeyConfig {
    let mut bindings = Vec::new();

    let push_binding =
        |bindings: &mut Vec<HotkeyBinding>, name: &str, accel: &str, args: &[&str]| {
            let trimmed = accel.trim();
            if trimmed.is_empty() {
                return;
            }
            bindings.push(HotkeyBinding {
                name: Some(name.to_string()),
                accelerator: normalize_settings_accel(trimmed),
                args: args.iter().map(|s| s.to_string()).collect(),
            });
        };

    push_binding(
        &mut bindings,
        "open_file",
        &app_config.shortcut_open_file,
        &["open-file"],
    );
    push_binding(
        &mut bindings,
        "open_from_clipboard",
        &app_config.shortcut_open_from_clipboard,
        &["open-from-clipboard"],
    );
    push_binding(
        &mut bindings,
        "restore_recently_closed",
        &app_config.shortcut_restore_recently_closed,
        &["restore-recently-closed"],
    );
    push_binding(
        &mut bindings,
        "toggle_overlays",
        &app_config.shortcut_toggle_overlays,
        &["toggle-overlays"],
    );
    push_binding(
        &mut bindings,
        "capture_area",
        &app_config.shortcut_capture_area,
        &["capture", "area"],
    );
    push_binding(
        &mut bindings,
        "capture_crosshair",
        &app_config.shortcut_capture_crosshair,
        &["capture", "crosshair"],
    );
    push_binding(
        &mut bindings,
        "capture_previous_area",
        &app_config.shortcut_capture_previous_area,
        &["capture", "previous-area"],
    );
    push_binding(
        &mut bindings,
        "capture_screen",
        &app_config.shortcut_capture_fullscreen,
        &["capture", "screen"],
    );
    // Window capture is temporarily discontinued — do not register the binding
    // even if a leftover config shortcut remains.
    let _ = &app_config.shortcut_capture_window;
    push_binding(
        &mut bindings,
        "show_last_preview",
        &app_config.shortcut_show_last_preview,
        &["show-last-preview"],
    );
    push_binding(
        &mut bindings,
        "open_recording_ui",
        &app_config.shortcut_open_recording_ui,
        &["record", "ui"],
    );
    push_binding(
        &mut bindings,
        "record_screen",
        &app_config.shortcut_record_screen,
        &["record", "screen", "--overlay-stop"],
    );
    // Recording controls — one binding each (names must match DaemonIpc::trigger).
    // CLI aliases `apexshot record stop|toggle-pause|restart|discard` also work.
    push_binding(
        &mut bindings,
        "recording_pause_resume",
        &app_config.shortcut_recording_pause_resume,
        &["record", "toggle-pause"],
    );
    push_binding(
        &mut bindings,
        "recording_stop_save",
        &app_config.shortcut_recording_stop_save,
        &["record", "stop"],
    );
    push_binding(
        &mut bindings,
        "recording_restart",
        &app_config.shortcut_recording_restart,
        &["record", "restart"],
    );
    push_binding(
        &mut bindings,
        "recording_discard",
        &app_config.shortcut_recording_discard,
        &["record", "discard"],
    );

    HotkeyConfig { bindings }
}

pub fn sync_hotkeys_from_app_config(app_config: &crate::config::AppConfig) -> anyhow::Result<()> {
    let path = default_config_path();
    let cfg = hotkey_config_from_app_config(app_config);
    save_hotkey_config(&path, &cfg)?;

    // For compositors that don't provide a GlobalShortcuts portal, generate
    // compositor-native bind-snippet files so the user's configured shortcuts
    // are picked up by the WM. The daemon portal listener is best-effort and
    // may fail on compositors without portal support.
    if let Some(comp) = crate::compositor::detect_compositor() {
        match comp.name() {
            "Hyprland" => {
                write_hyprland_hotkey_snippet(&cfg)?;
            }
            "Sway/i3" => {
                write_sway_hotkey_snippet(&cfg)?;
            }
            "Niri" => {
                write_niri_hotkey_snippet(&cfg)?;
            }
            "River" => {
                write_river_hotkey_snippet(&cfg)?;
            }
            _ => {}
        }
    }

    sync_kde_hotkeys_if_applicable(&cfg)?;

    Ok(())
}

fn is_kde_desktop() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains("kde")
        || std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains("plasma")
}

/// KGlobalAccel action id must be D-Bus type `as` (string array), not a struct.
/// Using `[String; 4]` made zbus emit `(ssss)`, which Plasma rejects with
/// UnknownMethod and a long signature error shown in Settings.
fn kde_component_action(binding: &HotkeyBinding, idx: usize) -> Vec<String> {
    let id = binding
        .name
        .clone()
        .unwrap_or_else(|| format!("binding_{idx}"));
    vec![
        crate::app_identity::app_id().to_string(),
        id.clone(),
        crate::app_identity::app_name().to_string(),
        id.replace('_', " "),
    ]
}

fn kde_shortcut_keys_for_accel(accel: &str) -> Option<Vec<(Vec<i32>,)>> {
    let parts: Vec<&str> = accel.split('+').filter(|p| !p.trim().is_empty()).collect();
    let key = parts.last()?.trim();

    let mut mods = 0i32;
    for part in &parts[..parts.len().saturating_sub(1)] {
        match part.trim().to_ascii_uppercase().as_str() {
            "SHIFT" => mods |= 0x0200_0000,
            "CTRL" | "CONTROL" | "PRIMARY" => mods |= 0x0400_0000,
            "ALT" => mods |= 0x0800_0000,
            "SUPER" | "META" => mods |= 0x1000_0000,
            _ => {}
        }
    }

    let key_code = match key.to_ascii_uppercase().as_str() {
        "BACKSPACE" => 0x0100_0003,
        "TAB" => 0x0100_0001,
        "RETURN" | "ENTER" => 0x0100_0004,
        "SPACE" => 0x20,
        "PRINT" => 0x0100_0009,
        other if other.len() == 1 => other.chars().next().map(|c| c as i32).unwrap_or(0),
        _ => return None,
    };

    Some(vec![(vec![mods | key_code, 0, 0, 0],)])
}

fn sync_kde_hotkeys_if_applicable(cfg: &HotkeyConfig) -> anyhow::Result<()> {
    if !is_kde_desktop() {
        return Ok(());
    }

    let bus = match zbus::blocking::Connection::session() {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("[hotkeys] KDE sync skipped: no session bus: {e}");
            return Ok(());
        }
    };

    let proxy = match zbus::blocking::Proxy::new(
        &bus,
        "org.kde.kglobalaccel",
        "/kglobalaccel",
        "org.kde.KGlobalAccel",
    ) {
        Ok(proxy) => proxy,
        Err(e) => {
            eprintln!("[hotkeys] KDE sync skipped: kglobalaccel unavailable: {e}");
            return Ok(());
        }
    };

    // Best-effort: never fail Settings save because one KDE shortcut could not
    // be registered. Config + hotkeys.yml are already written by the caller.
    for (idx, binding) in cfg.bindings.iter().enumerate() {
        let action = kde_component_action(binding, idx);
        let Some(keys) = kde_shortcut_keys_for_accel(&binding.accelerator) else {
            eprintln!(
                "[hotkeys] KDE sync skipped unsupported accelerator '{}': {:?}",
                binding.accelerator, binding.name
            );
            continue;
        };

        match proxy.call::<_, _, Vec<(Vec<i32>,)>>("setShortcutKeys", &(action, keys, 4u32)) {
            Ok(_) => {}
            Err(e) => {
                eprintln!(
                    "[hotkeys] KDE setShortcutKeys failed for {:?}: {e}",
                    binding.name
                );
            }
        }
    }

    Ok(())
}

fn hyprland_hotkey_snippet_path() -> PathBuf {
    let mut hypr_path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"));
    hypr_path.push("hypr");
    hypr_path.push("apexshot.conf");
    hypr_path
}

fn write_hyprland_hotkey_snippet(cfg: &HotkeyConfig) -> anyhow::Result<PathBuf> {
    let hypr_path = hyprland_hotkey_snippet_path();
    let output = export_hotkeys_for_hyprland_config(&cfg.bindings)?;
    if let Some(parent) = hypr_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create Hyprland config dir {}", parent.display())
        })?;
    }
    std::fs::write(&hypr_path, output).with_context(|| {
        format!(
            "Failed to write Hyprland hotkeys to {}",
            hypr_path.display()
        )
    })?;
    ensure_hyprland_sources_hotkey_snippet(&hypr_path)?;
    reload_hyprland_config();
    Ok(hypr_path)
}

fn hyprland_main_config_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"));
    path.push("hypr");
    path.push("hyprland.conf");
    path
}

fn ensure_hyprland_sources_hotkey_snippet(snippet_path: &Path) -> anyhow::Result<()> {
    let config_path = hyprland_main_config_path();
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create Hyprland config dir {}", parent.display())
        })?;
    }

    let source_line = format!("source = {}", snippet_path.display());
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == source_line) {
        return Ok(());
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str("\n# ApexShot shortcuts\n");
    updated.push_str(&source_line);
    updated.push('\n');

    std::fs::write(&config_path, updated).with_context(|| {
        format!(
            "Failed to add ApexShot source line to {}",
            config_path.display()
        )
    })?;
    Ok(())
}

fn reload_hyprland_config() {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return;
    }

    match std::process::Command::new("hyprctl").arg("reload").status() {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("[hotkeys] hyprctl reload exited with {status}"),
        Err(e) => eprintln!("[hotkeys] failed to run hyprctl reload: {e}"),
    }
}

// ── Sway hotkey snippet ──────────────────────────────────────────────────────

fn sway_hotkey_snippet_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"));
    path.push("sway");
    path.push("apexshot.conf");
    path
}

fn write_sway_hotkey_snippet(cfg: &HotkeyConfig) -> anyhow::Result<PathBuf> {
    let snippet_path = sway_hotkey_snippet_path();
    let output = export_hotkeys_for_sway_config(&cfg.bindings)?;
    if let Some(parent) = snippet_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create Sway config dir {}", parent.display()))?;
    }
    std::fs::write(&snippet_path, &output)
        .with_context(|| format!("Failed to write Sway hotkeys to {}", snippet_path.display()))?;
    eprintln!(
        "[hotkeys] Wrote Sway hotkey snippet to {} (add `include {}` to your sway config)",
        snippet_path.display(),
        snippet_path.display()
    );
    reload_sway_config();
    Ok(snippet_path)
}

fn reload_sway_config() {
    if std::env::var_os("SWAYSOCK").is_none() && std::env::var_os("I3SOCK").is_none() {
        return;
    }
    match std::process::Command::new("swaymsg").arg("reload").status() {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("[hotkeys] swaymsg reload exited with {status}"),
        Err(e) => eprintln!("[hotkeys] failed to run swaymsg reload: {e}"),
    }
}

// ── Niri hotkey snippet ──────────────────────────────────────────────────────

fn niri_hotkey_snippet_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"));
    path.push("niri");
    path.push("apexshot.niri");
    path
}

fn write_niri_hotkey_snippet(cfg: &HotkeyConfig) -> anyhow::Result<PathBuf> {
    let snippet_path = niri_hotkey_snippet_path();
    // Wrap in a binds { } block so it can be sourced standalone.
    let bind_lines = export_hotkeys_for_niri_config(&cfg.bindings)?;
    let output = format!(
        "// ApexShot hotkeys — source this from your config.niri\nbinds {{\n{bind_lines}}}\n"
    );
    if let Some(parent) = snippet_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create Niri config dir {}", parent.display()))?;
    }
    std::fs::write(&snippet_path, &output)
        .with_context(|| format!("Failed to write Niri hotkeys to {}", snippet_path.display()))?;
    eprintln!(
        "[hotkeys] Wrote Niri hotkey snippet to {}",
        snippet_path.display()
    );
    Ok(snippet_path)
}

// ── River hotkey snippet ─────────────────────────────────────────────────────

fn river_hotkey_snippet_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"));
    path.push("river");
    path.push("apexshot");
    path
}

fn write_river_hotkey_snippet(cfg: &HotkeyConfig) -> anyhow::Result<PathBuf> {
    let snippet_path = river_hotkey_snippet_path();
    let output = export_hotkeys_for_river_config(&cfg.bindings)?;
    if let Some(parent) = snippet_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create River config dir {}", parent.display()))?;
    }
    std::fs::write(&snippet_path, &output).with_context(|| {
        format!(
            "Failed to write River hotkeys to {}",
            snippet_path.display()
        )
    })?;
    eprintln!(
        "[hotkeys] Wrote River hotkey snippet to {} (source this from your river init script)",
        snippet_path.display()
    );
    Ok(snippet_path)
}

pub fn reset_hotkey_config(config_path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let path = config_path.unwrap_or_else(default_config_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config dir {}", parent.display()))?;
    }

    let cfg = HotkeyConfig::default();
    let raw = serde_yml::to_string(&cfg).context("Failed to serialize default hotkey config")?;
    std::fs::write(&path, raw)
        .with_context(|| format!("Failed to write hotkey config to {}", path.display()))?;
    Ok(path)
}

fn default_config_path() -> PathBuf {
    let mut dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    dir.push("apexshot");
    dir.push("hotkeys.yml");
    dir
}

fn load_or_create_config(path: Option<PathBuf>) -> anyhow::Result<(PathBuf, HotkeyConfig)> {
    let path = path.unwrap_or_else(default_config_path);

    if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read hotkey config at {}", path.display()))?;
        let mut cfg: HotkeyConfig = serde_yml::from_str(&raw)
            .with_context(|| format!("Failed to parse YAML hotkey config at {}", path.display()))?;
        if merge_missing_default_hotkeys(&mut cfg) {
            save_hotkey_config(&path, &cfg)?;
        }
        return Ok((path, cfg));
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config dir {}", parent.display()))?;
    }

    let cfg = HotkeyConfig::default();
    let raw = serde_yml::to_string(&cfg).context("Failed to serialize default hotkey config")?;
    std::fs::write(&path, raw).with_context(|| {
        format!(
            "Failed to write default hotkey config to {}",
            path.display()
        )
    })?;

    Ok((path, cfg))
}

fn save_hotkey_config(path: &Path, cfg: &HotkeyConfig) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config dir {}", parent.display()))?;
    }

    let raw = serde_yml::to_string(cfg).context("Failed to serialize hotkey config")?;
    std::fs::write(path, raw)
        .with_context(|| format!("Failed to write hotkey config to {}", path.display()))?;
    Ok(())
}

fn prompt_line(prompt: &str) -> anyhow::Result<String> {
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

fn pretty_action_name(binding: &HotkeyBinding, idx: usize) -> String {
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

fn shell_quote(arg: &str) -> String {
    let escaped = arg
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");
    format!("\"{escaped}\"")
}

fn gsettings_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn parse_gsettings_list(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;

    for ch in raw.chars() {
        if in_quote {
            if ch == '\'' {
                out.push(current.clone());
                current.clear();
                in_quote = false;
            } else {
                current.push(ch);
            }
            continue;
        }

        if ch == '\'' {
            in_quote = true;
        }
    }

    out
}

fn format_gsettings_list(values: &[String]) -> String {
    let entries = values
        .iter()
        .map(|v| gsettings_string(v))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{entries}]")
}

fn run_gsettings(args: &[String]) -> anyhow::Result<String> {
    let out = std::process::Command::new("gsettings")
        .args(args)
        .output()
        .with_context(|| format!("Failed to run gsettings with args: {:?}", args))?;

    if !out.status.success() {
        anyhow::bail!(
            "gsettings failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn gnome_custom_keybinding_paths() -> anyhow::Result<Vec<String>> {
    Ok(parse_gsettings_list(&run_gsettings(&[
        "get".into(),
        "org.gnome.settings-daemon.plugins.media-keys".into(),
        "custom-keybindings".into(),
    ])?))
}

fn managed_gnome_path(binding: &HotkeyBinding, idx: usize) -> String {
    let base = binding
        .name
        .clone()
        .unwrap_or_else(|| format!("binding_{idx}"))
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/apexshot-{base}/")
}

fn gnome_binding_command(exe: &Path, args: &[String]) -> String {
    std::iter::once(exe.to_string_lossy().to_string())
        .chain(args.iter().cloned())
        .map(|a| shell_quote(&a))
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExpectedGnomeBinding {
    action_name: String,
    path: String,
    command_raw: String,
    binding_raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct GnomeBindingSnapshot {
    paths: HashSet<String>,
    commands: HashMap<String, String>,
    bindings: HashMap<String, String>,
}

fn gnome_binding_schema(path: &str) -> String {
    format!("org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:{path}")
}

fn gnome_binding_value(path: &str, key: &str) -> anyhow::Result<String> {
    run_gsettings(&["get".into(), gnome_binding_schema(path), key.into()])
}

fn expected_gnome_bindings(cfg: &HotkeyConfig, action_exe: &Path) -> Vec<ExpectedGnomeBinding> {
    cfg.bindings
        .iter()
        .enumerate()
        .map(|(idx, binding)| ExpectedGnomeBinding {
            action_name: pretty_action_name(binding, idx),
            path: managed_gnome_path(binding, idx),
            command_raw: gsettings_string(&gnome_binding_command(action_exe, &binding.args)),
            binding_raw: gsettings_string(&as_gnome_accel(&binding.accelerator)),
        })
        .collect()
}

fn load_gnome_binding_snapshot(
    expected: &[ExpectedGnomeBinding],
) -> anyhow::Result<GnomeBindingSnapshot> {
    let paths = gnome_custom_keybinding_paths()?
        .into_iter()
        .collect::<HashSet<_>>();
    let mut snapshot = GnomeBindingSnapshot {
        paths,
        commands: HashMap::new(),
        bindings: HashMap::new(),
    };

    for binding in expected {
        if !snapshot.paths.contains(&binding.path) {
            continue;
        }
        snapshot.commands.insert(
            binding.path.clone(),
            gnome_binding_value(&binding.path, "command")?,
        );
        snapshot.bindings.insert(
            binding.path.clone(),
            gnome_binding_value(&binding.path, "binding")?,
        );
    }

    Ok(snapshot)
}

fn gnome_binding_issues_from_snapshot(
    expected: &[ExpectedGnomeBinding],
    snapshot: &GnomeBindingSnapshot,
) -> Vec<String> {
    let mut issues = Vec::new();

    for binding in expected {
        if !snapshot.paths.contains(&binding.path) {
            issues.push(format!(
                "{}: missing GNOME custom keybinding at {}",
                binding.action_name, binding.path
            ));
            continue;
        }

        match snapshot.commands.get(&binding.path) {
            Some(actual) if actual == &binding.command_raw => {}
            Some(actual) => issues.push(format!(
                "{}: stale GNOME command at {} (expected {}, found {})",
                binding.action_name, binding.path, binding.command_raw, actual
            )),
            None => issues.push(format!(
                "{}: could not read GNOME command at {}",
                binding.action_name, binding.path
            )),
        }

        match snapshot.bindings.get(&binding.path) {
            Some(actual) if actual == &binding.binding_raw => {}
            Some(actual) => issues.push(format!(
                "{}: stale GNOME accelerator at {} (expected {}, found {})",
                binding.action_name, binding.path, binding.binding_raw, actual
            )),
            None => issues.push(format!(
                "{}: could not read GNOME accelerator at {}",
                binding.action_name, binding.path
            )),
        }
    }

    issues
}

fn gnome_binding_issues(cfg: &HotkeyConfig) -> anyhow::Result<Vec<String>> {
    let action_exe = resolve_action_exe()?;
    let expected = expected_gnome_bindings(cfg, &action_exe);
    let snapshot = load_gnome_binding_snapshot(&expected)?;
    Ok(gnome_binding_issues_from_snapshot(&expected, &snapshot))
}

pub fn sync_gnome_hotkeys_for_current_desktop(
    config_path: Option<PathBuf>,
) -> anyhow::Result<GnomeHotkeySyncResult> {
    let (_config_path, cfg) = load_or_create_config(config_path)?;
    if cfg.bindings.is_empty() {
        return Ok(GnomeHotkeySyncResult {
            updated: false,
            issues: Vec::new(),
        });
    }

    let issues = gnome_binding_issues(&cfg)?;
    if issues.is_empty() {
        return Ok(GnomeHotkeySyncResult {
            updated: false,
            issues,
        });
    }

    install_gnome_custom_keybindings(&cfg)?;
    let remaining = gnome_binding_issues(&cfg)?;
    if !remaining.is_empty() {
        anyhow::bail!(
            "GNOME custom keybindings remain out of sync after reinstall: {}",
            remaining.join("; ")
        );
    }

    Ok(GnomeHotkeySyncResult {
        updated: true,
        issues,
    })
}

fn install_gnome_custom_keybindings(cfg: &HotkeyConfig) -> anyhow::Result<()> {
    let existing = gnome_custom_keybinding_paths()?;
    let unmanaged = existing
        .into_iter()
        .filter(|p| !p.contains("/apexshot-") && !p.contains("/cleanshitx-"))
        .collect::<Vec<_>>();

    let action_exe = resolve_action_exe()?;
    let managed_paths = cfg
        .bindings
        .iter()
        .enumerate()
        .map(|(idx, b)| managed_gnome_path(b, idx))
        .collect::<Vec<_>>();

    let merged = unmanaged
        .iter()
        .cloned()
        .chain(managed_paths.iter().cloned())
        .collect::<Vec<_>>();

    run_gsettings(&[
        "set".into(),
        "org.gnome.settings-daemon.plugins.media-keys".into(),
        "custom-keybindings".into(),
        format_gsettings_list(&merged),
    ])?;

    for (idx, binding) in cfg.bindings.iter().enumerate() {
        let path = managed_gnome_path(binding, idx);
        let schema = format!(
            "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:{}",
            path
        );
        let display_name = pretty_action_name(binding, idx);
        let command = gnome_binding_command(&action_exe, &binding.args);
        let accel = as_gnome_accel(&binding.accelerator);

        run_gsettings(&[
            "set".into(),
            schema.clone(),
            "name".into(),
            gsettings_string(&display_name),
        ])?;

        run_gsettings(&[
            "set".into(),
            schema.clone(),
            "command".into(),
            gsettings_string(&command),
        ])?;

        run_gsettings(&[
            "set".into(),
            schema,
            "binding".into(),
            gsettings_string(&accel),
        ])?;
    }

    Ok(())
}

fn uninstall_gnome_custom_keybindings() -> anyhow::Result<()> {
    let existing = gnome_custom_keybinding_paths()?;
    let unmanaged = existing
        .into_iter()
        .filter(|p| !p.contains("/apexshot-"))
        .collect::<Vec<_>>();

    run_gsettings(&[
        "set".into(),
        "org.gnome.settings-daemon.plugins.media-keys".into(),
        "custom-keybindings".into(),
        format_gsettings_list(&unmanaged),
    ])?;

    Ok(())
}

pub fn install_hotkeys_for_current_desktop(config_path: Option<PathBuf>) -> anyhow::Result<()> {
    let (_config_path, cfg) = load_or_create_config(config_path)?;

    if is_gnome_desktop() {
        install_gnome_custom_keybindings(&cfg)?;
        println!("Installed GNOME custom keybindings for ApexShot (no daemon required).");
        return Ok(());
    }

    anyhow::bail!(
        "No-daemon hotkey install is currently supported on GNOME only (current desktop: {}).",
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".into())
    )
}

pub fn uninstall_hotkeys_for_current_desktop() -> anyhow::Result<()> {
    if is_gnome_desktop() {
        uninstall_gnome_custom_keybindings()?;
        println!("Removed ApexShot GNOME custom keybindings.");
        return Ok(());
    }

    anyhow::bail!(
        "No-daemon hotkey uninstall is currently supported on GNOME only (current desktop: {}).",
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".into())
    )
}

pub fn setup_hotkeys_for_current_desktop(config_path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let (path, mut cfg) = load_or_create_config(config_path)?;

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
        binding.accelerator = as_portal_trigger(&entered);
    }

    cfg.bindings.retain(|b| !b.accelerator.trim().is_empty());

    if cfg.bindings.is_empty() {
        anyhow::bail!("No bindings left after setup; aborting to avoid disabling all shortcuts");
    }

    save_hotkey_config(&path, &cfg)?;
    println!("Saved hotkey config: {}", path.display());

    if is_gnome_desktop() {
        install_gnome_custom_keybindings(&cfg)?;
        println!("Installed GNOME custom keybindings. Hotkeys now work without running daemon.");
    } else if let Some(comp) = crate::compositor::detect_compositor() {
        match comp.name() {
            "Hyprland" => {
                if let Ok(hypr_path) = write_hyprland_hotkey_snippet(&cfg) {
                    println!("\n[Hyprland detected]");
                    println!("1. Saved bindings to: {}", hypr_path.display());
                    println!("2. Add this line to your hyprland.conf:");
                    println!("   source = {}", hypr_path.display());
                }
            }
            "Sway/i3" => {
                if let Ok(output) = export_hotkeys_for_sway_config(&cfg.bindings) {
                    println!("\n[Sway/i3 detected]");
                    println!("Add these lines to your config file:\n");
                    println!("{}", output);
                }
            }
            "Niri" => {
                if let Ok(output) = export_hotkeys_for_niri_config(&cfg.bindings) {
                    println!("\n[Niri detected]");
                    println!("Add these lines to your binds {{ ... }} block:\n");
                    println!("{}", output);
                }
            }
            "River" => {
                if let Ok(output) = export_hotkeys_for_river_config(&cfg.bindings) {
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

fn print_trigger_count(cfg: &HotkeyConfig) -> usize {
    cfg.bindings
        .iter()
        .filter(|binding| {
            as_portal_trigger(&binding.accelerator)
                .to_ascii_uppercase()
                .ends_with("PRINT")
        })
        .count()
}

fn is_print_trigger(trigger: &str) -> bool {
    trigger.to_ascii_uppercase().ends_with("PRINT")
}

pub async fn run_gnome_hotkey_daemon(config_path: Option<PathBuf>) -> anyhow::Result<()> {
    let (config_path, cfg) = load_or_create_config(config_path)?;
    if cfg.bindings.is_empty() {
        anyhow::bail!("No bindings configured in {}", config_path.display());
    }

    let _pid_guard = acquire_daemon_pid_guard()?;

    println!("Hotkey config: {}", config_path.display());

    let conn = zbus::Connection::session()
        .await
        .context("Failed to connect to session DBus")?;

    let shell = zbus::Proxy::new(
        &conn,
        "org.gnome.Shell",
        "/org/gnome/Shell",
        "org.gnome.Shell",
    )
    .await
    .context("Failed to create org.gnome.Shell proxy (are you on GNOME?)")?;

    let mut action_to_binding: HashMap<u32, HotkeyBinding> = HashMap::new();

    // GNOME Shell DBus API (Shell 49):
    // - GrabAccelerator(s accelerator, u modeFlags, u grabFlags) -> u action
    // - GrabAccelerators(a(suu) accelerators) -> au actions
    // modeFlags is a Shell.ActionMode bitmask; using 15 (ALL) is the most reliable.
    let mode_flags: u32 = 15;
    let grab_flags: u32 = 0;

    // Prefer batch, fallback to single if needed.
    let batch: Vec<(String, u32, u32)> = cfg
        .bindings
        .iter()
        .map(|b| (as_gnome_accel(&b.accelerator), mode_flags, grab_flags))
        .collect();
    let grabbed: Result<Vec<u32>, zbus::Error> = shell.call("GrabAccelerators", &(batch)).await;

    match grabbed {
        Ok(actions) => {
            if actions.len() != cfg.bindings.len() {
                eprintln!(
                    "Warning: GrabAccelerators returned {} actions for {} bindings",
                    actions.len(),
                    cfg.bindings.len()
                );
            }
            for (idx, action) in actions.into_iter().enumerate() {
                if let Some(binding) = cfg.bindings.get(idx) {
                    if action == 0 {
                        let name = binding.name.as_deref().unwrap_or("(unnamed)");
                        eprintln!(
                            "Warning: could not grab '{}' for {} (likely reserved by GNOME).",
                            binding.accelerator, name
                        );
                        continue;
                    }
                    action_to_binding.insert(action, binding.clone());
                }
            }
        }
        Err(_) => {
            for binding in &cfg.bindings {
                let name = binding.name.as_deref().unwrap_or("(unnamed)");
                let accel = as_gnome_accel(&binding.accelerator);
                let res: Result<u32, zbus::Error> = shell
                    .call("GrabAccelerator", &(accel, mode_flags, grab_flags))
                    .await;

                match res {
                    Ok(action) => {
                        if action == 0 {
                            eprintln!(
                                "Warning: could not grab '{}' for {} (likely reserved by GNOME).",
                                binding.accelerator, name
                            );
                            continue;
                        }
                        action_to_binding.insert(action, binding.clone());
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: failed to grab '{}' for {}: {}",
                            binding.accelerator, name, e
                        );
                    }
                }
            }
        }
    }

    if action_to_binding.is_empty() {
        anyhow::bail!(
            "No accelerators could be grabbed. Edit {} to choose different shortcuts (or disable conflicting GNOME shortcuts).",
            config_path.display()
        );
    }

    println!("Hotkey daemon running (GNOME Shell)");
    println!("Config: {}", config_path.display());
    for (action, binding) in &action_to_binding {
        let name = binding.name.as_deref().unwrap_or("(unnamed)");
        println!("  action {}: {} -> {:?}", action, name, binding.args);
    }

    let match_rule = "type='signal',interface='org.gnome.Shell',member='AcceleratorActivated',path='/org/gnome/Shell'";
    let rule: zbus::MatchRule = match_rule
        .try_into()
        .context("Failed to build DBus match rule")?;

    let mut stream = zbus::MessageStream::for_match_rule(rule, &conn, None)
        .await
        .context("Failed to subscribe to AcceleratorActivated")?;

    loop {
        let msg = match stream.next().await {
            Some(Ok(m)) => m,
            Some(Err(e)) => return Err(anyhow::anyhow!("DBus stream error: {e}")),
            None => return Err(anyhow::anyhow!("DBus stream ended")),
        };

        // Signature: (u action, a{sv} parameters)
        let action_id = match msg
            .body()
            .deserialize::<(u32, HashMap<String, OwnedValue>)>()
        {
            Ok((action, _params)) => action,
            Err(_) => continue,
        };

        let Some(binding) = action_to_binding.get(&action_id).cloned() else {
            continue;
        };

        if let Err(e) = spawn_hotkey_action(None, &binding.args) {
            eprintln!(
                "Failed to spawn command for action {} ({:?}): {}",
                action_id, binding.args, e
            );
        }
    }
}

fn token(prefix: &str) -> String {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // xdg-desktop-portal expects a restricted token charset (commonly [A-Za-z0-9_]).
    let prefix = prefix
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>();
    format!("{prefix}_{pid}_{nanos}")
}

fn portal_sender_id(connection: &zbus::Connection) -> anyhow::Result<String> {
    let unique = connection
        .unique_name()
        .ok_or_else(|| anyhow::anyhow!("DBus connection has no unique name"))?
        .as_str();

    Ok(unique.trim_start_matches(':').replace('.', "_"))
}

fn portal_request_path(sender_id: &str, token: &str) -> anyhow::Result<OwnedObjectPath> {
    let path = format!("/org/freedesktop/portal/desktop/request/{sender_id}/{token}");
    path.try_into().context("Invalid portal request path")
}

async fn portal_response_stream(
    connection: &zbus::Connection,
    request_path: &OwnedObjectPath,
) -> anyhow::Result<zbus::MessageStream> {
    let match_rule = format!(
        "type='signal',interface='org.freedesktop.portal.Request',member='Response',path='{}'",
        request_path.as_str()
    );

    let rule: zbus::MatchRule = match_rule
        .as_str()
        .try_into()
        .context("Failed to build portal match rule")?;

    zbus::MessageStream::for_match_rule(rule, connection, Some(1))
        .await
        .context("Failed to create portal response stream")
}

async fn read_portal_response(
    stream: &mut zbus::MessageStream,
) -> anyhow::Result<(u32, HashMap<String, OwnedValue>)> {
    let message = stream
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("No response from portal"))?
        .context("Portal response stream error")?;

    let (status, results): (u32, HashMap<String, OwnedValue>) = message
        .body()
        .deserialize()
        .context("Failed to deserialize portal response")?;

    Ok((status, results))
}

pub async fn run_portal_hotkey_daemon(
    config_path: Option<PathBuf>,
    configure: bool,
    allow_desktop_relaunch: bool,
) -> anyhow::Result<()> {
    let mut log = None;
    let log_path = open_daemon_log_if_needed().map(|(p, f)| {
        log = Some(f);
        p
    });
    if let Some(p) = &log_path {
        log_line(&mut log, &format!("Hotkey daemon log: {}", p.display()));
    }

    let (config_path, cfg) = load_or_create_config(config_path)?;
    if cfg.bindings.is_empty() {
        anyhow::bail!("No bindings configured in {}", config_path.display());
    }

    log_line(
        &mut log,
        &format!("Hotkey config: {}", config_path.display()),
    );

    if let Some(pid) = existing_daemon_pid() {
        let msg = format!(
            "Hotkey daemon already running (pid {pid}). Stop it first (e.g. `pkill -f \"apexshot daemon\"`) and retry"
        );
        log_line(&mut log, &msg);
        anyhow::bail!(msg);
    }

    // Ensure the portal can associate us with an application id.
    let app_id = portal_app_id();
    let desktop_path = ensure_desktop_entry(&app_id)?;
    log_line(
        &mut log,
        &format!(
            "Portal: using app_id={} (desktop: {})",
            app_id,
            desktop_path.display()
        ),
    );

    // On GNOME, GlobalShortcuts portal activations are often not delivered if the app is
    // launched from a terminal. Prefer relaunching via the .desktop entry.
    let terminal_launch = std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE").is_none();
    if is_gnome_desktop() && terminal_launch && !allow_desktop_relaunch {
        log_line(
            &mut log,
            &format!(
                "GNOME detected: this daemon was launched from a terminal; GlobalShortcuts activations often won't be delivered in this mode. Run without --no-desktop-relaunch (recommended) or start via the desktop entry (e.g. `gtk-launch {}`).",
                app_id
            ),
        );
    }
    if is_gnome_desktop() && terminal_launch && allow_desktop_relaunch {
        match try_relaunch_via_desktop(&app_id, &config_path, configure) {
            Ok(()) => {
                log_line(
                    &mut log,
                    "GNOME detected: relaunched hotkey daemon via desktop entry for reliable global shortcuts; exiting this terminal-started process.",
                );
                let follow_path = std::env::var_os("APEXSHOT_HOTKEY_LOG")
                    .map(PathBuf::from)
                    .or_else(default_daemon_log_path)
                    .or(log_path);
                if let Some(p) = follow_path {
                    log_line(
                        &mut log,
                        &format!("Follow logs with: tail -f {}", p.display()),
                    );
                }
                return Ok(());
            }
            Err(e) => {
                log_line(
                    &mut log,
                    &format!("GNOME detected but desktop relaunch failed (continuing anyway): {e}"),
                );
            }
        }
    }

    if is_gnome_desktop() {
        apply_gio_desktop_launch_env(&desktop_path);
    }

    let _pid_guard = match acquire_daemon_pid_guard() {
        Ok(guard) => guard,
        Err(e) => {
            log_line(&mut log, &format!("{e}"));
            return Err(e);
        }
    };

    let conn = zbus::Connection::session()
        .await
        .context("Failed to connect to session DBus")?;
    if let Err(e) = register_portal_app_id(&conn, &app_id).await {
        log_line(
            &mut log,
            &format!("Portal: Registry.Register failed (continuing): {e}"),
        );
    }

    let portal = zbus::Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.GlobalShortcuts",
    )
    .await
    .context("Failed to create GlobalShortcuts portal proxy")?;

    let portal_version: u32 = portal.get_property("version").await.unwrap_or(1);
    log_line(
        &mut log,
        &format!("Portal: GlobalShortcuts version={portal_version}"),
    );

    let sender_id = portal_sender_id(&conn)?;

    // Create session
    let create_token = token("apexshot_hk");
    let session_token = token("apexshot_hk_session");
    let mut create_opts: HashMap<String, Value> = HashMap::new();
    create_opts.insert("handle_token".into(), Value::from(create_token.clone()));
    create_opts.insert("session_handle_token".into(), Value::from(session_token));

    // Avoid a race where the portal answers before we subscribe to Request::Response.
    let expected_create_request_path = portal_request_path(&sender_id, &create_token)?;
    let mut create_stream = portal_response_stream(&conn, &expected_create_request_path).await?;

    log_line(&mut log, "Portal: calling CreateSession…");

    let request_path: OwnedObjectPath = portal
        .call("CreateSession", &(create_opts))
        .await
        .context("GlobalShortcuts.CreateSession call failed")?;

    if request_path != expected_create_request_path {
        eprintln!(
            "Portal: CreateSession returned unexpected request path {} (expected {})",
            request_path.as_str(),
            expected_create_request_path.as_str()
        );
    }

    log_line(
        &mut log,
        "Portal: waiting for CreateSession response… (approve any prompt)",
    );

    let (create_status, results) = read_portal_response(&mut create_stream)
        .await
        .context("CreateSession failed")?;

    if create_status != 0 {
        anyhow::bail!("CreateSession ended with response={create_status}");
    }

    // The portal returns session_handle as a string-typed variant containing an object path.
    let session_handle_str: String = results
        .get("session_handle")
        .ok_or_else(|| anyhow::anyhow!("Portal response missing session_handle"))?
        .try_clone()
        .context("Failed to clone session_handle")?
        .try_into()
        .context("Invalid session_handle value")?;

    let session_handle: OwnedObjectPath = session_handle_str
        .try_into()
        .context("Invalid session_handle object path")?;

    // Bind shortcuts (will typically prompt user once)
    let mut id_to_binding: HashMap<String, HotkeyBinding> = HashMap::new();
    let mut shortcuts: Vec<(String, HashMap<String, Value>)> = Vec::new();

    for (idx, binding) in cfg.bindings.iter().enumerate() {
        let id = binding
            .name
            .clone()
            .unwrap_or_else(|| format!("binding_{idx}"));

        let preferred_trigger = as_portal_trigger(&binding.accelerator);

        let mut props: HashMap<String, Value> = HashMap::new();
        props.insert(
            "description".into(),
            Value::from(binding.name.clone().unwrap_or_else(|| id.clone())),
        );

        if is_print_trigger(&preferred_trigger) {
            eprintln!(
                "Portal: '{}' uses Print-based trigger '{}' (often reserved). Omitting preferred trigger so you can assign a key in the portal dialog.",
                id, preferred_trigger
            );
        } else {
            props.insert("preferred_trigger".into(), Value::from(preferred_trigger));
        }

        shortcuts.push((id.clone(), props));
        id_to_binding.insert(id, binding.clone());
    }

    log_line(
        &mut log,
        "Requesting global shortcuts via portal… (you may get a prompt)",
    );

    let bind_token = token("apexshot_hk_bind");
    let mut bind_opts: HashMap<String, Value> = HashMap::new();
    bind_opts.insert("handle_token".into(), Value::from(bind_token.clone()));

    let expected_bind_request_path = portal_request_path(&sender_id, &bind_token)?;
    let mut bind_stream = portal_response_stream(&conn, &expected_bind_request_path).await?;

    let bind_request: OwnedObjectPath = portal
        .call(
            "BindShortcuts",
            &(session_handle.clone(), shortcuts, "".to_string(), bind_opts),
        )
        .await
        .context("GlobalShortcuts.BindShortcuts call failed")?;

    if bind_request != expected_bind_request_path {
        eprintln!(
            "Portal: BindShortcuts returned unexpected request path {} (expected {})",
            bind_request.as_str(),
            expected_bind_request_path.as_str()
        );
    }

    log_line(
        &mut log,
        "Portal: waiting for BindShortcuts response… (set/confirm shortcuts in the dialog)",
    );

    let (bind_status, bind_results) = read_portal_response(&mut bind_stream)
        .await
        .context("BindShortcuts failed")?;

    match bind_status {
        0 => {}
        1 => anyhow::bail!("BindShortcuts ended with response=1 (user cancelled)"),
        2 => {
            let print_triggers = print_trigger_count(&cfg);
            if print_triggers > 0 {
                anyhow::bail!(
                    "BindShortcuts ended with response=2. {print_triggers} configured shortcut(s) use Print-based triggers, which are often reserved by the desktop and rejected by the portal. Edit {} to use non-Print shortcuts, then run the daemon again",
                    config_path.display()
                );
            }

            anyhow::bail!(
                "BindShortcuts ended with response=2 (portal backend rejected the request unexpectedly)"
            );
        }
        other => anyhow::bail!("BindShortcuts ended with response={other}"),
    }

    // If the portal didn't bind any shortcuts, there's nothing to listen for.
    // BindShortcuts returns the subset of shortcut ids that were actually bound.
    if let Some(bound_value) = bind_results.get("shortcuts") {
        let bound: Option<Vec<(String, HashMap<String, OwnedValue>)>> =
            bound_value.try_clone().ok().and_then(|v| v.try_into().ok());

        if let Some(bound) = bound {
            if bound.is_empty() {
                anyhow::bail!(
                    "BindShortcuts did not bind any shortcuts. This usually means the portal backend rejected the triggers (often due to conflicts/reserved keys like Print). Try assigning a different key combo in the dialog (e.g. CTRL+ALT+P) or edit {}",
                    config_path.display()
                );
            }
        }
    }

    // Show what triggers the portal actually configured.
    let list_token = token("apexshot_hk_list");
    let mut list_opts: HashMap<String, Value> = HashMap::new();
    list_opts.insert("handle_token".into(), Value::from(list_token.clone()));

    let expected_list_request_path = portal_request_path(&sender_id, &list_token)?;
    let mut list_stream = portal_response_stream(&conn, &expected_list_request_path).await?;
    let list_request: OwnedObjectPath = portal
        .call("ListShortcuts", &(session_handle.clone(), list_opts))
        .await
        .context("GlobalShortcuts.ListShortcuts call failed")?;

    if list_request != expected_list_request_path {
        eprintln!(
            "Portal: ListShortcuts returned unexpected request path {} (expected {})",
            list_request.as_str(),
            expected_list_request_path.as_str()
        );
    }

    if let Ok((list_status, list_results)) = read_portal_response(&mut list_stream).await {
        if list_status != 0 {
            eprintln!("Portal: ListShortcuts ended with response={list_status}");
        }
        if let Some(shortcuts_value) = list_results.get("shortcuts") {
            let parsed: Result<Vec<(String, HashMap<String, OwnedValue>)>, _> = shortcuts_value
                .try_clone()
                .ok()
                .and_then(|v| v.try_into().ok())
                .ok_or_else(|| anyhow::anyhow!("Invalid shortcuts list"));

            if let Ok(shortcuts) = parsed {
                log_line(&mut log, "Configured shortcuts (portal):");
                for (id, props) in shortcuts {
                    let trigger_desc: Option<String> = props
                        .get("trigger_description")
                        .and_then(|v| v.try_clone().ok())
                        .and_then(|v| v.try_into().ok());
                    let preferred: Option<String> = props
                        .get("preferred_trigger")
                        .and_then(|v| v.try_clone().ok())
                        .and_then(|v| v.try_into().ok());
                    let desc: Option<String> = props
                        .get("description")
                        .and_then(|v| v.try_clone().ok())
                        .and_then(|v| v.try_into().ok());

                    log_line(
                        &mut log,
                        &format!(
                            "  {}: {} | preferred={:?} | trigger={:?}",
                            id,
                            desc.as_deref().unwrap_or(""),
                            preferred,
                            trigger_desc
                        ),
                    );
                }
            }
        }
    }

    log_line(&mut log, "Hotkey daemon running (portal GlobalShortcuts)");
    log_line(&mut log, &format!("Config: {}", config_path.display()));
    for (id, binding) in &id_to_binding {
        let name = binding.name.as_deref().unwrap_or("(unnamed)");
        log_line(
            &mut log,
            &format!("  {}: {} -> {:?}", id, name, binding.args),
        );
    }

    if configure {
        if portal_version >= 2 {
            let opts: HashMap<String, Value> = HashMap::new();
            let call: Result<(), zbus::Error> = portal
                .call(
                    "ConfigureShortcuts",
                    &(session_handle.clone(), "".to_string(), opts),
                )
                .await;

            match call {
                Ok(()) => log_line(&mut log, "Portal: opened shortcut configuration UI"),
                Err(e) => log_line(
                    &mut log,
                    &format!(
                        "Portal: ConfigureShortcuts failed (continuing without forcing UI): {e}"
                    ),
                ),
            }
        } else {
            log_line(
                &mut log,
                "Portal: ConfigureShortcuts is not supported by this portal backend (version < 2). Use the BindShortcuts dialog (if it appears) or system settings to edit shortcuts."
            );
        }
    }

    let action_exe = resolve_action_exe()?;
    log_line(
        &mut log,
        &format!("Hotkey actions will spawn: {}", action_exe.display()),
    );

    // Listen for activations.
    // Different portal backends may emit signals on different object paths, so don't
    // restrict the match rule by path; we filter by session_handle in the payload.
    let debug = hotkey_debug_enabled();
    if debug {
        log_line(&mut log, "Hotkey debug: enabled");
    }
    let match_rule = if debug {
        "type='signal',interface='org.freedesktop.portal.GlobalShortcuts'"
    } else {
        "type='signal',interface='org.freedesktop.portal.GlobalShortcuts',member='Activated'"
    };
    let rule: zbus::MatchRule = match_rule
        .try_into()
        .context("Failed to build GlobalShortcuts match rule")?;

    let mut stream = zbus::MessageStream::for_match_rule(rule, &conn, None)
        .await
        .context("Failed to subscribe to GlobalShortcuts.Activated")?;

    loop {
        let msg = match stream.next().await {
            Some(Ok(m)) => m,
            Some(Err(e)) => return Err(anyhow::anyhow!("DBus stream error: {e}")),
            None => return Err(anyhow::anyhow!("DBus stream ended")),
        };

        let parsed: Result<(OwnedObjectPath, String, u64, HashMap<String, OwnedValue>), _> =
            msg.body().deserialize();

        let (sess, shortcut_id, _ts, _opts) = match parsed {
            Ok(v) => v,
            Err(e) => {
                if debug {
                    log_line(&mut log, &format!("Hotkey debug: received non-Activated or unexpected GlobalShortcuts signal: {e}"));
                    log_line(&mut log, &format!("Hotkey debug: raw message: {msg:?}"));
                }
                continue;
            }
        };

        if sess != session_handle {
            eprintln!(
                "Ignoring activation for other session {} (expected {})",
                sess.as_str(),
                session_handle.as_str()
            );
            continue;
        }

        let Some(binding) = id_to_binding.get(&shortcut_id).cloned() else {
            eprintln!("Activated unknown shortcut id: {}", shortcut_id);
            continue;
        };

        log_line(&mut log, &format!("Activated shortcut: {}", shortcut_id));

        match spawn_hotkey_action(Some(&action_exe), &binding.args) {
            Ok((child, used_exe)) => {
                log_line(
                    &mut log,
                    &format!(
                        "Spawned: pid={} exe={} args={:?}",
                        child.id(),
                        used_exe.display(),
                        binding.args
                    ),
                );
            }
            Err(e) => {
                log_line(
                    &mut log,
                    &format!(
                        "Failed to spawn command for shortcut {} ({:?}): {}",
                        shortcut_id, binding.args, e
                    ),
                );
            }
        }
    }
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
    match run_portal_hotkey_daemon(config_path.clone(), configure, allow_desktop_relaunch).await {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("Portal hotkeys unavailable/failed:\n{e:#}");
            // On Wayland, GNOME Shell accelerator grabbing is typically forbidden, so falling back
            // just produces confusing AccessDenied errors.
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                return Err(e);
            }

            run_gnome_hotkey_daemon(config_path).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn default_hotkeys_include_recording_control_bindings() {
        let cfg = HotkeyConfig::default();
        let names = cfg
            .bindings
            .iter()
            .map(|binding| binding.name.clone().unwrap_or_default())
            .collect::<Vec<_>>();

        assert!(names.contains(&"recording_pause_resume".to_string()));
        assert!(names.contains(&"recording_stop_save".to_string()));
        assert!(names.contains(&"recording_restart".to_string()));
        assert!(names.contains(&"recording_discard".to_string()));
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
            shortcut_recording_pause_resume: "Ctrl+Alt+Shift+P".into(),
            shortcut_recording_stop_save: "Ctrl+Alt+Shift+S".into(),
            shortcut_recording_restart: "Ctrl+Alt+Shift+N".into(),
            shortcut_recording_discard: "Ctrl+Alt+Shift+BackSpace".into(),
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
            shortcut_recording_restart: String::new(),
            ..crate::config::AppConfig::default()
        };

        let hotkeys = hotkey_config_from_app_config(&cfg);

        assert!(!hotkeys
            .bindings
            .iter()
            .any(|binding| binding.name.as_deref() == Some("open_recording_ui")));
        assert!(!hotkeys
            .bindings
            .iter()
            .any(|binding| binding.name.as_deref() == Some("recording_restart")));
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
