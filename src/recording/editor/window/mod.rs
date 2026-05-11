mod dialogs;
mod footer;
mod panels;
mod preview;
mod timeline;
mod toolbar;

use super::ui_support::install_recording_editor_css;
use super::ffmpeg;
use super::model::{AudioMode, DimensionPreset, VideoEditState, VideoMetadata};
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::{
    prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, DropTarget, Label,
    Orientation, Overlay, Revealer,
};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

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

    populate_root(&root, &window, state.clone(), thumbnails);
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
        let Ok(new_metadata) = ffmpeg::probe_metadata(&path) else {
            return false;
        };
        let new_thumbnails = ffmpeg::generate_thumbnails(&new_metadata).unwrap_or_default();
        // Update shared state
        {
            let mut s = state_ref.lock().unwrap();
            s.metadata = new_metadata;
            s.trim_start_seconds = 0.0;
            s.trim_end_seconds = s.metadata.duration_seconds;
            s.playhead_seconds = 0.0;
            s.dimension_preset = DimensionPreset::Original;
            s.custom_width = s.metadata.width;
            s.custom_height = s.metadata.height;
            s.quality = 70;
            s.audio_mode = AudioMode::Unchanged;
        }
        // Clear and rebuild root contents
        populate_root(&root_ref, &window_ref, state_ref.clone(), new_thumbnails);
        true
    });
    window.add_controller(drop_target);

    let exporting = Rc::new(Cell::new(false));
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
) {
    // Remove all existing children
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }

    let exporting = Rc::new(Cell::new(false));
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
