use anyhow::Context;
use std::path::{Path, PathBuf};

pub(super) fn export_hotkeys_for_hyprland_config(
    bindings: &[super::config::HotkeyBinding],
) -> anyhow::Result<String> {
    let exe = super::resolve_action_exe()?;
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
    export_hotkeys_for_hyprland_config(&super::config::default_hotkey_bindings())
}

pub fn export_configured_hotkeys_for_hyprland(
    config_path: Option<PathBuf>,
) -> anyhow::Result<String> {
    let (_path, cfg) = super::config::load_or_create_config(config_path)?;
    export_hotkeys_for_hyprland_config(&cfg.bindings)
}

pub(super) fn export_hotkeys_for_sway_config(
    bindings: &[super::config::HotkeyBinding],
) -> anyhow::Result<String> {
    let exe = super::resolve_action_exe()?;
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
    export_hotkeys_for_sway_config(&super::config::default_hotkey_bindings())
}

pub(super) fn export_hotkeys_for_niri_config(
    bindings: &[super::config::HotkeyBinding],
) -> anyhow::Result<String> {
    let exe = super::resolve_action_exe()?;
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
    export_hotkeys_for_niri_config(&super::config::default_hotkey_bindings())
}

pub(super) fn export_hotkeys_for_river_config(
    bindings: &[super::config::HotkeyBinding],
) -> anyhow::Result<String> {
    let exe = super::resolve_action_exe()?;
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
    export_hotkeys_for_river_config(&super::config::default_hotkey_bindings())
}

pub(super) fn hyprland_hotkey_snippet_path() -> PathBuf {
    let mut hypr_path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"));
    hypr_path.push("hypr");
    hypr_path.push("apexshot.conf");
    hypr_path
}

pub(super) fn write_hyprland_hotkey_snippet(
    cfg: &super::config::HotkeyConfig,
) -> anyhow::Result<PathBuf> {
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

pub(super) fn hyprland_main_config_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"));
    path.push("hypr");
    path.push("hyprland.conf");
    path
}

pub(super) fn ensure_hyprland_sources_hotkey_snippet(snippet_path: &Path) -> anyhow::Result<()> {
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

pub(super) fn reload_hyprland_config() {
    if crate::app_identity::portal_only() {
        return;
    }
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return;
    }

    match std::process::Command::new("hyprctl").arg("reload").status() {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("[hotkeys] hyprctl reload exited with {status}"),
        Err(e) => eprintln!("[hotkeys] failed to run hyprctl reload: {e}"),
    }
}

pub(super) fn sway_hotkey_snippet_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"));
    path.push("sway");
    path.push("apexshot.conf");
    path
}

pub(super) fn write_sway_hotkey_snippet(
    cfg: &super::config::HotkeyConfig,
) -> anyhow::Result<PathBuf> {
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

pub(super) fn reload_sway_config() {
    if crate::app_identity::portal_only() {
        return;
    }
    if std::env::var_os("SWAYSOCK").is_none() && std::env::var_os("I3SOCK").is_none() {
        return;
    }
    match std::process::Command::new("swaymsg").arg("reload").status() {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("[hotkeys] swaymsg reload exited with {status}"),
        Err(e) => eprintln!("[hotkeys] failed to run swaymsg reload: {e}"),
    }
}

pub(super) fn niri_hotkey_snippet_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"));
    path.push("niri");
    path.push("apexshot.niri");
    path
}

pub(super) fn write_niri_hotkey_snippet(
    cfg: &super::config::HotkeyConfig,
) -> anyhow::Result<PathBuf> {
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

pub(super) fn river_hotkey_snippet_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"));
    path.push("river");
    path.push("apexshot");
    path
}

pub(super) fn write_river_hotkey_snippet(
    cfg: &super::config::HotkeyConfig,
) -> anyhow::Result<PathBuf> {
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
