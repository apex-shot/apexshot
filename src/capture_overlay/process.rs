#[cfg(unix)]
fn synthetic_output(status_code: i32) -> Output {
    Output {
        status: std::process::ExitStatus::from_raw(status_code << 8),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

fn run_capture_binary(
    extra_args: &[&str],
    background_png: Option<&Path>,
) -> Result<Output, SelectionError> {
    if builtin_screenshot_overlay_active() {
        return Err(blocked_selection_error(
            LaunchBlockedReason::BuiltinOverlayActive,
        ));
    }

    // Prefer the warm worker (Qt already up). Fall back to cold spawn on any
    // warm-path failure so capture never hard-breaks.
    #[cfg(unix)]
    {
        match run_capture_via_warm_worker(extra_args, background_png) {
            Ok(output) => {
                reemit_capture_stderr(&output.stderr);
                match classify_overlay_exit_code(output.status.code()) {
                    Ok(Some("forwarded")) => {
                        return Ok(synthetic_output(
                            OverlayExitCode::ForwardedToExistingOverlay as i32,
                        ));
                    }
                    Err(reason) => return Err(blocked_selection_error(reason)),
                    _ => {}
                }
                return Ok(output);
            }
            Err(warm_err) => {
                eprintln!(
                    "[capture_overlay] Warm capture helper unavailable ({warm_err}); \
                     falling back to cold spawn."
                );
            }
        }
    }

    run_capture_binary_cold(extra_args, background_png)
}

fn run_capture_binary_cold(
    extra_args: &[&str],
    background_png: Option<&Path>,
) -> Result<Output, SelectionError> {
    let binary = find_capture_binary().ok_or_else(|| {
        SelectionError::InitError(if crate::app_identity::portal_only() {
            "apexshot-capture is not shipped in Flatpak/portal-only builds; \
             still capture must use the XDG Screenshot portal path."
                .into()
        } else {
            "apexshot-capture binary not found. \
             Re-run `cargo build --release` to compile it, or check your PATH."
                .into()
        })
    })?;

    // Capture requests often originate from the autostart daemon, which uses a
    // different desktop identity for tray/hotkey purposes. Override that
    // identity while spawning the capture helper so xdg-desktop-portal stores
    // screenshot/screencast grants against the main ApexShot desktop file.
    let _portal_identity = crate::utils::desktop_env::scoped_portal_capture_identity();

    let mut interactive_session = InteractiveOverlaySessionGuard::begin(extra_args);

    let mut cmd = Command::new(&binary);
    cmd.env("QT_IM_MODULE", "compose")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        // Capture stderr so portal/KWin failure detail can reach the UI.
        // Re-emit to the process journal after wait so existing log workflows still work.
        .stderr(Stdio::piped());

    for arg in extra_args {
        cmd.arg(arg);
    }

    if let Some(bg) = background_png {
        cmd.arg("--background").arg(bg);
    }

    let child = cmd.spawn().map_err(|e| {
        SelectionError::InitError(format!(
            "Failed to launch apexshot-capture ({}): {}",
            binary.display(),
            e
        ))
    })?;
    interactive_session.attach_child_pid(child.id());
    let output = child.wait_with_output().map_err(|e| {
        SelectionError::InitError(format!(
            "Failed to wait for apexshot-capture ({}): {}",
            binary.display(),
            e
        ))
    })?;

    reemit_capture_stderr(&output.stderr);

    match classify_overlay_exit_code(output.status.code()) {
        Ok(Some("forwarded")) => {
            #[cfg(unix)]
            {
                return Ok(synthetic_output(
                    OverlayExitCode::ForwardedToExistingOverlay as i32,
                ));
            }
            #[cfg(not(unix))]
            {
                return Ok(output);
            }
        }
        Err(reason) => return Err(blocked_selection_error(reason)),
        _ => {}
    }

    Ok(output)
}

/// Write capture-helper stderr to our own stderr so journalctl still shows it.
fn reemit_capture_stderr(stderr: &[u8]) {
    if stderr.is_empty() {
        return;
    }
    use std::io::Write;
    let _ = std::io::stderr().write_all(stderr);
}
