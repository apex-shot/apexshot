use super::{crop_dialog, footer};
use crate::recording::editor::model::{
    closest_aspect_ratio, format_webcut_time, view_to_source, VideoBackground,
    VideoEditState, ZoomClip, ZoomMode, WEBCUT_ASPECT_RATIOS,
};
use gtk4::{
    glib, prelude::*, Align, ApplicationWindow, AspectFrame, Box as GtkBox, Button, DrawingArea,
    CssProvider, GestureDrag, Image, Label, MediaFile, Orientation, Overlay, Picture, Popover,
    Separator,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub(super) fn build_preview(
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
) -> (GtkBox, MediaFile, Button) {
    build_preview_inner(state, estimate_label, None, true)
}

pub(super) fn build_preview_with_media(
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
    media: Option<MediaFile>,
) -> (GtkBox, MediaFile, Button) {
    build_preview_inner(state, estimate_label, media, false)
}

fn build_preview_inner(
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
    media: Option<MediaFile>,
    show_player_bar: bool,
) -> (GtkBox, MediaFile, Button) {
    let path = state.lock().unwrap().metadata.path.clone();

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

    let media = media.unwrap_or_else(|| MediaFile::for_filename(path));
    media.set_loop(true);

    let picture = Picture::for_paintable(&media);
    picture.add_css_class("recording-editor-video");
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_halign(Align::Fill);
    picture.set_valign(Align::Fill);
    picture.set_keep_aspect_ratio(true);
    picture.set_can_shrink(true);
    picture.add_css_class("recording-editor-video-zoom-live");
    let zoom_css = CssProvider::new();
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &zoom_css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 3,
        );
    }

    let clip = Overlay::new();
    clip.add_css_class("recording-editor-preview-clip");
    clip.set_overflow(gtk4::Overflow::Hidden);
    clip.set_hexpand(true);
    clip.set_vexpand(true);
    clip.set_halign(Align::Fill);
    clip.set_valign(Align::Fill);
    let bounds = GtkBox::new(Orientation::Vertical, 0);
    bounds.set_hexpand(true);
    bounds.set_vexpand(true);
    clip.set_child(Some(&bounds));
    clip.add_overlay(&picture);

    let overlay = Overlay::new();
    overlay.add_css_class("recording-editor-preview-canvas");
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);
    overlay.set_halign(Align::Fill);
    overlay.set_valign(Align::Fill);
    overlay.set_overflow(gtk4::Overflow::Hidden);
    overlay.set_child(Some(&clip));

    let initial_ratio = {
        let state = state.lock().unwrap();
        let (w, h) = state.padded_output_dimensions();
        canvas_ratio(w, h)
    };
    let stage = AspectFrame::new(0.5, 0.5, initial_ratio, false);
    stage.add_css_class("recording-editor-preview-stage");
    stage.set_hexpand(true);
    stage.set_vexpand(true);
    stage.set_halign(Align::Fill);
    stage.set_valign(Align::Fill);
    stage.set_child(Some(&overlay));

    let cursor_layer = DrawingArea::new();
    cursor_layer.set_hexpand(true);
    cursor_layer.set_vexpand(true);
    cursor_layer.set_can_target(true);
    let placing_focus = Rc::new(Cell::new(false));
    cursor_layer.set_draw_func({
        let state = state.clone();
        let picture = picture.clone();
        let placing_focus = placing_focus.clone();
        move |_, cr, width, height| {
            draw_preview_overlays(&state, &picture, cr, width, height, placing_focus.get());
        }
    });
    overlay.add_overlay(&cursor_layer);

    let zoom_badge = Label::new(None);
    zoom_badge.add_css_class("recording-editor-dim-badge");
    zoom_badge.set_halign(Align::End);
    zoom_badge.set_valign(Align::End);
    zoom_badge.set_margin_end(12);
    zoom_badge.set_margin_bottom(12);
    zoom_badge.set_can_target(false);
    overlay.add_overlay(&zoom_badge);

    let player = build_player_bar(true);
    wire_aspect_menu(
        &player.list,
        &player.popover,
        &player.aspect_label,
        &player.aspect_icon,
        state.clone(),
        estimate_label,
    );
    let play_button = player.play_button.clone();
    let clock = player.clock.clone();
    let aspect_label = player.aspect_label.clone();
    let aspect_icon = player.aspect_icon.clone();
    let player_bar = player.bar;

    {
        let state = state.clone();
        let zoom_badge = zoom_badge.clone();
        let cursor_layer = cursor_layer.clone();
        let picture = picture.clone();
        let clip = clip.clone();
        let zoom_css = zoom_css.clone();
        let stage = stage.clone();
        let clock = clock.clone();
        let aspect_label = aspect_label.clone();
        let aspect_icon = aspect_icon.clone();
        let media_tick = media.clone();
        let placing_focus = placing_focus.clone();
        let cursor_layer_tick = cursor_layer.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            let playing = media_tick.is_playing();
            let (dims, zoom, pad, playhead, duration, hidden, label, placing) = {
                let s = state.lock().unwrap();
                let source_t = s.source_playhead();
                let (scale, _) = s.eval_zoom(source_t);
                (
                    s.padded_output_dimensions(),
                    scale,
                    !s.background.is_none(),
                    source_t,
                    s.metadata.duration_seconds,
                    s.video_hidden,
                    s.canvas_label(),
                    placing_manual(&s, playing),
                )
            };
            placing_focus.set(placing);
            let crosshair = placing
                .then(|| gtk4::gdk::Cursor::from_name("crosshair", None))
                .flatten();
            cursor_layer_tick.set_cursor(crosshair.as_ref());
            picture.set_opacity(if hidden { 0.0 } else { 1.0 });
            clock.set_text(&format!(
                "{} / {}",
                format_webcut_time(playhead),
                format_webcut_time(duration)
            ));
            aspect_label.set_text(label);
            aspect_icon.set_icon_name(Some(aspect_ratio_icon(label)));
            if zoom > 1.01 && !hidden && !placing {
                zoom_badge.set_text(&format!("{:.0}%", zoom * 100.0));
                zoom_badge.set_visible(true);
            } else {
                zoom_badge.set_visible(false);
            }
            let next_ratio = canvas_ratio(dims.0, dims.1);
            if (stage.ratio() - next_ratio).abs() > 0.001 {
                stage.set_ratio(next_ratio);
            }
            apply_preview_zoom(&state, &picture, &zoom_css, playhead, placing);
            apply_preview_pad(&clip, pad);
            cursor_layer.queue_draw();
            glib::ControlFlow::Continue
        });
    }

    let drag = GestureDrag::new();
    drag.set_button(1);
    let dragging_focus = Rc::new(RefCell::new(None::<(f64, f64, f64, f64)>));
    drag.connect_drag_begin({
        let state = state.clone();
        let dragging_focus = dragging_focus.clone();
        let media = media.clone();
        let cursor_layer = cursor_layer.clone();
        move |gesture, x, y| {
            if media.is_playing() {
                return;
            }
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let height = gesture
                .widget()
                .map(|widget| widget.allocated_height().max(1) as f64)
                .unwrap_or(1.0);
            let view = {
                let mut state = state.lock().unwrap();
                if !placing_manual(&state, false) || state.zoom_locked {
                    return;
                }
                let view = state.crop_or_full();
                state.set_selected_zoom_center(view_to_source(view, x, y, width, height));
                view
            };
            *dragging_focus.borrow_mut() = Some(view);
            cursor_layer.queue_draw();
        }
    });
    drag.connect_drag_update({
        let state = state.clone();
        let dragging_focus = dragging_focus.clone();
        let cursor_layer = cursor_layer.clone();
        move |gesture, offset_x, offset_y| {
            let Some(view) = *dragging_focus.borrow() else {
                return;
            };
            let Some((start_x, start_y)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let height = gesture
                .widget()
                .map(|widget| widget.allocated_height().max(1) as f64)
                .unwrap_or(1.0);
            let mut state = state.lock().unwrap();
            state.set_selected_zoom_center(view_to_source(
                view,
                start_x + offset_x,
                start_y + offset_y,
                width,
                height,
            ));
            drop(state);
            cursor_layer.queue_draw();
        }
    });
    drag.connect_drag_end({
        let dragging_focus = dragging_focus.clone();
        move |_, _, _| {
            *dragging_focus.borrow_mut() = None;
        }
    });
    cursor_layer.add_controller(drag);

    workspace.append(&stage);
    root.append(&workspace);
    if show_player_bar {
        root.append(&player_bar);
    }
    (root, media, play_button)
}

pub(super) fn build_stage_tools(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) -> GtkBox {
    let bar = GtkBox::new(Orientation::Horizontal, 10);
    bar.add_css_class("recording-editor-stage-tools");
    bar.set_halign(Align::Center);
    bar.set_valign(Align::End);
    bar.set_hexpand(true);

    let initial = {
        let guard = state.lock().unwrap();
        stage_aspect_label(&guard)
    };
    let aspect = Button::new();
    aspect.set_has_frame(false);
    aspect.add_css_class("recording-editor-stage-chip");
    aspect.set_tooltip_text(Some("Change aspect ratio"));
    let aspect_label = Label::new(Some(initial));
    aspect_label.add_css_class("recording-editor-stage-chip-label");
    aspect.set_child(Some(&aspect_label));

    let popover = Popover::new();
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Top);
    popover.add_css_class("recording-editor-stage-aspect");
    let list = GtkBox::new(Orientation::Vertical, 2);
    list.add_css_class("recording-editor-stage-aspect-list");
    popover.set_child(Some(&list));
    popover.set_parent(&aspect);
    aspect.connect_clicked({
        let popover = popover.clone();
        move |_| popover.popup()
    });

    append_stage_aspect_item(
        &list,
        &popover,
        &aspect_label,
        "Original",
        None,
        state.clone(),
        on_change.clone(),
    );
    for &(label, width, height) in &WEBCUT_ASPECT_RATIOS {
        append_stage_aspect_item(
            &list,
            &popover,
            &aspect_label,
            label,
            Some((width, height)),
            state.clone(),
            on_change.clone(),
        );
    }

    let rule = Separator::new(Orientation::Vertical);
    rule.add_css_class("recording-editor-stage-rule");
    rule.set_valign(Align::Center);

    let crop = Button::new();
    crop.set_has_frame(false);
    crop.add_css_class("recording-editor-stage-chip");
    crop.set_tooltip_text(Some("Crop video"));
    let crop_row = GtkBox::new(Orientation::Horizontal, 6);
    let crop_icon = Image::from_icon_name("image-crop-symbolic");
    crop_icon.set_pixel_size(13);
    let crop_label = Label::new(Some("Crop video"));
    crop_label.add_css_class("recording-editor-stage-chip-label");
    crop_row.append(&crop_icon);
    crop_row.append(&crop_label);
    crop.set_child(Some(&crop_row));
    {
        let window = window.clone();
        let state = state.clone();
        let on_change = on_change.clone();
        crop.connect_clicked(move |_| crop_dialog::show_crop(&window, &state, on_change.clone()));
    }

    bar.append(&aspect);
    bar.append(&rule);
    bar.append(&crop);
    bar
}

fn stage_aspect_label(state: &VideoEditState) -> &'static str {
    let (width, height) = state.canvas_dimensions();
    closest_aspect_ratio(width, height)
}

fn append_stage_aspect_item(
    list: &GtkBox,
    popover: &Popover,
    aspect_label: &Label,
    label: &'static str,
    size: Option<(u32, u32)>,
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) {
    let item = Button::new();
    item.set_has_frame(false);
    item.add_css_class("recording-editor-stage-aspect-item");
    item.set_hexpand(true);
    item.set_child(Some(&Label::new(Some(label))));
    item.connect_clicked({
        let aspect_label = aspect_label.clone();
        let popover = popover.clone();
        move |_| {
            {
                let mut guard = state.lock().unwrap();
                match size {
                    Some((width, height)) => guard.apply_aspect_ratio(width, height),
                    None => guard.reset_aspect_ratio(),
                }
                aspect_label.set_text(stage_aspect_label(&guard));
            }
            popover.popdown();
            on_change();
        }
    });
    list.append(&item);
}

pub(super) fn build_empty_player_bar() -> GtkBox {
    let player = build_player_bar(false);
    player.clock.set_text("00:00:00.000 / 00:00:00.000");
    player.aspect_label.set_text("Original");
    player.bar
}

struct PlayerBar {
    bar: GtkBox,
    play_button: Button,
    clock: Label,
    aspect_label: Label,
    aspect_icon: Image,
    list: GtkBox,
    popover: Popover,
}

fn build_player_bar(enabled: bool) -> PlayerBar {
    let bar = GtkBox::new(Orientation::Horizontal, 0);
    bar.add_css_class("recording-editor-player-bar");
    bar.set_hexpand(true);
    bar.set_vexpand(false);

    let left = GtkBox::new(Orientation::Horizontal, 0);
    left.set_halign(Align::Start);
    left.set_hexpand(true);
    left.set_valign(Align::Center);
    let clock = Label::new(Some("00:00:00.000 / 00:00:00.000"));
    clock.add_css_class("recording-editor-player-clock");
    clock.set_xalign(0.0);
    left.append(&clock);

    let play_button = Button::new();
    play_button.add_css_class("recording-editor-play-button");
    play_button.add_css_class("recording-editor-player-play");
    let play_icon = Image::from_icon_name("media-playback-start-symbolic");
    play_icon.set_pixel_size(18);
    play_button.set_child(Some(&play_icon));
    play_button.set_valign(Align::Center);
    play_button.set_halign(Align::Center);
    play_button.set_tooltip_text(Some("Play"));
    play_button.set_sensitive(enabled);

    let right = GtkBox::new(Orientation::Horizontal, 0);
    right.set_halign(Align::End);
    right.set_hexpand(true);
    right.set_valign(Align::Center);

    let aspect_button = Button::new();
    aspect_button.set_has_frame(false);
    aspect_button.add_css_class("recording-editor-aspect-button");
    aspect_button.set_sensitive(enabled);
    aspect_button.set_tooltip_text(Some("Change video size"));

    let aspect_row = GtkBox::new(Orientation::Horizontal, 4);
    let aspect_icon = Image::from_icon_name(aspect_ratio_icon("Original"));
    aspect_icon.set_pixel_size(14);
    aspect_icon.add_css_class("recording-editor-aspect-item-icon");
    let aspect_label = Label::new(Some("Original"));
    aspect_label.add_css_class("recording-editor-aspect-label");
    aspect_row.append(&aspect_icon);
    aspect_row.append(&aspect_label);
    aspect_button.set_child(Some(&aspect_row));

    let popover = Popover::new();
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Top);
    popover.add_css_class("recording-editor-dropdown-popover");
    popover.add_css_class("recording-editor-aspect-popover");
    let list = GtkBox::new(Orientation::Vertical, 0);
    list.add_css_class("recording-editor-dropdown-list");
    list.add_css_class("recording-editor-aspect-list");
    popover.set_child(Some(&list));
    popover.set_parent(&aspect_button);
    aspect_button.connect_clicked({
        let popover = popover.clone();
        move |_| {
            popover.popup();
        }
    });
    right.append(&aspect_button);

    bar.append(&left);
    bar.append(&play_button);
    bar.append(&right);
    PlayerBar {
        bar,
        play_button,
        clock,
        aspect_label,
        aspect_icon,
        list,
        popover,
    }
}

fn aspect_ratio_icon(label: &str) -> &'static str {
    match label {
        "Original" => "video-x-generic-symbolic",
        "21:9" => "tv-symbolic",
        "16:9" => "video-display-symbolic",
        "4:3" => "tablet-symbolic",
        "9:16" => "phone-symbolic",
        "3:4" => "computer-apple-ipad-symbolic",
        "1:1" => "view-grid-symbolic",
        _ => "video-display-symbolic",
    }
}

fn wire_aspect_menu(
    list: &GtkBox,
    popover: &Popover,
    aspect_label: &Label,
    aspect_icon: &Image,
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
) {
    append_aspect_item(
        list,
        popover,
        aspect_label,
        aspect_icon,
        "Original",
        state.clone(),
        estimate_label.clone(),
        None,
    );
    for &(label, width, height) in &WEBCUT_ASPECT_RATIOS {
        append_aspect_item(
            list,
            popover,
            aspect_label,
            aspect_icon,
            label,
            state.clone(),
            estimate_label.clone(),
            Some((width, height)),
        );
    }
}

fn append_aspect_item(
    list: &GtkBox,
    popover: &Popover,
    aspect_label: &Label,
    aspect_icon: &Image,
    label: &'static str,
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
    size: Option<(u32, u32)>,
) {
    let item = Button::new();
    item.set_has_frame(false);
    item.add_css_class("recording-editor-dropdown-item");
    item.add_css_class("recording-editor-aspect-item");
    item.set_hexpand(true);

    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.set_halign(Align::Start);
    let icon = Image::from_icon_name(aspect_ratio_icon(label));
    icon.set_pixel_size(14);
    icon.add_css_class("recording-editor-aspect-item-icon");
    let text = Label::new(Some(label));
    text.set_xalign(0.0);
    row.append(&icon);
    row.append(&text);
    item.set_child(Some(&row));

    let aspect_label = aspect_label.clone();
    let aspect_icon = aspect_icon.clone();
    let popover = popover.clone();
    item.connect_clicked(move |_| {
        {
            let mut guard = state.lock().unwrap();
            match size {
                Some((width, height)) => guard.apply_aspect_ratio(width, height),
                None => guard.reset_aspect_ratio(),
            }
        }
        aspect_label.set_text(label);
        aspect_icon.set_icon_name(Some(aspect_ratio_icon(label)));
        popover.popdown();
        footer::update_estimate(&estimate_label, &state, false);
    });
    list.append(&item);
}

fn canvas_ratio(width: u32, height: u32) -> f32 {
    width.max(1) as f32 / height.max(1) as f32
}

fn apply_preview_pad(clip: &Overlay, padded: bool) {
    let pad = if padded { 18 } else { 0 };
    clip.set_margin_start(pad);
    clip.set_margin_end(pad);
    clip.set_margin_top(pad);
    clip.set_margin_bottom(pad);
}

/// Source-coords rect currently visible on stage: static crop intersected
/// with the active zoom window.
fn source_to_zoomed_point(
    x: f64,
    y: f64,
    view: (f64, f64, f64, f64),
    center: (f64, f64),
    scale: f64,
    widget_w: f64,
    widget_h: f64,
) -> (f64, f64) {
    let (vx, vy, vw, vh) = view;
    let ox = (center.0 - vx) / vw.max(1.0);
    let oy = (center.1 - vy) / vh.max(1.0);
    let nx = (x - vx) / vw.max(1.0);
    let ny = (y - vy) / vh.max(1.0);
    (
        (ox + (nx - ox) * scale) * widget_w,
        (oy + (ny - oy) * scale) * widget_h,
    )
}

fn placing_manual(state: &VideoEditState, playing: bool) -> bool {
    !playing
        && state
            .selected_zoom_clip()
            .is_some_and(|clip| clip.mode == ZoomMode::Manual)
}

fn apply_preview_zoom(
    state: &Arc<Mutex<VideoEditState>>,
    picture: &Picture,
    provider: &CssProvider,
    playhead: f64,
    placing: bool,
) {
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_halign(Align::Fill);
    picture.set_valign(Align::Fill);
    picture.set_size_request(-1, -1);
    picture.set_margin_start(0);
    picture.set_margin_top(0);
    let css = {
        let state = state.lock().unwrap();
        let (scale, center) = if placing {
            (1.0, (0.0, 0.0))
        } else {
            state.eval_zoom(playhead)
        };
        if placing || scale <= 1.01 {
            ".recording-editor-video-zoom-live { transform: none; }".to_string()
        } else {
            let (cx, cy, cw, ch) = state.crop_or_full();
            let ox = ((center.0 - cx) / cw.max(1.0) * 100.0).clamp(0.0, 100.0);
            let oy = ((center.1 - cy) / ch.max(1.0) * 100.0).clamp(0.0, 100.0);
            format!(
                ".recording-editor-video-zoom-live {{ transform: scale({scale:.4}); transform-origin: {ox:.2}% {oy:.2}%; }}"
            )
        }
    };
    provider.load_from_data(&css);
}

fn draw_preview_overlays(
    state: &Arc<Mutex<VideoEditState>>,
    picture: &Picture,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    placing: bool,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    if matches!(state.background, VideoBackground::Plain { .. }) {
        if let VideoBackground::Plain { r, g, b } = state.background {
            cr.set_source_rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
            cr.rectangle(0.0, 0.0, w, h);
            let _ = cr.fill();
        }
    } else if matches!(state.background, VideoBackground::Gradient(_)) {
        let gradient = gtk4::cairo::LinearGradient::new(0.0, 0.0, w, h);
        gradient.add_color_stop_rgb(0.0, 0.18, 0.22, 0.42);
        gradient.add_color_stop_rgb(1.0, 0.42, 0.18, 0.28);
        let _ = cr.set_source(&gradient);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
    }

    let source_t = state.source_playhead();
    let view = state.crop_or_full();
    let (scale, center) = if placing {
        (1.0, (0.0, 0.0))
    } else {
        state.eval_zoom(source_t)
    };
    let _ = picture;

    if let Some(sidecar) = &state.sidecar {
        if let Some((x, y, kind)) = sidecar.interpolated_at(source_t) {
            let pulse = sidecar.click_pulse_at(source_t);
            let (px, py) = source_to_zoomed_point(x, y, view, center, scale, w, h);
            draw_apexshot_cursor(cr, px, py, pulse, kind.as_str());
        }
    }

    if placing {
        if let Some(clip) = state.selected_zoom_clip() {
            let (x, y, rect_w, rect_h) = manual_focus_rect(clip, view, w, h);
            cr.set_source_rgba(1.0, 0.48, 0.12, 0.22);
            cr.rectangle(x, y, rect_w, rect_h);
            let _ = cr.fill_preserve();
            cr.set_source_rgba(1.0, 0.62, 0.20, 0.95);
            cr.set_line_width(2.0);
            let _ = cr.stroke();
        }
    }
}

fn manual_focus_rect(
    clip: &ZoomClip,
    view: (f64, f64, f64, f64),
    widget_w: f64,
    widget_h: f64,
) -> (f64, f64, f64, f64) {
    let (view_x, view_y, view_w, view_h) = view;
    let rect_w = widget_w / clip.scale.max(1.0);
    let rect_h = widget_h / clip.scale.max(1.0);
    let x = ((clip.center.0 - view_x) / view_w.max(1.0)) * widget_w - rect_w / 2.0;
    let y = ((clip.center.1 - view_y) / view_h.max(1.0)) * widget_h - rect_h / 2.0;
    (x, y, rect_w, rect_h)
}

fn draw_apexshot_cursor(cr: &gtk4::cairo::Context, x: f64, y: f64, pulse: f64, kind: &str) {
    let size = 16.0 * pulse;
    if pulse > 1.02 {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.28);
        cr.arc(x, y, size + 6.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.set_line_width(1.4);
    match kind {
        "text" => {
            cr.move_to(x, y - size * 0.6);
            cr.line_to(x, y + size * 0.6);
            let _ = cr.stroke();
        }
        "hand" => {
            cr.arc(x, y, size * 0.35, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        }
        _ => {
            cr.move_to(x, y);
            cr.line_to(x + size * 0.15, y + size * 0.85);
            cr.line_to(x + size * 0.38, y + size * 0.62);
            cr.close_path();
            let _ = cr.fill();
            cr.set_source_rgb(0.12, 0.12, 0.14);
            cr.move_to(x, y);
            cr.line_to(x + size * 0.15, y + size * 0.85);
            cr.line_to(x + size * 0.38, y + size * 0.62);
            cr.close_path();
            let _ = cr.stroke();
        }
    }
}
