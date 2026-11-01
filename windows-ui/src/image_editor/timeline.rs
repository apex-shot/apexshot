//! The Motion timeline dock, ported from the Linux editor's
//! `motion_timeline.rs`. Ruler, source lane, Motion lane, Text lane and a
//! painted playhead, in the video editor card's language.
//!
//! This slice is a shell: the model is local to the module, so scrubbing,
//! playback, adding a clip and selecting drive it directly, but nothing a
//! drawing backend could read exists on this platform yet.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::cairo::Context as CairoContext;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, DrawingArea, Image, Label, Orientation, Overlay};

use super::ui_support::px;
use crate::capture::editor::window::icon_names::custom;
use crate::i18n::t;

/// The shipped composition length and its bounds, the Linux model's
/// constants.
const DEFAULT_DURATION: f64 = 6.0;
/// The clip length the Add buttons insert at the playhead, matching the
/// Linux default segment.
const DEFAULT_SEGMENT: f64 = 1.0;

/// Clip colours. Motion clips use the editor palette's azure so the two
/// timelines read as different cards; text clips take the primary orange.
/// Selection never lightens a hue: the fill stays solid and the selection
/// states itself with the deeper same-hue ring and the trim grips.
const MOTION_CLIP_BLUE: (f64, f64, f64) = (0.04, 0.52, 1.00);
const MOTION_CLIP_BLUE_DEEP: (f64, f64, f64) = (0.02, 0.29, 0.72);
const MOTION_CLIP_BLUE_EDGE: (f64, f64, f64) = (0.55, 0.78, 1.00);
const TEXT_CLIP_ORANGE: (f64, f64, f64) = (1.0, 103.0 / 255.0, 13.0 / 255.0);
const TEXT_CLIP_ORANGE_DEEP: (f64, f64, f64) = (0.62, 0.22, 0.0);
const TEXT_CLIP_ORANGE_EDGE: (f64, f64, f64) = (1.0, 178.0 / 255.0, 115.0 / 255.0);

/// Playhead geometry, the Linux painter's constants.
const PLAYHEAD_HANDLE_W: f64 = 12.0;
const PLAYHEAD_HANDLE_H: f64 = 26.0;
const PLAYHEAD_HANDLE_TOP: f64 = 2.0;

/// Which lane the pointer is over. Rows own their controllers, so the ghost
/// can never promise a span on the neighbouring lane.
#[derive(Clone, Copy, PartialEq)]
enum HoverLane {
    None,
    Motion,
    Text,
}

/// Fill, edge and label tone for one clip state.
#[derive(Clone, Copy)]
struct ClipTone {
    fill: (f64, f64, f64, f64),
    edge: (f64, f64, f64, f64),
    handle: (f64, f64, f64, f64),
    label: f64,
}

fn motion_clip_tone(selected: bool, faint: bool) -> ClipTone {
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
            label: 0.82,
        }
    }
}

fn text_clip_tone(selected: bool, faint: bool) -> ClipTone {
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
            label: 0.84,
        }
    }
}

/// The exact span a click at `start` would create in `clips`, or `None`
/// when the click is inside a clip or the free gap is too short. The lane's
/// ghost and its Add button share this, so both promise exactly what
/// happens.
fn add_span(clips: &[(f64, f64)], duration: f64, start: f64) -> Option<(f64, f64)> {
    let start = start.clamp(0.0, duration);
    if clips
        .iter()
        .any(|(begin, end)| *begin <= start && start <= *end)
    {
        return None;
    }
    let gap_start = clips
        .iter()
        .filter(|(_, end)| *end <= start + 1e-9)
        .fold(0.0_f64, |best, (_, end)| best.max(*end));
    let gap_end = clips
        .iter()
        .filter(|(begin, _)| *begin >= start - 1e-9)
        .fold(duration, |best, (begin, _)| best.min(*begin));
    if gap_end - gap_start + 1e-9 < DEFAULT_SEGMENT {
        return None;
    }
    let fitted = start.clamp(gap_start, gap_end - DEFAULT_SEGMENT);
    Some((fitted, fitted + DEFAULT_SEGMENT))
}

/// The shell's local model: composition length, playhead, the two clip
/// lanes, and one selection per lane. Painters and handlers both reach it.
struct Model {
    duration: f64,
    playhead: f64,
    motion_clips: Vec<(f64, f64)>,
    text_clips: Vec<(f64, f64)>,
    selected_motion: Option<usize>,
    selected_text: Option<usize>,
}

/// What the painters and handlers share: the mutable model plus the hover
/// read-out.
struct Shared {
    model: RefCell<Model>,
    hover_lane: Cell<HoverLane>,
    hover_time: Cell<f64>,
}

/// The dock and the handles its rows need for interaction wiring.
pub(super) struct Timeline {
    pub dock: GtkBox,
    update_duration: Rc<dyn Fn(f64)>,
}

impl Timeline {
    /// Resize the composition the ruler, lanes and hover read-out map
    /// against; the clock label follows.
    pub(super) fn set_duration(&self, seconds: f64) {
        (self.update_duration)(seconds);
    }
}

/// Rebuild the dock. `dark` decides the ruler and playhead tones once, the
/// way the Linux builder decides at build time.
pub(super) fn build(scale: f64, dark: bool) -> Timeline {
    let light = !dark;
    let shared = Rc::new(Shared {
        model: RefCell::new(Model {
            duration: DEFAULT_DURATION,
            playhead: 0.0,
            motion_clips: Vec::new(),
            text_clips: Vec::new(),
            selected_motion: None,
            selected_text: None,
        }),
        hover_lane: Cell::new(HoverLane::None),
        hover_time: Cell::new(0.0),
    });

    let dock = GtkBox::new(Orientation::Vertical, 0);
    dock.add_css_class("recording-editor-timeline-dock");
    dock.add_css_class("editor-motion-timeline-dock");
    dock.set_hexpand(true);
    dock.set_vexpand(false);
    dock.set_valign(Align::End);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("recording-editor-timeline-card");
    card.set_hexpand(true);

    let play_label = Label::new(Some("0:00"));
    play_label.add_css_class("recording-editor-timeline-clock");
    let duration_label = Label::new(Some(&format_clock(DEFAULT_DURATION)));
    duration_label.add_css_class("recording-editor-timeline-clock");

    let play_button = icon_button("media-playback-start-symbolic", &t("Play"), scale);
    play_button.add_css_class("recording-editor-timeline-play");
    let skip_back = icon_button("media-skip-backward-symbolic", &t("Skip back 1s"), scale);
    let skip_forward = icon_button("media-skip-forward-symbolic", &t("Skip forward 1s"), scale);

    let add_btn = labeled_tool_button(
        custom::SPLINE_SYMBOLIC,
        &t("Motion"),
        &t("Add motion at playhead"),
    );
    add_btn.add_css_class("recording-editor-timeline-tool-active");
    let add_text_btn = labeled_tool_button(
        custom::TYPE_SYMBOLIC,
        &t("Text"),
        &t("Add text at playhead"),
    );

    let undo_btn = icon_button(custom::EDIT_UNDO_SYMBOLIC, &t("Undo"), scale);
    let redo_btn = icon_button(custom::EDIT_UNDO_RTL_SYMBOLIC, &t("Redo"), scale);
    undo_btn.set_sensitive(false);
    redo_btn.set_sensitive(false);

    let toolbar = GtkBox::new(Orientation::Horizontal, 0);
    toolbar.add_css_class("recording-editor-timeline-toolbar");
    toolbar.set_hexpand(true);

    let left = GtkBox::new(Orientation::Horizontal, px(4.0, scale));
    left.set_halign(Align::Start);
    left.set_hexpand(true);
    left.append(&add_btn);
    left.append(&add_text_btn);

    let center = GtkBox::new(Orientation::Horizontal, px(8.0, scale));
    center.add_css_class("recording-editor-timeline-transport");
    center.set_halign(Align::Center);
    center.set_hexpand(false);
    center.append(&play_label);
    center.append(&skip_back);
    center.append(&play_button);
    center.append(&skip_forward);
    center.append(&duration_label);

    let right = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
    right.set_halign(Align::End);
    right.set_hexpand(true);
    right.append(&undo_btn);
    right.append(&redo_btn);

    toolbar.append(&left);
    toolbar.append(&center);
    toolbar.append(&right);

    let ruler = rows_drawing_area(scale, 36.0, "recording-editor-card-ruler");
    let source_track = rows_drawing_area(scale, 48.0, "recording-editor-card-zoom-track");
    let motion_track = rows_drawing_area(scale, 56.0, "recording-editor-card-zoom-track");
    let text_track = rows_drawing_area(scale, 44.0, "recording-editor-card-zoom-track");

    let tracks = GtkBox::new(Orientation::Vertical, px(10.0, scale));
    tracks.add_css_class("recording-editor-card-tracks");
    tracks.set_hexpand(true);
    tracks.append(&ruler);
    tracks.append(&source_track);
    tracks.append(&motion_track);
    tracks.append(&text_track);

    let board = Overlay::new();
    board.add_css_class("recording-editor-card-board");
    board.set_hexpand(true);
    board.set_child(Some(&tracks));

    // The head lines are overlays over the whole board, painted from model
    // state; nothing moves as a widget, so a scrub never invalidates layout.
    let hover_playhead = DrawingArea::new();
    hover_playhead.set_hexpand(true);
    hover_playhead.set_vexpand(true);
    hover_playhead.set_can_target(false);
    let playhead = DrawingArea::new();
    playhead.set_hexpand(true);
    playhead.set_vexpand(true);
    playhead.set_can_target(false);
    playhead.add_css_class("recording-editor-card-playhead");
    board.add_overlay(&hover_playhead);
    board.add_overlay(&playhead);

    card.append(&toolbar);
    card.append(&board);
    dock.append(&card);

    // Every repaint is the same six surfaces; one closure keeps the rule.
    let redraw = {
        let ruler = ruler.clone();
        let source_track = source_track.clone();
        let motion_track = motion_track.clone();
        let text_track = text_track.clone();
        let hover_playhead = hover_playhead.clone();
        let playhead = playhead.clone();
        move || {
            ruler.queue_draw();
            source_track.queue_draw();
            motion_track.queue_draw();
            text_track.queue_draw();
            hover_playhead.queue_draw();
            playhead.queue_draw();
        }
    };
    let redraw: Rc<dyn Fn()> = Rc::new(redraw);

    // ── The head's clock mirrors the model; the label is outside the draw
    // pass so a redraw chain never repaints widget text.
    let set_playhead_label = {
        let play_label = play_label.clone();
        let shared = shared.clone();
        move || {
            play_label.set_text(&format_clock(shared.model.borrow().playhead));
        }
    };

    // ── Transport ──
    // The head is the model's playhead; a scrub, a skip button and playback
    // all write through `move_playhead`, which queues every row and both
    // head overlays.
    let move_playhead: Rc<dyn Fn(f64)> = {
        let shared = shared.clone();
        let redraw = redraw.clone();
        let set_playhead_label = set_playhead_label.clone();
        Rc::new(move |time: f64| {
            let mut model = shared.model.borrow_mut();
            model.playhead = time.clamp(0.0, model.duration);
            drop(model);
            set_playhead_label();
            redraw();
        })
    };

    for (skip, delta) in [(&skip_back, -1.0), (&skip_forward, 1.0)] {
        let shared = shared.clone();
        let move_playhead = move_playhead.clone();
        skip.connect_clicked(move |_| {
            let playhead = shared.model.borrow().playhead;
            move_playhead(playhead + delta);
        });
    }

    // Play runs the head through the duration on the frame clock; the icon
    // toggles with it, and the head parks at the end. One shared playing
    // flag guards the tick callback, so repeated clicks never stack ticks.
    let playing: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    play_button.connect_clicked({
        let playing = playing.clone();
        let shared = shared.clone();
        let set_playhead_label = set_playhead_label.clone();
        let redraw = redraw.clone();
        let play_button = play_button.clone();
        move |_| {
            if playing.replace(!playing.get()) {
                play_button.set_icon_name("media-playback-start-symbolic");
                return;
            }
            play_button.set_icon_name("media-playback-pause-symbolic");
            let playing = playing.clone();
            let shared = shared.clone();
            let set_playhead_label = set_playhead_label.clone();
            let redraw = redraw.clone();
            let button = play_button.clone();
            let last_frame: Rc<Cell<Option<i64>>> = Rc::new(Cell::new(None));
            let weak_button = button.downgrade();
            button.add_tick_callback(move |_, frame_clock| {
                let Some(button) = weak_button.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if !playing.get() {
                    return glib::ControlFlow::Break;
                }
                let frame_time = frame_clock.frame_time();
                let Some(previous) = last_frame.replace(Some(frame_time)) else {
                    return glib::ControlFlow::Continue;
                };
                let span = frame_time.saturating_sub(previous) as f64 / 1_000_000.0;
                let parked = {
                    let mut model = shared.model.borrow_mut();
                    model.playhead = (model.playhead + span).min(model.duration);
                    (model.duration - model.playhead).abs() < 1e-6
                };
                set_playhead_label();
                redraw();
                if parked {
                    playing.set(false);
                    button.set_icon_name("media-playback-start-symbolic");
                }
                let _ = playhead;
                if parked {
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
        }
    });

    // ── Add buttons ──
    // A click adds the lane's default clip at the playhead when the gap
    // allows it, the same span the hover ghost promises.
    let add_clip: Rc<dyn Fn(bool)> = {
        let shared = shared.clone();
        let redraw = redraw.clone();
        Rc::new(move |text: bool| {
            let mut model = shared.model.borrow_mut();
            let playhead = model.playhead;
            if text {
                let Some((start, end)) = add_span(&model.text_clips, model.duration, playhead)
                else {
                    return;
                };
                model.text_clips.push((start, end));
                model.selected_motion = None;
                model.selected_text = Some(model.text_clips.len() - 1);
            } else {
                let Some((start, end)) = add_span(&model.motion_clips, model.duration, playhead)
                else {
                    return;
                };
                model.motion_clips.push((start, end));
                model.selected_text = None;
                model.selected_motion = Some(model.motion_clips.len() - 1);
            }
            drop(model);
            redraw();
        })
    };
    add_btn.connect_clicked({
        let add_clip = add_clip.clone();
        move |_| add_clip(false)
    });
    add_text_btn.connect_clicked({
        let add_clip = add_clip.clone();
        move |_| add_clip(true)
    });

    // ── Scrub ──
    // The ruler is the grab surface; clicking and dragging move the head.
    // Scrub: the ruler is the grab surface; clicking and dragging on it
    // move the head, the way the Linux ruler does.
    let scrub: Rc<dyn Fn(f64)> = {
        let shared = shared.clone();
        let ruler = ruler.clone();
        let move_playhead = move_playhead.clone();
        Rc::new(move |x: f64| {
            let width = f64::from(ruler.allocated_width().max(1));
            let duration = shared.model.borrow().duration;
            move_playhead(x / width * duration);
        })
    };
    {
        let scrub_click = scrub.clone();
        let click = gtk4::GestureClick::new();
        click.set_button(1);
        click.connect_pressed(move |_, _, x, _| scrub_click(x));
        ruler.add_controller(click);
        let drag = gtk4::GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin({
            let drag = drag.clone();
            let scrub_begin = scrub.clone();
            move |gesture, x, _| {
                let _ = &drag;
                if let Some((start_x, _)) = gesture.start_point() {
                    scrub_begin(start_x + x);
                }
            }
        });
        drag.connect_drag_update({
            let scrub_update = scrub.clone();
            move |gesture, dx, _| {
                if let Some((start_x, _)) = gesture.start_point() {
                    scrub_update(start_x + dx);
                }
            }
        });
        ruler.add_controller(drag);
    }

    // ── Lanes ──
    // A lane click selects the clip it covers and clears the other lane.
    for (row, text) in [(&motion_track, false), (&text_track, true)] {
        let shared = shared.clone();
        let redraw = redraw.clone();
        let row = row.clone();
        let click = gtk4::GestureClick::new();
        click.set_button(1);
        click.connect_pressed({
            let row = row.clone();
            move |_, _, x, _| {
                let width = f64::from(row.allocated_width().max(1));
                let mut model = shared.model.borrow_mut();
                let time = x / width * model.duration;
                if text {
                    model.selected_text = find_clip(&model.text_clips, time);
                    model.selected_motion = None;
                } else {
                    model.selected_motion = find_clip(&model.motion_clips, time);
                    model.selected_text = None;
                }
                drop(model);
                redraw();
            }
        });
        row.add_controller(click);
    }

    // ── Hover read-out and lane ghosts ──
    // Each row reports its own lane, so the ghost can never jump rows; the
    // ruler and source lane only move the red read-out line.
    for (row, lane) in [
        (&ruler, HoverLane::None),
        (&source_track, HoverLane::None),
        (&motion_track, HoverLane::Motion),
        (&text_track, HoverLane::Text),
    ] {
        let shared = shared.clone();
        let redraw = redraw.clone();
        let row = row.clone();
        let hover = gtk4::EventControllerMotion::new();
        hover.connect_enter({
            let shared = shared.clone();
            let redraw = redraw.clone();
            let row = row.clone();
            move |_, _, x| {
                shared.hover_lane.set(lane);
                update_hover_time(&shared, &row, x);
                redraw();
            }
        });
        hover.connect_motion({
            let shared = shared.clone();
            let redraw = redraw.clone();
            let row = row.clone();
            move |_, _, x| {
                update_hover_time(&shared, &row, x);
                redraw();
            }
        });
        hover.connect_leave({
            let shared = shared.clone();
            let redraw = redraw.clone();
            move |_| {
                shared.hover_lane.set(HoverLane::None);
                shared.hover_time.set(0.0);
                redraw();
            }
        });
        row.add_controller(hover);
    }

    // ── Painters ──
    ruler.set_draw_func({
        let shared = shared.clone();
        move |_, context, width, height| {
            let model = shared.model.borrow();
            draw_ruler(context, width, height, &model, light);
        }
    });
    source_track.set_draw_func({
        let shared = shared.clone();
        move |_, context, width, height| {
            let model = shared.model.borrow();
            draw_source_track(context, width, height, &model);
        }
    });
    for (row, lane) in [
        (&motion_track, HoverLane::Motion),
        (&text_track, HoverLane::Text),
    ] {
        let shared = shared.clone();
        row.set_draw_func(move |_, context, width, height| {
            let model = shared.model.borrow();
            draw_clip_track(
                context,
                width,
                height,
                &model,
                lane,
                shared.hover_lane.get(),
                shared.hover_time.get(),
            );
        });
    }
    hover_playhead.set_draw_func({
        let shared = shared.clone();
        move |_, context, width, height| {
            let duration = shared.model.borrow().duration;
            draw_hover_playhead(context, width, height, shared.hover_time.get(), duration);
        }
    });
    playhead.set_draw_func({
        let shared = shared.clone();
        move |_, context, width, height| {
            let model = shared.model.borrow();
            draw_playhead(context, width, height, &model, light);
        }
    });

    // The composition length lives behind the Duration slider in the Motion
    // inspector; this closure applies it to the model, clock and ruler.
    let update_duration: Rc<dyn Fn(f64)> = {
        let shared = shared.clone();
        let redraw = redraw.clone();
        let duration_label = duration_label.clone();
        Rc::new(move |seconds: f64| {
            let seconds = seconds.clamp(1.0, 10.0);
            let changed = {
                let mut model = shared.model.borrow_mut();
                if (model.duration - seconds).abs() < 1e-6 {
                    false
                } else {
                    model.duration = seconds;
                    if model.playhead > seconds {
                        model.playhead = seconds;
                    }
                    true
                }
            };
            if changed {
                duration_label.set_text(&format_clock(seconds));
                redraw();
            }
        })
    };

    Timeline {
        dock,
        update_duration,
    }
}

/// Icon-only transport button: a 14px glyph, the Linux dock's size.
fn icon_button(icon_name: &str, tooltip: &str, scale: f64) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-timeline-icon");
    button.set_tooltip_text(Some(tooltip));
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(px(14.0, scale));
    button.set_child(Some(&icon));
    button.set_valign(Align::Center);
    button
}

/// A labelled lane tool: icon plus name, the Add buttons' shape.
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

/// A lane row: one height in logical px at the dock's scale, with the class.
fn rows_drawing_area(scale: f64, height: f64, class: &str) -> DrawingArea {
    let row = DrawingArea::builder()
        .height_request(px(height, scale))
        .build();
    row.add_css_class(class);
    row.set_hexpand(true);
    row
}

fn update_hover_time(shared: &Rc<Shared>, row: &DrawingArea, x: f64) {
    let width = f64::from(row.allocated_width().max(1));
    shared
        .hover_time
        .set(x / width * shared.model.borrow().duration);
}

/// The clip owning `time`, so a lane click selects what it covers and
/// clears its selection elsewhere.
fn find_clip(clips: &[(f64, f64)], time: f64) -> Option<usize> {
    clips
        .iter()
        .position(|(start, end)| *start <= time && time <= *end)
}

// ── Painters ────────────────────────────────────────────────────────
fn time_to_x(time: f64, duration: f64, width: f64) -> f64 {
    (time / duration.max(0.001)).clamp(0.0, 1.0) * width
}

fn draw_ruler(context: &CairoContext, width: i32, height: i32, model: &Model, light: bool) {
    let w = f64::from(width.max(1));
    let h = f64::from(height.max(1));
    let duration = model.duration.max(0.001);
    let playhead = model.playhead;
    let major = ruler_major_step(duration);
    let minor = (major / 5.0).max(0.05);

    context.select_font_face(
        "Inter",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(10.0);
    context.set_line_width(1.0);

    let mut tick = 0.0;
    while tick <= duration + 0.0001 {
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
        context.set_source_rgba(tick_r, tick_g, tick_b, tick_alpha);
        context.move_to(x, if on_major { h - 11.0 } else { h - 5.0 });
        context.line_to(x, h);
        let _ = context.stroke();
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
            context.set_source_rgba(tick_r, tick_g, tick_b, label_alpha);
            if let Ok(ext) = context.text_extents(&label) {
                let label_x = if tick <= 0.001 {
                    0.0
                } else {
                    (x - ext.width() / 2.0).clamp(0.0, w - ext.width())
                };
                context.move_to(label_x, 12.0);
                let _ = context.show_text(&label);
            }
        }
        tick += minor;
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
        format!("{minutes}:{:0>2}", secs.floor() as u64)
    }
}

fn draw_source_track(context: &CairoContext, width: i32, height: i32, model: &Model) {
    let w = f64::from(width.max(1));
    let h = f64::from(height.max(1));
    let inset = 6.0;
    let y = inset;
    let clip_h = (h - inset * 2.0).max(1.0);

    rounded_rect(context, 0.0, y, w, clip_h, 5.0);
    context.set_source_rgba(0.10, 0.11, 0.13, 0.9);
    let _ = context.fill();

    draw_source_chip(context, y, clip_h, model.duration);
}

/// The source lane's identity badge: icon, "Image", and the clip length.
fn draw_source_chip(context: &CairoContext, y: f64, clip_h: f64, duration: f64) {
    let title = t("Image");
    context.select_font_face(
        "Inter",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(11.0);
    let title_w = context
        .text_extents(&title)
        .map(|ext| ext.width())
        .unwrap_or(0.0);
    let duration_label = format!("{}s", duration.round().max(0.0) as i64);
    context.select_font_face(
        "Inter",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(10.0);
    let duration_w = context
        .text_extents(&duration_label)
        .map(|ext| ext.width())
        .unwrap_or(0.0);

    let chip_h = 34.0;
    let chip_x = 8.0;
    let chip_y = y + (clip_h - chip_h) / 2.0;
    let icon_size = 20.0;
    let chip_w = 8.0 + icon_size + 7.0 + title_w.max(duration_w) + 10.0;
    rounded_rect(context, chip_x, chip_y, chip_w, chip_h, 8.0);
    context.set_source_rgba(0.06, 0.06, 0.08, 0.92);
    let _ = context.fill();

    let icon_x = chip_x + 8.0;
    let icon_y = chip_y + (chip_h - icon_size) / 2.0;
    rounded_rect(context, icon_x, icon_y, icon_size, icon_size, 5.0);
    context.set_source_rgba(0.95, 0.96, 0.98, 1.0);
    let _ = context.fill();
    let _ = context.save();
    rounded_rect(context, icon_x, icon_y, icon_size, icon_size, 5.0);
    context.clip();
    context.set_source_rgba(0.10, 0.10, 0.12, 1.0);
    context.arc(icon_x + 6.5, icon_y + 6.5, 2.2, 0.0, std::f64::consts::TAU);
    let _ = context.fill();
    context.move_to(icon_x + 2.0, icon_y + icon_size - 3.0);
    context.line_to(icon_x + 8.0, icon_y + 9.0);
    context.line_to(icon_x + 12.5, icon_y + 14.5);
    context.line_to(icon_x + icon_size - 1.0, icon_y + 7.5);
    context.line_to(icon_x + icon_size - 1.0, icon_y + icon_size - 1.0);
    context.line_to(icon_x + 2.0, icon_y + icon_size - 1.0);
    context.close_path();
    let _ = context.fill();
    let _ = context.restore();

    let text_x = icon_x + icon_size + 7.0;
    context.set_source_rgba(0.96, 0.97, 1.0, 0.96);
    context.select_font_face(
        "Inter",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(11.0);
    context.move_to(text_x, chip_y + 14.0);
    let _ = context.show_text(&title);
    context.set_source_rgba(0.86, 0.88, 0.92, 0.72);
    context.select_font_face(
        "Inter",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(10.0);
    context.move_to(text_x, chip_y + 26.0);
    let _ = context.show_text(&duration_label);
}

/// One clip lane: the hover ghost, then the clips, their glyphs and labels,
/// and the trim grips on a selected clip.
fn draw_clip_track(
    context: &CairoContext,
    width: i32,
    height: i32,
    model: &Model,
    lane_paint: HoverLane,
    hover_lane: HoverLane,
    hover_time: f64,
) {
    let text = lane_paint == HoverLane::Text;
    let clips = if text {
        &model.text_clips
    } else {
        &model.motion_clips
    };
    let selected = if text {
        model.selected_text
    } else {
        model.selected_motion
    };
    let w = f64::from(width.max(1));
    let h = f64::from(height.max(1));
    let duration = model.duration.max(0.001);

    if hover_lane == lane_paint {
        if let Some((start, end)) = add_span(clips, duration, hover_time) {
            let tint = if text {
                TEXT_CLIP_ORANGE
            } else {
                MOTION_CLIP_BLUE
            };
            let edge = if text {
                TEXT_CLIP_ORANGE_EDGE
            } else {
                MOTION_CLIP_BLUE_EDGE
            };
            draw_add_track(
                context,
                w,
                h,
                duration,
                start,
                end,
                GhostStyle {
                    tint,
                    edge,
                    label: if text { "Text" } else { "Motion" },
                },
            );
        }
    }

    let has_selection = selected.is_some();
    for (index, (start, end)) in clips.iter().enumerate() {
        let x0 = time_to_x(*start, duration, w);
        let x1 = time_to_x(*end, duration, w);
        let clip_w = (x1 - x0).max(22.0);
        let y = if text { 6.0 } else { 7.0 };
        let clip_h = if text { h - 12.0 } else { h - 14.0 };
        let is_selected = selected == Some(index);
        let tone = if text {
            text_clip_tone(is_selected, has_selection && !is_selected)
        } else {
            motion_clip_tone(is_selected, has_selection && !is_selected)
        };
        rounded_rect(context, x0, y, clip_w, clip_h, 5.0);
        context.set_source_rgba(tone.fill.0, tone.fill.1, tone.fill.2, tone.fill.3);
        let _ = context.fill();
        if is_selected {
            rounded_rect(
                context,
                x0 + 0.5,
                y + 0.5,
                (clip_w - 1.0).max(0.0),
                (clip_h - 1.0).max(0.0),
                4.5,
            );
            context.set_source_rgba(tone.edge.0, tone.edge.1, tone.edge.2, tone.edge.3);
            context.set_line_width(1.5);
            let _ = context.stroke();
            draw_trim_handles(context, x0, y, clip_w, clip_h, tone);
        }
        if clip_w > 32.0 {
            let _ = context.save();
            rounded_rect(context, x0, y, clip_w, clip_h, 5.0);
            context.clip();
            let center_y = y + clip_h / 2.0;
            draw_clip_glyph(context, x0 + 12.0, center_y, tone.label, text);
            if clip_w > 42.0 {
                let _ = context.save();
                context.rectangle(x0 + 27.0, y + 2.0, (clip_w - 40.0).max(0.0), clip_h - 4.0);
                context.clip();
                context.set_source_rgba(1.0, 1.0, 1.0, tone.label);
                context.select_font_face(
                    "Inter",
                    gtk4::cairo::FontSlant::Normal,
                    gtk4::cairo::FontWeight::Normal,
                );
                context.set_font_size(11.0);
                context.move_to(x0 + 28.0, y + clip_h * if text { 0.64 } else { 0.62 });
                let _ = context.show_text(&t(if text { "Text" } else { "Motion" }));
                let _ = context.restore();
            }
            let _ = context.restore();
        }
    }
}

/// Empty-lane affordance: a ghost of the clip the next click will create,
/// starting at the red hover hairline and running for a default clip.
/// One ghost's look: the lane's fill colour, its edge, and the label the
/// clip would carry.
struct GhostStyle {
    tint: (f64, f64, f64),
    edge: (f64, f64, f64),
    label: &'static str,
}

fn draw_add_track(
    context: &CairoContext,
    w: f64,
    h: f64,
    duration: f64,
    start: f64,
    end: f64,
    GhostStyle { tint, edge, label }: GhostStyle,
) {
    let x0 = time_to_x(start, duration, w);
    let x1 = time_to_x(end, duration, w);
    let clip_w = (x1 - x0).max(22.0);
    let y = 7.0;
    let clip_h = (h - 14.0).max(1.0);
    rounded_rect(context, x0, y, clip_w, clip_h, 5.0);
    context.set_source_rgba(tint.0, tint.1, tint.2, 0.18);
    let _ = context.fill_preserve();
    context.set_source_rgba(edge.0, edge.1, edge.2, 0.45);
    context.set_line_width(1.0);
    let _ = context.stroke();

    context.select_font_face(
        "Inter",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(11.0);
    let label_ext = context.text_extents(label).ok();
    let label_w = label_ext.as_ref().map(|ext| ext.width()).unwrap_or(0.0);
    let plus_r = 5.0;
    let content_w = plus_r * 2.0 + 7.0 + label_w;
    let cy = y + clip_h / 2.0;

    context.set_source_rgba(0.94, 0.96, 1.0, 0.9);
    context.set_line_width(1.6);
    let cx = if content_w + 16.0 <= clip_w {
        x0 + (clip_w - content_w) / 2.0 + plus_r
    } else {
        x0 + clip_w / 2.0
    };
    context.move_to(cx - plus_r, cy);
    context.line_to(cx + plus_r, cy);
    context.move_to(cx, cy - plus_r);
    context.line_to(cx, cy + plus_r);
    let _ = context.stroke();

    if content_w + 16.0 <= clip_w {
        if let Some(ext) = label_ext {
            context.move_to(
                x0 + (clip_w - content_w) / 2.0 + plus_r * 2.0 + 7.0,
                cy - ext.y_bearing() - ext.height() / 2.0,
            );
            let _ = context.show_text(label);
        }
    }
}

/// The clip glyph: the lane's kind, drawn like the Linux painter draws.
fn draw_clip_glyph(context: &CairoContext, x: f64, center_y: f64, opacity: f64, text: bool) {
    let _ = context.save();
    context.translate(x, center_y - 7.0);
    context.scale(14.0 / 24.0, 14.0 / 24.0);
    context.set_source_rgba(1.0, 1.0, 1.0, opacity);
    context.set_line_width(2.0);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    if text {
        context.move_to(12.0, 4.0);
        context.line_to(12.0, 20.0);
        context.move_to(4.0, 7.0);
        context.line_to(4.0, 5.0);
        context.arc(
            5.0,
            5.0,
            1.0,
            std::f64::consts::PI,
            3.0 * std::f64::consts::FRAC_PI_2,
        );
        context.line_to(19.0, 4.0);
        context.arc(
            19.0,
            5.0,
            1.0,
            3.0 * std::f64::consts::FRAC_PI_2,
            std::f64::consts::TAU,
        );
        context.line_to(20.0, 7.0);
        context.move_to(9.0, 20.0);
        context.line_to(15.0, 20.0);
    } else {
        for (cx, cy) in [(19.0, 5.0), (5.0, 19.0)] {
            context.new_sub_path();
            context.arc(cx, cy, 2.0, 0.0, std::f64::consts::TAU);
        }
        context.new_sub_path();
        context.arc(
            17.0,
            17.0,
            12.0,
            std::f64::consts::PI,
            3.0 * std::f64::consts::FRAC_PI_2,
        );
    }
    let _ = context.stroke();
    let _ = context.restore();
}

/// Edge grips that advertise trim-dragging on the selected clip. They
/// render only with a selection, which keeps unselected lanes quiet.
fn draw_trim_handles(
    context: &CairoContext,
    x0: f64,
    y: f64,
    clip_w: f64,
    clip_h: f64,
    tone: ClipTone,
) {
    let handle_h = (clip_h - 12.0).max(10.0);
    let handle_y = y + (clip_h - handle_h) / 2.0;
    context.set_source_rgba(tone.handle.0, tone.handle.1, tone.handle.2, tone.handle.3);
    for handle_x in [x0 + 6.0, x0 + clip_w - 9.0] {
        rounded_rect(context, handle_x, handle_y, 3.0, handle_h, 2.0);
        let _ = context.fill();
    }
}

/// Pointer read-out line. Unlike the playhead it has no capsule, so it is
/// drawn under everything and updates on every motion event cheaply.
fn draw_hover_playhead(
    context: &CairoContext,
    width: i32,
    height: i32,
    hover_time: f64,
    duration: f64,
) {
    let w = f64::from(width.max(1));
    let h = f64::from(height.max(1));
    let x = time_to_x(hover_time, duration.max(0.001), w).floor() + 0.5;
    context.set_source_rgba(0.80, 0.22, 0.20, 0.95);
    context.set_line_width(2.0);
    context.move_to(x, 0.0);
    context.line_to(x, h);
    let _ = context.stroke();
}

fn draw_playhead(context: &CairoContext, width: i32, height: i32, model: &Model, light: bool) {
    let w = f64::from(width.max(1));
    let h = f64::from(height.max(1));
    let x = time_to_x(model.playhead, model.duration.max(0.001), w).floor() + 0.5;

    // Stem: thin line running from the handle to the bottom of the board.
    let (stem_r, stem_g, stem_b) = if light {
        (0.07, 0.08, 0.09)
    } else {
        (0.86, 0.90, 0.98)
    };
    context.set_source_rgba(stem_r, stem_g, stem_b, 0.9);
    context.set_line_width(1.5);
    context.move_to(x, PLAYHEAD_HANDLE_TOP + PLAYHEAD_HANDLE_H);
    context.line_to(x, h);
    let _ = context.stroke();

    // The stem must pierce the capsule's center even on the first/last
    // frame: clamp the stem, not the capsule, and let the capsule clip.
    let hx = x - PLAYHEAD_HANDLE_W / 2.0;
    rounded_rect(
        context,
        hx,
        PLAYHEAD_HANDLE_TOP,
        PLAYHEAD_HANDLE_W,
        PLAYHEAD_HANDLE_H,
        PLAYHEAD_HANDLE_H / 2.0,
    );
    if light {
        context.set_source_rgba(0.94, 0.96, 1.0, 1.0);
    } else {
        context.set_source_rgba(0.04, 0.05, 0.07, 1.0);
    }
    let _ = context.fill_preserve();
    context.set_source_rgba(stem_r, stem_g, stem_b, 1.0);
    context.set_line_width(1.5);
    let _ = context.stroke();
}

pub(super) fn format_clock(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as i64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// A rounded stroke-and-fill path helper, same corner recipe everywhere.
fn rounded_rect(context: &CairoContext, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    context.new_path();
    context.move_to(x + r, y);
    context.line_to(x + w - r, y);
    context.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    context.line_to(x + w, y + h - r);
    context.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    context.line_to(x + r, y + h);
    context.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.line_to(x, y + r);
    context.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    context.close_path();
}
