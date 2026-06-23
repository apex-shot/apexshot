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
    CaptureCrosshair,
    CaptureScreen,
    CaptureWindow,
    OpenRecordingUi,
    OpenVideoEditor,
    RecordScreen,
    ToggleRecordingPause,
    StopRecordingSave,
    RestartRecording,
    DiscardRecording,
    ShowLastPreview,
    OpenLastCapture,
    OpenSettings,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TrayPresentation {
    Idle,
    Recording { elapsed_text: String, paused: bool },
}

/// The ksni tray icon state.
pub struct ApexShotTray {
    /// Channel to send actions to the daemon main loop.
    tx: Sender<TrayAction>,
    presentation: TrayPresentation,
}

impl ApexShotTray {
    pub fn new(tx: Sender<TrayAction>) -> Self {
        Self {
            tx,
            presentation: TrayPresentation::Idle,
        }
    }

    fn send(&self, action: TrayAction) {
        let _ = self.tx.send(action);
    }

    pub fn show_recording_timer(&mut self, elapsed_text: impl Into<String>) {
        self.presentation = TrayPresentation::Recording {
            elapsed_text: elapsed_text.into(),
            paused: false,
        };
    }

    pub fn show_recording_paused(&mut self, elapsed_text: impl Into<String>) {
        self.presentation = TrayPresentation::Recording {
            elapsed_text: elapsed_text.into(),
            paused: true,
        };
    }

    pub fn show_idle(&mut self) {
        self.presentation = TrayPresentation::Idle;
    }
}

fn icon_from_surface(mut surface: gtk4::cairo::ImageSurface, size: i32) -> ksni::Icon {
    surface.flush();

    let stride = surface.stride() as usize;
    let width = size as usize;
    let height = size as usize;
    let mut pixels = vec![0u8; width * height * 4];

    {
        let data = surface
            .data()
            .expect("Failed to extract cairo surface data");
        for y in 0..height {
            let src_start = y * stride;
            let src_end = src_start + width * 4;
            let dst_start = y * width * 4;
            let dst_end = dst_start + width * 4;
            pixels[dst_start..dst_end].copy_from_slice(&data[src_start..src_end]);
        }
    }

    for pixel in pixels.chunks_exact_mut(4) {
        let b = pixel[0];
        let g = pixel[1];
        let r = pixel[2];
        let a = pixel[3];
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

/// Generate the default ApexShot tray icon procedurally.
fn apex_icon(size: i32) -> ksni::Icon {
    use gtk4::cairo::{Context, Format, ImageSurface};
    let surface = ImageSurface::create(Format::ARgb32, size, size)
        .expect("Failed to create tray icon surface");
    let cr = Context::new(&surface).expect("Failed to create context");

    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    cr.paint()
        .expect("Failed to clear tray transparent background");

    let s = size as f64 / 24.0;
    cr.scale(s, s);

    cr.set_source_rgba(0.913, 0.329, 0.125, 1.0);
    cr.set_line_width(2.5);
    cr.set_line_cap(gtk4::cairo::LineCap::Round);
    cr.move_to(2.0, 21.0);
    cr.curve_to(6.0, 21.0, 8.0, 2.0, 12.0, 2.0);
    cr.curve_to(16.0, 2.0, 18.0, 21.0, 22.0, 21.0);
    cr.stroke().expect("Failed to draw tray icon logo");

    drop(cr);
    icon_from_surface(surface, size)
}

fn recording_circle_icon(size: i32, paused: bool) -> ksni::Icon {
    use gtk4::cairo::{Context, Format, ImageSurface};
    let surface = ImageSurface::create(Format::ARgb32, size, size)
        .expect("Failed to create recording tray icon surface");
    let cr = Context::new(&surface).expect("Failed to create recording icon context");

    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    cr.paint()
        .expect("Failed to clear recording tray transparent background");

    let w = size as f64;
    let h = size as f64;
    let diameter = (w.min(h) * 0.82).max(9.0);
    let radius = diameter / 2.0;
    let cx = w / 2.0;
    let cy = h / 2.0;

    if paused {
        cr.set_source_rgba(0.74, 0.33, 0.06, 1.0);
    } else {
        cr.set_source_rgba(0.89, 0.16, 0.21, 1.0);
    }
    cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
    cr.fill().expect("Failed to paint recording circle icon");

    cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    if paused {
        let bar_h = (diameter * 0.42).round();
        let bar_w = (diameter * 0.12).max(2.0).round();
        let gap = (diameter * 0.10).round();
        let total_w = bar_w * 2.0 + gap;
        let start_x = (w - total_w) / 2.0;
        let start_y = (h - bar_h) / 2.0;
        cr.rectangle(start_x, start_y, bar_w, bar_h);
        cr.rectangle(start_x + bar_w + gap, start_y, bar_w, bar_h);
        cr.fill()
            .expect("Failed to paint pause glyph in recording icon");
    } else {
        let stop_size = (diameter * 0.34).round();
        let stop_x = (w - stop_size) / 2.0;
        let stop_y = (h - stop_size) / 2.0;
        cr.rectangle(stop_x, stop_y, stop_size, stop_size);
        cr.fill()
            .expect("Failed to paint stop glyph in recording icon");
    }

    drop(cr);
    icon_from_surface(surface, size)
}

impl ksni::Tray for ApexShotTray {
    fn activate(&mut self, _x: i32, _y: i32) {
        // Primary click fallback on hosts that don't open the context menu on left-click.
        match self.presentation {
            TrayPresentation::Idle => self.send(TrayAction::CaptureArea),
            TrayPresentation::Recording { .. } => self.send(TrayAction::StopRecordingSave),
        }
    }

    fn icon_name(&self) -> String {
        // Empty string forces the tray to use icon_pixmap instead.
        String::new()
    }

    fn id(&self) -> String {
        crate::app_identity::app_id().to_string()
    }

    fn text_direction(&self) -> ksni::TextDirection {
        // Keep icon column on the leading (left) side in LTR locales.
        ksni::TextDirection::LeftToRight
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        // Provide multiple sizes for HiDPI support.
        match self.presentation {
            TrayPresentation::Idle => vec![apex_icon(16), apex_icon(22), apex_icon(32)],
            TrayPresentation::Recording { paused, .. } => vec![
                recording_circle_icon(16, paused),
                recording_circle_icon(22, paused),
                recording_circle_icon(32, paused),
            ],
        }
    }

    fn title(&self) -> String {
        match &self.presentation {
            TrayPresentation::Idle => "ApexShot".to_string(),
            TrayPresentation::Recording {
                elapsed_text,
                paused,
            } => {
                if *paused {
                    format!("Paused • {elapsed_text}")
                } else {
                    format!("Recording • {elapsed_text}")
                }
            }
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let (title, description) = match &self.presentation {
            TrayPresentation::Idle => (
                "ApexShot".to_string(),
                "Left-click: Capture Area • Right-click: Menu".to_string(),
            ),
            TrayPresentation::Recording {
                elapsed_text,
                paused,
            } => (
                if *paused {
                    format!("Paused • {elapsed_text}")
                } else {
                    format!("Recording • {elapsed_text}")
                },
                if *paused {
                    "Recording paused • Click to stop • Open menu to resume • Timer is shown here on hover".to_string()
                } else {
                    "Recording in progress • Click to stop • Open menu for more actions • Timer is shown here on hover".to_string()
                },
            ),
        };

        ksni::ToolTip {
            icon_name: String::new(),
            icon_pixmap: vec![apex_icon(22)],
            title,
            description,
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        let ltr = |label: &str| format!("\u{200E}{label}");

        if let TrayPresentation::Recording { paused, .. } = self.presentation {
            return vec![
                StandardItem {
                    label: ltr(if paused {
                        "Resume Recording"
                    } else {
                        "Pause Recording"
                    }),
                    activate: Box::new(|tray: &mut Self| {
                        tray.send(TrayAction::ToggleRecordingPause)
                    }),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: ltr("Stop Recording"),
                    activate: Box::new(|tray: &mut Self| tray.send(TrayAction::StopRecordingSave)),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: ltr("Restart Recording"),
                    activate: Box::new(|tray: &mut Self| tray.send(TrayAction::RestartRecording)),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: ltr("Discard Recording"),
                    activate: Box::new(|tray: &mut Self| tray.send(TrayAction::DiscardRecording)),
                    ..Default::default()
                }
                .into(),
            ];
        }

        vec![
            // ── Capture section ──────────────────────────────────────────
            StandardItem {
                label: ltr("Capture Area"),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::CaptureArea)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: ltr("Crosshair Capture"),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::CaptureCrosshair)),
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
                label: ltr("Open Recording UI"),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::OpenRecordingUi)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: ltr("Record Screen"),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::RecordScreen)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: ltr("Video Editor"),
                activate: Box::new(|tray: &mut Self| tray.send(TrayAction::OpenVideoEditor)),
                ..Default::default()
            }
            .into(),
            // ── Separator ────────────────────────────────────────────────
            MenuItem::Separator,
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

#[cfg(test)]
mod tests {
    use super::{ApexShotTray, TrayAction};
    use ksni::Tray;
    use std::sync::mpsc::channel;

    #[test]
    fn tray_defaults_to_idle_presentation() {
        let (tx, _rx) = channel::<TrayAction>();
        let tray = ApexShotTray::new(tx);

        assert_eq!(tray.title(), "ApexShot");
        assert!(tray.menu().len() > 1);
    }

    #[test]
    fn tray_switches_to_recording_timer_mode() {
        let (tx, _rx) = channel::<TrayAction>();
        let mut tray = ApexShotTray::new(tx);
        tray.show_recording_timer("1:23");

        assert_eq!(tray.title(), "Recording • 1:23");
        assert_eq!(tray.menu().len(), 4);
    }
}
