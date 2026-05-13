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
    prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button, DropTarget,
    Entry, FileChooserAction, FileChooserNative, FileFilter, Image, Label, MenuButton,
    Orientation, Overlay, Revealer, ResponseType, Scale, Spinner,
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
        build_window(application, Some((state.clone(), thumbnail_paths.clone())));
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

pub fn open_empty() -> anyhow::Result<()> {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |application| {
        crate::capture::editor::ui_support::install_editor_css();
        install_recording_editor_css();
        build_window(application, None);
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn build_window(
    application: &Application,
    initial_video: Option<(Arc<Mutex<VideoEditState>>, Vec<PathBuf>)>,
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
    if prefers_dark {
        overlay.add_css_class("editor-theme-dark");
    } else {
        overlay.add_css_class("editor-theme-light");
    }
    if crate::capture::editor::ui_support::prefers_reduced_transparency() {
        overlay.add_css_class("editor-reduced-transparency");
    }
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

    match initial_video {
        Some((state, thumbnails)) => {
            populate_loaded_root(&root, &window, state, thumbnails, exporting.clone());
        }
        None => {
            populate_empty_root(
                &root,
                &window,
                exporting.clone(),
                &loading_revealer,
                &loading_spinner,
            );
        }
    }
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
        if !is_supported_video_path(&path) {
            return false;
        }
        load_video_async(
            path.to_path_buf(),
            &root_ref,
            &window_ref,
            exporting_for_drop.clone(),
            &loading_revealer_drop,
            &loading_spinner_drop,
        );
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

fn populate_loaded_root(
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

fn populate_empty_root(
    root: &GtkBox,
    window: &ApplicationWindow,
    exporting: Rc<Cell<bool>>,
    loading_revealer: &Revealer,
    loading_spinner: &Spinner,
) {
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }

    let top_controls = toolbar::build_toolbar(window, "Video Editor");
    root.append(&top_controls);

    let empty_preview = build_empty_preview_area(
        root,
        window,
        exporting.clone(),
        loading_revealer,
        loading_spinner,
    );
    root.append(&empty_preview);

    let empty_bottom_tools = build_empty_bottom_tools();
    root.append(&empty_bottom_tools);

    crate::capture::editor::ui_support::install_window_drag(&top_controls, window);
}

fn build_empty_preview_area(
    root: &GtkBox,
    window: &ApplicationWindow,
    exporting: Rc<Cell<bool>>,
    loading_revealer: &Revealer,
    loading_spinner: &Spinner,
) -> GtkBox {
    let frame = GtkBox::new(Orientation::Vertical, 0);
    frame.add_css_class("recording-editor-preview-frame");
    frame.set_hexpand(true);
    frame.set_vexpand(true);
    frame.set_halign(Align::Fill);
    frame.set_valign(Align::Fill);

    let workspace = GtkBox::new(Orientation::Vertical, 0);
    workspace.add_css_class("recording-editor-preview-workspace");
    workspace.add_css_class("recording-editor-empty-workspace");
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);
    workspace.set_halign(Align::Fill);
    workspace.set_valign(Align::Fill);

    let center = GtkBox::new(Orientation::Vertical, 14);
    center.add_css_class("recording-editor-empty-dropzone");
    center.set_halign(Align::Center);
    center.set_valign(Align::Center);

    let icon = Image::from_icon_name("video-x-generic-symbolic");
    icon.add_css_class("recording-editor-empty-icon");
    icon.set_pixel_size(42);

    let title = Label::new(Some("Drop a video here"));
    title.add_css_class("recording-editor-empty-title");

    let hint = Label::new(Some("MP4, WebM, MKV, MOV, AVI, FLV, WMV, MPG, or MPEG"));
    hint.add_css_class("recording-editor-empty-hint");
    hint.set_wrap(true);
    hint.set_justify(gtk4::Justification::Center);

    let open_button = Button::with_label("Open Folder");
    open_button.set_has_frame(false);
    open_button.add_css_class("recording-editor-primary-button");
    open_button.add_css_class("recording-editor-empty-open-button");

    let root_for_open = root.clone();
    let window_for_open = window.clone();
    let loading_revealer_for_open = loading_revealer.clone();
    let loading_spinner_for_open = loading_spinner.clone();
    open_button.connect_clicked(move |_| {
        show_open_video_dialog(
            &root_for_open,
            &window_for_open,
            exporting.clone(),
            &loading_revealer_for_open,
            &loading_spinner_for_open,
        );
    });

    center.append(&icon);
    center.append(&title);
    center.append(&hint);
    center.append(&open_button);

    let top_spacer = GtkBox::new(Orientation::Vertical, 0);
    top_spacer.set_vexpand(true);
    let bottom_spacer = GtkBox::new(Orientation::Vertical, 0);
    bottom_spacer.set_vexpand(true);

    workspace.append(&top_spacer);
    workspace.append(&center);
    workspace.append(&bottom_spacer);
    frame.append(&workspace);
    frame
}

fn build_empty_bottom_tools() -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("recording-editor-bottom-tools");

    root.append(&build_empty_timeline());
    root.append(&build_empty_panels());
    root.append(&build_empty_footer());
    root
}

fn build_empty_timeline() -> GtkBox {
    let timeline = GtkBox::new(Orientation::Vertical, 0);
    timeline.add_css_class("recording-editor-timeline");
    timeline.set_hexpand(true);
    timeline.set_vexpand(false);
    timeline.set_size_request(-1, 64);

    let card = GtkBox::new(Orientation::Horizontal, 0);
    card.add_css_class("recording-editor-timeline-card");
    card.set_hexpand(true);
    card.set_vexpand(false);

    let play_button = Button::new();
    play_button.add_css_class("recording-editor-play-button");
    play_button.set_sensitive(false);
    let play_icon = Image::from_icon_name("media-playback-start-symbolic");
    play_icon.set_pixel_size(22);
    play_button.set_child(Some(&play_icon));
    play_button.set_valign(Align::Center);
    card.append(&play_button);

    let timeline_vbox = GtkBox::new(Orientation::Vertical, 4);
    timeline_vbox.set_hexpand(true);
    timeline_vbox.set_vexpand(false);

    let strip = GtkBox::new(Orientation::Horizontal, 0);
    strip.add_css_class("recording-editor-thumbnail-strip");
    strip.add_css_class("recording-editor-empty-thumbnail-strip");
    strip.set_hexpand(true);
    strip.set_vexpand(false);
    strip.set_halign(Align::Fill);
    strip.set_valign(Align::Center);
    strip.set_size_request(-1, 48);

    let time_row = GtkBox::new(Orientation::Horizontal, 0);
    time_row.set_hexpand(true);
    let start = Label::new(Some("Start 0:00.0"));
    start.add_css_class("recording-editor-time-label");
    start.set_xalign(0.0);
    let end = Label::new(Some("End 0:00.0"));
    end.add_css_class("recording-editor-time-label");
    end.set_xalign(1.0);
    end.set_hexpand(true);
    time_row.append(&start);
    time_row.append(&end);

    timeline_vbox.append(&strip);
    timeline_vbox.append(&time_row);
    card.append(&timeline_vbox);

    let tools = GtkBox::new(Orientation::Horizontal, 6);
    tools.add_css_class("recording-editor-timeline-tools");
    for icon_name in [
        "edit-cut-symbolic",
        "view-sort-ascending-symbolic",
        "edit-undo-symbolic",
    ] {
        let button = Button::new();
        button.add_css_class("recording-editor-cut-button");
        button.set_sensitive(false);
        let icon = Image::from_icon_name(icon_name);
        icon.set_pixel_size(18);
        button.set_child(Some(&icon));
        button.set_valign(Align::Center);
        tools.append(&button);
    }
    card.append(&tools);

    timeline.append(&card);
    timeline
}

fn build_empty_panels() -> GtkBox {
    let panels = GtkBox::new(Orientation::Horizontal, 12);
    panels.add_css_class("recording-editor-panels");
    panels.set_hexpand(true);

    let dimensions = GtkBox::new(Orientation::Vertical, 0);
    dimensions.add_css_class("recording-editor-panel");
    dimensions.set_hexpand(true);

    let dimensions_title = Label::new(Some("Dimensions"));
    dimensions_title.add_css_class("recording-editor-panel-title");
    dimensions_title.set_xalign(0.0);
    dimensions.append(&dimensions_title);

    let dimensions_body = GtkBox::new(Orientation::Vertical, 8);
    dimensions_body.add_css_class("recording-editor-panel-body");

    let dimension_button = MenuButton::new();
    dimension_button.set_has_frame(false);
    dimension_button.add_css_class("recording-editor-dropdown");
    dimension_button.set_hexpand(true);
    dimension_button.set_label("No video selected");
    dimension_button.set_sensitive(false);
    dimensions_body.append(&dimension_button);

    let width_entry = Entry::new();
    width_entry.add_css_class("recording-editor-size-entry");
    width_entry.set_text("");
    width_entry.set_sensitive(false);
    let height_entry = Entry::new();
    height_entry.add_css_class("recording-editor-size-entry");
    height_entry.set_text("");
    height_entry.set_sensitive(false);
    dimensions_body.append(&empty_field_row("Width", &width_entry));
    dimensions_body.append(&empty_field_row("Height", &height_entry));
    dimensions.append(&dimensions_body);

    let settings = GtkBox::new(Orientation::Vertical, 0);
    settings.add_css_class("recording-editor-panel");
    settings.set_hexpand(true);

    let quality_label = Label::new(Some("Quality"));
    quality_label.add_css_class("recording-editor-panel-title");
    quality_label.set_xalign(0.0);
    settings.append(&quality_label);

    let quality_body = GtkBox::new(Orientation::Vertical, 8);
    quality_body.add_css_class("recording-editor-panel-body");
    let quality_row = GtkBox::new(Orientation::Horizontal, 8);
    let low = Label::new(Some("Low"));
    low.add_css_class("recording-editor-label");
    let high = Label::new(Some("High"));
    high.add_css_class("recording-editor-label");
    let quality_scale = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    quality_scale.add_css_class("recording-editor-quality-slider");
    quality_scale.set_value(70.0);
    quality_scale.set_hexpand(true);
    quality_scale.set_draw_value(false);
    quality_scale.set_sensitive(false);
    quality_row.append(&low);
    quality_row.append(&quality_scale);
    quality_row.append(&high);
    quality_body.append(&quality_row);

    let audio_label = Label::new(Some("Audio"));
    audio_label.add_css_class("recording-editor-panel-title");
    audio_label.set_xalign(0.0);
    settings.append(&audio_label);

    let audio_body = GtkBox::new(Orientation::Vertical, 4);
    audio_body.add_css_class("recording-editor-panel-body");
    let audio_unchanged = gtk4::CheckButton::with_label("Don't change");
    let audio_mono = gtk4::CheckButton::with_label("Convert to mono");
    let audio_muted = gtk4::CheckButton::with_label("Mute");
    audio_unchanged.set_active(true);
    for button in [&audio_unchanged, &audio_mono, &audio_muted] {
        button.add_css_class("recording-editor-audio-choice");
        button.set_sensitive(false);
        audio_body.append(button);
    }
    settings.append(&quality_body);
    settings.append(&audio_body);

    panels.append(&dimensions);
    panels.append(&settings);
    panels
}

fn empty_field_row(label: &str, entry: &Entry) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 10);
    let label = Label::new(Some(label));
    label.add_css_class("recording-editor-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);
    row.append(entry);
    row
}

fn build_empty_footer() -> GtkBox {
    let footer = GtkBox::new(Orientation::Horizontal, 10);
    footer.add_css_class("recording-editor-footer");
    footer.set_hexpand(true);

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);

    let estimate = Label::new(Some("Estimated file size: --"));
    estimate.add_css_class("recording-editor-estimate");

    let trim_only = Button::with_label("Save Trim");
    trim_only.set_has_frame(false);
    trim_only.add_css_class("recording-editor-secondary-button");
    trim_only.set_sensitive(false);

    let convert = Button::with_label("Save & Convert");
    convert.set_has_frame(false);
    convert.add_css_class("recording-editor-primary-button");
    convert.set_sensitive(false);

    footer.append(&spacer);
    footer.append(&estimate);
    footer.append(&trim_only);
    footer.append(&convert);
    footer
}

fn show_open_video_dialog(
    root: &GtkBox,
    window: &ApplicationWindow,
    exporting: Rc<Cell<bool>>,
    loading_revealer: &Revealer,
    loading_spinner: &Spinner,
) {
    let chooser = FileChooserNative::new(
        Some("Open video"),
        Some(window),
        FileChooserAction::Open,
        Some("Open"),
        Some("Cancel"),
    );

    let filter = FileFilter::new();
    filter.set_name(Some("Videos"));
    for mime_type in [
        "video/mp4",
        "video/webm",
        "video/x-matroska",
        "video/quicktime",
        "video/x-msvideo",
        "video/x-flv",
        "video/x-ms-wmv",
        "video/mpeg",
    ] {
        filter.add_mime_type(mime_type);
    }
    for pattern in [
        "*.mp4", "*.webm", "*.mkv", "*.avi", "*.mov", "*.flv", "*.wmv", "*.mpg", "*.mpeg",
    ] {
        filter.add_pattern(pattern);
    }
    chooser.add_filter(&filter);

    let root_ref = root.clone();
    let window_ref = window.clone();
    let loading_revealer_ref = loading_revealer.clone();
    let loading_spinner_ref = loading_spinner.clone();
    chooser.connect_response(move |dialog, response| {
        if response == ResponseType::Accept {
            if let Some(file) = dialog.file() {
                if let Some(path) = file.path() {
                    if is_supported_video_path(&path) {
                        load_video_async(
                            path,
                            &root_ref,
                            &window_ref,
                            exporting.clone(),
                            &loading_revealer_ref,
                            &loading_spinner_ref,
                        );
                    }
                }
            }
        }
        dialog.hide();
    });
    chooser.show();
}

fn load_video_async(
    path: PathBuf,
    root: &GtkBox,
    window: &ApplicationWindow,
    exporting: Rc<Cell<bool>>,
    loading_revealer: &Revealer,
    loading_spinner: &Spinner,
) {
    loading_revealer.set_reveal_child(true);
    loading_spinner.set_visible(true);
    loading_spinner.start();

    let (sender, receiver) = mpsc::channel::<Result<(VideoMetadata, Vec<PathBuf>), String>>();
    std::thread::spawn(move || {
        let result = (|| -> anyhow::Result<_> {
            ffmpeg::ensure_tools_available()?;
            let metadata = ffmpeg::probe_metadata(&path)?;
            let thumbnails = ffmpeg::generate_thumbnails(&metadata)?;
            Ok((metadata, thumbnails))
        })();
        let _ = sender.send(result.map_err(|e| e.to_string()));
    });

    let root = root.clone();
    let window = window.clone();
    let loading_revealer = loading_revealer.clone();
    let loading_spinner = loading_spinner.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || match receiver.try_recv() {
        Ok(Ok((metadata, thumbnails))) => {
            loading_revealer.set_reveal_child(false);
            loading_spinner.stop();
            loading_spinner.set_visible(false);

            let state = Arc::new(Mutex::new(VideoEditState::new(metadata)));
            {
                let mut state = state.lock().unwrap();
                state.quality = 70;
                state.audio_mode = AudioMode::Unchanged;
            }
            populate_loaded_root(&root, &window, state, thumbnails, exporting.clone());
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
    });
}

fn is_supported_video_path(path: &std::path::Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    [
        "mp4", "webm", "mkv", "avi", "mov", "flv", "wmv", "mpg", "mpeg",
    ]
    .contains(&ext.as_str())
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
