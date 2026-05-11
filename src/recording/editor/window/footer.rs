use super::dialogs;
use super::panels::EditorControls;
use crate::recording::editor::ffmpeg;
use crate::recording::editor::model::{format_size, VideoEditState};
use gtk4::{
    glib, prelude::*, ApplicationWindow, Box as GtkBox, Button, Label, Orientation, Spinner,
};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(super) fn build_footer(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
    controls: EditorControls,
    exporting: Rc<Cell<bool>>,
) -> GtkBox {
    let footer = GtkBox::new(Orientation::Horizontal, 10);
    footer.add_css_class("recording-editor-footer");
    footer.set_hexpand(true);

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);

    let trim_only = Button::with_label("Save Trim");
    trim_only.set_has_frame(false);
    trim_only.add_css_class("recording-editor-secondary-button");
    let convert = Button::with_label("Save & Convert");
    convert.set_has_frame(false);
    convert.add_css_class("recording-editor-primary-button");
    let spinner = Spinner::new();
    spinner.set_visible(false);

    let export_controls = vec![
        trim_only.clone().upcast::<gtk4::Widget>(),
        convert.clone().upcast::<gtk4::Widget>(),
        controls.dimension_button.clone().upcast::<gtk4::Widget>(),
        controls.width_entry.clone().upcast::<gtk4::Widget>(),
        controls.height_entry.clone().upcast::<gtk4::Widget>(),
        controls.quality_scale.clone().upcast::<gtk4::Widget>(),
        controls.audio_unchanged.clone().upcast::<gtk4::Widget>(),
        controls.audio_mono.clone().upcast::<gtk4::Widget>(),
        controls.audio_muted.clone().upcast::<gtk4::Widget>(),
    ];

    wire_export_button(
        &trim_only,
        window,
        state.clone(),
        false,
        export_controls.clone(),
        spinner.clone(),
        exporting.clone(),
    );
    wire_export_button(
        &convert,
        window,
        state,
        true,
        export_controls,
        spinner.clone(),
        exporting,
    );

    footer.append(&spacer);
    footer.append(&estimate_label);
    footer.append(&spinner);
    footer.append(&trim_only);
    footer.append(&convert);
    footer
}

pub(super) fn update_estimate(label: &Label, state: &Arc<Mutex<VideoEditState>>, trim_only: bool) {
    let state = state.lock().unwrap();
    label.set_text(&format!(
        "Estimated file size: ~{}",
        format_size(state.estimated_size_bytes(trim_only))
    ));
}

fn wire_export_button(
    button: &Button,
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    convert: bool,
    controls: Vec<gtk4::Widget>,
    spinner: Spinner,
    exporting: Rc<Cell<bool>>,
) {
    let window = window.clone();
    button.connect_clicked(move |_| {
        if exporting.get() {
            return;
        }
        exporting.set(true);
        spinner.set_visible(true);
        spinner.start();
        for control in &controls {
            control.set_sensitive(false);
        }

        let state_snapshot = state.lock().unwrap().clone();
        let (sender, receiver) = std::sync::mpsc::channel::<Result<PathBuf, String>>();
        std::thread::spawn(move || {
            let result = if convert {
                ffmpeg::run_convert(&state_snapshot)
            } else {
                ffmpeg::run_trim_only(&state_snapshot)
            };
            let _ = sender.send(result.map_err(|err| err.to_string()));
        });

        let controls = controls.clone();
        let spinner = spinner.clone();
        let exporting = exporting.clone();
        let window = window.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || match receiver.try_recv() {
            Ok(result) => {
                exporting.set(false);
                spinner.stop();
                spinner.set_visible(false);
                for control in &controls {
                    control.set_sensitive(true);
                }
                match result {
                    Ok(path) => dialogs::show_success(&window, path),
                    Err(err) if !convert => dialogs::show_error(
                        &window,
                        "Trim failed",
                        "ApexShot could not trim this recording without conversion. Try Trim & Convert.",
                        Some(&err),
                    ),
                    Err(err) => dialogs::show_error(
                        &window,
                        "Export failed",
                        "ApexShot could not export this recording.",
                        Some(&err),
                    ),
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                exporting.set(false);
                spinner.stop();
                spinner.set_visible(false);
                for control in &controls {
                    control.set_sensitive(true);
                }
                dialogs::show_error(
                    &window,
                    "Export failed",
                    "ApexShot lost contact with the export worker.",
                    None,
                );
                glib::ControlFlow::Break
            }
        });
    });
}
