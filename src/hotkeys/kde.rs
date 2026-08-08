pub(super) fn is_kde_desktop() -> bool {
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
pub(super) fn kde_component_action(
    binding: &super::config::HotkeyBinding,
    idx: usize,
) -> Vec<String> {
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

pub(super) fn kde_shortcut_keys_for_accel(accel: &str) -> Option<Vec<(Vec<i32>,)>> {
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

pub(super) fn sync_kde_hotkeys_if_applicable(
    cfg: &super::config::HotkeyConfig,
) -> anyhow::Result<()> {
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
