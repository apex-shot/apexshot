use crate::hotkeys::{accel_to_gnome, ensure_desktop_entry_pub, load_hotkey_config, HotkeyBinding};
use anyhow::Context;

use super::*;

pub(super) async fn run_hotkey_listener(
    tx: std::sync::mpsc::Sender<DaemonAction>,
) -> anyhow::Result<()> {
    let (_config_path, cfg) = load_hotkey_config(None)?;
    if cfg.bindings.is_empty() {
        eprintln!("[daemon] No hotkey bindings configured.");
        return Ok(());
    }

    // Ensure GIO_LAUNCHED_DESKTOP_FILE is set so GNOME trusts us even when
    // launched from a terminal. The hotkeys module's ensure_desktop_entry()
    // writes the file; we just need to export the env vars.
    ensure_gio_desktop_env();

    // Tier 1: GNOME Shell GrabAccelerators (fast, no dialog, works on GNOME).
    match run_hotkey_listener_gnome_shell(&cfg, tx.clone()).await {
        Ok(()) => return Ok(()),
        Err(e) => {
            eprintln!("[daemon] GNOME Shell hotkeys unavailable ({e}), trying portal…");
        }
    }

    // Tier 2: XDG GlobalShortcuts portal (works on KDE, GNOME with portal, etc.)
    run_hotkey_listener_portal(&cfg, tx).await
}

/// Set GIO_LAUNCHED_DESKTOP_FILE env vars so the portal can identify us.
///
/// We point to the main app's desktop file so that xdg-desktop-portal
/// associates the daemon with `io.github.codegoddy.apexshot` — the same
/// app ID used in the PermissionStore by `ensure_portal_permissions()`.
/// Without this, the portal derives the app ID from the autostart desktop
/// file name (`apexshot`) which never matches, so permissions are never
/// found and the user is asked to approve every time.
///
/// The autostart desktop file has NoDisplay=true, so GNOME Shell won't
/// show a duplicate dock entry regardless of which desktop file we point to.
pub(super) fn ensure_gio_desktop_env() {
    let desktop_path = if let Some(desktop_path) = crate::app_identity::desktop_file_for_portal() {
        desktop_path
    } else {
        let app_id = crate::app_identity::app_id();
        match ensure_desktop_entry_pub(app_id) {
            Ok(path) => path,
            Err(_) => return,
        }
    };

    if std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE").is_none() {
        std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", &desktop_path);
    }
    if std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE_PID").is_none() {
        std::env::set_var(
            "GIO_LAUNCHED_DESKTOP_FILE_PID",
            std::process::id().to_string(),
        );
    }
    eprintln!("[daemon] GIO desktop env set ({})", desktop_path.display());
}

pub(super) fn hotkey_debug_enabled() -> bool {
    std::env::var_os("APEXSHOT_HOTKEY_DEBUG").is_some()
}

/// Tier 1: GNOME Shell `GrabAccelerators` / `AcceleratorActivated`.
pub(super) async fn run_hotkey_listener_gnome_shell(
    cfg: &crate::hotkeys::HotkeyConfig,
    tx: std::sync::mpsc::Sender<DaemonAction>,
) -> anyhow::Result<()> {
    use futures_util::StreamExt;
    use std::collections::HashMap;
    use zbus::zvariant::OwnedValue;

    let conn = zbus::Connection::session().await?;

    let shell = zbus::Proxy::new(
        &conn,
        "org.gnome.Shell",
        "/org/gnome/Shell",
        "org.gnome.Shell",
    )
    .await?;

    let grab_args: Vec<(String, u32, u32)> = cfg
        .bindings
        .iter()
        .map(|b| (accel_to_gnome(&b.accelerator), 15u32, 0u32))
        .collect();

    let action_ids: Vec<u32> = shell
        .call("GrabAccelerators", &(grab_args,))
        .await
        .context("GrabAccelerators call failed")?;

    let mut action_map: HashMap<u32, HotkeyBinding> = HashMap::new();
    for (idx, action_id) in action_ids.into_iter().enumerate() {
        if action_id != 0 {
            if let Some(binding) = cfg.bindings.get(idx) {
                action_map.insert(action_id, binding.clone());
            }
        }
    }

    if action_map.is_empty() {
        anyhow::bail!("GrabAccelerators returned no valid action IDs (all conflicts or refused)");
    }

    eprintln!(
        "[daemon] {} hotkey(s) registered via GNOME Shell.",
        action_map.len()
    );

    let match_rule = "type='signal',interface='org.gnome.Shell',member='AcceleratorActivated',path='/org/gnome/Shell'";
    let rule: zbus::MatchRule = match_rule.try_into()?;
    let mut stream = zbus::MessageStream::for_match_rule(rule, &conn, None).await?;

    while let Some(Ok(msg)) = stream.next().await {
        let Ok((action_id, _params)) = msg
            .body()
            .deserialize::<(u32, HashMap<String, OwnedValue>)>()
        else {
            continue;
        };

        if super::is_hotkey_suppressed() {
            eprintln!("[daemon] Hotkey suppressed (shortcut edit active)");
            continue;
        }

        if let Some(binding) = action_map.get(&action_id) {
            if let Some(act) = binding_to_daemon_action(binding) {
                eprintln!("[daemon] Hotkey fired: {:?}", act);
                let _ = tx.send(act);
            }
        }
    }

    Ok(())
}

/// Tier 2: XDG GlobalShortcuts portal.
/// Mirrors the working `run_portal_hotkey_daemon` in src/hotkeys/mod.rs exactly.
pub(super) async fn run_hotkey_listener_portal(
    cfg: &crate::hotkeys::HotkeyConfig,
    tx: std::sync::mpsc::Sender<DaemonAction>,
) -> anyhow::Result<()> {
    use crate::hotkeys::{accel_to_portal, ensure_desktop_entry_pub};
    use futures_util::StreamExt;
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

    let app_id = std::env::var("APEXSHOT_APP_ID")
        .unwrap_or_else(|_| crate::app_identity::app_id().to_string());

    let conn = zbus::Connection::session()
        .await
        .context("Failed to connect to session D-Bus")?;

    // Register app_id with the portal so it can associate us with our .desktop file.
    let _ = ensure_desktop_entry_pub(&app_id);
    if let Err(e) = portal_register_app_id(&conn, &app_id).await {
        eprintln!("[daemon] Portal Registry.Register failed (continuing): {e}");
    }

    let portal = zbus::Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.GlobalShortcuts",
    )
    .await
    .context("GlobalShortcuts portal not available")?;

    // Helpers shared with hotkeys/mod.rs pattern.
    let sender_id = conn
        .unique_name()
        .ok_or_else(|| anyhow::anyhow!("No D-Bus unique name"))?
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_")
        .to_string();

    let mk_token = || {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        // Portal token charset: [A-Za-z0-9_] only.
        format!("apexshot_{pid}_{nanos}")
    };

    let mk_request_path = |tok: &str| -> anyhow::Result<OwnedObjectPath> {
        format!("/org/freedesktop/portal/desktop/request/{sender_id}/{tok}")
            .try_into()
            .context("Invalid portal request path")
    };

    // ── CreateSession ─────────────────────────────────────────────────────────
    let create_tok = mk_token();
    let session_tok = mk_token();
    let mut create_opts: HashMap<String, Value> = HashMap::new();
    create_opts.insert("handle_token".into(), Value::from(create_tok.clone()));
    create_opts.insert("session_handle_token".into(), Value::from(session_tok));

    let create_req_path = mk_request_path(&create_tok)?;
    // Subscribe BEFORE the call to avoid a race condition.
    let create_rule_str = format!(
        "type='signal',interface='org.freedesktop.portal.Request',member='Response',path='{}'",
        create_req_path.as_str()
    );
    let create_rule: zbus::MatchRule = create_rule_str.as_str().try_into()?;
    let mut create_stream =
        zbus::MessageStream::for_match_rule(create_rule, &conn, Some(1)).await?;

    let _req: OwnedObjectPath = portal
        .call("CreateSession", &(create_opts))
        .await
        .context("GlobalShortcuts.CreateSession failed")?;

    let (create_status, create_results) = {
        let msg = create_stream
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("No CreateSession response"))??;
        msg.body()
            .deserialize::<(u32, HashMap<String, OwnedValue>)>()
            .context("Failed to deserialize CreateSession response")?
    };
    if create_status != 0 {
        anyhow::bail!("CreateSession response={create_status}");
    }

    let session_handle_str: String = create_results
        .get("session_handle")
        .ok_or_else(|| anyhow::anyhow!("Missing session_handle in CreateSession response"))?
        .try_clone()
        .context("clone session_handle")?
        .try_into()
        .context("session_handle not a string")?;

    let session_handle: OwnedObjectPath = session_handle_str
        .try_into()
        .context("Invalid session_handle object path")?;

    eprintln!("[daemon] Portal session created.");

    // ── BindShortcuts ─────────────────────────────────────────────────────────
    let mut id_to_binding: HashMap<String, HotkeyBinding> = HashMap::new();
    let mut shortcuts: Vec<(String, HashMap<String, Value>)> = Vec::new();

    for (idx, binding) in cfg.bindings.iter().enumerate() {
        let id = binding
            .name
            .clone()
            .unwrap_or_else(|| format!("binding_{idx}"));
        let preferred_trigger = accel_to_portal(&binding.accelerator);
        let mut props: HashMap<String, Value> = HashMap::new();
        props.insert("description".into(), Value::from(id.replace('_', " ")));
        // Skip Print-based triggers — they're often reserved on desktops.
        if !preferred_trigger.to_ascii_uppercase().ends_with("PRINT") {
            props.insert("preferred_trigger".into(), Value::from(preferred_trigger));
        }
        shortcuts.push((id.clone(), props));
        id_to_binding.insert(id, binding.clone());
    }

    let bind_tok = mk_token();
    let mut bind_opts: HashMap<String, Value> = HashMap::new();
    bind_opts.insert("handle_token".into(), Value::from(bind_tok.clone()));

    let bind_req_path = mk_request_path(&bind_tok)?;
    let bind_rule_str = format!(
        "type='signal',interface='org.freedesktop.portal.Request',member='Response',path='{}'",
        bind_req_path.as_str()
    );
    let bind_rule: zbus::MatchRule = bind_rule_str.as_str().try_into()?;
    let mut bind_stream = zbus::MessageStream::for_match_rule(bind_rule, &conn, Some(1)).await?;

    let _bind_req: OwnedObjectPath = portal
        .call(
            "BindShortcuts",
            &(session_handle.clone(), shortcuts, "".to_string(), bind_opts),
        )
        .await
        .context("GlobalShortcuts.BindShortcuts failed")?;

    eprintln!("[daemon] Registering shortcuts with portal…");

    let (bind_status, _bind_results) = {
        let msg = bind_stream
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("No BindShortcuts response"))??;
        msg.body()
            .deserialize::<(u32, HashMap<String, OwnedValue>)>()
            .context("Failed to deserialize BindShortcuts response")?
    };
    match bind_status {
        0 => eprintln!(
            "[daemon] Portal shortcuts bound ({} shortcut(s)).",
            id_to_binding.len()
        ),
        1 => anyhow::bail!("BindShortcuts cancelled by user"),
        s => {
            // Status 2 can mean "shortcuts set but user may need to confirm in Settings".
            // This is non-fatal — activations will still be delivered.
            eprintln!("[daemon] BindShortcuts response={s} (non-fatal, continuing to listen).");
        }
    }

    // ── Listen for Activated signals ─────────────────────────────────────────
    // Different portal backends may emit GlobalShortcuts signals on different
    // object paths, so don't restrict the match rule by path; filter by the
    // session_handle carried in the signal payload instead.
    let debug = hotkey_debug_enabled();
    let activated_rule = if debug {
        "type='signal',interface='org.freedesktop.portal.GlobalShortcuts'"
    } else {
        "type='signal',interface='org.freedesktop.portal.GlobalShortcuts',member='Activated'"
    };
    let rule: zbus::MatchRule = activated_rule.try_into()?;
    let mut activated_stream = zbus::MessageStream::for_match_rule(rule, &conn, None).await?;

    eprintln!("[daemon] Listening for portal hotkey activations…");

    while let Some(Ok(msg)) = activated_stream.next().await {
        // Signal body: (o session_handle, s shortcut_id, t timestamp, a{sv} options)
        let parsed: Result<(OwnedObjectPath, String, u64, HashMap<String, OwnedValue>), _> =
            msg.body().deserialize();

        let (signal_session, shortcut_id, _ts, _opts) = match parsed {
            Ok(v) => v,
            Err(e) => {
                if debug {
                    eprintln!(
                        "[daemon] Hotkey debug: received non-Activated or unexpected GlobalShortcuts signal: {e}"
                    );
                    eprintln!("[daemon] Hotkey debug: raw message: {msg:?}");
                }
                continue;
            }
        };

        if signal_session != session_handle {
            if debug {
                eprintln!(
                    "[daemon] Hotkey debug: ignoring activation for other session {} (expected {})",
                    signal_session.as_str(),
                    session_handle.as_str()
                );
            }
            continue;
        }

        if super::is_hotkey_suppressed() {
            eprintln!("[daemon] Hotkey suppressed (shortcut edit active)");
            continue;
        }
        if let Some(binding) = id_to_binding.get(&shortcut_id) {
            if let Some(act) = binding_to_daemon_action(binding) {
                eprintln!("[daemon] Portal hotkey fired: {:?}", act);
                let _ = tx.send(act);
            }
        }
    }

    Ok(())
}

/// Register this process's D-Bus peer with the portal's host Registry so it can
/// be associated with our app_id / .desktop file.
pub(super) async fn portal_register_app_id(
    conn: &zbus::Connection,
    app_id: &str,
) -> anyhow::Result<()> {
    use std::collections::HashMap;
    use zbus::zvariant::Value;

    let registry = zbus::Proxy::new(
        conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.host.portal.Registry",
    )
    .await
    .context("Failed to create host Registry proxy")?;

    let opts: HashMap<String, Value> = HashMap::new();
    for attempt in 0..2u8 {
        let call: Result<(), zbus::Error> = registry
            .call("Register", &(app_id.to_string(), opts.clone()))
            .await;
        match call {
            Ok(()) => {
                eprintln!("[daemon] Portal: registered app_id={app_id}");
                return Ok(());
            }
            Err(e) if attempt == 0 && e.to_string().contains("App info not found") => {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            Err(e) => return Err(anyhow::anyhow!("Registry.Register failed: {e}")),
        }
    }
    anyhow::bail!("Registry.Register failed after retries")
}

pub(super) fn binding_to_daemon_action(binding: &HotkeyBinding) -> Option<DaemonAction> {
    // First try matching by the binding's name field.
    if let Some(name) = binding.name.as_deref() {
        match name {
            "capture_area" | "capture-area" => return Some(super::DaemonAction::CaptureArea),
            "capture_crosshair" | "capture-crosshair" => {
                return Some(super::DaemonAction::CaptureCrosshair);
            }
            "capture_screen" | "capture-screen" => return Some(super::DaemonAction::CaptureScreen),
            "capture_window" | "capture-window" => return Some(super::DaemonAction::CaptureWindow),
            "open_file" | "open-file" => {
                return Some(super::DaemonAction::OpenFile);
            }
            "open_from_clipboard" | "open-from-clipboard" => {
                return Some(super::DaemonAction::OpenFromClipboard);
            }
            "restore_recently_closed" | "restore-recently-closed" => {
                return Some(super::DaemonAction::RestoreRecentlyClosed);
            }
            "toggle_overlays" | "toggle-overlays" => {
                return Some(super::DaemonAction::ToggleOverlays);
            }
            "show_last_preview" | "show-last-preview" => {
                return Some(super::DaemonAction::ShowLastPreview);
            }
            "record_screen" | "record-screen" => return Some(super::DaemonAction::RecordScreen),
            "record_area" | "record-area" => return Some(super::DaemonAction::RecordArea),
            "open_recording_ui" | "open-recording-ui" => {
                return Some(super::DaemonAction::OpenRecordingUi);
            }
            "open_video_editor" | "open-video-editor" => {
                return Some(super::DaemonAction::OpenVideoEditor);
            }
            "recording_stop_save"
            | "recording-stop-save"
            | "stop_recording"
            | "stop-recording"
            | "stop_recording_save" => {
                return Some(super::DaemonAction::StopRecordingSave);
            }
            _ => {}
        }
    }

    // Fallback: derive action from the args list.
    match binding.args.first().map(|s| s.as_str()) {
        Some("capture") => match binding.args.get(1).map(|s| s.as_str()) {
            Some("area") => Some(super::DaemonAction::CaptureArea),
            Some("crosshair") => Some(super::DaemonAction::CaptureCrosshair),
            Some("screen") => Some(super::DaemonAction::CaptureScreen),
            Some("window") => Some(super::DaemonAction::CaptureWindow),
            _ => None,
        },
        Some("open-file") => Some(super::DaemonAction::OpenFile),
        Some("open-from-clipboard") => Some(super::DaemonAction::OpenFromClipboard),
        Some("restore-recently-closed") => Some(super::DaemonAction::RestoreRecentlyClosed),
        Some("toggle-overlays") => Some(super::DaemonAction::ToggleOverlays),
        Some("show-last-preview") => Some(super::DaemonAction::ShowLastPreview),
        Some("record") => match binding.args.get(1).map(|s| s.as_str()) {
            Some("ui") => Some(super::DaemonAction::OpenRecordingUi),
            Some("screen") => Some(super::DaemonAction::RecordScreen),
            Some("area") => Some(super::DaemonAction::RecordArea),
            Some("stop") => Some(super::DaemonAction::StopRecordingSave),
            _ => None,
        },
        Some("recording-control") => match binding.args.get(1).map(|s| s.as_str()) {
            Some("stop-save") => Some(super::DaemonAction::StopRecordingSave),
            _ => None,
        },
        _ => None,
    }
}
