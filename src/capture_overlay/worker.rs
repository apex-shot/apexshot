// ── Warm capture helper ──────────────────────────────────────────────────────

#[cfg(unix)]
fn warm_capture_disabled() -> bool {
    std::env::var_os("APEXSHOT_DISABLE_WARM_CAPTURE").is_some()
}

#[cfg(unix)]
fn worker_socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("apexshot-capture-worker.sock")
}

#[cfg(unix)]
struct WarmWorkerState {
    child: Child,
}

#[cfg(unix)]
fn warm_worker_state() -> &'static Mutex<Option<WarmWorkerState>> {
    static STATE: OnceLock<Mutex<Option<WarmWorkerState>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

#[derive(Debug, Deserialize)]
struct WorkerJobResponse {
    exit_code: i32,
    #[serde(default)]
    stdout: String,
    #[serde(default)]
    stderr: String,
}

#[derive(Debug, Serialize)]
struct WorkerJobRequest<'a> {
    args: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    background: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct WorkerCmdRequest<'a> {
    cmd: &'a str,
}

/// Start (or verify) the long-lived `apexshot-capture --worker` process.
///
/// Safe to call from the daemon at startup and again before each capture.
/// No-op when warm capture is disabled or the binary is missing.
pub fn ensure_warm_capture_helper() {
    #[cfg(unix)]
    {
        if warm_capture_disabled() {
            return;
        }
        match ensure_warm_worker_running() {
            Ok(()) => {}
            Err(err) => {
                eprintln!("[capture_overlay] Warm capture helper not ready: {err}");
            }
        }
    }
}

/// Ask the warm helper to exit and clear local process bookkeeping.
pub fn shutdown_warm_capture_helper() {
    #[cfg(unix)]
    {
        if warm_capture_disabled() {
            return;
        }
        let _ = worker_send_cmd("shutdown", Duration::from_millis(800));
        if let Ok(mut guard) = warm_worker_state().lock() {
            if let Some(mut state) = guard.take() {
                let _ = state.child.kill();
                let _ = state.child.wait();
            }
        }
        let path = worker_socket_path();
        let _ = std::fs::remove_file(&path);
        eprintln!("[capture_overlay] Warm capture helper shut down.");
    }
}

#[cfg(unix)]
fn ensure_warm_worker_running() -> Result<(), String> {
    // Reuse a live worker when possible.
    if worker_ping(Duration::from_millis(250)).is_ok() {
        return Ok(());
    }

    let binary = find_capture_binary()
        .ok_or_else(|| "apexshot-capture binary not found for warm helper".to_string())?;

    // Clear a stale socket before spawn.
    let sock = worker_socket_path();
    let _ = std::fs::remove_file(&sock);

    // Drop any bookkeeping for a dead child.
    if let Ok(mut guard) = warm_worker_state().lock() {
        if let Some(mut state) = guard.take() {
            let _ = state.child.kill();
            let _ = state.child.wait();
        }
    }

    let _portal_identity = crate::utils::desktop_env::scoped_portal_capture_identity();

    let mut cmd = Command::new(&binary);
    cmd.arg("--worker")
        .env("QT_IM_MODULE", "compose")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        // Worker logs readiness + job stderr to its stderr; inherit so journalctl
        // still shows freeze timing from the warm path.
        .stderr(Stdio::inherit());

    let child = cmd.spawn().map_err(|e| {
        format!(
            "failed to spawn warm capture worker ({}): {e}",
            binary.display()
        )
    })?;
    let pid = child.id();

    if let Ok(mut guard) = warm_worker_state().lock() {
        *guard = Some(WarmWorkerState { child });
    }

    // Wait until the worker socket accepts pings.
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if worker_ping(Duration::from_millis(200)).is_ok() {
            eprintln!("[capture_overlay] Warm capture helper ready (pid={pid}).");
            return Ok(());
        }
        // If the child already died, fail fast.
        if let Ok(mut guard) = warm_worker_state().lock() {
            if let Some(state) = guard.as_mut() {
                if let Ok(Some(status)) = state.child.try_wait() {
                    *guard = None;
                    return Err(format!(
                        "warm capture worker exited during startup: {status}"
                    ));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    }

    Err("warm capture worker did not become ready in time".into())
}

#[cfg(unix)]
fn worker_ping(timeout: Duration) -> Result<(), String> {
    worker_send_cmd("ping", timeout)?;
    Ok(())
}

#[cfg(unix)]
fn worker_send_cmd(cmd: &str, timeout: Duration) -> Result<String, String> {
    let path = worker_socket_path();
    let mut stream = UnixStream::connect(&path)
        .map_err(|e| format!("connect worker socket {}: {e}", path.display()))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let req = serde_json::to_string(&WorkerCmdRequest { cmd })
        .map_err(|e| format!("serialize worker cmd: {e}"))?;
    stream
        .write_all(req.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|e| format!("write worker cmd: {e}"))?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("read worker cmd response: {e}"))?;
    if line.trim().is_empty() {
        return Err("empty worker cmd response".into());
    }
    Ok(line)
}

#[cfg(unix)]
fn run_capture_via_warm_worker(
    extra_args: &[&str],
    background_png: Option<&Path>,
) -> Result<Output, String> {
    if warm_capture_disabled() {
        return Err("warm capture disabled by env".into());
    }

    ensure_warm_worker_running()?;

    let _portal_identity = crate::utils::desktop_env::scoped_portal_capture_identity();
    let mut interactive_session = InteractiveOverlaySessionGuard::begin(extra_args);

    // Track the worker pid for GNOME screenshot-lock stacking when available.
    if let Ok(guard) = warm_worker_state().lock() {
        if let Some(state) = guard.as_ref() {
            interactive_session.attach_child_pid(state.child.id());
        }
    }

    let path = worker_socket_path();
    let mut stream =
        UnixStream::connect(&path).map_err(|e| format!("connect worker for job: {e}"))?;

    // Interactive area selection can take a long time; keep a generous timeout.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15 * 60)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

    let background = background_png.map(|p| p.to_string_lossy().into_owned());
    let req = WorkerJobRequest {
        args: extra_args.to_vec(),
        background: background.as_deref(),
    };
    let payload = serde_json::to_string(&req).map_err(|e| format!("serialize worker job: {e}"))?;
    stream
        .write_all(payload.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|e| format!("write worker job: {e}"))?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("read worker job response: {e}"))?;
    if line.trim().is_empty() {
        return Err("empty worker job response".into());
    }

    let response: WorkerJobResponse = serde_json::from_str(line.trim())
        .map_err(|e| format!("parse worker job response: {e} ({})", line.trim()))?;

    // Drop guard after job finishes so GNOME lock is released.
    drop(interactive_session);

    Ok(Output {
        status: std::process::ExitStatus::from_raw(response.exit_code << 8),
        stdout: response.stdout.into_bytes(),
        stderr: response.stderr.into_bytes(),
    })
}
