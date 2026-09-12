pub fn spawn_recording_controls_via_cpp(
    dbus_dest: &str,
    session_id: &str,
    params: crate::recording::RecordingControlsParams,
) -> anyhow::Result<Child> {
    let binary = find_capture_binary().ok_or_else(|| {
        anyhow::anyhow!(
            "apexshot-capture binary not found. Re-run `cargo build --release` to compile it, or check your PATH."
        )
    })?;

    let mut cmd = Command::new(&binary);
    cmd.env("QT_IM_MODULE", "compose")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .arg("--record-controls")
        .arg(format!("--dbus-dest={dbus_dest}"))
        .arg(format!("--session-id={session_id}"))
        .arg(format!("--capture-x={}", params.capture_x))
        .arg(format!("--capture-y={}", params.capture_y))
        .arg(format!("--capture-w={}", params.capture_w))
        .arg(format!("--capture-h={}", params.capture_h));

    if params.is_fullscreen {
        cmd.arg("--fullscreen");
    }
    if params.show_timer {
        cmd.arg("--show-timer");
    } else {
        cmd.arg("--hide-timer");
    }

    cmd.spawn().map_err(|e| {
        anyhow::anyhow!(
            "Failed to launch apexshot-capture record-controls ({}): {}",
            binary.display(),
            e
        )
    })
}
