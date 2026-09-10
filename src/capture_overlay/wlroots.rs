fn wait_for_layer_shell_overlay_to_unmap() {
    // Layer-shell window destruction is acknowledged by the compositor just
    // after GTK returns. Give Hyprland/sway one frame to remove our overlay
    // before wlr-screencopy grabs the real screenshot, otherwise the capture
    // can include ApexShot's own UI.
    std::thread::sleep(std::time::Duration::from_millis(180));
}

fn selected_display(choice: &crate::overlay::MonitorChoice) -> CaptureDisplay {
    CaptureDisplay {
        x: choice.x,
        y: choice.y,
        width: choice.width,
        height: choice.height,
    }
}

fn capture_selected_monitor(
    choice: &crate::overlay::MonitorChoice,
) -> Result<CaptureData, SelectionError> {
    let backend = WaylandBackend::new()
        .map_err(|err| SelectionError::InitError(format!("Wayland backend unavailable: {err}")))?;
    backend
        .capture_screen_for_selection_at(Some((choice.x, choice.y)))
        .or_else(|_| backend.capture_screen())
        .map_err(|err| {
            SelectionError::InitError(format!("Wayland background capture failed: {err}"))
        })
}

fn recording_request_for_display(
    choice: &crate::overlay::MonitorChoice,
    menu: crate::overlay::CaptureMenuResult,
) -> RecordingRequest {
    let config = crate::config::load_config();
    RecordingRequest {
        x: choice.x,
        y: choice.y,
        width: choice.width,
        height: choice.height,
        record_type: RecordingType::Video,
        controls: config.rec_controls,
        mic: menu.microphone,
        speaker: menu.speaker,
        display_rec_time: true,
        hidpi: config.rec_hidpi,
        notifications: config.rec_notifications,
        cursor: config.rec_cursor,
        remember_selection: config.rec_remember_selection,
        dim_screen: config.rec_dim_screen,
        countdown: config.rec_countdown,
        video_format: 0,
        video_max_res: config.rec_video_max_res,
        video_fps: config.rec_video_fps,
        record_mono: config.rec_video_mono,
        open_editor: config.rec_video_open_editor,
        noise_suppression: config.rec_noise_suppression,
        gif_fps: config.rec_gif_fps,
        gif_quality: config.rec_gif_quality,
        gif_size_idx: config.rec_gif_size_idx,
        optimize_gif: config.rec_gif_optimize,
        fullscreen: true,
    }
}

/// GTK quick-capture fallback for wlroots desktops.
///
/// The display must be selected before the capture menu maps. Every action
/// then stays scoped to that display, exactly as in the Qt/GNOME flow.
fn open_quick_capture_via_gtk_layer_shell_wlroots() -> Result<AreaCapturePathResult, SelectionError>
{
    force_wayland_gdk_for_layer_shell();
    if let Err(err) = gtk4::init() {
        if gdk::Display::default().is_none() {
            return Err(SelectionError::InitError(format!(
                "GTK init failed for capture menu: {err}"
            )));
        }
    }

    let monitor_choice = crate::overlay::select_target_monitor_choice()?;
    let menu = crate::overlay::choose_capture_mode(&monitor_choice);
    // `choose_capture_mode` is a layer-shell surface.  Match the C++ flow by
    // giving the compositor one frame to unmap it before freezing the selected
    // output; otherwise the compact menu is baked into the Area background.
    if menu.action != crate::overlay::CaptureMenuAction::Cancel {
        wait_for_layer_shell_overlay_to_unmap();
    }
    match menu.action {
        crate::overlay::CaptureMenuAction::Cancel => Ok(AreaCapturePathResult::Cancelled),
        crate::overlay::CaptureMenuAction::Display if menu.recording => {
            Ok(AreaCapturePathResult::RecordingRequested(
                recording_request_for_display(&monitor_choice, menu),
            ))
        }
        crate::overlay::CaptureMenuAction::Display => {
            if menu.timer_seconds > 0 {
                // The area selector owns the visible timer pill. Full-display
                // capture has no selector surface, so preserve the delay here.
                std::thread::sleep(std::time::Duration::from_secs(u64::from(
                    menu.timer_seconds,
                )));
            }
            let capture = capture_selected_monitor(&monitor_choice)?;
            let path = save_capture_to_temp_png(&capture)?;
            Ok(AreaCapturePathResult::CapturedOnDisplay(
                path,
                selected_display(&monitor_choice),
            ))
        }
        crate::overlay::CaptureMenuAction::Window => Err(SelectionError::InitError(
            "Window capture is temporarily discontinued. Use area or display capture instead."
                .into(),
        )),
        crate::overlay::CaptureMenuAction::Area => {
            let full_capture = capture_selected_monitor(&monitor_choice)?;
            match crate::overlay::select_area_from_capture_with_gtk_from_capture_menu(
                &full_capture,
                monitor_choice.clone(),
                menu,
            ) {
                Ok(crate::overlay::OverlaySelection::Area(Some(area))) => {
                    let capture =
                        crop_background(&full_capture, area.x, area.y, area.width, area.height)?;
                    Ok(AreaCapturePathResult::CapturedOnDisplay(
                        save_capture_to_temp_png(&capture)?,
                        selected_display(&monitor_choice),
                    ))
                }
                Ok(crate::overlay::OverlaySelection::Area(None)) => {
                    Ok(AreaCapturePathResult::Cancelled)
                }
                Ok(crate::overlay::OverlaySelection::Recording(request)) => {
                    Ok(AreaCapturePathResult::RecordingRequested(request))
                }
                Err(SelectionError::OcrRequested(area)) => {
                    let capture =
                        crop_background(&full_capture, area.x, area.y, area.width, area.height)?;
                    Ok(AreaCapturePathResult::OcrRequested(capture))
                }
                Err(error) => Err(error),
            }
        }
    }
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
        AreaCapturePathResult::CapturedOnDisplay(path, display) => {
            let capture = load_capture_data_from_path(&path);
            let _ = std::fs::remove_file(&path);
            capture.map(|capture| AreaCaptureResult::CapturedOnDisplay(capture, display))
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
