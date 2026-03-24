//! Bridge to the native C++ Qt5 capture overlay binary (`apexshot-capture`).
//!
//! The binary is compiled from `capture-overlay/` by `build.rs` and placed
//! next to the Rust binary. This module finds and runs it as a subprocess,
//! parses the JSON output, and returns selection/capture data.
//!
//! Protocol:
//!   overlay mode: exit 0 + `{"x":N,"y":N,"width":N,"height":N}`
//!   capture mode: exit 0 + `{"path":"/tmp/...png",...}`
//!   exit 1 → cancelled by user
//!   exit 2 → error

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::{
    backend::{CaptureData, DisplayBackend, PixelFormat, WaylandBackend},
    capture::{save_capture, ImageFormat, SaveConfig},
    overlay::{SelectionArea, SelectionError, SelectionResult},
};

/// Find the `apexshot-capture` binary.
///
/// Search order:
/// 1. `APEXSHOT_CAPTURE_BIN` env variable (manual override).
/// 2. Same directory as the currently-running executable.
/// 3. Build-time output directory embedded by build.rs via `APEXSHOT_CAPTURE_BIN_DIR`.
/// 4. Common target profile directories relative to the exe (handles `cargo run` edge cases).
/// 5. PATH lookup.
fn find_capture_binary() -> Option<PathBuf> {
    // 1. Env override — highest priority for manual testing
    if let Some(p) = std::env::var_os("APEXSHOT_CAPTURE_BIN") {
        let path = PathBuf::from(p);
        if path.exists() {
            eprintln!(
                "[capture_overlay] Found apexshot-capture via env: {}",
                path.display()
            );
            return Some(path);
        }
    }

    // 2. Same directory as the running executable — useful for installed bundles.
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe.with_file_name("apexshot-capture");
        if candidate.exists() {
            eprintln!(
                "[capture_overlay] Found apexshot-capture next to exe: {}",
                candidate.display()
            );
            return Some(candidate);
        }
    }

    // 3. Build-time output directory embedded by build.rs
    if let Some(dir) = option_env!("APEXSHOT_CAPTURE_BIN_DIR") {
        let candidate = PathBuf::from(dir).join("apexshot-capture");
        if candidate.exists() {
            eprintln!(
                "[capture_overlay] Found apexshot-capture via build dir: {}",
                candidate.display()
            );
            return Some(candidate);
        }
    }

    // 4. Walk up from exe dir to find target/release or target/debug
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        while let Some(d) = dir {
            for profile in &["release", "debug"] {
                let candidate = d.join(profile).join("apexshot-capture");
                if candidate.exists() {
                    eprintln!(
                        "[capture_overlay] Found apexshot-capture in target/{}: {}",
                        profile,
                        candidate.display()
                    );
                    return Some(candidate);
                }
            }
            let candidate = d.join("apexshot-capture");
            if candidate.exists() && candidate != exe.with_file_name("apexshot-capture") {
                eprintln!(
                    "[capture_overlay] Found apexshot-capture in parent dir: {}",
                    candidate.display()
                );
                return Some(candidate);
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }

    // 5. PATH
    eprintln!("[capture_overlay] Searching PATH for apexshot-capture");
    which_in_path("apexshot-capture")
}

fn which_in_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(name);
            if full.exists() {
                Some(full)
            } else {
                None
            }
        })
    })
}

fn run_capture_binary(
    extra_args: &[&str],
    background_png: Option<&Path>,
) -> Result<Output, SelectionError> {
    let binary = find_capture_binary().ok_or_else(|| {
        SelectionError::InitError(
            "apexshot-capture binary not found. \
             Re-run `cargo build --release` to compile it, or check your PATH."
                .into(),
        )
    })?;

    let mut cmd = Command::new(&binary);
    cmd.env("QT_IM_MODULE", "compose")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    for arg in extra_args {
        cmd.arg(arg);
    }

    if let Some(bg) = background_png {
        cmd.arg("--background").arg(bg);
    }

    cmd.output().map_err(|e| {
        SelectionError::InitError(format!(
            "Failed to launch apexshot-capture ({}): {}",
            binary.display(),
            e
        ))
    })
}

/// Result of running the capture overlay — either an area selection or a
/// full window capture (when user clicks the Window toolbar button).
pub enum OverlayResult {
    /// User selected an area — coordinates to crop from.
    Area(SelectionArea),
    /// User clicked Window tool — full window pixel data already captured.
    Window(CaptureData),
    /// User cancelled.
    Cancelled,
}

/// Result of area capture initiation through the C++ overlay.
#[derive(Debug)]
pub enum AreaCaptureResult {
    Captured(CaptureData),
    ScrollCaptured(CaptureData),
    OcrRequested(CaptureData),
    RecordingRequested(RecordingRequest),
    Cancelled,
}

/// Recording request from the capture overlay.
#[derive(Debug, Clone)]
pub struct RecordingRequest {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub record_type: RecordingType,
    pub controls: bool,
    pub mic: bool,
    pub speaker: bool,
    pub clicks: bool,
    pub keystrokes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingType {
    Video,
    Gif,
}

pub enum AreaCapturePathResult {
    Captured(PathBuf),
    ScrollCaptured(PathBuf),
    OcrRequested(CaptureData),
    RecordingRequested(RecordingRequest),
    Cancelled,
}

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
        Some(1) | None => Ok(None),
        Some(3) => {
            // Window capture requested — signal by returning a special sentinel area
            Ok(Some(SelectionArea {
                x: i32::MIN,
                y: i32::MIN,
                width: i32::MIN,
                height: i32::MIN,
            }))
        }
        Some(4) => {
            // User switched to Area mode from window picker toolbar
            // Signal with sentinel x = i32::MIN + 1
            eprintln!("[capture_overlay] Window picker: switch to area mode requested");
            Ok(Some(SelectionArea {
                x: i32::MIN + 1,
                y: i32::MIN,
                width: i32::MIN,
                height: i32::MIN,
            }))
        }
        Some(5) => {
            // User switched to Fullscreen mode from window picker toolbar
            // Signal with sentinel x = i32::MIN + 2
            eprintln!("[capture_overlay] Window picker: switch to fullscreen mode requested");
            Ok(Some(SelectionArea {
                x: i32::MIN + 2,
                y: i32::MIN,
                width: i32::MIN,
                height: i32::MIN,
            }))
        }
        Some(code) => Err(SelectionError::InitError(format!(
            "apexshot-capture exited with code {code}"
        ))),
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
        Some(1) | None => Ok(None),
        Some(3) => {
            // Window capture requested via toolbar button (Wayland path)
            eprintln!(
                "[capture_overlay] Window capture requested — launching GNOME DBus window capture"
            );
            let _ = capture_window_via_cpp(); // result handled by caller via CaptureData path
            Ok(None) // signal caller to use capture_window_via_cpp instead
        }
        Some(code) => Err(SelectionError::InitError(format!(
            "apexshot-capture exited with code {code}"
        ))),
    }
}

fn save_capture_to_temp_png(
    capture: &CaptureData,
    prefix: &str,
) -> Result<PathBuf, SelectionError> {
    save_capture(
        capture,
        &SaveConfig::default()
            .with_output_dir(std::env::temp_dir())
            .with_format(ImageFormat::Png)
            .with_prefix(prefix),
    )
    .map_err(|e| SelectionError::InitError(format!("Failed to save temporary capture: {e}")))
}

pub fn capture_window_file_via_cpp() -> Result<PathBuf, SelectionError> {
    if WaylandBackend::is_supported() {
        eprintln!("[capture_overlay] capture_window_file: using Wayland ScreenCast window capture");
        let backend = WaylandBackend::new().map_err(|e| {
            SelectionError::InitError(format!("Failed to initialize Wayland backend: {e}"))
        })?;
        let capture = backend.capture_window(0).map_err(|e| {
            SelectionError::InitError(format!("Wayland window capture failed: {e}"))
        })?;
        return save_capture_to_temp_png(&capture, "window_");
    }

    eprintln!("[capture_overlay] capture_window_via_cpp: launching --window-capture");
    let output = run_capture_binary(&["--window-capture"], None)?;
    let exit_code = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!(
        "[capture_overlay] capture_window_via_cpp: exit={:?} stdout={:?} stderr={:?}",
        exit_code,
        stdout.trim(),
        stderr.trim()
    );

    match exit_code {
        Some(0) => parse_capture_screen_json(stdout.trim()),
        Some(1) | None => Err(SelectionError::Cancelled),
        Some(code) => Err(SelectionError::InitError(format!(
            "apexshot-capture --window-capture exited with code {code}"
        ))),
    }
}

pub fn capture_window_via_cpp() -> Result<CaptureData, SelectionError> {
    let path = capture_window_file_via_cpp()?;
    let capture = load_capture_data_from_path(&path);
    let _ = std::fs::remove_file(&path);
    capture
}

pub fn capture_screen_file_via_cpp() -> Result<PathBuf, SelectionError> {
    let output = run_capture_binary(&["--capture-screen"], None)?;

    match output.status.code() {
        Some(0) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_capture_screen_json(stdout.trim())
        }
        Some(1) | None => Err(SelectionError::Cancelled),
        Some(code) => Err(SelectionError::InitError(format!(
            "apexshot-capture --capture-screen exited with code {code}"
        ))),
    }
}

pub fn capture_screen_via_cpp() -> Result<CaptureData, SelectionError> {
    let path = capture_screen_file_via_cpp()?;
    let capture = load_capture_data_from_path(&path);
    let _ = std::fs::remove_file(&path);
    capture
}

pub fn capture_area_file_via_cpp() -> Result<AreaCapturePathResult, SelectionError> {
    eprintln!("[capture_overlay] capture_area_via_cpp: launching --area-init");
    let output = run_capture_binary(&["--area-init"], None)?;
    let exit_code = output.status.code();
    eprintln!(
        "[capture_overlay] capture_area_via_cpp: --area-init exited with code {:?}",
        exit_code
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!(
        "[capture_overlay] capture_area_via_cpp: stdout = {:?}",
        stdout.trim()
    );

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
                    Ok(AreaCapturePathResult::Captured(path))
                }
            }
        }
        Some(1) | None => {
            eprintln!("[capture_overlay] capture_area_via_cpp: cancelled or no exit code");
            Ok(AreaCapturePathResult::Cancelled)
        }
        Some(3) => {
            eprintln!(
                "[capture_overlay] Window capture requested from area toolbar — launching portal"
            );
            capture_window_file_via_cpp().map(AreaCapturePathResult::Captured)
        }
        Some(code) => Err(SelectionError::InitError(format!(
            "apexshot-capture --area-init exited with code {code}"
        ))),
    }
}

pub fn capture_area_via_cpp() -> Result<AreaCaptureResult, SelectionError> {
    match capture_area_file_via_cpp()? {
        AreaCapturePathResult::Captured(path) => {
            let capture = load_capture_data_from_path(&path);
            let _ = std::fs::remove_file(&path);
            capture.map(AreaCaptureResult::Captured)
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
        AreaCapturePathResult::Cancelled => Ok(AreaCaptureResult::Cancelled),
    }
}

fn load_capture_data_from_path(path: &Path) -> Result<CaptureData, SelectionError> {
    let image = image::open(path).map_err(|e| {
        SelectionError::InitError(format!(
            "Failed to load capture image from {}: {e}",
            path.display()
        ))
    })?;
    let rgba = image.into_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    Ok(CaptureData::new(
        rgba.into_raw(),
        width,
        height,
        PixelFormat::RGBA32,
    ))
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
    let clicks = extract_bool(json, "clicks").unwrap_or(false);
    let keystrokes = extract_bool(json, "keystrokes").unwrap_or(false);

    Ok(RecordingRequest {
        x,
        y,
        width,
        height,
        record_type,
        controls,
        mic,
        speaker,
        clicks,
        keystrokes,
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
            Ok(Some(SelectionArea {
                x,
                y,
                width,
                height,
            }))
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

#[cfg(test)]
mod tests {
    use super::{
        parse_capture_screen_json, parse_capture_screen_json_with_mode, parse_selection_json,
    };

    #[test]
    fn test_parse_normal() {
        let result = parse_selection_json(r#"{"x":10,"y":20,"width":300,"height":200}"#).unwrap();
        let area = result.unwrap();
        assert_eq!(area.x, 10);
        assert_eq!(area.y, 20);
        assert_eq!(area.width, 300);
        assert_eq!(area.height, 200);
    }

    #[test]
    fn test_parse_zero_size_is_error() {
        assert!(parse_selection_json(r#"{"x":0,"y":0,"width":0,"height":0}"#).is_err());
    }

    #[test]
    fn test_parse_capture_screen_path() {
        let path =
            parse_capture_screen_json(r#"{"path":"/tmp/demo.png","width":1920,"height":1080}"#)
                .unwrap();
        assert_eq!(path.to_string_lossy(), "/tmp/demo.png");
    }

    #[test]
    fn test_parse_capture_screen_path_with_mode() {
        let (path, mode) = parse_capture_screen_json_with_mode(
            r#"{"path":"/tmp/demo.png","width":1920,"height":1080,"mode":"ocr"}"#,
        )
        .unwrap();
        assert_eq!(path.to_string_lossy(), "/tmp/demo.png");
        assert_eq!(mode.as_deref(), Some("ocr"));
    }

    #[test]
    fn test_parse_selection_coords() {
        let parsed = parse_selection_json(r#"{"x":1,"y":2,"width":3,"height":4}"#).unwrap();
        let area = parsed.unwrap();
        assert_eq!(area.x, 1);
        assert_eq!(area.y, 2);
        assert_eq!(area.width, 3);
        assert_eq!(area.height, 4);
    }
}
