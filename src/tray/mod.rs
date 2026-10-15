use std::sync::mpsc::Sender;

#[derive(Debug, Clone)]
pub enum TrayAction {
    OpenUpdate(String),
    QuickCapture,
    CaptureArea,
    CaptureCrosshair,
    CaptureScreen,
    CaptureWindow,
    OpenVideoEditor,
    OpenImageEditor,
    RecordScreen,
    RecordWindow,
    StopRecordingSave,
    ShowLastPreview,
    OpenLastCapture,
    OpenHistory,
    OpenSettings,
    Quit,
}

pub struct ApexShotTray {
    tx: Sender<TrayAction>,
    recording: bool,
    recording_supported: bool,
    update: Option<crate::update::UpdateInfo>,
}

impl ApexShotTray {
    pub fn new(tx: Sender<TrayAction>) -> Self {
        Self {
            tx,
            recording: false,
            recording_supported: crate::recording::recording_supported(),
            update: None,
        }
    }

    fn send(&self, action: TrayAction) {
        let _ = self.tx.send(action);
    }

    pub fn set_recording(&mut self, recording: bool) {
        self.recording = recording;
    }

    pub fn set_update_available(&mut self, update: crate::update::UpdateInfo) {
        self.update = Some(update);
    }
}

/// The arch's stroke colour, #E95420.
const ARCH_ORANGE: [u8; 3] = [233, 84, 32];
/// The recording accent the record button and the preview overlay already use,
/// #e81123.
const RECORDING_RED: [u8; 3] = [232, 17, 35];
/// Recording badge geometry in the 24-unit icon space: the dot sits in the
/// arch's lower-left opening, where it never meets the stroke, so it needs no
/// cleared ring and stays legible down to 16px.
const BADGE_CENTRE: (f64, f64) = (9.0, 18.0);
const BADGE_RADIUS: f64 = 2.6;

fn paint_icon_pixel(data: &mut [u8], size: usize, x: i32, y: i32, coverage: f64, colour: [u8; 3]) {
    if x < 0 || y < 0 || x >= size as i32 || y >= size as i32 {
        return;
    }
    let index = (y as usize * size + x as usize) * 4;
    let alpha = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
    if alpha <= data[index] {
        return;
    }
    // StatusNotifier pixmaps use network-order ARGB bytes.
    data[index..index + 4].copy_from_slice(&[alpha, colour[0], colour[1], colour[2]]);
}

fn paint_icon_disc(data: &mut [u8], size: usize, cx: f64, cy: f64, radius: f64, colour: [u8; 3]) {
    let min_x = (cx - radius - 1.0).floor() as i32;
    let max_x = (cx + radius + 1.0).ceil() as i32;
    let min_y = (cy - radius - 1.0).floor() as i32;
    let max_y = (cy + radius + 1.0).ceil() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            let coverage = (radius + 0.75 - dx.hypot(dy)).clamp(0.0, 1.0);
            paint_icon_pixel(data, size, x, y, coverage, colour);
        }
    }
}

fn cubic_point(
    t: f64,
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
) -> (f64, f64) {
    let mt = 1.0 - t;
    let a = mt * mt * mt;
    let b = 3.0 * mt * mt * t;
    let c = 3.0 * mt * t * t;
    let d = t * t * t;
    (
        a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
        a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
    )
}

fn icon(size: i32, recording: bool) -> ksni::Icon {
    let size = size.max(1) as usize;
    let scale = size as f64 / 24.0;
    let mut data = vec![0; size * size * 4];
    let radius = (1.25 * scale).max(0.8);
    let segments = (size * 4).max(32);
    let curves = [
        ((2.0, 21.0), (6.0, 21.0), (8.0, 2.0), (12.0, 2.0)),
        ((12.0, 2.0), (16.0, 2.0), (18.0, 21.0), (22.0, 21.0)),
    ];
    for (p0, p1, p2, p3) in curves {
        for step in 0..=segments {
            let (x, y) = cubic_point(step as f64 / segments as f64, p0, p1, p2, p3);
            paint_icon_disc(&mut data, size, x * scale, y * scale, radius, ARCH_ORANGE);
        }
    }
    if recording {
        let (cx, cy) = (BADGE_CENTRE.0 * scale, BADGE_CENTRE.1 * scale);
        paint_icon_disc(&mut data, size, cx, cy, BADGE_RADIUS * scale, RECORDING_RED);
    }
    ksni::Icon {
        width: size as i32,
        height: size as i32,
        data,
    }
}

/// The pixmaps a host draws for the item. A host that reads `IconName` takes
/// the badged themed icon instead, so the two paths never both apply.
fn tray_icons(recording: bool) -> &'static Vec<ksni::Icon> {
    static IDLE: std::sync::OnceLock<Vec<ksni::Icon>> = std::sync::OnceLock::new();
    static RECORDING: std::sync::OnceLock<Vec<ksni::Icon>> = std::sync::OnceLock::new();
    if recording {
        RECORDING.get_or_init(|| vec![icon(16, true), icon(22, true), icon(32, true)])
    } else {
        IDLE.get_or_init(|| vec![icon(16, false), icon(22, false), icon(32, false)])
    }
}

impl ksni::Tray for ApexShotTray {
    fn activate(&mut self, _x: i32, _y: i32) {
        if !self.recording {
            self.send(TrayAction::QuickCapture);
        }
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::SystemServices
    }

    fn id(&self) -> String {
        status_notifier_id()
    }
    /// The badged icon while a recording runs. Hosts that read the pixmap
    /// instead get the same dot painted into it, so either way the item shows
    /// the dot in the bottom-left corner and never two of them.
    fn icon_name(&self) -> String {
        if self.recording {
            crate::app_identity::recording_icon_name()
        } else {
            crate::app_identity::icon_name().to_string()
        }
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        tray_icons(self.recording).clone()
    }
    fn title(&self) -> String {
        "ApexShot".to_string()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            icon_name: self.icon_name(),
            icon_pixmap: vec![tray_icons(self.recording)[1].clone()],
            title: "ApexShot".to_string(),
            description: "ApexShot".to_string(),
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{Disposition, MenuItem, StandardItem};
        let idle = !self.recording;
        // Recording is refused outright on Fedora, so its entries stay visible
        // but disabled rather than accepting a click that cannot succeed.
        let record_enabled = idle && self.recording_supported;
        macro_rules! item {
            ($label:expr, $enabled:expr, $action:expr) => {
                StandardItem {
                    label: $label.to_string(),
                    enabled: $enabled,
                    activate: Box::new(|tray: &mut Self| tray.send($action)),
                    ..Default::default()
                }
                .into()
            };
        }
        let mut menu = Vec::new();
        if let Some(update) = &self.update {
            let label = crate::i18n::tfmt(
                "Update available · v{version}",
                &[("version", &update.version)],
            );
            let release_url = update.release_url.clone();
            menu.push(
                StandardItem {
                    label,
                    activate: Box::new(move |tray: &mut Self| {
                        tray.send(TrayAction::OpenUpdate(release_url.clone()))
                    }),
                    ..Default::default()
                }
                .into(),
            );
            menu.push(MenuItem::Separator);
        }

        menu.extend([
            item!(
                &crate::i18n::t("Quick Capture"),
                idle,
                TrayAction::QuickCapture
            ),
            item!(
                &crate::i18n::t("Capture Area"),
                idle,
                TrayAction::CaptureArea
            ),
            item!(
                &crate::i18n::t("Crosshair Capture"),
                idle,
                TrayAction::CaptureCrosshair
            ),
            item!(
                &crate::i18n::t("Capture Screen"),
                idle,
                TrayAction::CaptureScreen
            ),
            MenuItem::Separator,
            item!(
                &crate::i18n::t("Record Screen"),
                record_enabled,
                TrayAction::RecordScreen
            ),
            item!(
                &crate::i18n::t("Record Window"),
                record_enabled,
                TrayAction::RecordWindow
            ),
        ]);
        if !self.recording_supported {
            // Fedora: recording stays visible but disabled, and the menu states
            // the reason instead of refusing only after the user clicks.
            menu.push(
                StandardItem {
                    label: crate::i18n::t("Recording not supported"),
                    enabled: false,
                    disposition: Disposition::Informative,
                    ..Default::default()
                }
                .into(),
            );
        }
        menu.extend([
            item!(
                &crate::i18n::t("Stop Recording"),
                self.recording,
                TrayAction::StopRecordingSave
            ),
            item!(
                &crate::i18n::t("Video Editor"),
                idle,
                TrayAction::OpenVideoEditor
            ),
            item!(
                &crate::i18n::t("Image Editor"),
                idle,
                TrayAction::OpenImageEditor
            ),
            MenuItem::Separator,
            item!(
                &crate::i18n::t("Open Last Capture"),
                idle,
                TrayAction::OpenLastCapture
            ),
            item!(&crate::i18n::t("History"), idle, TrayAction::OpenHistory),
            item!(&crate::i18n::t("Settings"), idle, TrayAction::OpenSettings),
            MenuItem::Separator,
            item!(&crate::i18n::t("Quit"), true, TrayAction::Quit),
        ]);
        menu
    }
}

pub fn status_notifier_id() -> String {
    format!("{}.Tray", crate::app_identity::app_id())
}

pub fn spawn_tray(tx: Sender<TrayAction>) -> anyhow::Result<ksni::Handle<ApexShotTray>> {
    let service = ksni::TrayService::new(ApexShotTray::new(tx));
    let handle = service.handle();
    if crate::app_identity::portal_only() {
        service.spawn_without_dbus_name();
    } else {
        service.spawn();
    }
    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_id_is_not_the_application_window_id() {
        let id = status_notifier_id();
        assert_ne!(id, crate::app_identity::app_id());
        assert!(id.ends_with(".Tray"));
    }

    #[test]
    fn software_tray_icons_are_valid_argb_pixmaps() {
        for icon in tray_icons(false).iter().chain(tray_icons(true)) {
            assert!(icon.width > 0 && icon.height > 0);
            assert_eq!(
                icon.data.len(),
                icon.width as usize * icon.height as usize * 4
            );
            assert!(icon.data.chunks_exact(4).any(|pixel| pixel[0] > 0));
        }
    }

    #[test]
    fn the_recording_dot_is_red_and_sits_in_the_bottom_left() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut tray = ApexShotTray::new(tx);

        let idle = ksni::Tray::icon_pixmap(&tray);
        assert!(recording_red_bounds(&idle[0]).is_none());

        tray.set_recording(true);
        for icon in &ksni::Tray::icon_pixmap(&tray) {
            let (left, top, right, _) = recording_red_bounds(icon).expect("the dot is painted");
            assert!(right < icon.width as usize / 2, "the dot sits on the left");
            assert!(
                top >= icon.height as usize / 2,
                "the dot sits at the bottom"
            );
            assert!(left > 0, "the dot keeps a margin from the edge");
        }

        tray.set_recording(false);
        let idle = ksni::Tray::icon_pixmap(&tray);
        assert!(recording_red_bounds(&idle[0]).is_none());
    }

    /// Bounding box of the fully opaque recording-red pixels, if any.
    fn recording_red_bounds(icon: &ksni::Icon) -> Option<(usize, usize, usize, usize)> {
        let width = icon.width as usize;
        let mut bounds: Option<(usize, usize, usize, usize)> = None;
        for (index, pixel) in icon.data.chunks_exact(4).enumerate() {
            if pixel[0] == 255 && pixel[1..4] == RECORDING_RED {
                let (x, y) = (index % width, index / width);
                bounds = Some(match bounds {
                    Some((left, top, right, bottom)) => {
                        (left.min(x), top.min(y), right.max(x), bottom.max(y))
                    }
                    None => (x, y, x, y),
                });
            }
        }
        bounds
    }

    #[test]
    fn the_tray_swaps_to_the_badged_icon_while_recording() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut tray = ApexShotTray::new(tx);

        assert_eq!(
            ksni::Tray::icon_name(&tray),
            crate::app_identity::icon_name()
        );

        tray.set_recording(true);
        assert_eq!(
            ksni::Tray::icon_name(&tray),
            crate::app_identity::recording_icon_name()
        );

        tray.set_recording(false);
        assert_eq!(
            ksni::Tray::icon_name(&tray),
            crate::app_identity::icon_name()
        );
    }

    #[test]
    fn the_badged_icon_ships_in_the_packaging_assets() {
        // include_str! fails the build if the asset goes missing, so the icon
        // every package installs cannot drift away from this name.
        let svg = include_str!("../../packaging/apexshot-recording.svg");
        assert!(svg.contains("#e81123"));
    }
}
