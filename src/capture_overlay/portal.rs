pub fn capture_still_via_portal(_interactive: bool) -> Result<CaptureData, SelectionError> {
    Err(SelectionError::InitError(
        "The bundled apexshot-capture helper is missing. Reinstall the Flatpak package to use permission-first screenshot capture.".into(),
    ))
}

fn capture_still_file_via_portal(interactive: bool) -> Result<PathBuf, SelectionError> {
    let capture = capture_still_via_portal(interactive)?;
    save_capture_to_temp_png(&capture)
}
