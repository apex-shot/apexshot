use crate::backend::portal_still::{PortalStillSession, StillError, StillMonitor};

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

fn still_key(choice: &crate::overlay::MonitorChoice) -> String {
    let topology = gdk::Display::default()
        .map(|display| {
            crate::overlay::monitor_picker::list_monitors(&display)
                .iter()
                .filter_map(|monitor| {
                    let geometry = monitor.geometry();
                    Some(format!(
                        "{}@{},{},{}x{}",
                        monitor.connector()?,
                        geometry.x(),
                        geometry.y(),
                        geometry.width(),
                        geometry.height()
                    ))
                })
                .collect::<Vec<_>>()
                .join(";")
        })
        .unwrap_or_default();
    format!(
        "gtk-wlroots-still|{}@{},{},{}x{}|{topology}",
        choice.connector, choice.x, choice.y, choice.width, choice.height
    )
}

fn still_selection_error(error: StillError) -> SelectionError {
    match error {
        StillError::Cancelled => SelectionError::Cancelled,
        error => SelectionError::InitError(format!("Monitor capture unavailable: {error}")),
    }
}

fn capture_for_choice(
    capture: CaptureData,
    choice: &crate::overlay::MonitorChoice,
) -> Result<CaptureData, SelectionError> {
    if (capture.output_origin_x, capture.output_origin_y) != (choice.x, choice.y) {
        return Err(SelectionError::InitError(
            "The selected display is no longer available".into(),
        ));
    }
    Ok(capture)
}

/// A still source bound to one display. Prepare it before any countdown or
/// selector so a consent prompt never lands after the timer.
struct MonitorStill {
    choice: crate::overlay::MonitorChoice,
    portal: Option<(tokio::runtime::Runtime, PortalStillSession)>,
}

impl MonitorStill {
    fn prepare(choice: crate::overlay::MonitorChoice) -> Result<Self, SelectionError> {
        if WaylandBackend::native_screencopy_available() {
            return Ok(Self {
                choice,
                portal: None,
            });
        }
        let runtime = tokio::runtime::Runtime::new().map_err(|err| {
            SelectionError::InitError(format!("Portal runtime unavailable: {err}"))
        })?;
        let monitor = StillMonitor {
            x: choice.x,
            y: choice.y,
            width: choice.width,
            height: choice.height,
        };
        let key = still_key(&choice);
        let session = runtime
            .block_on(PortalStillSession::prepare(&key, monitor, false))
            .map_err(still_selection_error)?;
        Ok(Self {
            choice,
            portal: Some((runtime, session)),
        })
    }

    fn capture(&self, play_sound: bool) -> Result<CaptureData, SelectionError> {
        match &self.portal {
            None => {
                let capture = WaylandBackend::capture_monitor_via_native_screencopy_at(Some((
                    self.choice.x,
                    self.choice.y,
                )))
                .ok_or_else(|| {
                    SelectionError::InitError(
                        "Native wlroots capture failed for the selected display".into(),
                    )
                })?
                .map_err(|err| {
                    SelectionError::InitError(format!("Native wlroots capture failed: {err}"))
                })?;
                let capture = capture_for_choice(capture, &self.choice)?;
                if play_sound {
                    crate::utils::capture_sound::play_shutter_sound_if_enabled();
                }
                Ok(capture)
            }
            Some((runtime, session)) => {
                let result = runtime.block_on(session.capture());
                result.map_err(|err| {
                    SelectionError::InitError(format!("Portal monitor capture failed: {err}"))
                })
            }
        }
    }
}

impl Drop for MonitorStill {
    fn drop(&mut self) {
        if let Some((runtime, session)) = self.portal.take() {
            runtime.block_on(session.close());
        }
    }
}

fn crop_selected_still(
    still: &MonitorStill,
    frozen: &CaptureData,
    area: crate::overlay::SelectionArea,
    countdown: Option<u32>,
) -> Result<CaptureData, SelectionError> {
    if let Some(seconds) = countdown {
        if seconds > 0 {
            std::thread::sleep(std::time::Duration::from_secs(u64::from(seconds)));
        }
        wait_for_layer_shell_overlay_to_unmap();
        let fresh = still.capture(false)?;
        if (fresh.width, fresh.height) != (frozen.width, frozen.height) {
            return Err(SelectionError::InitError(
                "The selected display changed resolution during capture. Please try again.".into(),
            ));
        }
        return crop_background(&fresh, area.x, area.y, area.width, area.height);
    }
    crop_background(frozen, area.x, area.y, area.width, area.height)
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
            let still = MonitorStill::prepare(monitor_choice.clone())?;
            if menu.timer_seconds > 0 {
                // The area selector owns the visible timer pill. Full-display
                // capture has no selector surface, so preserve the delay here.
                std::thread::sleep(std::time::Duration::from_secs(u64::from(
                    menu.timer_seconds,
                )));
            }
            let capture = still.capture(true)?;
            let path = save_capture_to_temp_png(&capture)?;
            Ok(AreaCapturePathResult::CapturedOnDisplay(
                path,
                selected_display(&monitor_choice),
            ))
        }
        crate::overlay::CaptureMenuAction::Window => Err(SelectionError::InitError(
            "The native wlroots window picker is unavailable. Use area or display capture here; \
             Quick Capture > Shot > Window is available on GNOME or X11."
                .into(),
        )),
        crate::overlay::CaptureMenuAction::Area => {
            let still = MonitorStill::prepare(monitor_choice.clone())?;
            let full_capture = still.capture(true)?;
            let countdown = (menu.timer_seconds > 0).then_some(0);
            match crate::overlay::select_area_from_capture_with_gtk_from_capture_menu(
                &full_capture,
                monitor_choice.clone(),
                menu,
            ) {
                Ok(crate::overlay::OverlaySelection::Area(Some(area))) => {
                    let capture = crop_selected_still(&still, &full_capture, area, countdown)?;
                    Ok(AreaCapturePathResult::CapturedOnDisplay(
                        save_capture_to_temp_png(&capture)?,
                        selected_display(&monitor_choice),
                    ))
                }
                Ok(crate::overlay::OverlaySelection::Area(None)) => {
                    Ok(AreaCapturePathResult::Cancelled)
                }
                Err(SelectionError::OcrRequested(area)) => {
                    let capture = crop_selected_still(&still, &full_capture, area, countdown)?;
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

    let still = MonitorStill::prepare(monitor_choice.clone())?;
    let full_capture = still.capture(true)?;
    let seconds = crate::config::load_config().screenshot_timer_interval;
    let countdown = (seconds > 0).then_some(seconds);
    match crate::overlay::select_area_from_capture_with_gtk_on_monitor(
        &full_capture,
        Some(monitor_choice),
    ) {
        Ok(crate::overlay::OverlaySelection::Area(Some(area))) => {
            // Crop from the frozen background instead of capturing from the
            // live screen — avoids capturing our own overlay UI.
            let capture = crop_selected_still(&still, &full_capture, area, countdown)?;
            save_capture_to_temp_png(&capture).map(AreaCapturePathResult::Captured)
        }
        Ok(crate::overlay::OverlaySelection::Area(None)) => Err(SelectionError::Cancelled),
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
            let capture = crop_selected_still(&still, &full_capture, area, countdown)?;
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

    let still = MonitorStill::prepare(monitor_choice.clone())?;
    let full_capture = still.capture(true)?;
    let seconds = crate::config::load_config().screenshot_timer_interval;
    let countdown = (seconds > 0).then_some(seconds);
    let area = match crate::overlay::select_crosshair_from_capture_with_gtk_on_monitor(
        &full_capture,
        Some(monitor_choice),
    )? {
        OverlaySelection::Area(Some(area)) => area,
        OverlaySelection::Area(None) => return Err(SelectionError::Cancelled),
    };
    // Crop from the frozen background — the overlay was just visible and may
    // not have been fully unmapped by the compositor yet.
    let capture = crop_selected_still(&still, &full_capture, area, countdown)?;

    save_capture_to_temp_png(&capture)
}

fn capture_crosshair_via_gtk_layer_shell_wlroots() -> Result<AreaCaptureResult, SelectionError> {
    let path = capture_crosshair_file_via_gtk_layer_shell_wlroots()?;
    let capture = load_capture_data_from_path(&path);
    let _ = std::fs::remove_file(&path);
    capture.map(AreaCaptureResult::Captured)
}

fn capture_screen_file_via_wlroots() -> Result<PathBuf, SelectionError> {
    force_wayland_gdk_for_layer_shell();
    if let Err(err) = gtk4::init() {
        if gdk::Display::default().is_none() {
            return Err(SelectionError::InitError(format!(
                "GTK init failed for monitor picker: {err}"
            )));
        }
    }
    let config = crate::config::load_config();
    let monitor_choice = crate::overlay::select_target_monitor_choice()?;
    let still = MonitorStill::prepare(monitor_choice)?;
    if config.screenshot_timer_interval > 0 {
        std::thread::sleep(std::time::Duration::from_secs(u64::from(
            config.screenshot_timer_interval,
        )));
    }
    let capture = still.capture(true)?;
    save_capture_to_temp_png(&capture)
}

#[cfg(test)]
mod monitor_still_tests {
    use super::*;

    fn choice(x: i32) -> crate::overlay::MonitorChoice {
        crate::overlay::MonitorChoice {
            index: 0,
            x,
            y: 0,
            width: 1920,
            height: 1080,
            connector: "test".into(),
            is_primary: true,
        }
    }

    fn capture_at(x: i32) -> CaptureData {
        let mut capture = CaptureData::new(vec![0; 4], 1, 1, PixelFormat::RGBA32);
        capture.output_origin_x = x;
        capture.output_origin_y = 0;
        capture
    }

    #[test]
    fn native_capture_must_come_from_the_selected_display() {
        assert!(capture_for_choice(capture_at(1920), &choice(1920)).is_ok());
        assert!(matches!(
            capture_for_choice(capture_at(0), &choice(1920)),
            Err(SelectionError::InitError(_))
        ));
    }

    #[test]
    fn user_cancellation_stays_cancellation() {
        assert!(matches!(
            still_selection_error(StillError::Cancelled),
            SelectionError::Cancelled
        ));
    }

    #[test]
    fn untimed_selection_crops_the_authorized_freeze_without_another_capture() {
        let still = MonitorStill {
            choice: choice(0),
            portal: None,
        };
        let frozen = CaptureData::new(vec![70; 4 * 4 * 4], 4, 4, PixelFormat::RGBA32);
        let cropped = crop_selected_still(
            &still,
            &frozen,
            crate::overlay::SelectionArea {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            },
            None,
        )
        .unwrap();
        assert_eq!((cropped.width, cropped.height), (2, 2));
        assert_eq!(cropped.pixels, vec![70; 2 * 2 * 4]);
    }
}
