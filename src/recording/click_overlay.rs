//! Click overlay rendering for non-GNOME recordings.
//!
//! When GNOME Shell is not available, clicks are rendered directly into
//! the recorded video via a GStreamer `cairooverlay` element.  This module
//! tracks mouse click events and provides a `draw` callback the pipeline
//! can use on every frame.

use std::sync::{Arc, Mutex};
use std::time::Instant;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt;

// ── Click settings ────────────────────────────────────────────────────────

/// Mirrors the colour palette used by the click-options popup.
pub(crate) static CLICK_COLORS: &[(f64, f64, f64)] = &[
    (0.71, 0.71, 0.71), // Gray
    (0.48, 0.39, 1.0),  // Indigo
    (1.0, 0.24, 0.24),  // Red
    (0.24, 0.47, 1.0),  // Blue
    (0.24, 0.78, 0.31), // Green
    (1.0, 0.82, 0.20),  // Yellow
    (1.0, 0.59, 0.12),  // Orange
    (0.71, 0.24, 0.86), // Purple
    (1.0, 1.0, 1.0),    // White
];

#[derive(Debug, Clone, Copy)]
pub(crate) struct ClickOverlayConfig {
    #[allow(dead_code)]
    pub enabled: bool,
    pub size: f64, // 0.0 – 1.0
    pub color: u8, // index into CLICK_COLORS
    pub style: u8, // 0 = Outline, 1 = Filled
    pub animate: bool,
}

impl Default for ClickOverlayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            size: 0.5,
            color: 3,
            style: 0,
            animate: true,
        }
    }
}

// ── Click event ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub(crate) struct ClickEvent {
    /// Normalised position within the captured area (0.0 – 1.0).
    pub x: f64,
    pub y: f64,
    /// When the click was detected.
    pub at: Instant,
}

// ── Click tracker (shared, thread-safe) ───────────────────────────────────

#[derive(Clone)]
pub(crate) struct ClickTracker {
    inner: Arc<Mutex<ClickTrackerInner>>,
}

struct ClickTrackerInner {
    clicks: Vec<ClickEvent>,
    config: ClickOverlayConfig,
    /// Capture area dimensions in pixels (for normalisation).
    area_w: u32,
    area_h: u32,
}

impl ClickTracker {
    pub fn new(area_w: u32, area_h: u32, config: ClickOverlayConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ClickTrackerInner {
                clicks: Vec::new(),
                config,
                area_w,
                area_h,
            })),
        }
    }

    /// Record a click at a screen position within the capture area.
    pub fn record_click(&self, screen_x: i32, screen_y: i32, capture_x: i32, capture_y: i32) {
        // Normalise to [0,1] within capture area
        let inner = self.inner.lock().unwrap();
        if inner.area_w == 0 || inner.area_h == 0 {
            return;
        }
        let x = ((screen_x - capture_x) as f64 / inner.area_w as f64).clamp(0.0, 1.0);
        let y = ((screen_y - capture_y) as f64 / inner.area_h as f64).clamp(0.0, 1.0);
        drop(inner);

        let mut inner = self.inner.lock().unwrap();
        inner.clicks.push(ClickEvent {
            x,
            y,
            at: Instant::now(),
        });
    }

    /// Expire clicks older than `max_age` and return the survivors.
    pub fn active_clicks(&self, max_age: std::time::Duration) -> Vec<ClickEvent> {
        let mut inner = self.inner.lock().unwrap();
        let now = Instant::now();
        inner.clicks.retain(|c| now - c.at < max_age);
        inner.clicks.clone()
    }

    pub fn config(&self) -> ClickOverlayConfig {
        self.inner.lock().unwrap().config
    }
}

// ── Cairo rendering ───────────────────────────────────────────────────────

/// Helper: draw a single click indicator on a Cairo context.
///
/// `frame_w` / `frame_h` are the video frame dimensions in pixels;
/// the click coordinates (0-1) are mapped onto them.
/// `elapsed` is the time since the click, used for animation.
pub(crate) fn draw_click_indicator(
    ctx: &gtk4::cairo::Context,
    click: &ClickEvent,
    elapsed: std::time::Duration,
    frame_w: f64,
    frame_h: f64,
    config: &ClickOverlayConfig,
) {
    let max_age = std::time::Duration::from_millis(800);
    if elapsed > max_age {
        return;
    }

    let cx = click.x * frame_w;
    let cy = click.y * frame_h;

    // Radius: 10–60 px based on size (0.0–1.0)
    let base_radius = (10.0 + config.size * 50.0).min(frame_w.min(frame_h) * 0.15);

    // Animation curve
    let t = elapsed.as_secs_f64() / max_age.as_secs_f64(); // 0.0 → 1.0
    let (radius, alpha) = if config.animate {
        if t < 0.25 {
            // Grow phase
            let grow = t / 0.25;
            (base_radius * (0.5 + 0.5 * grow), 1.0)
        } else {
            // Fade phase
            let fade = 1.0 - (t - 0.25) / 0.75;
            (base_radius, fade.clamp(0.0, 1.0))
        }
    } else {
        // No animation – appear for 500 ms then disappear
        if t < 0.625 {
            (base_radius, 1.0)
        } else {
            return;
        }
    };

    let (r, g, b) = CLICK_COLORS[config.color as usize % CLICK_COLORS.len()];

    match config.style {
        1 => {
            // Filled circle
            ctx.set_source_rgba(r, g, b, alpha * 0.65);
            ctx.new_path();
            ctx.arc(cx, cy, radius, 0.0, 2.0 * std::f64::consts::PI);
            ctx.fill().ok();
            // Thin outline
            ctx.set_source_rgba(r, g, b, alpha);
            ctx.set_line_width(2.0);
            ctx.new_path();
            ctx.arc(cx, cy, radius, 0.0, 2.0 * std::f64::consts::PI);
            ctx.stroke().ok();
        }
        _ => {
            // Outline circle (style 0 = Outline)
            ctx.set_source_rgba(r, g, b, alpha);
            ctx.set_line_width(3.0);
            ctx.new_path();
            ctx.arc(cx, cy, radius, 0.0, 2.0 * std::f64::consts::PI);
            ctx.stroke().ok();
        }
    }
}

/// Draw all active click indicators on a Cairo context of the given frame size.
pub(crate) fn draw_click_overlay(
    ctx: &gtk4::cairo::Context,
    frame_w: f64,
    frame_h: f64,
    tracker: &ClickTracker,
) {
    let config = tracker.config();
    let clicks = tracker.active_clicks(std::time::Duration::from_millis(1000));
    let now = Instant::now();
    for click in &clicks {
        let elapsed = now - click.at;
        draw_click_indicator(ctx, click, elapsed, frame_w, frame_h, &config);
    }
}

// ── Mouse polling (X11) ───────────────────────────────────────────────────

/// Start a background thread that polls the X11 pointer position and button
/// state during recording.  When a button press is detected inside the capture
/// area, a `ClickEvent` is appended to the tracker.
fn start_click_polling_x11(
    tracker: ClickTracker,
    capture_x: i32,
    capture_y: i32,
    capture_w: u32,
    capture_h: u32,
) -> Option<ClickPollingHandle> {
    let (conn, _screen_num) = match x11rb::connect(None) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[click-overlay] Cannot connect to X11 display: {e}");
            return None;
        }
    };

    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running_clone = running.clone();

    std::thread::Builder::new()
        .name("click-poll-x11".into())
        .spawn(move || {
            let root = conn.setup().roots[_screen_num].root;
            let mut prev_buttons: u16 = 0;
            let interval = std::time::Duration::from_millis(10);

            while running_clone.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(cookie) = conn.query_pointer(root) {
                    if let Ok(reply) = cookie.reply() {
                        let px = reply.root_x as i32;
                        let py = reply.root_y as i32;
                        let buttons = reply.mask;

                        let newly_pressed = u16::from(buttons) & !prev_buttons;
                        if newly_pressed != 0
                            && px >= capture_x
                            && py >= capture_y
                            && px < capture_x + capture_w as i32
                            && py < capture_y + capture_h as i32
                        {
                            tracker.record_click(px, py, capture_x, capture_y);
                        }
                        prev_buttons = u16::from(buttons);
                    } else {
                        break;
                    }
                } else {
                    break;
                }
                std::thread::sleep(interval);
            }
        })
        .ok()
        .map(|_handle| ClickPollingHandle { running })
}

// ── Wayland compositor click polling ───────────────────────────────────────

/// Connect to the Hyprland event socket (`.socket2.sock`) and listen for
/// `mousebuttondown` events.  Mouse position is tracked via `mousemove` events.
fn start_click_polling_hyprland(
    tracker: ClickTracker,
    capture_x: i32,
    capture_y: i32,
    capture_w: u32,
    capture_h: u32,
) -> Option<ClickPollingHandle> {
    use std::io::{BufRead, BufReader};

    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    let mut path = std::path::PathBuf::from(runtime);
    path.push("hypr");
    path.push(&sig);
    path.push(".socket2.sock");

    let stream = match std::os::unix::net::UnixStream::connect(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[click-overlay] Cannot connect to Hyprland socket2: {e}");
            return None;
        }
    };

    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running_clone = running.clone();

    std::thread::Builder::new()
        .name("click-poll-hypr".into())
        .spawn(move || {
            let reader = BufReader::new(stream);
            let mut cursor_x: f64 = 0.0;
            let mut cursor_y: f64 = 0.0;

            for line in reader.lines() {
                if !running_clone.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let line = match line {
                    Ok(l) => l,
                    Err(_) => break,
                };

                if let Some(rest) = line.strip_prefix("mousemove>>") {
                    // Format: "x,y"
                    if let Some((x_str, y_str)) = rest.split_once(',') {
                        cursor_x = x_str.trim().parse().unwrap_or(cursor_x);
                        cursor_y = y_str.trim().parse().unwrap_or(cursor_y);
                    }
                } else if line.starts_with("mousebuttondown>>") {
                    let px = cursor_x as i32;
                    let py = cursor_y as i32;
                    if px >= capture_x
                        && py >= capture_y
                        && px < capture_x + capture_w as i32
                        && py < capture_y + capture_h as i32
                    {
                        tracker.record_click(px, py, capture_x, capture_y);
                    }
                }
            }
        })
        .ok()
        .map(|_handle| ClickPollingHandle { running })
}

/// Connect to Sway IPC and subscribe to input events, parsing mouse button
/// presses to record clicks.
fn start_click_polling_sway(
    tracker: ClickTracker,
    capture_x: i32,
    capture_y: i32,
    capture_w: u32,
    capture_h: u32,
) -> Option<ClickPollingHandle> {
    use std::io::{Read, Write};

    let socket_path = std::env::var_os("SWAYSOCK")
        .or_else(|| std::env::var_os("I3SOCK"))
        .map(std::path::PathBuf::from)?;

    let mut stream = match std::os::unix::net::UnixStream::connect(&socket_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[click-overlay] Cannot connect to Sway socket: {e}");
            return None;
        }
    };

    // Sway IPC subscribe to input events (type 2, payload '["input"]')
    let payload = r#"["input"]"#;
    let magic = b"i3-ipc";
    let len = payload.len() as u32;
    let msg_type: u32 = 2; // SUBSCRIBE

    let mut header = Vec::new();
    header.extend_from_slice(magic);
    header.extend_from_slice(&len.to_ne_bytes());
    header.extend_from_slice(&msg_type.to_ne_bytes());

    if stream.write_all(&header).is_err() || stream.write_all(payload.as_bytes()).is_err() {
        eprintln!("[click-overlay] Failed to subscribe to Sway IPC input events");
        return None;
    }

    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running_clone = running.clone();

    std::thread::Builder::new()
        .name("click-poll-sway".into())
        .spawn(move || {
            let mut header_buf = [0u8; 14];
            let mut payload_buf = Vec::new();

            loop {
                if !running_clone.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }

                if stream.read_exact(&mut header_buf).is_err() {
                    break;
                }
                let payload_len = u32::from_ne_bytes([
                    header_buf[6],
                    header_buf[7],
                    header_buf[8],
                    header_buf[9],
                ]) as usize;

                payload_buf.resize(payload_len, 0);
                if stream.read_exact(&mut payload_buf).is_err() {
                    break;
                }

                let Ok(text) = std::str::from_utf8(&payload_buf) else {
                    continue;
                };

                // Sway IPC sends JSON with "change":"run" events for each input.
                // Look for mouse button press events.
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(text) {
                    if event.get("change").and_then(|v| v.as_str()) == Some("run") {
                        if let Some(input) = event.get("input") {
                            if input.get("type").and_then(|v| v.as_i64()) == Some(4) {
                                // Pointer event
                                if input.get("state").and_then(|v| v.as_str()) == Some("pressed") {
                                    let px =
                                        input.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                    let py =
                                        input.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                    if px >= capture_x
                                        && py >= capture_y
                                        && px < capture_x + capture_w as i32
                                        && py < capture_y + capture_h as i32
                                    {
                                        tracker.record_click(px, py, capture_x, capture_y);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        })
        .ok()
        .map(|_handle| ClickPollingHandle { running })
}

/// Start click polling: tries X11 first, then Wayland compositor-specific
/// event sockets (Hyprland, Sway).
pub(crate) fn start_click_polling(
    tracker: ClickTracker,
    capture_x: i32,
    capture_y: i32,
    capture_w: u32,
    capture_h: u32,
) -> Option<ClickPollingHandle> {
    // Try X11 first
    if std::env::var("DISPLAY").is_ok() {
        return start_click_polling_x11(
            tracker.clone(),
            capture_x,
            capture_y,
            capture_w,
            capture_h,
        );
    }

    // Wayland compositor-specific
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        return start_click_polling_hyprland(tracker, capture_x, capture_y, capture_w, capture_h);
    }
    if std::env::var_os("SWAYSOCK").is_some() || std::env::var_os("I3SOCK").is_some() {
        return start_click_polling_sway(tracker, capture_x, capture_y, capture_w, capture_h);
    }

    eprintln!("[click-overlay] No supported compositor found for click polling");
    None
}

pub(crate) struct ClickPollingHandle {
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for ClickPollingHandle {
    fn drop(&mut self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}
