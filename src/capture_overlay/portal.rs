/// Map portal/backend errors into selection errors, treating user dismissals
/// as cancellation rather than hard failures.
fn portal_display_error_to_selection(err: crate::backend::DisplayError) -> SelectionError {
    let msg = err.to_string();
    let lower = msg.to_ascii_lowercase();
    if lower.contains("cancel") {
        SelectionError::Cancelled
    } else {
        SelectionError::InitError(msg)
    }
}

/// Still-image capture for Flatpak / portal-only builds.
///
/// `interactive=true` opens the desktop Screenshot portal selector (area /
/// region / window UI owned by the portal). `interactive=false` requests a
/// non-interactive fullscreen still via the Screenshot portal first.
pub fn capture_still_via_portal(interactive: bool) -> Result<CaptureData, SelectionError> {
    let backend = WaylandBackend::new()
        .map_err(|err| SelectionError::InitError(format!("Wayland backend unavailable: {err}")))?;
    let capture = if interactive {
        eprintln!("[capture_overlay] portal-only: interactive Screenshot portal");
        backend.capture_area_via_portal_interactive_impl()
    } else {
        eprintln!("[capture_overlay] portal-only: non-interactive Screenshot portal fullscreen");
        backend.capture_screen_impl()
    }
    .map_err(portal_display_error_to_selection)?;
    Ok(capture)
}

fn capture_still_file_via_portal(interactive: bool) -> Result<PathBuf, SelectionError> {
    let capture = capture_still_via_portal(interactive)?;
    save_capture_to_temp_png(&capture)
}
