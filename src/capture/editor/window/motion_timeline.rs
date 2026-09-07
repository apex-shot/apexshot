//! Motion timeline chrome — same dock/card language as the video editor.

use gtk4::cairo::Context;
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, DrawingArea, Image, Label, Orientation, Overlay,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::i18n::t;
use crate::typography::UI_FONT_FAMILY;

use super::motion_mode::MotionRuntime;

pub(super) struct MotionTimeline {
    pub dock: GtkBox,
    pub play_btn: Button,
    pub skip_back: Button,
    pub skip_forward: Button,
    pub add_btn: Button,
    pub add_text_btn: Button,
    pub playhead_clock: Label,
    pub duration_clock: Label,
    pub ruler: DrawingArea,
    pub source_track: DrawingArea,
    pub track: DrawingArea,
    pub text_track: DrawingArea,
    pub playhead: DrawingArea,
}

pub(super) fn build_motion_timeline(runtime: Rc<RefCell<MotionRuntime>>) -> MotionTimeline {
    let dock = GtkBox::new(Orientation::Vertical, 0);
    dock.add_css_class("recording-editor-timeline-dock");
    dock.set_hexpand(true);
    dock.set_vexpand(false);
    dock.set_valign(Align::End);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("recording-editor-timeline-card");
    card.set_hexpand(true);

    let playhead_clock = Label::new(Some("0:00"));
    playhead_clock.add_css_class("recording-editor-timeline-clock");
    let duration_clock = Label::new(Some("0:03"));
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
    playhead.set_draw_func({
        let runtime = runtime.clone();
        move |_, cr, width, height| draw_playhead(cr, width, height, &runtime)
    });
    board.add_overlay(&playhead);

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
        playhead_clock,
        duration_clock,
        ruler,
        source_track,
        track,
        text_track,
        playhead,
    }
}

fn draw_source_track(cr: &Context, width: i32, height: i32, runtime: &Rc<RefCell<MotionRuntime>>) {
    let runtime = runtime.borrow();
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let inset = 6.0;
    let x = 0.0;
    let y = inset;
    let clip_w = w;
    let clip_h = (h - inset * 2.0).max(1.0);

    rounded_rect(cr, x, y, clip_w, clip_h, 5.0);
    cr.set_source_rgba(0.19, 0.23, 0.30, 0.72);
    let _ = cr.fill();

    let Some(card) = runtime.card.as_ref() else {
        return;
    };
    let _ = cr.save();
    rounded_rect(cr, x, y, clip_w, clip_h, 5.0);
    cr.clip();
    let image_w = card.width().max(1) as f64;
    let image_h = card.height().max(1) as f64;
    // Repeated cover thumbnails make the still readable across the full
    // duration without inventing motion that does not exist in the source.
    let thumbnail_w: f64 = 92.0;
    let mut thumbnail_x = x;
    while thumbnail_x < x + clip_w {
        let tile_w = thumbnail_w.min(x + clip_w - thumbnail_x);
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
        let _ = cr.paint_with_alpha(0.82);
        let _ = cr.restore();
        thumbnail_x += thumbnail_w;
    }
    let _ = cr.restore();
    cr.set_source_rgba(0.64, 0.74, 0.88, 0.72);
    rounded_rect(
        cr,
        x + 0.5,
        y + 0.5,
        (clip_w - 1.0).max(0.0),
        (clip_h - 1.0).max(0.0),
        4.5,
    );
    cr.set_line_width(1.0);
    let _ = cr.stroke();
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

fn draw_motion_track(cr: &Context, width: i32, height: i32, runtime: &Rc<RefCell<MotionRuntime>>) {
    let runtime = runtime.borrow();
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let duration = runtime.motion.duration.max(0.001);
    for (index, segment) in runtime.motion.segments.iter().enumerate() {
        let x0 = time_to_x(segment.start, duration, w);
        let x1 = time_to_x(segment.end, duration, w);
        let clip_w = (x1 - x0).max(22.0);
        let y = 7.0;
        let clip_h = h - 14.0;
        let selected = runtime.motion.selected == Some(index);
        rounded_rect(cr, x0, y, clip_w, clip_h, 5.0);
        cr.set_source_rgba(0.30, 0.48, 0.86, if selected { 0.42 } else { 0.26 });
        let _ = cr.fill();
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
    for (index, segment) in runtime.motion.text_segments.iter().enumerate() {
        let x0 = time_to_x(segment.start, duration, w);
        let x1 = time_to_x(segment.end, duration, w);
        let clip_w = (x1 - x0).max(22.0);
        let y = 6.0;
        let clip_h = h - 12.0;
        let selected = runtime.motion.selected_text == Some(index);
        rounded_rect(cr, x0, y, clip_w, clip_h, 5.0);
        cr.set_source_rgba(0.69, 0.36, 0.22, if selected { 0.50 } else { 0.30 });
        let _ = cr.fill();
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

fn draw_playhead(cr: &Context, width: i32, height: i32, runtime: &Rc<RefCell<MotionRuntime>>) {
    let runtime = runtime.borrow();
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let x = time_to_x(runtime.motion.playhead, runtime.motion.duration, w).floor() + 0.5;
    cr.set_source_rgba(0.86, 0.90, 0.98, 1.0);
    cr.set_line_width(2.0);
    cr.move_to(x, 26.0);
    cr.line_to(x, h);
    let _ = cr.stroke();
    cr.move_to(x - 4.5, 26.0);
    cr.line_to(x + 4.5, 26.0);
    cr.line_to(x, 32.0);
    cr.close_path();
    let _ = cr.fill();
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
