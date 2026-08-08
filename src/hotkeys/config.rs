use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub(super) fn strip_deleted_suffix(path: &std::path::Path) -> PathBuf {
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

pub(super) fn as_portal_trigger(input: &str) -> String {
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

pub(super) fn as_gnome_accel(input: &str) -> String {
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

pub(super) fn default_hotkey_bindings() -> Vec<HotkeyBinding> {
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
            name: Some("recording_stop_save".into()),
            accelerator: "CTRL+ALT+SHIFT+S".into(),
            args: vec!["record".into(), "stop".into()],
        },
    ]
}

pub(super) fn merge_missing_default_hotkeys(cfg: &mut HotkeyConfig) -> bool {
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

pub(super) fn normalize_settings_accel(value: &str) -> String {
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
    // The stop shortcut remains available when the system tray is unavailable.
    push_binding(
        &mut bindings,
        "recording_stop_save",
        &app_config.shortcut_recording_stop_save,
        &["record", "stop"],
    );

    HotkeyConfig { bindings }
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

pub(super) fn default_config_path() -> PathBuf {
    let mut dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    dir.push("apexshot");
    dir.push("hotkeys.yml");
    dir
}

pub(super) fn load_or_create_config(
    path: Option<PathBuf>,
) -> anyhow::Result<(PathBuf, HotkeyConfig)> {
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

pub(super) fn save_hotkey_config(path: &Path, cfg: &HotkeyConfig) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config dir {}", parent.display()))?;
    }

    let raw = serde_yml::to_string(cfg).context("Failed to serialize hotkey config")?;
    std::fs::write(path, raw)
        .with_context(|| format!("Failed to write hotkey config to {}", path.display()))?;
    Ok(())
}
