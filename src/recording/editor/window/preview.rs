use super::{crop_dialog, footer};
use crate::recording::editor::model::background_render::render_gradient;
use crate::recording::editor::model::{
    even_crop_rect, format_timecode, source_to_zoomed_point, view_to_source, zoom_camera_transform,
    CursorSettings, ExportQuality, VideoBackground, VideoEditState, VideoGradient, ZoomClip,
    ZoomMode, FRAME_ASPECT_RATIOS,
};
use crate::recording::editor::sidecar::CursorMotion;
use gtk4::{
    gdk, glib, prelude::*, Align, ApplicationWindow, AspectFrame, Box as GtkBox, Button,
    CssProvider, DrawingArea, GestureDrag, Image, Label, MediaFile, Orientation, Overlay, Picture,
    Popover, Separator,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::i18n::t;

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
    let (path, has_video) = {
        let state = state.lock().unwrap();
        (
            state.metadata.path.clone(),
            state.metadata.duration_seconds > 0.0,
        )
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

    let media = media.unwrap_or_else(|| MediaFile::for_filename(path));
    media.set_loop(false);

    let picture = Picture::for_paintable(&media);
    picture.add_css_class("recording-editor-video");
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_halign(Align::Fill);
    picture.set_valign(Align::Fill);
    picture.set_content_fit(gtk4::ContentFit::Contain);
    picture.set_can_shrink(true);
    picture.add_css_class("recording-editor-video-zoom-live");
    picture.set_visible(has_video);
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

    // Background sits behind the video so wallpaper/color surrounds the
    // padded canvas instead of painting over the footage. `clip` (the video)
    // becomes an overlay with an 18px inset when a background is active,
    // leaving the fill visible around it.
    let bg_box = GtkBox::new(Orientation::Vertical, 0);
    bg_box.add_css_class("recording-editor-preview-bg");
    bg_box.set_hexpand(true);
    bg_box.set_vexpand(true);
    bg_box.set_halign(Align::Fill);
    bg_box.set_valign(Align::Fill);
    let bg_picture = Picture::new();
    bg_picture.add_css_class("recording-editor-preview-bg-image");
    bg_picture.set_hexpand(true);
    bg_picture.set_vexpand(true);
    bg_picture.set_halign(Align::Fill);
    bg_picture.set_valign(Align::Fill);
    bg_picture.set_content_fit(gtk4::ContentFit::Cover);
    bg_picture.set_can_shrink(true);
    bg_picture.set_visible(false);
    let bg_css = CssProvider::new();
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &bg_css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
        );
    }
    let bg_stack = Overlay::new();
    bg_stack.set_hexpand(true);
    bg_stack.set_vexpand(true);
    bg_stack.set_halign(Align::Fill);
    bg_stack.set_valign(Align::Fill);
    bg_stack.set_child(Some(&bg_box));
    bg_stack.add_overlay(&bg_picture);
    overlay.set_child(Some(&bg_stack));
    overlay.add_overlay(&clip);

    let initial_ratio = {
        let state = state.lock().unwrap();
        let (w, h) = state.output_dimensions();
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
    cursor_layer.add_css_class("recording-editor-cursor-layer");
    cursor_layer.set_hexpand(true);
    cursor_layer.set_vexpand(true);
    cursor_layer.set_can_target(true);
    let placing_focus = Rc::new(Cell::new(false));
    cursor_layer.set_draw_func({
        let state = state.clone();
        let placing_focus = placing_focus.clone();
        move |_, cr, width, height| {
            draw_preview_overlays(&state, cr, width, height, placing_focus.get());
        }
    });
    overlay.add_overlay(&cursor_layer);

    let empty_hint = Label::new(Some(&t("Click to open a video, or drop one here")));
    empty_hint.add_css_class("recording-editor-empty-preview-hint");
    empty_hint.set_halign(Align::Center);
    empty_hint.set_valign(Align::Center);
    empty_hint.set_wrap(true);
    empty_hint.set_can_target(false);
    empty_hint.set_visible(!has_video);
    overlay.add_overlay(&empty_hint);

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
        let bg_css = bg_css.clone();
        let bg_picture = bg_picture.clone();
        let overlay_tick = overlay.clone();
        let stage = stage.clone();
        let clock = clock.clone();
        let aspect_label = aspect_label.clone();
        let aspect_icon = aspect_icon.clone();
        let media_tick = media.clone();
        let placing_focus = placing_focus.clone();
        let cursor_layer_tick = cursor_layer.clone();
        let empty_hint = empty_hint.clone();
        let last_zoom_css = Rc::new(RefCell::new(String::new()));
        let last_bg_css = Rc::new(RefCell::new(String::new()));
        let last_wallpaper = Rc::new(RefCell::new(String::new()));
        let last_gradient = Rc::new(RefCell::new(String::new()));
        let last_margins = Rc::new(RefCell::new((i32::MIN, 0, 0, 0)));
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            let playing = media_tick.is_playing();
            let (dims, video, zoom, playhead, duration, hidden, label, placing, background) = {
                let s = state.lock().unwrap();
                let source_t = s.source_playhead();
                let (scale, _) = s.eval_zoom(source_t);
                (
                    s.output_dimensions(),
                    s.video_rect_dimensions(),
                    scale,
                    source_t,
                    s.metadata.duration_seconds,
                    s.video_hidden,
                    s.canvas_label(),
                    placing_manual(&s, playing),
                    s.background.clone(),
                )
            };
            placing_focus.set(placing);
            let crosshair = placing
                .then(|| gtk4::gdk::Cursor::from_name("crosshair", None))
                .flatten();
            cursor_layer_tick.set_cursor(crosshair.as_ref());
            picture.set_visible(duration > 0.0);
            empty_hint.set_visible(duration <= 0.0);
            picture.set_opacity(if hidden { 0.0 } else { 1.0 });
            clock.set_text(&format!(
                "{} / {}",
                format_timecode(playhead),
                format_timecode(duration)
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
            // Video rect first so the zoom transform below measures the
            // footage: the background fill surrounds the video with no black
            // letterbox bars. Covers the unpadded case too (zero margins),
            // and keeps cursor math mapped to the video.
            apply_preview_clip(
                &clip,
                &cursor_layer,
                &overlay_tick,
                video,
                dims,
                &last_margins,
            );
            apply_preview_view(
                &state,
                &picture,
                &clip,
                &zoom_css,
                &last_zoom_css,
                playhead,
                placing,
            );
            apply_preview_background(
                &bg_css,
                &bg_picture,
                &last_bg_css,
                &last_wallpaper,
                &last_gradient,
                &background,
                duration > 0.0,
                dims,
            );
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
    estimate_label: Label,
    on_change: Rc<dyn Fn()>,
) -> GtkBox {
    let bar = GtkBox::new(Orientation::Horizontal, 10);
    bar.add_css_class("recording-editor-stage-tools");
    bar.set_halign(Align::Center);
    bar.set_valign(Align::End);
    bar.set_hexpand(true);

    let initial = {
        let guard = state.lock().unwrap();
        guard.canvas_label()
    };
    let aspect = Button::new();
    aspect.set_has_frame(false);
    aspect.add_css_class("recording-editor-stage-chip");
    aspect.set_tooltip_text(Some(&t("Change aspect ratio")));
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
    for &(label, width, height) in &FRAME_ASPECT_RATIOS {
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
    crop.set_tooltip_text(Some(&t("Crop video")));
    let crop_row = GtkBox::new(Orientation::Horizontal, 6);
    let crop_icon = Image::from_icon_name("image-crop-symbolic");
    crop_icon.set_pixel_size(13);
    let crop_label = Label::new(Some(&t("Crop video")));
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

    let quality_initial = quality_label_for(state.lock().unwrap().quality);
    let quality = Button::new();
    quality.set_has_frame(false);
    quality.add_css_class("recording-editor-stage-chip");
    quality.set_tooltip_text(Some(&t("Export quality")));
    let quality_label = Label::new(Some(&quality_initial));
    quality_label.add_css_class("recording-editor-stage-chip-label");
    quality.set_child(Some(&quality_label));

    let quality_popover = Popover::new();
    quality_popover.set_has_arrow(false);
    quality_popover.set_position(gtk4::PositionType::Top);
    quality_popover.add_css_class("recording-editor-stage-aspect");
    let quality_list = GtkBox::new(Orientation::Vertical, 2);
    quality_list.add_css_class("recording-editor-stage-aspect-list");
    quality_popover.set_child(Some(&quality_list));
    quality_popover.set_parent(&quality);
    quality.connect_clicked({
        let quality_popover = quality_popover.clone();
        move |_| quality_popover.popup()
    });

    for tier in [
        ExportQuality::Balanced,
        ExportQuality::High,
        ExportQuality::Ultra,
    ] {
        append_stage_quality_item(
            &quality_list,
            &quality_popover,
            &quality_label,
            tier,
            state.clone(),
            on_change.clone(),
        );
    }

    let quality_rule = Separator::new(Orientation::Vertical);
    quality_rule.add_css_class("recording-editor-stage-rule");
    quality_rule.set_valign(Align::Center);

    let estimate_rule = Separator::new(Orientation::Vertical);
    estimate_rule.add_css_class("recording-editor-stage-rule");
    estimate_rule.set_valign(Align::Center);
    // Paint once at build: afterwards every edit refreshes it through the
    // same helper, so the footer never shows a stale value.
    footer::update_estimate(&estimate_label, &state, false);

    bar.append(&aspect);
    bar.append(&rule);
    bar.append(&crop);
    bar.append(&quality_rule);
    bar.append(&quality);
    bar.append(&estimate_rule);
    bar.append(&estimate_label);
    bar
}

fn quality_label_for(tier: ExportQuality) -> String {
    match tier {
        ExportQuality::Balanced => t("Balanced"),
        ExportQuality::High => t("High"),
        ExportQuality::Ultra => t("Ultra"),
    }
}

fn append_stage_quality_item(
    list: &GtkBox,
    popover: &Popover,
    quality_label: &Label,
    tier: ExportQuality,
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) {
    let item = Button::new();
    item.set_has_frame(false);
    item.add_css_class("recording-editor-stage-aspect-item");
    item.set_hexpand(true);
    item.set_child(Some(&Label::new(Some(&quality_label_for(tier)))));
    item.connect_clicked({
        let quality_label = quality_label.clone();
        let popover = popover.clone();
        move |_| {
            state.lock().unwrap().quality = tier;
            quality_label.set_text(&quality_label_for(tier));
            popover.popdown();
            on_change();
        }
    });
    list.append(&item);
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
                aspect_label.set_text(guard.canvas_label());
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
    player.aspect_label.set_text(&t("Original"));
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
    play_button.set_tooltip_text(Some(&t("Play")));
    play_button.set_sensitive(enabled);

    let right = GtkBox::new(Orientation::Horizontal, 0);
    right.set_halign(Align::End);
    right.set_hexpand(true);
    right.set_valign(Align::Center);

    let aspect_button = Button::new();
    aspect_button.set_has_frame(false);
    aspect_button.add_css_class("recording-editor-aspect-button");
    aspect_button.set_sensitive(enabled);
    aspect_button.set_tooltip_text(Some(&t("Change video size")));

    let aspect_row = GtkBox::new(Orientation::Horizontal, 4);
    let aspect_icon = Image::from_icon_name(aspect_ratio_icon("Original"));
    aspect_icon.set_pixel_size(14);
    aspect_icon.add_css_class("recording-editor-aspect-item-icon");
    let aspect_label = Label::new(Some(&t("Original")));
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
    for &(label, width, height) in &FRAME_ASPECT_RATIOS {
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

// Center the video rect inside the stage for both the video and the cursor
// layer. The stage carries the output canvas aspect, and the rect is the
// fitted footage inside that canvas: a fixed Frame insets the video (padding
// plus any letterbox), so the background fill shows around it exactly like
// export instead of black bars. Keeping the cursor layer on the same rect
// keeps cursor math mapped to the video.
fn apply_preview_clip(
    clip: &Overlay,
    cursor_layer: &DrawingArea,
    overlay: &Overlay,
    video: (u32, u32),
    out: (u32, u32),
    last_margins: &RefCell<(i32, i32, i32, i32)>,
) {
    let fx = 1.0 - video.0.max(1) as f64 / out.0.max(1) as f64;
    let fy = 1.0 - video.1.max(1) as f64 / out.1.max(1) as f64;
    let margins = if fx <= 0.0005 && fy <= 0.0005 {
        (0, 0, 0, 0)
    } else {
        let ow = overlay.allocated_width().max(0) as f64;
        let oh = overlay.allocated_height().max(0) as f64;
        if ow < 2.0 || oh < 2.0 {
            // Allocation not ready yet; fixed inset until the next frame
            // measures the stage.
            (18, 18, 18, 18)
        } else {
            let mx = ((fx / 2.0 * ow).round().max(0.0)) as i32;
            let my = ((fy / 2.0 * oh).round().max(0.0)) as i32;
            (mx, mx, my, my)
        }
    };
    if *last_margins.borrow() == margins {
        return;
    }
    last_margins.replace(margins);
    let (ms, me, mt, mb) = margins;
    clip.set_margin_start(ms);
    clip.set_margin_end(me);
    clip.set_margin_top(mt);
    clip.set_margin_bottom(mb);
    cursor_layer.set_margin_start(ms);
    cursor_layer.set_margin_end(me);
    cursor_layer.set_margin_top(mt);
    cursor_layer.set_margin_bottom(mb);
}

fn visible_source_view(
    state: &VideoEditState,
    playhead: f64,
    placing: bool,
) -> (f64, f64, f64, f64) {
    let (cx, cy, cw, ch) = state.crop_or_full();
    if placing {
        return (cx, cy, cw, ch);
    }
    let (scale, center) = state.eval_zoom(playhead);
    if scale <= 1.01 {
        return (cx, cy, cw, ch);
    }
    let (zx, zy, zw, zh) = even_crop_rect(
        scale,
        (center.0 - cx, center.1 - cy),
        cw.max(2.0) as u32,
        ch.max(2.0) as u32,
    );
    (cx + zx as f64, cy + zy as f64, zw as f64, zh as f64)
}

fn placing_manual(state: &VideoEditState, playing: bool) -> bool {
    !playing
        && state
            .selected_zoom_clip()
            .is_some_and(|clip| clip.mode == ZoomMode::Manual)
}

fn apply_preview_view(
    state: &Arc<Mutex<VideoEditState>>,
    picture: &Picture,
    clip: &Overlay,
    provider: &CssProvider,
    last_css: &RefCell<String>,
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
    picture.set_margin_end(0);
    picture.set_margin_bottom(0);
    let clip_w = clip.allocated_width().max(0) as f64;
    let clip_h = clip.allocated_height().max(0) as f64;
    if clip_w < 2.0 || clip_h < 2.0 {
        return;
    }
    let (view, src_w, src_h) = {
        let state = state.lock().unwrap();
        (
            visible_source_view(&state, playhead, placing),
            state.metadata.width.max(1) as f64,
            state.metadata.height.max(1) as f64,
        )
    };
    let (tx, ty, sx, sy) = zoom_camera_transform(view, src_w, src_h, clip_w, clip_h);
    let video_css = if (sx - 1.0).abs() < 0.002
        && (sy - 1.0).abs() < 0.002
        && tx.abs() < 0.5
        && ty.abs() < 0.5
    {
        ".recording-editor-video-zoom-live { transform: none; }".to_string()
    } else {
        format!(
            ".recording-editor-video-zoom-live {{ transform-origin: 0px 0px; transform: translate({tx:.2}px, {ty:.2}px) scale({sx:.4}, {sy:.4}); }}"
        )
    };
    if *last_css.borrow() != video_css {
        provider.load_from_data(&video_css);
        last_css.replace(video_css);
    }
}

/// What the preview's image layer is showing. Wallpapers load from disk;
/// gradients are rasterized through the same renderer the export will use, so
/// the live preview cannot drift from the final still.
enum PreviewFill<'a> {
    Wallpaper(&'a std::path::PathBuf),
    Gradient(&'a VideoGradient),
}

fn apply_preview_background(
    provider: &CssProvider,
    picture: &Picture,
    last_css: &RefCell<String>,
    last_wallpaper: &RefCell<String>,
    last_gradient: &RefCell<String>,
    background: &VideoBackground,
    has_video: bool,
    dims: (u32, u32),
) {
    // Solid fills go through CSS on the stage box; wallpapers use a Cover-fit
    // Picture so bundled JPGs render without a Cairo decode on this path.
    // Gradients stay under a black backdrop: their dots can carry alpha, so
    // the CSS layer is what a faded stop composites onto.
    // With no fill the canvas stays black, matching the export's unset-fill
    // scene (and the image editor's black export backdrop); the empty editor
    // keeps a transparent stage so the drop hint sits on the workspace.
    let (css, fill) = match background {
        VideoBackground::None if !has_video => (
            ".recording-editor-preview-bg { background: transparent; }".to_string(),
            None,
        ),
        VideoBackground::None => (
            ".recording-editor-preview-bg { background: @recording-editor-video-surface; }"
                .to_string(),
            None,
        ),
        VideoBackground::Plain { r, g, b } => (
            format!(".recording-editor-preview-bg {{ background: rgb({r},{g},{b}); }}"),
            None,
        ),
        VideoBackground::Gradient(gradient) => (
            ".recording-editor-preview-bg { background: @recording-editor-video-surface; }"
                .to_string(),
            Some(PreviewFill::Gradient(gradient)),
        ),
        VideoBackground::Wallpaper(path) => (
            ".recording-editor-preview-bg { background: #111111; }".to_string(),
            Some(PreviewFill::Wallpaper(path)),
        ),
    };
    if *last_css.borrow() != css {
        provider.load_from_data(&css);
        last_css.replace(css);
    }
    match fill {
        Some(PreviewFill::Wallpaper(path)) => {
            last_gradient.borrow_mut().clear();
            let path = path.to_string_lossy().into_owned();
            if path != *last_wallpaper.borrow() {
                if std::path::Path::new(&path).is_file() {
                    picture.set_filename(Some(std::path::Path::new(&path)));
                }
                picture.set_visible(std::path::Path::new(&path).is_file());
                last_wallpaper.replace(path);
            } else {
                picture.set_visible(true);
            }
        }
        Some(PreviewFill::Gradient(gradient)) => {
            last_wallpaper.replace(String::new());
            let gradient = gradient.normalized();
            let key = format!("{gradient:?}");
            if key != *last_gradient.borrow() {
                let texture = gradient_texture(&gradient, dims);
                picture.set_filename(None::<&std::path::Path>);
                picture.set_paintable(Some(&texture));
                last_gradient.replace(key);
            }
            picture.set_visible(true);
        }
        None => {
            picture.set_visible(false);
            last_wallpaper.replace(String::new());
            last_gradient.borrow_mut().clear();
        }
    }
}

/// Rasterize a gradient for the preview's image layer.
///
/// The texture is deliberately small — the canvas aspect at ~480px wide — and
/// scaled up by the Cover-fit picture; a drag repaints it every frame, so
/// rendering at canvas resolution would buy nothing.
fn gradient_texture(gradient: &VideoGradient, dims: (u32, u32)) -> gdk::MemoryTexture {
    let width = 480u32;
    let aspect = dims.1 as f64 / dims.0.max(1) as f64;
    let height = ((width as f64 * aspect).round() as u32).clamp(1, 960);
    let bitmap = render_gradient(gradient, width, height);
    let bytes = glib::Bytes::from_owned(bitmap.pixels);
    gdk::MemoryTexture::new(
        width as i32,
        height as i32,
        gdk::MemoryFormat::R8g8b8,
        &bytes,
        (width * 3) as usize,
    )
}

fn draw_preview_overlays(
    state: &Arc<Mutex<VideoEditState>>,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    placing: bool,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    // Background now lives behind the video (bg_box/bg_picture); this layer
    // stays transparent so footage shows through and only cursors paint here.

    let source_t = state.source_playhead();
    let view = visible_source_view(&state, source_t, placing);
    let (zoom, _) = state.eval_zoom(source_t);

    if let Some(sidecar) = state
        .sidecar
        .as_ref()
        .filter(|sidecar| sidecar.can_render_cursor_overlay())
    {
        if let Some(mut frame) = sidecar.presented_in_video_at(
            source_t,
            cursor_motion(state.cursor),
            state.metadata.width as f64,
            state.metadata.height as f64,
        ) {
            frame.alpha *= state.cursor_hide_alpha_for_source(source_t);
            let cursor = overlay_cursor(state.cursor, zoom);
            for (x, y, progress) in sidecar.click_ripples_in_video_at(
                source_t,
                cursor.click_window_seconds(),
                state.metadata.width as f64,
                state.metadata.height as f64,
            ) {
                let (px, py) = source_to_zoomed_point(x, y, view, w, h);
                crate::recording::editor::cursor_sprite::draw_click(
                    cr,
                    px,
                    py,
                    progress,
                    cursor,
                    frame.alpha,
                );
            }
            for &(x, y, ghost) in &frame.trail {
                let (px, py) = source_to_zoomed_point(x, y, view, w, h);
                crate::recording::editor::cursor_sprite::draw_tilted(
                    cr,
                    px,
                    py,
                    1.0,
                    frame.kind.as_str(),
                    cursor,
                    frame.alpha * ghost,
                    frame.tilt,
                );
            }
            let (px, py) = source_to_zoomed_point(frame.x, frame.y, view, w, h);
            crate::recording::editor::cursor_sprite::draw_tilted(
                cr,
                px,
                py,
                1.0,
                frame.kind.as_str(),
                cursor,
                frame.alpha,
                frame.tilt,
            );
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

fn overlay_cursor(cursor: CursorSettings, zoom: f64) -> CursorSettings {
    let mut cursor = cursor.clamped();
    cursor.size = crate::recording::editor::cursor_sprite::overlay_scale(cursor.size, zoom);
    cursor
}

fn cursor_motion(cursor: CursorSettings) -> CursorMotion {
    let cursor = cursor.clamped();
    CursorMotion {
        smooth: cursor.smooth,
        hide_idle: cursor.hide_idle,
        idle_ms: cursor.idle_ms,
        trail: cursor.trail,
        tilt: cursor.tilt,
        sway: cursor.sway,
        speed: cursor.speed,
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
    let raw_x = ((clip.center.0 - view_x) / view_w.max(1.0)) * widget_w - rect_w / 2.0;
    let raw_y = ((clip.center.1 - view_y) / view_h.max(1.0)) * widget_h - rect_h / 2.0;
    let x = if rect_w >= widget_w {
        (widget_w - rect_w) / 2.0
    } else {
        raw_x.clamp(0.0, widget_w - rect_w)
    };
    let y = if rect_h >= widget_h {
        (widget_h - rect_h) / 2.0
    } else {
        raw_y.clamp(0.0, widget_h - rect_h)
    };
    (x, y, rect_w, rect_h)
}

#[cfg(test)]
mod tests {
    #[test]
    fn cursor_layer_is_not_css_zoomed() {
        let source = include_str!("preview.rs");
        assert!(source.contains("recording-editor-cursor-layer"));
        assert!(source.contains("cursor_sprite::overlay_scale"));
        assert!(source.contains("source_to_zoomed_point"));
        assert!(source.contains("cursor_hide_alpha_for_source"));
        assert!(
            !source.contains("cursor_layer.add_css_class(\"recording-editor-video-zoom-live\")")
        );
        let css = include_str!("../ui_support_css/01.css");
        assert!(css.contains(".recording-editor-cursor-layer"));
        assert!(css.contains("transform: none"));
    }

    #[test]
    fn background_renders_behind_video_not_over_it() {
        let source = include_str!("preview.rs");
        assert!(
            source.contains("recording-editor-preview-bg"),
            "preview must have a background layer behind the video"
        );
        assert!(
            source.contains("recording-editor-preview-bg-image"),
            "wallpaper needs a Cover-fit picture behind the video"
        );
        assert!(
            source.contains("fn apply_preview_background"),
            "preview must drive wallpaper/color from VideoBackground"
        );
        assert!(
            source.contains("VideoBackground::Wallpaper"),
            "preview must handle wallpaper backgrounds"
        );
        assert!(
            source.contains("fn gradient_texture") && source.contains("render_gradient(gradient"),
            "a custom gradient must preview through the shared renderer, not a stand-in"
        );
    }

    #[test]
    fn stage_tools_row_shows_the_export_estimate() {
        // The estimate label was orphaned once before when its home was
        // removed (f3f9195); pin that the footer row parents it so a future
        // cleanup cannot silently drop it again.
        let source = include_str!("preview.rs");
        let start = source
            .find("fn build_stage_tools")
            .expect("stage tools builder");
        let rest = &source[start..];
        let end = rest
            .find("\nfn append_stage_aspect_item")
            .expect("stage tools end");
        let body = &rest[..end];
        assert!(
            body.contains("estimate_label: Label"),
            "stage tools must take the estimate label"
        );
        assert!(
            body.contains("bar.append(&estimate_label)"),
            "stage tools must parent the estimate label"
        );
        assert!(
            body.contains("footer::update_estimate(&estimate_label, &state"),
            "stage tools must paint the estimate on first build"
        );
        let window = include_str!("mod.rs");
        assert!(
            window.contains("estimate_label.add_css_class(\"recording-editor-estimate\")"),
            "the estimate label must carry its muted styling"
        );
    }

    #[test]
    fn preview_insets_the_video_rect_so_the_fill_replaces_black_bars() {
        let source = include_str!("preview.rs");
        assert!(
            source.contains("fn apply_preview_clip"),
            "preview must center the video rect instead of fixed margins"
        );
        assert!(
            source.contains("cursor_layer.set_margin_start"),
            "cursor layer must share the video rect so cursor math stays mapped"
        );
        assert!(
            source.contains("s.video_rect_dimensions()"),
            "preview must inset the fitted video rect inside the Frame canvas"
        );
        assert!(
            source.contains("s.output_dimensions()"),
            "stage must carry the export canvas aspect"
        );
        assert!(
            source.contains(
                ".recording-editor-preview-bg { background: @recording-editor-video-surface; }"
            ),
            "no fill must still paint the video-play-area scene behind the video"
        );
    }
}
