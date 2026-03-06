//! System tray icon for ApexShot.
//!
//! Uses the `ksni` crate which implements the StatusNotifierItem D-Bus protocol.
//! This is supported on:
//!   - GNOME (via the AppIndicator extension, pre-installed on Ubuntu)
//!   - KDE Plasma (native support)
//!   - Any desktop that supports StatusNotifierItem / AppIndicator
//!
//! The tray icon runs on its own thread (ksni spawns it).  When the user
//! clicks a menu item, the action is sent through a channel to the daemon's
//! main loop, which executes it on the GTK main thread.

use std::sync::mpsc::Sender;

/// Actions that can be triggered from the tray menu.
#[derive(Debug, Clone)]
pub enum TrayAction {
    CaptureArea,
    CaptureScreen,
    CaptureWindow,
    RecordScreen,
    RecordArea,
    OpenLastCapture,
    OpenSettings,
    Quit,
}

/// The ksni tray icon state.
pub struct ApexShotTray {
    /// Channel to send actions to the daemon main loop.
    tx: Sender<TrayAction>,
}

impl ApexShotTray {
    pub fn new(tx: Sender<TrayAction>) -> Self {
        Self { tx }
    }

    fn send(&self, action: TrayAction) {
        let _ = self.tx.send(action);
    }
}

/// Blend a pixel with white color and given alpha.
#[inline]
fn blend_white(pixels: &mut [u8], w: usize, h: usize, x: usize, y: usize, alpha: f32) {
    if x < w && y < h {
        let off = (y * w + x) * 4;
        let a = (alpha * 255.0).round() as u8;
        if a > pixels[off] {
            pixels[off] = a;
            pixels[off + 1] = 255;
            pixels[off + 2] = 255;
            pixels[off + 3] = 255;
        }
    }
}

/// Generate a white 'A' reticle tray icon as raw ARGB32 bytes.
fn apex_icon(size: i32) -> ksni::Icon {
    let w = size as usize;
    let h = size as usize;
    let mut pixels: Vec<u8> = vec![0u8; w * h * 4];

    let s = size as f32 / 22.0;

    let stroke_width = 1.8 * s;
    let apex_x = 11.0 * s;
    let apex_y = 3.5 * s;
    let left_x = 4.0 * s;
    let right_x = 18.0 * s;
    let bot_y = 18.5 * s;

    let dash_y = 12.0 * s;
    let dot_x = 11.0 * s;
    let dot_y = 7.0 * s;
    let dot_r = 1.2 * s;

    let dist_to_segment = |px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32| -> f32 {
        let l2 = (x1 - x2) * (x1 - x2) + (y1 - y2) * (y1 - y2);
        if l2 == 0.0 {
            return ((px - x1) * (px - x1) + (py - y1) * (py - y1)).sqrt();
        }
        let mut t = ((px - x1) * (x2 - x1) + (py - y1) * (y2 - y1)) / l2;
        if t < 0.0 {
            t = 0.0;
        }
        if t > 1.0 {
            t = 1.0;
        }
        let proj_x = x1 + t * (x2 - x1);
        let proj_y = y1 + t * (y2 - y1);
        ((px - proj_x) * (px - proj_x) + (py - proj_y) * (py - proj_y)).sqrt()
    };

    for y in 0..h {
        for x in 0..w {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let mut alpha = 0.0_f32;

            // distance to left/right leg
            let d_left = dist_to_segment(fx, fy, apex_x, apex_y, left_x, bot_y);
            let d_right = dist_to_segment(fx, fy, apex_x, apex_y, right_x, bot_y);

            let d_legs = d_left.min(d_right);
            // soft anti-aliasing
            let a_legs = (stroke_width - d_legs + 0.5).clamp(0.0, 1.0);
            alpha = alpha.max(a_legs);

            // distance to dashed line
            let dash_thickness = 0.75 * s;
            let dy = (fy - dash_y).abs();
            if dy < dash_thickness + 0.5 {
                if fx > 1.0 * s && fx < 21.0 * s {
                    let dash_len = 2.5 * s;
                    let dash_pos = (fx - 1.0 * s) % dash_len;
                    if dash_pos < 1.4 * s {
                        let a_dash = (dash_thickness - dy + 0.5).clamp(0.0, 1.0);
                        alpha = alpha.max(a_dash);
                    }
                }
            }

            // distance to dot
            let d_dot = ((fx - dot_x) * (fx - dot_x) + (fy - dot_y) * (fy - dot_y)).sqrt();
            let a_dot = (dot_r - d_dot + 0.5).clamp(0.0, 1.0);
            alpha = alpha.max(a_dot);

            if alpha > 0.0 {
                blend_white(&mut pixels, w, h, x, y, alpha);
            }
        }
    }

    ksni::Icon {
        width: size,
        height: size,
        data: pixels,
    }
}

/// Generate a small PNG icon matching the toolbar's Area tool (L-corner brackets).
fn capture_area_menu_icon_png() -> Vec<u8> {
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

    let size = 18u32;
    let mut img = RgbaImage::from_pixel(size, size, Rgba([0, 0, 0, 0]));

    // Neutral gray for readability on both light/dark menus.
    let stroke = Rgba([120, 124, 132, 255]);

    let cx = (size as f32 - 1.0) / 2.0;
    let cy = cx;
    let h = 5.2f32;
    let stroke_half = 0.85f32;

    let segments = [
        // Top-left corner
        (cx - 7.0, cy - 1.5, cx - 7.0, cy - h),
        (cx - 7.0, cy - h, cx - 1.5, cy - h),
        // Top-right corner
        (cx + 1.5, cy - h, cx + 7.0, cy - h),
        (cx + 7.0, cy - h, cx + 7.0, cy - 1.5),
        // Bottom-left corner
        (cx - 7.0, cy + 1.5, cx - 7.0, cy + h),
        (cx - 7.0, cy + h, cx - 1.5, cy + h),
        // Bottom-right corner
        (cx + 1.5, cy + h, cx + 7.0, cy + h),
        (cx + 7.0, cy + h, cx + 7.0, cy + 1.5),
    ];

    let dist_to_segment = |px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32| -> f32 {
        let l2 = (x1 - x2) * (x1 - x2) + (y1 - y2) * (y1 - y2);
        if l2 == 0.0 {
            return ((px - x1) * (px - x1) + (py - y1) * (py - y1)).sqrt();
        }
        let mut t = ((px - x1) * (x2 - x1) + (py - y1) * (y2 - y1)) / l2;
        t = t.clamp(0.0, 1.0);
        let qx = x1 + t * (x2 - x1);
        let qy = y1 + t * (y2 - y1);
        ((px - qx) * (px - qx) + (py - qy) * (py - qy)).sqrt()
    };

    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let mut alpha = 0.0f32;

            for &(x1, y1, x2, y2) in &segments {
                let d = dist_to_segment(fx, fy, x1, y1, x2, y2);
                let a = (stroke_half - d + 0.5).clamp(0.0, 1.0);
                alpha = alpha.max(a);
            }

            if alpha > 0.0 {
                let mut color = stroke;
                color.0[3] = (alpha * 255.0).round() as u8;
                img.put_pixel(x, y, color);
            }
        }
    }

    let mut png = Vec::new();
    if DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)
        .is_ok()
    {
        png
    } else {
        Vec::new()
    }
}

impl ksni::Tray for ApexShotTray {
    fn activate(&mut self, _x: i32, _y: i32) {
        // Primary click fallback on hosts that don't open the context menu on left-click.
        self.send(TrayAction::CaptureArea);
    }

    fn icon_name(&self) -> String {
        // Empty string forces the tray to use icon_pixmap instead.
        String::new()
    }

    fn id(&self) -> String {
        "io.github.codegoddy.apexshot".to_string()
    }

    fn text_direction(&self) -> ksni::TextDirection {
        // Keep icon column on the leading (left) side in LTR locales.
        ksni::TextDirection::LeftToRight
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        // Provide multiple sizes for HiDPI support.
        vec![apex_icon(16), apex_icon(22), apex_icon(32)]
    }

    fn title(&self) -> String {
        "ApexShot".to_string()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            icon_name: String::new(),
            icon_pixmap: vec![apex_icon(22)],
            title: "ApexShot".to_string(),
            description: "Left-click: Capture Area • Right-click: Menu".to_string(),
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        let capture_area_icon = capture_area_menu_icon_png();
        let ltr = |label: &str| format!("\u{200E}{label}");

        vec![
            // ── Capture section ──────────────────────────────────────────
            StandardItem {
                label: ltr("Capture Area"),
                icon_name: String::new(),
                icon_data: capture_area_icon,
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::CaptureArea)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: ltr("Capture Screen"),
                icon_name: "computer-symbolic".to_string(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::CaptureScreen)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: ltr("Capture Window"),
                icon_name: "window-symbolic".to_string(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::CaptureWindow)),
                ..Default::default()
            }
            .into(),
            // ── Separator ────────────────────────────────────────────────
            MenuItem::Separator,
            // ── Recording section ─────────────────────────────────────────
            StandardItem {
                label: ltr("Record Screen"),
                icon_name: "media-record-symbolic".to_string(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::RecordScreen)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: ltr("Record Area"),
                icon_name: "media-record-symbolic".to_string(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::RecordArea)),
                ..Default::default()
            }
            .into(),
            // ── Separator ────────────────────────────────────────────────
            MenuItem::Separator,
            // ── Utility section ───────────────────────────────────────────
            StandardItem {
                label: ltr("Open Last Capture"),
                icon_name: "document-open-symbolic".to_string(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::OpenLastCapture)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: ltr("Settings"),
                icon_name: "preferences-system-symbolic".to_string(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::OpenSettings)),
                ..Default::default()
            }
            .into(),
            // ── Separator ────────────────────────────────────────────────
            MenuItem::Separator,
            // ── Quit ─────────────────────────────────────────────────────
            StandardItem {
                label: ltr("Quit"),
                icon_name: "application-exit-symbolic".to_string(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Spawn the tray icon on its own thread.
///
/// Returns a `Sender` to receive `TrayAction` events from menu clicks,
/// and a `ksni::Handle` that can be used to update the tray state.
///
/// The tray runs until the handle is dropped or `Quit` is triggered.
pub fn spawn_tray(tx: Sender<TrayAction>) -> anyhow::Result<ksni::Handle<ApexShotTray>> {
    let service = ksni::TrayService::new(ApexShotTray::new(tx));
    let handle = service.handle();
    service.spawn();
    Ok(handle)
}
