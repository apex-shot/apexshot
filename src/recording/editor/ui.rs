use super::css::install_recording_editor_css;
use super::ffmpeg;
use super::model::{format_size, AudioMode, DimensionPreset, VideoEditState, VideoMetadata};
use gtk4::gdk;
use gtk4::glib;
use gtk4::{
    prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button, CenterBox,
    DrawingArea, Entry, EventControllerMotion, GestureDrag, Image, Label, MediaFile, Orientation,
    Overlay, Picture, Popover, Scale, Spinner,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
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
    root.add_css_class("editor-theme-dark");
    if crate::capture::editor::ui_support::prefers_reduced_transparency() {
        root.add_css_class("editor-reduced-transparency");
    }

    let exporting = Rc::new(Cell::new(false));
    let estimate_label = Label::new(None);
    estimate_label.add_css_class("recording-editor-estimate");
    update_estimate(&estimate_label, &state, false);

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

    let top_controls = build_top_controls(&window, &file_stem);
    root.append(&top_controls);

    let preview = build_video_preview(state.clone(), estimate_label.clone(), thumbnails.clone());
    root.append(&preview);

    let bottom_tools =
        build_bottom_tools(&window, state.clone(), estimate_label, exporting.clone());
    root.append(&bottom_tools);

    crate::capture::editor::ui_support::install_window_drag(&top_controls, &window);
    crate::capture::editor::ui_support::install_edge_resize(&root, &window);

    let exporting_for_close = exporting.clone();
    window.connect_close_request(move |_| {
        if exporting_for_close.get() {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });

    window.set_child(Some(&root));
    window.present();
}

fn build_top_controls(window: &ApplicationWindow, file_stem: &str) -> CenterBox {
    let controls = CenterBox::new();
    controls.add_css_class("editor-toolbar");
    controls.add_css_class("recording-editor-window-controls");

    let close =
        crate::capture::editor::ui_support::traffic_light_button("traffic-light-red", "Close");
    let minimize = crate::capture::editor::ui_support::traffic_light_button(
        "traffic-light-yellow",
        "Minimize",
    );
    let zoom =
        crate::capture::editor::ui_support::traffic_light_button("traffic-light-green", "Zoom");

    let traffic_lights = GtkBox::new(Orientation::Horizontal, 6);
    traffic_lights.add_css_class("editor-traffic-lights");
    traffic_lights.append(&close);
    traffic_lights.append(&minimize);
    traffic_lights.append(&zoom);

    let left = GtkBox::new(Orientation::Horizontal, 16);
    left.add_css_class("editor-toolbar-left");
    left.append(&traffic_lights);
    controls.set_start_widget(Some(&left));

    let title = Label::new(Some(file_stem));
    title.add_css_class("recording-editor-title");
    controls.set_center_widget(Some(&title));

    let window_close = window.clone();
    close.connect_clicked(move |_| window_close.close());

    let window_minimize = window.clone();
    minimize.connect_clicked(move |_| window_minimize.minimize());

    let window_zoom = window.clone();
    zoom.connect_clicked(move |_| {
        if window_zoom.is_maximized() {
            window_zoom.unmaximize();
        } else {
            window_zoom.maximize();
        }
    });

    controls
}

fn build_video_preview(
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
    thumbnails: Vec<PathBuf>,
) -> GtkBox {
    let path = {
        let state = state.lock().unwrap();
        state.metadata.path.clone()
    };

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("recording-editor-preview-frame");
    root.set_hexpand(true);
    root.set_vexpand(true);

    let workspace = GtkBox::new(Orientation::Vertical, 0);
    workspace.add_css_class("recording-editor-preview-workspace");
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);
    workspace.set_halign(Align::Fill);
    workspace.set_valign(Align::Fill);

    let media = MediaFile::for_filename(path);
    media.set_loop(true);

    let picture = Picture::for_paintable(&media);
    picture.add_css_class("recording-editor-video");
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_halign(Align::Center);
    picture.set_valign(Align::Center);
    picture.set_keep_aspect_ratio(true);
    picture.set_can_shrink(true);

    workspace.append(&picture);
    root.append(&workspace);

    let timeline = build_timeline(state, estimate_label, thumbnails, media.clone());
    root.append(&timeline);

    root
}

fn build_bottom_tools(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
    exporting: Rc<Cell<bool>>,
) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-footer");
    root.add_css_class("recording-editor-bottom-tools");

    let (panels, controls) = build_panels(state.clone(), estimate_label.clone());
    root.append(&panels);

    let footer = build_footer(
        window,
        state.clone(),
        estimate_label,
        controls,
        exporting.clone(),
    );
    root.append(&footer);
    root
}

#[derive(Clone)]
struct EditorControls {
    dimension_button: Button,
    dimension_popover: Popover,
    width_entry: Entry,
    height_entry: Entry,
    quality_scale: Scale,
    audio_unchanged: gtk4::CheckButton,
    audio_mono: gtk4::CheckButton,
    audio_muted: gtk4::CheckButton,
}

fn build_timeline(
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
    thumbnails: Vec<PathBuf>,
    media: MediaFile,
) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("recording-editor-timeline");
    root.set_hexpand(true);

    let card = GtkBox::new(Orientation::Horizontal, 0);
    card.add_css_class("recording-editor-timeline-card");
    card.set_hexpand(true);

    let play_button = Button::new();
    play_button.add_css_class("recording-editor-play-button");
    let play_icon = Image::from_icon_name("media-playback-start-symbolic");
    play_icon.set_pixel_size(22);
    play_button.set_child(Some(&play_icon));
    play_button.set_valign(Align::Center);

    let media_play = media.clone();
    let play_button_ref = play_button.clone();
    let playing = Rc::new(Cell::new(false));
    play_button.connect_clicked(move |_| {
        let is_playing = playing.get();
        if is_playing {
            media_play.pause();
            playing.set(false);
            let icon = Image::from_icon_name("media-playback-start-symbolic");
            icon.set_pixel_size(22);
            play_button_ref.set_child(Some(&icon));
        } else {
            media_play.play();
            playing.set(true);
            let icon = Image::from_icon_name("media-playback-pause-symbolic");
            icon.set_pixel_size(22);
            play_button_ref.set_child(Some(&icon));
        }
    });

    card.append(&play_button);

    let timeline_vbox = GtkBox::new(Orientation::Vertical, 4);
    timeline_vbox.set_hexpand(true);

    let overlay = Overlay::new();
    overlay.add_css_class("recording-editor-trim-area");
    overlay.set_hexpand(true);

    let strip = GtkBox::new(Orientation::Horizontal, 0);
    strip.add_css_class("recording-editor-thumbnail-strip");
    strip.set_hexpand(true);
    strip.set_halign(Align::Fill);
    strip.set_valign(Align::Center);
    if thumbnails.is_empty() {
        for _ in 0..12 {
            let placeholder = GtkBox::new(Orientation::Vertical, 0);
            placeholder.add_css_class("recording-editor-thumbnail");
            placeholder.set_hexpand(true);
            strip.append(&placeholder);
        }
    } else {
        for path in thumbnails {
            let picture = Picture::for_filename(path);
            picture.add_css_class("recording-editor-thumbnail");
            picture.set_hexpand(true);
            strip.append(&picture);
        }
    }
    overlay.set_child(Some(&strip));

    let selection = DrawingArea::new();
    selection.set_hexpand(true);
    selection.set_vexpand(true);
    selection.set_draw_func({
        let state = state.clone();
        move |_, cr, width, height| {
            let state = state.lock().unwrap();
            let duration = state.metadata.duration_seconds.max(0.001);
            let start_x = (state.trim_start_seconds / duration) * width as f64;
            let end_x = (state.trim_end_seconds / duration) * width as f64;
            let range_width = (end_x - start_x).max(1.0);
            let h = height as f64;
            let r = 4.0; // corner radius
            let handle_w = 10.0;

            // Dimmed area outside trim (left)
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.55);
            cr.rectangle(0.0, 0.0, start_x, h);
            let _ = cr.fill();
            // Dimmed area outside trim (right)
            cr.rectangle(end_x, 0.0, width as f64 - end_x, h);
            let _ = cr.fill();

            // Trim selection fill with rounded corners
            cr.set_source_rgba(1.0, 0.84, 0.0, 0.18);
            let _ = cr.new_sub_path();
            cr.arc(
                start_x + r,
                r,
                r,
                std::f64::consts::PI,
                1.5 * std::f64::consts::PI,
            );
            cr.arc(
                start_x + range_width - r,
                r,
                r,
                -0.5 * std::f64::consts::PI,
                0.0,
            );
            cr.arc(
                start_x + range_width - r,
                h - r,
                r,
                0.0,
                0.5 * std::f64::consts::PI,
            );
            cr.arc(
                start_x + r,
                h - r,
                r,
                0.5 * std::f64::consts::PI,
                std::f64::consts::PI,
            );
            cr.close_path();
            let _ = cr.fill();

            // Trim selection border (rounded)
            cr.set_source_rgba(1.0, 0.83, 0.0, 0.85);
            cr.set_line_width(1.5);
            let _ = cr.new_sub_path();
            cr.arc(
                start_x + r,
                r,
                r,
                std::f64::consts::PI,
                1.5 * std::f64::consts::PI,
            );
            cr.arc(
                start_x + range_width - r,
                r,
                r,
                -0.5 * std::f64::consts::PI,
                0.0,
            );
            cr.arc(
                start_x + range_width - r,
                h - r,
                r,
                0.0,
                0.5 * std::f64::consts::PI,
            );
            cr.arc(
                start_x + r,
                h - r,
                r,
                0.5 * std::f64::consts::PI,
                std::f64::consts::PI,
            );
            cr.close_path();
            let _ = cr.stroke();

            // Left handle grip (vertical dashes)
            cr.set_source_rgba(1.0, 0.83, 0.0, 0.9);
            cr.set_line_width(1.5);
            let grip_y_start = h * 0.25;
            let grip_y_end = h * 0.75;
            let grip_spacing = 4.0;
            let mut y = grip_y_start;
            while y + 2.0 <= grip_y_end {
                cr.move_to(start_x + handle_w / 2.0 - 2.0, y);
                cr.line_to(start_x + handle_w / 2.0 - 2.0, y + 2.0);
                cr.move_to(start_x + handle_w / 2.0 + 2.0, y);
                cr.line_to(start_x + handle_w / 2.0 + 2.0, y + 2.0);
                y += grip_spacing;
            }
            let _ = cr.stroke();

            // Right handle grip (vertical dashes)
            y = grip_y_start;
            while y + 2.0 <= grip_y_end {
                cr.move_to(end_x - handle_w / 2.0 - 2.0, y);
                cr.line_to(end_x - handle_w / 2.0 - 2.0, y + 2.0);
                cr.move_to(end_x - handle_w / 2.0 + 2.0, y);
                cr.line_to(end_x - handle_w / 2.0 + 2.0, y + 2.0);
                y += grip_spacing;
            }
            let _ = cr.stroke();

            // Playhead line
            let playhead_x = (state.playhead_seconds / duration) * width as f64;
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.92);
            cr.set_line_width(1.5);
            cr.move_to(playhead_x, 0.0);
            cr.line_to(playhead_x, h);
            let _ = cr.stroke();

            // Playhead top triangle
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.92);
            cr.move_to(playhead_x - 4.0, 0.0);
            cr.line_to(playhead_x + 4.0, 0.0);
            cr.line_to(playhead_x, 6.0);
            cr.close_path();
            let _ = cr.fill();
        }
    });
    overlay.add_overlay(&selection);

    let drag_kind = Rc::new(RefCell::new(None::<TrimDragKind>));
    let drag = GestureDrag::new();
    drag.connect_drag_begin({
        let state = state.clone();
        let drag_kind = drag_kind.clone();
        let media = media.clone();
        let selection = selection.clone();
        move |gesture, x, _| {
            let width = gesture
                .widget()
                .and_then(|widget| widget.downcast::<DrawingArea>().ok())
                .map(|area| area.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut state = state.lock().unwrap();
            let duration = state.metadata.duration_seconds.max(0.001);
            let start_x = (state.trim_start_seconds / duration) * width;
            let end_x = (state.trim_end_seconds / duration) * width;
            let playhead_x = (state.playhead_seconds / duration) * width;
            let handle_threshold = 12.0;
            let kind = if (x - start_x).abs() <= handle_threshold
                && (x - start_x).abs() <= (x - end_x).abs()
            {
                TrimDragKind::Start
            } else if (x - end_x).abs() <= handle_threshold {
                TrimDragKind::End
            } else {
                let seconds = (x.clamp(0.0, width) / width) * duration;
                state.playhead_seconds = seconds;
                media.seek((seconds * 1_000_000.0) as i64);
                selection.queue_draw();
                let _ = playhead_x;
                TrimDragKind::Playhead
            };
            *drag_kind.borrow_mut() = Some(kind);
        }
    });
    let start_label = Label::new(None);
    start_label.add_css_class("recording-editor-time-label");
    start_label.set_xalign(0.0);
    start_label.set_hexpand(true);
    let end_label = Label::new(None);
    end_label.add_css_class("recording-editor-time-label");
    end_label.set_xalign(1.0);
    end_label.set_hexpand(true);
    update_time_labels(&start_label, &end_label, &state);

    drag.connect_drag_update({
        let state = state.clone();
        let drag_kind = drag_kind.clone();
        let selection = selection.clone();
        let estimate_label = estimate_label.clone();
        let start_label = start_label.clone();
        let end_label = end_label.clone();
        let media = media.clone();
        move |gesture, offset_x, _| {
            let Some(kind) = *drag_kind.borrow() else {
                return;
            };
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .and_then(|widget| widget.downcast::<DrawingArea>().ok())
                .map(|area| area.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let value_x = (start_x + offset_x).clamp(0.0, width);
            let mut state_guard = state.lock().unwrap();
            let duration = state_guard.metadata.duration_seconds.max(0.001);
            let seconds = (value_x / width) * duration;
            match kind {
                TrimDragKind::Start => state_guard.set_trim_start(seconds),
                TrimDragKind::End => state_guard.set_trim_end(seconds),
                TrimDragKind::Playhead => {
                    state_guard.playhead_seconds = seconds;
                    media.seek((seconds * 1_000_000.0) as i64);
                }
            }
            drop(state_guard);
            selection.queue_draw();
            if !matches!(kind, TrimDragKind::Playhead) {
                update_estimate(&estimate_label, &state, false);
                update_time_labels(&start_label, &end_label, &state);
            }
        }
    });
    drag.connect_drag_end({
        let drag_kind = drag_kind.clone();
        move |_, _, _| {
            *drag_kind.borrow_mut() = None;
        }
    });
    selection.add_controller(drag);

    let motion = EventControllerMotion::new();
    motion.connect_motion({
        let state = state.clone();
        move |controller, x, _| {
            let Some(widget) = controller.widget() else {
                return;
            };
            let width = widget.allocated_width().max(1) as f64;
            let state = state.lock().unwrap();
            let duration = state.metadata.duration_seconds.max(0.001);
            let start_x = (state.trim_start_seconds / duration) * width;
            let end_x = (state.trim_end_seconds / duration) * width;
            let playhead_x = (state.playhead_seconds / duration) * width;
            let handle_threshold = 12.0;
            let playhead_threshold = 8.0;
            let cursor_name = if (x - start_x).abs() <= handle_threshold {
                Some("w-resize")
            } else if (x - end_x).abs() <= handle_threshold {
                Some("e-resize")
            } else if (x - playhead_x).abs() <= playhead_threshold {
                Some("pointer")
            } else {
                None
            };
            let cursor = cursor_name.and_then(|name| gdk::Cursor::from_name(name, None));
            widget.set_cursor(cursor.as_ref());
        }
    });
    motion.connect_leave(|controller| {
        if let Some(widget) = controller.widget() {
            widget.set_cursor(None);
        }
    });
    selection.add_controller(motion);

    // Periodically sync playhead position from media and redraw
    let media_playhead = media.clone();
    let selection_playhead = selection.clone();
    let state_playhead = state.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        if media_playhead.is_playing() {
            let ts_us = media_playhead.timestamp();
            if ts_us > 0 {
                let seconds = ts_us as f64 / 1_000_000.0;
                let mut s = state_playhead.lock().unwrap();
                s.playhead_seconds = seconds;
                drop(s);
                selection_playhead.queue_draw();
            }
        }
        glib::ControlFlow::Continue
    });

    timeline_vbox.append(&overlay);
    let time_row = GtkBox::new(Orientation::Horizontal, 0);
    time_row.append(&start_label);
    time_row.append(&end_label);
    timeline_vbox.append(&time_row);

    card.append(&timeline_vbox);
    root.append(&card);
    root
}

#[derive(Clone, Copy)]
enum TrimDragKind {
    Start,
    End,
    Playhead,
}

fn build_panels(
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
) -> (GtkBox, EditorControls) {
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

    let original_label = {
        let state = state.lock().unwrap();
        format!(
            "{} x {} (Original)",
            state.metadata.width, state.metadata.height
        )
    };
    let dimension_options: Vec<String> = std::iter::once(original_label.clone())
        .chain(
            ["1920 x 1080", "1280 x 720", "854 x 480", "Custom"]
                .into_iter()
                .map(|s| s.to_string()),
        )
        .collect();

    let dimension_button = Button::new();
    dimension_button.set_has_frame(false);
    dimension_button.add_css_class("recording-editor-dropdown");
    dimension_button.set_hexpand(true);

    let dimension_button_box = GtkBox::new(Orientation::Horizontal, 8);
    dimension_button_box.set_hexpand(true);
    let dimension_label = Label::new(Some(&original_label));
    dimension_label.set_xalign(0.0);
    dimension_label.set_hexpand(true);
    dimension_label.add_css_class("recording-editor-dropdown-label");
    let dimension_arrow = Label::new(Some("\u{25BE}"));
    dimension_arrow.add_css_class("recording-editor-dropdown-arrow");
    dimension_button_box.append(&dimension_label);
    dimension_button_box.append(&dimension_arrow);
    dimension_button.set_child(Some(&dimension_button_box));

    let dimension_popover = Popover::new();
    dimension_popover.set_has_arrow(false);
    dimension_popover.set_autohide(true);
    dimension_popover.add_css_class("editor-popover");
    dimension_popover.add_css_class("recording-editor-dropdown-popover");
    dimension_popover.set_parent(&dimension_button);
    let dimension_list = GtkBox::new(Orientation::Vertical, 0);
    dimension_list.add_css_class("editor-popover-list");
    dimension_list.add_css_class("recording-editor-dropdown-list");
    dimension_popover.set_child(Some(&dimension_list));

    let dimension_popover_open = dimension_popover.clone();
    let dimension_button_ref = dimension_button.clone();
    let dimension_list_ref = dimension_list.clone();
    dimension_button.connect_clicked(move |_| {
        let btn_width = dimension_button_ref.allocated_width();
        dimension_list_ref.set_size_request(btn_width, -1);
        dimension_popover_open.popup();
    });

    dimensions_body.append(&dimension_button);

    let width_entry = Entry::new();
    let height_entry = Entry::new();
    width_entry.add_css_class("editor-crop-size-entry");
    width_entry.add_css_class("recording-editor-size-entry");
    height_entry.add_css_class("editor-crop-size-entry");
    height_entry.add_css_class("recording-editor-size-entry");
    {
        let state = state.lock().unwrap();
        width_entry.set_text(&state.metadata.width.to_string());
        height_entry.set_text(&state.metadata.height.to_string());
    }
    width_entry.set_sensitive(false);
    height_entry.set_sensitive(false);

    for option in dimension_options {
        let item = Button::with_label(&option);
        item.set_has_frame(false);
        item.add_css_class("editor-popover-list-item");
        item.add_css_class("flat");
        item.add_css_class("recording-editor-dropdown-item");
        item.set_hexpand(true);
        let state_select = state.clone();
        let estimate_label_select = estimate_label.clone();
        let width_entry_select = width_entry.clone();
        let height_entry_select = height_entry.clone();
        let dimension_label_select = dimension_label.clone();
        let dimension_popover_select = dimension_popover.clone();
        let option_select = option.clone();
        item.connect_clicked(move |_| {
            let preset = DimensionPreset::from_label(&option_select);
            width_entry_select.set_sensitive(preset == DimensionPreset::Custom);
            height_entry_select.set_sensitive(preset == DimensionPreset::Custom);
            let mut state_guard = state_select.lock().unwrap();
            state_guard.dimension_preset = preset;
            let (width, height) = state_guard.target_dimensions();
            width_entry_select.set_text(&width.to_string());
            height_entry_select.set_text(&height.to_string());
            drop(state_guard);
            dimension_label_select.set_text(&option_select);
            dimension_popover_select.popdown();
            update_estimate(&estimate_label_select, &state_select, false);
        });
        dimension_list.append(&item);
    }

    dimensions_body.append(&field_row("Width", &width_entry));
    dimensions_body.append(&field_row("Height", &height_entry));
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
    quality_scale.add_css_class("editor-toolbar-size-slider");
    quality_scale.add_css_class("recording-editor-quality-slider");
    quality_scale.set_value(70.0);
    quality_scale.set_hexpand(true);
    quality_scale.set_draw_value(false);
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
    for button in [&audio_unchanged, &audio_mono, &audio_muted] {
        button.add_css_class("editor-background-checkbox");
        button.add_css_class("recording-editor-audio-choice");
    }
    audio_mono.set_group(Some(&audio_unchanged));
    audio_muted.set_group(Some(&audio_unchanged));
    audio_unchanged.set_active(true);
    if !state.lock().unwrap().metadata.has_audio {
        audio_mono.set_sensitive(false);
        audio_muted.set_sensitive(false);
    }
    audio_body.append(&audio_unchanged);
    audio_body.append(&audio_mono);
    audio_body.append(&audio_muted);
    settings.append(&quality_body);
    settings.append(&audio_body);

    panels.append(&dimensions);
    panels.append(&settings);

    let controls = EditorControls {
        dimension_button,
        dimension_popover,
        width_entry,
        height_entry,
        quality_scale,
        audio_unchanged,
        audio_mono,
        audio_muted,
    };
    wire_controls(&controls, state, estimate_label);

    (panels, controls)
}

fn field_row(label: &str, entry: &Entry) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 10);
    let label = Label::new(Some(label));
    label.add_css_class("recording-editor-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);
    row.append(entry);
    row
}

fn wire_controls(
    controls: &EditorControls,
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
) {
    controls.width_entry.connect_changed({
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        let height_entry = controls.height_entry.clone();
        move |entry| {
            let width = entry.text().parse::<u32>().unwrap_or(64);
            let height = height_entry.text().parse::<u32>().unwrap_or(64);
            let mut state_guard = state.lock().unwrap();
            state_guard.custom_width = width;
            state_guard.custom_height = height;
            drop(state_guard);
            update_estimate(&estimate_label, &state, false);
        }
    });
    controls.height_entry.connect_changed({
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        let width_entry = controls.width_entry.clone();
        move |entry| {
            let width = width_entry.text().parse::<u32>().unwrap_or(64);
            let height = entry.text().parse::<u32>().unwrap_or(64);
            let mut state_guard = state.lock().unwrap();
            state_guard.custom_width = width;
            state_guard.custom_height = height;
            drop(state_guard);
            update_estimate(&estimate_label, &state, false);
        }
    });

    controls.quality_scale.connect_value_changed({
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        move |scale| {
            state.lock().unwrap().quality = scale.value().round().clamp(0.0, 100.0) as u8;
            update_estimate(&estimate_label, &state, false);
        }
    });

    for (button, mode) in [
        (controls.audio_unchanged.clone(), AudioMode::Unchanged),
        (controls.audio_mono.clone(), AudioMode::Mono),
        (controls.audio_muted.clone(), AudioMode::Muted),
    ] {
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                state.lock().unwrap().audio_mode = mode;
                update_estimate(&estimate_label, &state, false);
            }
        });
    }
}

fn build_footer(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
    controls: EditorControls,
    exporting: Rc<Cell<bool>>,
) -> GtkBox {
    let footer = GtkBox::new(Orientation::Horizontal, 10);
    footer.add_css_class("recording-editor-footer");
    footer.set_hexpand(true);

    let cancel = Button::with_label("Cancel");
    cancel.set_has_frame(false);
    cancel.add_css_class("editor-tool-button");
    cancel.add_css_class("recording-editor-secondary-button");
    let window_for_cancel = window.clone();
    cancel.connect_clicked(move |_| window_for_cancel.close());

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);

    let trim_only = Button::with_label("Trim Only");
    trim_only.set_has_frame(false);
    trim_only.add_css_class("editor-tool-button");
    trim_only.add_css_class("recording-editor-secondary-button");
    let convert = Button::with_label("Trim & Convert");
    convert.set_has_frame(false);
    convert.add_css_class("editor-done-button");
    convert.add_css_class("recording-editor-primary-button");
    let spinner = Spinner::new();
    spinner.set_visible(false);

    let export_controls = vec![
        cancel.clone().upcast::<gtk4::Widget>(),
        trim_only.clone().upcast::<gtk4::Widget>(),
        convert.clone().upcast::<gtk4::Widget>(),
        controls.dimension_button.clone().upcast::<gtk4::Widget>(),
        controls.dimension_popover.clone().upcast::<gtk4::Widget>(),
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

    footer.append(&cancel);
    footer.append(&spacer);
    footer.append(&estimate_label);
    footer.append(&spinner);
    footer.append(&trim_only);
    footer.append(&convert);
    footer
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
                    Ok(path) => show_success_dialog(&window, path),
                    Err(err) if !convert => show_error_dialog(
                        &window,
                        "Trim failed",
                        "ApexShot could not trim this recording without conversion. Try Trim & Convert.",
                        Some(&err),
                    ),
                    Err(err) => show_error_dialog(
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
                show_error_dialog(
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

fn update_estimate(label: &Label, state: &Arc<Mutex<VideoEditState>>, trim_only: bool) {
    let state = state.lock().unwrap();
    label.set_text(&format!(
        "Estimated file size: ~{}",
        format_size(state.estimated_size_bytes(trim_only))
    ));
}

fn update_time_labels(start_label: &Label, end_label: &Label, state: &Arc<Mutex<VideoEditState>>) {
    let state = state.lock().unwrap();
    start_label.set_text(&format!(
        "Start {}",
        format_duration(state.trim_start_seconds)
    ));
    end_label.set_text(&format!("End {}", format_duration(state.trim_end_seconds)));
}

fn show_success_dialog(parent: &ApplicationWindow, path: PathBuf) {
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let dialog = gtk4::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .message_type(gtk4::MessageType::Info)
        .buttons(gtk4::ButtonsType::None)
        .text("Export complete")
        .secondary_text(format!("Saved {} ({})", path.display(), format_size(size)))
        .build();
    dialog.add_button("Open Folder", gtk4::ResponseType::Accept);
    dialog.add_button("Close", gtk4::ResponseType::Close);
    dialog.connect_response(move |dialog, response| {
        if response == gtk4::ResponseType::Accept {
            if let Some(parent_dir) = path.parent() {
                let _ = std::process::Command::new("xdg-open")
                    .arg(parent_dir)
                    .spawn();
            }
        }
        dialog.close();
    });
    dialog.present();
}

fn show_error_dialog(parent: &ApplicationWindow, title: &str, message: &str, detail: Option<&str>) {
    let secondary = match detail {
        Some(detail) if !detail.is_empty() => format!("{message}\n\n{detail}"),
        _ => message.to_string(),
    };
    let dialog = gtk4::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .message_type(gtk4::MessageType::Error)
        .buttons(gtk4::ButtonsType::Close)
        .text(title)
        .secondary_text(secondary)
        .build();
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.present();
}

fn format_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let seconds = seconds - (minutes as f64 * 60.0);
    format!("{minutes}:{seconds:04.1}")
}
