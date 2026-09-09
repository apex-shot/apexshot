fn wait_for_layer_shell_overlay_to_unmap() {
    // Layer-shell window destruction is acknowledged by the compositor just
    // after GTK returns. Give Hyprland/sway one frame to remove our overlay
    // before wlr-screencopy grabs the real screenshot, otherwise the capture
    // can include ApexShot's own UI.
    std::thread::sleep(std::time::Duration::from_millis(180));
}

/// Crop a sub-region from a `CaptureData` without re-capturing from the display.
fn capture_area_file_via_gtk_layer_shell_wlroots() -> Result<AreaCapturePathResult, SelectionError>
{
    force_wayland_gdk_for_layer_shell();

    // Multi-monitor (C++-style): pick a display first, then freeze that output
    // so the picker never appears in the freeze frame.
    if let Err(err) = gtk4::init() {
        // Already initialized is fine; only hard-fail when no display is usable.
        if gdk::Display::default().is_none() {
            return Err(SelectionError::InitError(format!(
                "GTK init failed for monitor picker: {err}"
            )));
        }
    }
    let monitor_choice = crate::overlay::select_target_monitor_choice()?;

    let backend = WaylandBackend::new()
        .map_err(|err| SelectionError::InitError(format!("Wayland backend unavailable: {err}")))?;
    let full_capture = backend
        .capture_screen_for_selection_at(Some((monitor_choice.x, monitor_choice.y)))
        .or_else(|_| backend.capture_screen())
        .map_err(|err| {
            SelectionError::InitError(format!("Wayland background capture failed: {err}"))
        })?;
    match crate::overlay::select_area_from_capture_with_gtk_on_monitor(
        &full_capture,
        Some(monitor_choice),
    ) {
        Ok(crate::overlay::OverlaySelection::Area(Some(area))) => {
            // Crop from the frozen background instead of capturing from the
            // live screen — avoids capturing our own overlay UI.
            let capture = crop_background(&full_capture, area.x, area.y, area.width, area.height)?;
            save_capture_to_temp_png(&capture).map(AreaCapturePathResult::Captured)
        }
        Ok(crate::overlay::OverlaySelection::Area(None)) => Err(SelectionError::Cancelled),
        Ok(crate::overlay::OverlaySelection::Recording(request)) => {
            Ok(AreaCapturePathResult::RecordingRequested(request))
        }
        Err(SelectionError::WindowCaptureRequested) => {
            // Prefer the shared C++/GNOME window-capture path (in-overlay picker
            // on GNOME; ScreenCast only as a last resort for non-GNOME Wayland).
            eprintln!(
                "[capture] Window capture requested from GTK overlay — launching window capture"
            );
            wait_for_layer_shell_overlay_to_unmap();
            capture_window_file_via_cpp().map(AreaCapturePathResult::Captured)
        }
        Err(SelectionError::OcrRequested(area)) => {
            eprintln!("[capture] OCR requested from GTK overlay — capturing area for OCR");
            // Crop from frozen background — the overlay is still fully on screen
            // when this error variant is returned, so a live capture would show it.
            let capture = crop_background(&full_capture, area.x, area.y, area.width, area.height)?;
            Ok(AreaCapturePathResult::OcrRequested(capture))
        }
        Err(e) => Err(e),
    }
}

fn capture_area_via_gtk_layer_shell_wlroots() -> Result<AreaCaptureResult, SelectionError> {
    match capture_area_file_via_gtk_layer_shell_wlroots()? {
        AreaCapturePathResult::Captured(path) => {
            let capture = load_capture_data_from_path(&path);
            let _ = std::fs::remove_file(&path);
            capture.map(AreaCaptureResult::Captured)
        }
        AreaCapturePathResult::OcrRequested(capture) => {
            Ok(AreaCaptureResult::OcrRequested(capture))
        }
        AreaCapturePathResult::Cancelled => Ok(AreaCaptureResult::Cancelled),
        AreaCapturePathResult::ScrollCaptured(path) => {
            let capture = load_capture_data_from_path(&path);
            let _ = std::fs::remove_file(&path);
            capture.map(AreaCaptureResult::ScrollCaptured)
        }
        AreaCapturePathResult::RecordingRequested(request) => {
            Ok(AreaCaptureResult::RecordingRequested(request))
        }
        AreaCapturePathResult::RecordingConfigUpdated => Ok(AreaCaptureResult::Cancelled),
    }
}

fn capture_crosshair_file_via_gtk_layer_shell_wlroots() -> Result<PathBuf, SelectionError> {
    force_wayland_gdk_for_layer_shell();

    if let Err(err) = gtk4::init() {
        if gdk::Display::default().is_none() {
            return Err(SelectionError::InitError(format!(
                "GTK init failed for monitor picker: {err}"
            )));
        }
    }
    let monitor_choice = crate::overlay::select_target_monitor_choice()?;

    let backend = WaylandBackend::new()
        .map_err(|err| SelectionError::InitError(format!("Wayland backend unavailable: {err}")))?;
    let full_capture = backend
        .capture_screen_for_selection_at(Some((monitor_choice.x, monitor_choice.y)))
        .or_else(|_| backend.capture_screen())
        .map_err(|err| {
            SelectionError::InitError(format!("Wayland background capture failed: {err}"))
        })?;
    let area = match crate::overlay::select_crosshair_from_capture_with_gtk_on_monitor(
        &full_capture,
        Some(monitor_choice),
    )? {
        OverlaySelection::Area(Some(area)) => area,
        OverlaySelection::Area(None) => return Err(SelectionError::Cancelled),
        OverlaySelection::Recording(_) => return Err(SelectionError::Cancelled),
    };
    // Crop from the frozen background — the overlay was just visible and may
    // not have been fully unmapped by the compositor yet.
    let capture = crop_background(&full_capture, area.x, area.y, area.width, area.height)?;

    save_capture_to_temp_png(&capture)
}

fn capture_crosshair_via_gtk_layer_shell_wlroots() -> Result<AreaCaptureResult, SelectionError> {
    let path = capture_crosshair_file_via_gtk_layer_shell_wlroots()?;
    let capture = load_capture_data_from_path(&path);
    let _ = std::fs::remove_file(&path);
    capture.map(AreaCaptureResult::Captured)
}

fn capture_screen_file_via_wlroots() -> Result<PathBuf, SelectionError> {
    let backend = WaylandBackend::new()
        .map_err(|err| SelectionError::InitError(format!("Wayland backend unavailable: {err}")))?;
    let capture = backend
        .capture_screen_for_selection_impl()
        .or_else(|_| backend.capture_screen())
        .map_err(|err| {
            SelectionError::InitError(format!("Wayland fullscreen capture failed: {err}"))
        })?;
    save_capture_to_temp_png(&capture)
}
