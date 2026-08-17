mod dialogs;
mod footer;
mod inspector;
mod media_library;
mod panels;
mod preview;
mod rail;
mod timeline;
mod toolbar;

use super::ffmpeg;
use super::model::{AudioMode, VideoEditState, VideoMetadata};
use super::ui_support::install_recording_editor_css;
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::{
    prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button, DrawingArea,
    DropTarget, Entry, FileChooserAction, FileChooserNative, FileFilter, Image, Label, MenuButton,
    Orientation, Overlay, ResponseType, Revealer, Scale, Spinner,
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

    let loading = Rc::new(Cell::new(false));
    let open_button_slot = Rc::new(RefCell::new(None::<Button>));

    match initial_video {
        InitialVideo::None => {
            populate_empty_root(
                &root,
                &window,
                exporting.clone(),
                &loading_revealer,
                &loading_spinner,
                open_button_slot.clone(),
                loading.clone(),
            );
        }
        InitialVideo::AsyncLoad(path) => {
            populate_empty_root(
                &root,
                &window,
                exporting.clone(),
                &loading_revealer,
                &loading_spinner,
                open_button_slot.clone(),
                loading.clone(),
            );
            if let Some(btn) = open_button_slot.borrow().as_ref() {
                btn.set_sensitive(false);
            }
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
        }
    }
    crate::capture::editor::ui_support::install_edge_resize(&root, &window);

    // Drag-and-drop target for video files — attach to window so it doesn't eat events from root
    let drop_target = DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
    let drop_revealer_enter = drop_revealer.clone();
    let drop_revealer_leave = drop_revealer.clone();
    let root_ref = root.clone();
    let window_ref = window.clone();
    let exporting_for_drop = exporting.clone();
    let loading_revealer_drop = loading_revealer.clone();
    let loading_spinner_drop = loading_spinner.clone();
    let open_button_slot_drop = open_button_slot.clone();
    let loading_drop = loading.clone();
    let loading_drop_enter = loading.clone();
    drop_target.connect_enter(move |_, _x, _y| {
        if loading_drop_enter.get() {
            return gdk::DragAction::empty();
        }
        drop_revealer_enter.set_reveal_child(true);
        gdk::DragAction::COPY
    });
    drop_target.connect_leave(move |_| {
        drop_revealer_leave.set_reveal_child(false);
    });
    drop_target.connect_drop(move |_, value, _x, _y| {
        drop_revealer.set_reveal_child(false);
        if loading_drop.get() {
            return false;
        }
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
            open_button_slot_drop.clone(),
            loading_drop.clone(),
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

    let chrome = rail::build_tool_chrome(Some(state.clone()), Some(estimate_label.clone()), None);
    workspace.append(&chrome.panel);

    let inspector = inspector::build_inspector(
        window,
        state.clone(),
        estimate_label.clone(),
        exporting.clone(),
    );
    let (preview_widget, media, play_button) = preview::build_preview(
        state.clone(),
        estimate_label.clone(),
        inspector._controls.clone(),
    );
    workspace.append(&preview_widget);
    workspace.append(&inspector.root);
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

fn build_empty_inspector() -> GtkBox {
    let inspector = GtkBox::new(Orientation::Vertical, 0);
    inspector.add_css_class("recording-editor-inspector");
    inspector.set_width_request(inspector::INSPECTOR_WIDTH);
    inspector.set_hexpand(false);
    inspector.set_vexpand(true);

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_vexpand(true);
    let content = GtkBox::new(Orientation::Vertical, 12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(8);
    let estimate = Label::new(Some("~--"));
    estimate.add_css_class("recording-editor-estimate");
    content.append(&estimate);
    content.append(&build_empty_panels());
    scroll.set_child(Some(&content));
    inspector.append(&build_empty_footer());
    inspector.append(&scroll);
    inspector
}

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
    let mut t = 0.0;
    while t <= duration + 0.001 {
        let inner = (w - 16.0).max(1.0);
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

fn build_empty_panels() -> GtkBox {
    let panels = GtkBox::new(Orientation::Vertical, 12);
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

fn is_supported_video_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("mp4"))
        .unwrap_or(false)
}
