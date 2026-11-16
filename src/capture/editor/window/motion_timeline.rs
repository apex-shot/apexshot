//! Motion timeline chrome — same dock/card language as the video editor.
//!
//! The dock uses the compact rows: a short ruler, the source filmstrip, and
//! two clip lanes of which one is drawn at full height while the other rests
//! as a thin strip with its name and slim clip bars. The lanes trade height
//! with a fixed total, so the preview above never moves while they ease.

use gtk4::cairo::{Context, ImageSurface};
use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, Button, DrawingArea, Image, Label, Orientation, Overlay,
};
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use crate::i18n::t;
use crate::recording::editor::model::DEFAULT_MOTION_DURATION_SECONDS;
use crate::typography::UI_FONT_FAMILY;

use super::icon_names::custom::{
    EDIT_UNDO_RTL_SYMBOLIC, EDIT_UNDO_SYMBOLIC, SPLINE_SYMBOLIC, TYPE_SYMBOLIC,
};
use super::motion_mode::{MotionHoverTrack, MotionRuntime};

/// Row metrics of the compact dock, in logical pixels.
const RULER_HEIGHT: f64 = 28.0;
const SOURCE_LANE_HEIGHT: f64 = 42.0;
const CLIP_LANE_HEIGHT: f64 = 42.0;
const THIN_LANE_HEIGHT: f64 = 18.0;
const LANE_GAP: f64 = 8.0;

/// The source badge's cast: a blurred silhouette with enough pad for the
/// blur to fall off before the surface edge.
const BADGE_SHADOW_PAD: f64 = 10.0;
const BADGE_SHADOW_ICON: f64 = 20.0;

thread_local! {
    static BADGE_SHADOW: OnceCell<Option<ImageSurface>> = const { OnceCell::new() };
}

/// Which clip lane is open (drawn at full height) and how far each lane is
/// through its ease. Progress is `[motion, text]`, 0 resting .. 1 open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::capture::editor::window) struct MotionLanes {
    text_open: bool,
    progress: [f64; 2],
    /// The selection last seen, so a new selection brings its lane forward.
    seen_selection: (Option<usize>, Option<usize>),
}

impl Default for MotionLanes {
    fn default() -> Self {
        Self {
            text_open: false,
            progress: [1.0, 0.0],
            seen_selection: (None, None),
        }
    }
}

impl MotionLanes {
    /// Whether the lane is the one drawn at full height.
    pub(in crate::capture::editor::window) fn is_open(&self, text: bool) -> bool {
        self.text_open == text
    }

    /// Make the lane the full-height one. Returns whether it changed.
    pub(in crate::capture::editor::window) fn open(&mut self, text: bool) -> bool {
        let changed = self.text_open != text;
        self.text_open = text;
        changed
    }

    /// A newly selected clip opens its lane, so a selection made from the
    /// inspector, the clip menu or by adding a clip is always in view.
    pub(in crate::capture::editor::window) fn follow_selection(
        &mut self,
        motion: Option<usize>,
        text: Option<usize>,
    ) {
        let now = (motion, text);
        if self.seen_selection == now {
            return;
        }
        self.seen_selection = now;
        if text.is_some() {
            self.open(true);
        } else if motion.is_some() {
            self.open(false);
        }
    }

    fn targets(&self) -> [f64; 2] {
        [
            f64::from(u8::from(self.is_open(false))),
            f64::from(u8::from(self.is_open(true))),
        ]
    }

    pub(in crate::capture::editor::window) fn settled(&self) -> bool {
        self.progress == self.targets()
    }

    pub(in crate::capture::editor::window) fn progress(&self) -> [f64; 2] {
        self.progress
    }

    /// Ease both lanes toward their targets by `dt` seconds, the video
    /// editor's curve. Returns whether either is still moving.
    pub(in crate::capture::editor::window) fn step(&mut self, dt: f64) -> bool {
        // About 180 ms to settle: an exponential ease with a 45 ms constant.
        let amount = 1.0 - (-dt.max(0.0) / 0.045).exp();
        let targets = self.targets();
        let mut next = self.progress;
        for index in 0..2 {
            next[index] += (targets[index] - next[index]) * amount;
        }
        // Snap only once both arrive: their sum keeps the dock's height.
        let arrived = (0..2).all(|index| (targets[index] - next[index]).abs() < 0.002);
        self.progress = if arrived { targets } else { next };
        !arrived
    }
}

/// A clip lane's height for how open it is drawn.
fn clip_lane_height_exact(progress: f64) -> f64 {
    let progress = progress.clamp(0.0, 1.0);
    THIN_LANE_HEIGHT + (CLIP_LANE_HEIGHT - THIN_LANE_HEIGHT) * progress
}

/// Whole-pixel heights for the two clip lanes whose sum never changes while
/// they trade: the Motion lane rounds on its own and the Text lane takes the
/// rest of the fixed total, so a pixel one loses the other gains.
fn clip_lane_heights(progress: [f64; 2]) -> [i32; 2] {
    let total = (clip_lane_height_exact(1.0) + clip_lane_height_exact(0.0)).round() as i32;
    let motion = clip_lane_height_exact(progress[0]).round() as i32;
    [motion, total - motion]
}

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
    pub hover_playhead: DrawingArea,
    pub playhead_hovered: Rc<Cell<bool>>,
    /// Bring the lane of a new selection forward and ease the lanes toward
    /// the open one. Cheap to call on every redraw.
    pub sync_lanes: Rc<dyn Fn()>,
}

fn lane_area(height: f64) -> DrawingArea {
    let area = DrawingArea::new();
    area.add_css_class("recording-editor-card-zoom-track");
    area.set_hexpand(true);
    area.set_size_request(-1, height as i32);
    area
}

pub(super) fn build_motion_timeline(runtime: Rc<RefCell<MotionRuntime>>) -> MotionTimeline {
    let dock = GtkBox::new(Orientation::Vertical, 0);
    dock.add_css_class("recording-editor-timeline-dock");
    dock.add_css_class("editor-motion-timeline-dock");
    dock.add_css_class("recording-editor-timeline-compact");
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

    let add_btn = labeled_tool_button(SPLINE_SYMBOLIC, &t("Motion"), &t("Add motion at playhead"));
    add_btn.add_css_class("recording-editor-timeline-tool-active");
    let add_text_btn = labeled_tool_button(TYPE_SYMBOLIC, &t("Text"), &t("Add text at playhead"));

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

    // The ruler and playhead are painted by hand rather than styled, so they
    // need the same light/dark decision the themed surfaces get.
    let light_theme = !crate::capture::editor::ui_support::prefers_dark_glass_theme();

    let ruler = DrawingArea::new();
    ruler.add_css_class("recording-editor-card-ruler");
    ruler.set_hexpand(true);
    ruler.set_size_request(-1, RULER_HEIGHT as i32);
    ruler.set_draw_func({
        let runtime = runtime.clone();
        move |_, cr, width, height| draw_ruler(cr, width, height, &runtime, light_theme)
    });

    // The source is a real timeline clip, not an implied backdrop. ApexShot
    // has one static source, so the lane spans the entire Motion composition.
    let source_track = lane_area(SOURCE_LANE_HEIGHT);
    source_track.set_draw_func({
        let runtime = runtime.clone();
        move |_, cr, width, height| draw_source_track(cr, width, height, &runtime)
    });

    let initial = clip_lane_heights(runtime.borrow().lanes.progress());
    let track = lane_area(f64::from(initial[0]));
    track.set_tooltip_text(Some(&t("Motion")));
    track.set_draw_func({
        let runtime = runtime.clone();
        move |_, cr, width, height| draw_lane(cr, width, height, &runtime, false, light_theme)
    });

    let text_track = lane_area(f64::from(initial[1]));
    text_track.set_tooltip_text(Some(&t("Text")));
    text_track.set_draw_func({
        let runtime = runtime.clone();
        move |_, cr, width, height| draw_lane(cr, width, height, &runtime, true, light_theme)
    });

    let tracks = GtkBox::new(Orientation::Vertical, LANE_GAP as i32);
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

    let playhead_hovered = Rc::new(Cell::new(false));
    // Pure draw: the playhead is painted in this static overlay and follows
    // model state, so a scrub never writes widget geometry or invalidates
    // layout (the video editor card works the same way). The ruler is the
    // grab surface, and hover hit-tests the drawn head in board coordinates
    // (`playhead_head_hit`) instead of a positioned handle widget.
    playhead.set_draw_func({
        let runtime = runtime.clone();
        move |_, cr, width, height| {
            draw_playhead(cr, width, height, &runtime, light_theme);
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

    card.append(&toolbar);
    card.append(&board);
    dock.append(&card);

    // The lanes ease on the frame clock only while they move: the callback
    // ends once both arrive, so an idle timeline costs no frames.
    let sync_lanes: Rc<dyn Fn()> = {
        let runtime = runtime.clone();
        let tracks = tracks.clone();
        let motion_track = track.clone();
        let text_track = text_track.clone();
        let running = Rc::new(Cell::new(false));
        Rc::new(move || {
            let animate = {
                let Ok(mut runtime) = runtime.try_borrow_mut() else {
                    return;
                };
                let (motion, text) = (runtime.motion.selected, runtime.motion.selected_text);
                runtime.lanes.follow_selection(motion, text);
                !runtime.lanes.settled()
            };
            if !animate || running.replace(true) {
                return;
            }
            let runtime = runtime.clone();
            let running = running.clone();
            let motion_track = motion_track.clone();
            let text_track = text_track.clone();
            let last = Cell::new(None::<i64>);
            tracks.add_tick_callback(move |_, clock| {
                let now = clock.frame_time();
                let dt = last
                    .replace(Some(now))
                    .map(|previous| (now - previous) as f64 / 1_000_000.0)
                    .unwrap_or(0.0)
                    .min(0.1);
                let Ok(mut guard) = runtime.try_borrow_mut() else {
                    return glib::ControlFlow::Continue;
                };
                let moving = guard.lanes.step(dt);
                let heights = clip_lane_heights(guard.lanes.progress());
                drop(guard);
                if motion_track.height_request() != heights[0] {
                    motion_track.set_size_request(-1, heights[0]);
                }
                if text_track.height_request() != heights[1] {
                    text_track.set_size_request(-1, heights[1]);
                }
                motion_track.queue_draw();
                text_track.queue_draw();
                if moving {
                    glib::ControlFlow::Continue
                } else {
                    running.set(false);
                    glib::ControlFlow::Break
                }
            });
        })
    };

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
        hover_playhead,
        playhead_hovered,
        sync_lanes,
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

    // The preview texture is plenty for a 30px strip and far cheaper to
    // sample than the full-resolution still.
    if let Some(card) = runtime.card_preview.as_ref().or(runtime.card.as_ref()) {
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
            cr.source().set_filter(gtk4::cairo::Filter::Bilinear);
            let _ = cr.paint();
            let _ = cr.restore();
            thumbnail_x += thumbnail_w;
        }
        let _ = cr.restore();
    }

    draw_source_chip(cr, w, y, clip_h, runtime.motion.duration);

    if runtime.source_selected {
        rounded_rect(
            cr,
            0.5,
            y + 0.5,
            (w - 1.0).max(0.0),
            (clip_h - 1.0).max(0.0),
            4.5,
        );
        cr.set_source_rgba(0.80, 0.88, 1.0, 0.90);
        cr.set_line_width(1.5);
        let _ = cr.stroke();
    }
}

/// The badge's cast, built once: a rounded silhouette blurred three times so
/// the halo is a real gradient instead of stacked rings.
fn badge_shadow() -> Option<ImageSurface> {
    BADGE_SHADOW.with(|cache| cache.get_or_init(build_badge_shadow).clone())
}

fn build_badge_shadow() -> Option<ImageSurface> {
    let size = (BADGE_SHADOW_ICON + BADGE_SHADOW_PAD * 2.0) as i32;
    let mut surface = ImageSurface::create(gtk4::cairo::Format::ARgb32, size, size).ok()?;
    {
        let cr = Context::new(&surface).ok()?;
        rounded_rect(
            &cr,
            BADGE_SHADOW_PAD,
            BADGE_SHADOW_PAD,
            BADGE_SHADOW_ICON,
            BADGE_SHADOW_ICON,
            5.0,
        );
        cr.set_source_rgba(0.0, 0.0, 0.0, 1.0);
        let _ = cr.fill();
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().ok()?;
    let mut image = crate::capture::editor::render::cairo_argb_to_rgba_image(
        size as u32,
        size as u32,
        stride,
        data.as_ref(),
    );
    drop(data);
    let rect = crate::capture::editor::types::Rect {
        x: 0,
        y: 0,
        width: size,
        height: size,
    };
    for _ in 0..3 {
        crate::capture::editor::render::apply_blur_rect(&mut image, rect, 3.0, true);
    }
    crate::capture::editor::render::rgba_image_to_surface(&image)
}

/// The source lane's identity badge: the image glyph, "Image", and the clip
/// length, set directly on the filmstrip and centred on the clip.
fn draw_source_chip(cr: &Context, width: f64, y: f64, clip_h: f64, duration: f64) {
    let title = t("Image");
    let duration_label = format!("{}s", duration.round().max(0.0) as i64);
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

    let icon_size = 20.0;
    let gap = 7.0;
    let block_w = icon_size + gap + title_w.max(duration_w);
    let block_h = 30.0;
    let block_x = ((width - block_w) / 2.0).clamp(8.0, (width - block_w - 8.0).max(8.0));
    let block_y = y + (clip_h - block_h) / 2.0;

    let icon_x = block_x;
    let icon_y = block_y + (block_h - icon_size) / 2.0;
    // A real blurred silhouette, not stacked rings: the badge reads as a
    // floating tag over the filmstrip.
    if let Some(shadow) = badge_shadow() {
        cr.set_source_surface(
            &shadow,
            icon_x - BADGE_SHADOW_PAD,
            icon_y - BADGE_SHADOW_PAD + 2.0,
        )
        .ok();
        let _ = cr.paint();
        let _ = cr.paint();
    }
    rounded_rect(cr, icon_x, icon_y, icon_size, icon_size, 5.0);
    cr.set_source_rgba(0.93, 0.95, 0.99, 1.0);
    let _ = cr.fill_preserve();
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.16);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
    let _ = cr.save();
    rounded_rect(cr, icon_x, icon_y, icon_size, icon_size, 5.0);
    cr.clip();
    cr.set_source_rgba(0.07, 0.07, 0.09, 1.0);
    // Sun and two peaks, kept apart so the mark reads as a picture rather
    // than one solid mound.
    cr.arc(icon_x + 6.4, icon_y + 6.2, 2.0, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
    cr.move_to(icon_x + 2.5, icon_y + 17.0);
    cr.line_to(icon_x + 7.5, icon_y + 11.0);
    cr.line_to(icon_x + 10.6, icon_y + 14.2);
    cr.line_to(icon_x + 16.0, icon_y + 7.5);
    cr.line_to(icon_x + 17.5, icon_y + 9.0);
    cr.line_to(icon_x + 17.5, icon_y + 17.0);
    cr.close_path();
    let _ = cr.fill();
    let _ = cr.restore();

    let text_x = icon_x + icon_size + gap;
    paint_outlined_text(cr, text_x, block_y + 12.0, &title, 11.0, true, 0.98);
    paint_outlined_text(
        cr,
        text_x,
        block_y + 24.0,
        &duration_label,
        10.0,
        false,
        0.92,
    );
}

/// White type with a soft drop and a dark outline, the recipe that stays
/// legible over a bright still as well as a dark one.
fn paint_outlined_text(
    cr: &Context,
    x: f64,
    baseline: f64,
    text: &str,
    size: f64,
    bold: bool,
    alpha: f64,
) {
    cr.select_font_face(
        UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        if bold {
            gtk4::cairo::FontWeight::Bold
        } else {
            gtk4::cairo::FontWeight::Normal
        },
    );
    cr.set_font_size(size);
    for (dx, dy, shade) in [
        (0.0, 2.0, 0.10),
        (1.0, 1.0, 0.12),
        (-1.0, 1.0, 0.12),
        (0.0, 1.0, 0.22),
    ] {
        cr.new_path();
        cr.move_to(x + dx, baseline + dy);
        cr.text_path(text);
        cr.set_source_rgba(0.0, 0.0, 0.0, shade);
        let _ = cr.fill();
    }
    cr.new_path();
    cr.move_to(x, baseline);
    cr.text_path(text);
    cr.set_line_width(2.0);
    cr.set_source_rgba(0.05, 0.05, 0.07, 0.55);
    let _ = cr.stroke_preserve();
    cr.set_source_rgba(1.0, 1.0, 1.0, alpha);
    let _ = cr.fill();
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

fn draw_ruler(
    cr: &Context,
    width: i32,
    height: i32,
    runtime: &Rc<RefCell<MotionRuntime>>,
    light: bool,
) {
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
    cr.set_font_size(11.0);
    cr.set_line_width(1.0);

    // Ticks step by index, not by accumulating `minor`: a running sum drifts
    // below whole seconds and a label floors to the second before it.
    let steps = (duration / minor + 0.0001).floor() as i64;
    for step in 0..=steps {
        let tick = step as f64 * minor;
        let x = time_to_x(tick, duration, w).floor() + 0.5;
        let on_major = ((tick / major) - (tick / major).round()).abs() < 0.02;
        let (tick_r, tick_g, tick_b) = if light {
            (0.07, 0.08, 0.09)
        } else {
            (1.0, 1.0, 1.0)
        };
        let tick_alpha = if on_major {
            if light {
                0.24
            } else {
                0.28
            }
        } else if light {
            0.08
        } else {
            0.10
        };
        cr.set_source_rgba(tick_r, tick_g, tick_b, tick_alpha);
        cr.move_to(x, if on_major { h - 11.0 } else { h - 5.0 });
        cr.line_to(x, h);
        let _ = cr.stroke();
        if on_major {
            let label = format_ruler_label(tick, major);
            let near_playhead = (tick - playhead).abs() < major * 0.08;
            let label_alpha = if near_playhead {
                0.92
            } else if light {
                0.60
            } else {
                0.52
            };
            cr.set_source_rgba(tick_r, tick_g, tick_b, label_alpha);
            if let Ok(ext) = cr.text_extents(&label) {
                let label_x = if tick <= 0.001 {
                    0.0
                } else {
                    (x - ext.width() / 2.0).clamp(0.0, w - ext.width())
                };
                cr.move_to(label_x, 12.0);
                let _ = cr.show_text(&label);
            }
        }
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
        // A tick a hair under a whole second still names that second.
        format!("{minutes}:{:02}", (secs + 1e-6).floor() as u64)
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

/// Solid clip palette for the Motion lanes.
///
/// A text clip uses the editor's primary button orange (`#ff670d`), so the clip
/// and the button that creates it read as the same colour. A motion clip uses
/// the editor palette's own azure (`#0a85ff`, from `DRAW_COLORS`), rather than
/// the video editor's flat pure-blue zoom clips, so the two timelines stay
/// visually distinct.
///
/// Selection never lightens either hue. A clip that whitens on click reads as
/// a washed-out version of itself, so a selected clip keeps the lane's one
/// solid fill and states the selection with the deeper same-hue ring and grips
/// below.
const MOTION_CLIP_BLUE: (f64, f64, f64) = (0.04, 0.52, 1.00);
const MOTION_CLIP_BLUE_DEEP: (f64, f64, f64) = (0.02, 0.29, 0.72);
const MOTION_CLIP_BLUE_EDGE: (f64, f64, f64) = (0.55, 0.78, 1.00);
const TEXT_CLIP_ORANGE: (f64, f64, f64) = (1.0, 103.0 / 255.0, 13.0 / 255.0);
const TEXT_CLIP_ORANGE_DEEP: (f64, f64, f64) = (0.62, 0.22, 0.0);
const TEXT_CLIP_ORANGE_EDGE: (f64, f64, f64) = (1.0, 178.0 / 255.0, 115.0 / 255.0);

/// Colors for one effect clip. `faint` is the sibling state while another
/// clip in the same lane owns the selection: it steps back so the selected
/// clip reads first without disappearing. Every state keeps the lane's one
/// solid hue and fades it, rather than desaturating toward white, so all the
/// clips in a lane stay recognisably the same colour.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ClipTone {
    fill: (f64, f64, f64, f64),
    edge: (f64, f64, f64, f64),
    handle: (f64, f64, f64, f64),
    label: f64,
}

fn motion_clip_tone(selected: bool, faint: bool) -> ClipTone {
    // One fill for every state: selection is the ring and the grips, so a
    // clicked clip keeps its colour instead of turning pale.
    let blue = MOTION_CLIP_BLUE;
    if selected {
        ClipTone {
            fill: (blue.0, blue.1, blue.2, 1.0),
            edge: (
                MOTION_CLIP_BLUE_DEEP.0,
                MOTION_CLIP_BLUE_DEEP.1,
                MOTION_CLIP_BLUE_DEEP.2,
                0.95,
            ),
            handle: (
                MOTION_CLIP_BLUE_DEEP.0,
                MOTION_CLIP_BLUE_DEEP.1,
                MOTION_CLIP_BLUE_DEEP.2,
                0.98,
            ),
            label: 0.96,
        }
    } else if faint {
        ClipTone {
            fill: (blue.0, blue.1, blue.2, 0.55),
            edge: (0.0, 0.0, 0.0, 0.0),
            handle: (0.0, 0.0, 0.0, 0.0),
            label: 0.55,
        }
    } else {
        ClipTone {
            fill: (blue.0, blue.1, blue.2, 0.95),
            edge: (0.0, 0.0, 0.0, 0.0),
            handle: (0.0, 0.0, 0.0, 0.0),
            label: 0.92,
        }
    }
}

fn text_clip_tone(selected: bool, faint: bool) -> ClipTone {
    // Same rule as the Motion lane: the fill is the button orange in every
    // state, and selection is the deeper same-hue ring rather than a lighter
    // fill.
    let orange = TEXT_CLIP_ORANGE;
    if selected {
        ClipTone {
            fill: (orange.0, orange.1, orange.2, 1.0),
            edge: (
                TEXT_CLIP_ORANGE_DEEP.0,
                TEXT_CLIP_ORANGE_DEEP.1,
                TEXT_CLIP_ORANGE_DEEP.2,
                0.95,
            ),
            handle: (
                TEXT_CLIP_ORANGE_DEEP.0,
                TEXT_CLIP_ORANGE_DEEP.1,
                TEXT_CLIP_ORANGE_DEEP.2,
                0.98,
            ),
            label: 0.96,
        }
    } else if faint {
        ClipTone {
            fill: (orange.0, orange.1, orange.2, 0.55),
            edge: (0.0, 0.0, 0.0, 0.0),
            handle: (0.0, 0.0, 0.0, 0.0),
            label: 0.55,
        }
    } else {
        ClipTone {
            fill: (orange.0, orange.1, orange.2, 0.95),
            edge: (0.0, 0.0, 0.0, 0.0),
            handle: (0.0, 0.0, 0.0, 0.0),
            label: 0.92,
        }
    }
}

fn disabled_clip_tone(mut tone: ClipTone, disabled: bool) -> ClipTone {
    if disabled {
        let gray = (tone.fill.0 + tone.fill.1 + tone.fill.2) / 3.0;
        tone.fill.0 = (tone.fill.0 + gray) / 2.0;
        tone.fill.1 = (tone.fill.1 + gray) / 2.0;
        tone.fill.2 = (tone.fill.2 + gray) / 2.0;
        tone.fill.3 *= 0.55;
        tone.edge.3 *= 0.55;
        tone.handle.3 *= 0.6;
        tone.label *= 0.65;
    }
    tone
}

fn draw_clip_glyph(cr: &Context, x: f64, center_y: f64, opacity: f64, text: bool) {
    let _ = cr.save();
    cr.translate(x, center_y - 8.0);
    cr.scale(16.0 / 24.0, 16.0 / 24.0);
    cr.set_source_rgba(1.0, 1.0, 1.0, opacity);
    cr.set_line_width(2.4);
    cr.set_line_cap(gtk4::cairo::LineCap::Round);
    cr.set_line_join(gtk4::cairo::LineJoin::Round);
    if text {
        cr.move_to(12.0, 4.0);
        cr.line_to(12.0, 20.0);
        cr.move_to(4.0, 7.0);
        cr.line_to(4.0, 5.0);
        cr.arc(
            5.0,
            5.0,
            1.0,
            std::f64::consts::PI,
            3.0 * std::f64::consts::FRAC_PI_2,
        );
        cr.line_to(19.0, 4.0);
        cr.arc(
            19.0,
            5.0,
            1.0,
            3.0 * std::f64::consts::FRAC_PI_2,
            std::f64::consts::TAU,
        );
        cr.line_to(20.0, 7.0);
        cr.move_to(9.0, 20.0);
        cr.line_to(15.0, 20.0);
    } else {
        for (cx, cy) in [(19.0, 5.0), (5.0, 19.0)] {
            cr.new_sub_path();
            cr.arc(cx, cy, 2.0, 0.0, std::f64::consts::TAU);
        }
        cr.new_sub_path();
        cr.arc(
            17.0,
            17.0,
            12.0,
            std::f64::consts::PI,
            3.0 * std::f64::consts::FRAC_PI_2,
        );
    }
    let _ = cr.stroke();
    let _ = cr.restore();
}

/// Edge grips that advertise trim-dragging on the selected clip. They render
/// only with a selection, which keeps unselected lanes quiet.
fn draw_trim_handles(cr: &Context, x0: f64, y: f64, clip_w: f64, clip_h: f64, tone: ClipTone) {
    let handle_h = (clip_h - 12.0).max(10.0);
    let handle_y = y + (clip_h - handle_h) / 2.0;
    cr.set_source_rgba(tone.handle.0, tone.handle.1, tone.handle.2, tone.handle.3);
    for handle_x in [x0 + 6.0, x0 + clip_w - 9.0] {
        rounded_rect(cr, handle_x, handle_y, 3.0, handle_h, 2.0);
        let _ = cr.fill();
    }
}

/// The clip spans of one lane: `(start, end, disabled)`.
fn lane_clips(runtime: &MotionRuntime, text: bool) -> Vec<(f64, f64, bool)> {
    if text {
        runtime
            .motion
            .text_segments
            .iter()
            .map(|clip| (clip.start, clip.end, clip.is_disabled))
            .collect()
    } else {
        runtime
            .motion
            .segments
            .iter()
            .map(|clip| (clip.start, clip.end, clip.is_disabled))
            .collect()
    }
}

/// One clip lane at whatever height it is eased to: the full drawing, the
/// thin resting strip, or a cross-fade of both while the lanes trade height.
fn draw_lane(
    cr: &Context,
    width: i32,
    height: i32,
    runtime: &Rc<RefCell<MotionRuntime>>,
    text: bool,
    light: bool,
) {
    let runtime = runtime.borrow();
    let progress = runtime.lanes.progress()[usize::from(text)];
    if progress >= 0.999 {
        draw_clip_track(cr, width, height, &runtime, text);
    } else if progress <= 0.001 {
        draw_thin_lane(cr, width, height, &runtime, text, light);
    } else {
        cr.push_group();
        draw_thin_lane(cr, width, height, &runtime, text, light);
        let _ = cr.pop_group_to_source();
        let _ = cr.paint_with_alpha(1.0 - progress);
        cr.push_group();
        draw_clip_track(cr, width, height, &runtime, text);
        let _ = cr.pop_group_to_source();
        let _ = cr.paint_with_alpha(progress);
    }
}

/// The open lane: the hover ghost, then the clips with their glyphs and
/// labels, and the trim grips on a selected clip. A selection steps this
/// lane's other clips back so the edited clip reads first, without dimming
/// the other lane.
fn draw_clip_track(cr: &Context, width: i32, height: i32, runtime: &MotionRuntime, text: bool) {
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let duration = runtime.motion.duration.max(0.001);
    let hover_track = if text {
        MotionHoverTrack::Text
    } else {
        MotionHoverTrack::Motion
    };
    // The add ghost belongs to the open lane only.
    if runtime.lanes.is_open(text) && runtime.hover_track == Some(hover_track) {
        let span = runtime.hover_time.and_then(|hover| {
            if text {
                runtime.motion.text_add_span(hover)
            } else {
                runtime.motion.motion_add_span(hover)
            }
        });
        if let Some((start, end)) = span {
            let (label, tint, edge) = if text {
                (t("Text"), TEXT_CLIP_ORANGE, TEXT_CLIP_ORANGE_EDGE)
            } else {
                (t("Motion"), MOTION_CLIP_BLUE, MOTION_CLIP_BLUE_EDGE)
            };
            draw_add_track(cr, w, h, duration, start, end, &label, tint, edge);
        }
    }
    let selected = if text {
        runtime.motion.selected_text
    } else {
        runtime.motion.selected
    };
    let has_selection = selected.is_some();
    // Name each clip by its kind. Echoing the typed title here never fit:
    // long strings were cropped mid-word and short ones left the block
    // looking half-drawn, while the preview and the inspector show the text.
    let label = t(if text { "Text" } else { "Motion" });
    for (index, (start, end, disabled)) in lane_clips(runtime, text).into_iter().enumerate() {
        let x0 = time_to_x(start, duration, w);
        let x1 = time_to_x(end, duration, w);
        let clip_w = (x1 - x0).max(22.0);
        let y = if text { 6.0 } else { 7.0 };
        let clip_h = (h - y * 2.0).max(1.0);
        let is_selected = selected == Some(index);
        let faint = has_selection && !is_selected;
        let tone = disabled_clip_tone(
            if text {
                text_clip_tone(is_selected, faint)
            } else {
                motion_clip_tone(is_selected, faint)
            },
            disabled,
        );
        rounded_rect(cr, x0, y, clip_w, clip_h, 5.0);
        cr.set_source_rgba(tone.fill.0, tone.fill.1, tone.fill.2, tone.fill.3);
        let _ = cr.fill();
        if is_selected {
            rounded_rect(
                cr,
                x0 + 0.5,
                y + 0.5,
                (clip_w - 1.0).max(0.0),
                (clip_h - 1.0).max(0.0),
                4.5,
            );
            cr.set_source_rgba(tone.edge.0, tone.edge.1, tone.edge.2, tone.edge.3);
            cr.set_line_width(1.5);
            let _ = cr.stroke();
            draw_trim_handles(cr, x0, y, clip_w, clip_h, tone);
        }
        if clip_w > 32.0 {
            let _ = cr.save();
            rounded_rect(cr, x0, y, clip_w, clip_h, 5.0);
            cr.clip();
            let center_y = y + clip_h / 2.0;
            draw_clip_glyph(cr, x0 + 12.0, center_y, tone.label, text);
            if clip_w > 46.0 {
                let _ = cr.save();
                cr.rectangle(x0 + 30.0, y + 2.0, (clip_w - 44.0).max(0.0), clip_h - 4.0);
                cr.clip();
                cr.set_source_rgba(1.0, 1.0, 1.0, tone.label);
                cr.select_font_face(
                    UI_FONT_FAMILY,
                    gtk4::cairo::FontSlant::Normal,
                    gtk4::cairo::FontWeight::Bold,
                );
                cr.set_font_size(12.0);
                // Centre the label's ink on the glyph's centre instead of
                // leaning on a fraction of the lane height.
                if let Ok(ext) = cr.text_extents(&label) {
                    cr.move_to(x0 + 30.0, center_y - ext.y_bearing() - ext.height() / 2.0);
                    let _ = cr.show_text(&label);
                }
                let _ = cr.restore();
            }
            let _ = cr.restore();
        }
    }
}

/// A resting clip lane, the video editor's thin strip: the lane's name at the
/// left edge and each clip as a slim bar in its own colour. A bar running
/// under the name fades in from transparent, so the name never sits on solid
/// colour.
fn draw_thin_lane(
    cr: &Context,
    width: i32,
    height: i32,
    runtime: &MotionRuntime,
    text: bool,
    light: bool,
) {
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let duration = runtime.motion.duration.max(0.001);
    let label = t(if text { "Text" } else { "Motion" });
    cr.select_font_face(
        UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    cr.set_font_size(12.0);
    let extents = cr.text_extents(&label).ok();
    let text_w = extents.map_or(40.0, |extents| extents.x_advance());
    let text_h = extents.map_or(9.0, |extents| extents.height());
    let label_x = 6.0;
    let label_end = label_x + text_w;
    let fade_end = label_end + 40.0;

    let colour = if text {
        TEXT_CLIP_ORANGE
    } else {
        MOTION_CLIP_BLUE
    };
    let bar_h = (h - 4.0).max(3.0);
    let y = (h - bar_h) / 2.0;
    for (start, end, disabled) in lane_clips(runtime, text) {
        let alpha = if disabled { 0.35 } else { 1.0 };
        let x0 = time_to_x(start, duration, w);
        let x1 = time_to_x(end, duration, w).max(x0 + 4.0);
        rounded_rect(cr, x0, y, x1 - x0, bar_h, (bar_h * 0.35).min(5.0));
        if x0 < fade_end {
            let fade = gtk4::cairo::LinearGradient::new(0.0, 0.0, fade_end, 0.0);
            fade.add_color_stop_rgba(0.0, colour.0, colour.1, colour.2, 0.0);
            fade.add_color_stop_rgba(
                label_end / fade_end,
                colour.0,
                colour.1,
                colour.2,
                alpha * 0.15,
            );
            fade.add_color_stop_rgba(1.0, colour.0, colour.1, colour.2, alpha);
            let _ = cr.set_source(&fade);
        } else {
            cr.set_source_rgba(colour.0, colour.1, colour.2, alpha);
        }
        let _ = cr.fill();
    }
    let ink = if light { 0.0 } else { 1.0 };
    cr.set_source_rgba(ink, ink, ink, 0.6);
    cr.move_to(label_x, (h + text_h) / 2.0);
    let _ = cr.show_text(&label);
}

const PLAYHEAD_HANDLE_W: f64 = 12.0;
const PLAYHEAD_HANDLE_H: f64 = 26.0;
const PLAYHEAD_HANDLE_TOP: f64 = 2.0;
const PLAYHEAD_HOVER_SLOP: f64 = 6.0;

/// Hit-test for the playhead *head* (the capsule) only. The stem below it must
/// not count as grabbing the head.
pub(in crate::capture::editor::window) fn playhead_head_hit(
    pointer_x: f64,
    pointer_y: f64,
    line_x: f64,
) -> bool {
    let half_w = PLAYHEAD_HANDLE_W / 2.0;
    pointer_y <= PLAYHEAD_HANDLE_TOP + PLAYHEAD_HANDLE_H + PLAYHEAD_HOVER_SLOP
        && (pointer_x - line_x).abs() <= half_w + PLAYHEAD_HOVER_SLOP
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
    light: bool,
) {
    let runtime = runtime.borrow();
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let x = time_to_x(runtime.motion.playhead, runtime.motion.duration, w).floor() + 0.5;

    // Stem: thin line running from the handle to the bottom of the board. The
    // recording editor paints the same mark black on its light timeline.
    let (stem_r, stem_g, stem_b) = if light {
        (0.07, 0.08, 0.09)
    } else {
        (0.86, 0.90, 0.98)
    };
    cr.set_source_rgba(stem_r, stem_g, stem_b, 0.9);
    cr.set_line_width(1.5);
    cr.move_to(x, PLAYHEAD_HANDLE_TOP + PLAYHEAD_HANDLE_H);
    cr.line_to(x, h);
    let _ = cr.stroke();

    // The stem must pierce the capsule's center even on the first/last frame.
    // Clamping the capsule into the board pushed it off the stem, so let it
    // clip at the edge instead, like the video editor's playhead mark.
    let hx = x - PLAYHEAD_HANDLE_W / 2.0;

    // Handle: dark capsule with a light outline.
    rounded_rect(
        cr,
        hx,
        PLAYHEAD_HANDLE_TOP,
        PLAYHEAD_HANDLE_W,
        PLAYHEAD_HANDLE_H,
        PLAYHEAD_HANDLE_H / 2.0,
    );
    if light {
        cr.set_source_rgba(0.94, 0.96, 1.0, 1.0);
    } else {
        cr.set_source_rgba(0.04, 0.05, 0.07, 1.0);
    }
    let _ = cr.fill_preserve();
    cr.set_source_rgba(stem_r, stem_g, stem_b, 1.0);
    cr.set_line_width(1.5);
    let _ = cr.stroke();
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
    use super::{
        clip_lane_heights, disabled_clip_tone, format_ruler_label, motion_clip_tone,
        ruler_major_step, text_clip_tone, MotionLanes,
    };

    #[test]
    fn lucide_symbolic_icons_use_filled_outlines_instead_of_svg_strokes() {
        for svg in [
            include_str!("../../../../data/icons/spline-symbolic.svg"),
            include_str!("../../../../data/icons/type-symbolic.svg"),
        ] {
            assert!(svg.contains("fill=\"#222222\""));
            assert!(!svg.contains("stroke=") && !svg.contains("fill=\"none\""));
        }
    }

    #[test]
    fn timeline_hover_never_queues_large_preview_paints_per_pointer_event() {
        let controls = include_str!("motion_mode/controls/timeline.rs");
        let hover = controls
            .split("let hover = EventControllerMotion::new();")
            .nth(1)
            .unwrap();
        let hover = hover.split("board.add_controller(hover);").next().unwrap();
        assert!(hover.contains("preview_dirty = true"));
        assert!(!hover.contains("preview.queue_draw()"));
        let playback = include_str!("motion_mode/controls/playback.rs");
        assert!(playback.contains("add_tick_callback"));
        assert!(!playback.contains("timeout_add_local"));
    }

    #[test]
    fn tab_delete_buttons_are_replaced_by_clip_menu_deletion() {
        assert!(!include_str!("motion_mode/build.rs").contains("delete_btn"));
        assert!(!include_str!("motion_mode/parts.rs").contains("delete_btn"));
        assert!(include_str!("motion_mode/controls/clip_menu.rs").contains("remove_selected()"));
        assert!(include_str!("motion_mode/controls/mod.rs").contains("gdk::Key::Delete"));
    }

    #[test]
    fn disabled_clips_stay_visible_but_dim_their_fill_and_label() {
        for tone in [motion_clip_tone(true, false), text_clip_tone(true, false)] {
            let disabled = disabled_clip_tone(tone, true);
            assert!(disabled.fill.3 > 0.0 && disabled.fill.3 < tone.fill.3);
            assert!(disabled.label > 0.0 && disabled.label < tone.label);
            assert_eq!(disabled_clip_tone(tone, false), tone);
        }
    }

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

    #[test]
    fn selected_clip_outshines_unselected_and_faint_siblings() {
        for (selected, unselected, faint) in [
            (
                motion_clip_tone(true, false),
                motion_clip_tone(false, false),
                motion_clip_tone(false, true),
            ),
            (
                text_clip_tone(true, false),
                text_clip_tone(false, false),
                text_clip_tone(false, true),
            ),
        ] {
            assert!(selected.fill.3 > unselected.fill.3);
            assert!(unselected.fill.3 > faint.fill.3);
            assert!(
                selected.edge.3 >= 0.9,
                "the selected clip needs a bright outline"
            );
            assert!(selected.label > unselected.label);
            assert!(unselected.label > faint.label);
        }
    }

    #[test]
    fn motion_playhead_stays_draw_only_like_the_video_editor_card() {
        let source = include_str!("motion_timeline.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            !source.contains("set_margin_start") && !source.contains("set_width_request"),
            "the playhead must be painted from model state, not positioned as a widget"
        );
        let controls = include_str!("motion_mode/controls/timeline.rs");
        assert!(
            !controls.contains("playhead_handle") && !controls.contains("set_margin_start"),
            "scrubbing must not move a handle widget or invalidate layout"
        );
    }

    #[test]
    fn clip_selection_fades_siblings_in_its_own_lane() {
        let source = include_str!("motion_timeline.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        // Each lane reads only its own selection.
        assert!(source.contains(
            "runtime.motion.selected_text\n    } else {\n        runtime.motion.selected\n    };"
        ));
        assert!(source.contains("let has_selection = selected.is_some();"));
        assert!(source.contains("let faint = has_selection && !is_selected;"));
        assert!(source.contains("text_clip_tone(is_selected, faint)"));
        assert!(source.contains("motion_clip_tone(is_selected, faint)"));
        assert!(
            !source.contains(
                "runtime.motion.selected.is_some() || runtime.motion.selected_text.is_some()"
            ),
            "selection must not dim the other lane"
        );
    }

    #[test]
    fn one_clip_lane_is_open_and_a_new_selection_brings_its_lane_forward() {
        let mut lanes = MotionLanes::default();
        assert!(lanes.is_open(false) && !lanes.is_open(true));
        assert!(lanes.settled());

        lanes.follow_selection(None, Some(0));
        assert!(lanes.is_open(true));
        assert!(!lanes.settled());

        // The same selection seen again does not reopen a lane the user
        // switched away from by hand.
        lanes.open(false);
        lanes.follow_selection(None, Some(0));
        assert!(lanes.is_open(false));

        lanes.follow_selection(Some(1), None);
        assert!(lanes.is_open(false));
    }

    #[test]
    fn lanes_ease_to_their_targets_and_then_stop_animating() {
        let mut lanes = MotionLanes::default();
        lanes.open(true);
        let mut frames = 0;
        while lanes.step(1.0 / 60.0) {
            frames += 1;
            assert!(frames < 120, "the ease must settle");
        }
        assert_eq!(lanes.progress(), [0.0, 1.0]);
        assert!(lanes.settled());
        assert!(frames > 3, "the lanes must ease rather than jump");
    }

    #[test]
    fn trading_lanes_keep_the_dock_height_fixed() {
        let total = clip_lane_heights([1.0, 0.0]).iter().sum::<i32>();
        assert_eq!(clip_lane_heights([1.0, 0.0]), [42, 18]);
        assert_eq!(clip_lane_heights([0.0, 1.0]), [18, 42]);
        for step in 0..=20 {
            let progress = f64::from(step) / 20.0;
            let heights = clip_lane_heights([1.0 - progress, progress]);
            assert_eq!(heights.iter().sum::<i32>(), total);
        }
    }

    #[test]
    fn ruler_labels_name_whole_seconds_without_flooring_below_them() {
        let major = ruler_major_step(10.0);
        let minor = major / 5.0;
        let steps = (10.0 / minor + 0.0001).floor() as i64;
        let labels: Vec<String> = (0..=steps)
            .map(|step| step as f64 * minor)
            .filter(|tick| ((tick / major) - (tick / major).round()).abs() < 0.02)
            .map(|tick| format_ruler_label(tick, major))
            .collect();
        assert_eq!(labels.len(), 11);
        assert_eq!(labels.last().map(String::as_str), Some("0:10"));
        assert_eq!(format_ruler_label(2.999_999_9, 1.0), "0:03");
    }

    #[test]
    fn compact_dock_css_sizes_the_board_for_the_compact_rows() {
        let css = include_str!("../css/10-canvas-inspector.css");
        assert!(css.contains(
            ".editor-motion-timeline-dock.recording-editor-timeline-compact .recording-editor-card-board"
        ));
        // 28 ruler + 42 source + 42 open lane + 18 resting lane + 3 × 8 gaps.
        assert!(css.contains("min-height: 154px;"));
    }
}
