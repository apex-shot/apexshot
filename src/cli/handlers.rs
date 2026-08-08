use apexshot::{
    app_identity,
    backend::{CaptureData, DisplayBackend, WaylandBackend, X11Backend},
    capture::{save_capture, ImageFormat, SaveConfig},
    capture_overlay::{
        capture_area_via_cpp, capture_crosshair_via_cpp, capture_screen_via_cpp,
        is_launch_blocked_error, open_recording_ui_via_cpp, run_capture_overlay,
        AreaCapturePathResult, AreaCaptureResult,
    },
    hotkeys::ensure_desktop_entry_pub,
    ocr::{extract_text_from_capture, extract_text_from_path, OcrConfig},
    preview_launch::{launch_preview, show_preview_direct},
    recording::{
        run_overlay_recording_request, run_recording_with_controls, start_recording,
        RecordingConfig, RecordingControlsParams, StopAction,
    },
};
use std::path::PathBuf;

pub(crate) fn capture_daemon_action(capture_type: &str) -> Option<&'static str> {
    match capture_type {
        "area" => Some("capture_area"),
        "crosshair" => Some("capture_crosshair"),
        "previous-area" | "previous_area" => Some("capture_area"),
        "screen" => Some("capture_screen"),
        "window" => Some("capture_window"),
        _ => None,
    }
}

/// Map `apexshot record <type>` to a daemon D-Bus Trigger action name.
/// Control actions require a running daemon; start actions can
/// fall back to an in-process path when the daemon is unavailable.
pub(crate) fn record_daemon_action(record_type: &str) -> Option<&'static str> {
    match record_type {
        "ui" => Some("open_recording_ui"),
        "screen" => Some("record_screen"),
        "area" => Some("record_area"),
        // Must match `DaemonIpc::trigger` action names in daemon/mod.rs.
        "stop" => Some("recording_stop_save"),
        _ => None,
    }
}

pub(crate) fn is_record_control_action(record_type: &str) -> bool {
    matches!(record_type, "stop")
}

pub(crate) fn ensure_gio_desktop_env_for_capture() {
    if let Some(desktop_path) = app_identity::desktop_file_for_portal() {
        std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", desktop_path);
        std::env::set_var(
            "GIO_LAUNCHED_DESKTOP_FILE_PID",
            std::process::id().to_string(),
        );
        return;
    }

    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return;
    }

    let app_id =
        std::env::var("APEXSHOT_APP_ID").unwrap_or_else(|_| app_identity::app_id().to_string());

    if let Ok(desktop_path) = ensure_desktop_entry_pub(&app_id) {
        std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", &desktop_path);
        std::env::set_var(
            "GIO_LAUNCHED_DESKTOP_FILE_PID",
            std::process::id().to_string(),
        );
    }
}

pub(crate) fn run_capture(args: &[String]) {
    ensure_gio_desktop_env_for_capture();

    // Parse capture type
    let capture_type = args[2].as_str();

    // Parse options
    let mut output_path: Option<PathBuf> = None;
    let mut include_cursor = true;
    let mut use_jpeg = false;
    let mut jpeg_quality = 85;
    let mut prefix: Option<String> = None;
    let mut run_ocr = false;
    let mut ocr_lang: Option<String> = None;
    let mut ocr_min_conf: Option<i32> = None;
    let mut ocr_clipboard = true;

    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--output" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --output requires a path");
                    std::process::exit(1);
                }
                output_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--no-cursor" => {
                include_cursor = false;
                i += 1;
            }
            "--jpeg" => {
                use_jpeg = true;
                // Check if next arg is a number
                if i + 1 < args.len() {
                    if let Ok(q) = args[i + 1].parse::<u8>() {
                        jpeg_quality = q;
                        i += 2;
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            "--prefix" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --prefix requires text");
                    std::process::exit(1);
                }
                prefix = Some(args[i + 1].clone());
                i += 2;
            }
            "--ocr" => {
                run_ocr = true;
                i += 1;
            }
            "--lang" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --lang requires a language code");
                    std::process::exit(1);
                }
                ocr_lang = Some(args[i + 1].clone());
                i += 2;
            }
            "--min-conf" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --min-conf requires a number");
                    std::process::exit(1);
                }
                let value: i32 = match args[i + 1].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        eprintln!("Error: --min-conf requires a valid number");
                        std::process::exit(1);
                    }
                };
                ocr_min_conf = Some(value);
                i += 2;
            }
            "--no-clipboard" => {
                ocr_clipboard = false;
                i += 1;
            }
            _ => {
                eprintln!("Error: unknown option '{}'", args[i]);
                std::process::exit(1);
            }
        }
    }

    let capture: CaptureData = match capture_type {
        "screen" => match capture_screen_via_cpp() {
            Ok(capture) => {
                println!("Capturing full screen...");
                capture
            }
            Err(err) if is_launch_blocked_error(&err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
            Err(err) => {
                eprintln!("[capture] C++ fullscreen capture failed: {err}");
                std::process::exit(1);
            }
        },
        "area" => match capture_area_via_cpp() {
            Ok(AreaCaptureResult::Captured(capture)) => {
                println!("Captured area...");
                capture
            }
            Ok(AreaCaptureResult::ScrollCaptured(capture)) => {
                println!("Captured area (scroll)...");
                capture
            }
            Ok(AreaCaptureResult::OcrRequested(capture)) => {
                println!("Captured area (OCR requested)...");
                run_ocr = true;
                capture
            }
            Ok(AreaCaptureResult::RecordingRequested(request)) => {
                // Run recording as a subprocess to fully isolate GTK/layer-shell
                // state from the just-closed capture overlay.
                let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
                let json = serde_json::to_string(&request).unwrap();
                let status = std::process::Command::new(&exe)
                    .arg("record-from-overlay")
                    .arg(&json)
                    .status()
                    .expect("Failed to spawn recording subprocess");
                std::process::exit(status.code().unwrap_or(1));
            }
            Ok(AreaCaptureResult::Cancelled) => {
                eprintln!("Selection cancelled");
                std::process::exit(0);
            }
            Err(err) if is_launch_blocked_error(&err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
            Err(err) => {
                eprintln!("[capture] C++ area-init capture failed: {err}");
                std::process::exit(1);
            }
        },
        "crosshair" => match capture_crosshair_via_cpp() {
            Ok(AreaCaptureResult::Captured(capture)) => {
                println!("Captured crosshair area...");
                capture
            }
            Ok(AreaCaptureResult::Cancelled) => {
                eprintln!("Selection cancelled");
                std::process::exit(0);
            }
            Ok(_) => {
                eprintln!("Error: crosshair capture returned unsupported result");
                std::process::exit(1);
            }
            Err(err) if is_launch_blocked_error(&err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
            Err(err) => {
                eprintln!("[capture] C++ crosshair capture failed: {err}");
                std::process::exit(1);
            }
        },
        "window" => {
            eprintln!(
                "Error: window capture is temporarily discontinued.\n\
                 Use 'apexshot capture area' or 'apexshot capture screen' instead."
            );
            std::process::exit(1);
        }
        _ if WaylandBackend::is_supported() => {
            eprintln!("Error: unknown capture type '{}'", capture_type);
            crate::print_usage();
            std::process::exit(1);
        }
        _ if X11Backend::is_supported() => {
            println!("Using X11 backend...");

            match capture_type {
                "window" => {
                    eprintln!(
                        "Error: window capture is temporarily discontinued.\n\
                         Use 'capture area' or 'capture screen' instead."
                    );
                    std::process::exit(1);
                }
                _ => {
                    eprintln!("Error: unknown capture type '{}'", capture_type);
                    crate::print_usage();
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("Error: No supported display backend found");
            eprintln!("This application requires X11 or Wayland");
            std::process::exit(1);
        }
    };

    println!("Captured: {}x{}", capture.width, capture.height);
    println!(
        "Format: {:?} ({} bpp)",
        capture.format, capture.format.bits_per_pixel
    );
    if capture.cursor.is_some() {
        println!(
            "Cursor: captured ({})",
            if include_cursor {
                "will include"
            } else {
                "will exclude"
            }
        );
    }

    // Build save config
    let format = if use_jpeg {
        ImageFormat::Jpeg {
            quality: jpeg_quality,
        }
    } else {
        ImageFormat::Png
    };

    let mut config = SaveConfig::default()
        .with_format(format)
        .with_cursor(include_cursor);

    if let Some(path) = output_path {
        config = config.with_output_dir(path);
    }

    if let Some(p) = prefix {
        config = config.with_prefix(p);
    }

    // Save the capture (skip for OCR-only mode)
    let saved_path = if run_ocr {
        println!("Running OCR...");
        let mut ocr_config = OcrConfig::default().with_clipboard(ocr_clipboard);

        if let Some(lang) = ocr_lang {
            ocr_config = ocr_config.with_language(lang);
        }

        if let Some(conf) = ocr_min_conf {
            ocr_config = ocr_config.with_min_confidence(conf);
        }

        match extract_text_from_capture(&capture, &ocr_config) {
            Ok(result) => {
                match &result.source {
                    apexshot::ocr::ContentSource::QrCode => {
                        println!("QR code detected and decoded!");
                        println!("Content:");
                        println!("{}", "-".repeat(40));
                        println!("{}", result.text);
                        println!("{}", "-".repeat(40));
                    }
                    apexshot::ocr::ContentSource::Ocr { confidence } => {
                        println!("OCR successful!");
                        println!("Confidence: {}%", confidence);
                        println!("Extracted text:");
                        println!("{}", "-".repeat(40));
                        println!("{}", result.text);
                        println!("{}", "-".repeat(40));
                    }
                }
                if result.copied_to_clipboard {
                    println!("Copied to clipboard");
                }
            }
            Err(e) => {
                eprintln!("OCR failed: {}", e);
                std::process::exit(1);
            }
        }

        // OCR-only mode — exit after copying text to clipboard
        return;
    } else {
        match save_capture(&capture, &config) {
            Ok(path) => {
                println!("Saved to: {}", path.display());
                path
            }
            Err(e) => {
                eprintln!("Error saving capture: {}", e);
                std::process::exit(1);
            }
        }
    };

    // Hotkeys normally go through the daemon (which auto-uploads). When the
    // daemon is down we fall back to this in-process path — still honor
    // Settings → Cloud → "Upload after capture" so Ubuntu/GNOME keybindings
    // don't silently skip cloud upload.
    apexshot::cloud::upload::spawn_auto_upload_after_capture(saved_path.clone());

    // Keep preview in a subprocess on desktops where that preserves the
    // existing GTK isolation / shell tracking behavior. KDE Wayland uses a
    // direct launch path to avoid extra taskbar/loading artifacts.
    if let Err(e) = launch_preview(&saved_path) {
        eprintln!("Warning: Failed to launch preview overlay: {}", e);
        show_preview_direct(saved_path.clone());
    }
}

/// Save a CaptureData as a temp PNG for passing to the C++ overlay as background.
/// Returns the path if successful, None on failure (overlay will run without background).
pub(crate) fn save_temp_png(capture: &CaptureData) -> Option<std::path::PathBuf> {
    use image::{ImageBuffer, Rgba};

    let tmp = std::env::temp_dir().join(format!(
        "apexshot_bg_{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));

    let bytes_per_pixel = capture.format.bytes_per_pixel as usize;
    let stride = capture.stride as usize;
    let w = capture.width;
    let h = capture.height;

    use apexshot::backend::PixelFormat;
    let is_bgr = capture.format == PixelFormat::BGR24
        || capture.format == PixelFormat::BGR32
        || capture.format == PixelFormat::BGRA32;

    // Build RGBA pixel buffer from capture data
    let mut rgba: Vec<u8> = Vec::with_capacity((w * h * 4) as usize);
    for row in 0..h as usize {
        let row_start = row * stride;
        let row_end = row_start + w as usize * bytes_per_pixel;
        let row_data = &capture.pixels[row_start..row_end.min(capture.pixels.len())];
        for px in row_data.chunks(bytes_per_pixel) {
            if px.len() >= 4 {
                if is_bgr {
                    rgba.push(px[2]); // R (from BGR byte[2])
                    rgba.push(px[1]); // G
                    rgba.push(px[0]); // B (from BGR byte[0])
                    rgba.push(px[3]); // A
                } else {
                    rgba.push(px[0]); // R
                    rgba.push(px[1]); // G
                    rgba.push(px[2]); // B
                    rgba.push(px[3]); // A
                }
            } else if px.len() == 3 {
                if is_bgr {
                    rgba.push(px[2]);
                    rgba.push(px[1]);
                    rgba.push(px[0]);
                } else {
                    rgba.push(px[0]);
                    rgba.push(px[1]);
                    rgba.push(px[2]);
                }
                rgba.push(255);
            }
        }
    }

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(w, h, rgba)?;
    img.save(&tmp).ok()?;
    Some(tmp)
}

pub(crate) fn run_ocr(args: &[String]) {
    let image_path = &args[2];

    // Parse OCR options
    let mut ocr_lang: Option<String> = None;
    let mut ocr_min_conf: Option<i32> = None;
    let mut ocr_clipboard = true;

    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--lang" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --lang requires a language code");
                    std::process::exit(1);
                }
                ocr_lang = Some(args[i + 1].clone());
                i += 2;
            }
            "--min-conf" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --min-conf requires a number");
                    std::process::exit(1);
                }
                let value: i32 = match args[i + 1].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        eprintln!("Error: --min-conf requires a valid number");
                        std::process::exit(1);
                    }
                };
                ocr_min_conf = Some(value);
                i += 2;
            }
            "--no-clipboard" => {
                ocr_clipboard = false;
                i += 1;
            }
            _ => {
                eprintln!("Error: unknown option '{}'", args[i]);
                crate::print_usage();
                std::process::exit(1);
            }
        }
    }

    // Build OCR config
    let mut ocr_config = OcrConfig::default().with_clipboard(ocr_clipboard);

    if let Some(lang) = ocr_lang {
        ocr_config = ocr_config.with_language(lang);
    }

    if let Some(conf) = ocr_min_conf {
        ocr_config = ocr_config.with_min_confidence(conf);
    }

    // Run OCR
    println!("Running OCR on: {}", image_path);
    match extract_text_from_path(image_path, &ocr_config) {
        Ok(result) => {
            match &result.source {
                apexshot::ocr::ContentSource::QrCode => {
                    println!("QR code detected and decoded!");
                    println!("Content:");
                    println!("{}", "-".repeat(40));
                    println!("{}", result.text);
                    println!("{}", "-".repeat(40));
                }
                apexshot::ocr::ContentSource::Ocr { confidence } => {
                    println!("OCR successful!");
                    println!("Confidence: {}%", confidence);
                    println!("Extracted text:");
                    println!("{}", "-".repeat(40));
                    println!("{}", result.text);
                    println!("{}", "-".repeat(40));
                }
            }
            if result.copied_to_clipboard {
                println!("Copied to clipboard");
            }
        }
        Err(e) => {
            eprintln!("OCR failed: {}", e);
            std::process::exit(1);
        }
    }
}

pub(crate) async fn run_record(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    // Fedora: video recording intentionally unsupported (screenshots still work).
    if apexshot::recording::is_fedora_recording_unsupported() {
        apexshot::recording::refuse_fedora_recording()
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
        return Ok(());
    }

    let record_type = args[2].as_str();
    let mut output_path: Option<PathBuf> = None;
    let mut is_gif = false;
    let mut overlay_stop = false;

    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--output" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --output requires a path");
                    std::process::exit(1);
                }
                output_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--gif" => {
                is_gif = true;
                i += 1;
            }
            "--overlay-stop" => {
                overlay_stop = true;
                i += 1;
            }
            _ => {
                eprintln!("Error: unknown option '{}'", args[i]);
                std::process::exit(1);
            }
        }
    }

    let mut config = RecordingConfig::default();

    // Configure output path
    if let Some(p) = output_path {
        config.output_path = p;
        if is_gif
            && config
                .output_path
                .extension()
                .map(|e| e != "gif")
                .unwrap_or(true)
        {
            config.output_path.set_extension("gif");
        }
    } else if is_gif {
        config.output_path.set_extension("gif");
    }

    // Handle area selection if needed
    if record_type == "area" {
        // If on X11, launch overlay
        if std::env::var("WAYLAND_DISPLAY").is_err() && X11Backend::is_supported() {
            println!("Select an area to record by dragging the mouse. Press ESC to cancel.");

            let selection =
                run_capture_overlay(None).map_err(|e| format!("Selection failed: {}", e))?;
            if let apexshot::OverlaySelection::Area(Some(area)) = selection {
                config.x = Some(area.x);
                config.y = Some(area.y);
                config.width = Some(area.width as u32);
                config.height = Some(area.height as u32);
            } else {
                println!("Selection cancelled.");
                return Ok(());
            }
        } else {
            // Wayland area = portal selection (handled in start_recording)
            println!("Wayland detected: 'area' recording triggers system screen/window selection.");
        }
    } else if record_type == "ui" {
        match open_recording_ui_via_cpp()
            .map_err(|e| format!("Failed to open recording UI: {e}"))?
        {
            AreaCapturePathResult::RecordingRequested(request) => {
                let _ = run_overlay_recording_request(request)?;
                return Ok(());
            }
            AreaCapturePathResult::RecordingConfigUpdated | AreaCapturePathResult::Cancelled => {
                return Ok(());
            }
            other => {
                return Err(format!("Unexpected recording UI result: {other:?}").into());
            }
        }
    } else if record_type != "screen" {
        eprintln!(
            "Error: recording type '{record_type}' not supported \
             (use 'screen', 'area', 'ui', or control: stop|pause|resume|toggle-pause|restart|discard)"
        );
        std::process::exit(1);
    }

    let final_path = if overlay_stop {
        let params = RecordingControlsParams {
            capture_x: 0,
            capture_y: 0,
            capture_w: 0,
            capture_h: 0,
            is_fullscreen: true,
            show_timer: true,
            use_shell_mask: false,
            dim_screen: false,
            countdown_enabled: false,
            countdown_seconds: 3,
            session_id: None,
        };

        let controls_outcome = run_recording_with_controls(config, params)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error>
            })?;

        match controls_outcome {
            (path, StopAction::Discard) => {
                let _ = std::fs::remove_file(&path);
                return Ok(());
            }
            (path, StopAction::Save) => {
                eprintln!("Recording saved: {:?}", path);
                path
            }
        }
    } else {
        start_recording(config)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?
    };

    // Post-processing
    if let Some(ext) = final_path.extension() {
        if ext == "gif" {
            // For GIFs, we default to copying to clipboard (feature requested)
            if let Err(e) = apexshot::recording::copy_to_clipboard(&final_path) {
                eprintln!("Warning: Failed to copy GIF to clipboard: {}", e);
            }
        }
    }

    Ok(())
}
