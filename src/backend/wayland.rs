//! Wayland backend implementation.
//!
//! Capture strategy:
//!
//! 0. **ScreenCast portal + PipeWire** — primary Wayland path. This is the
//!    "share screen" flow and gives ApexShot a frame stream we can customize
//!    consistently across GNOME, KDE Plasma, Sway, Hyprland, Niri, and other
//!    portal-backed desktops.
//!
//! Legacy/native paths are intentionally not used for normal screen capture
//! because they do not provide the same controllable stream behavior across
//! desktops.
//!
//! 1. **`org.freedesktop.portal.Screenshot`** — retained only for the explicit
//!    interactive screenshot-selector helper.

use super::{screencopy, CaptureData, DisplayBackend, DisplayError, DisplayResult, PixelFormat};
use ashpd::desktop::{
    screencast::{CursorMode, Screencast, SourceType},
    screenshot::Screenshot,
    PersistMode,
};
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::time::Duration;

pub struct WaylandBackend;

const PORTAL_DIALOG_DISMISSAL_DELAY_MS: u64 = 650;

// ──────────────────────────────────────────────────────────────────────────────
// ScreenCast restore-token helpers
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
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

fn should_wait_for_portal_dialog_to_close(restore_token: Option<&str>) -> bool {
    restore_token.is_none()
}

// ──────────────────────────────────────────────────────────────────────────────
// Native PipeWire single-frame capture (replaces GStreamer pipewiresrc)
// ──────────────────────────────────────────────────────────────────────────────

fn capture_single_frame_from_pipewire(
    node_id: u32,
    pipewire_fd: &OwnedFd,
) -> DisplayResult<CaptureData> {
    let frame = crate::pipewire_engine::capture_single_frame(
        pipewire_fd
            .try_clone()
            .map_err(|e| DisplayError::CaptureError(format!("Failed to clone fd: {e}")))?,
        node_id,
        Duration::from_secs(3),
    )
    .map_err(|e| DisplayError::CaptureError(format!("PipeWire capture failed: {e}")))?;

    Ok(CaptureData::new(
        frame.pixels,
        frame.width,
        frame.height,
        PixelFormat::RGBA32,
    ))
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
    fn should_use_screenshot_portal() -> bool {
        std::env::var_os("APEXSHOT_WAYLAND_SCREENSHOT_PORTAL").is_some()
    }

    fn should_try_native_screencopy() -> bool {
        if std::env::var_os("APEXSHOT_DISABLE_WLR_SCREENCOPY").is_some() {
            return false;
        }

        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
            || std::env::var_os("SWAYSOCK").is_some()
        {
            return true;
        }

        std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split([':', ';', ','])
            .map(|part| part.trim().to_ascii_lowercase())
            .any(|part| {
                [
                    "hyprland", "sway", "river", "dwl", "wayfire", "labwc", "niri",
                ]
                .iter()
                .any(|needle| part.contains(needle))
            })
    }

    fn capture_monitor_via_native_screencopy() -> Option<DisplayResult<CaptureData>> {
        if !Self::should_try_native_screencopy() {
            return None;
        }

        let start = std::time::Instant::now();
        match screencopy::capture() {
            Ok(Some(capture)) => {
                eprintln!(
                    "[capture] Native wlr-screencopy succeeded in {:.0}ms ({}x{}).",
                    start.elapsed().as_millis(),
                    capture.width,
                    capture.height
                );
                Some(Ok(capture))
            }
            Ok(None) => {
                eprintln!(
                    "[capture] Native wlr-screencopy unavailable ({:.0}ms).",
                    start.elapsed().as_millis()
                );
                None
            }
            Err(err) => {
                eprintln!(
                    "[capture] Native wlr-screencopy failed ({:.0}ms): {err}.",
                    start.elapsed().as_millis()
                );
                None
            }
        }
    }

    /// Screenshot portal capture.
    ///
    /// `interactive=true` opens the desktop's selector UI first.
    async fn capture_via_screenshot_portal(interactive: bool) -> DisplayResult<CaptureData> {
        let _portal_identity = crate::utils::desktop_env::scoped_portal_capture_identity();

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
        let _portal_identity = crate::utils::desktop_env::scoped_portal_capture_identity();

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

        let stream = response.streams().first().ok_or_else(|| {
            DisplayError::PortalError("No streams returned by ScreenCast portal".into())
        })?;
        let node_id = stream.pipe_wire_node_id();

        // Get the actual window size and position within the stream
        // For window captures, the stream may be at monitor resolution with the window
        // content at a specific position and size
        let stream_size = stream.size();
        let stream_position = stream.position();

        if !interactive {
            if let Some(token) = response.restore_token() {
                if !token.trim().is_empty() {
                    save_restore_token(target, token);
                }
            }
        }

        if should_wait_for_portal_dialog_to_close(restore_token) {
            // GNOME can keep the portal "Share screen" dialog composited for a
            // frame or two after Start succeeds. Wait before opening PipeWire so
            // the first grabbed frame is the desktop, not the dismissed dialog.
            tokio::time::sleep(Duration::from_millis(PORTAL_DIALOG_DISMISSAL_DELAY_MS)).await;
        }

        let pipewire_fd = screencast
            .open_pipe_wire_remote(&session)
            .await
            .map_err(|e| {
                DisplayError::PortalError(format!("Failed to open PipeWire remote: {e}"))
            })?;
        let capture = capture_single_frame_from_pipewire(node_id, &pipewire_fd);
        let _ = session.close().await;

        // For window captures, crop to the actual window dimensions if provided
        if target == CaptureTarget::Window {
            eprintln!(
                "[capture] Window capture: stream_size={:?}, stream_position={:?}",
                stream_size, stream_position
            );
            if let (Some((win_width, win_height)), Some((win_x, win_y))) =
                (stream_size, stream_position)
            {
                if win_width > 0 && win_height > 0 && win_x >= 0 && win_y >= 0 {
                    let data = capture?;
                    eprintln!(
                        "[capture] Window capture: cropping from {}x{} to {}x{} at ({}, {})",
                        data.width, data.height, win_width, win_height, win_x, win_y
                    );
                    return crop_capture(data, win_x, win_y, win_width, win_height);
                }
            }

            // If stream_position is None (common on GNOME), try to detect content bounds
            // by finding non-transparent/non-black pixels
            let data = capture?;
            if let Some((x, y, w, h)) = Self::detect_content_bounds(&data) {
                eprintln!(
                    "[capture] Window capture: auto-detected content bounds {}x{} at ({}, {}) from {}x{}",
                    w, h, x, y, data.width, data.height
                );
                return crop_capture(data, x, y, w, h);
            }
            eprintln!(
                "[capture] Window capture: no content bounds detected, returning full frame {}x{}",
                data.width, data.height
            );
            return Ok(data);
        }

        capture
    }

    /// Detect the bounding box of actual content in a captured frame.
    /// Looks for non-transparent and non-black pixels to find the window content.
    fn detect_content_bounds(data: &CaptureData) -> Option<(i32, i32, i32, i32)> {
        if data.format != PixelFormat::RGBA32 {
            return None;
        }

        let width = data.width as usize;
        let height = data.height as usize;
        let pixels = &data.pixels;

        let mut min_x = width;
        let mut min_y = height;
        let mut max_x = 0;
        let mut max_y = 0;
        let mut found_content = false;

        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) * 4;
                if idx + 3 >= pixels.len() {
                    break;
                }
                let r = pixels[idx];
                let g = pixels[idx + 1];
                let b = pixels[idx + 2];
                let a = pixels[idx + 3];

                // Check if pixel is non-transparent and not pure black
                // (window content should have some visible pixels)
                if a > 0 && (r > 0 || g > 0 || b > 0) {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                    found_content = true;
                }
            }
        }

        if !found_content || min_x >= max_x || min_y >= max_y {
            return None;
        }

        // Add small padding to avoid cutting off edge pixels
        let pad = 2;
        let x = (min_x.saturating_sub(pad)) as i32;
        let y = (min_y.saturating_sub(pad)) as i32;
        let w = ((max_x - min_x + 1).min(width - min_x) + pad * 2) as i32;
        let h = ((max_y - min_y + 1).min(height - min_y) + pad * 2) as i32;

        // Only crop if the detected bounds are significantly smaller than the frame
        // (at least 10% smaller in at least one dimension)
        if w < (width as i32 * 90 / 100) || h < (height as i32 * 90 / 100) {
            Some((x, y, w, h))
        } else {
            None
        }
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

    /// Run a full-screen monitor capture via the ScreenCast portal + PipeWire.
    ///
    /// Always uses the ScreenCast path for full customization and cross-distro
    /// consistency. wlr-screencopy, grim, and the Screenshot portal are bypassed.
    pub fn capture_screen_impl(&self) -> DisplayResult<CaptureData> {
        if Self::should_use_screenshot_portal() {
            let start = std::time::Instant::now();
            let result = block_on_async(Self::capture_via_screenshot_portal(false));
            match &result {
                Ok(d) => eprintln!(
                    "[capture] Screenshot portal succeeded in {:.0}ms ({}x{}).",
                    start.elapsed().as_millis(),
                    d.width,
                    d.height
                ),
                Err(e) => eprintln!(
                    "[capture] Screenshot portal failed ({:.0}ms): {e}",
                    start.elapsed().as_millis()
                ),
            }
            return result;
        }

        if let Some(result) = Self::capture_monitor_via_native_screencopy() {
            return result;
        }

        Self::capture_monitor_via_screencast()
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

        let full = self.capture_screen_for_selection_impl()?;
        crop_capture(full, x, y, width, height)
    }

    /// Capture used for direct area crops on Wayland.
    ///
    /// This intentionally stays on the native wlr-screencopy path. We do not
    /// fall back to the ScreenCast portal for screenshot/area capture here.
    pub fn capture_screen_for_selection_impl(&self) -> DisplayResult<CaptureData> {
        eprintln!("[capture] capture_screen_for_selection_impl: using native wlr-screencopy only");
        match Self::capture_monitor_via_native_screencopy() {
            Some(result) => result,
            None => Err(DisplayError::CaptureError(
                "Native wlr-screencopy is unavailable for Wayland area capture".into(),
            )),
        }
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
        // Try the ScreenCast portal first (works on GNOME/KDE with proper portal).
        // This shows the portal's own source-selection dialog to the user.
        let portal_result = block_on_async(async {
            Self::capture_via_screencast(CaptureTarget::Window, true).await
        });

        if portal_result.is_ok() {
            return portal_result;
        }

        // Fallback for wlroots compositors (Hyprland, Sway, etc.) where the
        // ScreenCast portal may not be available: capture the full screen via
        // wlr-screencopy, then crop to the active window bounds from the
        // compositor's window list.
        if let Some(compositor) = crate::compositor::detect_compositor() {
            if let Ok(Some(window)) = compositor.get_active_window() {
                eprintln!(
                    "[wayland] Portal unavailable, falling back to wlr-screencopy window crop: {} \"{}\" at {}x{}+{}x{}",
                    window.class, window.title, window.x, window.y, window.width, window.height
                );
                let full = self.capture_screen_for_selection_impl()?;
                // Clamp crop to valid screen bounds
                let x = window.x.max(0);
                let y = window.y.max(0);
                let width = window.width.min(full.width as i32 - x).max(1);
                let height = window.height.min(full.height as i32 - y).max(1);
                return self.capture_area_direct_impl(x, y, width, height);
            }
        }

        portal_result
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
    fn screencast_waits_for_portal_dialog_only_without_restore_token() {
        assert!(should_wait_for_portal_dialog_to_close(None));
        assert!(!should_wait_for_portal_dialog_to_close(Some(
            "restore-token"
        )));
        const { assert!(PORTAL_DIALOG_DISMISSAL_DELAY_MS >= 500) };
    }
}
