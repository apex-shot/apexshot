use std::sync::mpsc::Sender;

#[derive(Debug, Clone)]
pub enum TrayAction {
    CaptureArea,
    CaptureCrosshair,
    CaptureScreen,
    CaptureWindow,
    OpenRecordingUi,
    OpenVideoEditor,
    OpenImageEditor,
    RecordScreen,
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
}

impl ApexShotTray {
    pub fn new(tx: Sender<TrayAction>) -> Self {
        Self {
            tx,
            recording: false,
        }
    }

    fn send(&self, action: TrayAction) {
        let _ = self.tx.send(action);
    }

    pub fn set_recording(&mut self, recording: bool) {
        self.recording = recording;
    }
}

fn paint_icon_pixel(data: &mut [u8], size: usize, x: i32, y: i32, coverage: f64) {
    if x < 0 || y < 0 || x >= size as i32 || y >= size as i32 {
        return;
    }
    let index = (y as usize * size + x as usize) * 4;
    let alpha = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
    if alpha <= data[index] {
        return;
    }
    // StatusNotifier pixmaps use network-order ARGB bytes.
    data[index..index + 4].copy_from_slice(&[alpha, 233, 84, 32]);
}

fn paint_icon_disc(data: &mut [u8], size: usize, cx: f64, cy: f64, radius: f64) {
    let min_x = (cx - radius - 1.0).floor() as i32;
    let max_x = (cx + radius + 1.0).ceil() as i32;
    let min_y = (cy - radius - 1.0).floor() as i32;
    let max_y = (cy + radius + 1.0).ceil() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            let coverage = (radius + 0.75 - dx.hypot(dy)).clamp(0.0, 1.0);
            paint_icon_pixel(data, size, x, y, coverage);
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

fn icon(size: i32) -> ksni::Icon {
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
            paint_icon_disc(&mut data, size, x * scale, y * scale, radius);
        }
    }
    ksni::Icon {
        width: size as i32,
        height: size as i32,
        data,
    }
}

fn tray_icons() -> &'static Vec<ksni::Icon> {
    static ICONS: std::sync::OnceLock<Vec<ksni::Icon>> = std::sync::OnceLock::new();
    ICONS.get_or_init(|| vec![icon(16), icon(22), icon(32)])
}

impl ksni::Tray for ApexShotTray {
    fn activate(&mut self, _x: i32, _y: i32) {
        if !self.recording {
            self.send(TrayAction::CaptureArea);
        }
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::SystemServices
    }

    fn id(&self) -> String {
        status_notifier_id()
    }
    fn icon_name(&self) -> String {
        crate::app_identity::icon_name().to_string()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        tray_icons().clone()
    }
    fn title(&self) -> String {
        "ApexShot".to_string()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            icon_name: crate::app_identity::icon_name().to_string(),
            icon_pixmap: vec![tray_icons()[1].clone()],
            title: "ApexShot".to_string(),
            description: "ApexShot".to_string(),
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{MenuItem, StandardItem};
        let idle = !self.recording;
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
        vec![
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
                &crate::i18n::t("Open Recording UI"),
                idle,
                TrayAction::OpenRecordingUi
            ),
            item!(
                &crate::i18n::t("Record Screen"),
                idle,
                TrayAction::RecordScreen
            ),
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
        ]
    }
}

pub fn status_notifier_id() -> String {
    format!("{}.Tray", crate::app_identity::app_id())
}

pub fn spawn_tray(tx: Sender<TrayAction>) -> anyhow::Result<ksni::Handle<ApexShotTray>> {
    let service = ksni::TrayService::new(ApexShotTray::new(tx));
    let handle = service.handle();
    service.spawn();
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
        for icon in tray_icons() {
            assert!(icon.width > 0 && icon.height > 0);
            assert_eq!(
                icon.data.len(),
                icon.width as usize * icon.height as usize * 4
            );
            assert!(icon.data.chunks_exact(4).any(|pixel| pixel[0] > 0));
        }
    }
}
