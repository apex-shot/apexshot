//! Wayland backend implementation.
//!
//! Capture strategy (fastest first):
//!
//! 0. **wlr-screencopy** — direct Wayland protocol, ~50 ms, no popup.
//!    Works on Sway, Hyprland, Niri, KDE ≥ 6.3.
//!
//! 1. **`grim`** (subprocess) — wlr-screencopy via external binary, ~50 ms.
//!    Works on wlroots compositors.
//!
//! 2. **`org.freedesktop.portal.Screenshot`** — no persistent share session,
//!    ~200-400 ms.
//!
//! 3. **ScreenCast portal + PipeWire** — last resort, ~1-2 s first run.
//!    Shows the screensharing popup dialog.

use super::{CaptureData, DisplayBackend, DisplayError, DisplayResult, PixelFormat};
use ashpd::desktop::{
    screencast::{CursorMode, Screencast, SourceType},
    screenshot::Screenshot,
    PersistMode,
};
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use std::path::PathBuf;
use std::sync::OnceLock;

pub struct WaylandBackend;

// ──────────────────────────────────────────────────────────────────────────────
// ScreenCast restore-token helpers
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum CaptureTarget {
    Monitor,
    Window,
}

impl CaptureTarget {
    fn source_type(self) -> SourceType {
        match self {
            Self::Monitor => SourceType::Monitor,
            Self::Window => SourceType::Window,
        }
    }

    fn token_file_name(self) -> &'static str {
        match self {
            Self::Monitor => "wayland-screencast-monitor.token",
            Self::Window => "wayland-screencast-window.token",
        }
    }
}

fn restore_token_path(target: CaptureTarget) -> Option<PathBuf> {
    let mut path = dirs::cache_dir()?;
    path.push("apexshot");
    path.push(target.token_file_name());
    Some(path)
}

fn load_restore_token(target: CaptureTarget) -> Option<String> {
    let path = restore_token_path(target)?;
    let raw = std::fs::read_to_string(path).ok()?;
    let token = raw.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn save_restore_token(target: CaptureTarget, token: &str) {
    let Some(path) = restore_token_path(target) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, token);
}

fn clear_restore_token(target: CaptureTarget) {
    if let Some(path) = restore_token_path(target) {
        let _ = std::fs::remove_file(path);
    }
}

fn is_gnome_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        && (std::env::var_os("GNOME_SETUP_DISPLAY").is_some()
            || std::env::var("XDG_CURRENT_DESKTOP")
                .map(|desktop| desktop.to_ascii_lowercase().contains("gnome"))
                .unwrap_or(false))
}

fn gnome_wayland_fast_path(
    is_gnome_wayland: bool,
    allow_screencast: bool,
    allow_screenshot_portal: bool,
) -> bool {
    is_gnome_wayland && allow_screencast && allow_screenshot_portal
}

fn grim_available() -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join("grim"))
        .any(|path| path.is_file())
}

fn should_probe_grim_with_env(
    allow_screenshot_portal: bool,
    is_gnome_wayland: bool,
    grim_installed: bool,
) -> bool {
    if is_gnome_wayland && allow_screenshot_portal {
        return false;
    }
    grim_installed
}

// ──────────────────────────────────────────────────────────────────────────────
// GStreamer helpers (only used in the ScreenCast fallback)
// ──────────────────────────────────────────────────────────────────────────────

fn ensure_gstreamer_initialized() -> DisplayResult<()> {
    static GST_INIT: OnceLock<Result<(), String>> = OnceLock::new();
    match GST_INIT.get_or_init(|| gst::init().map_err(|e| e.to_string())) {
        Ok(()) => Ok(()),
        Err(err) => Err(DisplayError::InitializationError(format!(
            "Failed to initialize GStreamer: {err}"
        ))),
    }
}

fn capture_single_frame_from_pipewire(node_id: u32) -> DisplayResult<CaptureData> {
    ensure_gstreamer_initialized()?;

    let pipeline_str = format!(
        "pipewiresrc path={node_id} do-timestamp=true num-buffers=1 ! videoconvert ! video/x-raw,format=RGBA ! appsink name=sink emit-signals=false sync=false max-buffers=1 drop=true"
    );

    let pipeline = gst::parse::launch(&pipeline_str)
        .map_err(|e| DisplayError::CaptureError(format!("Failed to build pipeline: {e}")))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| DisplayError::CaptureError("Failed to cast pipeline".into()))?;

    let appsink = pipeline
        .by_name("sink")
        .ok_or_else(|| DisplayError::CaptureError("AppSink not found in pipeline".into()))?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| DisplayError::CaptureError("Failed to cast AppSink".into()))?;

    pipeline
        .set_state(gst::State::Playing)
        .map_err(|e| DisplayError::CaptureError(format!("Failed to start pipeline: {e}")))?;

    let sample = appsink
        .try_pull_sample(gst::ClockTime::from_seconds(2))
        .ok_or_else(|| DisplayError::CaptureError("Timed out waiting for PipeWire frame".into()))?;

    let caps = sample
        .caps()
        .ok_or_else(|| DisplayError::CaptureError("Missing sample caps".into()))?;

    let info = gst_video::VideoInfo::from_caps(caps)
        .map_err(|e| DisplayError::CaptureError(format!("Invalid video info from caps: {e}")))?;

    let width = info.width();
    let height = info.height();
    if width == 0 || height == 0 {
        let _ = pipeline.set_state(gst::State::Null);
        return Err(DisplayError::CaptureError(
            "PipeWire frame has invalid dimensions".into(),
        ));
    }

    let stride_raw = info.stride()[0];
    if stride_raw <= 0 {
        let _ = pipeline.set_state(gst::State::Null);
        return Err(DisplayError::CaptureError(
            "PipeWire frame has invalid stride".into(),
        ));
    }
    let stride = stride_raw as usize;

    let buffer = sample
        .buffer()
        .ok_or_else(|| DisplayError::CaptureError("Missing sample buffer".into()))?;
    let map = buffer
        .map_readable()
        .map_err(|_| DisplayError::CaptureError("Failed to map sample buffer".into()))?;

    let row_len = width as usize * 4;
    if row_len > stride {
        let _ = pipeline.set_state(gst::State::Null);
        return Err(DisplayError::CaptureError(
            "Frame stride is smaller than expected row length".into(),
        ));
    }

    let expected_min_len = stride * height as usize;
    if map.as_slice().len() < expected_min_len {
        let _ = pipeline.set_state(gst::State::Null);
        return Err(DisplayError::CaptureError(
            "Frame buffer is smaller than expected".into(),
        ));
    }

    let mut pixels = Vec::with_capacity(row_len * height as usize);
    for row in 0..height as usize {
        let src_start = row * stride;
        let src_end = src_start + row_len;
        pixels.extend_from_slice(&map.as_slice()[src_start..src_end]);
    }

    let _ = pipeline.set_state(gst::State::Null);

    Ok(CaptureData::new(pixels, width, height, PixelFormat::RGBA32))
}

fn crop_capture(
    capture: CaptureData,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> DisplayResult<CaptureData> {
    if width <= 0 || height <= 0 || x < 0 || y < 0 {
        return Err(DisplayError::InvalidArea(format!(
            "Invalid dimensions: {}x{}",
            width, height
        )));
    }

    let x_end = x
        .checked_add(width)
        .ok_or_else(|| DisplayError::InvalidArea("Area width overflow".into()))?;
    let y_end = y
        .checked_add(height)
        .ok_or_else(|| DisplayError::InvalidArea("Area height overflow".into()))?;

    if x_end as u32 > capture.width || y_end as u32 > capture.height {
        return Err(DisplayError::InvalidArea(format!(
            "Requested area ({x}, {y}, {width}, {height}) is out of bounds for {}x{} capture",
            capture.width, capture.height
        )));
    }

    let width_u32 = width as u32;
    let height_u32 = height as u32;
    let bytes_per_pixel = capture.format.bytes_per_pixel as usize;
    let source_stride = capture.stride as usize;
    let row_len = width_u32 as usize * bytes_per_pixel;

    let mut cropped = Vec::with_capacity(row_len * height_u32 as usize);
    for row in 0..height_u32 as usize {
        let src_y = y as usize + row;
        let src_offset = src_y * source_stride + x as usize * bytes_per_pixel;
        let src_end = src_offset + row_len;
        cropped.extend_from_slice(&capture.pixels[src_offset..src_end]);
    }

    Ok(CaptureData::new(
        cropped,
        width_u32,
        height_u32,
        capture.format,
    ))
}

// ──────────────────────────────────────────────────────────────────────────────
// Capture implementations
// ──────────────────────────────────────────────────────────────────────────────

impl WaylandBackend {
    /// `grim` speaks `wlr-screencopy` / `ext-image-copy-capture` directly over
    /// the Wayland socket — no D-Bus, no portal daemon.  This is the same path
    /// the system screenshot button takes on wlroots compositors.
    ///
    /// Returns `Err` if `grim` is not installed or fails, so the caller can
    /// fall through to the next tier.
    fn capture_via_grim() -> DisplayResult<CaptureData> {
        // Write to a deterministic temp file so we don't have to parse stdout.
        let tmp = std::env::temp_dir().join(format!(
            "apexshot_grim_{}.png",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));

        let status = std::process::Command::new("grim")
            .arg(tmp.as_os_str())
            // Forward the Wayland display socket so grim can connect even when
            // we're running from a spawned process.
            .env(
                "WAYLAND_DISPLAY",
                std::env::var("WAYLAND_DISPLAY").unwrap_or_default(),
            )
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| {
                DisplayError::CaptureError(format!("grim not found or failed to start: {e}"))
            })?;

        if !status.success() {
            return Err(DisplayError::CaptureError(format!(
                "grim exited with status {}",
                status
            )));
        }

        let img = image::open(&tmp)
            .map_err(|e| DisplayError::CaptureError(format!("Failed to load grim output: {e}")))?
            .into_rgba8();

        let _ = std::fs::remove_file(&tmp);

        let width = img.width();
        let height = img.height();
        let pixels = img.into_raw();

        Ok(CaptureData::new(pixels, width, height, PixelFormat::RGBA32))
    }

    /// Screenshot portal capture.
    ///
    /// `interactive=true` opens the desktop's selector UI first.
    async fn capture_via_screenshot_portal(interactive: bool) -> DisplayResult<CaptureData> {
        let request = Screenshot::request()
            .interactive(interactive)
            .modal(false)
            .send()
            .await
            .map_err(|e| {
                DisplayError::PortalError(format!("Screenshot portal request failed: {e}"))
            })?;

        let response = request.response().map_err(|e| {
            DisplayError::PortalError(format!("Screenshot portal response failed: {e}"))
        })?;

        let path = response.uri().to_file_path().map_err(|_| {
            DisplayError::PortalError(format!(
                "Screenshot portal returned non-file URI: {}",
                response.uri()
            ))
        })?;

        let img = image::open(&path).map_err(|e| {
            DisplayError::CaptureError(format!(
                "Failed to load Screenshot portal image {}: {e}",
                path.display()
            ))
        })?;

        let _ = std::fs::remove_file(&path);

        let img = img.into_rgba8();
        let width = img.width();
        let height = img.height();
        let pixels = img.into_raw();

        Ok(CaptureData::new(pixels, width, height, PixelFormat::RGBA32))
    }

    /// **Tier 4 — ScreenCast portal + PipeWire** (~1-2 s first run).
    ///
    /// Reuses a saved restore-token on subsequent calls to avoid showing the
    /// source-selection dialog again.
    async fn capture_via_screencast(
        target: CaptureTarget,
        interactive: bool,
    ) -> DisplayResult<CaptureData> {
        if !interactive {
            if let Some(token) = load_restore_token(target) {
                match Self::capture_screencast_once(target, interactive, Some(token.as_str())).await
                {
                    Ok(capture) => return Ok(capture),
                    Err(_) => clear_restore_token(target),
                }
            }
        }
        Self::capture_screencast_once(target, interactive, None).await
    }

    async fn capture_screencast_once(
        target: CaptureTarget,
        interactive: bool,
        restore_token: Option<&str>,
    ) -> DisplayResult<CaptureData> {
        let screencast = Screencast::new().await.map_err(|e| {
            DisplayError::PortalError(format!("Failed to create ScreenCast proxy: {e}"))
        })?;

        let session = screencast.create_session().await.map_err(|e| {
            DisplayError::PortalError(format!("Failed to create ScreenCast session: {e}"))
        })?;

        let persist_mode = if interactive {
            PersistMode::DoNot
        } else {
            PersistMode::ExplicitlyRevoked
        };

        let select_request = screencast
            .select_sources(
                &session,
                CursorMode::Embedded,
                target.source_type().into(),
                false,
                restore_token,
                persist_mode,
            )
            .await
            .map_err(|e| DisplayError::PortalError(format!("Failed to select sources: {e}")))?;

        select_request.response().map_err(|e| {
            DisplayError::PortalError(format!("Source selection cancelled/failed: {e}"))
        })?;

        let start_request = screencast.start(&session, None).await.map_err(|e| {
            DisplayError::PortalError(format!("Failed to start ScreenCast session: {e}"))
        })?;

        let response = start_request
            .response()
            .map_err(|e| DisplayError::PortalError(format!("ScreenCast start failed: {e}")))?;

        let node_id = response
            .streams()
            .first()
            .ok_or_else(|| {
                DisplayError::PortalError("No streams returned by ScreenCast portal".into())
            })?
            .pipe_wire_node_id();

        if !interactive {
            if let Some(token) = response.restore_token() {
                if !token.trim().is_empty() {
                    save_restore_token(target, token);
                }
            }
        }

        let capture = capture_single_frame_from_pipewire(node_id);
        let _ = session.close().await;
        capture
    }

    fn capture_monitor_via_screencast() -> DisplayResult<CaptureData> {
        let t3_start = std::time::Instant::now();
        let result = block_on_async(async {
            Self::capture_via_screencast(CaptureTarget::Monitor, false).await
        });
        match &result {
            Ok(d) => eprintln!(
                "[capture] Tier 3 (ScreenCast) succeeded in {:.0}ms ({}x{}).",
                t3_start.elapsed().as_millis(),
                d.width,
                d.height
            ),
            Err(e) => eprintln!(
                "[capture] Tier 3 (ScreenCast) failed ({:.0}ms): {e}",
                t3_start.elapsed().as_millis()
            ),
        }
        result
    }

    fn capture_screen_chain(
        &self,
        allow_screenshot_portal: bool,
        allow_screencast: bool,
    ) -> DisplayResult<CaptureData> {
        eprintln!("[capture] capture_screen_chain: starting (allow_screenshot_portal={allow_screenshot_portal}, allow_screencast={allow_screencast})");

        // Tier 0: direct wlr-screencopy — fastest, no popup, no portal daemon.
        // Returns Ok(None) if compositor lacks zwlr_screencopy_manager_v1.
        eprintln!("[capture] Tier 0 (wlr-screencopy): attempting...");
        let t0_start = std::time::Instant::now();
        match super::screencopy::capture() {
            Ok(Some(capture)) => {
                eprintln!("[capture] Tier 0 (wlr-screencopy) succeeded in {:.0}ms — NO portal flash/sound.", t0_start.elapsed().as_millis());
                return Ok(capture);
            }
            Ok(None) => eprintln!(
                "[capture] Tier 0 (wlr-screencopy) not supported by compositor ({:.0}ms).",
                t0_start.elapsed().as_millis()
            ),
            Err(e) => eprintln!(
                "[capture] Tier 0 (wlr-screencopy) failed ({:.0}ms): {e}",
                t0_start.elapsed().as_millis()
            ),
        }

        let is_gnome_wayland = is_gnome_wayland_session();
        let prefer_screencast_first =
            gnome_wayland_fast_path(is_gnome_wayland, allow_screencast, allow_screenshot_portal);

        if should_probe_grim_with_env(allow_screenshot_portal, is_gnome_wayland, grim_available()) {
            // Tier 1: grim subprocess — wlroots compositors.
            eprintln!("[capture] Tier 1 (grim): attempting...");
            let t1_start = std::time::Instant::now();
            match Self::capture_via_grim() {
                Ok(capture) => {
                    eprintln!(
                        "[capture] Tier 1 (grim) succeeded in {:.0}ms — NO portal flash/sound.",
                        t1_start.elapsed().as_millis()
                    );
                    return Ok(capture);
                }
                Err(e) => eprintln!(
                    "[capture] Tier 1 (grim) failed ({:.0}ms): {e}",
                    t1_start.elapsed().as_millis()
                ),
            }
        } else {
            eprintln!("[capture] Tier 1 (grim): SKIPPED for current environment.");
        }

        if prefer_screencast_first {
            eprintln!(
                "[capture] GNOME Wayland detected — preferring ScreenCast before Screenshot portal to avoid flash/sound."
            );
            eprintln!(
                "[capture] Tier 3 (ScreenCast portal + PipeWire): attempting before Screenshot portal..."
            );
            if let Ok(capture) = Self::capture_monitor_via_screencast() {
                return Ok(capture);
            }
        }

        if allow_screenshot_portal {
            eprintln!("[capture] Tier 2 (Screenshot portal): attempting — ⚠ THIS WILL TRIGGER PORTAL SCREENSHOT (flash/sound expected)");
            let t2_start = std::time::Instant::now();
            match block_on_async(Self::capture_via_screenshot_portal(false)) {
                Ok(capture) => {
                    eprintln!("[capture] Tier 2 (Screenshot portal) succeeded in {:.0}ms — portal flash/sound was triggered here.", t2_start.elapsed().as_millis());
                    return Ok(capture);
                }
                Err(e) => eprintln!(
                    "[capture] Tier 2 (Screenshot portal) failed ({:.0}ms): {e}",
                    t2_start.elapsed().as_millis()
                ),
            }
        } else {
            eprintln!(
                "[capture] Tier 2 (Screenshot portal): SKIPPED (allow_screenshot_portal=false)"
            );
        }

        if allow_screencast && !prefer_screencast_first {
            eprintln!(
                "[capture] Tier 3 (ScreenCast portal + PipeWire): attempting (last resort)..."
            );
            Self::capture_monitor_via_screencast()
        } else if allow_screencast {
            Err(DisplayError::CaptureError(
                "GNOME Wayland ScreenCast capture failed and Screenshot portal fallback also failed"
                    .into(),
            ))
        } else {
            eprintln!("[capture] Tier 3 (ScreenCast): SKIPPED (allow_screencast=false) — all tiers exhausted.");
            Err(DisplayError::CaptureError(
                "No non-screencast Wayland capture path available (all tiers failed or skipped)"
                    .into(),
            ))
        }
    }

    /// Run the capture chain for a full-screen monitor capture.
    ///
    /// Tiers (fastest → slowest):
    /// 0. wlr-screencopy (direct Wayland, ~50 ms, no popup)
    /// 1. grim subprocess (~50 ms, wlroots)
    /// 2. Screenshot portal (~200-400 ms, no persistent share session)
    /// 3. ScreenCast + PipeWire (last resort, may show popup)
    pub fn capture_screen_impl(&self) -> DisplayResult<CaptureData> {
        self.capture_screen_chain(true, true)
    }

    /// Area capture via interactive Screenshot portal selector.
    pub fn capture_area_via_portal_interactive_impl(&self) -> DisplayResult<CaptureData> {
        block_on_async(Self::capture_via_screenshot_portal(true))
    }

    /// Direct area capture optimized for Wayland when coordinates are known.
    ///
    /// Attempts compositor-native paths before falling back to full-screen crop.
    pub fn capture_area_direct_impl(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> DisplayResult<CaptureData> {
        if width <= 0 || height <= 0 {
            return Err(DisplayError::InvalidArea(format!(
                "Invalid dimensions: {}x{}",
                width, height
            )));
        }

        let full = self
            .capture_screen_for_selection_impl()
            .or_else(|_| self.capture_screen_impl())?;
        crop_capture(full, x, y, width, height)
    }

    /// Capture used for area-selector backgrounds on Wayland.
    ///
    /// Allows the Screenshot portal fallback, but intentionally skips
    /// ScreenCast to avoid triggering a persistent screen-sharing session.
    /// ⚠ On GNOME Wayland (no wlr-screencopy, no grim), this will fall through to
    /// the Screenshot portal (Tier 2), which triggers the system screenshot sound + flash.
    pub fn capture_screen_for_selection_impl(&self) -> DisplayResult<CaptureData> {
        eprintln!("[capture] capture_screen_for_selection_impl: called (Wayland selector background capture)");
        self.capture_screen_chain(true, false)
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Async executor helper
// ──────────────────────────────────────────────────────────────────────────────

fn block_on_async<F, R>(future: F) -> DisplayResult<R>
where
    F: std::future::Future<Output = DisplayResult<R>>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(move || handle.block_on(future)),
        Err(_) => {
            let rt = tokio::runtime::Runtime::new().map_err(|e| {
                DisplayError::InitializationError(format!("Failed to create tokio runtime: {e}"))
            })?;
            rt.block_on(future)
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// DisplayBackend implementation
// ──────────────────────────────────────────────────────────────────────────────

impl DisplayBackend for WaylandBackend {
    fn new() -> DisplayResult<Self> {
        Ok(WaylandBackend)
    }

    fn capture_screen(&self) -> DisplayResult<CaptureData> {
        self.capture_screen_impl()
    }

    fn capture_area(&self, x: i32, y: i32, width: i32, height: i32) -> DisplayResult<CaptureData> {
        self.capture_area_direct_impl(x, y, width, height)
    }

    fn capture_window(&self, _window_id: u64) -> DisplayResult<CaptureData> {
        // Window capture requires the ScreenCast portal (grim/Screenshot portal
        // always captures the full display, not a single window).
        block_on_async(async { Self::capture_via_screencast(CaptureTarget::Window, true).await })
    }

    fn is_supported() -> bool {
        // Pure env-var check — no plugin scans, no I/O, no D-Bus.
        // GStreamer and grim availability are probed lazily at capture time.
        std::env::var("XDG_SESSION_TYPE")
            .map(|s| s.to_lowercase() == "wayland")
            .unwrap_or(false)
            && std::env::var("WAYLAND_DISPLAY").is_ok()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_supported() {
        let supported = WaylandBackend::is_supported();
        println!("Wayland backend supported: {}", supported);
    }

    #[test]
    fn test_backend_creation() {
        let backend = WaylandBackend::new();
        assert!(backend.is_ok());
    }

    #[test]
    fn test_crop_capture_bounds_and_dimensions() {
        let data = CaptureData::new(vec![255; 4 * 4 * 4], 4, 4, PixelFormat::RGBA32);

        let cropped = crop_capture(data, 1, 1, 2, 2).expect("crop should succeed");
        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 2);
        assert_eq!(cropped.pixels.len(), 2 * 2 * 4);
    }

    #[test]
    fn test_crop_capture_rejects_invalid_area() {
        let data = CaptureData::new(vec![255; 4 * 4 * 4], 4, 4, PixelFormat::RGBA32);
        let result = crop_capture(data, 3, 3, 3, 3);
        assert!(result.is_err());
    }

    #[test]
    fn gnome_wayland_fast_path_is_enabled_for_selector_backgrounds() {
        assert!(gnome_wayland_fast_path(true, true, true));
        assert!(!gnome_wayland_fast_path(true, false, true));
        assert!(!gnome_wayland_fast_path(false, true, true));
        assert!(!gnome_wayland_fast_path(true, true, false));
    }

    #[test]
    fn grim_probe_respects_environment_and_binary_presence() {
        assert!(!should_probe_grim_with_env(true, true, false));
        assert!(!should_probe_grim_with_env(true, false, false));
        assert!(should_probe_grim_with_env(true, false, true));
        assert!(should_probe_grim_with_env(false, false, true));
        assert!(!should_probe_grim_with_env(false, false, false));
    }
}
