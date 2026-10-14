use apexshot::{
    backend::{
        portal_still::{PortalStillSession, StillError, StillMonitor},
        CaptureData, DisplayBackend, DisplayError, X11Backend,
    },
    capture::capture_to_rgba_image,
    ocr::{self, ContentSource, OcrConfig, OcrError},
};
use gtk4::{gdk, prelude::*};
use image::{imageops, Rgba, RgbaImage};
use serde_json::{json, Map, Value};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const SCHEMA_VERSION: u32 = 1;
const MAX_ARGUMENTS: usize = 32;
const MAX_ARGUMENT_LENGTH: usize = 8192;
const MAX_IMAGE_WIDTH: u32 = 16384;
const MAX_IMAGE_HEIGHT: u32 = 16384;
const MAX_IMAGE_PIXELS: u64 = 50_000_000;
const MAX_CURSOR_PIXELS: u64 = 4_000_000;
const MAX_OCR_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_OCR_TEXT_BYTES: usize = 1024 * 1024;
const MAX_OCR_LANGUAGE_LENGTH: usize = 64;
const SKILL_SOURCE: &str = include_str!("../../agent-skills/apexshot/SKILL.md");

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ErrorCode {
    InvalidArguments,
    Cancelled,
    Unsupported,
    PermissionDenied,
    CaptureFailed,
    IoError,
    OcrFailed,
}

impl ErrorCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArguments => "invalid_arguments",
            Self::Cancelled => "cancelled",
            Self::Unsupported => "unsupported",
            Self::PermissionDenied => "permission_denied",
            Self::CaptureFailed => "capture_failed",
            Self::IoError => "io_error",
            Self::OcrFailed => "ocr_failed",
        }
    }

    fn exit_code(self) -> i32 {
        match self {
            Self::InvalidArguments => 2,
            Self::Cancelled => 3,
            Self::Unsupported => 4,
            Self::PermissionDenied => 5,
            Self::CaptureFailed | Self::IoError | Self::OcrFailed => 1,
        }
    }
}

#[derive(Debug)]
struct AgentError {
    code: ErrorCode,
    message: String,
}

impl AgentError {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArguments, message)
    }

    fn io(error: io::Error) -> Self {
        Self::new(ErrorCode::IoError, error.to_string())
    }
}

type AgentResult<T> = Result<T, AgentError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Region {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Debug)]
struct CaptureArgs {
    monitor: Option<String>,
    region: Option<Region>,
    output: PathBuf,
    include_cursor: bool,
    overwrite: bool,
}

#[derive(Debug)]
struct OcrArgs {
    path: PathBuf,
    language: String,
    min_confidence: u8,
}

#[derive(Clone, Debug)]
struct MonitorInfo {
    id: String,
    name: String,
    index: usize,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    scale: i32,
    primary: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct X11PhysicalBounds {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DesktopBackend {
    Wayland,
    X11,
    Unsupported,
}

impl DesktopBackend {
    fn as_str(self) -> &'static str {
        match self {
            Self::Wayland => "wayland",
            Self::X11 => "x11",
            Self::Unsupported => "unsupported",
        }
    }
}

pub(crate) fn run_cli(args: &[String]) -> i32 {
    let (exit_code, envelope) = execute(args);
    match serde_json::to_string(&envelope) {
        Ok(serialized) => {
            let stdout = io::stdout();
            let mut stdout = stdout.lock();
            if let Err(error) = writeln!(stdout, "{serialized}").and_then(|()| stdout.flush()) {
                eprintln!("Failed to write agent response: {error}");
                1
            } else {
                exit_code
            }
        }
        Err(error) => {
            eprintln!("Failed to encode agent response: {error}");
            1
        }
    }
}

pub(crate) fn execute(args: &[String]) -> (i32, Value) {
    let result = validate_arguments(args).and_then(|()| execute_command(args));
    match result {
        Ok(value) => success_envelope(value),
        Err(error) => error_envelope(error),
    }
}

fn success_envelope(result: Value) -> (i32, Value) {
    (
        0,
        json!({
            "schema_version": SCHEMA_VERSION,
            "ok": true,
            "result": result,
        }),
    )
}

fn error_envelope(error: AgentError) -> (i32, Value) {
    (
        error.code.exit_code(),
        json!({
            "schema_version": SCHEMA_VERSION,
            "ok": false,
            "error": {
                "code": error.code.as_str(),
                "message": error.message,
            },
        }),
    )
}

fn validate_arguments(args: &[String]) -> AgentResult<()> {
    if args.len() > MAX_ARGUMENTS {
        return Err(AgentError::invalid("Too many arguments"));
    }
    if args
        .iter()
        .any(|argument| argument.len() > MAX_ARGUMENT_LENGTH)
    {
        return Err(AgentError::invalid(
            "An argument exceeds the supported length",
        ));
    }
    Ok(())
}

fn execute_command(args: &[String]) -> AgentResult<Value> {
    match args.first().map(String::as_str) {
        Some("capabilities") if args.len() == 1 => Ok(capabilities()),
        Some("monitors") if args.len() == 1 => monitors_result(),
        Some("capture") => capture_command(args),
        Some("ocr") => ocr_command(args),
        Some("install-skill") => install_skill_command(args),
        Some("capabilities" | "monitors") => Err(AgentError::invalid(
            "This agent command does not accept additional arguments",
        )),
        Some(command) => Err(AgentError::invalid(format!(
            "Unknown agent command '{command}'"
        ))),
        None => Err(AgentError::invalid(
            "Expected capabilities, monitors, capture, ocr, or install-skill",
        )),
    }
}

fn detect_backend() -> DesktopBackend {
    match std::env::var("XDG_SESSION_TYPE")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "wayland" if std::env::var_os("WAYLAND_DISPLAY").is_some() => DesktopBackend::Wayland,
        "x11" if std::env::var_os("DISPLAY").is_some() => DesktopBackend::X11,
        "wayland" | "x11" => DesktopBackend::Unsupported,
        _ if std::env::var_os("WAYLAND_DISPLAY").is_some() => DesktopBackend::Wayland,
        _ if std::env::var_os("DISPLAY").is_some() => DesktopBackend::X11,
        _ => DesktopBackend::Unsupported,
    }
}

fn capabilities() -> Value {
    let backend = detect_backend();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    let session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let supported = backend != DesktopBackend::Unsupported;
    let reason = (!supported).then_some(
        "No Wayland or X11 display session was detected; monitor enumeration and capture are unavailable.",
    );
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "session": {
            "desktop": desktop,
            "session_type": session_type,
            "display_backend": backend.as_str(),
            "portal_only": apexshot::app_identity::portal_only(),
            "available": supported,
            "reason": reason,
        },
        "permissions": {
            "screen_capture": {
                "status": "unknown",
                "consent_may_be_required": backend == DesktopBackend::Wayland,
            },
        },
        "operations": {
            "capabilities": {"supported": true},
            "monitors": {"supported": supported, "reason": reason},
            "capture": {
                "supported": supported,
                "reason": reason,
                "formats": ["image/png"],
                "coordinate_space": "monitor-local-physical-pixels",
                "monitor_selection": "explicit-id-or-deterministic-primary",
                "consent_may_be_required": backend == DesktopBackend::Wayland,
                "limitations": [
                    "Wayland cursor inclusion is not available through the supported capture contract.",
                    "The portal may display its own source-consent UI; the selected source is verified before a result is returned.",
                ],
            },
            "ocr": {
                "supported": true,
                "engine_availability": "checked-when-requested",
                "clipboard": false,
                "automatic_upload": false,
                "maximum_image_pixels": MAX_IMAGE_PIXELS,
                "maximum_output_bytes": MAX_OCR_TEXT_BYTES,
            },
            "install-skill": {"supported": true, "opt_in": true},
        },
    })
}

fn monitors_result() -> AgentResult<Value> {
    let monitors = enumerate_monitors()?;
    let values = monitors
        .iter()
        .map(|monitor| {
            json!({
                "id": monitor.id,
                "name": monitor.name,
                "index": monitor.index,
                "x": monitor.x,
                "y": monitor.y,
                "width": monitor.width,
                "height": monitor.height,
                "scale": monitor.scale,
                "primary": monitor.primary,
                "primary_is_heuristic": true,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "monitors": values,
        "coordinate_space": "logical-pixels",
        "primary_selection": "monitor closest to logical origin (0,0)",
    }))
}

fn enumerate_monitors() -> AgentResult<Vec<MonitorInfo>> {
    gtk4::init().map_err(|error| {
        AgentError::new(
            ErrorCode::Unsupported,
            format!("GTK could not connect to the desktop display: {error}"),
        )
    })?;
    let display = gdk::Display::default().ok_or_else(|| {
        AgentError::new(
            ErrorCode::Unsupported,
            "No GDK display is available for monitor enumeration",
        )
    })?;
    let model = display.monitors();
    let mut raw = Vec::new();
    for index in 0..model.n_items() {
        let Some(object) = model.item(index) else {
            continue;
        };
        let Ok(monitor) = object.downcast::<gdk::Monitor>() else {
            continue;
        };
        let geometry = monitor.geometry();
        let connector = monitor.connector().map(|value| value.to_string());
        let model = monitor.model().map(|value| value.to_string());
        let name = model
            .clone()
            .or_else(|| connector.clone())
            .unwrap_or_else(|| format!("Display {}", index + 1));
        raw.push((
            index as usize,
            connector,
            model,
            name,
            geometry.x(),
            geometry.y(),
            geometry.width(),
            geometry.height(),
            monitor.scale_factor().max(1),
        ));
    }
    raw.sort_by(|left, right| {
        left.4
            .cmp(&right.4)
            .then_with(|| left.5.cmp(&right.5))
            .then_with(|| left.6.cmp(&right.6))
            .then_with(|| left.7.cmp(&right.7))
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.0.cmp(&right.0))
    });
    if raw.is_empty() {
        return Err(AgentError::new(
            ErrorCode::Unsupported,
            "The desktop did not report any attached monitors",
        ));
    }

    let primary_index = raw
        .iter()
        .enumerate()
        .min_by_key(|(_, monitor)| {
            (
                i64::from(monitor.4).unsigned_abs() + i64::from(monitor.5).unsigned_abs(),
                monitor.4,
                monitor.5,
            )
        })
        .map(|(index, _)| index)
        .unwrap_or(0);
    let mut connector_counts = HashMap::new();
    let mut bounds_counts = HashMap::new();
    for monitor in &raw {
        if let Some(connector) = &monitor.1 {
            *connector_counts.entry(connector.clone()).or_insert(0usize) += 1;
        }
        *bounds_counts
            .entry((monitor.4, monitor.5, monitor.6, monitor.7))
            .or_insert(0usize) += 1;
    }

    Ok(raw
        .into_iter()
        .enumerate()
        .map(
            |(index, (original_index, connector, _model, name, x, y, width, height, scale))| {
                let bounds = (x, y, width, height);
                let id = match connector {
                    Some(connector) if connector_counts.get(&connector) == Some(&1) => {
                        format!("connector:{connector}")
                    }
                    _ => {
                        let base = format!("monitor:{x},{y},{width},{height}");
                        if bounds_counts.get(&bounds) == Some(&1) {
                            base
                        } else {
                            format!("{base}:{index}")
                        }
                    }
                };
                MonitorInfo {
                    id,
                    name,
                    index: original_index,
                    x,
                    y,
                    width,
                    height,
                    scale,
                    primary: index == primary_index,
                }
            },
        )
        .collect())
}

fn capture_command(args: &[String]) -> AgentResult<Value> {
    let parsed = parse_capture_args(args)?;
    let path = absolute_path(&parsed.output)?;
    let path_string = path_to_string(&path)?;
    preflight_output_path(&path, parsed.overwrite)?;
    let monitors = enumerate_monitors()?;
    let monitor = select_monitor(&monitors, parsed.monitor.as_deref())?;
    let backend = detect_backend();
    if backend == DesktopBackend::Unsupported {
        return Err(AgentError::new(
            ErrorCode::Unsupported,
            "No supported desktop display backend is available",
        ));
    }
    if parsed.include_cursor && backend == DesktopBackend::Wayland {
        return Err(AgentError::new(
            ErrorCode::Unsupported,
            "Cursor inclusion is unavailable through the supported Wayland capture contract",
        ));
    }
    if monitors
        .iter()
        .filter(|candidate| candidate.x == monitor.x && candidate.y == monitor.y)
        .count()
        > 1
        && backend == DesktopBackend::Wayland
    {
        return Err(AgentError::new(
            ErrorCode::Unsupported,
            "The requested monitor shares a logical origin with another output; a unique source cannot be verified",
        ));
    }
    if monitors
        .iter()
        .filter(|candidate| {
            (candidate.x, candidate.y, candidate.width, candidate.height)
                == (monitor.x, monitor.y, monitor.width, monitor.height)
        })
        .count()
        > 1
    {
        return Err(AgentError::new(
            ErrorCode::Unsupported,
            "The requested monitor has duplicate geometry; a unique source cannot be verified",
        ));
    }
    validate_monitor_geometry(monitor, backend)?;
    let capture = capture_monitor(monitor, backend)?;
    validate_capture_data(&capture)?;
    if parsed.include_cursor && capture.cursor.is_none() {
        return Err(AgentError::new(
            ErrorCode::Unsupported,
            "The selected X11 backend did not provide cursor image data",
        ));
    }

    let mut image = capture_to_rgba_image(&capture).map_err(|error| {
        AgentError::new(
            ErrorCode::CaptureFailed,
            format!("Image conversion failed: {error}"),
        )
    })?;
    if parsed.include_cursor {
        let cursor = capture.cursor.as_ref().ok_or_else(|| {
            AgentError::new(ErrorCode::Unsupported, "Cursor image data is unavailable")
        })?;
        composite_cursor(&mut image, cursor)?;
    }
    let output_image = match parsed.region {
        Some(region) => crop_image(&image, region)?,
        None => image,
    };
    write_png_exact(&path, &output_image, parsed.overwrite)?;

    let mut result = Map::new();
    result.insert("path".into(), json!(path_string));
    result.insert("width".into(), json!(output_image.width()));
    result.insert("height".into(), json!(output_image.height()));
    result.insert("mime_type".into(), json!("image/png"));
    result.insert("monitor_id".into(), json!(monitor.id));
    result.insert(
        "coordinate_space".into(),
        json!("monitor-local-physical-pixels"),
    );
    result.insert(
        "consent_may_be_required".into(),
        json!(backend == DesktopBackend::Wayland),
    );
    result.insert(
        "monitor_bounds_logical".into(),
        json!({
            "x": monitor.x,
            "y": monitor.y,
            "width": monitor.width,
            "height": monitor.height,
        }),
    );
    result.insert(
        "physical_scale".into(),
        json!({
            "x": f64::from(capture.width) / f64::from(monitor.width),
            "y": f64::from(capture.height) / f64::from(monitor.height),
        }),
    );
    if let Some(region) = parsed.region {
        result.insert(
            "region".into(),
            json!({
                "x": region.x,
                "y": region.y,
                "width": region.width,
                "height": region.height,
            }),
        );
    }
    Ok(Value::Object(result))
}

fn parse_capture_args(args: &[String]) -> AgentResult<CaptureArgs> {
    let mut monitor = None;
    let mut region = None;
    let mut output = None;
    let mut include_cursor = false;
    let mut overwrite = false;
    let mut seen = HashMap::new();
    let mut index = 1;
    while index < args.len() {
        let option = args[index].as_str();
        match option {
            "--monitor" | "--region" | "--output" => {
                if seen.insert(option, ()).is_some() {
                    return Err(AgentError::invalid(format!(
                        "{option} may only be specified once"
                    )));
                }
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| AgentError::invalid(format!("{option} requires a value")))?;
                if value.starts_with("--") || value.is_empty() {
                    return Err(AgentError::invalid(format!("{option} requires a value")));
                }
                match option {
                    "--monitor" => monitor = Some(value.clone()),
                    "--region" => region = Some(parse_region(value)?),
                    "--output" => output = Some(PathBuf::from(value)),
                    _ => return Err(AgentError::invalid("Unknown capture option")),
                }
                index += 2;
            }
            "--include-cursor" => {
                if include_cursor {
                    return Err(AgentError::invalid(
                        "--include-cursor may only be specified once",
                    ));
                }
                include_cursor = true;
                index += 1;
            }
            "--overwrite" => {
                if overwrite {
                    return Err(AgentError::invalid(
                        "--overwrite may only be specified once",
                    ));
                }
                overwrite = true;
                index += 1;
            }
            _ => {
                return Err(AgentError::invalid(format!(
                    "Unknown capture option '{option}'"
                )))
            }
        }
    }
    let output = output.ok_or_else(|| AgentError::invalid("--output is required"))?;
    validate_png_path(&output)?;
    Ok(CaptureArgs {
        monitor,
        region,
        output,
        include_cursor,
        overwrite,
    })
}

fn parse_region(value: &str) -> AgentResult<Region> {
    let values = value.split(',').collect::<Vec<_>>();
    if values.len() != 4 {
        return Err(AgentError::invalid(
            "--region must be x,y,width,height using monitor-local physical pixels",
        ));
    }
    let parsed = values
        .iter()
        .map(|part| {
            part.parse::<u32>()
                .map_err(|_| AgentError::invalid("Region values must be non-negative integers"))
        })
        .collect::<AgentResult<Vec<_>>>()?;
    let region = Region {
        x: parsed[0],
        y: parsed[1],
        width: parsed[2],
        height: parsed[3],
    };
    if region.width == 0 || region.height == 0 {
        return Err(AgentError::invalid(
            "Region width and height must be greater than zero",
        ));
    }
    if region.x.checked_add(region.width).is_none() || region.y.checked_add(region.height).is_none()
    {
        return Err(AgentError::invalid(
            "Region bounds exceed the supported range",
        ));
    }
    Ok(region)
}

fn select_monitor<'a>(
    monitors: &'a [MonitorInfo],
    requested: Option<&str>,
) -> AgentResult<&'a MonitorInfo> {
    if let Some(requested) = requested {
        return monitors
            .iter()
            .find(|monitor| monitor.id == requested)
            .ok_or_else(|| AgentError::invalid(format!("Unknown monitor id '{requested}'")));
    }
    monitors
        .iter()
        .find(|monitor| monitor.primary)
        .ok_or_else(|| AgentError::new(ErrorCode::Unsupported, "No primary monitor is available"))
}

fn capture_monitor(monitor: &MonitorInfo, backend: DesktopBackend) -> AgentResult<CaptureData> {
    match backend {
        DesktopBackend::X11 => {
            let bounds = x11_physical_bounds(monitor)?;
            let backend = X11Backend::new().map_err(map_display_error)?;
            backend
                .capture_area(bounds.x, bounds.y, bounds.width, bounds.height)
                .map_err(map_display_error)
        }
        DesktopBackend::Wayland => {
            if native_wayland_capture_enabled() {
                match apexshot::backend::screencopy::capture_at(monitor.x, monitor.y) {
                    Ok(Some(capture)) if native_capture_matches_monitor(&capture, monitor) => {
                        return Ok(capture);
                    }
                    Ok(Some(capture)) => eprintln!(
                        "[agent] Native capture did not match monitor {}: expected origin=({}, {}) logical={}x{}, got origin=({}, {}) frame={}x{}; trying a monitor-bound portal session",
                        monitor.id,
                        monitor.x,
                        monitor.y,
                        monitor.width,
                        monitor.height,
                        capture.output_origin_x,
                        capture.output_origin_y,
                        capture.width,
                        capture.height
                    ),
                    Ok(None) => eprintln!(
                        "[agent] Native capture is unavailable for monitor {}; trying a monitor-bound portal session",
                        monitor.id
                    ),
                    Err(error) => eprintln!(
                        "[agent] Native capture failed for monitor {}: {error}; trying a monitor-bound portal session",
                        monitor.id
                    ),
                }
            }
            capture_via_portal(monitor)
        }
        DesktopBackend::Unsupported => Err(AgentError::new(
            ErrorCode::Unsupported,
            "No supported desktop display backend is available",
        )),
    }
}

fn native_wayland_capture_enabled() -> bool {
    should_use_native_wayland_capture(
        apexshot::app_identity::portal_only(),
        std::env::var_os("APEXSHOT_WAYLAND_SCREENSHOT_PORTAL").is_some(),
        std::env::var_os("APEXSHOT_DISABLE_WLR_SCREENCOPY").is_some(),
    )
}

fn should_use_native_wayland_capture(
    portal_only: bool,
    force_portal: bool,
    native_capture_disabled: bool,
) -> bool {
    !portal_only && !force_portal && !native_capture_disabled
}

fn native_capture_matches_monitor(capture: &CaptureData, monitor: &MonitorInfo) -> bool {
    capture.output_origin_x == monitor.x
        && capture.output_origin_y == monitor.y
        && capture.output_scale > 0
        && frame_matches_monitor_aspect(
            capture.width,
            capture.height,
            monitor.width,
            monitor.height,
        )
}

fn validate_monitor_geometry(monitor: &MonitorInfo, backend: DesktopBackend) -> AgentResult<()> {
    if monitor.width <= 0
        || monitor.height <= 0
        || monitor.width as u32 > MAX_IMAGE_WIDTH
        || monitor.height as u32 > MAX_IMAGE_HEIGHT
        || (monitor.width as u64) * (monitor.height as u64) > MAX_IMAGE_PIXELS
    {
        return Err(AgentError::new(
            ErrorCode::Unsupported,
            "The selected monitor has unsupported logical dimensions",
        ));
    }
    if backend == DesktopBackend::Wayland {
        let scale = monitor.scale.max(1) as u64;
        let estimated_pixels = (monitor.width as u64)
            .checked_mul(monitor.height as u64)
            .and_then(|pixels| pixels.checked_mul(scale))
            .and_then(|pixels| pixels.checked_mul(scale));
        if estimated_pixels.is_none_or(|pixels| pixels > MAX_IMAGE_PIXELS) {
            return Err(AgentError::new(
                ErrorCode::Unsupported,
                "The selected monitor's estimated physical frame exceeds the supported size limit",
            ));
        }
    }
    if backend == DesktopBackend::X11 {
        x11_physical_bounds(monitor)?;
    }
    Ok(())
}

fn x11_physical_bounds(monitor: &MonitorInfo) -> AgentResult<X11PhysicalBounds> {
    if monitor.scale <= 0 {
        return Err(AgentError::new(
            ErrorCode::Unsupported,
            "The X11 monitor has an invalid GDK scale factor",
        ));
    }
    let scale_coordinate = |value: i32| {
        value.checked_mul(monitor.scale).ok_or_else(|| {
            AgentError::new(
                ErrorCode::Unsupported,
                "The scaled X11 monitor bounds overflow the supported coordinate range",
            )
        })
    };
    let bounds = X11PhysicalBounds {
        x: scale_coordinate(monitor.x)?,
        y: scale_coordinate(monitor.y)?,
        width: scale_coordinate(monitor.width)?,
        height: scale_coordinate(monitor.height)?,
    };
    if bounds.x < 0
        || bounds.y < 0
        || bounds.x > i16::MAX as i32
        || bounds.y > i16::MAX as i32
        || bounds.width <= 0
        || bounds.height <= 0
        || bounds.width > u16::MAX as i32
        || bounds.height > u16::MAX as i32
        || bounds.width as u32 > MAX_IMAGE_WIDTH
        || bounds.height as u32 > MAX_IMAGE_HEIGHT
        || (bounds.width as u64) * (bounds.height as u64) > MAX_IMAGE_PIXELS
    {
        return Err(AgentError::new(
            ErrorCode::Unsupported,
            "The scaled monitor bounds exceed the X11 capture range",
        ));
    }
    Ok(bounds)
}

fn portal_frame_matches_monitor(capture: &CaptureData, monitor: &MonitorInfo) -> bool {
    frame_matches_monitor_aspect(capture.width, capture.height, monitor.width, monitor.height)
}

fn frame_matches_monitor_aspect(
    frame_width: u32,
    frame_height: u32,
    logical_width: i32,
    logical_height: i32,
) -> bool {
    if frame_width == 0 || frame_height == 0 || logical_width <= 0 || logical_height <= 0 {
        return false;
    }
    let frame_width = u64::from(frame_width);
    let frame_height = u64::from(frame_height);
    let logical_width = logical_width as u64;
    let logical_height = logical_height as u64;
    (frame_width * logical_height).abs_diff(frame_height * logical_width)
        <= frame_width + frame_height
}

fn capture_via_portal(monitor: &MonitorInfo) -> AgentResult<CaptureData> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            AgentError::new(
                ErrorCode::Unsupported,
                format!("Portal runtime is unavailable: {error}"),
            )
        })?;
    let still_monitor = StillMonitor {
        x: monitor.x,
        y: monitor.y,
        width: monitor.width,
        height: monitor.height,
    };
    let key = format!(
        "agent-still|{}@{},{},{}x{}",
        monitor.id, monitor.x, monitor.y, monitor.width, monitor.height
    );
    let session = runtime
        .block_on(PortalStillSession::prepare(&key, still_monitor, false))
        .map_err(map_still_error)?;
    let source_matches = session.position == Some((monitor.x, monitor.y))
        && session.size == Some((monitor.width, monitor.height));
    if !source_matches {
        runtime.block_on(session.close());
        return Err(AgentError::new(
            ErrorCode::Unsupported,
            "The screen-sharing portal did not provide enough metadata to verify the selected monitor",
        ));
    }
    let result = runtime
        .block_on(session.capture())
        .map_err(map_display_error);
    runtime.block_on(session.close());
    let capture = result?;
    if !portal_frame_matches_monitor(&capture, monitor) {
        return Err(AgentError::new(
            ErrorCode::Unsupported,
            "The portal frame geometry does not match the selected monitor",
        ));
    }
    Ok(capture)
}

fn map_still_error(error: StillError) -> AgentError {
    match error {
        StillError::Cancelled => {
            AgentError::new(ErrorCode::Cancelled, "Screen sharing was cancelled")
        }
        StillError::Capture(error) => map_display_error(error),
        StillError::Portal(ashpd::Error::Portal(ashpd::PortalError::Cancelled(_))) => {
            AgentError::new(ErrorCode::Cancelled, "Screen sharing was cancelled")
        }
        StillError::Portal(ashpd::Error::Portal(ashpd::PortalError::NotAllowed(message))) => {
            AgentError::new(ErrorCode::PermissionDenied, message)
        }
        StillError::Portal(ashpd::Error::PortalNotFound(_))
        | StillError::Portal(ashpd::Error::RequiresVersion(_, _)) => AgentError::new(
            ErrorCode::Unsupported,
            "The desktop does not provide a compatible screen-sharing portal",
        ),
        StillError::Portal(error) => AgentError::new(ErrorCode::CaptureFailed, error.to_string()),
    }
}

fn map_display_error(error: DisplayError) -> AgentError {
    match error {
        DisplayError::Cancelled => AgentError::new(ErrorCode::Cancelled, "Capture was cancelled"),
        DisplayError::UnsupportedBackend(message) | DisplayError::InitializationError(message) => {
            AgentError::new(ErrorCode::Unsupported, message)
        }
        DisplayError::InvalidArea(message) => AgentError::invalid(message),
        DisplayError::IoError(error) => AgentError::io(error),
        DisplayError::CaptureError(message) | DisplayError::PortalError(message) => {
            AgentError::new(ErrorCode::CaptureFailed, message)
        }
    }
}

fn validate_capture_data(capture: &CaptureData) -> AgentResult<()> {
    if capture.width == 0
        || capture.height == 0
        || capture.width > MAX_IMAGE_WIDTH
        || capture.height > MAX_IMAGE_HEIGHT
        || u64::from(capture.width) * u64::from(capture.height) > MAX_IMAGE_PIXELS
    {
        return Err(AgentError::new(
            ErrorCode::CaptureFailed,
            "The captured frame has unsupported dimensions",
        ));
    }
    let bytes_per_pixel = u32::from(capture.format.bytes_per_pixel);
    let minimum_stride = capture
        .width
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| AgentError::new(ErrorCode::CaptureFailed, "Capture stride overflow"))?;
    if capture.stride < minimum_stride {
        return Err(AgentError::new(
            ErrorCode::CaptureFailed,
            "The captured frame has an invalid row stride",
        ));
    }
    let required_bytes = u64::from(capture.stride)
        .checked_mul(u64::from(capture.height))
        .ok_or_else(|| AgentError::new(ErrorCode::CaptureFailed, "Capture size overflow"))?;
    if required_bytes > capture.pixels.len() as u64 {
        return Err(AgentError::new(
            ErrorCode::CaptureFailed,
            "The captured frame pixel buffer is truncated",
        ));
    }
    Ok(())
}

fn composite_cursor(
    image: &mut RgbaImage,
    cursor: &apexshot::backend::CursorData,
) -> AgentResult<()> {
    let expected_bytes = u64::from(cursor.width)
        .checked_mul(u64::from(cursor.height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| AgentError::new(ErrorCode::CaptureFailed, "Cursor size overflow"))?;
    if expected_bytes / 4 > MAX_CURSOR_PIXELS || expected_bytes > cursor.pixels.len() as u64 {
        return Err(AgentError::new(
            ErrorCode::CaptureFailed,
            "The cursor image has unsupported dimensions or a truncated pixel buffer",
        ));
    }
    for cursor_y in 0..cursor.height {
        let image_y = i64::from(cursor.y) + i64::from(cursor_y);
        if image_y < 0 || image_y >= i64::from(image.height()) {
            continue;
        }
        for cursor_x in 0..cursor.width {
            let image_x = i64::from(cursor.x) + i64::from(cursor_x);
            if image_x < 0 || image_x >= i64::from(image.width()) {
                continue;
            }
            let cursor_index =
                (u64::from(cursor_y) * u64::from(cursor.width) + u64::from(cursor_x)) * 4;
            let cursor_pixel = &cursor.pixels[cursor_index as usize..cursor_index as usize + 4];
            let alpha = u32::from(cursor_pixel[3]);
            if alpha == 0 {
                continue;
            }
            let pixel = image.get_pixel_mut(image_x as u32, image_y as u32);
            let inverse_alpha = 255 - alpha;
            *pixel = Rgba([
                ((u32::from(cursor_pixel[0]) * alpha + u32::from(pixel[0]) * inverse_alpha) / 255)
                    as u8,
                ((u32::from(cursor_pixel[1]) * alpha + u32::from(pixel[1]) * inverse_alpha) / 255)
                    as u8,
                ((u32::from(cursor_pixel[2]) * alpha + u32::from(pixel[2]) * inverse_alpha) / 255)
                    as u8,
                255,
            ]);
        }
    }
    Ok(())
}

fn crop_image(image: &RgbaImage, region: Region) -> AgentResult<RgbaImage> {
    let right = region
        .x
        .checked_add(region.width)
        .ok_or_else(|| AgentError::invalid("Region bounds exceed the supported range"))?;
    let bottom = region
        .y
        .checked_add(region.height)
        .ok_or_else(|| AgentError::invalid("Region bounds exceed the supported range"))?;
    if region.width == 0 || region.height == 0 || right > image.width() || bottom > image.height() {
        return Err(AgentError::invalid(
            "The monitor-local physical-pixel region lies outside the captured frame",
        ));
    }
    Ok(imageops::crop_imm(image, region.x, region.y, region.width, region.height).to_image())
}

fn validate_png_path(path: &Path) -> AgentResult<()> {
    if path.file_name().is_none() {
        return Err(AgentError::invalid("--output must name an exact file"));
    }
    if path
        .to_str()
        .is_some_and(|path| path.ends_with(std::path::MAIN_SEPARATOR))
    {
        return Err(AgentError::invalid(
            "--output must not end in a directory separator",
        ));
    }
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    {
        return Err(AgentError::invalid(
            "Only PNG output is supported; --output must have a .png extension",
        ));
    }
    Ok(())
}

fn absolute_path(path: &Path) -> AgentResult<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .map_err(AgentError::io)
    }
}

fn preflight_output_path(path: &Path, overwrite: bool) -> AgentResult<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent_metadata = fs::metadata(parent).map_err(AgentError::io)?;
    if !parent_metadata.is_dir() {
        return Err(AgentError::io(io::Error::new(
            io::ErrorKind::NotADirectory,
            "Output parent is not a directory",
        )));
    }
    match fs::symlink_metadata(path) {
        Ok(_) if !overwrite => Err(AgentError::io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("Output already exists: {}", path.display()),
        ))),
        Ok(metadata) if metadata.file_type().is_dir() => Err(AgentError::io(io::Error::new(
            io::ErrorKind::IsADirectory,
            "Output path names a directory",
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AgentError::io(error)),
    }
}

fn path_to_string(path: &Path) -> AgentResult<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| AgentError::new(ErrorCode::IoError, "The output path is not valid UTF-8"))
}

fn write_png_exact(path: &Path, image: &RgbaImage, overwrite: bool) -> AgentResult<()> {
    validate_png_path(path)?;
    write_exact(path, overwrite, 0o600, |file| {
        apexshot::utils::png_io::encode(
            file,
            image.as_raw(),
            image.width(),
            image.height(),
            image::ColorType::Rgba8,
        )
        .map_err(map_image_error)
    })
}

fn write_exact(
    path: &Path,
    overwrite: bool,
    mode: u32,
    write_content: impl FnOnce(&mut File) -> AgentResult<()>,
) -> AgentResult<()> {
    let mode = mode & 0o777;
    let (temporary, mut file) = create_temporary_file(path, mode)?;
    let result = write_file(&mut file, mode, write_content);
    drop(file);
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if overwrite {
        if let Err(error) = fs::rename(&temporary, path) {
            let _ = fs::remove_file(&temporary);
            return Err(AgentError::io(error));
        }
    } else {
        if let Err(error) = fs::hard_link(&temporary, path) {
            let _ = fs::remove_file(&temporary);
            return Err(AgentError::io(error));
        }
        let _ = fs::remove_file(&temporary);
    }
    Ok(())
}

fn write_file(
    file: &mut File,
    mode: u32,
    write_content: impl FnOnce(&mut File) -> AgentResult<()>,
) -> AgentResult<()> {
    file.set_permissions(fs::Permissions::from_mode(mode))
        .map_err(AgentError::io)?;
    write_content(file)?;
    file.flush().map_err(AgentError::io)?;
    file.sync_all().map_err(AgentError::io)
}

fn create_temporary_file(path: &Path, mode: u32) -> AgentResult<(PathBuf, File)> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AgentError::invalid("The output path must have a valid filename"))?;
    for _ in 0..32 {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".apexshot-agent-{}-{sequence}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(AgentError::io(error)),
        }
    }
    Err(AgentError::new(
        ErrorCode::IoError,
        "Could not reserve a temporary output file",
    ))
}

fn map_image_error(error: image::ImageError) -> AgentError {
    match error {
        image::ImageError::IoError(error) => AgentError::io(error),
        error => AgentError::new(ErrorCode::CaptureFailed, error.to_string()),
    }
}

fn ocr_command(args: &[String]) -> AgentResult<Value> {
    let parsed = parse_ocr_args(args)?;
    let metadata = fs::metadata(&parsed.path).map_err(AgentError::io)?;
    if metadata.len() > MAX_OCR_FILE_BYTES {
        return Err(AgentError::invalid(
            "The image file exceeds the supported size limit",
        ));
    }
    let reader = image::io::Reader::open(&parsed.path).map_err(AgentError::io)?;
    let reader = reader.with_guessed_format().map_err(AgentError::io)?;
    let (width, height) = reader.into_dimensions().map_err(|error| {
        AgentError::new(
            ErrorCode::OcrFailed,
            format!("Could not read image dimensions: {error}"),
        )
    })?;
    validate_image_dimensions(width, height)?;

    let config = OcrConfig::default()
        .with_language(parsed.language)
        .with_min_confidence(i32::from(parsed.min_confidence))
        .with_clipboard(false);
    let output = ocr::extract_text_from_path(&parsed.path, &config).map_err(map_ocr_error)?;
    if output.text.len() > MAX_OCR_TEXT_BYTES {
        return Err(AgentError::new(
            ErrorCode::OcrFailed,
            "OCR output exceeded the supported response size",
        ));
    }
    let (source, confidence) = match output.source {
        ContentSource::Ocr { confidence } => ("ocr", Some(confidence)),
        ContentSource::QrCode => ("qr_code", None),
    };
    Ok(json!({
        "text": output.text,
        "source": source,
        "confidence": confidence,
        "copied_to_clipboard": false,
    }))
}

fn parse_ocr_args(args: &[String]) -> AgentResult<OcrArgs> {
    let path = args
        .get(1)
        .filter(|value| !value.starts_with("--") && !value.is_empty())
        .ok_or_else(|| AgentError::invalid("ocr requires an image path"))?;
    let mut language = String::from("eng");
    let mut min_confidence = 50;
    let mut language_seen = false;
    let mut confidence_seen = false;
    let mut index = 2;
    while index < args.len() {
        let option = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or_else(|| AgentError::invalid(format!("{option} requires a value")))?;
        if value.starts_with("--") || value.is_empty() {
            return Err(AgentError::invalid(format!("{option} requires a value")));
        }
        match option {
            "--lang" if !language_seen => {
                validate_language(value)?;
                language = value.clone();
                language_seen = true;
            }
            "--min-confidence" if !confidence_seen => {
                min_confidence = value.parse::<u8>().map_err(|_| {
                    AgentError::invalid("--min-confidence must be an integer from 0 to 100")
                })?;
                if min_confidence > 100 {
                    return Err(AgentError::invalid(
                        "--min-confidence must be an integer from 0 to 100",
                    ));
                }
                confidence_seen = true;
            }
            "--lang" | "--min-confidence" => {
                return Err(AgentError::invalid(format!(
                    "{option} may only be specified once"
                )))
            }
            _ => {
                return Err(AgentError::invalid(format!(
                    "Unknown OCR option '{option}'"
                )))
            }
        }
        index += 2;
    }
    Ok(OcrArgs {
        path: PathBuf::from(path),
        language,
        min_confidence,
    })
}

fn validate_language(language: &str) -> AgentResult<()> {
    if language.len() > MAX_OCR_LANGUAGE_LENGTH
        || language.is_empty()
        || !language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'_' | b'-'))
    {
        return Err(AgentError::invalid(
            "OCR language must be a short language code such as eng or eng+fra",
        ));
    }
    Ok(())
}

fn validate_image_dimensions(width: u32, height: u32) -> AgentResult<()> {
    if width == 0
        || height == 0
        || width > MAX_IMAGE_WIDTH
        || height > MAX_IMAGE_HEIGHT
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
    {
        return Err(AgentError::invalid("The image has unsupported dimensions"));
    }
    Ok(())
}

fn map_ocr_error(error: OcrError) -> AgentError {
    match error {
        OcrError::TesseractNotFound => AgentError::new(
            ErrorCode::Unsupported,
            "No OCR engine is available for this image",
        ),
        OcrError::InitializationError(message) => AgentError::new(ErrorCode::OcrFailed, message),
        OcrError::RecognitionError(message)
        | OcrError::ImageError(message)
        | OcrError::ClipboardError(message) => AgentError::new(ErrorCode::OcrFailed, message),
        OcrError::NoTextDetected => {
            AgentError::new(ErrorCode::OcrFailed, "No text or QR content was detected")
        }
        OcrError::LowConfidence(actual, required) => AgentError::new(
            ErrorCode::OcrFailed,
            format!("OCR confidence {actual}% is below the requested {required}%"),
        ),
    }
}

fn install_skill_command(args: &[String]) -> AgentResult<Value> {
    let mut destination = None;
    let mut overwrite = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--destination" => {
                if destination.is_some() {
                    return Err(AgentError::invalid(
                        "--destination may only be specified once",
                    ));
                }
                let value = args
                    .get(index + 1)
                    .filter(|value| !value.is_empty() && !value.starts_with("--"))
                    .ok_or_else(|| AgentError::invalid("--destination requires a directory"))?;
                destination = Some(PathBuf::from(value));
                index += 2;
            }
            "--overwrite" if !overwrite => {
                overwrite = true;
                index += 1;
            }
            "--overwrite" => {
                return Err(AgentError::invalid(
                    "--overwrite may only be specified once",
                ))
            }
            option => {
                return Err(AgentError::invalid(format!(
                    "Unknown install-skill option '{option}'"
                )))
            }
        }
    }
    let destination = destination
        .ok_or_else(|| AgentError::invalid("install-skill requires --destination DIRECTORY"))?;
    fs::create_dir_all(&destination).map_err(AgentError::io)?;
    let path = absolute_path(&destination.join("SKILL.md"))?;
    let path_string = path_to_string(&path)?;
    write_exact(&path, overwrite, 0o644, |file| {
        file.write_all(SKILL_SOURCE.as_bytes())
            .map_err(AgentError::io)
    })?;
    Ok(json!({"path": path_string}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use apexshot::backend::PixelFormat;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn temp_dir() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "apexshot-agent-test-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn capture_arguments_require_an_exact_png_path_and_validate_region() {
        assert!(parse_capture_args(&args(&["capture"])).is_err());
        assert!(parse_capture_args(&args(&["capture", "--output", "folder"])).is_err());
        assert!(parse_capture_args(&args(&["capture", "--output", "folder.png/"])).is_err());
        assert!(parse_capture_args(&args(&[
            "capture",
            "--output",
            "capture.png",
            "--region",
            "-1,0,2,2",
        ]))
        .is_err());
        assert!(parse_capture_args(&args(&[
            "capture",
            "--output",
            "capture.png",
            "--region",
            "4294967295,0,1,1",
        ]))
        .is_err());
        let parsed = parse_capture_args(&args(&[
            "capture",
            "--output",
            "capture.PNG",
            "--region",
            "1,2,3,4",
        ]))
        .expect("valid capture arguments");
        assert_eq!(
            parsed.region,
            Some(Region {
                x: 1,
                y: 2,
                width: 3,
                height: 4
            })
        );
    }

    #[test]
    fn envelopes_keep_schema_and_exit_codes_consistent() {
        let (success_code, success) = execute(&args(&["unknown"]));
        assert_eq!(success_code, 2);
        assert_eq!(success["schema_version"], SCHEMA_VERSION);
        assert_eq!(success["ok"], false);
        let error_code = success["error"]["code"].as_str().expect("error code");
        assert_eq!(error_code, ErrorCode::InvalidArguments.as_str());
        assert_eq!(success_code, ErrorCode::InvalidArguments.exit_code());

        for (code, expected_exit) in [
            (ErrorCode::InvalidArguments, 2),
            (ErrorCode::Cancelled, 3),
            (ErrorCode::Unsupported, 4),
            (ErrorCode::PermissionDenied, 5),
            (ErrorCode::CaptureFailed, 1),
            (ErrorCode::IoError, 1),
            (ErrorCode::OcrFailed, 1),
        ] {
            let (exit_code, envelope) = error_envelope(AgentError::new(code, "test"));
            assert_eq!(exit_code, expected_exit);
            assert_eq!(envelope["schema_version"], SCHEMA_VERSION);
            assert_eq!(envelope["ok"], false);
            assert_eq!(envelope["error"]["code"], code.as_str());
        }

        let (success_code, envelope) = success_envelope(json!({"ready": true}));
        assert_eq!(success_code, 0);
        assert_eq!(envelope["schema_version"], SCHEMA_VERSION);
        assert_eq!(envelope["ok"], true);
        assert_eq!(envelope["result"]["ready"], true);

        let (capability_code, capability) = execute(&args(&["capabilities"]));
        assert_eq!(capability_code, 0);
        assert_eq!(capability["schema_version"], SCHEMA_VERSION);
        assert_eq!(capability["ok"], true);
        assert!(capability["result"].is_object());
    }

    #[test]
    fn capabilities_never_claim_screen_permission_is_granted() {
        let capability = capabilities();
        assert_eq!(
            capability["permissions"]["screen_capture"]["status"],
            "unknown"
        );
        assert_ne!(
            capability["permissions"]["screen_capture"]["status"],
            "granted"
        );
    }

    #[test]
    fn region_crop_accepts_edges_and_rejects_negative_or_overflowing_bounds() {
        let image = RgbaImage::from_pixel(4, 3, Rgba([1, 2, 3, 255]));
        let edge = crop_image(
            &image,
            Region {
                x: 2,
                y: 1,
                width: 2,
                height: 2,
            },
        )
        .expect("crop within bounds");
        assert_eq!(edge.dimensions(), (2, 2));
        assert!(parse_region("-1,0,1,1").is_err());
        assert!(parse_region("4294967295,0,1,1").is_err());
        assert!(crop_image(
            &image,
            Region {
                x: 3,
                y: 0,
                width: 2,
                height: 1,
            }
        )
        .is_err());
    }

    #[test]
    fn native_wayland_routing_respects_portal_only_and_capture_overrides() {
        assert!(should_use_native_wayland_capture(false, false, false));
        assert!(!should_use_native_wayland_capture(true, false, false));
        assert!(!should_use_native_wayland_capture(false, true, false));
        assert!(!should_use_native_wayland_capture(false, false, true));
    }

    #[test]
    fn native_wayland_source_check_accepts_fractional_scale_but_rejects_default_origin() {
        let monitor = MonitorInfo {
            id: "connector:DP-1".to_owned(),
            name: "DP-1".to_owned(),
            index: 0,
            x: 1600,
            y: 0,
            width: 853,
            height: 480,
            scale: 1,
            primary: false,
        };
        let capture = CaptureData {
            pixels: Vec::new(),
            width: 1280,
            height: 720,
            stride: 5120,
            format: PixelFormat::RGBA32,
            cursor: None,
            output_origin_x: 1600,
            output_origin_y: 0,
            output_scale: 1,
        };
        assert!(native_capture_matches_monitor(&capture, &monitor));

        let screenshot_portal_defaults = CaptureData {
            output_origin_x: 0,
            ..capture.clone()
        };
        assert!(!native_capture_matches_monitor(
            &screenshot_portal_defaults,
            &monitor
        ));

        let wrong_aspect = CaptureData {
            height: 1200,
            ..capture
        };
        assert!(!native_capture_matches_monitor(&wrong_aspect, &monitor));
        assert!(frame_matches_monitor_aspect(1280, 720, 853, 480));
        assert!(!frame_matches_monitor_aspect(1280, 1200, 853, 480));
    }

    #[test]
    fn x11_capture_bounds_apply_gdk_scale_and_reject_overflow() {
        let monitor = MonitorInfo {
            id: "monitor:10,20,640,480".to_owned(),
            name: "Display 1".to_owned(),
            index: 0,
            x: 10,
            y: 20,
            width: 640,
            height: 480,
            scale: 1,
            primary: true,
        };
        assert_eq!(
            x11_physical_bounds(&monitor).expect("scale-one bounds"),
            X11PhysicalBounds {
                x: 10,
                y: 20,
                width: 640,
                height: 480,
            }
        );

        let scaled = MonitorInfo {
            scale: 2,
            ..monitor.clone()
        };
        assert_eq!(
            x11_physical_bounds(&scaled).expect("scale-two bounds"),
            X11PhysicalBounds {
                x: 20,
                y: 40,
                width: 1280,
                height: 960,
            }
        );

        let overflowing = MonitorInfo {
            x: i32::MAX,
            scale: 2,
            ..monitor
        };
        assert!(x11_physical_bounds(&overflowing).is_err());
    }

    #[test]
    fn output_preflight_rejects_known_path_errors_before_capture() {
        let directory = temp_dir();
        let missing_parent = directory.join("missing").join("capture.png");
        assert!(preflight_output_path(&missing_parent, false).is_err());

        let existing = directory.join("existing.png");
        fs::write(&existing, b"existing").expect("write existing output");
        assert!(preflight_output_path(&existing, false).is_err());
        assert!(preflight_output_path(&existing, true).is_ok());

        let output_directory = directory.join("directory.png");
        fs::create_dir(&output_directory).expect("create output directory");
        assert!(preflight_output_path(&output_directory, true).is_err());
        fs::remove_dir_all(directory).expect("remove temp dir");
    }

    #[test]
    fn png_writer_uses_the_exact_path_and_requires_explicit_overwrite() {
        let directory = temp_dir();
        let path = directory.join("exact-name.png");
        let first = RgbaImage::from_pixel(2, 1, Rgba([10, 20, 30, 255]));
        let second = RgbaImage::from_pixel(1, 2, Rgba([40, 50, 60, 255]));
        write_png_exact(&path, &first, false).expect("write exact destination");
        let decoded = image::open(&path).expect("read written png").to_rgba8();
        assert_eq!((decoded.width(), decoded.height()), (2, 1));
        assert!(write_png_exact(&path, &second, false).is_err());
        let unchanged = image::open(&path)
            .expect("read png after rejected overwrite")
            .to_rgba8();
        assert_eq!(unchanged.get_pixel(0, 0), &Rgba([10, 20, 30, 255]));
        write_png_exact(&path, &second, true).expect("overwrite exact destination");
        let decoded = image::open(&path).expect("read overwritten png").to_rgba8();
        assert_eq!(decoded.dimensions(), (1, 2));
        assert_eq!(decoded.get_pixel(0, 0), &Rgba([40, 50, 60, 255]));
        fs::remove_dir_all(directory).expect("remove temp dir");
    }

    #[cfg(unix)]
    #[test]
    fn overwrite_replaces_a_symlink_without_following_its_target() {
        let directory = temp_dir();
        let target = directory.join("target.png");
        let link = directory.join("link.png");
        fs::write(&target, b"untouched").expect("write target");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");
        let image = RgbaImage::from_pixel(1, 1, Rgba([1, 2, 3, 255]));
        write_png_exact(&link, &image, true).expect("replace symlink entry");
        assert_eq!(fs::read(&target).expect("read target"), b"untouched");
        assert!(!fs::symlink_metadata(&link)
            .expect("read link metadata")
            .file_type()
            .is_symlink());
        fs::remove_dir_all(directory).expect("remove temp dir");
    }

    #[test]
    fn ocr_arguments_reject_unsupported_language_and_confidence_values() {
        assert!(parse_ocr_args(&args(&["ocr", "image.png", "--lang", "eng;touch"])).is_err());
        assert!(parse_ocr_args(&args(&["ocr", "image.png", "--min-confidence", "101",])).is_err());
        assert!(parse_ocr_args(&args(&["ocr", "image.png", "--min-confidence", "NaN"])).is_err());
    }

    #[test]
    fn production_agent_route_has_no_daemon_ui_cloud_or_clipboard_calls() {
        let source = include_str!("agent.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        for forbidden in [
            "daemon::",
            "preview_launch::",
            "show_capture_preview",
            "cloud::",
            "clipboard::",
            "copy_to_clipboard(",
        ] {
            assert!(
                !production.contains(forbidden),
                "unexpected dependency: {forbidden}"
            );
        }
        assert!(production.contains("backend::screencopy::capture_at"));
        assert!(production.contains("PortalStillSession::prepare"));
        assert!(production.contains("session.position == Some"));
        assert!(production.contains("session.size == Some"));
        assert!(!production.contains("capture_screen_for_selection_at"));
    }
}
