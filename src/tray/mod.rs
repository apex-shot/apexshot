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
    OpenRecentCaptures,
    ShowLastPreview,
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

/// Generate the new 'A-Mark' tray icon procedurally as raw ARGB32 bytes.
///
/// This provides razor-sharp, pixel-perfect lines by drawing the logo 
/// directly using geometric primitives at the desired resolution.
fn apex_icon(size: i32) -> ksni::Icon {
    use gtk4::cairo::{Context, Format, ImageSurface, LineCap, LineJoin};
    let mut surface = ImageSurface::create(Format::ARgb32, size, size)
        .expect("Failed to create tray icon surface");
    let cr = Context::new(&surface).expect("Failed to create context");

    let cx = size as f64 / 2.0;
    let cy = size as f64 / 2.0;

    // Transparent background for system tray
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    cr.paint().expect("Failed to clear tray transparent background");

    // Viewfinder / Crop Corners
    cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    cr.set_line_width(size as f64 * 0.08);
    cr.set_line_cap(LineCap::Square);
    cr.set_line_join(LineJoin::Miter);

    let crn_dist = size as f64 * 0.40;
    let crn_len = size as f64 * 0.20;

    // Top Left
    cr.move_to(cx - crn_dist, cy - crn_dist + crn_len);
    cr.line_to(cx - crn_dist, cy - crn_dist);
    cr.line_to(cx - crn_dist + crn_len, cy - crn_dist);
    cr.stroke().expect("Failed to draw tray icon");
    // Top Right
    cr.move_to(cx + crn_dist - crn_len, cy - crn_dist);
    cr.line_to(cx + crn_dist, cy - crn_dist);
    cr.line_to(cx + crn_dist, cy - crn_dist + crn_len);
    cr.stroke().expect("Failed to draw tray icon");
    // Bottom Right
    cr.move_to(cx + crn_dist, cy + crn_dist - crn_len);
    cr.line_to(cx + crn_dist, cy + crn_dist);
    cr.line_to(cx + crn_dist - crn_len, cy + crn_dist);
    cr.stroke().expect("Failed to draw tray icon");
    // Bottom Left
    cr.move_to(cx - crn_dist + crn_len, cy + crn_dist);
    cr.line_to(cx - crn_dist, cy + crn_dist);
    cr.line_to(cx - crn_dist, cy + crn_dist - crn_len);
    cr.stroke().expect("Failed to draw tray icon");

    // The Peak / Apex
    let peak_y = cy - size as f64 * 0.12;
    let base_y = cy + size as f64 * 0.22;
    let peak_half_w = size as f64 * 0.26;

    cr.move_to(cx, peak_y);
    cr.line_to(cx + peak_half_w, base_y);
    cr.line_to(cx - peak_half_w, base_y);
    cr.close_path();
    cr.fill().expect("Failed to draw tray icon");

    // Theme Orange (#b05c38) Shadow / Slice on the peak
    cr.set_source_rgba(0.69, 0.36, 0.22, 1.0); 
    cr.move_to(cx, peak_y);
    cr.line_to(cx + peak_half_w, base_y);
    cr.line_to(cx, base_y);
    cr.close_path();
    cr.fill().expect("Failed to draw tray icon");

    drop(cr);
    surface.flush();

    let stride = surface.stride() as usize;
    let width = size as usize;
    let height = size as usize;
    let mut pixels = vec![0u8; width * height * 4];

    {
        let data = surface.data().expect("Failed to extract cairo surface data");
        // Extract raw stride rows into exact contiguous W * 4 buffer
        for y in 0..height {
            let src_start = y * stride;
            let src_end = src_start + width * 4;
            let dst_start = y * width * 4;
            let dst_end = dst_start + width * 4;
            
            pixels[dst_start..dst_end].copy_from_slice(&data[src_start..src_end]);
        }
    }

    // Convert Cairo native-endian ARGB32 (which is BGRA on little-endian) to KsNi expected ARGB byte order.
    // KsNi expects exactly: [A, R, G, B] per pixel.
    for pixel in pixels.chunks_exact_mut(4) {
        let b = pixel[0];
        let g = pixel[1];
        let r = pixel[2];
        let a = pixel[3];
        // Swapping to ksni network byte order format
        pixel[0] = a;
        pixel[1] = r;
        pixel[2] = g;
        pixel[3] = b;
    }

    ksni::Icon {
        width: size,
        height: size,
        data: pixels,
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

        let ltr = |label: &str| format!("\u{200E}{label}");

        vec![
            // ── Capture section ──────────────────────────────────────────
            StandardItem {
                label: ltr("Capture Area"),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::CaptureArea)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: ltr("Capture Screen"),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::CaptureScreen)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: ltr("Capture Window"),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::CaptureWindow)),
                ..Default::default()
            }
            .into(),
            // ── Separator ────────────────────────────────────────────────
            MenuItem::Separator,
            // ── Recording section ─────────────────────────────────────────
            StandardItem {
                label: ltr("Record Screen"),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::RecordScreen)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: ltr("Record Area"),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::RecordArea)),
                ..Default::default()
            }
            .into(),
            // ── Separator ────────────────────────────────────────────────
            MenuItem::Separator,
            // ── Utility section ───────────────────────────────────────────
            StandardItem {
                label: ltr("Recent Captures"),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::OpenRecentCaptures)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: ltr("Open Last Capture"),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::OpenLastCapture)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: ltr("Settings"),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::OpenSettings)),
                ..Default::default()
            }
            .into(),
            // ── Separator ────────────────────────────────────────────────
            MenuItem::Separator,
            // ── Quit ─────────────────────────────────────────────────────
            StandardItem {
                label: ltr("Quit"),
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
