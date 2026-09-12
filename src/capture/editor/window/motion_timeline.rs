//! Motion timeline chrome — same dock/card language as the video editor.

use gtk4::cairo::Context;
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, DrawingArea, Image, Label, Orientation, Overlay,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::i18n::t;
use crate::recording::editor::model::DEFAULT_MOTION_DURATION_SECONDS;
use crate::typography::UI_FONT_FAMILY;

use super::icon_names::custom::{EDIT_UNDO_RTL_SYMBOLIC, EDIT_UNDO_SYMBOLIC};
use super::motion_mode::{MotionHoverTrack, MotionRuntime};

pub(super) struct MotionTimeline {
    pub dock: GtkBox,
    pub play_btn: Button,
    pub skip_back: Button,
    pub skip_forward: Button,
    pub add_btn: Button,
    pub add_text_btn: Button,
    pub undo_btn: Button,
    pub redo_btn: Button,
    pub playhead_clock: Label,
    pub duration_clock: Label,
    pub ruler: DrawingArea,
    pub source_track: DrawingArea,
    pub track: DrawingArea,
    pub text_track: DrawingArea,
    pub playhead: DrawingArea,
    pub playhead_handle: DrawingArea,
    pub hover_playhead: DrawingArea,
    pub playhead_dragging: Rc<Cell<bool>>,
    pub playhead_hovered: Rc<Cell<bool>>,
}

pub(super) fn build_motion_timeline(runtime: Rc<RefCell<MotionRuntime>>) -> MotionTimeline {
    let dock = GtkBox::new(Orientation::Vertical, 0);
    dock.add_css_class("recording-editor-timeline-dock");
    dock.add_css_class("editor-motion-timeline-dock");
    dock.set_hexpand(true);
    dock.set_vexpand(false);
    dock.set_valign(Align::End);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("recording-editor-timeline-card");
    card.set_hexpand(true);

    let playhead_clock = Label::new(Some("0:00"));
    playhead_clock.add_css_class("recording-editor-timeline-clock");
    let duration_clock = Label::new(Some(&format_clock(DEFAULT_MOTION_DURATION_SECONDS)));
    duration_clock.add_css_class("recording-editor-timeline-clock");

    let play_btn = icon_button("media-playback-start-symbolic", &t("Play"));
    play_btn.add_css_class("recording-editor-timeline-play");
    let skip_back = icon_button("media-skip-backward-symbolic", &t("Skip back 1s"));
    let skip_forward = icon_button("media-skip-forward-symbolic", &t("Skip forward 1s"));

    let add_btn = labeled_tool_button(
        "list-add-symbolic",
        &t("Motion"),
        &t("Add motion at playhead"),
    );
    add_btn.add_css_class("recording-editor-timeline-tool-active");
    let add_text_btn =
        labeled_tool_button("list-add-symbolic", &t("Text"), &t("Add text at playhead"));

    let undo_btn = icon_button(EDIT_UNDO_SYMBOLIC, &t("Undo"));
    let redo_btn = icon_button(EDIT_UNDO_RTL_SYMBOLIC, &t("Redo"));
    undo_btn.set_sensitive(false);
    redo_btn.set_sensitive(false);

    let toolbar = GtkBox::new(Orientation::Horizontal, 0);
    toolbar.add_css_class("recording-editor-timeline-toolbar");
    toolbar.set_hexpand(true);

    let left = GtkBox::new(Orientation::Horizontal, 4);
    left.set_halign(Align::Start);
    left.set_hexpand(true);
    left.append(&add_btn);
    left.append(&add_text_btn);

    let center = GtkBox::new(Orientation::Horizontal, 8);
    center.add_css_class("recording-editor-timeline-transport");
    center.set_halign(Align::Center);
    center.set_hexpand(false);
    center.append(&playhead_clock);
    center.append(&skip_back);
    center.append(&play_btn);
    center.append(&skip_forward);
    center.append(&duration_clock);

    let right = GtkBox::new(Orientation::Horizontal, 6);
    right.set_halign(Align::End);
    right.set_hexpand(true);
    right.append(&undo_btn);
    right.append(&redo_btn);

    toolbar.append(&left);
    toolbar.append(&center);
    toolbar.append(&right);

    let ruler = DrawingArea::new();
    ruler.add_css_class("recording-editor-card-ruler");
    ruler.set_hexpand(true);
    ruler.set_size_request(-1, 36);
    ruler.set_draw_func({
        let runtime = runtime.clone();
        move |_, cr, width, height| draw_ruler(cr, width, height, &runtime)
    });

    // The source is a real timeline clip, not an implied backdrop. Shotbase
    // exposes this as ThumbnailTrack/MotionPreviewSegmentView; ApexShot has
    // one static source, so the lane spans the entire Motion composition.
    let source_track = DrawingArea::new();
    source_track.add_css_class("recording-editor-card-zoom-track");
    source_track.set_hexpand(true);
    source_track.set_size_request(-1, 48);
    source_track.set_draw_func({
        let runtime = runtime.clone();
        move |_, cr, width, height| draw_source_track(cr, width, height, &runtime)
    });

    let track = DrawingArea::new();
    track.add_css_class("recording-editor-card-zoom-track");
    track.set_hexpand(true);
    track.set_size_request(-1, 56);
    track.set_draw_func({
        let runtime = runtime.clone();
        move |_, cr, width, height| draw_motion_track(cr, width, height, &runtime)
    });

    let text_track = DrawingArea::new();
    text_track.add_css_class("recording-editor-card-zoom-track");
    text_track.set_hexpand(true);
    text_track.set_size_request(-1, 44);
    text_track.set_draw_func({
        let runtime = runtime.clone();
        move |_, cr, width, height| draw_text_track(cr, width, height, &runtime)
    });

    let tracks = GtkBox::new(Orientation::Vertical, 10);
    tracks.add_css_class("recording-editor-card-tracks");
    tracks.set_hexpand(true);
    tracks.append(&ruler);
    tracks.append(&source_track);
    tracks.append(&track);
    tracks.append(&text_track);

    let board = Overlay::new();
    board.add_css_class("recording-editor-card-board");
    board.set_hexpand(true);
    board.set_child(Some(&tracks));

    let playhead = DrawingArea::new();
    playhead.add_css_class("recording-editor-card-playhead");
    playhead.set_hexpand(true);
    playhead.set_vexpand(true);
    playhead.set_can_target(false);

    // A narrow grabbable strip that follows the drawn playhead line. Clicking
    // the tracks no longer scrubs, so dragging this handle is the way to
    // reposition the playhead. Its allocation is frozen while a drag is in
    // flight: moving it under the pointer would feed the drag offset (which
    // GTK derives from widget-local coordinates) back into the position we
    // compute from it, which reads as lag and rubber-banding.
    let playhead_handle = DrawingArea::new();
    playhead_handle.set_width_request(PLAYHEAD_HANDLE_W as i32);
    playhead_handle.set_height_request(PLAYHEAD_HANDLE_H as i32);
    playhead_handle.set_halign(Align::Start);
    playhead_handle.set_valign(Align::Start);
    playhead_handle.set_margin_top(PLAYHEAD_HANDLE_TOP as i32);
    let playhead_dragging = Rc::new(Cell::new(false));
    let playhead_hovered = Rc::new(Cell::new(false));
    // Pure draw: never touch layout here. Mutating margin/width inside a
    // draw invalidates layout, which re-queues a draw — one layout pass per
    // frame the pointer moves. The handle is positioned from the redraw path
    // (`sync_playhead_handle`) instead, so scrubbing is draw-only.
    playhead.set_draw_func({
        let runtime = runtime.clone();
        let dragging = playhead_dragging.clone();
        let hovered = playhead_hovered.clone();
        move |_, cr, width, height| {
            let expanded = dragging.get() || hovered.get();
            draw_playhead(cr, width, height, &runtime, expanded);
        }
    });
    // Hover read-out: a red hairline under the playhead. It tracks the pointer
    // anywhere over the timeline (no capsule yet); snapping and other behavior
    // will attach to it later.
    let hover_playhead = DrawingArea::new();
    hover_playhead.set_hexpand(true);
    hover_playhead.set_vexpand(true);
    hover_playhead.set_can_target(false);
    hover_playhead.set_draw_func({
        let runtime = runtime.clone();
        move |_, cr, width, height| draw_hover_playhead(cr, width, height, &runtime)
    });
    board.add_overlay(&hover_playhead);
    board.add_overlay(&playhead);
    board.add_overlay(&playhead_handle);

    card.append(&toolbar);
    card.append(&board);
    dock.append(&card);

    MotionTimeline {
        dock,
        play_btn,
        skip_back,
        skip_forward,
        add_btn,
        add_text_btn,
        undo_btn,
        redo_btn,
        playhead_clock,
        duration_clock,
        ruler,
        source_track,
        track,
        text_track,
        playhead,
        playhead_handle,
        hover_playhead,
        playhead_dragging,
        playhead_hovered,
    }
}

fn draw_source_track(cr: &Context, width: i32, height: i32, runtime: &Rc<RefCell<MotionRuntime>>) {
    let runtime = runtime.borrow();
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let inset = 6.0;
    let y = inset;
    let clip_h = (h - inset * 2.0).max(1.0);

    rounded_rect(cr, 0.0, y, w, clip_h, 5.0);
    cr.set_source_rgba(0.10, 0.11, 0.13, 0.9);
    let _ = cr.fill();

    if let Some(card) = runtime.card.as_ref() {
        let _ = cr.save();
        rounded_rect(cr, 0.0, y, w, clip_h, 5.0);
        cr.clip();
        let image_w = card.width().max(1) as f64;
        let image_h = card.height().max(1) as f64;
        // Repeated cover thumbnails make the still readable across the full
        // duration without inventing motion that does not exist in the source.
        let thumbnail_w: f64 = 92.0;
        let mut thumbnail_x = 0.0;
        while thumbnail_x < w {
            let tile_w = thumbnail_w.min(w - thumbnail_x);
            let scale = (tile_w / image_w).max(clip_h / image_h);
            let painted_w = image_w * scale;
            let painted_h = image_h * scale;
            let _ = cr.save();
            cr.rectangle(thumbnail_x, y, tile_w, clip_h);
            cr.clip();
            cr.translate(
                thumbnail_x + (tile_w - painted_w) / 2.0,
                y + (clip_h - painted_h) / 2.0,
            );
            cr.scale(scale, scale);
            cr.set_source_surface(card, 0.0, 0.0).ok();
            let _ = cr.paint();
            let _ = cr.restore();
            thumbnail_x += thumbnail_w;
        }
        let _ = cr.restore();
    }

    draw_source_chip(cr, y, clip_h, runtime.motion.duration);

    if runtime.source_selected {
        rounded_rect(
            cr,
            0.5,
            y + 0.5,
            (w - 1.0).max(0.0),
            (clip_h - 1.0).max(0.0),
            4.5,
        );
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.45);
        cr.set_line_width(1.0);
        let _ = cr.stroke();
    }
}

/// The source lane's identity badge: icon, "Image", and the clip length.
fn draw_source_chip(cr: &Context, y: f64, clip_h: f64, duration: f64) {
    let title = t("Image");
    cr.select_font_face(
        UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    cr.set_font_size(11.0);
    let title_w = cr
        .text_extents(&title)
        .map(|ext| ext.width())
        .unwrap_or(0.0);
    let duration_label = format!("{}s", duration.round().max(0.0) as i64);
    cr.select_font_face(
        UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(10.0);
    let duration_w = cr
        .text_extents(&duration_label)
        .map(|ext| ext.width())
        .unwrap_or(0.0);

    let chip_h = 34.0;
    let chip_x = 8.0;
    let chip_y = y + (clip_h - chip_h) / 2.0;
    let icon_size = 20.0;
    let chip_w = 8.0 + icon_size + 7.0 + title_w.max(duration_w) + 10.0;
    rounded_rect(cr, chip_x, chip_y, chip_w, chip_h, 8.0);
    cr.set_source_rgba(0.06, 0.06, 0.08, 0.92);
    let _ = cr.fill();

    let icon_x = chip_x + 8.0;
    let icon_y = chip_y + (chip_h - icon_size) / 2.0;
    rounded_rect(cr, icon_x, icon_y, icon_size, icon_size, 5.0);
    cr.set_source_rgba(0.95, 0.96, 0.98, 1.0);
    let _ = cr.fill();
    let _ = cr.save();
    rounded_rect(cr, icon_x, icon_y, icon_size, icon_size, 5.0);
    cr.clip();
    cr.set_source_rgba(0.10, 0.10, 0.12, 1.0);
    cr.arc(icon_x + 6.5, icon_y + 6.5, 2.2, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
    cr.move_to(icon_x + 2.0, icon_y + icon_size - 3.0);
    cr.line_to(icon_x + 8.0, icon_y + 9.0);
    cr.line_to(icon_x + 12.5, icon_y + 14.5);
    cr.line_to(icon_x + icon_size - 1.0, icon_y + 7.5);
    cr.line_to(icon_x + icon_size - 1.0, icon_y + icon_size - 1.0);
    cr.line_to(icon_x + 2.0, icon_y + icon_size - 1.0);
    cr.close_path();
    let _ = cr.fill();
    let _ = cr.restore();

    let text_x = icon_x + icon_size + 7.0;
    cr.set_source_rgba(0.96, 0.97, 1.0, 0.96);
    cr.select_font_face(
        UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    cr.set_font_size(11.0);
    cr.move_to(text_x, chip_y + 14.0);
    let _ = cr.show_text(&title);
    cr.set_source_rgba(0.86, 0.88, 0.92, 0.72);
    cr.select_font_face(
        UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(10.0);
    cr.move_to(text_x, chip_y + 26.0);
    let _ = cr.show_text(&duration_label);
}

fn icon_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-timeline-icon");
    button.set_tooltip_text(Some(tooltip));
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    button.set_child(Some(&icon));
    button.set_valign(Align::Center);
    button
}

fn labeled_tool_button(icon_name: &str, label: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-timeline-tool");
    button.set_tooltip_text(Some(tooltip));
    let row = GtkBox::new(Orientation::Horizontal, 6);
    let text = Label::new(Some(label));
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(18);
    row.append(&icon);
    row.append(&text);
    button.set_child(Some(&row));
    button
}

fn time_to_x(time: f64, duration: f64, width: f64) -> f64 {
    (time / duration.max(0.001)).clamp(0.0, 1.0) * width
}

fn draw_ruler(cr: &Context, width: i32, height: i32, runtime: &Rc<RefCell<MotionRuntime>>) {
    let runtime = runtime.borrow();
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let duration = runtime.motion.duration.max(0.001);
    let playhead = runtime.motion.playhead;
    let major = ruler_major_step(duration);
    let minor = (major / 5.0).max(0.05);

    cr.select_font_face(
        UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(10.0);
    cr.set_line_width(1.0);

    let mut t = 0.0;
    while t <= duration + 0.0001 {
        let x = time_to_x(t, duration, w).floor() + 0.5;
        let on_major = ((t / major) - (t / major).round()).abs() < 0.02;
        cr.set_source_rgba(1.0, 1.0, 1.0, if on_major { 0.28 } else { 0.10 });
        cr.move_to(x, if on_major { h - 11.0 } else { h - 5.0 });
        cr.line_to(x, h);
        let _ = cr.stroke();
        if on_major {
            let label = format_ruler_label(t, major);
            let near_playhead = (t - playhead).abs() < major * 0.08;
            cr.set_source_rgba(1.0, 1.0, 1.0, if near_playhead { 0.92 } else { 0.52 });
            if let Ok(ext) = cr.text_extents(&label) {
                let label_x = if t <= 0.001 {
                    0.0
                } else {
                    (x - ext.width() / 2.0).clamp(0.0, w - ext.width())
                };
                cr.move_to(label_x, 12.0);
                let _ = cr.show_text(&label);
            }
        }
        t += minor;
    }
}

fn ruler_major_step(visible: f64) -> f64 {
    const STEPS: [f64; 8] = [0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0];
    *STEPS
        .iter()
        .find(|step| visible / *step <= 10.0)
        .unwrap_or(&STEPS[STEPS.len() - 1])
}

fn format_ruler_label(seconds: f64, major: f64) -> String {
    let total = seconds.max(0.0);
    let minutes = (total / 60.0).floor() as u64;
    let secs = total - minutes as f64 * 60.0;
    if major < 1.0 {
        format!("{minutes}:{secs:04.1}")
    } else {
        format!("{minutes}:{:02}", secs.floor() as u64)
    }
}

/// Empty-lane affordance: a ghost of the clip the next click will create,
/// starting at the red hover hairline and running for the default clip length.
/// It names the clip kind so the row explains itself.
fn draw_add_track(
    cr: &Context,
    w: f64,
    h: f64,
    duration: f64,
    start: f64,
    end: f64,
    label: &str,
    tint: (f64, f64, f64),
    edge: (f64, f64, f64),
) {
    // The span comes from the model, so the ghost begins exactly at the hover
    // line and is only shown when the click would really create that clip.
    let x0 = time_to_x(start, duration, w);
    let x1 = time_to_x(end, duration, w);
    let clip_w = (x1 - x0).max(22.0);
    let y = 7.0;
    let clip_h = (h - 14.0).max(1.0);
    rounded_rect(cr, x0, y, clip_w, clip_h, 5.0);
    cr.set_source_rgba(tint.0, tint.1, tint.2, 0.18);
    let _ = cr.fill_preserve();
    cr.set_source_rgba(edge.0, edge.1, edge.2, 0.45);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    cr.select_font_face(
        UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(11.0);
    let label_ext = cr.text_extents(label).ok();
    let label_w = label_ext.map(|ext| ext.width()).unwrap_or(0.0);
    let plus_r = 5.0;
    let content_w = plus_r * 2.0 + 7.0 + label_w;
    let cy = y + clip_h / 2.0;

    cr.set_source_rgba(0.94, 0.96, 1.0, 0.9);
    cr.set_line_width(1.6);
    let cx = if content_w + 16.0 <= clip_w {
        x0 + (clip_w - content_w) / 2.0 + plus_r
    } else {
        x0 + clip_w / 2.0
    };
    cr.move_to(cx - plus_r, cy);
    cr.line_to(cx + plus_r, cy);
    cr.move_to(cx, cy - plus_r);
    cr.line_to(cx, cy + plus_r);
    let _ = cr.stroke();

    if content_w + 16.0 <= clip_w {
        if let Some(ext) = label_ext {
            cr.move_to(
                x0 + (clip_w - content_w) / 2.0 + plus_r * 2.0 + 7.0,
                cy - ext.y_bearing() - ext.height() / 2.0,
            );
            let _ = cr.show_text(label);
        }
    }
}

fn draw_motion_track(cr: &Context, width: i32, height: i32, runtime: &Rc<RefCell<MotionRuntime>>) {
    let runtime = runtime.borrow();
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let duration = runtime.motion.duration.max(0.001);
    if runtime.hover_track == Some(MotionHoverTrack::Motion) {
        if let Some((start, end)) = runtime
            .hover_time
            .and_then(|hover| runtime.motion.motion_add_span(hover))
        {
            draw_add_track(
                cr,
                w,
                h,
                duration,
                start,
                end,
                &t("Motion"),
                (0.23, 0.38, 0.62),
                (0.72, 0.84, 1.0),
            );
        }
    }
    for (index, segment) in runtime.motion.segments.iter().enumerate() {
        let x0 = time_to_x(segment.start, duration, w);
        let x1 = time_to_x(segment.end, duration, w);
        let clip_w = (x1 - x0).max(22.0);
        let y = 7.0;
        let clip_h = h - 14.0;
        let selected = runtime.motion.selected == Some(index);
        let (fill_r, fill_g, fill_b) = if selected {
            (0.28, 0.46, 0.74)
        } else {
            (0.23, 0.38, 0.62)
        };
        rounded_rect(cr, x0, y, clip_w, clip_h, 5.0);
        cr.set_source_rgba(fill_r, fill_g, fill_b, 1.0);
        let _ = cr.fill();
        if selected {
            rounded_rect(
                cr,
                x0 + 0.5,
                y + 0.5,
                (clip_w - 1.0).max(0.0),
                (clip_h - 1.0).max(0.0),
                4.5,
            );
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.45);
            cr.set_line_width(1.0);
            let _ = cr.stroke();
        }
        cr.set_source_rgba(0.72, 0.84, 1.0, 0.98);
        rounded_rect(
            cr,
            x0 + 6.0,
            y + (clip_h - (clip_h - 12.0).max(10.0)) / 2.0,
            3.0,
            (clip_h - 12.0).max(10.0),
            2.0,
        );
        let _ = cr.fill();
        rounded_rect(
            cr,
            x0 + clip_w - 9.0,
            y + (clip_h - (clip_h - 12.0).max(10.0)) / 2.0,
            3.0,
            (clip_h - 12.0).max(10.0),
            2.0,
        );
        let _ = cr.fill();
        if clip_w > 40.0 {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.82);
            cr.select_font_face(
                UI_FONT_FAMILY,
                gtk4::cairo::FontSlant::Normal,
                gtk4::cairo::FontWeight::Normal,
            );
            cr.set_font_size(11.0);
            cr.move_to(x0 + 14.0, y + clip_h * 0.62);
            let _ = cr.show_text(&t("Motion"));
        }
    }
}

fn draw_text_track(cr: &Context, width: i32, height: i32, runtime: &Rc<RefCell<MotionRuntime>>) {
    let runtime = runtime.borrow();
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let duration = runtime.motion.duration.max(0.001);
    if runtime.hover_track == Some(MotionHoverTrack::Text) {
        if let Some((start, end)) = runtime
            .hover_time
            .and_then(|hover| runtime.motion.text_add_span(hover))
        {
            draw_add_track(
                cr,
                w,
                h,
                duration,
                start,
                end,
                &t("Text"),
                (0.62, 0.32, 0.18),
                (0.98, 0.78, 0.62),
            );
        }
    }
    for (index, segment) in runtime.motion.text_segments.iter().enumerate() {
        let x0 = time_to_x(segment.start, duration, w);
        let x1 = time_to_x(segment.end, duration, w);
        let clip_w = (x1 - x0).max(22.0);
        let y = 6.0;
        let clip_h = h - 12.0;
        let selected = runtime.motion.selected_text == Some(index);
        let (fill_r, fill_g, fill_b) = if selected {
            (0.72, 0.39, 0.22)
        } else {
            (0.62, 0.32, 0.18)
        };
        rounded_rect(cr, x0, y, clip_w, clip_h, 5.0);
        cr.set_source_rgba(fill_r, fill_g, fill_b, 1.0);
        let _ = cr.fill();
        if selected {
            rounded_rect(
                cr,
                x0 + 0.5,
                y + 0.5,
                (clip_w - 1.0).max(0.0),
                (clip_h - 1.0).max(0.0),
                4.5,
            );
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.45);
            cr.set_line_width(1.0);
            let _ = cr.stroke();
        }
        cr.set_source_rgba(0.98, 0.78, 0.62, 0.98);
        rounded_rect(
            cr,
            x0 + 6.0,
            y + (clip_h - (clip_h - 10.0).max(8.0)) / 2.0,
            3.0,
            (clip_h - 10.0).max(8.0),
            2.0,
        );
        let _ = cr.fill();
        rounded_rect(
            cr,
            x0 + clip_w - 9.0,
            y + (clip_h - (clip_h - 10.0).max(8.0)) / 2.0,
            3.0,
            (clip_h - 10.0).max(8.0),
            2.0,
        );
        let _ = cr.fill();
        if clip_w > 36.0 {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.86);
            cr.select_font_face(
                UI_FONT_FAMILY,
                gtk4::cairo::FontSlant::Normal,
                gtk4::cairo::FontWeight::Normal,
            );
            cr.set_font_size(11.0);
            cr.move_to(x0 + 14.0, y + clip_h * 0.64);
            let label = segment.text.trim();
            let label = if label.is_empty() {
                t("Text")
            } else {
                label.to_string()
            };
            let _ = cr.show_text(&label);
        }
    }
}

const PLAYHEAD_HANDLE_W: f64 = 12.0;
const PLAYHEAD_HANDLE_H: f64 = 26.0;
const PLAYHEAD_HANDLE_TOP: f64 = 2.0;
// Hovered/dragged handle: a wide pill that shows the playhead clock, because
// the thumb covers the ruler ticks users would otherwise read.
const PLAYHEAD_CLOCK_W: f64 = 58.0;
const PLAYHEAD_HOVER_SLOP: f64 = 6.0;

/// Hit-test for the playhead *head* (the capsule) only. The stem below it must
/// not expand the capsule; and once expanded the pointer may roam the wide
/// pill without it collapsing.
pub(in crate::capture::editor::window) fn playhead_head_hit(
    pointer_x: f64,
    pointer_y: f64,
    line_x: f64,
    expanded: bool,
) -> bool {
    let half_w = if expanded {
        PLAYHEAD_CLOCK_W / 2.0
    } else {
        PLAYHEAD_HANDLE_W / 2.0
    };
    pointer_y <= PLAYHEAD_HANDLE_TOP + PLAYHEAD_HANDLE_H + PLAYHEAD_HOVER_SLOP
        && (pointer_x - line_x).abs() <= half_w + PLAYHEAD_HOVER_SLOP
}

/// Position the grab handle from model state, outside any draw callback.
/// The drawn capsule may clip at the board edge so its stem stays centered;
/// the widget itself clamps into layout so it never gets a negative margin.
pub(in crate::capture::editor::window) fn sync_playhead_handle(
    handle: &DrawingArea,
    runtime: &Rc<RefCell<MotionRuntime>>,
    board_width: f64,
    expanded: bool,
) {
    let (playhead, duration) = {
        let runtime = runtime.borrow();
        (runtime.motion.playhead, runtime.motion.duration.max(0.001))
    };
    let pill_w = if expanded {
        PLAYHEAD_CLOCK_W
    } else {
        PLAYHEAD_HANDLE_W
    };
    let x = time_to_x(playhead, duration, board_width.max(1.0));
    let margin = (x - pill_w / 2.0).max(0.0) as i32;
    // Width/margin writes each invalidate layout, so skip no-ops: during a
    // scrub this runs per pointer event and must stay allocation-free.
    if handle.width_request() != pill_w as i32 {
        handle.set_width_request(pill_w as i32);
    }
    if handle.margin_start() != margin {
        handle.set_margin_start(margin);
    }
}

/// Pointer read-out line. Unlike the playhead it has no capsule, so it can be
/// drawn under everything and updated on every motion event cheaply.
fn draw_hover_playhead(
    cr: &Context,
    width: i32,
    height: i32,
    runtime: &Rc<RefCell<MotionRuntime>>,
) {
    let runtime = runtime.borrow();
    let Some(time) = runtime.hover_time else {
        return;
    };
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let x = time_to_x(time, runtime.motion.duration, w).floor() + 0.5;
    cr.set_source_rgba(0.80, 0.22, 0.20, 0.95);
    cr.set_line_width(2.0);
    cr.move_to(x, 0.0);
    cr.line_to(x, h);
    let _ = cr.stroke();
}

fn draw_playhead(
    cr: &Context,
    width: i32,
    height: i32,
    runtime: &Rc<RefCell<MotionRuntime>>,
    expanded: bool,
) -> (f64, f64) {
    let runtime = runtime.borrow();
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let x = time_to_x(runtime.motion.playhead, runtime.motion.duration, w).floor() + 0.5;

    // Stem: thin light line running from the handle to the bottom of the board.
    cr.set_source_rgba(0.86, 0.90, 0.98, 0.9);
    cr.set_line_width(1.5);
    cr.move_to(x, PLAYHEAD_HANDLE_TOP + PLAYHEAD_HANDLE_H);
    cr.line_to(x, h);
    let _ = cr.stroke();

    let (pill_w, pill_h) = if expanded {
        (PLAYHEAD_CLOCK_W, PLAYHEAD_HANDLE_H)
    } else {
        (PLAYHEAD_HANDLE_W, PLAYHEAD_HANDLE_H)
    };
    // The stem must pierce the capsule's center even on the first/last frame.
    // Clamping the capsule into the board pushed it off the stem, so let it
    // clip at the edge instead, like the video editor's playhead mark.
    let hx = x - pill_w / 2.0;

    // Handle: dark capsule with a light outline; expanded it carries the clock.
    rounded_rect(cr, hx, PLAYHEAD_HANDLE_TOP, pill_w, pill_h, pill_h / 2.0);
    cr.set_source_rgba(0.04, 0.05, 0.07, 1.0);
    let _ = cr.fill_preserve();
    cr.set_source_rgba(0.86, 0.90, 0.98, 1.0);
    cr.set_line_width(1.5);
    let _ = cr.stroke();

    if expanded {
        cr.select_font_face(
            UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        cr.set_font_size(13.0);
        let label = format_clock(runtime.motion.playhead);
        if let Ok(ext) = cr.text_extents(&label) {
            cr.set_source_rgba(0.94, 0.96, 1.0, 1.0);
            cr.move_to(
                hx + (pill_w - ext.width()) / 2.0 - ext.x_bearing(),
                PLAYHEAD_HANDLE_TOP + pill_h / 2.0 - ext.y_bearing() - ext.height() / 2.0,
            );
            let _ = cr.show_text(&label);
        }
    }

    (hx, pill_w)
}

fn rounded_rect(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_path();
    cr.move_to(x + r, y);
    cr.line_to(x + w - r, y);
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.line_to(x + w, y + h - r);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.line_to(x + r, y + h);
    cr.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.line_to(x, y + r);
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
}

pub(super) fn format_clock(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as i64;
    format!("{}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod tests {
    #[test]
    fn motion_timeline_reuses_video_editor_dock_classes() {
        let source = include_str!("motion_timeline.rs");
        assert!(
            source.contains("recording-editor-timeline-dock")
                && source.contains("recording-editor-timeline-toolbar")
                && source.contains("recording-editor-timeline-transport")
                && source.contains("recording-editor-card-ruler")
                && source.contains("recording-editor-card-zoom-track")
                && source.contains("recording-editor-card-playhead"),
            "Motion timeline must use the video editor card chrome, not a flat orange strip"
        );
    }
}
