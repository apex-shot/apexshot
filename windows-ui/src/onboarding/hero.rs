//! The drawn product preview beside the account and recording pages.
//!
//! Everything here is Cairo so the mock keeps its proportions at every DPI
//! scale: a warm studio backdrop, a floating ApexShot window, a selection
//! frame, the Quick Access card, and (for the recording page) a camera bubble,
//! a voice meter, and a timeline. The scene is data, so choosing "screen only"
//! on the recording page changes the picture in place.

use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;
use std::time::Duration;

use gtk4::cairo::{self, Context};
use gtk4::prelude::*;
use gtk4::DrawingArea;
use pango::{self, Weight};
use pangocairo::functions::{create_layout, show_layout};

use crate::config::AppConfig;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum HeroScene {
    #[default]
    CameraAndVoice,
    ScreenAndVoice,
    ScreenOnly,
}

impl HeroScene {
    pub(crate) fn for_config(config: &AppConfig) -> Self {
        if config.rec_webcam_enabled {
            Self::CameraAndVoice
        } else if config.rec_mic {
            Self::ScreenAndVoice
        } else {
            Self::ScreenOnly
        }
    }

    pub(crate) fn camera(self) -> bool {
        matches!(self, Self::CameraAndVoice)
    }

    pub(crate) fn microphone(self) -> bool {
        matches!(self, Self::CameraAndVoice | Self::ScreenAndVoice)
    }
}

/// Build the animated preview. The shared scene can be swapped later without
/// rebuilding the drawing area.
pub(crate) fn build(scene: Rc<RefCell<HeroScene>>) -> DrawingArea {
    let area = DrawingArea::new();
    area.add_css_class("ob-hero");
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_content_width(560);
    area.set_content_height(660);

    area.set_draw_func(move |_, context, width, height| {
        draw_hero(
            context,
            f64::from(width),
            f64::from(height),
            *scene.borrow(),
        );
    });

    // A slow ambient pulse on the record dot and voice meter is what makes the
    // panel read as a live product instead of a still illustration.
    let weak = area.downgrade();
    gtk4::glib::timeout_add_local(Duration::from_millis(70), move || {
        let Some(area) = weak.upgrade() else {
            return gtk4::glib::ControlFlow::Break;
        };
        if area.is_mapped() {
            area.queue_draw();
        }
        gtk4::glib::ControlFlow::Continue
    });

    area
}

/// A small window mock for one of the appearance choices.
pub(crate) fn theme_preview(kind: &'static str) -> DrawingArea {
    let area = DrawingArea::new();
    area.add_css_class("ob-theme-preview");
    area.set_content_width(168);
    area.set_content_height(104);
    area.set_size_request(168, 104);
    area.set_draw_func(move |_, context, width, height| {
        draw_theme_preview(context, f64::from(width), f64::from(height), kind);
    });
    area
}

fn draw_hero(cr: &Context, width: f64, height: f64, scene: HeroScene) {
    if width < 2.0 || height < 2.0 {
        return;
    }
    let s = (width / 560.0).clamp(0.55, 2.4);

    cr.save().ok();
    rounded_rect(cr, 0.0, 0.0, width, height, 12.0 * s);
    cr.clip();
    draw_backdrop(cr, width, height);
    draw_clouds(cr, width, height);
    draw_grain(cr, width, height);

    draw_recording_window(cr, width, height, s, scene);

    // Inner hairline so the panel reads as a separate surface at every size.
    rounded_rect(cr, 0.5, 0.5, width - 1.0, height - 1.0, 12.0 * s);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.14);
    cr.set_line_width(1.0);
    cr.stroke().ok();
    cr.restore().ok();
}

fn draw_backdrop(cr: &Context, width: f64, height: f64) {
    let gradient = cairo::LinearGradient::new(0.0, 0.0, width * 0.35, height);
    gradient.add_color_stop_rgb(0.0, 1.0, 0.87, 0.74);
    gradient.add_color_stop_rgb(0.34, 0.99, 0.64, 0.46);
    gradient.add_color_stop_rgb(0.66, 0.86, 0.38, 0.36);
    gradient.add_color_stop_rgb(1.0, 0.33, 0.12, 0.23);
    cr.set_source(&gradient).ok();
    cr.paint().ok();

    let sun = cairo::RadialGradient::new(
        width * 0.76,
        height * 0.12,
        0.0,
        width * 0.76,
        height * 0.12,
        width * 0.62,
    );
    sun.add_color_stop_rgba(0.0, 1.0, 0.97, 0.88, 0.62);
    sun.add_color_stop_rgba(0.38, 1.0, 0.86, 0.68, 0.22);
    sun.add_color_stop_rgba(1.0, 1.0, 0.86, 0.68, 0.0);
    cr.set_source(&sun).ok();
    cr.paint().ok();

    let floor = cairo::LinearGradient::new(0.0, height * 0.55, 0.0, height);
    floor.add_color_stop_rgba(0.0, 0.24, 0.08, 0.16, 0.0);
    floor.add_color_stop_rgba(1.0, 0.17, 0.05, 0.12, 0.34);
    cr.set_source(&floor).ok();
    cr.paint().ok();
}

fn draw_clouds(cr: &Context, width: f64, height: f64) {
    let wisps = [
        (0.16, 0.20, 0.26, 0.050),
        (0.30, 0.12, 0.20, 0.045),
        (0.62, 0.28, 0.30, 0.035),
        (0.10, 0.46, 0.22, 0.030),
        (0.82, 0.42, 0.24, 0.032),
    ];
    for (x, y, w, alpha) in wisps {
        cr.save().ok();
        cr.translate(width * x, height * y);
        cr.scale(1.0, 0.38);
        cr.arc(0.0, 0.0, width * w, 0.0, 2.0 * PI);
        cr.set_source_rgba(1.0, 0.98, 0.95, alpha);
        cr.fill().ok();
        cr.restore().ok();
    }
}

fn draw_grain(cr: &Context, width: f64, height: f64) {
    // Deterministic 32-bit LCG: the same quiet texture on every repaint, with
    // no allocation and no visible pattern.
    let mut seed: u32 = 0x9E37_79B9;
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.020);
    for _ in 0..220 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let x = f64::from((seed >> 8) & 0xFFFF) / 65535.0 * width;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let y = f64::from((seed >> 8) & 0xFFFF) / 65535.0 * height;
        cr.arc(x, y, 0.7, 0.0, 2.0 * PI);
        cr.fill().ok();
    }
}

fn draw_recording_window(cr: &Context, width: f64, height: f64, s: f64, scene: HeroScene) {
    let x = width * 0.105;
    let y = height * 0.155;
    let w = width * 0.79;
    let h = height * 0.665;
    let radius = 18.0 * s;

    // Floating shadow: three offset rounded rectangles stand in for a blur.
    for (offset, alpha) in [(14.0, 0.06), (9.0, 0.08), (4.0, 0.10)] {
        rounded_rect(cr, x, y + offset * s, w, h, radius);
        cr.set_source_rgba(0.09, 0.03, 0.02, alpha);
        cr.fill().ok();
    }

    rounded_rect(cr, x, y, w, h, radius);
    cr.set_source_rgba(0.085, 0.065, 0.058, 0.94);
    cr.fill().ok();
    rounded_rect(cr, x + 0.5, y + 0.5, w - 1.0, h - 1.0, radius);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.13);
    cr.set_line_width(1.0);
    cr.stroke().ok();

    let title_h = h * 0.072;
    let inset = w * 0.022;
    let content_x = x + inset;
    let content_y = y + title_h + inset * 0.4;
    let content_w = w - inset * 2.0;
    let content_h = h - title_h - inset * 1.4;

    draw_window_chrome(cr, x, y, w, title_h, s);
    draw_preview(cr, content_x, content_y, content_w, content_h, s, scene);
    draw_recording_pill(
        cr,
        x + w * 0.36,
        y + title_h + h * 0.028,
        w * 0.28,
        h * 0.062,
        s,
    );
    draw_quick_access(cr, x - w * 0.045, y + h * 0.66, w * 0.47, h * 0.245, s);

    if scene.camera() {
        draw_camera_bubble(cr, x + w * 0.80, y + h * 0.70, width * 0.105, s);
    }
    if scene.microphone() {
        draw_voice_meter(cr, x + w * 0.025, y + h * 0.905, w * 0.25, h * 0.060, s);
    }
}

fn draw_window_chrome(cr: &Context, x: f64, y: f64, w: f64, title_h: f64, s: f64) {
    cr.save().ok();
    rounded_rect(cr, x, y, w, title_h * 1.9, 18.0 * s);
    cr.clip();
    cr.rectangle(x, y, w, title_h);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.045);
    cr.fill().ok();
    cr.restore().ok();

    text(
        cr,
        "ApexShot",
        x + 16.0 * s,
        y + title_h / 2.0,
        9.2 * s,
        Weight::Semibold,
        (1.0, 1.0, 1.0, 0.52),
        0.0,
    );

    let glyph_y = y + title_h / 2.0;
    let right = x + w - 16.0 * s;
    let line = 5.0 * s;
    cr.set_line_width(1.1 * s);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.42);

    // Windows order, left to right: minimise, maximise, close.
    let minimize_x = right - 42.0 * s;
    cr.move_to(minimize_x - line / 2.0, glyph_y);
    cr.line_to(minimize_x + line / 2.0, glyph_y);
    cr.stroke().ok();
    cr.rectangle(right - 23.0 * s, glyph_y - 2.5 * s, 5.0 * s, 5.0 * s);
    cr.stroke().ok();
    cr.move_to(right - 2.5 * s, glyph_y - 2.5 * s);
    cr.line_to(right + 2.5 * s, glyph_y + 2.5 * s);
    cr.move_to(right + 2.5 * s, glyph_y - 2.5 * s);
    cr.line_to(right - 2.5 * s, glyph_y + 2.5 * s);
    cr.stroke().ok();
}

fn draw_preview(cr: &Context, x: f64, y: f64, w: f64, h: f64, s: f64, scene: HeroScene) {
    let radius = 10.0 * s;
    let canvas_h = h * 0.70;

    // Canvas plate. The clip belongs to this block only: the timeline beneath
    // it and everything drawn after the preview must stay unclipped.
    cr.save().ok();
    rounded_rect(cr, x, y, w, canvas_h, radius);
    cr.clip_preserve();
    let canvas = cairo::LinearGradient::new(x, y, x + w, y + canvas_h);
    canvas.add_color_stop_rgb(0.0, 0.16, 0.14, 0.20);
    canvas.add_color_stop_rgb(1.0, 0.23, 0.14, 0.17);
    cr.set_source(&canvas).ok();
    cr.fill_preserve().ok();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.07);
    cr.set_line_width(1.0);
    cr.stroke().ok();
    cr.restore().ok();

    // The captured picture inside the app frame: a warm horizon, a sun, and
    // two soft hills. It gives the selection something real to sit on.
    let px = x + w * 0.055;
    let py = y + canvas_h * 0.10;
    let pw = w * 0.89;
    let ph = canvas_h * 0.80;
    let picture = cairo::LinearGradient::new(px, py, px, py + ph);
    picture.add_color_stop_rgb(0.0, 0.99, 0.76, 0.55);
    picture.add_color_stop_rgb(0.45, 0.93, 0.52, 0.42);
    picture.add_color_stop_rgb(1.0, 0.37, 0.17, 0.28);
    cr.save().ok();
    rounded_rect(cr, px, py, pw, ph, 8.0 * s);
    cr.clip_preserve();
    cr.set_source(&picture).ok();
    cr.fill().ok();
    cr.arc(px + pw * 0.72, py + ph * 0.32, pw * 0.085, 0.0, 2.0 * PI);
    cr.set_source_rgba(1.0, 0.93, 0.78, 0.85);
    cr.fill().ok();
    cr.save().ok();
    cr.translate(px + pw * 0.26, py + ph * 0.98);
    cr.scale(1.0, 0.30);
    cr.arc(0.0, 0.0, pw * 0.34, PI, 2.0 * PI);
    cr.set_source_rgba(0.22, 0.10, 0.18, 0.55);
    cr.fill().ok();
    cr.restore().ok();
    cr.save().ok();
    cr.translate(px + pw * 0.80, py + ph * 1.00);
    cr.scale(1.0, 0.26);
    cr.arc(0.0, 0.0, pw * 0.30, PI, 2.0 * PI);
    cr.set_source_rgba(0.16, 0.07, 0.13, 0.60);
    cr.fill().ok();
    cr.restore().ok();
    cr.restore().ok();

    draw_selection(
        cr,
        px + pw * 0.11,
        py + ph * 0.20,
        pw * 0.58,
        ph * 0.46,
        s,
        if scene == HeroScene::ScreenOnly {
            "1920 × 1080"
        } else {
            "1480 × 920"
        },
    );

    // Editor chrome below the picture: three tracks with clips.
    draw_timeline(
        cr,
        x,
        y + canvas_h + 12.0 * s,
        w,
        h - canvas_h - 12.0 * s,
        s,
        scene,
    );
}

fn draw_selection(cr: &Context, x: f64, y: f64, w: f64, h: f64, s: f64, caption: &str) {
    cr.set_source_rgba(1.0, 0.48, 0.20, 0.95);
    cr.set_line_width(2.0 * s);
    cr.rectangle(x, y, w, h);
    cr.stroke().ok();

    let handle = 3.2 * s;
    for (hx, hy) in [
        (x, y),
        (x + w / 2.0, y),
        (x + w, y),
        (x, y + h / 2.0),
        (x + w, y + h / 2.0),
        (x, y + h),
        (x + w / 2.0, y + h),
        (x + w, y + h),
    ] {
        cr.rectangle(hx - handle, hy - handle, handle * 2.0, handle * 2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.96);
        cr.fill().ok();
        cr.set_source_rgba(0.10, 0.06, 0.05, 0.30);
        cr.set_line_width(1.0);
        cr.stroke().ok();
    }

    let chip_w = 76.0 * s;
    let chip_h = 20.0 * s;
    let chip_x = x + w / 2.0 - chip_w / 2.0;
    let chip_y = y - chip_h - 7.0 * s;
    rounded_rect(cr, chip_x, chip_y, chip_w, chip_h, 9.0 * s);
    cr.set_source_rgba(0.10, 0.07, 0.06, 0.92);
    cr.fill().ok();
    text(
        cr,
        caption,
        chip_x + chip_w / 2.0,
        chip_y + chip_h / 2.0,
        9.0 * s,
        Weight::Medium,
        (1.0, 1.0, 1.0, 0.92),
        0.5,
    );
}

fn draw_timeline(cr: &Context, x: f64, y: f64, w: f64, h: f64, s: f64, scene: HeroScene) {
    if h < 8.0 {
        return;
    }
    let track_h = (h * 0.20).clamp(2.0, 7.0 * s);
    let gap = (h * 0.12).clamp(1.0, 4.0 * s);
    let colors = [
        (1.0, 0.48, 0.20, 0.85),
        (0.96, 0.88, 0.78, 0.42),
        (0.40, 0.72, 0.68, 0.65),
    ];
    let mut cursor = y + gap;
    for (index, color) in colors.iter().enumerate() {
        rounded_rect(cr, x + w * 0.02, cursor, w * 0.96, track_h, track_h / 2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.055);
        cr.fill().ok();
        let clip_x = x + w * (0.04 + index as f64 * 0.05);
        let clip_w = w * (0.30 - index as f64 * 0.04);
        rounded_rect(cr, clip_x, cursor, clip_w, track_h, track_h / 2.0);
        cr.set_source_rgba(color.0, color.1, color.2, color.3);
        cr.fill().ok();
        cursor += track_h + gap;
    }

    // Playhead.
    let play_x = x + w * 0.50;
    cr.set_source_rgba(1.0, 0.45, 0.20, 0.92);
    cr.set_line_width(1.2 * s);
    cr.move_to(play_x, y + 1.0);
    cr.line_to(play_x, y + h - 1.0);
    cr.stroke().ok();

    if scene.microphone() {
        // A tiny waveform on the voice track, so the choice reads as sound.
        let wave_y = y + gap + (track_h + gap) * 1.5;
        let phase = phase();
        cr.set_source_rgba(0.98, 0.92, 0.82, 0.70);
        cr.set_line_width(1.4 * s);
        cr.set_line_cap(cairo::LineCap::Round);
        let mut xpos = x + w * 0.08;
        while xpos < x + w * 0.30 {
            let i = (xpos - x) / (w * 0.012);
            let amp = ((i * 1.7 + phase * 4.0).sin().abs() * 0.7 + 0.3) * track_h * 0.46;
            cr.move_to(xpos, wave_y - amp);
            cr.line_to(xpos, wave_y + amp);
            xpos += w * 0.012;
        }
        cr.stroke().ok();
    }
}

fn draw_recording_pill(cr: &Context, x: f64, y: f64, w: f64, h: f64, s: f64) {
    rounded_rect(cr, x, y, w, h, h / 2.0);
    cr.set_source_rgba(0.06, 0.05, 0.05, 0.92);
    cr.fill().ok();
    rounded_rect(cr, x + 0.5, y + 0.5, w - 1.0, h - 1.0, h / 2.0);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.14);
    cr.set_line_width(1.0);
    cr.stroke().ok();

    let pulse = 0.55 + 0.45 * (phase() * 2.0 * PI).sin().abs();
    let dot_x = x + h * 0.66;
    let dot_y = y + h / 2.0;
    cr.arc(dot_x, dot_y, h * 0.16, 0.0, 2.0 * PI);
    cr.set_source_rgba(1.0, 0.30, 0.24, pulse);
    cr.fill().ok();

    text(
        cr,
        "00:12",
        x + h * 1.05,
        y + h / 2.0,
        9.5 * s,
        Weight::Semibold,
        (1.0, 0.97, 0.94, 0.95),
        0.0,
    );
    let stop_w = h * 0.16;
    rounded_rect(
        cr,
        x + w - h * 0.72,
        y + h / 2.0 - stop_w,
        stop_w * 2.0,
        stop_w * 2.0,
        2.5 * s,
    );
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.82);
    cr.fill().ok();
}

fn draw_quick_access(cr: &Context, x: f64, y: f64, w: f64, h: f64, s: f64) {
    for (offset, alpha) in [(9.0, 0.10), (5.0, 0.12)] {
        rounded_rect(cr, x, y + offset * s, w, h, 12.0 * s);
        cr.set_source_rgba(0.06, 0.04, 0.03, alpha);
        cr.fill().ok();
    }
    rounded_rect(cr, x, y, w, h, 12.0 * s);
    cr.set_source_rgba(0.105, 0.085, 0.072, 0.97);
    cr.fill().ok();
    rounded_rect(cr, x + 0.5, y + 0.5, w - 1.0, h - 1.0, 12.0 * s);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.14);
    cr.set_line_width(1.0);
    cr.stroke().ok();

    let thumb = h * 0.62;
    let thumb_x = x + h * 0.24;
    let thumb_y = y + h / 2.0 - thumb / 2.0;
    rounded_rect(cr, thumb_x, thumb_y, thumb, thumb, 6.0 * s);
    cr.clip_preserve();
    let thumb_gradient =
        cairo::LinearGradient::new(thumb_x, thumb_y, thumb_x + thumb, thumb_y + thumb);
    thumb_gradient.add_color_stop_rgb(0.0, 0.98, 0.66, 0.45);
    thumb_gradient.add_color_stop_rgb(0.55, 0.75, 0.35, 0.36);
    thumb_gradient.add_color_stop_rgb(1.0, 0.28, 0.12, 0.22);
    cr.set_source(&thumb_gradient).ok();
    cr.fill().ok();

    let text_x = thumb_x + thumb + h * 0.20;
    text(
        cr,
        "Screenshot 20.47.39.png",
        text_x,
        y + h * 0.35,
        8.6 * s,
        Weight::Semibold,
        (1.0, 0.97, 0.93, 0.95),
        0.0,
    );
    text(
        cr,
        "Copied to clipboard",
        text_x,
        y + h * 0.57,
        7.4 * s,
        Weight::Normal,
        (0.72, 0.90, 0.72, 0.88),
        0.0,
    );

    let button_y = y + h * 0.78;
    for (index, glyph) in ["copy", "save", "more"].into_iter().enumerate() {
        let bx = text_x + index as f64 * h * 0.24;
        cr.arc(bx + h * 0.08, button_y, h * 0.075, 0.0, 2.0 * PI);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
        cr.fill().ok();
        cr.set_source_rgba(1.0, 0.97, 0.93, 0.72);
        cr.set_line_width(1.1 * s);
        let cx = bx + h * 0.08;
        match glyph {
            "copy" => {
                cr.rectangle(cx - 2.4 * s, button_y - 2.6 * s, 3.6 * s, 4.2 * s);
                cr.stroke().ok();
                cr.rectangle(cx - 0.6 * s, button_y - 1.0 * s, 3.6 * s, 4.2 * s);
                cr.stroke().ok();
            }
            "save" => {
                cr.move_to(cx, button_y - 3.2 * s);
                cr.line_to(cx, button_y + 1.0 * s);
                cr.move_to(cx - 1.8 * s, button_y - 0.8 * s);
                cr.line_to(cx, button_y + 1.0 * s);
                cr.line_to(cx + 1.8 * s, button_y - 0.8 * s);
                cr.move_to(cx - 3.0 * s, button_y + 2.8 * s);
                cr.line_to(cx + 3.0 * s, button_y + 2.8 * s);
                cr.stroke().ok();
            }
            _ => {
                for dx in [-2.4, 0.0, 2.4] {
                    cr.arc(cx + dx * s, button_y, 0.7 * s, 0.0, 2.0 * PI);
                    cr.fill().ok();
                }
            }
        }
    }
}

fn draw_camera_bubble(cr: &Context, cx: f64, cy: f64, radius: f64, s: f64) {
    cr.arc(cx, cy, radius + 2.0 * s, 0.0, 2.0 * PI);
    cr.set_source_rgba(1.0, 0.98, 0.95, 0.92);
    cr.fill().ok();

    cr.save().ok();
    cr.arc(cx, cy, radius, 0.0, 2.0 * PI);
    cr.clip();
    let bubble = cairo::LinearGradient::new(cx - radius, cy - radius, cx + radius, cy + radius);
    bubble.add_color_stop_rgb(0.0, 0.99, 0.77, 0.58);
    bubble.add_color_stop_rgb(0.52, 0.87, 0.47, 0.44);
    bubble.add_color_stop_rgb(1.0, 0.42, 0.18, 0.30);
    cr.set_source(&bubble).ok();
    cr.paint().ok();

    cr.arc(cx, cy - radius * 0.22, radius * 0.27, 0.0, 2.0 * PI);
    cr.set_source_rgba(0.26, 0.13, 0.14, 0.72);
    cr.fill().ok();
    cr.save().ok();
    cr.translate(cx, cy + radius * 0.92);
    cr.scale(1.0, 0.72);
    cr.arc(0.0, 0.0, radius * 0.55, PI, 2.0 * PI);
    cr.set_source_rgba(0.26, 0.13, 0.14, 0.72);
    cr.fill().ok();
    cr.restore().ok();
    cr.restore().ok();

    // A small camera badge on the bubble's shoulder.
    let bx = cx + radius * 0.72;
    let by = cy + radius * 0.72;
    cr.arc(bx, by, radius * 0.26, 0.0, 2.0 * PI);
    cr.set_source_rgba(0.09, 0.07, 0.06, 0.92);
    cr.fill().ok();
    cr.rectangle(
        bx - radius * 0.10,
        by - radius * 0.06,
        radius * 0.13,
        radius * 0.12,
    );
    cr.set_source_rgba(1.0, 0.96, 0.92, 0.86);
    cr.fill().ok();
    cr.move_to(bx + radius * 0.03, by);
    cr.line_to(bx + radius * 0.11, by - radius * 0.05);
    cr.line_to(bx + radius * 0.11, by + radius * 0.05);
    cr.close_path();
    cr.fill().ok();
}

fn draw_voice_meter(cr: &Context, x: f64, y: f64, w: f64, h: f64, s: f64) {
    rounded_rect(cr, x, y, w, h, h / 2.0);
    cr.set_source_rgba(0.08, 0.06, 0.05, 0.88);
    cr.fill().ok();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.12);
    cr.set_line_width(1.0);
    rounded_rect(cr, x + 0.5, y + 0.5, w - 1.0, h - 1.0, h / 2.0);
    cr.stroke().ok();

    let bars = 9;
    let phase = phase();
    let inner_x = x + h * 0.55;
    let usable = w - h * 1.1;
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_width(2.0 * s);
    for index in 0..bars {
        let t = index as f64 / (bars - 1) as f64;
        let amp = ((t * 5.0 + phase * 6.0).sin().abs() * 0.55 + 0.25) * h * 0.34;
        let bx = inner_x + usable * t;
        cr.move_to(bx, y + h / 2.0 - amp);
        cr.line_to(bx, y + h / 2.0 + amp);
        cr.set_source_rgba(1.0, 0.55, 0.30, 0.90);
        cr.stroke().ok();
    }
}

fn draw_theme_preview(cr: &Context, width: f64, height: f64, kind: &'static str) {
    if width < 2.0 || height < 2.0 {
        return;
    }
    match kind {
        "system" => {
            draw_mini_window(cr, 4.0, 6.0, width / 2.0 - 6.0, height - 12.0, true);
            draw_mini_window(
                cr,
                width / 2.0 + 2.0,
                6.0,
                width / 2.0 - 6.0,
                height - 12.0,
                false,
            );
        }
        "light" => draw_mini_window(cr, 10.0, 7.0, width - 20.0, height - 14.0, false),
        _ => draw_mini_window(cr, 10.0, 7.0, width - 20.0, height - 14.0, true),
    }
}

fn draw_mini_window(cr: &Context, x: f64, y: f64, w: f64, h: f64, dark: bool) {
    let radius = 7.0;
    rounded_rect(cr, x, y, w, h, radius);
    if dark {
        cr.set_source_rgba(0.075, 0.07, 0.065, 1.0);
    } else {
        cr.set_source_rgba(0.976, 0.965, 0.945, 1.0);
    }
    cr.fill().ok();
    cr.set_source_rgba(
        if dark { 1.0 } else { 0.08 },
        if dark { 1.0 } else { 0.06 },
        if dark { 1.0 } else { 0.05 },
        if dark { 0.10 } else { 0.14 },
    );
    cr.set_line_width(1.0);
    rounded_rect(cr, x + 0.5, y + 0.5, w - 1.0, h - 1.0, radius);
    cr.stroke().ok();

    let sidebar_w = w * 0.27;
    cr.save().ok();
    rounded_rect(cr, x, y, w, h, radius);
    cr.clip();
    cr.rectangle(x, y, sidebar_w, h);
    if dark {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.045);
    } else {
        cr.set_source_rgba(0.07, 0.06, 0.05, 0.055);
    }
    cr.fill().ok();
    cr.restore().ok();

    // Sidebar nav bars.
    for index in 0..4 {
        let bar_y = y + h * (0.26 + index as f64 * 0.14);
        rounded_rect(cr, x + w * 0.06, bar_y, sidebar_w * 0.60, 3.0, 1.5);
        if dark {
            cr.set_source_rgba(1.0, 1.0, 1.0, if index == 0 { 0.62 } else { 0.20 });
        } else {
            cr.set_source_rgba(0.08, 0.07, 0.06, if index == 0 { 0.62 } else { 0.18 });
        }
        cr.fill().ok();
    }

    // Accent row in the sidebar head.
    rounded_rect(cr, x + w * 0.06, y + h * 0.12, sidebar_w * 0.34, 3.0, 1.5);
    cr.set_source_rgba(1.0, 0.42, 0.16, 0.90);
    cr.fill().ok();

    // Content headline and two cards.
    rounded_rect(
        cr,
        x + sidebar_w + w * 0.06,
        y + h * 0.16,
        w * 0.42,
        3.2,
        1.6,
    );
    cr.set_source_rgba(
        if dark { 1.0 } else { 0.08 },
        if dark { 1.0 } else { 0.07 },
        if dark { 1.0 } else { 0.06 },
        if dark { 0.66 } else { 0.64 },
    );
    cr.fill().ok();
    for index in 0..2 {
        let card_y = y + h * (0.32 + index as f64 * 0.33);
        let card_w = w - sidebar_w - w * 0.12;
        rounded_rect(cr, x + sidebar_w + w * 0.06, card_y, card_w, h * 0.24, 3.0);
        if dark {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.05);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.92);
        }
        cr.fill().ok();
        cr.set_source_rgba(
            if dark { 1.0 } else { 0.08 },
            if dark { 1.0 } else { 0.07 },
            if dark { 1.0 } else { 0.06 },
            0.10,
        );
        cr.set_line_width(0.8);
        rounded_rect(
            cr,
            x + sidebar_w + w * 0.06 + 0.4,
            card_y + 0.4,
            card_w - 0.8,
            h * 0.24 - 0.8,
            3.0,
        );
        cr.stroke().ok();
    }
}

/// Seconds as a free-running phase, used by the pulse and the meters.
fn phase() -> f64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

fn rounded_rect(cr: &Context, x: f64, y: f64, w: f64, h: f64, radius: f64) {
    let radius = radius.max(0.0).min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - radius, y + radius, radius, -PI / 2.0, 0.0);
    cr.arc(x + w - radius, y + h - radius, radius, 0.0, PI / 2.0);
    cr.arc(x + radius, y + h - radius, radius, PI / 2.0, PI);
    cr.arc(x + radius, y + radius, radius, PI, 3.0 * PI / 2.0);
    cr.close_path();
}

#[allow(clippy::too_many_arguments)]
fn text(
    cr: &Context,
    value: &str,
    x: f64,
    y: f64,
    size: f64,
    weight: Weight,
    color: (f64, f64, f64, f64),
    align: f64,
) {
    let layout = create_layout(cr);
    let weight_name = match weight {
        Weight::Bold => "Bold",
        Weight::Semibold => "SemiBold",
        Weight::Medium => "Medium",
        _ => "Regular",
    };
    let desc = pango::FontDescription::from_string(&format!(
        "Inter {} {:.2}",
        weight_name,
        (size * 0.75).max(4.0)
    ));
    layout.set_font_description(Some(&desc));
    layout.set_text(value);
    let (text_w, text_h) = layout.pixel_size();
    let draw_x = x - f64::from(text_w) * align;
    let draw_y = y - f64::from(text_h) / 2.0;
    cr.set_source_rgba(color.0, color.1, color.2, color.3);
    cr.move_to(draw_x, draw_y);
    show_layout(cr, &layout);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenes_follow_the_recording_defaults() {
        let mut config = AppConfig {
            rec_webcam_enabled: true,
            rec_mic: true,
            ..AppConfig::default()
        };
        assert_eq!(HeroScene::for_config(&config), HeroScene::CameraAndVoice);

        config.rec_webcam_enabled = false;
        assert_eq!(HeroScene::for_config(&config), HeroScene::ScreenAndVoice);

        config.rec_mic = false;
        assert_eq!(HeroScene::for_config(&config), HeroScene::ScreenOnly);
    }
}
