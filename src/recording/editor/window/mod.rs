mod dialogs;
mod footer;
mod panels;
mod preview;
mod timeline;
mod toolbar;

use super::ffmpeg;
use super::model::{AudioMode, VideoEditState, VideoMetadata};
use super::ui_support::install_recording_editor_css;
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::{
    prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, DropTarget, Label,
    Orientation, Overlay, Revealer, Spinner,
};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn open(metadata: VideoMetadata) -> anyhow::Result<()> {
    let state = Arc::new(Mutex::new(VideoEditState::new(metadata)));
    let thumbnail_paths = {
        let state_guard = state.lock().unwrap();
        ffmpeg::generate_thumbnails(&state_guard.metadata).unwrap_or_default()
    };
    let thumbnail_dir = {
        let state_guard = state.lock().unwrap();
        ffmpeg::thumbnail_cache_dir(&state_guard.metadata.path)
    };

    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let thumbnail_dir_for_cleanup = thumbnail_dir.clone();
    app.connect_shutdown(move |_| {
        let _ = std::fs::remove_dir_all(&thumbnail_dir_for_cleanup);
    });

    app.connect_activate(move |application| {
        crate::capture::editor::ui_support::install_editor_css();
        install_recording_editor_css();
        build_window(application, state.clone(), thumbnail_paths.clone());
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn build_window(
    application: &Application,
    state: Arc<Mutex<VideoEditState>>,
    thumbnails: Vec<PathBuf>,
) {
    let window = ApplicationWindow::builder()
        .application(application)
        .title("ApexShot Recording Editor")
        .icon_name(crate::app_identity::icon_name())
        .default_width(1040)
        .default_height(860)
        .decorated(false)
        .build();
    window.add_css_class("editor-window");

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    root.add_css_class("recording-editor-root");
    let prefers_dark = crate::capture::editor::ui_support::prefers_dark_glass_theme();
    if prefers_dark {
        root.add_css_class("editor-theme-dark");
    } else {
        root.add_css_class("editor-theme-light");
    }
    if crate::capture::editor::ui_support::prefers_reduced_transparency() {
        root.add_css_class("editor-reduced-transparency");
    }

    let overlay = Overlay::new();
    overlay.set_child(Some(&root));

    let exporting = Rc::new(Cell::new(false));

    // Drop feedback banner
    let drop_banner = GtkBox::new(Orientation::Vertical, 0);
    drop_banner.add_css_class("recording-editor-drop-banner");
    drop_banner.set_can_target(false);
    let drop_label = Label::new(Some("Drop video file to open"));
    drop_label.add_css_class("recording-editor-drop-label");
    drop_label.set_can_target(false);
    drop_banner.append(&drop_label);
    let drop_revealer = Revealer::new();
    drop_revealer.set_can_target(false);
    drop_revealer.set_halign(Align::Center);
    drop_revealer.set_valign(Align::Start);
    drop_revealer.set_child(Some(&drop_banner));
    drop_revealer.set_transition_type(gtk4::RevealerTransitionType::SlideDown);
    drop_revealer.set_reveal_child(false);
    overlay.add_overlay(&drop_revealer);

    // Loading banner for async drop handling
    let loading_box = GtkBox::new(Orientation::Horizontal, 8);
    loading_box.add_css_class("recording-editor-drop-banner");
    loading_box.set_can_target(false);
    loading_box.set_halign(Align::Center);
    let loading_spinner = Spinner::new();
    loading_spinner.set_size_request(16, 16);
    loading_spinner.set_can_target(false);
    let loading_label = Label::new(Some("Loading video…"));
    loading_label.add_css_class("recording-editor-drop-label");
    loading_label.set_can_target(false);
    loading_box.append(&loading_spinner);
    loading_box.append(&loading_label);
    let loading_revealer = Revealer::new();
    loading_revealer.set_can_target(false);
    loading_revealer.set_halign(Align::Center);
    loading_revealer.set_valign(Align::Start);
    loading_revealer.set_child(Some(&loading_box));
    loading_revealer.set_transition_type(gtk4::RevealerTransitionType::SlideDown);
    loading_revealer.set_reveal_child(false);
    overlay.add_overlay(&loading_revealer);

    populate_root(&root, &window, state.clone(), thumbnails, exporting.clone());
    crate::capture::editor::ui_support::install_edge_resize(&root, &window);

    // Drag-and-drop target for video files — attach to window so it doesn't eat events from root
    let drop_target = DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
    let drop_revealer_enter = drop_revealer.clone();
    let drop_revealer_leave = drop_revealer.clone();
    drop_target.connect_enter(move |_, _x, _y| {
        drop_revealer_enter.set_reveal_child(true);
        gdk::DragAction::COPY
    });
    drop_target.connect_leave(move |_| {
        drop_revealer_leave.set_reveal_child(false);
    });
    let root_ref = root.clone();
    let window_ref = window.clone();
    let state_ref = state.clone();
    let exporting_for_drop = exporting.clone();
    let loading_revealer_drop = loading_revealer.clone();
    let loading_spinner_drop = loading_spinner.clone();
    drop_target.connect_drop(move |_, value, _x, _y| {
        drop_revealer.set_reveal_child(false);
        let Ok(file) = value.get::<gio::File>() else {
            return false;
        };
        let Some(path) = file.path() else {
            return false;
        };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ![
            "mp4", "webm", "mkv", "avi", "mov", "flv", "wmv", "mpg", "mpeg",
        ]
        .contains(&ext.as_str())
        {
            return false;
        }

        // Show loading state
        loading_revealer_drop.set_reveal_child(true);
        loading_spinner_drop.start();

        let path = path.to_path_buf();
        let (sender, receiver) = mpsc::channel::<Result<(VideoMetadata, Vec<PathBuf>), String>>();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<_> {
                let metadata = ffmpeg::probe_metadata(&path)?;
                let thumbnails = ffmpeg::generate_thumbnails(&metadata)?;
                Ok((metadata, thumbnails))
            })();
            let _ = sender.send(result.map_err(|e| e.to_string()));
        });

        let root = root_ref.clone();
        let window = window_ref.clone();
        let state = state_ref.clone();
        let exporting = exporting_for_drop.clone();
        let loading_revealer = loading_revealer_drop.clone();
        let loading_spinner = loading_spinner_drop.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            match receiver.try_recv() {
                Ok(Ok((metadata, thumbnails))) => {
                    loading_revealer.set_reveal_child(false);
                    loading_spinner.stop();
                    loading_spinner.set_visible(false);
                    {
                        let mut s = state.lock().unwrap();
                        *s = VideoEditState::new(metadata);
                        s.quality = 70;
                        s.audio_mode = AudioMode::Unchanged;
                    }
                    populate_root(&root, &window, state.clone(), thumbnails, exporting.clone());
                    glib::ControlFlow::Break
                }
                Ok(Err(err)) => {
                    loading_revealer.set_reveal_child(false);
                    loading_spinner.stop();
                    loading_spinner.set_visible(false);
                    dialogs::show_error(
                        &window,
                        "Failed to open video",
                        "ApexShot could not open this video file.",
                        Some(&err),
                    );
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    loading_revealer.set_reveal_child(false);
                    loading_spinner.stop();
                    loading_spinner.set_visible(false);
                    glib::ControlFlow::Break
                }
            }
        });
        true
    });
    window.add_controller(drop_target);

    let exporting_for_close = exporting.clone();
    window.connect_close_request(move |_| {
        if exporting_for_close.get() {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });

    window.set_child(Some(&overlay));
    window.present();
}

fn populate_root(
    root: &GtkBox,
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    thumbnails: Vec<PathBuf>,
    exporting: Rc<Cell<bool>>,
) {
    // Remove all existing children
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }

    let estimate_label = Label::new(None);
    estimate_label.add_css_class("recording-editor-estimate");
    footer::update_estimate(&estimate_label, &state, false);

    let file_stem = {
        let state = state.lock().unwrap();
        state
            .metadata
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Recording")
            .to_string()
    };

    let top_controls = toolbar::build_toolbar(window, &file_stem);
    root.append(&top_controls);

    let preview_widget = preview::build_preview(state.clone(), estimate_label.clone(), thumbnails);
    root.append(&preview_widget);

    let bottom_tools = build_bottom_tools(window, state.clone(), estimate_label, exporting.clone());
    root.append(&bottom_tools);

    crate::capture::editor::ui_support::install_window_drag(&top_controls, window);
}

fn build_bottom_tools(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
    exporting: Rc<Cell<bool>>,
) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("recording-editor-bottom-tools");

    let (panels_widget, controls) = panels::build_panels(state.clone(), estimate_label.clone());
    root.append(&panels_widget);

    let footer_widget = footer::build_footer(
        window,
        state.clone(),
        estimate_label,
        controls,
        exporting.clone(),
    );
    root.append(&footer_widget);
    root
}
