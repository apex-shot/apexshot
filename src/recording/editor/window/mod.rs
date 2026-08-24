#[allow(dead_code)]
mod crop_dialog;
#[allow(dead_code)]
mod dialogs;
#[allow(dead_code)]
mod footer;
#[allow(dead_code)]
mod inspector;
#[allow(dead_code)]
mod media_library;
#[allow(dead_code)]
mod preview;
#[allow(dead_code)]
mod rail;
#[allow(dead_code)]
mod timeline;
mod timeline_card;
mod tool_sidebar;
mod toolbar;

use super::ffmpeg;
use super::model::{AudioMode, VideoEditState, VideoMetadata};
use super::ui_support::install_recording_editor_css;
use gtk4::{
    gdk, gio, glib, prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button,
    DrawingArea, DropTarget, FileChooserAction, FileChooserNative, FileFilter, GestureClick, Image,
    Label, MediaFile, Orientation, Overlay, ResponseType, Revealer, Scale, Spinner,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn open_empty() -> anyhow::Result<()> {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let initial = InitialVideo::None;
    app.connect_activate(move |application| {
        install_recording_editor_icons();
        crate::capture::editor::ui_support::install_editor_css();
        install_recording_editor_css();
        build_window(application, initial.clone());
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn install_recording_editor_icons() {
    use crate::capture::editor::window::icon_names;
    use std::sync::Once;
    static INIT_ICONS: Once = Once::new();
    INIT_ICONS.call_once(|| {
        relm4_icons::initialize_icons(icon_names::GRESOURCE_BYTES, icon_names::RESOURCE_PREFIX);
    });
}

/// Open the editor with an empty window and load the video asynchronously,
/// showing a loading spinner while ffprobe + thumbnail generation run in
/// the background. This avoids a long frozen gap before the window appears
/// for large recordings.
pub fn open_with_path(path: PathBuf) -> anyhow::Result<()> {
    let thumbnail_dir = ffmpeg::thumbnail_cache_dir(&path);

    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let thumbnail_dir_for_cleanup = thumbnail_dir.clone();
    app.connect_shutdown(move |_| {
        let _ = std::fs::remove_dir_all(&thumbnail_dir_for_cleanup);
    });

    let initial = InitialVideo::AsyncLoad(path);
    app.connect_activate(move |application| {
        install_recording_editor_icons();
        crate::capture::editor::ui_support::install_editor_css();
        install_recording_editor_css();
        build_window(application, initial.clone());
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

#[derive(Clone)]
enum InitialVideo {
    None,
    AsyncLoad(PathBuf),
}

fn build_window(application: &Application, initial_video: InitialVideo) {
    let window = ApplicationWindow::builder()
        .application(application)
        .title("ApexShot Recording Editor")
        .icon_name(crate::app_identity::icon_name())
        .default_width(1400)
        .default_height(900)
        .decorated(false)
        .build();
    window.set_size_request(1400, 900);
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

    let state = match &initial_video {
        InitialVideo::AsyncLoad(path) => match ffmpeg::probe_metadata(path) {
            Ok(metadata) => Some(Arc::new(Mutex::new(VideoEditState::new(metadata)))),
            Err(err) => {
                eprintln!(
                    "[recording-editor] failed to probe {}: {err}",
                    path.display()
                );
                None
            }
        },
        InitialVideo::None => None,
    };
    let state = state.unwrap_or_else(|| Arc::new(Mutex::new(placeholder_edit_state())));
    let media = Rc::new(RefCell::new(match &initial_video {
        InitialVideo::AsyncLoad(path) => Some(MediaFile::for_filename(path)),
        InitialVideo::None => Some(MediaFile::new()),
    }));
    let exporting = Rc::new(Cell::new(false));
    let (title_bar, title_label, upload_btn, export_btn) =
        build_window_controls(&window, state.clone(), exporting.clone());
    root.append(&title_bar);

    let paint_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let refresh_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let ping = {
        let paint_slot = paint_slot.clone();
        let refresh_slot = refresh_slot.clone();
        let state = state.clone();
        let title_label = title_label.clone();
        let upload_btn = upload_btn.clone();
        let export_btn = export_btn.clone();
        let exporting = exporting.clone();
        Rc::new(move || {
            let (name, has_video) = {
                let state = state.lock().unwrap();
                (state.title.clone(), state.has_source_video())
            };
            title_label.set_text(&name);
            title_label.set_tooltip_text(Some(&name));
            let enabled = has_video && !exporting.get();
            upload_btn.set_sensitive(enabled);
            export_btn.set_sensitive(enabled);
            if let Some(paint) = paint_slot.borrow().clone() {
                paint();
            }
            if let Some(refresh) = refresh_slot.borrow().clone() {
                refresh();
            }
        }) as Rc<dyn Fn()>
    };

    let workspace = GtkBox::new(Orientation::Horizontal, 8);
    workspace.add_css_class("recording-editor-workspace");
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);

    let stage = GtkBox::new(Orientation::Vertical, 0);
    stage.add_css_class("recording-editor-stage");
    stage.set_hexpand(true);
    stage.set_vexpand(true);
    let estimate_label = Label::new(None);
    let preview_media = media.borrow().clone();
    let (preview_widget, _, _) =
        preview::build_preview_with_media(state.clone(), estimate_label, preview_media);
    stage.append(&preview_widget);
    stage.append(&preview::build_stage_tools(
        &window,
        state.clone(),
        ping.clone(),
    ));
    workspace.append(&stage);

    let sidebar = tool_sidebar::build_tool_sidebar(state.clone(), ping.clone());
    workspace.append(&sidebar.widget);
    root.append(&workspace);

    let (timeline, paint) =
        timeline_card::build_timeline_card(state.clone(), media.clone(), ping.clone());
    root.append(&timeline);
    *paint_slot.borrow_mut() = Some(paint);
    *refresh_slot.borrow_mut() = Some(sidebar.refresh);
    let drop_target = DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
    drop_target.connect_drop({
        let state = state.clone();
        let media = media.clone();
        let window = window.clone();
        let ping = ping.clone();
        move |_, value, _, _| {
            let Ok(file) = value.get::<gio::File>() else {
                return false;
            };
            let Some(path) = file.path() else {
                return false;
            };
            load_preview_video(path, &state, &media, &window, &ping)
        }
    });
    preview_widget.add_controller(drop_target);

    let open_click = GestureClick::new();
    open_click.set_button(1);
    open_click.connect_released({
        let state = state.clone();
        let media = media.clone();
        let window = window.clone();
        let ping = ping.clone();
        move |_, _, _, _| {
            if state.lock().unwrap().metadata.duration_seconds > 0.0 {
                return;
            }
            show_open_preview_video(&window, state.clone(), media.clone(), ping.clone());
        }
    });
    preview_widget.add_controller(open_click);
    ping();

    crate::capture::editor::ui_support::install_edge_resize(&root, &window);
    window.set_child(Some(&root));
    window.present();
}

fn load_preview_video(
    path: PathBuf,
    state: &Arc<Mutex<VideoEditState>>,
    media: &Rc<RefCell<Option<MediaFile>>>,
    window: &ApplicationWindow,
    ping: &Rc<dyn Fn()>,
) -> bool {
    let Ok(metadata) = ffmpeg::probe_metadata(&path) else {
        return false;
    };
    *state.lock().unwrap() = VideoEditState::new(metadata);
    let has_mouse_data = state.lock().unwrap().supports_auto_zoom();
    if let Some(player) = media.borrow().as_ref() {
        player.set_file(Some(&gio::File::for_path(&path)));
    }
    ping();
    if !has_mouse_data {
        dialogs::show_manual_zoom_notice(window);
    }
    true
}

fn show_open_preview_video(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    media: Rc<RefCell<Option<MediaFile>>>,
    ping: Rc<dyn Fn()>,
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
    filter.add_mime_type("video/mp4");
    filter.add_pattern("*.mp4");
    chooser.add_filter(&filter);
    let window = window.clone();
    chooser.connect_response(move |dialog, response| {
        if response == ResponseType::Accept {
            if let Some(path) = dialog.file().and_then(|file| file.path()) {
                load_preview_video(path, &state, &media, &window, &ping);
            }
        }
        dialog.hide();
    });
    chooser.show();
}

fn placeholder_edit_state() -> VideoEditState {
    let mut state = VideoEditState::new(VideoMetadata {
        path: PathBuf::from("Screen Recording.mp4"),
        duration_seconds: 0.0,
        width: 1920,
        height: 1080,
        file_size_bytes: 0,
        has_audio: false,
    });
    state.title = "Drop a recording to begin".into();
    state
}

fn build_window_controls(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    exporting: Rc<Cell<bool>>,
) -> (GtkBox, Label, Button, Button) {
    const TRAFFIC_LIGHTS_WIDTH: i32 = 84;
    let bar = GtkBox::new(Orientation::Horizontal, 8);
    bar.add_css_class("recording-editor-window-controls");
    bar.set_hexpand(true);
    bar.set_vexpand(false);
    bar.set_valign(Align::Start);

    let left_balance = GtkBox::new(Orientation::Horizontal, 0);
    left_balance.set_size_request(TRAFFIC_LIGHTS_WIDTH, -1);
    bar.append(&left_balance);

    let title_text = state.lock().unwrap().title.clone();
    let title = Label::new(Some(&title_text));
    title.add_css_class("recording-editor-title");
    title.set_hexpand(true);
    title.set_halign(Align::Center);
    title.set_valign(Align::Center);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_max_width_chars(64);
    title.set_can_target(false);
    bar.append(&title);

    let (export, export_spinner) =
        footer::build_export_action(window, state.clone(), exporting.clone());
    let (upload, upload_spinner) = footer::build_upload_action(state, exporting);
    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.add_css_class("recording-editor-title-actions");
    actions.set_valign(Align::Center);
    actions.append(&upload_spinner);
    actions.append(&upload);
    actions.append(&export_spinner);
    actions.append(&export);
    let lights = toolbar::build_traffic_lights(window);
    lights.set_size_request(TRAFFIC_LIGHTS_WIDTH, -1);
    lights.set_halign(Align::End);
    lights.set_valign(Align::Center);
    let right_balance = GtkBox::new(Orientation::Horizontal, 16);
    right_balance.set_size_request(tool_sidebar::TOOL_SIDEBAR_WIDTH + TRAFFIC_LIGHTS_WIDTH, -1);
    right_balance.set_hexpand(false);
    let right_spacer = GtkBox::new(Orientation::Horizontal, 0);
    right_spacer.set_hexpand(true);
    right_balance.append(&right_spacer);
    right_balance.append(&actions);
    right_balance.append(&lights);
    bar.append(&right_balance);

    crate::capture::editor::ui_support::install_window_drag(&bar, window);
    (bar, title, upload, export)
}

#[allow(dead_code)]
fn populate_loaded_root(
    root: &GtkBox,
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    thumbnails: Vec<PathBuf>,
    waveform: Option<PathBuf>,
    exporting: Rc<Cell<bool>>,
) {
    // Remove all existing children
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }

    let estimate_label = Label::new(None);
    estimate_label.add_css_class("recording-editor-estimate");
    footer::update_estimate(&estimate_label, &state, false);

    let titlebar = toolbar::build_titlebar(window, Some(state.clone()));
    root.append(&titlebar);

    let workspace = GtkBox::new(Orientation::Horizontal, 0);
    workspace.add_css_class("recording-editor-workspace");
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);

    let chrome = rail::build_tool_chrome(Some(state.clone()), None);
    workspace.append(&chrome.panel);

    let inspector = inspector::build_inspector(window, state.clone(), exporting.clone());
    let (preview_widget, media, play_button) =
        preview::build_preview(state.clone(), estimate_label.clone());
    workspace.append(&preview_widget);
    workspace.append(&inspector);
    root.append(&workspace);

    let timeline_widget = timeline::build_timeline(
        state,
        estimate_label,
        thumbnails,
        waveform,
        media,
        play_button,
    );
    root.append(&timeline_widget);
}

#[allow(dead_code)]
fn populate_empty_root(
    root: &GtkBox,
    window: &ApplicationWindow,
    exporting: Rc<Cell<bool>>,
    loading_revealer: &Revealer,
    loading_spinner: &Spinner,
    open_button_slot: Rc<RefCell<Option<Button>>>,
    loading: Rc<Cell<bool>>,
) {
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }

    let titlebar = toolbar::build_titlebar(window, None);
    root.append(&titlebar);

    let workspace = GtkBox::new(Orientation::Horizontal, 0);
    workspace.add_css_class("recording-editor-workspace");
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);

    let chrome = rail::build_tool_chrome(
        None,
        Some(media_library::EmptyOpenHooks {
            on_click: {
                let root = root.clone();
                let window = window.clone();
                let exporting = exporting.clone();
                let loading_revealer = loading_revealer.clone();
                let loading_spinner = loading_spinner.clone();
                let open_button_slot = open_button_slot.clone();
                let loading = loading.clone();
                Rc::new(move || {
                    show_open_video_dialog(
                        &root,
                        &window,
                        exporting.clone(),
                        &loading_revealer,
                        &loading_spinner,
                        open_button_slot.clone(),
                        loading.clone(),
                    );
                })
            },
            on_drop_video: {
                let root = root.clone();
                let window = window.clone();
                let exporting = exporting.clone();
                let loading_revealer = loading_revealer.clone();
                let loading_spinner = loading_spinner.clone();
                let open_button_slot = open_button_slot.clone();
                let loading = loading.clone();
                Rc::new(move |path: PathBuf| {
                    load_video_async(
                        path,
                        &root,
                        &window,
                        exporting.clone(),
                        &loading_revealer,
                        &loading_spinner,
                        open_button_slot.clone(),
                        loading.clone(),
                    );
                })
            },
            open_button_slot: open_button_slot.clone(),
        }),
    );
    workspace.append(&chrome.panel);

    let empty_preview = build_empty_preview_area();
    workspace.append(&empty_preview);

    let inspector = build_empty_inspector();
    workspace.append(&inspector);
    root.append(&workspace);
    root.append(&build_empty_timeline());
}

#[allow(dead_code)]
fn build_empty_preview_area() -> GtkBox {
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

    frame.append(&workspace);
    frame.append(&preview::build_empty_player_bar());
    frame
}

#[allow(dead_code)]
fn build_empty_inspector() -> GtkBox {
    let inspector = GtkBox::new(Orientation::Vertical, 0);
    inspector.add_css_class("recording-editor-inspector");
    inspector.set_width_request(inspector::INSPECTOR_WIDTH);
    inspector.set_hexpand(false);
    inspector.set_vexpand(true);
    inspector.append(&build_empty_footer());
    inspector
}

#[allow(dead_code)]
fn build_empty_timeline() -> GtkBox {
    let timeline = GtkBox::new(Orientation::Vertical, 0);
    timeline.add_css_class("recording-editor-timeline");
    timeline.set_hexpand(true);
    timeline.set_vexpand(false);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("recording-editor-timeline-shell");
    card.set_hexpand(true);

    let transport = GtkBox::new(Orientation::Horizontal, 8);
    transport.add_css_class("recording-editor-transport");
    let gutter = GtkBox::new(Orientation::Horizontal, 0);
    gutter.add_css_class("recording-editor-track-header");
    gutter.add_css_class("recording-editor-transport-gutter");
    gutter.set_size_request(120, -1);
    gutter.set_hexpand(false);
    let tools = GtkBox::new(Orientation::Horizontal, 8);
    tools.set_hexpand(true);
    tools.set_halign(Align::Start);
    for icon_name in [
        "edit-undo-symbolic",
        "edit-redo-symbolic",
        "edit-delete-symbolic",
        "edit-cut-symbolic",
        "view-sort-ascending-symbolic",
        "zoom-in-symbolic",
    ] {
        tools.append(&disabled_transport_button(icon_name));
    }
    let scale_row = GtkBox::new(Orientation::Horizontal, 6);
    scale_row.add_css_class("recording-editor-timeline-scale-control");
    scale_row.set_hexpand(false);
    let zoom_out = empty_tool_button("zoom-out-symbolic", "Zoom out timeline");
    let zoom_in = empty_tool_button("zoom-in-symbolic", "Zoom in timeline");
    let scale = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 10.0);
    scale.add_css_class("recording-editor-timeline-scale");
    scale.set_draw_value(false);
    scale.set_value(0.0);
    scale.set_size_request(72, 12);
    scale.set_valign(Align::Center);
    scale_row.append(&zoom_out);
    scale_row.append(&scale);
    scale_row.append(&zoom_in);
    transport.append(&gutter);
    transport.append(&tools);
    transport.append(&scale_row);

    let empty_scale = Rc::new(Cell::new(0.0_f64));
    let ruler = DrawingArea::new();
    ruler.add_css_class("recording-editor-ruler");
    ruler.set_hexpand(true);
    ruler.set_size_request(-1, 28);
    ruler.set_draw_func({
        let empty_scale = empty_scale.clone();
        move |_, cr, width, height| draw_empty_ruler(cr, width, height, empty_scale.get())
    });
    scale.connect_value_changed({
        let empty_scale = empty_scale.clone();
        let ruler = ruler.clone();
        move |scale| {
            empty_scale.set(scale.value().clamp(0.0, 100.0));
            ruler.queue_draw();
        }
    });
    zoom_out.connect_clicked({
        let scale = scale.clone();
        move |_| {
            scale.set_value((scale.value() - 10.0).max(0.0));
        }
    });
    zoom_in.connect_clicked({
        let scale = scale.clone();
        move |_| {
            scale.set_value((scale.value() + 10.0).min(100.0));
        }
    });
    let (ruler_row, _) = empty_track_row(None, &ruler);

    let strip = Overlay::new();
    strip.add_css_class("recording-editor-thumbnail-strip");
    strip.add_css_class("recording-editor-empty-thumbnail-strip");
    strip.set_hexpand(true);
    strip.set_size_request(-1, 64);
    let prompt = GtkBox::new(Orientation::Horizontal, 8);
    prompt.set_halign(Align::Start);
    prompt.set_valign(Align::Center);
    prompt.set_margin_start(12);
    prompt.set_can_target(false);
    let prompt_icon = Image::from_icon_name("video-x-generic-symbolic");
    prompt_icon.set_pixel_size(14);
    prompt_icon.add_css_class("recording-editor-empty-track-prompt");
    let prompt_label = Label::new(Some("Select a video, begin your creation."));
    prompt_label.add_css_class("recording-editor-empty-track-prompt");
    prompt.append(&prompt_icon);
    prompt.append(&prompt_label);
    strip.add_overlay(&prompt);
    let (video_row, _) = empty_track_row(Some("camera-video-symbolic"), &strip);
    video_row.add_css_class("recording-editor-empty-track-row");

    let tracks = GtkBox::new(Orientation::Vertical, 0);
    tracks.set_hexpand(true);
    tracks.append(&ruler_row);
    tracks.append(&video_row);
    let tracks_overlay = Overlay::new();
    tracks_overlay.set_hexpand(true);
    tracks_overlay.set_child(Some(&tracks));
    let playhead = DrawingArea::new();
    playhead.set_hexpand(true);
    playhead.set_vexpand(true);
    playhead.set_can_target(false);
    playhead.set_draw_func(|_, cr, width, height| draw_empty_playhead(cr, width, height));
    tracks_overlay.add_overlay(&playhead);

    let board = Overlay::new();
    board.add_css_class("recording-editor-timeline-board");
    board.set_hexpand(true);
    let board_inner = GtkBox::new(Orientation::Vertical, 0);
    board_inner.append(&transport);
    board_inner.append(&tracks_overlay);
    let well = GtkBox::new(Orientation::Vertical, 0);
    well.add_css_class("recording-editor-timeline-well");
    well.set_hexpand(true);
    board_inner.append(&well);
    board.set_child(Some(&board_inner));
    let divider = GtkBox::new(Orientation::Vertical, 0);
    divider.add_css_class("recording-editor-rail-divider");
    divider.set_halign(Align::Start);
    divider.set_valign(Align::Fill);
    divider.set_vexpand(true);
    divider.set_can_target(false);
    divider.set_size_request(1, -1);
    divider.set_margin_start(120);
    board.add_overlay(&divider);
    card.append(&board);

    timeline.append(&card);
    timeline
}

#[allow(dead_code)]
fn empty_track_row(header_icon: Option<&str>, body: &impl IsA<gtk4::Widget>) -> (GtkBox, GtkBox) {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("recording-editor-track-row");
    row.set_hexpand(true);
    let header = GtkBox::new(Orientation::Horizontal, 4);
    header.add_css_class("recording-editor-track-header");
    header.set_size_request(120, -1);
    header.set_hexpand(false);
    if let Some(icon_name) = header_icon {
        let icon = Image::from_icon_name(icon_name);
        icon.add_css_class("recording-editor-track-icon");
        icon.set_pixel_size(14);
        header.append(&icon);
        header.append(&disabled_transport_button("changes-allow-symbolic"));
        header.append(&disabled_transport_button("view-reveal-symbolic"));
    }
    row.append(&header);
    let body_wrap = GtkBox::new(Orientation::Horizontal, 0);
    body_wrap.add_css_class("recording-editor-track-body");
    body_wrap.set_hexpand(true);
    body_wrap.set_size_request(0, -1);
    body_wrap.append(body);
    row.append(&body_wrap);
    (row, body_wrap)
}

#[allow(dead_code)]
fn empty_tool_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-tool-icon");
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(16);
    button.set_child(Some(&icon));
    button.set_valign(Align::Center);
    button.set_tooltip_text(Some(tooltip));
    button
}

#[allow(dead_code)]
fn draw_empty_ruler(cr: &gtk4::cairo::Context, width: i32, height: i32, scale: f64) {
    let w = width as f64;
    let h = height as f64;
    let duration = 10.0;
    let factor = 1.0 + (scale.clamp(0.0, 100.0) / 100.0) * 7.0;
    let span = 1.0 / factor;
    let visible = (duration * span).max(0.001);
    let major: f64 = if visible > 8.0 {
        2.0
    } else if visible > 3.0 {
        1.0
    } else {
        0.5
    };
    let minor = (major / 5.0).max(0.1);
    cr.select_font_face(
        "sans-serif",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(10.0);
    cr.set_line_width(1.0);
    let inner = (w - 16.0).max(1.0);
    let view_end = (visible * (inner + 40.0) / inner.max(1.0)).max(visible);
    let mut t = 0.0;
    while t <= view_end + 0.001 {
        let x = (16.0 + ((t / duration) / span) * inner).floor() + 0.5;
        if x < -20.0 || x > w + 20.0 {
            t += minor;
            continue;
        }
        let is_major = (t / major).fract().abs() < 0.02 || (1.0 - (t / major).fract()).abs() < 0.02;
        cr.set_source_rgba(1.0, 1.0, 1.0, if is_major { 0.28 } else { 0.12 });
        cr.move_to(x, if is_major { h - 8.0 } else { h - 4.0 });
        cr.line_to(x, h);
        let _ = cr.stroke();
        if is_major {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.48);
            let mins = (t as i64) / 60;
            let secs = (t as i64) % 60;
            let label = format!("{mins:02}:{secs:02}");
            if let Ok(ext) = cr.text_extents(&label) {
                cr.move_to(x - ext.width() / 2.0, 11.0);
                let _ = cr.show_text(&label);
            }
        }
        t += minor;
    }
}

#[allow(dead_code)]
fn draw_empty_playhead(cr: &gtk4::cairo::Context, width: i32, height: i32) {
    let _ = width;
    let h = height as f64;
    let x = 144.5;
    let head_w = 10.0;
    let body_h = 7.0;
    let tip_h = 5.0;
    let left = x - head_w / 2.0;
    let right = x + head_w / 2.0;
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.move_to(left, 0.0);
    cr.line_to(right, 0.0);
    cr.line_to(right, body_h);
    cr.line_to(x, body_h + tip_h);
    cr.line_to(left, body_h);
    cr.close_path();
    let _ = cr.fill();
    cr.set_line_width(1.0);
    cr.set_line_cap(gtk4::cairo::LineCap::Butt);
    cr.move_to(x, body_h + tip_h);
    cr.line_to(x, h);
    let _ = cr.stroke();
}

#[allow(dead_code)]
fn disabled_transport_button(icon_name: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-tool-icon");
    button.set_sensitive(false);
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(16);
    button.set_child(Some(&icon));
    button.set_valign(Align::Center);
    button
}

#[allow(dead_code)]
fn build_empty_footer() -> GtkBox {
    let footer = GtkBox::new(Orientation::Horizontal, 6);
    footer.add_css_class("recording-editor-footer");
    footer.add_css_class("recording-editor-inspector-toolbar");
    footer.add_css_class("editor-sidebar-actions");
    footer.set_hexpand(true);

    let copy = disabled_transport_button(
        crate::capture::editor::window::icon_names::custom::COPY_SYMBOLIC,
    );
    copy.add_css_class("recording-editor-inspector-icon");
    let upload = disabled_transport_button(
        crate::capture::editor::window::icon_names::custom::CLOUD_OUTLINE_THIN_SYMBOLIC,
    );
    upload.add_css_class("recording-editor-inspector-icon");
    let done = Button::with_label("Done");
    done.set_has_frame(false);
    done.add_css_class("recording-editor-primary-button");
    done.set_sensitive(false);
    done.set_halign(gtk4::Align::End);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    footer.append(&copy);
    footer.append(&upload);
    footer.append(&spacer);
    footer.append(&done);
    footer
}

#[allow(dead_code)]
fn show_open_video_dialog(
    root: &GtkBox,
    window: &ApplicationWindow,
    exporting: Rc<Cell<bool>>,
    loading_revealer: &Revealer,
    loading_spinner: &Spinner,
    open_button_slot: Rc<RefCell<Option<Button>>>,
    loading: Rc<Cell<bool>>,
) {
    let chooser = FileChooserNative::new(
        Some("Open video"),
        Some(window),
        FileChooserAction::Open,
        Some("Open"),
        Some("Cancel"),
    );

    let filter = FileFilter::new();
    filter.set_name(Some("MP4 files"));
    filter.add_mime_type("video/mp4");
    filter.add_pattern("*.mp4");
    chooser.add_filter(&filter);

    let root_ref = root.clone();
    let window_ref = window.clone();
    let loading_revealer_ref = loading_revealer.clone();
    let loading_spinner_ref = loading_spinner.clone();
    let open_button_slot_ref = open_button_slot.clone();
    let loading_ref = loading.clone();
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
                            open_button_slot_ref.clone(),
                            loading_ref.clone(),
                        );
                    }
                }
            }
        }
        dialog.hide();
    });
    chooser.show();
}

#[allow(dead_code)]
fn load_video_async(
    path: PathBuf,
    root: &GtkBox,
    window: &ApplicationWindow,
    exporting: Rc<Cell<bool>>,
    loading_revealer: &Revealer,
    loading_spinner: &Spinner,
    open_button_slot: Rc<RefCell<Option<Button>>>,
    loading: Rc<Cell<bool>>,
) {
    loading.set(true);
    loading_revealer.set_reveal_child(true);
    loading_spinner.set_visible(true);
    loading_spinner.start();
    if let Some(btn) = open_button_slot.borrow().as_ref() {
        btn.set_sensitive(false);
    }

    let (sender, receiver) =
        mpsc::channel::<Result<(VideoMetadata, Vec<PathBuf>, Option<PathBuf>), String>>();
    std::thread::spawn(move || {
        let result = (|| -> anyhow::Result<_> {
            ffmpeg::ensure_tools_available()?;
            let metadata = ffmpeg::probe_metadata(&path)?;
            let thumbnails = ffmpeg::generate_thumbnails(&metadata)?;
            let waveform = ffmpeg::generate_waveform(&metadata).ok();
            Ok((metadata, thumbnails, waveform))
        })();
        let _ = sender.send(result.map_err(|e| e.to_string()));
    });

    let root = root.clone();
    let window = window.clone();
    let loading_revealer = loading_revealer.clone();
    let loading_spinner = loading_spinner.clone();
    let open_button_slot = open_button_slot.clone();
    let loading = loading.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let stop_loading = || {
            loading.set(false);
            loading_revealer.set_reveal_child(false);
            loading_spinner.stop();
            loading_spinner.set_visible(false);
            if let Some(btn) = open_button_slot.borrow().as_ref() {
                btn.set_sensitive(true);
            }
        };
        match receiver.try_recv() {
            Ok(Ok((metadata, thumbnails, waveform))) => {
                stop_loading();

                let state = Arc::new(Mutex::new(VideoEditState::new(metadata)));
                {
                    let mut state = state.lock().unwrap();
                    state.quality = 70;
                    state.audio_mode = AudioMode::Unchanged;
                }
                populate_loaded_root(
                    &root,
                    &window,
                    state,
                    thumbnails,
                    waveform,
                    exporting.clone(),
                );
                glib::ControlFlow::Break
            }
            Ok(Err(err)) => {
                stop_loading();
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
                stop_loading();
                glib::ControlFlow::Break
            }
        }
    });
}

#[allow(dead_code)]
fn is_supported_video_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("mp4"))
        .unwrap_or(false)
}
