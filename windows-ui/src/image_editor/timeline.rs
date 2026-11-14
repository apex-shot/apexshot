//! The Motion timeline dock, ported from the Linux editor's
//! `motion_timeline.rs`. Ruler, source lane, Motion lane, Text lane and a
//! painted playhead, in the video editor card's language.
//!
//! Motion clips use the same model as the Linux editor.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use gtk4::cairo::Context as CairoContext;
use gtk4::cairo::ImageSurface;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, DrawingArea, Image, Label, Orientation, Overlay};

use super::ui_support::px;
use crate::capture::editor::window::icon_names::custom;
use crate::i18n::t;
use crate::recording::editor::model::{
    MotionBlurBudgetMode, MotionState, MotionTransform, DEFAULT_MOTION_DURATION_SECONDS,
};

mod clip;

/// The shipped composition length and its bounds, the Linux model's
/// constants.
const DEFAULT_DURATION: f64 = DEFAULT_MOTION_DURATION_SECONDS;

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

/// The source badge's cast: a blurred silhouette with enough pad for the
/// blur to fall off before the surface edge.
const BADGE_SHADOW_PAD: f64 = 10.0;
const BADGE_SHADOW_ICON: f64 = 20.0;

thread_local! {
    static BADGE_SHADOW: OnceCell<Option<ImageSurface>> = const { OnceCell::new() };
}

/// Which lane the board pointer is over.
#[derive(Clone, Copy, Debug, PartialEq)]
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
            label: 0.92,
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

type Model = MotionState;
type PreviewRange = (f64, f64);
type ThumbnailSource = Rc<dyn Fn() -> Option<ImageSurface>>;

/// What the painters and handlers share: the mutable model plus the hover
/// read-out.
struct Shared {
    model: RefCell<Model>,
    hover_lane: Cell<HoverLane>,
    hover_time: Cell<Option<f64>>,
    playing: Cell<bool>,
    preview_changed: RefCell<Option<Rc<dyn Fn()>>>,
    state_changed: RefCell<Option<Rc<dyn Fn()>>>,
    undo: RefCell<Vec<MotionState>>,
    redo: RefCell<Vec<MotionState>>,
    interaction: RefCell<Option<MotionState>>,
    preview_on_finish: Cell<bool>,
    dragging_clip: Cell<bool>,
    /// A ruler drag owns the head: hover must not steer the preview then, or
    /// every pointer event would ask for a second frame behind the scrub.
    scrubbing: Cell<bool>,
    /// The still the source lane repeats as a filmstrip; absent before an
    /// image is loaded.
    thumbnail: RefCell<Option<ThumbnailSource>>,
    source_revision: Cell<u64>,
}

impl Shared {
    fn hover_previewing(&self) -> bool {
        if self.playing.get() || self.hover_lane.get() != HoverLane::Motion {
            return false;
        }
        self.hover_time
            .get()
            .is_some_and(|time| self.model.borrow().segment_index_at(time).is_some())
    }

    fn preview_time(&self) -> f64 {
        if self.hover_previewing() {
            if let Some(time) = self.hover_time.get() {
                return time;
            }
        }
        self.model.borrow().playhead
    }

    fn notify_preview(&self) {
        if let Some(callback) = self.preview_changed.borrow().as_ref() {
            callback();
        }
    }

    fn notify_state(&self) {
        if let Some(callback) = self.state_changed.borrow().as_ref() {
            callback();
        }
    }

    fn record_edit(&self, before: MotionState) {
        if self.interaction.borrow().is_none() && *self.model.borrow() != before {
            let mut undo = self.undo.borrow_mut();
            undo.push(before);
            if undo.len() > 100 {
                undo.remove(0);
            }
            self.redo.borrow_mut().clear();
        }
    }
}

/// The dock and the handles its rows need for interaction wiring.
pub(super) struct Timeline {
    pub dock: GtkBox,
    update_duration: Rc<dyn Fn(f64)>,
    pause: Rc<dyn Fn()>,
    reset_source: Rc<dyn Fn()>,
    redraw: Rc<dyn Fn()>,
    update_clock: Rc<dyn Fn()>,
    start_playback: Rc<dyn Fn(Option<PreviewRange>)>,
    /// One step of the Motion history, shared by the dock's Undo/Redo
    /// buttons and the window's Ctrl+Z / Ctrl+Y shortcuts.
    history_step: Rc<dyn Fn(bool)>,
    shared: Rc<Shared>,
}

impl Timeline {
    /// Resize the composition the ruler, lanes and hover read-out map
    /// against; the clock label follows.
    pub(super) fn set_duration(&self, seconds: f64) {
        (self.update_duration)(seconds);
    }

    /// Step the Motion history back one snapshot, the same path the dock's
    /// Undo button takes.
    pub(super) fn undo(&self) {
        (self.history_step)(true);
    }

    /// Step the Motion history forward again after an undo.
    pub(super) fn redo(&self) {
        (self.history_step)(false);
    }

    pub(super) fn preview_frames(
        &self,
        width: f64,
        height: f64,
    ) -> Vec<(MotionTransform, (f64, f64))> {
        let time = self.shared.preview_time();
        let model = self.shared.model.borrow();
        motion_blur_frames(
            &model,
            time,
            width,
            height,
            MotionBlurBudgetMode::LivePreviewPlayback,
            5,
        )
    }

    pub(super) fn motion_state(&self) -> MotionState {
        self.shared.model.borrow().clone()
    }

    /// The time the preview is showing: the playhead, or the hover frame
    /// while the pointer is over the Motion lane.
    pub(super) fn preview_time(&self) -> f64 {
        self.shared.preview_time()
    }

    pub(super) fn set_state_listener(&self, listener: Rc<dyn Fn()>) {
        *self.shared.state_changed.borrow_mut() = Some(listener);
    }

    pub(super) fn edit_motion(&self, edit: impl FnOnce(&mut MotionState), show_pose: bool) {
        self.pause();
        let mut model = self.shared.model.borrow_mut();
        let before = model.clone();
        edit(&mut model);
        let changed = *model != before;
        let range = if show_pose && changed {
            model.selected_segment().map(motion_preview_range)
        } else {
            None
        };
        if show_pose {
            if let Some(segment) = model.selected_segment() {
                model.playhead =
                    segment.start + segment.timing.transition_duration.min(segment.duration());
            }
        }
        drop(model);
        self.shared.record_edit(before);
        (self.update_clock)();
        (self.redraw)();
        self.shared.notify_preview();
        self.shared.notify_state();
        if let Some(range) = range {
            if self.shared.interaction.borrow().is_some() {
                self.shared.preview_on_finish.set(true);
            } else {
                (self.start_playback)(Some(range));
            }
        }
    }

    pub(super) fn begin_edit(&self) {
        let mut interaction = self.shared.interaction.borrow_mut();
        if interaction.is_none() {
            *interaction = Some(self.shared.model.borrow().clone());
            self.shared.preview_on_finish.set(false);
        }
    }

    pub(super) fn finish_edit(&self) {
        let before = self.shared.interaction.borrow_mut().take();
        if let Some(before) = before {
            self.shared.record_edit(before);
            (self.redraw)();
            // The drag painted interactive frames only. A closing playback
            // repaints its parked frame itself; otherwise the release owes the
            // preview that one sharp frame.
            let range = if self.shared.preview_on_finish.replace(false) {
                self.shared
                    .model
                    .borrow()
                    .selected_segment()
                    .map(motion_preview_range)
            } else {
                None
            };
            match range {
                Some(range) => (self.start_playback)(Some(range)),
                None => self.shared.notify_preview(),
            }
        }
    }

    pub(super) fn set_preview_listener(&self, listener: Rc<dyn Fn()>) {
        *self.shared.preview_changed.borrow_mut() = Some(listener);
    }

    /// The source lane repeats this surface across the composition. The
    /// provider is called at paint time so it can answer with the cached
    /// card of the current image and annotations.
    pub(super) fn set_thumbnail_source(&self, provider: ThumbnailSource) {
        *self.shared.thumbnail.borrow_mut() = Some(provider);
    }

    /// Whether the preview frame is being repainted while the head or the
    /// pointer moves, so the painter can keep the cheap interactive shadow.
    /// A control drag counts: every inspector slider and pad paints a frame
    /// per pointer event through `edit_motion`, and the parked treatment of
    /// the wallpaper, card and shadow is far too slow for that.
    pub(super) fn is_live(&self) -> bool {
        self.shared.scrubbing.get()
            || self.shared.playing.get()
            || self.shared.hover_previewing()
            || self.shared.interaction.borrow().is_some()
    }

    pub(super) fn pause(&self) {
        (self.pause)();
    }

    /// Drop the timed Motion edits when the editor leaves Motion. Appearance,
    /// watermark and frame settings are shared with Static, so they stay.
    pub(super) fn clear_clips(&self) {
        self.pause();
        let mut model = self.shared.model.borrow_mut();
        model.segments.clear();
        model.text_segments.clear();
        model.selected = None;
        model.selected_text = None;
        model.playhead = 0.0;
        drop(model);
        self.shared.undo.borrow_mut().clear();
        self.shared.redo.borrow_mut().clear();
        self.shared.interaction.borrow_mut().take();
        self.shared.preview_on_finish.set(false);
        self.shared.dragging_clip.set(false);
        (self.update_clock)();
        (self.redraw)();
        self.shared.notify_preview();
        self.shared.notify_state();
    }

    pub(super) fn reset_source(&self) {
        (self.reset_source)();
    }
}

/// The head a stopped transport lands on. A full composition run rewinds to
/// the start, the way the Linux transport does, so the next Play begins at the
/// top and the preview shows the opening pose. A range-bounded preview — the
/// short run a freshly added Motion clip starts — parks on its own end instead,
/// keeping the pose that clip finishes on.
fn playhead_when_stopped(range: Option<PreviewRange>) -> Option<f64> {
    range.is_none().then_some(0.0)
}

/// Rebuild the dock. `dark` decides the ruler and playhead tones once, the
/// way the Linux builder decides at build time.
pub(super) fn build(scale: f64, dark: bool) -> Timeline {
    let light = !dark;
    let shared = Rc::new(Shared {
        model: RefCell::new(Model::default()),
        hover_lane: Cell::new(HoverLane::None),
        hover_time: Cell::new(None),
        playing: Cell::new(false),
        preview_changed: RefCell::new(None),
        state_changed: RefCell::new(None),
        undo: RefCell::new(Vec::new()),
        redo: RefCell::new(Vec::new()),
        interaction: RefCell::new(None),
        preview_on_finish: Cell::new(false),
        dragging_clip: Cell::new(false),
        scrubbing: Cell::new(false),
        thumbnail: RefCell::new(None),
        source_revision: Cell::new(0),
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
        scale,
    );
    add_btn.add_css_class("recording-editor-timeline-tool-active");
    let add_text_btn = labeled_tool_button(
        custom::TYPE_SYMBOLIC,
        &t("Text"),
        &t("Add text at playhead"),
        scale,
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
        let shared = shared.clone();
        let undo_btn = undo_btn.downgrade();
        let redo_btn = redo_btn.downgrade();
        move || {
            ruler.queue_draw();
            source_track.queue_draw();
            motion_track.queue_draw();
            text_track.queue_draw();
            hover_playhead.queue_draw();
            playhead.queue_draw();
            if let Some(button) = undo_btn.upgrade() {
                button.set_sensitive(!shared.undo.borrow().is_empty());
            }
            if let Some(button) = redo_btn.upgrade() {
                button.set_sensitive(!shared.redo.borrow().is_empty());
            }
        }
    };
    let redraw: Rc<dyn Fn()> = Rc::new(redraw);

    // A moving head changes the head overlays and the ruler's near-head tick
    // emphasis, and nothing else. Keeping this apart from `redraw` means a
    // scrub or a playback tick never re-lays a clip lane.
    let redraw_playhead: Rc<dyn Fn()> = {
        let ruler = ruler.clone();
        let playhead = playhead.clone();
        Rc::new(move || {
            ruler.queue_draw();
            playhead.queue_draw();
        })
    };

    // ── The head's clock mirrors the model; the label is outside the draw
    // pass so a redraw chain never repaints widget text.
    let set_playhead_label: Rc<dyn Fn()> = {
        let play_label = play_label.clone();
        let shared = shared.clone();
        Rc::new(move || {
            play_label.set_text(&format_clock(shared.model.borrow().playhead));
        })
    };

    // ── Transport ──
    // The head is the model's playhead; a scrub, a skip button and playback
    // all write through `move_playhead`, which repaints the head surfaces and
    // asks the preview for one frame.
    let move_playhead: Rc<dyn Fn(f64)> = {
        let shared = shared.clone();
        let redraw_playhead = redraw_playhead.clone();
        let set_playhead_label = set_playhead_label.clone();
        let play_button = play_button.clone();
        Rc::new(move |time: f64| {
            shared.playing.set(false);
            play_button.set_icon_name("media-playback-start-symbolic");
            let mut model = shared.model.borrow_mut();
            let next = time.clamp(0.0, model.duration);
            if (next - model.playhead).abs() < 1e-9 {
                return;
            }
            model.playhead = next;
            drop(model);
            set_playhead_label();
            redraw_playhead();
            shared.notify_preview();
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

    // One generation owns the tick callback, including short automatic
    // previews of newly created motion segments.
    let playback_generation = Rc::new(Cell::new(0_u64));
    let start_playback: Rc<dyn Fn(Option<PreviewRange>)> = {
        let shared = shared.clone();
        let set_playhead_label = set_playhead_label.clone();
        let redraw_playhead = redraw_playhead.clone();
        let button = play_button.downgrade();
        let generation = playback_generation.clone();
        Rc::new(move |range: Option<(f64, f64)>| {
            let Some(button) = button.upgrade() else {
                return;
            };
            let token = generation.get().wrapping_add(1);
            generation.set(token);
            let end = {
                let mut model = shared.model.borrow_mut();
                if let Some((start, _)) = range {
                    model.playhead = start;
                } else if model.playhead >= model.duration {
                    model.playhead = 0.0;
                }
                range.map(|(_, end)| end).unwrap_or(model.duration)
            };
            if range.is_some() {
                shared.hover_time.set(None);
                shared.hover_lane.set(HoverLane::None);
            }
            shared.playing.set(true);
            button.set_icon_name("media-playback-pause-symbolic");
            set_playhead_label();
            redraw_playhead();
            shared.notify_preview();
            let shared = shared.clone();
            let set_playhead_label = set_playhead_label.clone();
            let redraw_playhead = redraw_playhead.clone();
            let generation = generation.clone();
            let last_frame = Cell::new(None::<i64>);
            button.add_tick_callback(move |button, frame_clock| {
                if !shared.playing.get() || generation.get() != token {
                    return glib::ControlFlow::Break;
                }
                let frame_time = frame_clock.frame_time();
                let Some(previous) = last_frame.replace(Some(frame_time)) else {
                    return glib::ControlFlow::Continue;
                };
                let span = frame_time.saturating_sub(previous) as f64 / 1_000_000.0;
                let parked = {
                    let mut model = shared.model.borrow_mut();
                    let end = end.min(model.duration);
                    model.playhead = (model.playhead + span).min(end);
                    model.playhead >= end
                };
                if parked {
                    shared.playing.set(false);
                    button.set_icon_name("media-playback-start-symbolic");
                    if let Some(playhead) = playhead_when_stopped(range) {
                        shared.model.borrow_mut().playhead = playhead;
                    }
                }
                set_playhead_label();
                redraw_playhead();
                shared.notify_preview();
                if parked {
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
        })
    };
    play_button.connect_clicked({
        let shared = shared.clone();
        let start_playback = start_playback.clone();
        move |button| {
            if shared.playing.replace(false) {
                button.set_icon_name("media-playback-start-symbolic");
                shared.notify_preview();
            } else {
                start_playback(None);
            }
        }
    });

    let pause: Rc<dyn Fn()> = {
        let shared = shared.clone();
        let play_button = play_button.downgrade();
        let redraw = redraw.clone();
        Rc::new(move || {
            shared.playing.set(false);
            shared.hover_time.set(None);
            shared.hover_lane.set(HoverLane::None);
            if let Some(button) = play_button.upgrade() {
                button.set_icon_name("media-playback-start-symbolic");
            }
            redraw();
            shared.notify_preview();
        })
    };

    // ── Add buttons ──
    // A click adds the lane's default clip at the playhead when the gap
    // allows it, the same span the hover ghost promises.
    let add_clip: Rc<dyn Fn(bool)> = {
        let shared = shared.clone();
        let redraw = redraw.clone();
        let start_playback = start_playback.clone();
        Rc::new(move |text: bool| {
            let mut model = shared.model.borrow_mut();
            let playhead = model.playhead;
            let before = model.clone();
            let preview = if text {
                model.add_text_at(playhead);
                None
            } else {
                model
                    .add_segment_at(playhead)
                    .map(|index| motion_preview_range(&model.segments[index]))
            };
            drop(model);
            shared.record_edit(before);
            redraw();
            shared.notify_state();
            shared.notify_preview();
            if let Some(range) = preview {
                start_playback(Some(range));
            }
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
    // A scrub owns the head for its length: it pauses playback and drops the
    // hover read-out, so the preview follows one time (the head's) instead of
    // racing the pointer, and the release paints the parked frame sharply.
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
    let begin_scrub: Rc<dyn Fn()> = {
        let shared = shared.clone();
        let pause = pause.clone();
        Rc::new(move || {
            if !shared.scrubbing.replace(true) {
                (pause)();
            }
        })
    };
    let end_scrub: Rc<dyn Fn()> = {
        let shared = shared.clone();
        Rc::new(move || {
            if shared.scrubbing.replace(false) {
                shared.notify_preview();
            }
        })
    };
    {
        let scrub_click = scrub.clone();
        let dragging = Rc::new(Cell::new(false));
        let click = gtk4::GestureClick::new();
        click.set_button(1);
        click.connect_pressed({
            let begin_scrub = begin_scrub.clone();
            move |_, _, x, _| {
                begin_scrub();
                scrub_click(x);
            }
        });
        click.connect_released({
            let dragging = dragging.clone();
            let end_scrub = end_scrub.clone();
            move |_, _, _, _| {
                if !dragging.get() {
                    end_scrub();
                }
            }
        });
        ruler.add_controller(click);
        let drag = gtk4::GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin({
            let scrub_begin = scrub.clone();
            let begin_scrub = begin_scrub.clone();
            let dragging = dragging.clone();
            move |_, x, _| {
                dragging.set(true);
                begin_scrub();
                scrub_begin(x);
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
        drag.connect_drag_end({
            let dragging = dragging.clone();
            let end_scrub = end_scrub.clone();
            move |_, _, _| {
                dragging.set(false);
                end_scrub();
            }
        });
        drag.connect_cancel({
            let dragging = dragging.clone();
            move |_, _| {
                dragging.set(false);
                end_scrub();
            }
        });
        ruler.add_controller(drag);
    }

    // ── Lanes ──
    // A click selects an existing clip or creates one in an available gap.
    for (row, text) in [(&motion_track, false), (&text_track, true)] {
        let shared = shared.clone();
        let redraw = redraw.clone();
        let start_playback = start_playback.clone();
        let set_playhead_label = set_playhead_label.clone();
        let row = row.clone();
        let dragged = Rc::new(Cell::new(false));
        let click = gtk4::GestureClick::new();
        click.set_button(1);
        click.connect_released({
            let row = row.clone();
            let dragged = dragged.clone();
            let shared = shared.clone();
            let redraw = redraw.clone();
            let start_playback = start_playback.clone();
            let set_playhead_label = set_playhead_label.clone();
            move |_, _, x, _| {
                if dragged.replace(false) {
                    return;
                }
                let width = f64::from(row.allocated_width().max(1));
                let mut model = shared.model.borrow_mut();
                let before = model.clone();
                let count = model.segments.len() + model.text_segments.len();
                let preview = activate_lane(&mut model, text, x, width);
                let added = count != model.segments.len() + model.text_segments.len();
                drop(model);
                if added {
                    shared.record_edit(before);
                }
                set_playhead_label();
                redraw();
                shared.notify_state();
                shared.notify_preview();
                if let Some(range) = preview {
                    start_playback(Some(range));
                }
            }
        });
        row.add_controller(click);
        clip::install(
            &row,
            text,
            scale,
            clip::Hooks {
                shared: shared.clone(),
                redraw: redraw.clone(),
                pause: pause.clone(),
                start_playback: start_playback.clone(),
                dragged,
            },
        );
    }

    // Track the whole board, including gaps between rows, in board coordinates.
    let hover = gtk4::EventControllerMotion::new();
    hover.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let on_hover: Rc<dyn Fn(f64, f64)> = {
        let shared = shared.clone();
        let board = board.downgrade();
        let motion_track = motion_track.clone();
        let text_track = text_track.clone();
        let hover_playhead = hover_playhead.clone();
        Rc::new(move |x, y| {
            if shared.dragging_clip.get() || shared.scrubbing.get() {
                return;
            }
            let Some(board) = board.upgrade() else { return };
            let previous = shared.preview_time();
            let old_lane = shared.hover_lane.get();
            let lane = if pointer_in_lane(&motion_track, &board, y) {
                HoverLane::Motion
            } else if pointer_in_lane(&text_track, &board, y) {
                HoverLane::Text
            } else {
                HoverLane::None
            };
            let duration = shared.model.borrow().duration;
            let width = f64::from(board.allocated_width().max(1));
            shared.hover_lane.set(lane);
            shared
                .hover_time
                .set(Some((x / width).clamp(0.0, 1.0) * duration));
            hover_playhead.queue_draw();
            if old_lane == HoverLane::Motion || lane == HoverLane::Motion {
                motion_track.queue_draw();
            }
            if old_lane == HoverLane::Text || lane == HoverLane::Text {
                text_track.queue_draw();
            }
            if (previous - shared.preview_time()).abs() > f64::EPSILON {
                shared.notify_preview();
            }
        })
    };
    hover.connect_enter({
        let on_hover = on_hover.clone();
        move |_, x, y| on_hover(x, y)
    });
    hover.connect_motion(move |_, x, y| on_hover(x, y));
    hover.connect_leave({
        let shared = shared.clone();
        let motion_track = motion_track.clone();
        let text_track = text_track.clone();
        let hover_playhead = hover_playhead.clone();
        move |_| {
            if shared.scrubbing.get() {
                return;
            }
            let previous = shared.preview_time();
            shared.hover_lane.set(HoverLane::None);
            shared.hover_time.set(None);
            hover_playhead.queue_draw();
            motion_track.queue_draw();
            text_track.queue_draw();
            if (previous - shared.preview_time()).abs() > f64::EPSILON {
                shared.notify_preview();
            }
        }
    });
    board.add_controller(hover);

    // ── Painters ──
    ruler.set_draw_func({
        let shared = shared.clone();
        move |_, context, width, height| {
            let model = shared.model.borrow();
            draw_ruler(context, width, height, &model, light, scale);
        }
    });
    source_track.set_draw_func({
        let shared = shared.clone();
        move |_, context, width, height| {
            let thumbnail = shared
                .thumbnail
                .borrow()
                .as_ref()
                .and_then(|provider| provider());
            let model = shared.model.borrow();
            draw_source_track(context, width, height, &model, thumbnail.as_ref(), scale);
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
                scale,
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
            draw_playhead(context, width, height, &model, light, scale);
        }
    });

    // The composition length lives behind the Duration slider in the Motion
    // inspector; this closure applies it to the model, clock and ruler.
    let update_duration: Rc<dyn Fn(f64)> = {
        let shared = shared.clone();
        let redraw = redraw.clone();
        let duration_label = duration_label.clone();
        let set_playhead_label = set_playhead_label.clone();
        Rc::new(move |seconds: f64| {
            let seconds = seconds.clamp(1.0, 10.0);
            let changed = {
                let mut model = shared.model.borrow_mut();
                if (model.duration - seconds).abs() < 1e-6 {
                    false
                } else {
                    let before = model.clone();
                    model.set_duration(seconds);
                    drop(model);
                    shared.record_edit(before);
                    true
                }
            };
            if changed {
                duration_label.set_text(&format_clock(seconds));
                set_playhead_label();
                redraw();
                shared.notify_preview();
                shared.notify_state();
            }
        })
    };

    let reset_source: Rc<dyn Fn()> = {
        let shared = shared.clone();
        let pause = pause.clone();
        let redraw = redraw.clone();
        let set_playhead_label = set_playhead_label.clone();
        Rc::new(move || {
            pause();
            let mut model = shared.model.borrow_mut();
            let duration = model.duration;
            *model = MotionState {
                duration,
                ..MotionState::default()
            };
            drop(model);
            shared.undo.borrow_mut().clear();
            shared.redo.borrow_mut().clear();
            shared.interaction.borrow_mut().take();
            shared.preview_on_finish.set(false);
            shared.dragging_clip.set(false);
            shared
                .source_revision
                .set(shared.source_revision.get().wrapping_add(1));
            set_playhead_label();
            redraw();
            shared.notify_preview();
            shared.notify_state();
        })
    };

    let history_step: Rc<dyn Fn(bool)> = {
        let shared = shared.clone();
        let redraw = redraw.clone();
        let set_playhead_label = set_playhead_label.clone();
        let duration_label = duration_label.clone();
        let pause = pause.clone();
        Rc::new(move |undo: bool| {
            pause();
            let (source, target) = if undo {
                (&shared.undo, &shared.redo)
            } else {
                (&shared.redo, &shared.undo)
            };
            let Some(snapshot) = source.borrow_mut().pop() else {
                return;
            };
            let mut model = shared.model.borrow_mut();
            target.borrow_mut().push(model.clone());
            *model = snapshot;
            let duration = model.duration;
            drop(model);
            duration_label.set_text(&format_clock(duration));
            set_playhead_label();
            redraw();
            shared.notify_preview();
            shared.notify_state();
        })
    };

    for (button, undo) in [(&undo_btn, true), (&redo_btn, false)] {
        let history_step = history_step.clone();
        button.connect_clicked(move |_| history_step(undo));
    }

    Timeline {
        dock,
        update_duration,
        pause,
        reset_source,
        redraw,
        update_clock: set_playhead_label,
        start_playback,
        history_step,
        shared,
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
fn labeled_tool_button(icon_name: &str, label: &str, tooltip: &str, scale: f64) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-timeline-tool");
    button.set_tooltip_text(Some(tooltip));
    let row = GtkBox::new(Orientation::Horizontal, 6);
    let text = Label::new(Some(label));
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(px(18.0, scale));
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

fn pointer_in_lane(lane: &DrawingArea, board: &Overlay, y: f64) -> bool {
    lane.translate_coordinates(board, 0.0, 0.0)
        .is_some_and(|(_, top)| y >= top && y <= top + f64::from(lane.allocated_height()))
}

/// The poses across the exposure window that ends at `time`, one per
/// temporal sample, capped at `max_samples`. No blur or no travel yields the
/// single current pose, which callers paint without compositing.
pub(super) fn motion_blur_frames(
    model: &MotionState,
    time: f64,
    width: f64,
    height: f64,
    budget: MotionBlurBudgetMode,
    max_samples: usize,
) -> Vec<(MotionTransform, (f64, f64))> {
    let settings = model.motion_blur_settings;
    let current = model.sample(time);
    let previous = model.sample((time - settings.exposure_seconds(30.0)).max(0.0));
    let travel = width.hypot(height)
        * ((current.scale - previous.scale).abs() * 0.5
            + (current.pos_x - previous.pos_x).abs() * 0.5
            + (current.pos_y - previous.pos_y).abs() * 0.5
            + (current.rotation_x - previous.rotation_x)
                .abs()
                .to_radians()
            + (current.rotation_y - previous.rotation_y)
                .abs()
                .to_radians()
            + (current.rotation_z - previous.rotation_z)
                .abs()
                .to_radians());
    let offsets = settings.temporal_offsets(30.0, travel, budget);
    if offsets.is_empty() {
        return vec![(current, model.zoom_anchor_at(time))];
    }
    let count = offsets.len().min(max_samples);
    let last = *offsets.last().unwrap_or(&0.0);
    (0..count)
        .map(|index| {
            let sample_time = (time + last * index as f64 / (count - 1) as f64).max(0.0);
            (model.sample(sample_time), model.zoom_anchor_at(sample_time))
        })
        .collect()
}

fn motion_preview_range(segment: &crate::recording::editor::model::MotionSegment) -> (f64, f64) {
    let transition = segment
        .timing
        .clamped()
        .transition_duration
        .min(segment.duration());
    (
        segment.start,
        (segment.start + transition + 0.12).min(segment.end),
    )
}

fn lane_add_span(model: &Model, text: bool, time: f64, width: f64) -> Option<(f64, f64)> {
    let tolerance = 10.0 / width.max(1.0) * model.duration;
    if text {
        if model.text_index_at(time).is_some() {
            return None;
        }
        model.text_add_span(model.snap_text_time(time, tolerance, None))
    } else {
        if model.segment_index_at(time).is_some() {
            return None;
        }
        model.motion_add_span(model.snap_effect_time(time, tolerance, None))
    }
}

/// Selection uses the raw pointer position; only insertion is snapped.
fn activate_lane(model: &mut Model, text: bool, x: f64, width: f64) -> Option<(f64, f64)> {
    let width = width.max(1.0);
    let time = (x / width).clamp(0.0, 1.0) * model.duration;
    let tolerance = 10.0 / width * model.duration;
    if text {
        if let Some(index) = model.text_index_at(time) {
            model.selected_text = Some(index);
            model.selected = None;
            // Selecting a title keeps the head where it is when the head is
            // already showing that title; a head outside it lands past the
            // entrance so the preview shows more than an empty reveal.
            let playhead = model.playhead;
            let target = model.text_segments.get(index).and_then(|segment| {
                (playhead < segment.start || playhead >= segment.end)
                    .then(|| segment.start + (0.5 * (segment.end - segment.start)).min(1.0))
            });
            if let Some(target) = target {
                model.playhead = target;
            }
            None
        } else {
            let time = model.snap_text_time(time, tolerance, None);
            model.add_text_at(time).and_then(|index| {
                let segment = model.text_segments.get(index)?;
                let range = (segment.start, segment.end);
                model.playhead = range.0;
                Some(range)
            })
        }
    } else if let Some(index) = model.segment_index_at(time) {
        model.selected = Some(index);
        model.selected_text = None;
        let playhead = model.playhead;
        let target = model.segments.get(index).and_then(|segment| {
            (playhead < segment.start || playhead >= segment.end).then_some(segment.start)
        });
        if let Some(target) = target {
            model.playhead = target;
        }
        None
    } else {
        let time = model.snap_effect_time(time, tolerance, None);
        model.add_segment_at(time).map(|index| {
            let range = motion_preview_range(&model.segments[index]);
            model.playhead = model.segments[index].start;
            range
        })
    }
}

// ── Painters ────────────────────────────────────────────────────────
fn time_to_x(time: f64, duration: f64, width: f64) -> f64 {
    (time / duration.max(0.001)).clamp(0.0, 1.0) * width
}

fn draw_ruler(
    context: &CairoContext,
    width: i32,
    height: i32,
    model: &Model,
    light: bool,
    scale: f64,
) {
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
    context.set_font_size(10.0 * scale);
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

fn draw_source_track(
    context: &CairoContext,
    width: i32,
    height: i32,
    model: &Model,
    thumbnail: Option<&ImageSurface>,
    scale: f64,
) {
    let w = f64::from(width.max(1));
    let h = f64::from(height.max(1));
    let inset = 6.0;
    let y = inset;
    let clip_h = (h - inset * 2.0).max(1.0);

    rounded_rect(context, 0.0, y, w, clip_h, 5.0);
    context.set_source_rgba(0.10, 0.11, 0.13, 0.9);
    let _ = context.fill();

    // Repeated cover thumbnails make the still readable across the whole
    // duration without inventing motion the source does not have.
    if let Some(card) = thumbnail {
        let image_w = f64::from(card.width().max(1));
        let image_h = f64::from(card.height().max(1));
        let tile_w: f64 = 92.0;
        let _ = context.save();
        rounded_rect(context, 0.0, y, w, clip_h, 5.0);
        context.clip();
        let mut tile_x = 0.0;
        while tile_x < w {
            let visible_w = tile_w.min(w - tile_x);
            let scale = (visible_w / image_w).max(clip_h / image_h);
            let painted_w = image_w * scale;
            let painted_h = image_h * scale;
            let _ = context.save();
            context.rectangle(tile_x, y, visible_w, clip_h);
            context.clip();
            context.translate(
                tile_x + (visible_w - painted_w) / 2.0,
                y + (clip_h - painted_h) / 2.0,
            );
            context.scale(scale, scale);
            context.set_source_surface(card, 0.0, 0.0).ok();
            context.source().set_filter(gtk4::cairo::Filter::Bilinear);
            let _ = context.paint();
            let _ = context.restore();
            tile_x += tile_w;
        }
        let _ = context.restore();
    }

    draw_source_chip(context, w, y, clip_h, model.duration, scale);
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
        let context = CairoContext::new(&surface).ok()?;
        rounded_rect(
            &context,
            BADGE_SHADOW_PAD,
            BADGE_SHADOW_PAD,
            BADGE_SHADOW_ICON,
            BADGE_SHADOW_ICON,
            5.0,
        );
        context.set_source_rgba(0.0, 0.0, 0.0, 1.0);
        let _ = context.fill();
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
fn draw_source_chip(
    context: &CairoContext,
    width: f64,
    y: f64,
    clip_h: f64,
    duration: f64,
    scale: f64,
) {
    let title = t("Image");
    let duration_label = format!("{}s", duration.round().max(0.0) as i64);
    context.select_font_face(
        "Inter",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(11.0 * scale);
    let title_w = context
        .text_extents(&title)
        .map(|ext| ext.width())
        .unwrap_or(0.0);
    context.select_font_face(
        "Inter",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(10.0 * scale);
    let duration_w = context
        .text_extents(&duration_label)
        .map(|ext| ext.width())
        .unwrap_or(0.0);

    let icon_size = 20.0;
    let gap = 7.0;
    let block_w = icon_size + gap + title_w.max(duration_w);
    let block_h = 34.0 * scale;
    let block_x = ((width - block_w) / 2.0).clamp(8.0, (width - block_w - 8.0).max(8.0));
    let block_y = y + (clip_h - block_h) / 2.0;

    let icon_x = block_x;
    let icon_y = block_y + (block_h - icon_size) / 2.0;
    // A real blurred silhouette, not stacked rings: the badge reads as a
    // floating tag the same way the reference does.
    if let Some(shadow) = badge_shadow() {
        context
            .set_source_surface(
                &shadow,
                icon_x - BADGE_SHADOW_PAD,
                icon_y - BADGE_SHADOW_PAD + 2.0,
            )
            .ok();
        let _ = context.paint();
        let _ = context.paint();
    }
    rounded_rect(context, icon_x, icon_y, icon_size, icon_size, 5.0);
    context.set_source_rgba(0.93, 0.95, 0.99, 1.0);
    let _ = context.fill_preserve();
    context.set_source_rgba(0.0, 0.0, 0.0, 0.16);
    context.set_line_width(1.0);
    let _ = context.stroke();
    let _ = context.save();
    rounded_rect(context, icon_x, icon_y, icon_size, icon_size, 5.0);
    context.clip();
    context.set_source_rgba(0.07, 0.07, 0.09, 1.0);
    // Sun and two peaks, kept apart so the mark reads as a picture rather
    // than one solid mound.
    context.arc(icon_x + 6.4, icon_y + 6.2, 2.0, 0.0, std::f64::consts::TAU);
    let _ = context.fill();
    context.move_to(icon_x + 2.5, icon_y + 17.0);
    context.line_to(icon_x + 7.5, icon_y + 11.0);
    context.line_to(icon_x + 10.6, icon_y + 14.2);
    context.line_to(icon_x + 16.0, icon_y + 7.5);
    context.line_to(icon_x + 17.5, icon_y + 9.0);
    context.line_to(icon_x + 17.5, icon_y + 17.0);
    context.close_path();
    let _ = context.fill();
    let _ = context.restore();

    let text_x = icon_x + icon_size + gap;
    paint_source_text(context, text_x, block_y, &title, &duration_label, scale);
}

/// White type with a dark outline, the recipe that stays legible over a
/// bright still as well as a dark one.
fn paint_source_text(
    context: &CairoContext,
    text_x: f64,
    block_y: f64,
    title: &str,
    duration: &str,
    scale: f64,
) {
    context.select_font_face(
        "Inter",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(11.0 * scale);
    for (dx, dy, alpha) in [
        (0.0, 2.0, 0.10),
        (1.0, 1.0, 0.12),
        (-1.0, 1.0, 0.12),
        (0.0, 1.0, 0.22),
    ] {
        context.new_path();
        context.move_to(text_x + dx, block_y + 14.0 * scale + dy);
        context.text_path(title);
        context.set_source_rgba(0.0, 0.0, 0.0, alpha);
        let _ = context.fill();
    }
    context.new_path();
    context.move_to(text_x, block_y + 14.0 * scale);
    context.text_path(title);
    context.set_line_width(2.0);
    context.set_source_rgba(0.05, 0.05, 0.07, 0.55);
    let _ = context.stroke_preserve();
    context.set_source_rgba(1.0, 1.0, 1.0, 0.98);
    let _ = context.fill();

    context.select_font_face(
        "Inter",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(10.0 * scale);
    for (dx, dy, alpha) in [
        (0.0, 2.0, 0.10),
        (1.0, 1.0, 0.12),
        (-1.0, 1.0, 0.12),
        (0.0, 1.0, 0.22),
    ] {
        context.new_path();
        context.move_to(text_x + dx, block_y + 26.0 * scale + dy);
        context.text_path(duration);
        context.set_source_rgba(0.0, 0.0, 0.0, alpha);
        let _ = context.fill();
    }
    context.new_path();
    context.move_to(text_x, block_y + 26.0 * scale);
    context.text_path(duration);
    context.set_line_width(2.0);
    context.set_source_rgba(0.05, 0.05, 0.07, 0.55);
    let _ = context.stroke_preserve();
    context.set_source_rgba(1.0, 1.0, 1.0, 0.92);
    let _ = context.fill();
}

/// One clip lane: the hover ghost, then the clips, their glyphs and labels,
/// and the trim grips on a selected clip.
#[allow(clippy::too_many_arguments)]
fn draw_clip_track(
    context: &CairoContext,
    width: i32,
    height: i32,
    model: &Model,
    lane_paint: HoverLane,
    hover_lane: HoverLane,
    hover_time: Option<f64>,
    scale: f64,
) {
    let text = lane_paint == HoverLane::Text;
    let clips: Vec<(f64, f64, bool)> = if text {
        model
            .text_segments
            .iter()
            .map(|clip| (clip.start, clip.end, clip.is_disabled))
            .collect()
    } else {
        model
            .segments
            .iter()
            .map(|clip| (clip.start, clip.end, clip.is_disabled))
            .collect()
    };
    let selected = if text {
        model.selected_text
    } else {
        model.selected
    };
    let w = f64::from(width.max(1));
    let h = f64::from(height.max(1));
    let duration = model.duration.max(0.001);

    if hover_lane == lane_paint {
        let span = hover_time.and_then(|time| lane_add_span(model, text, time, w));
        if let Some((start, end)) = span {
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
                scale,
            );
        }
    }

    let has_selection = selected.is_some();
    for (index, (start, end, disabled)) in clips.iter().enumerate() {
        let x0 = time_to_x(*start, duration, w);
        let x1 = time_to_x(*end, duration, w);
        let clip_w = (x1 - x0).max(22.0);
        let y = if text { 6.0 } else { 7.0 };
        let clip_h = if text { h - 12.0 } else { h - 14.0 };
        let is_selected = selected == Some(index);
        let tone = disabled_clip_tone(
            if text {
                text_clip_tone(is_selected, has_selection && !is_selected)
            } else {
                motion_clip_tone(is_selected, has_selection && !is_selected)
            },
            *disabled,
        );
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
            if clip_w > 46.0 {
                let label = t(if text { "Text" } else { "Motion" });
                let _ = context.save();
                context.rectangle(x0 + 30.0, y + 2.0, (clip_w - 44.0).max(0.0), clip_h - 4.0);
                context.clip();
                context.set_source_rgba(1.0, 1.0, 1.0, tone.label);
                context.select_font_face(
                    "Inter",
                    gtk4::cairo::FontSlant::Normal,
                    gtk4::cairo::FontWeight::Bold,
                );
                context.set_font_size(12.0 * scale);
                // Centre the label's ink on the glyph's centre instead of
                // leaning on a fraction of the lane height.
                if let Ok(ext) = context.text_extents(&label) {
                    context.move_to(x0 + 30.0, center_y - ext.y_bearing() - ext.height() / 2.0);
                    let _ = context.show_text(&label);
                }
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

#[allow(clippy::too_many_arguments)]
fn draw_add_track(
    context: &CairoContext,
    w: f64,
    h: f64,
    duration: f64,
    start: f64,
    end: f64,
    GhostStyle { tint, edge, label }: GhostStyle,
    scale: f64,
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
    context.set_font_size(11.0 * scale);
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
    context.translate(x, center_y - 8.0);
    context.scale(16.0 / 24.0, 16.0 / 24.0);
    context.set_source_rgba(1.0, 1.0, 1.0, opacity);
    context.set_line_width(2.4);
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
    hover_time: Option<f64>,
    duration: f64,
) {
    let Some(hover_time) = hover_time else {
        return;
    };
    let w = f64::from(width.max(1));
    let h = f64::from(height.max(1));
    let x = time_to_x(hover_time, duration.max(0.001), w).floor() + 0.5;
    context.set_source_rgba(0.80, 0.22, 0.20, 0.95);
    context.set_line_width(2.0);
    context.move_to(x, 0.0);
    context.line_to(x, h);
    let _ = context.stroke();
}

fn draw_playhead(
    context: &CairoContext,
    width: i32,
    height: i32,
    model: &Model,
    light: bool,
    scale: f64,
) {
    let w = f64::from(width.max(1));
    let h = f64::from(height.max(1));
    let handle_w = PLAYHEAD_HANDLE_W * scale;
    let handle_h = PLAYHEAD_HANDLE_H * scale;
    let x = time_to_x(model.playhead, model.duration.max(0.001), w).floor() + 0.5;

    // Stem: thin line running from the handle to the bottom of the board.
    let (stem_r, stem_g, stem_b) = if light {
        (0.07, 0.08, 0.09)
    } else {
        (0.86, 0.90, 0.98)
    };
    context.set_source_rgba(stem_r, stem_g, stem_b, 0.9);
    context.set_line_width(1.5);
    context.move_to(x, PLAYHEAD_HANDLE_TOP + handle_h);
    context.line_to(x, h);
    let _ = context.stroke();

    // The stem must pierce the capsule's center even on the first/last
    // frame: clamp the stem, not the capsule, and let the capsule clip.
    let hx = x - handle_w / 2.0;
    rounded_rect(
        context,
        hx,
        PLAYHEAD_HANDLE_TOP,
        handle_w,
        handle_h,
        handle_h / 2.0,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_run_rewinds_to_the_start_while_a_preview_parks() {
        // Playback to the end takes the head back to 00:00 so the next Play
        // starts at the top; the automatic preview of a new clip stays on the
        // pose that clip ends with.
        assert_eq!(playhead_when_stopped(None), Some(0.0));
        assert_eq!(playhead_when_stopped(Some((1.5, 2.25))), None);
    }

    #[test]
    fn motion_and_text_clips_paint_their_lane_colour() {
        let mut model = Model::default();
        model.add_segment_at(1.0).unwrap();
        model.add_text_at(1.0).unwrap();
        model.selected = None;
        model.selected_text = None;
        for (lane, expect_blue) in [(HoverLane::Motion, true), (HoverLane::Text, false)] {
            let mut surface =
                gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 600, 56).unwrap();
            {
                let context = gtk4::cairo::Context::new(&surface).unwrap();
                draw_clip_track(&context, 600, 56, &model, lane, HoverLane::None, None, 1.0);
            }
            surface.flush();
            let stride = surface.stride() as usize;
            let data = surface.data().unwrap();
            let offset = 12 * stride + 150 * 4;
            let (blue, green, red) = (data[offset], data[offset + 1], data[offset + 2]);
            if expect_blue {
                assert!(
                    blue > 180 && red < 60 && green > 80,
                    "motion clip is azure, got r{red} g{green} b{blue}"
                );
            } else {
                assert!(
                    red > 180 && blue < 60,
                    "text clip is orange, got r{red} g{green} b{blue}"
                );
            }
        }
    }

    #[test]
    fn source_lane_repeats_the_still_as_a_filmstrip() {
        let model = Model::default();
        let thumb = ImageSurface::create(gtk4::cairo::Format::ARgb32, 40, 40).unwrap();
        {
            let context = gtk4::cairo::Context::new(&thumb).unwrap();
            context.set_source_rgba(1.0, 0.0, 0.0, 1.0);
            let _ = context.paint();
        }
        thumb.flush();
        let mut surface = ImageSurface::create(gtk4::cairo::Format::ARgb32, 600, 48).unwrap();
        {
            let context = gtk4::cairo::Context::new(&surface).unwrap();
            draw_source_track(&context, 600, 48, &model, Some(&thumb), 1.0);
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        for x in [10_usize, 110] {
            let offset = 24 * stride + x * 4;
            let (blue, green, red) = (data[offset], data[offset + 1], data[offset + 2]);
            assert!(
                red > 200 && green < 40 && blue < 40,
                "tile at x={x} shows the still, got r{red} g{green} b{blue}"
            );
        }
    }

    #[test]
    fn lane_click_adds_selects_and_snaps_real_motion_segments() {
        let mut model = Model::default();
        assert_eq!(model.motion_add_span(1.0), Some((1.0, 2.0)));
        assert_eq!(
            activate_lane(&mut model, false, 100.0, 600.0),
            Some((1.0, 2.0))
        );
        assert_eq!(model.selected, Some(0));
        assert_eq!(model.segments[0].to.scale, 2.0);
        assert!(model.sample(1.5).scale > 1.0);
        assert_eq!(lane_add_span(&model, false, 1.5, 600.0), None);
        assert_eq!(lane_add_span(&model, false, 2.05, 600.0), Some((2.0, 3.0)));

        assert_eq!(activate_lane(&mut model, false, 150.0, 600.0), None);
        assert_eq!(model.segments.len(), 1);
        assert_eq!(
            activate_lane(&mut model, false, 205.0, 600.0),
            Some((2.0, 3.0))
        );
        assert_eq!(model.segments.len(), 2);
        assert_eq!(model.selected, Some(1));
        assert_eq!(model.segments[1].from.scale, 2.0);
        assert_eq!(model.segments[1].to.scale, 1.0);

        model.set_duration(1.5);
        assert_eq!(model.segments.len(), 1);
        assert_eq!(model.segments[0].end, 1.5);
        assert_eq!(model.selected, None);
    }

    #[test]
    fn selecting_a_lane_clip_keeps_an_inside_head_and_parks_an_outside_one() {
        let mut model = Model::default();
        model.add_text_at(1.0).unwrap();
        model.add_segment_at(3.0).unwrap();

        // A head outside the title lands halfway through it, past the
        // entrance the first frames would otherwise hide.
        model.playhead = 0.0;
        assert_eq!(activate_lane(&mut model, true, 125.0, 600.0), None);
        assert_eq!(model.selected_text, Some(0));
        assert!((model.playhead - 1.5).abs() < 1e-9);

        // A head already showing the title stays where the user left it.
        model.playhead = 1.2;
        assert_eq!(activate_lane(&mut model, true, 125.0, 600.0), None);
        assert!((model.playhead - 1.2).abs() < 1e-9);

        // A motion clip parks at its start, and an inside head is untouched.
        model.playhead = 5.0;
        assert_eq!(activate_lane(&mut model, false, 325.0, 600.0), None);
        assert_eq!(model.selected, Some(0));
        assert!((model.playhead - 3.0).abs() < 1e-9);
        model.playhead = 3.4;
        assert_eq!(activate_lane(&mut model, false, 325.0, 600.0), None);
        assert!((model.playhead - 3.4).abs() < 1e-9);
    }

    #[test]
    fn clearing_clips_keeps_the_shared_appearance() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let timeline = build(1.0, true);
            timeline.edit_motion(
                |state| {
                    state.add_segment_at(0.0).unwrap();
                    state.add_text_at(0.5).unwrap();
                    state.playhead = 0.75;
                    state.perspective_intensity = 12.0;
                    state.motion_blur = 0.8;
                    state.set_duration(6.0);
                },
                false,
            );
            timeline.clear_clips();
            let model = timeline.motion_state();
            assert!(model.segments.is_empty());
            assert!(model.text_segments.is_empty());
            assert_eq!(model.playhead, 0.0);
            assert_eq!(model.perspective_intensity, 12.0);
            assert_eq!(model.motion_blur, 0.8);
            assert_eq!(model.duration, 6.0);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn hover_tracks_horizontal_pointer_and_lane_and_click_creates_clip() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let timeline = build(1.0, true);
            timeline.dock.measure(Orientation::Horizontal, -1);
            let height = timeline.dock.measure(Orientation::Vertical, 600).0.max(300);
            timeline.dock.allocate(600, height, -1, None);
            let card = timeline.dock.first_child().unwrap();
            let board = card.last_child().unwrap().downcast::<Overlay>().unwrap();
            let tracks = board.child().unwrap();
            let ruler = tracks.first_child().unwrap();
            let source = ruler.next_sibling().unwrap();
            let motion = source
                .next_sibling()
                .unwrap()
                .downcast::<DrawingArea>()
                .unwrap();
            let (_, y) = motion.translate_coordinates(&board, 0.0, 0.0).unwrap();
            let y = y + f64::from(motion.height()) / 2.0;
            let width = f64::from(board.width());
            assert!(width > 0.0);
            let controllers = board.observe_controllers();
            let hover = (0..controllers.n_items())
                .filter_map(|index| controllers.item(index))
                .find_map(|item| item.downcast::<gtk4::EventControllerMotion>().ok())
                .unwrap();
            hover.emit_by_name::<()>("enter", &[&(width * 0.25), &y]);
            assert_eq!(timeline.shared.hover_lane.get(), HoverLane::Motion);
            assert!((timeline.shared.hover_time.get().unwrap() - 1.5).abs() < 0.001);
            hover.emit_by_name::<()>("motion", &[&(width * 0.75), &y]);
            assert!((timeline.shared.hover_time.get().unwrap() - 4.5).abs() < 0.001);

            let controllers = motion.observe_controllers();
            let click = (0..controllers.n_items())
                .filter_map(|index| controllers.item(index))
                .filter_map(|item| item.downcast::<gtk4::GestureClick>().ok())
                .find(|click| click.button() == 1)
                .unwrap();
            let motion_width = f64::from(motion.width());
            click.emit_by_name::<()>("released", &[&1_i32, &(motion_width * 0.5), &10.0_f64]);
            assert_eq!(timeline.shared.model.borrow().segments.len(), 1);
            assert_eq!(timeline.shared.model.borrow().segments[0].start, 3.0);
            assert!(timeline.shared.playing.get());
            timeline.pause();
            hover.emit_by_name::<()>("motion", &[&(width * (3.5 / 6.0)), &y]);
            assert!(timeline.preview_frames(100.0, 100.0)[0].0.scale > 1.0);
            hover.emit_by_name::<()>("leave", &[]);
            assert_eq!(timeline.shared.hover_time.get(), None);
            assert_eq!(timeline.shared.hover_lane.get(), HoverLane::None);
            assert_eq!(timeline.preview_frames(100.0, 100.0)[0].0.scale, 1.0);
            timeline.reset_source();
            assert!(timeline.shared.model.borrow().segments.is_empty());
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn motion_blur_frames_honours_each_budget_and_sample_cap() {
        let mut model = Model::default();
        model.add_segment_at(0.0);
        model.playhead = 0.3;
        model.set_motion_blur(1.0);
        let preview = motion_blur_frames(
            &model,
            0.3,
            2000.0,
            1000.0,
            MotionBlurBudgetMode::LivePreviewPlayback,
            5,
        );
        let export = motion_blur_frames(
            &model,
            0.3,
            2000.0,
            1000.0,
            MotionBlurBudgetMode::FullQuality,
            16,
        );
        assert!(preview.len() > 1 && preview.len() <= 5);
        assert!(export.len() >= preview.len() && export.len() <= 16);
        assert!(export[0].0.scale > export.last().unwrap().0.scale);
        // The cap is applied after the budget picks its density.
        let capped = motion_blur_frames(
            &model,
            0.3,
            2000.0,
            1000.0,
            MotionBlurBudgetMode::FullQuality,
            2,
        );
        assert_eq!(capped.len(), 2);
    }

    #[test]
    fn blur_samples_only_when_exposure_and_motion_are_present() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let timeline = build(1.0, true);
            timeline.edit_motion(
                |state| {
                    state.add_segment_at(0.0);
                    state.playhead = 0.3;
                },
                false,
            );
            assert_eq!(timeline.preview_frames(2000.0, 1000.0).len(), 1);
            timeline.edit_motion(|state| state.set_motion_blur(1.0), false);
            let frames = timeline.preview_frames(2000.0, 1000.0);
            assert!(frames.len() > 1);
            assert!(frames.len() <= 5);
            assert!(frames[0].0.scale > frames.last().unwrap().0.scale);
            timeline.edit_motion(
                |state| state.motion_blur_settings.shutter_angle = 0.0,
                false,
            );
            assert_eq!(timeline.preview_frames(2000.0, 1000.0).len(), 1);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
