use anyhow::Context;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::path::PathBuf;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

pub(super) fn portal_app_id() -> String {
    std::env::var("APEXSHOT_APP_ID").unwrap_or_else(|_| crate::app_identity::app_id().to_string())
}

pub(super) async fn register_portal_app_id(
    conn: &zbus::Connection,
    app_id: &str,
) -> anyhow::Result<()> {
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

pub(super) fn token(prefix: &str) -> String {
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

pub(super) fn portal_sender_id(connection: &zbus::Connection) -> anyhow::Result<String> {
    let unique = connection
        .unique_name()
        .ok_or_else(|| anyhow::anyhow!("DBus connection has no unique name"))?
        .as_str();

    Ok(unique.trim_start_matches(':').replace('.', "_"))
}

pub(super) fn portal_request_path(sender_id: &str, token: &str) -> anyhow::Result<OwnedObjectPath> {
    let path = format!("/org/freedesktop/portal/desktop/request/{sender_id}/{token}");
    path.try_into().context("Invalid portal request path")
}

pub(super) async fn portal_response_stream(
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

pub(super) async fn read_portal_response(
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
    let log_path = super::open_daemon_log_if_needed().map(|(p, f)| {
        log = Some(f);
        p
    });
    if let Some(p) = &log_path {
        super::log_line(&mut log, &format!("Hotkey daemon log: {}", p.display()));
    }

    let (config_path, cfg) = super::config::load_or_create_config(config_path)?;
    if cfg.bindings.is_empty() {
        anyhow::bail!("No bindings configured in {}", config_path.display());
    }

    super::log_line(
        &mut log,
        &format!("Hotkey config: {}", config_path.display()),
    );

    if let Some(pid) = super::existing_daemon_pid() {
        let msg = format!(
            "Hotkey daemon already running (pid {pid}). Stop it first (e.g. `pkill -f \"apexshot daemon\"`) and retry"
        );
        super::log_line(&mut log, &msg);
        anyhow::bail!(msg);
    }

    // Ensure the portal can associate us with an application id.
    let app_id = portal_app_id();
    let desktop_path = super::ensure_desktop_entry(&app_id)?;
    super::log_line(
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
    if super::gnome::is_gnome_desktop() && terminal_launch && !allow_desktop_relaunch {
        super::log_line(
            &mut log,
            &format!(
                "GNOME detected: this daemon was launched from a terminal; GlobalShortcuts activations often won't be delivered in this mode. Run without --no-desktop-relaunch (recommended) or start via the desktop entry (e.g. `gtk-launch {}`).",
                app_id
            ),
        );
    }
    if super::gnome::is_gnome_desktop() && terminal_launch && allow_desktop_relaunch {
        match super::try_relaunch_via_desktop(&app_id, &config_path, configure) {
            Ok(()) => {
                super::log_line(
                    &mut log,
                    "GNOME detected: relaunched hotkey daemon via desktop entry for reliable global shortcuts; exiting this terminal-started process.",
                );
                let follow_path = std::env::var_os("APEXSHOT_HOTKEY_LOG")
                    .map(PathBuf::from)
                    .or_else(super::default_daemon_log_path)
                    .or(log_path);
                if let Some(p) = follow_path {
                    super::log_line(
                        &mut log,
                        &format!("Follow logs with: tail -f {}", p.display()),
                    );
                }
                return Ok(());
            }
            Err(e) => {
                super::log_line(
                    &mut log,
                    &format!("GNOME detected but desktop relaunch failed (continuing anyway): {e}"),
                );
            }
        }
    }

    if super::gnome::is_gnome_desktop() {
        super::apply_gio_desktop_launch_env(&desktop_path);
    }

    let _pid_guard = match super::acquire_daemon_pid_guard() {
        Ok(guard) => guard,
        Err(e) => {
            super::log_line(&mut log, &format!("{e}"));
            return Err(e);
        }
    };

    let conn = zbus::Connection::session()
        .await
        .context("Failed to connect to session DBus")?;
    if let Err(e) = register_portal_app_id(&conn, &app_id).await {
        super::log_line(
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
    super::log_line(
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

    super::log_line(&mut log, "Portal: calling CreateSession…");

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

    super::log_line(
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
    let mut id_to_binding: HashMap<String, super::config::HotkeyBinding> = HashMap::new();
    let mut shortcuts: Vec<(String, HashMap<String, Value>)> = Vec::new();

    for (idx, binding) in cfg.bindings.iter().enumerate() {
        let id = binding
            .name
            .clone()
            .unwrap_or_else(|| format!("binding_{idx}"));

        let preferred_trigger = super::config::as_portal_trigger(&binding.accelerator);

        let mut props: HashMap<String, Value> = HashMap::new();
        props.insert(
            "description".into(),
            Value::from(binding.name.clone().unwrap_or_else(|| id.clone())),
        );

        if super::is_print_trigger(&preferred_trigger) {
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

    super::log_line(
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

    super::log_line(
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
            let print_triggers = super::print_trigger_count(&cfg);
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
                super::log_line(&mut log, "Configured shortcuts (portal):");
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

                    super::log_line(
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

    super::log_line(&mut log, "Hotkey daemon running (portal GlobalShortcuts)");
    super::log_line(&mut log, &format!("Config: {}", config_path.display()));
    for (id, binding) in &id_to_binding {
        let name = binding.name.as_deref().unwrap_or("(unnamed)");
        super::log_line(
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
                Ok(()) => super::log_line(&mut log, "Portal: opened shortcut configuration UI"),
                Err(e) => super::log_line(
                    &mut log,
                    &format!(
                        "Portal: ConfigureShortcuts failed (continuing without forcing UI): {e}"
                    ),
                ),
            }
        } else {
            super::log_line(
                &mut log,
                "Portal: ConfigureShortcuts is not supported by this portal backend (version < 2). Use the BindShortcuts dialog (if it appears) or system settings to edit shortcuts."
            );
        }
    }

    let action_exe = super::resolve_action_exe()?;
    super::log_line(
        &mut log,
        &format!("Hotkey actions will spawn: {}", action_exe.display()),
    );

    // Listen for activations.
    // Different portal backends may emit signals on different object paths, so don't
    // restrict the match rule by path; we filter by session_handle in the payload.
    let debug = super::hotkey_debug_enabled();
    if debug {
        super::log_line(&mut log, "Hotkey debug: enabled");
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
                    super::log_line(&mut log, &format!("Hotkey debug: received non-Activated or unexpected GlobalShortcuts signal: {e}"));
                    super::log_line(&mut log, &format!("Hotkey debug: raw message: {msg:?}"));
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

        super::log_line(&mut log, &format!("Activated shortcut: {}", shortcut_id));

        match super::spawn_hotkey_action(Some(&action_exe), &binding.args) {
            Ok((child, used_exe)) => {
                super::log_line(
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
                super::log_line(
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
