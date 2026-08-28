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

fn icon(size: i32) -> ksni::Icon {
    use gtk4::cairo::{Context, Format, ImageSurface};
    let mut surface = ImageSurface::create(Format::ARgb32, size, size).expect("tray surface");
    let context = Context::new(&surface).expect("tray context");
    let scale = size as f64 / 24.0;
    context.scale(scale, scale);
    context.set_source_rgba(0.913, 0.329, 0.125, 1.0);
    context.set_line_width(2.5);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.move_to(2.0, 21.0);
    context.curve_to(6.0, 21.0, 8.0, 2.0, 12.0, 2.0);
    context.curve_to(16.0, 2.0, 18.0, 21.0, 22.0, 21.0);
    context.stroke().expect("draw tray icon");
    drop(context);
    surface.flush();

    let stride = surface.stride() as usize;
    let mut data = vec![0; size as usize * size as usize * 4];
    let pixels = surface.data().expect("tray pixels");
    for y in 0..size as usize {
        for x in 0..size as usize {
            let source = y * stride + x * 4;
            let target = (y * size as usize + x) * 4;
            data[target..target + 4].copy_from_slice(&pixels[source..source + 4]);
            data.swap(target, target + 3);
            data.swap(target + 1, target + 2);
        }
    }
    ksni::Icon {
        width: size,
        height: size,
        data,
    }
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
        String::new()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![icon(16), icon(22), icon(32)]
    }
    fn title(&self) -> String {
        "ApexShot".to_string()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            icon_name: String::new(),
            icon_pixmap: vec![icon(22)],
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
            item!("Capture Area", idle, TrayAction::CaptureArea),
            item!("Crosshair Capture", idle, TrayAction::CaptureCrosshair),
            item!("Capture Screen", idle, TrayAction::CaptureScreen),
            MenuItem::Separator,
            item!("Open Recording UI", idle, TrayAction::OpenRecordingUi),
            item!("Record Screen", idle, TrayAction::RecordScreen),
            item!(
                "Stop Recording",
                self.recording,
                TrayAction::StopRecordingSave
            ),
            item!("Video Editor", idle, TrayAction::OpenVideoEditor),
            item!("Image Editor", idle, TrayAction::OpenImageEditor),
            MenuItem::Separator,
            item!("Open Last Capture", idle, TrayAction::OpenLastCapture),
            item!("History", idle, TrayAction::OpenHistory),
            item!("Settings", idle, TrayAction::OpenSettings),
            MenuItem::Separator,
            item!("Quit", true, TrayAction::Quit),
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
}
