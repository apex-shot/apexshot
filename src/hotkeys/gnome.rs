use anyhow::Context;
use futures_util::StreamExt;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use zbus::zvariant::OwnedValue;

pub(super) fn is_gnome_desktop() -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    desktop.to_ascii_lowercase().contains("gnome")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GnomeHotkeySyncResult {
    pub updated: bool,
    pub issues: Vec<String>,
}

pub(super) fn gsettings_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

pub(super) fn parse_gsettings_list(raw: &str) -> Vec<String> {
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

pub(super) fn format_gsettings_list(values: &[String]) -> String {
    let entries = values
        .iter()
        .map(|v| gsettings_string(v))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{entries}]")
}

pub(super) fn run_gsettings(args: &[String]) -> anyhow::Result<String> {
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

pub(super) fn gnome_custom_keybinding_paths() -> anyhow::Result<Vec<String>> {
    Ok(parse_gsettings_list(&run_gsettings(&[
        "get".into(),
        "org.gnome.settings-daemon.plugins.media-keys".into(),
        "custom-keybindings".into(),
    ])?))
}

pub(super) fn managed_gnome_path(binding: &super::config::HotkeyBinding, idx: usize) -> String {
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

pub(super) fn gnome_binding_command(exe: &Path, args: &[String]) -> String {
    std::iter::once(exe.to_string_lossy().to_string())
        .chain(args.iter().cloned())
        .map(|a| super::shell_quote(&a))
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExpectedGnomeBinding {
    pub(super) action_name: String,
    pub(super) path: String,
    pub(super) command_raw: String,
    pub(super) binding_raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct GnomeBindingSnapshot {
    pub(super) paths: HashSet<String>,
    pub(super) commands: HashMap<String, String>,
    pub(super) bindings: HashMap<String, String>,
}

pub(super) fn gnome_binding_schema(path: &str) -> String {
    format!("org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:{path}")
}

pub(super) fn gnome_binding_value(path: &str, key: &str) -> anyhow::Result<String> {
    run_gsettings(&["get".into(), gnome_binding_schema(path), key.into()])
}

pub(super) fn expected_gnome_bindings(
    cfg: &super::config::HotkeyConfig,
    action_exe: &Path,
) -> Vec<ExpectedGnomeBinding> {
    cfg.bindings
        .iter()
        .enumerate()
        .map(|(idx, binding)| ExpectedGnomeBinding {
            action_name: super::pretty_action_name(binding, idx),
            path: managed_gnome_path(binding, idx),
            command_raw: gsettings_string(&gnome_binding_command(action_exe, &binding.args)),
            binding_raw: gsettings_string(&super::config::as_gnome_accel(&binding.accelerator)),
        })
        .collect()
}

pub(super) fn load_gnome_binding_snapshot(
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

pub(super) fn gnome_binding_issues_from_snapshot(
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

pub(super) fn gnome_binding_issues(
    cfg: &super::config::HotkeyConfig,
) -> anyhow::Result<Vec<String>> {
    let action_exe = super::resolve_action_exe()?;
    let expected = expected_gnome_bindings(cfg, &action_exe);
    let snapshot = load_gnome_binding_snapshot(&expected)?;
    Ok(gnome_binding_issues_from_snapshot(&expected, &snapshot))
}

pub fn sync_gnome_hotkeys_for_current_desktop(
    config_path: Option<PathBuf>,
) -> anyhow::Result<GnomeHotkeySyncResult> {
    if crate::app_identity::portal_only() {
        return Ok(GnomeHotkeySyncResult {
            updated: false,
            issues: Vec::new(),
        });
    }
    let (_config_path, cfg) = super::config::load_or_create_config(config_path)?;
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

pub(super) fn install_gnome_custom_keybindings(
    cfg: &super::config::HotkeyConfig,
) -> anyhow::Result<()> {
    let existing = gnome_custom_keybinding_paths()?;
    let unmanaged = existing
        .into_iter()
        .filter(|p| !p.contains("/apexshot-") && !p.contains("/cleanshitx-"))
        .collect::<Vec<_>>();

    let action_exe = super::resolve_action_exe()?;
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
        let display_name = super::pretty_action_name(binding, idx);
        let command = gnome_binding_command(&action_exe, &binding.args);
        let accel = super::config::as_gnome_accel(&binding.accelerator);

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

pub(super) fn uninstall_gnome_custom_keybindings() -> anyhow::Result<()> {
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

pub async fn run_gnome_hotkey_daemon(config_path: Option<PathBuf>) -> anyhow::Result<()> {
    let (config_path, cfg) = super::config::load_or_create_config(config_path)?;
    if cfg.bindings.is_empty() {
        anyhow::bail!("No bindings configured in {}", config_path.display());
    }

    let _pid_guard = super::acquire_daemon_pid_guard()?;

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

    let mut action_to_binding: HashMap<u32, super::config::HotkeyBinding> = HashMap::new();

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
        .map(|b| {
            (
                super::config::as_gnome_accel(&b.accelerator),
                mode_flags,
                grab_flags,
            )
        })
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
                let accel = super::config::as_gnome_accel(&binding.accelerator);
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

        if let Err(e) = super::spawn_hotkey_action(None, &binding.args) {
            eprintln!(
                "Failed to spawn command for action {} ({:?}): {}",
                action_id, binding.args, e
            );
        }
    }
}
