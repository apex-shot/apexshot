fn classify_overlay_exit_code(
    code: Option<i32>,
) -> Result<Option<&'static str>, LaunchBlockedReason> {
    match code {
        Some(code) if code == OverlayExitCode::ForwardedToExistingOverlay as i32 => {
            Ok(Some("forwarded"))
        }
        Some(code) if code == OverlayExitCode::BlockedByBuiltinOverlay as i32 => {
            Err(LaunchBlockedReason::BuiltinOverlayActive)
        }
        _ => Ok(None),
    }
}

fn parse_area_capture_output_with_stderr(
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> Result<AreaCapturePathResult, SelectionError> {
    parse_area_capture_output_with_persist(exit_code, stdout, stderr, |request| {
        crate::recording::persist_overlay_recording_request_state(request)
    })
}

fn parse_area_capture_output_with_persist(
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
    persist_record_config: impl FnOnce(&RecordingRequest) -> anyhow::Result<()>,
) -> Result<AreaCapturePathResult, SelectionError> {
    match exit_code {
        Some(0) => {
            let mode = extract_string(stdout.trim(), "mode");
            if matches!(mode.as_deref(), Some("record")) {
                let request = parse_recording_json(stdout.trim())?;
                Ok(AreaCapturePathResult::RecordingRequested(request))
            } else {
                let (path, mode) = parse_capture_screen_json_with_mode(stdout.trim())?;
                if matches!(mode.as_deref(), Some("ocr")) {
                    match load_capture_data_from_path(&path) {
                        Ok(capture) => {
                            let _ = std::fs::remove_file(&path);
                            Ok(AreaCapturePathResult::OcrRequested(capture))
                        }
                        Err(e) => {
                            let _ = std::fs::remove_file(&path);
                            Err(e)
                        }
                    }
                } else if matches!(mode.as_deref(), Some("scroll")) {
                    Ok(AreaCapturePathResult::ScrollCaptured(path))
                } else {
                    let display = match (
                        extract_int(stdout.trim(), "screen_x"),
                        extract_int(stdout.trim(), "screen_y"),
                        extract_int(stdout.trim(), "screen_width"),
                        extract_int(stdout.trim(), "screen_height"),
                    ) {
                        (Some(x), Some(y), Some(width), Some(height))
                            if width > 0 && height > 0 =>
                        {
                            Some(CaptureDisplay {
                                x,
                                y,
                                width,
                                height,
                            })
                        }
                        _ => None,
                    };
                    Ok(match display {
                        Some(display) => AreaCapturePathResult::CapturedOnDisplay(path, display),
                        None => AreaCapturePathResult::Captured(path),
                    })
                }
            }
        }
        Some(code) if code == OverlayExitCode::RecordConfigUpdated as i32 => {
            let mode = extract_string(stdout.trim(), "mode");
            if matches!(mode.as_deref(), Some("record-config")) {
                let request = parse_recording_json(stdout.trim())?;
                persist_record_config(&request).map_err(|e| {
                    SelectionError::InitError(format!(
                        "Failed to persist recording overlay state: {e}"
                    ))
                })?;
                Ok(AreaCapturePathResult::RecordingConfigUpdated)
            } else {
                Err(SelectionError::InitError(format!(
                    "apexshot-capture --area-init exited with record-config code but stdout was not record-config: {stdout}"
                )))
            }
        }
        Some(1) | None => {
            eprintln!("[capture_overlay] capture_area_via_cpp: cancelled or no exit code");
            Ok(AreaCapturePathResult::Cancelled)
        }
        Some(code) if code == OverlayExitCode::ForwardedToExistingOverlay as i32 => {
            eprintln!(
                "[capture_overlay] capture_area_via_cpp: request forwarded to active overlay"
            );
            Ok(AreaCapturePathResult::Cancelled)
        }
        Some(3) => {
            // Legacy handoff: older capture binaries exited with code 3 when the
            // Window toolbar tool was clicked. Current overlays open an in-place
            // window picker and no longer emit this code. Keep the path for
            // mixed install/dev binaries.
            eprintln!(
                "[capture_overlay] Legacy exit 3 from area toolbar — launching window capture"
            );
            capture_window_file_via_cpp().map(AreaCapturePathResult::Captured)
        }
        Some(code) => Err(overlay_exit_error("--area-init", code, stderr)),
    }
}

fn parse_capture_screen_json(json: &str) -> Result<PathBuf, SelectionError> {
    let path = extract_string(json, "path").ok_or_else(|| {
        SelectionError::InitError(format!(
            "Failed to parse path from fullscreen capture output: '{json}'"
        ))
    })?;
    Ok(PathBuf::from(path))
}

fn parse_capture_screen_json_with_mode(
    json: &str,
) -> Result<(PathBuf, Option<String>), SelectionError> {
    let path = parse_capture_screen_json(json)?;
    let mode = extract_string(json, "mode");
    Ok((path, mode))
}

fn parse_recording_json(json: &str) -> Result<RecordingRequest, SelectionError> {
    let x = extract_int(json, "x").ok_or_else(|| SelectionError::InitError("Missing x".into()))?;
    let y = extract_int(json, "y").ok_or_else(|| SelectionError::InitError("Missing y".into()))?;
    let width = extract_int(json, "width")
        .ok_or_else(|| SelectionError::InitError("Missing width".into()))?;
    let height = extract_int(json, "height")
        .ok_or_else(|| SelectionError::InitError("Missing height".into()))?;

    let record_type_str = extract_string(json, "record_type").unwrap_or_else(|| "video".into());
    let record_type = match record_type_str.as_str() {
        "gif" => RecordingType::Gif,
        _ => RecordingType::Video,
    };

    let controls = extract_bool(json, "controls").unwrap_or(false);
    let mic = extract_bool(json, "mic").unwrap_or(false);
    let speaker = extract_bool(json, "speaker").unwrap_or(false);

    // General tab settings
    let display_rec_time = extract_bool(json, "display_rec_time").unwrap_or(false);
    let hidpi = extract_bool(json, "hidpi").unwrap_or(false);
    let notifications = extract_bool(json, "notifications").unwrap_or(true);
    let cursor = extract_bool(json, "cursor").unwrap_or(true);
    let remember_selection = extract_bool(json, "remember_selection").unwrap_or(false);
    let dim_screen = extract_bool(json, "dim_screen").unwrap_or(true);
    let countdown = extract_bool(json, "countdown").unwrap_or(true);

    // Video tab settings
    let video_format = extract_int(json, "video_format").unwrap_or(0).clamp(0, 0) as u8;
    let video_max_res = extract_int(json, "video_max_res").unwrap_or(0) as u8;
    let video_fps = extract_int(json, "video_fps").unwrap_or(2) as u8; // Default matches the overlay constructor
    let record_mono = extract_bool(json, "record_mono").unwrap_or(false);
    let open_editor = extract_bool(json, "open_editor").unwrap_or(false);
    let noise_suppression = extract_bool(json, "noise_suppression").unwrap_or(false);
    let gif_fps = extract_int(json, "gif_fps").unwrap_or(50).clamp(5, 60) as u8;
    let gif_quality = extract_float(json, "gif_quality")
        .unwrap_or(0.75)
        .clamp(0.0, 1.0);
    let gif_size_idx = extract_int(json, "gif_size_idx").unwrap_or(0).clamp(0, 3) as u8;
    let optimize_gif = extract_bool(json, "optimize_gif").unwrap_or(true);
    let fullscreen = extract_bool(json, "fullscreen").unwrap_or(false);

    Ok(RecordingRequest {
        x,
        y,
        width,
        height,
        record_type,
        controls,
        mic,
        speaker,
        display_rec_time,
        hidpi,
        notifications,
        cursor,
        remember_selection,
        dim_screen,
        countdown,
        video_format,
        video_max_res,
        video_fps,
        record_mono,
        open_editor,
        noise_suppression,
        gif_fps,
        gif_quality,
        gif_size_idx,
        optimize_gif,
        fullscreen,
    })
}

fn extract_bool(json: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{}\":", key);
    let start = json.find(&needle)? + needle.len();
    let rest = json[start..].trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Parse `{"x":N,"y":N,"width":N,"height":N}` produced by the C++ binary.
fn parse_selection_json(json: &str) -> SelectionResult {
    let x = extract_int(json, "x");
    let y = extract_int(json, "y");
    let w = extract_int(json, "width");
    let h = extract_int(json, "height");

    match (x, y, w, h) {
        (Some(x), Some(y), Some(width), Some(height)) if width > 0 && height > 0 => {
            Ok(OverlaySelection::Area(Some(SelectionArea {
                x,
                y,
                width,
                height,
            })))
        }
        _ => Err(SelectionError::InitError(format!(
            "Failed to parse selection from apexshot-capture output: '{json}'"
        ))),
    }
}

fn extract_int(json: &str, key: &str) -> Option<i32> {
    let needle = format!("\"{}\":", key);
    let start = json.find(&needle)? + needle.len();
    let rest = json[start..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn extract_float(json: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{}\":", key);
    let start = json.find(&needle)? + needle.len();
    let rest = json[start..].trim_start();
    let end = rest
        .find(|c: char| {
            !c.is_ascii_digit() && c != '-' && c != '.' && c != 'e' && c != 'E' && c != '+'
        })
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn extract_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let start = json.find(&needle)? + needle.len();
    let mut out = String::new();
    let mut escaped = false;

    for ch in json[start..].chars() {
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => out.push(other),
            }
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == '"' {
            return Some(out);
        }

        out.push(ch);
    }

    None
}
