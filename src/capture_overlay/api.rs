/// Run the capture overlay and handle the Window toolbar button (exit code 3)
/// by immediately doing a window capture via the portal.
/// Returns `SelectionResult` — `Ok(None)` means "window capture was done and
/// the result should be retrieved from `capture_window_via_cpp()`".
pub fn run_capture_overlay_with_window(
    background_png: Option<&std::path::Path>,
) -> SelectionResult {
    let output = run_capture_binary(&[], background_png)?;

    match output.status.code() {
        Some(0) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_selection_json(stdout.trim())
        }
        Some(1) | None => Ok(OverlaySelection::Area(None)),
        Some(code) if code == OverlayExitCode::ForwardedToExistingOverlay as i32 => {
            Ok(OverlaySelection::Area(None))
        }
        Some(3) => Ok(OverlaySelection::Area(Some(SelectionArea {
            x: i32::MIN,
            y: i32::MIN,
            width: i32::MIN,
            height: i32::MIN,
        }))),
        Some(4) => {
            eprintln!("[capture_overlay] Window picker: switch to area mode requested");
            Ok(OverlaySelection::Area(Some(SelectionArea {
                x: i32::MIN + 1,
                y: i32::MIN,
                width: i32::MIN,
                height: i32::MIN,
            })))
        }
        Some(5) => {
            eprintln!("[capture_overlay] Window picker: switch to fullscreen mode requested");
            Ok(OverlaySelection::Area(Some(SelectionArea {
                x: i32::MIN + 2,
                y: i32::MIN,
                width: i32::MIN,
                height: i32::MIN,
            })))
        }
        Some(code) => Err(overlay_exit_error(
            "overlay",
            code,
            &String::from_utf8_lossy(&output.stderr),
        )),
    }
}

/// Run the native Qt capture overlay and return the selected area.
///
/// * `background_png` — optional path to a PNG screenshot to show as the
///   overlay background. If `None`, a dark semi-transparent overlay is used.
///
/// Exit code 3 means "window capture requested" — we then invoke
/// `--window-capture` to use GNOME Shell DBus.
pub fn run_capture_overlay(background_png: Option<&std::path::Path>) -> SelectionResult {
    let output = run_capture_binary(&[], background_png)?;

    match output.status.code() {
        Some(0) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_selection_json(stdout.trim())
        }
        Some(1) | None => Ok(OverlaySelection::Area(None)),
        Some(code) if code == OverlayExitCode::ForwardedToExistingOverlay as i32 => {
            Ok(OverlaySelection::Area(None))
        }
        Some(3) => {
            eprintln!(
                "[capture_overlay] Window capture requested — launching GNOME DBus window capture"
            );
            let _ = capture_window_via_cpp();
            Ok(OverlaySelection::Area(None))
        }
        Some(code) => Err(overlay_exit_error(
            "overlay",
            code,
            &String::from_utf8_lossy(&output.stderr),
        )),
    }
}

/// Window capture is temporarily discontinued (Wayland window listing / picker
/// maintenance cost). Keep the API so callers compile; return a clear error.
pub fn capture_window_file_via_cpp() -> Result<PathBuf, SelectionError> {
    Err(SelectionError::InitError(
        "Window capture is temporarily discontinued. Use area or fullscreen capture instead."
            .into(),
    ))
}

pub fn capture_window_via_cpp() -> Result<CaptureData, SelectionError> {
    Err(SelectionError::InitError(
        "Window capture is temporarily discontinued. Use area or fullscreen capture instead."
            .into(),
    ))
}

pub fn capture_screen_file_via_cpp() -> Result<PathBuf, SelectionError> {
    // Flatpak builds omit apexshot-capture; use the XDG Screenshot portal.
    if crate::app_identity::portal_only() {
        return capture_still_file_via_portal(false);
    }

    if should_use_gtk_layer_shell_selector() {
        eprintln!("[capture_overlay] Using native wlroots fullscreen capture");
        return capture_screen_file_via_wlroots();
    }

    let output = run_capture_binary(&["--capture-screen"], None)?;

    match output.status.code() {
        Some(0) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_capture_screen_json(stdout.trim())
        }
        Some(1) | None => Err(SelectionError::Cancelled),
        Some(code) if code == OverlayExitCode::ForwardedToExistingOverlay as i32 => {
            Err(SelectionError::Cancelled)
        }
        Some(code) => Err(overlay_exit_error(
            "--capture-screen",
            code,
            &String::from_utf8_lossy(&output.stderr),
        )),
    }
}

pub fn capture_screen_via_cpp() -> Result<CaptureData, SelectionError> {
    if crate::app_identity::portal_only() {
        return capture_still_via_portal(false);
    }

    let path = capture_screen_file_via_cpp()?;
    let capture = load_capture_data_from_path(&path);
    let _ = std::fs::remove_file(&path);
    capture
}

pub fn open_recording_ui_via_cpp() -> Result<AreaCapturePathResult, SelectionError> {
    let config = crate::config::load_config();
    let extra_args = build_recording_ui_args(&config);
    let arg_refs: Vec<&str> = extra_args.iter().map(|s| s.as_str()).collect();
    let output = run_capture_binary(&arg_refs, None)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_area_capture_output_with_stderr(output.status.code(), stdout.trim(), stderr.trim())
}

pub fn open_quick_capture_via_cpp() -> Result<AreaCapturePathResult, SelectionError> {
    if should_use_gtk_layer_shell_selector() {
        eprintln!(
            "[capture_overlay] Using ApexShot GTK capture menu on wlroots compositor"
        );
        return open_quick_capture_via_gtk_layer_shell_wlroots();
    }

    let config = crate::config::load_config();
    let extra_args = build_quick_capture_args(&config);
    let arg_refs: Vec<&str> = extra_args.iter().map(String::as_str).collect();
    let output = run_capture_binary(&arg_refs, None)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_area_capture_output_with_stderr(output.status.code(), stdout.trim(), stderr.trim())
}

pub fn quick_capture_via_cpp() -> Result<AreaCaptureResult, SelectionError> {
    match open_quick_capture_via_cpp()? {
        AreaCapturePathResult::Captured(path) => {
            let capture = load_capture_data_from_path(&path);
            let _ = std::fs::remove_file(&path);
            capture.map(AreaCaptureResult::Captured)
        }
        AreaCapturePathResult::CapturedOnDisplay(path, display) => {
            let capture = load_capture_data_from_path(&path);
            let _ = std::fs::remove_file(&path);
            capture.map(|capture| AreaCaptureResult::CapturedOnDisplay(capture, display))
        }
        AreaCapturePathResult::ScrollCaptured(path) => {
            let capture = load_capture_data_from_path(&path);
            let _ = std::fs::remove_file(&path);
            capture.map(AreaCaptureResult::ScrollCaptured)
        }
        AreaCapturePathResult::OcrRequested(capture) => {
            Ok(AreaCaptureResult::OcrRequested(capture))
        }
        AreaCapturePathResult::RecordingRequested(request) => {
            Ok(AreaCaptureResult::RecordingRequested(request))
        }
        AreaCapturePathResult::RecordingConfigUpdated | AreaCapturePathResult::Cancelled => {
            Ok(AreaCaptureResult::Cancelled)
        }
    }
}

pub fn capture_area_file_via_cpp() -> Result<AreaCapturePathResult, SelectionError> {
    // Flatpak builds omit apexshot-capture; use the interactive Screenshot portal.
    if crate::app_identity::portal_only() {
        return capture_still_file_via_portal(true).map(AreaCapturePathResult::Captured);
    }

    if should_use_gtk_layer_shell_selector() {
        eprintln!(
            "[capture_overlay] Using ApexShot GTK layer-shell selector on wlroots compositor"
        );
        return capture_area_file_via_gtk_layer_shell_wlroots();
    }

    // Check config for remember selection
    let config = crate::config::load_config();
    let extra_args = build_area_init_args(&config);

    let arg_refs: Vec<&str> = extra_args.iter().map(|s| s.as_str()).collect();
    eprintln!(
        "[capture_overlay] capture_area_via_cpp: launching {:?}",
        arg_refs
    );
    let output = run_capture_binary(&arg_refs, None)?;
    let exit_code = output.status.code();
    eprintln!(
        "[capture_overlay] capture_area_via_cpp: --area-init exited with code {:?}",
        exit_code
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!(
        "[capture_overlay] capture_area_via_cpp: stdout = {:?}",
        stdout.trim()
    );

    parse_area_capture_output_with_stderr(exit_code, stdout.trim(), stderr.trim())
}

pub fn capture_area_via_cpp() -> Result<AreaCaptureResult, SelectionError> {
    if crate::app_identity::portal_only() {
        return capture_still_via_portal(true).map(AreaCaptureResult::Captured);
    }

    if should_use_gtk_layer_shell_selector() {
        eprintln!(
            "[capture_overlay] Using ApexShot GTK layer-shell selector on wlroots compositor"
        );
        return capture_area_via_gtk_layer_shell_wlroots();
    }

    match capture_area_file_via_cpp()? {
        AreaCapturePathResult::Captured(path) => {
            let capture = load_capture_data_from_path(&path);
            let _ = std::fs::remove_file(&path);
            capture.map(AreaCaptureResult::Captured)
        }
        AreaCapturePathResult::CapturedOnDisplay(path, display) => {
            let capture = load_capture_data_from_path(&path);
            let _ = std::fs::remove_file(&path);
            capture.map(|capture| AreaCaptureResult::CapturedOnDisplay(capture, display))
        }
        AreaCapturePathResult::ScrollCaptured(path) => {
            let capture = load_capture_data_from_path(&path);
            let _ = std::fs::remove_file(&path);
            capture.map(AreaCaptureResult::ScrollCaptured)
        }
        AreaCapturePathResult::OcrRequested(capture) => {
            Ok(AreaCaptureResult::OcrRequested(capture))
        }
        AreaCapturePathResult::RecordingRequested(request) => {
            Ok(AreaCaptureResult::RecordingRequested(request))
        }
        AreaCapturePathResult::RecordingConfigUpdated => Ok(AreaCaptureResult::Cancelled),
        AreaCapturePathResult::Cancelled => Ok(AreaCaptureResult::Cancelled),
    }
}

pub fn capture_crosshair_file_via_cpp() -> Result<PathBuf, SelectionError> {
    // No custom crosshair overlay in portal-only builds — interactive portal UI.
    if crate::app_identity::portal_only() {
        return capture_still_file_via_portal(true);
    }

    if should_use_gtk_layer_shell_selector() {
        eprintln!(
            "[capture_overlay] Using ApexShot GTK layer-shell crosshair selector on wlroots compositor"
        );
        return capture_crosshair_file_via_gtk_layer_shell_wlroots();
    }

    let config = crate::config::load_config();
    let args = build_crosshair_args(&config);
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let output = run_capture_binary(&arg_refs, None)?;

    match output.status.code() {
        Some(0) => parse_capture_screen_json(&String::from_utf8_lossy(&output.stdout)),
        Some(1) | None => Err(SelectionError::Cancelled),
        Some(code) if code == OverlayExitCode::ForwardedToExistingOverlay as i32 => {
            Err(SelectionError::Cancelled)
        }
        Some(code) => Err(overlay_exit_error(
            "crosshair mode",
            code,
            &String::from_utf8_lossy(&output.stderr),
        )),
    }
}

pub fn capture_crosshair_via_cpp() -> Result<AreaCaptureResult, SelectionError> {
    if crate::app_identity::portal_only() {
        return capture_still_via_portal(true).map(AreaCaptureResult::Captured);
    }

    if should_use_gtk_layer_shell_selector() {
        eprintln!(
            "[capture_overlay] Using ApexShot GTK layer-shell crosshair selector on wlroots compositor"
        );
        return capture_crosshair_via_gtk_layer_shell_wlroots();
    }

    let path = capture_crosshair_file_via_cpp()?;
    let capture = load_capture_data_from_path(&path);
    let _ = std::fs::remove_file(&path);
    capture.map(AreaCaptureResult::Captured)
}
