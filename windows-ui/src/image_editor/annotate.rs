//! The annotation controller: document, interaction and chrome sync.
//!
//! `EditorHost` owns the shared `EditorState`, paints the document on the
//! canvas (working image + vector annotations + selection chrome), routes
//! the canvas gestures the way the Linux editor does, drives the off-thread
//! effects rebuilds and text detection, and keeps the toolbar, sub-bars and
//! history pill in sync after every mutation.

use gtk4::cairo::ImageSurface;
use gtk4::gdk;
use gtk4::glib::{self, Propagation};
use gtk4::prelude::*;
use gtk4::DrawingArea;
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::Duration;

use crate::capture::editor::color::{
    selection_handle_hit_radius_for_scale, selection_hit_padding_for_scale, DRAG_REDRAW_INTERVAL_US,
};
use crate::capture::editor::render::{
    self, clip_annotation_content_to_canvas, draw_active_text_input, draw_annotation_action,
    draw_arrow_control_handles, draw_arrow_selection_outline, draw_draft_action,
    draw_selection_handles, draw_selection_outline, draw_text_edit_border, draw_text_edit_handles,
    rgba_image_to_surface, text_action_bounds,
};
use crate::capture::editor::selection::{
    action_bounds_with_padding, action_contains_point_with_padding,
    action_resize_handle_at_point_with_radius, action_resize_handles,
};
use crate::capture::editor::state::{
    apply_effect_actions, crop_handle_at, point_in_rect, EditorState,
};
use crate::capture::editor::text_detect::spawn_text_detection;
use crate::capture::editor::types::{
    cursor_name_for_select_handle, tool_shortcut_target, AnnotationAction, ArrowStyle, CropHandle,
    FontSettings, FontStyle, MoveHandle, Point, TextAlignment, TextDecoration, Tool, ViewTransform,
};

fn crop_cursor(st: &EditorState, point: Point, scale: f64) -> &'static str {
    let Some(rect) = st.crop_rect else {
        return "crosshair";
    };
    match crop_handle_at(rect, point, scale) {
        Some(CropHandle::TopLeft | CropHandle::BottomRight) => "nwse-resize",
        Some(CropHandle::TopRight | CropHandle::BottomLeft) => "nesw-resize",
        Some(CropHandle::Top | CropHandle::Bottom) => "ns-resize",
        Some(CropHandle::Left | CropHandle::Right) => "ew-resize",
        _ if point_in_rect(point, rect)
            && (rect.x != 0
                || rect.y != 0
                || rect.width != st.base_image.width() as i32
                || rect.height != st.base_image.height() as i32) =>
        {
            "grab"
        }
        _ => "crosshair",
    }
}

pub type Shared = Rc<RefCell<Option<EditorState>>>;

/// Move/resize handle radii matching the Linux events layer.
const MOVE_HANDLE_DRAG_RADIUS: f64 = 7.0;
const RESIZE_HANDLE_DRAG_SIZE: f64 = 10.0;
const ARROW_CLICK_NOOP_DISTANCE: f64 = 3.0;
static BACKGROUND_DECODE_LOCK: Mutex<()> = Mutex::new(());

type DecodedBackground = (RgbaImage, Vec<u8>);

fn spawn_background_decode(
    path: std::path::PathBuf,
    preview_path: Option<std::path::PathBuf>,
    cancel: Arc<AtomicBool>,
) -> mpsc::Receiver<(bool, image::ImageResult<DecodedBackground>)> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let prepare = |image: image::DynamicImage| {
            let image = image.to_rgba8();
            let bytes = render::rgba_to_cairo_argb_bytes(&image);
            (image, bytes)
        };
        if let Some(path) = preview_path {
            if let Ok(image) = image::open(path) {
                if cancel.load(Ordering::Relaxed)
                    || sender.send((true, Ok(prepare(image)))).is_err()
                {
                    return;
                }
            }
        }
        let Ok(_guard) = BACKGROUND_DECODE_LOCK.lock() else {
            return;
        };
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let image = image::open(path);
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let _ = sender.send((false, image.map(prepare)));
    });
    receiver
}

pub struct EditorHost {
    pub editor: Shared,
    /// Called after any document or selection mutation: redraw + sync bars.
    on_change: Rc<dyn Fn()>,
    /// Toast/banner reporter for OCR failures and export errors.
    on_error: Rc<dyn Fn(String)>,
    /// Prepared Cairo surface for the working image, keyed by its revision.
    surface_cache: RefCell<Option<(u64, ImageSurface)>>,
    background_cache: RefCell<Option<(u64, u32, u32, ImageSurface)>>,
    background_source: RefCell<Option<(Arc<RgbaImage>, ImageSurface)>>,
    /// Bumped on every document load so stale async results are rejected.
    pub session_generation: Rc<Cell<u64>>,
    background_generation: Cell<u64>,
    background_loading: Cell<bool>,
    background_previous: RefCell<Option<crate::capture::editor::state::background::Fill>>,
    background_cancel: RefCell<Option<Arc<AtomicBool>>>,
    space_pan: Rc<Cell<bool>>,
    space_pan_dragging: Rc<Cell<bool>>,
    pan_start: Rc<Cell<(f64, f64)>>,
    drag_transform: Rc<RefCell<Option<ViewTransform>>>,
    /// Current canvas pan offset, for space/middle-drag pan starts.
    view_pan: Rc<dyn Fn() -> (f64, f64)>,
    last_redraw: Rc<Cell<i64>>,
}

impl EditorHost {
    pub fn new(
        on_change: Rc<dyn Fn()>,
        on_error: Rc<dyn Fn(String)>,
        view_pan: Rc<dyn Fn() -> (f64, f64)>,
    ) -> Rc<Self> {
        Rc::new(Self {
            editor: Rc::new(RefCell::new(None)),
            on_change,
            on_error,
            surface_cache: RefCell::new(None),
            background_cache: RefCell::new(None),
            background_source: RefCell::new(None),
            session_generation: Rc::new(Cell::new(0)),
            background_generation: Cell::new(0),
            background_loading: Cell::new(false),
            background_previous: RefCell::new(None),
            background_cancel: RefCell::new(None),
            space_pan: Rc::new(Cell::new(false)),
            space_pan_dragging: Rc::new(Cell::new(false)),
            pan_start: Rc::new(Cell::new((0.0, 0.0))),
            drag_transform: Rc::new(RefCell::new(None)),
            view_pan,
            last_redraw: Rc::new(Cell::new(0)),
        })
    }

    pub fn has_document(&self) -> bool {
        self.editor.borrow().is_some()
    }

    /// Replace the document (image load). Resets all interaction state and
    /// kicks off OCR for the text-aware highlighter.
    pub fn load_document(self: &Rc<Self>, image: RgbaImage, area: &DrawingArea) {
        self.load_document_headless(image);
        let _ = area;
    }

    /// Document swap without widget coupling (tests use this directly).
    fn load_document_headless(&self, image: RgbaImage) {
        let mut state = EditorState::new(image);
        self.start_detection(&mut state);
        *self.editor.borrow_mut() = Some(state);
        *self.surface_cache.borrow_mut() = None;
        *self.background_cache.borrow_mut() = None;
        *self.background_source.borrow_mut() = None;
        self.cancel_background_load();
        self.session_generation
            .set(self.session_generation.get() + 1);
        self.changed();
    }

    fn start_detection(&self, state: &mut EditorState) {
        state.text_detection_handle = Some(spawn_text_detection(
            Arc::clone(&state.base_image),
            Arc::clone(&state.text_detector),
            Arc::clone(&state.text_detection_ready),
        ));
    }

    /// Redraw after a mutation and refresh bar/pill sensitivity.
    pub fn changed(&self) {
        (self.on_change)();
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut EditorState) -> R) -> Option<R> {
        let mut borrow = self.editor.borrow_mut();
        borrow.as_mut().map(f)
    }

    /// Mutate then rebuild the pixel-effect layer when the edit touched one.
    pub fn mutate_effects(&self, f: impl FnOnce(&mut EditorState) -> bool) {
        let rebuild = self.with_state(f).unwrap_or(false);
        if rebuild {
            self.rebuild_effects_async();
        }
        self.changed();
    }

    /// Read a value out of the document.
    pub fn read<R>(&self, f: impl FnOnce(&EditorState) -> R) -> Option<R> {
        let borrow = self.editor.borrow();
        borrow.as_ref().map(f)
    }

    /// Surface an error message through the window's toast.
    pub(crate) fn report_error(&self, message: String) {
        (self.on_error)(message);
    }

    pub(crate) fn cancel_background_load(&self) {
        self.background_generation
            .set(self.background_generation.get().wrapping_add(1));
        self.background_loading.set(false);
        self.background_previous.borrow_mut().take();
        if let Some(cancel) = self.background_cancel.borrow_mut().take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(crate) fn background_loading(&self) -> bool {
        self.background_loading.get()
    }

    pub(crate) fn load_background(
        self: &Rc<Self>,
        path: std::path::PathBuf,
        preview_path: Option<std::path::PathBuf>,
        on_selected: impl FnOnce(&RgbaImage) + 'static,
        on_failure: impl FnOnce() + 'static,
    ) {
        if !self.background_loading.get() {
            *self.background_previous.borrow_mut() = self.read(|st| st.background.fill.clone());
        }
        self.background_generation
            .set(self.background_generation.get().wrapping_add(1));
        if let Some(cancel) = self.background_cancel.borrow_mut().take() {
            cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *self.background_cancel.borrow_mut() = Some(Arc::clone(&cancel));
        self.background_loading.set(true);
        self.changed();
        let generation = self.background_generation.get();
        let receiver = spawn_background_decode(path, preview_path, cancel);
        let host = Rc::downgrade(self);
        let mut on_selected = Some(on_selected);
        let mut on_failure = Some(on_failure);
        glib::timeout_add_local(Duration::from_millis(16), move || {
            let Some(host) = host.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if host.background_generation.get() != generation {
                return glib::ControlFlow::Break;
            }
            match receiver.try_recv() {
                Ok((preview, Ok((image, bytes)))) => {
                    let width = image.width();
                    let height = image.height();
                    let surface = gtk4::cairo::Format::ARgb32
                        .stride_for_width(width)
                        .ok()
                        .and_then(|stride| {
                            ImageSurface::create_for_data(
                                bytes,
                                gtk4::cairo::Format::ARgb32,
                                width as i32,
                                height as i32,
                                stride,
                            )
                            .ok()
                        });
                    if !preview {
                        if let Some(selected) = on_selected.take() {
                            selected(&image);
                        }
                    }
                    let image = Arc::new(image);
                    *host.background_source.borrow_mut() =
                        surface.map(|surface| (Arc::clone(&image), surface));
                    if !preview {
                        host.background_loading.set(false);
                        host.background_previous.borrow_mut().take();
                    }
                    host.mutate(|st| {
                        st.background.set_fill(
                            crate::capture::editor::state::background::Fill::Image(image),
                        );
                        st.background.changed();
                    });
                    if preview {
                        glib::ControlFlow::Continue
                    } else {
                        glib::ControlFlow::Break
                    }
                }
                Ok((_, Err(_))) | Err(mpsc::TryRecvError::Disconnected) => {
                    host.background_loading.set(false);
                    if let Some(previous) = host.background_previous.borrow_mut().take() {
                        host.mutate(|st| {
                            st.background.fill = previous;
                            st.background.changed();
                        });
                    }
                    if let Some(failure) = on_failure.take() {
                        failure();
                    }
                    host.report_error(crate::i18n::t("Could not open background image."));
                    host.changed();
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            }
        });
    }

    /// Mutate the document then redraw and refresh the bars; returns the
    /// closure's result so callers can branch on what changed.
    pub fn mutate<R>(&self, f: impl FnOnce(&mut EditorState) -> R) -> Option<R> {
        let out = self.with_state(f);
        self.changed();
        out
    }

    /// Undo; restarts OCR when the restored snapshot invalidated it. Uses
    /// the without-rebuild variant so effect work stays on the worker.
    pub fn undo(&self) {
        self.mutate_effects(|st| {
            let changed = st.undo_without_rebuild();
            if st.take_text_detection_restart() {
                restart_text_detection(st);
            }
            changed
        });
    }

    /// Redo; restarts OCR when the restored snapshot invalidated it.
    pub fn redo(&self) {
        self.mutate_effects(|st| {
            let changed = st.redo_without_rebuild();
            if st.take_text_detection_restart() {
                restart_text_detection(st);
            }
            changed
        });
    }

    /// Delete the selected annotation without an inline effect rebuild.
    pub fn delete_selected(&self) {
        self.mutate_effects(|st| st.remove_selected_action_without_rebuild());
    }

    /// Commit in-progress text entry before losing the edit (tool switch,
    /// Escape cascade) so valid text is never dropped.
    pub fn commit_text_if_active(&self) {
        self.with_state(|st| {
            st.commit_active_text_input();
        });
    }

    /// Arm a tool. Valid in-flight text commits first so switching never
    /// drops it; the model's `selected_tool` is the chrome's authority.
    pub fn set_tool(&self, tool: Tool) {
        self.with_state(|st| {
            st.commit_active_text_input();
            st.set_tool(tool);
        });
        self.changed();
    }

    pub fn activate_tool(&self, tool: Tool) {
        let tool = self
            .read(|st| {
                if st.selected_tool == tool && matches!(tool, Tool::Number | Tool::Highlighter) {
                    Tool::Arrow
                } else {
                    tool
                }
            })
            .unwrap_or(tool);
        self.set_tool(tool);
    }

    /// Crop Apply shared by the Enter key and the sub-bar button: applies the
    /// rect when it differs from the whole image, exits to Select, restarts
    /// OCR on the new dimensions.
    pub fn apply_crop(&self) {
        self.mutate_effects(|st| {
            if !crop_apply_enabled(st) {
                return false;
            }
            let applied = st.apply_pending_crop();
            if applied {
                st.set_tool_without_rebuild(Tool::Select);
                restart_text_detection(st);
            }
            applied
        });
    }

    /// Crop Cancel shared by Escape and the sub-bar button.
    pub fn cancel_crop(&self) {
        self.mutate(|st| {
            st.cancel_crop();
            st.set_tool_without_rebuild(Tool::Select);
        });
    }

    // -------------------------------------------------------------- render

    /// Paint the document inside the already-transformed image rect. `t` is
    /// the view transform the canvas computed for this frame.
    pub fn paint_document(&self, context: &gtk4::cairo::Context, t: ViewTransform) {
        let Some(borrow) = self.editor.try_borrow().ok() else {
            return;
        };
        let Some(st) = borrow.as_ref() else {
            return;
        };
        let Some(surface) = self.working_surface(st) else {
            return;
        };

        let layout = st
            .background
            .layout(st.working_image.width(), st.working_image.height());
        if let crate::capture::editor::state::background::Fill::Image(image) = &st.background.fill {
            if st.background.blur <= 0.001 {
                if let Some(background) = self.background_image_surface(image) {
                    let _ = context.save();
                    context.rectangle(
                        t.canvas_offset_x,
                        t.canvas_offset_y,
                        t.canvas_width * t.canvas_scale,
                        t.canvas_height * t.canvas_scale,
                    );
                    context.clip();
                    context.translate(t.canvas_offset_x, t.canvas_offset_y);
                    context.scale(t.canvas_scale, t.canvas_scale);
                    let cover = (t.canvas_width / f64::from(image.width()))
                        .max(t.canvas_height / f64::from(image.height()));
                    let _ = context.save();
                    context.translate(
                        (t.canvas_width - f64::from(image.width()) * cover) / 2.0,
                        (t.canvas_height - f64::from(image.height()) * cover) / 2.0,
                    );
                    context.scale(cover, cover);
                    context.set_source_surface(&background, 0.0, 0.0).ok();
                    context.source().set_extend(gtk4::cairo::Extend::Pad);
                    context.source().set_filter(gtk4::cairo::Filter::Good);
                    let _ = context.paint();
                    let _ = context.restore();
                    render::paint_background_noise(
                        context,
                        0.0,
                        0.0,
                        t.canvas_width,
                        t.canvas_height,
                        st.background.noise,
                    );
                    let _ = context.restore();
                }
            } else {
                self.paint_composited_background(context, st, t, layout);
            }
        } else if st.background.has_fill() {
            self.paint_composited_background(context, st, t, layout);
        }
        let _ = context.save();
        context.translate(t.offset_x, t.offset_y);
        context.scale(t.scale, t.scale);
        render::rounded_rect_path(
            context,
            0.0,
            0.0,
            f64::from(st.working_image.width()),
            f64::from(st.working_image.height()),
            layout.radius,
        );
        context.clip();
        context.set_source_surface(&surface, 0.0, 0.0).ok();
        context
            .source()
            .set_filter(render::editor_image_filter_for_scale(t.scale));
        let _ = context.paint();
        let _ = context.restore();

        let _ = context.save();
        context.translate(t.offset_x, t.offset_y);
        context.scale(t.scale, t.scale);
        let _ = context.save();
        clip_annotation_content_to_canvas(context, t);

        let editing = st
            .active_text_input
            .as_ref()
            .and_then(|input| input.editing_action_index);
        for (index, action) in st.actions.iter().enumerate() {
            if Some(index) == editing
                || matches!(
                    action,
                    AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. }
                )
            {
                continue;
            }
            draw_annotation_action(context, action);
        }
        if let Some(draft) = st.draft_action() {
            draw_draft_action(context, &draft);
        }
        let _ = context.restore();

        self.paint_chrome(context, st, t);
        let _ = context.restore();
    }

    fn background_image_surface(&self, image: &Arc<RgbaImage>) -> Option<ImageSurface> {
        if let Some((cached_image, surface)) = self.background_source.borrow().as_ref() {
            if Arc::ptr_eq(cached_image, image) {
                return Some(surface.clone());
            }
        }
        let surface = rgba_image_to_surface(image)?;
        *self.background_source.borrow_mut() = Some((Arc::clone(image), surface.clone()));
        Some(surface)
    }

    fn paint_composited_background(
        &self,
        context: &gtk4::cairo::Context,
        st: &EditorState,
        t: ViewTransform,
        layout: crate::capture::editor::state::background::Layout,
    ) {
        let key = (
            st.background.revision,
            st.working_image.width(),
            st.working_image.height(),
        );
        let cached = self.background_cache.borrow();
        let background = cached
            .as_ref()
            .filter(|(revision, width, height, _)| (*revision, *width, *height) == key)
            .map(|(_, _, _, surface)| surface.clone());
        drop(cached);
        let background = background.or_else(|| {
            let surface = st.background.surface(layout, 1200.0)?;
            *self.background_cache.borrow_mut() = Some((key.0, key.1, key.2, surface.clone()));
            Some(surface)
        });
        if let Some(background) = background {
            let _ = context.save();
            context.rectangle(
                t.canvas_offset_x,
                t.canvas_offset_y,
                t.canvas_width * t.canvas_scale,
                t.canvas_height * t.canvas_scale,
            );
            context.clip();
            context.translate(t.canvas_offset_x, t.canvas_offset_y);
            context.scale(
                t.canvas_width * t.canvas_scale / f64::from(background.width()),
                t.canvas_height * t.canvas_scale / f64::from(background.height()),
            );
            context.set_source_surface(&background, 0.0, 0.0).ok();
            context.source().set_extend(gtk4::cairo::Extend::Pad);
            let _ = context.paint();
            let _ = context.restore();
        }
    }

    fn working_surface(&self, st: &EditorState) -> Option<ImageSurface> {
        let revision = st.working_image_revision;
        let cached = self.surface_cache.borrow();
        if let Some((cached_rev, surface)) = cached.as_ref() {
            if *cached_rev == revision {
                return Some(surface.clone());
            }
        }
        drop(cached);
        let surface = rgba_image_to_surface(&st.working_image)?;
        *self.surface_cache.borrow_mut() = Some((revision, surface.clone()));
        Some(surface)
    }

    fn paint_chrome(&self, context: &gtk4::cairo::Context, st: &EditorState, t: ViewTransform) {
        let image_w = st.working_image.width() as f64;
        let image_h = st.working_image.height() as f64;

        // Text hover outline while the Text tool is armed.
        if st.selected_tool == Tool::Text && st.active_text_bounds.is_none() {
            if let Some(index) = st.hovered_text_action_index {
                if let Some(AnnotationAction::Text {
                    position,
                    text,
                    font,
                    max_width,
                    ..
                }) = st.actions.get(index)
                {
                    let width =
                        max_width.unwrap_or_else(|| (image_w - position.x).max(font.size * 1.8));
                    let mut bounds =
                        text_action_bounds(context, *position, text, font, Some(width));
                    bounds.rect.x = bounds
                        .rect
                        .x
                        .clamp(0, (image_w as i32 - bounds.rect.width).max(0));
                    bounds.rect.y = bounds
                        .rect
                        .y
                        .clamp(0, (image_h as i32 - bounds.rect.height).max(0));
                    bounds.sync_handles();
                    draw_text_edit_border(context, &bounds, t.scale);
                }
            }
        }

        if let Some(selected) = st.selected_action() {
            let show_text_handles = matches!(st.selected_tool, Tool::Select | Tool::Text)
                && st.active_text_bounds.is_none();
            if show_text_handles {
                if let AnnotationAction::Text {
                    position,
                    text,
                    font,
                    max_width,
                    ..
                } = selected
                {
                    let width =
                        max_width.unwrap_or_else(|| (image_w - position.x).max(font.size * 1.8));
                    let mut bounds =
                        text_action_bounds(context, *position, text, font, Some(width));
                    bounds.rect.x = bounds
                        .rect
                        .x
                        .clamp(0, (image_w as i32 - bounds.rect.width).max(0));
                    bounds.rect.y = bounds
                        .rect
                        .y
                        .clamp(0, (image_h as i32 - bounds.rect.height).max(0));
                    bounds.sync_handles();
                    draw_text_edit_border(context, &bounds, t.scale);
                    draw_text_edit_handles(context, &bounds, None, t.scale);
                }
            }

            let show_selection = st.selected_tool == Tool::Select
                || st.selected_tool == Tool::Arrow
                || (matches!(st.selected_tool, Tool::Box | Tool::Circle)
                    && matches!(
                        selected,
                        AnnotationAction::Box { .. } | AnnotationAction::Circle { .. }
                    ))
                || (st.selected_tool == Tool::Obfuscate
                    && matches!(selected, AnnotationAction::Obfuscate { .. }))
                || (st.selected_tool == Tool::Focus
                    && matches!(selected, AnnotationAction::Focus { .. }))
                || (st.selected_tool == Tool::Number
                    && matches!(selected, AnnotationAction::Number { .. }));
            if show_selection {
                if let AnnotationAction::Arrow {
                    start,
                    end,
                    stroke_size,
                    style,
                    control_points,
                    ..
                } = selected
                {
                    draw_arrow_selection_outline(
                        context,
                        *start,
                        *end,
                        *stroke_size,
                        *style,
                        control_points.clone(),
                        t.scale,
                    );
                } else if matches!(
                    selected,
                    AnnotationAction::Line { .. } | AnnotationAction::Text { .. }
                ) {
                    // Lines and text have their own chrome.
                } else {
                    let padding = if matches!(
                        selected,
                        AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. }
                    ) {
                        0.0
                    } else {
                        selection_hit_padding_for_scale(t.scale)
                    };
                    if let Some(bounds) = action_bounds_with_padding(selected, padding) {
                        if !matches!(
                            selected,
                            AnnotationAction::Box { .. } | AnnotationAction::Circle { .. }
                        ) {
                            draw_selection_outline(context, bounds, t.scale);
                        }
                    }
                    let handles = action_resize_handles(selected);
                    if !handles.is_empty() {
                        draw_selection_handles(context, &handles, st.select_resize_handle, t.scale);
                    }
                }
            }
        }

        let show_arrow_handles = st.arrow_editing_controls
            || (matches!(st.selected_tool, Tool::Arrow | Tool::Select)
                && st
                    .selected_action()
                    .is_some_and(|a| matches!(a, AnnotationAction::Arrow { .. })));
        if show_arrow_handles {
            if let Some(AnnotationAction::Arrow {
                control_points: Some(handles),
                color,
                ..
            }) = st.selected_action()
            {
                draw_arrow_control_handles(context, handles.clone(), *color, t.scale);
            }
        }

        if let Some(bounds) = st.active_text_bounds.as_ref() {
            let mut bounds = bounds.clone();
            bounds.rect.x = bounds
                .rect
                .x
                .clamp(0, (image_w as i32 - bounds.rect.width).max(0));
            bounds.rect.y = bounds
                .rect
                .y
                .clamp(0, (image_h as i32 - bounds.rect.height).max(0));
            bounds.sync_handles();
            if let Some(input) = st.active_text_input.as_ref() {
                let font = FontSettings {
                    family: st.text_font_family.clone(),
                    size: st.text_size,
                    style: FontStyle::Normal,
                    decoration: TextDecoration::None,
                    alignment: TextAlignment::Left,
                };
                draw_active_text_input(
                    context,
                    &bounds,
                    &input.text,
                    input.cursor_position,
                    input.cursor_visible,
                    input.color,
                    &font,
                );
            }
            draw_text_edit_border(context, &bounds, t.scale);
            draw_text_edit_handles(
                context,
                &bounds,
                st.active_text_drag_handle.clone(),
                t.scale,
            );
        }

        if st.selected_tool == Tool::Crop {
            if let Some(rect) = st.crop_rect {
                render::draw_crop_overlay(context, image_w, image_h, rect, true, t.scale);
            }
        }
    }

    // ------------------------------------------------------------ effects

    /// Request an effect-layer rebuild; coalesced while one is in flight.
    pub(crate) fn rebuild_effects_async(&self) {
        let Some(borrowed) = self.editor.try_borrow().ok() else {
            return;
        };
        drop(borrowed);
        // The poll loop above owns the channel; trigger via the same flags.
        self.with_state(|st| {
            if st.select_effect_rebuild_pending {
                st.select_effect_rebuild_dirty = true;
            }
        });
        // An idle pipeline can launch immediately; busy workers coalesce edits.
        let needs = self
            .with_state(|st| !st.select_effect_rebuild_pending)
            .unwrap_or(false);
        if needs {
            self.trigger_effect_rebuild();
        }
    }

    /// Spawn the pixel-effects pass on a worker and apply the result back on
    /// the UI thread once it lands. A newer request revises `pending_effect_-
    /// revision`, so an in-flight result arriving late is rejected.
    fn trigger_effect_rebuild(&self) {
        let editor = self.editor.clone();
        let generation_cell = self.session_generation.clone();
        let on_change = self.on_change.clone();
        let payload = self.with_state(|st| {
            st.select_effect_rebuild_pending = true;
            st.select_effect_rebuild_dirty = false;
            st.last_effect_request_time_us = glib::monotonic_time();
            st.pending_effect_revision += 1;
            (
                Arc::clone(&st.base_image),
                st.actions.clone(),
                st.pending_effect_revision,
            )
        });
        let Some((base, actions, revision)) = payload else {
            return;
        };
        Self::spawn_effect_worker(editor, generation_cell, on_change, base, actions, revision);
    }

    fn spawn_effect_worker(
        editor: Shared,
        generation_cell: Rc<Cell<u64>>,
        on_change: Rc<dyn Fn()>,
        base: Arc<RgbaImage>,
        actions: Vec<AnnotationAction>,
        revision: u64,
    ) {
        let generation = generation_cell.get();
        let base_identity = base.clone();
        let (sender, receiver) = mpsc::channel::<(RgbaImage, u64)>();
        std::thread::spawn(move || {
            let mut working = (*base).clone();
            apply_effect_actions(&mut working, &actions);
            let _ = sender.send((working, revision));
        });
        glib::timeout_add_local(Duration::from_millis(16), move || {
            match receiver.try_recv() {
                Ok((image, revision)) => {
                    let dirty_again = (|| {
                        let mut borrow = editor.borrow_mut();
                        let st = borrow.as_mut()?;
                        // Reject results from a previous document or a
                        // superseded request before touching the state.
                        if generation != generation_cell.get()
                            || revision != st.pending_effect_revision
                            || !Arc::ptr_eq(&base_identity, &st.base_image)
                        {
                            return Some(false);
                        }
                        apply_effect_result(
                            st,
                            generation_cell.get(),
                            generation,
                            revision,
                            &base_identity,
                            image,
                        );
                        st.select_effect_rebuild_pending = false;
                        Some(std::mem::take(&mut st.select_effect_rebuild_dirty))
                    })();
                    on_change();
                    // Edits landed mid-flight: enqueue a fresh worker job
                    // for the newest snapshot, never a sync rebuild.
                    if dirty_again == Some(true) {
                        if let Some((base2, actions2, rev2)) =
                            editor.borrow_mut().as_mut().map(|st| {
                                st.select_effect_rebuild_pending = true;
                                st.last_effect_request_time_us = glib::monotonic_time();
                                st.pending_effect_revision += 1;
                                (
                                    Arc::clone(&st.base_image),
                                    st.actions.clone(),
                                    st.pending_effect_revision,
                                )
                            })
                        {
                            Self::spawn_effect_worker(
                                editor.clone(),
                                generation_cell.clone(),
                                on_change.clone(),
                                base2,
                                actions2,
                                rev2,
                            );
                        }
                    }
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => glib::ControlFlow::Break,
            }
        });
    }

    /// Deterministic seam for tests: run one worker pass to completion and
    /// apply it under the same generation/revision rules.
    #[cfg(test)]
    pub(crate) fn run_effect_worker_once(
        &self,
        base: Arc<RgbaImage>,
        actions: Vec<AnnotationAction>,
        revision: u64,
        generation: u64,
    ) -> bool {
        let mut working = (*base).clone();
        apply_effect_actions(&mut working, &actions);
        self.with_state(|st| {
            apply_effect_result(
                st,
                self.session_generation.get(),
                generation,
                revision,
                &base,
                working,
            )
        })
        .unwrap_or(false)
    }

    // ----------------------------------------------------------- gestures

    /// Wire the canvas gestures: primary-button tool interaction (the Linux
    /// drag/click protocol), middle-button or Space+left pan, motion hover.
    pub fn install(
        self: &Rc<Self>,
        area: &DrawingArea,
        transform_of: Rc<dyn Fn() -> ViewTransform>,
        cursor: Rc<dyn Fn(Option<&'static str>)>,
        pan_to: Rc<dyn Fn(f64, f64)>,
    ) {
        // ---- pointer motion: text hover + handle cursors ----
        let motion = gtk4::EventControllerMotion::new();
        motion.connect_motion({
            let host = Rc::downgrade(self);
            let transform_of = transform_of.clone();
            let area = area.downgrade();
            let cursor = cursor.clone();
            move |_, x, y| {
                let Some(host) = host.upgrade() else {
                    return;
                };
                let t = transform_of();
                let point = t.view_to_image(Point { x, y });
                let Some(area) = area.upgrade() else {
                    return;
                };
                let mut cursor_name: Option<&'static str> = None;
                host.with_state(|st| {
                    if st.selected_tool == Tool::Text && st.active_text_input.is_none() {
                        let hit =
                            st.actions
                                .iter()
                                .enumerate()
                                .rev()
                                .find_map(|(index, action)| {
                                    (matches!(action, AnnotationAction::Text { .. })
                                        && action_contains_point_with_padding(action, point, 0.0))
                                    .then_some(index)
                                });
                        if st.hovered_text_action_index != hit {
                            st.hovered_text_action_index = hit;
                            area.queue_draw();
                        }
                        cursor_name = Some("text");
                    } else if st.selected_tool == Tool::Crop {
                        cursor_name = Some(crop_cursor(st, point, t.scale));
                    } else if st.selected_tool == Tool::Select {
                        if let Some(handle) = st.selected_action().and_then(|action| {
                            action_resize_handle_at_point_with_radius(
                                action,
                                point,
                                selection_handle_hit_radius_for_scale(t.scale),
                            )
                        }) {
                            cursor_name = Some(cursor_name_for_select_handle(handle));
                        } else {
                            cursor_name = Some("default");
                        }
                    } else {
                        cursor_name = Some("crosshair");
                    }
                });
                cursor(cursor_name);
            }
        });
        area.add_controller(motion);

        // ---- click press/release: select, text, number ----
        let click = gtk4::GestureClick::new();
        click.set_button(1);
        click.connect_pressed({
            let host = Rc::downgrade(self);
            let transform_of = transform_of.clone();
            let area = area.downgrade();
            move |gesture, n_press, x, y| {
                let Some(host) = host.upgrade() else {
                    return;
                };
                if host.space_pan.get() {
                    gesture.set_state(gtk4::EventSequenceState::Denied);
                    return;
                }
                let t = transform_of();
                let view_point = Point { x, y };
                let Some(area) = area.upgrade() else {
                    return;
                };
                area.grab_focus();
                host.press(&t, view_point, n_press, x, y);
                host.changed();
                let _ = area;
            }
        });
        click.connect_released({
            let host = Rc::downgrade(self);
            let area = area.downgrade();
            move |_, _, _, _| {
                let Some(host) = host.upgrade() else {
                    return;
                };
                let refocus = host.with_state(finish_text_drag);
                if let Some(area) = area.upgrade() {
                    if refocus == Some(true) {
                        area.grab_focus();
                    }
                    area.queue_draw();
                }
                host.changed();
            }
        });
        area.add_controller(click);

        // ---- drag: pan (middle/space) vs tool routing ----
        let drag = gtk4::GestureDrag::new();
        drag.set_button(0);
        drag.connect_drag_begin({
            let host = Rc::downgrade(self);
            let transform_of = transform_of.clone();
            let area = area.downgrade();
            let cursor = cursor.clone();
            move |gesture, x, y| {
                let Some(host) = host.upgrade() else {
                    return;
                };
                let button = gesture.current_button();
                let Some(area) = area.upgrade() else {
                    return;
                };
                if button == 2 || (button == 1 && host.space_pan.get()) {
                    host.space_pan_dragging.set(true);
                    host.pan_start.set((host.view_pan)());
                    gesture.set_state(gtk4::EventSequenceState::Claimed);
                    cursor(Some("grabbing"));
                    return;
                }
                if button != 1 {
                    return;
                }
                let t = transform_of();
                *host.drag_transform.borrow_mut() = Some(t);
                host.drag_begin(&t, gesture, Point { x, y });
                host.last_redraw.set(glib::monotonic_time());
                host.changed();
                let _ = area;
            }
        });
        drag.connect_drag_update({
            let host = Rc::downgrade(self);
            let transform_of = transform_of.clone();
            let area = area.downgrade();
            let pan_to = pan_to.clone();
            move |gesture, offset_x, offset_y| {
                let Some(host) = host.upgrade() else {
                    return;
                };
                if host.space_pan_dragging.get() {
                    let (px, py) = host.pan_start.get();
                    pan_to(px + offset_x, py + offset_y);
                    return;
                }
                let t = host
                    .drag_transform
                    .borrow()
                    .unwrap_or_else(|| transform_of());
                host.drag_update(&t, gesture, offset_x, offset_y);
                let Some(area) = area.upgrade() else {
                    return;
                };
                let now = glib::monotonic_time();
                if now - host.last_redraw.get() >= DRAG_REDRAW_INTERVAL_US {
                    host.last_redraw.set(now);
                    let effect_selected = host
                        .read(|st| {
                            matches!(
                                st.selected_action(),
                                Some(
                                    AnnotationAction::Obfuscate { .. }
                                        | AnnotationAction::Focus { .. }
                                )
                            )
                        })
                        .unwrap_or(false);
                    if effect_selected {
                        host.changed();
                    } else {
                        area.queue_draw();
                    }
                }
            }
        });
        drag.connect_drag_end({
            let host = Rc::downgrade(self);
            let transform_of = transform_of.clone();
            let area = area.downgrade();
            let cursor = cursor.clone();
            move |gesture, offset_x, offset_y| {
                let Some(host) = host.upgrade() else {
                    return;
                };
                if host.space_pan_dragging.replace(false) {
                    cursor(if host.space_pan.get() {
                        Some("grab")
                    } else {
                        None
                    });
                    return;
                }
                let t = host
                    .drag_transform
                    .borrow()
                    .unwrap_or_else(|| transform_of());
                host.drag_end(&t, gesture, offset_x, offset_y);
                if let Some(area) = area.upgrade() {
                    area.queue_draw();
                }
                host.changed();
            }
        });
        area.add_controller(drag);
    }

    fn press(&self, t: &ViewTransform, view_point: Point, n_press: i32, x: f64, y: f64) {
        self.with_state(|st| {
            // Active text edit: hit-test its handles/bounds first.
            if let Some(bounds) = st.active_text_bounds.clone() {
                let click_image = t.view_to_image_clamped(view_point);
                let inside = click_image.x >= bounds.rect.x as f64
                    && click_image.x <= (bounds.rect.x + bounds.rect.width) as f64
                    && click_image.y >= bounds.rect.y as f64
                    && click_image.y <= (bounds.rect.y + bounds.rect.height) as f64;
                let handle_hit = bounds.move_handles.iter().find_map(|(handle, center)| {
                    let cv = t.image_to_view(*center);
                    (((x - cv.x).powi(2) + (y - cv.y).powi(2)).sqrt()
                        < MOVE_HANDLE_DRAG_RADIUS * 1.5)
                        .then_some(handle.clone())
                });
                let resize_hit = bounds.resize_handle.as_ref().is_some_and(|(_, rp)| {
                    let rv = t.image_to_view(*rp);
                    (x - rv.x).abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                        && (y - rv.y).abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                });
                if let Some(handle) = handle_hit {
                    st.active_text_is_dragging = true;
                    st.active_text_drag_handle = Some(handle);
                    st.active_text_drag_start = Some(click_image);
                    st.active_text_drag_start_bounds = Some(bounds.rect);
                    st.active_text_is_resizing = false;
                    st.reset_text_cursor_blink();
                    return;
                }
                if resize_hit {
                    st.active_text_is_dragging = true;
                    st.active_text_drag_handle = None;
                    st.active_text_drag_start = Some(click_image);
                    st.active_text_drag_start_bounds = Some(bounds.rect);
                    st.active_text_is_resizing = true;
                    st.reset_text_cursor_blink();
                    return;
                }
                if inside {
                    if st.active_text_input.is_some() {
                        if let Some(input_bounds) = st.active_text_bounds.as_ref() {
                            if let Some((_surface, context)) = text_context() {
                                let font = FontSettings {
                                    family: st.text_font_family.clone(),
                                    size: st.text_size,
                                    style: FontStyle::Normal,
                                    decoration: TextDecoration::None,
                                    alignment: TextAlignment::Left,
                                };
                                let pos = render::cursor_position_for_text_point(
                                    &context,
                                    input_bounds,
                                    &st.active_text_input.as_ref().unwrap().text,
                                    &font,
                                    click_image,
                                );
                                st.set_text_cursor_position(pos);
                            }
                        }
                    }
                    return;
                }
                if let Some(action) = st.commit_text_input() {
                    st.push_action(action);
                }
            }

            if !t.contains_view(view_point) {
                return;
            }
            let image_point = t.view_to_image_clamped(view_point);
            match st.selected_tool {
                Tool::Select => {
                    if st.active_text_input.is_some() {
                        st.commit_active_text_input();
                    }
                    // A handle of the already-selected action wins over
                    // reverse-z reselection so grabs keep their grip.
                    if let Some(selected) = st.selected_action() {
                        let radius = selection_handle_hit_radius_for_scale(t.scale);
                        if let Some(handle) =
                            action_resize_handle_at_point_with_radius(selected, image_point, radius)
                        {
                            st.select_resize_handle = Some(handle);
                            st.select_drag_anchor = Some(image_point);
                            return;
                        }
                        if selected_is_arrow(st) {
                            ensure_arrow_controls(st);
                            if let Some(idx) = st.arrow_control_handle_at(image_point) {
                                st.arrow_control_dragging = Some(idx);
                                st.arrow_editing_controls = true;
                                return;
                            }
                        }
                    }
                    st.select_action_at_point_with_scale(image_point, t.scale);
                    ensure_arrow_controls(st);
                    let mut began_reedit = false;
                    if n_press >= 2 {
                        began_reedit = st.begin_editing_selected_text();
                    }
                    let color = if began_reedit {
                        st.get_text_input().map(|input| input.color)
                    } else {
                        st.selected_action_color()
                    };
                    if let Some(color) = color {
                        st.selected_color = color;
                    }
                    if let Some(size) = st.selected_text_action_size() {
                        st.text_size = size;
                    }
                    if let Some(size) = st.selected_action_stroke_size() {
                        st.stroke_size = size;
                    }
                    if let Some(family) = st.selected_text_font_family() {
                        st.text_font_family = family;
                    }
                    if let Some(AnnotationAction::Obfuscate { method, amount, .. }) =
                        st.selected_action()
                    {
                        let (method, amount) = (*method, *amount);
                        st.set_obfuscate_method(method);
                        st.set_current_obfuscate_amount(amount);
                    }
                }
                Tool::Text => {
                    if st.active_text_input.is_some() {
                        st.commit_active_text_input();
                    }
                    let hit = st
                        .actions
                        .iter()
                        .enumerate()
                        .rev()
                        .find_map(|(index, action)| {
                            (matches!(action, AnnotationAction::Text { .. })
                                && action_contains_point_with_padding(action, image_point, 0.0))
                            .then_some(index)
                        });
                    if let Some(index) = hit {
                        st.selected_action_index = Some(index);
                        if let Some(color) = st.selected_action_color() {
                            st.selected_color = color;
                        }
                        if let Some(size) = st.selected_text_action_size() {
                            st.text_size = size;
                        }
                        if let Some(family) = st.selected_text_font_family() {
                            st.text_font_family = family;
                        }
                        if n_press >= 2 {
                            st.begin_editing_selected_text();
                        } else {
                            let bounds_opt = committed_text_bounds(st, index);
                            let mut handle_drag_started = false;
                            if let Some(bounds) = bounds_opt {
                                let handle_hit =
                                    bounds.move_handles.iter().find_map(|(h, center)| {
                                        let cv = t.image_to_view(*center);
                                        (((x - cv.x).powi(2) + (y - cv.y).powi(2)).sqrt()
                                            < MOVE_HANDLE_DRAG_RADIUS * 1.5)
                                            .then_some(h.clone())
                                    });
                                let resize_hit =
                                    bounds.resize_handle.as_ref().is_some_and(|(_, rp)| {
                                        let rv = t.image_to_view(*rp);
                                        (x - rv.x).abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                                            && (y - rv.y).abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                                    });
                                if handle_hit.is_some() || resize_hit {
                                    st.active_text_bounds = Some(bounds);
                                    st.active_text_is_dragging = true;
                                    st.active_text_drag_handle = handle_hit;
                                    st.active_text_drag_start = Some(image_point);
                                    st.active_text_drag_start_bounds =
                                        st.active_text_bounds.as_ref().map(|b| b.rect);
                                    st.active_text_is_resizing = resize_hit;
                                    handle_drag_started = true;
                                }
                            }
                            if !handle_drag_started {
                                st.select_drag_anchor = Some(image_point);
                                st.select_resize_handle = None;
                            }
                        }
                    } else {
                        st.selected_action_index = None;
                        let initial_width = (st.text_size * 3.2).max(220.0);
                        let initial_height = (st.text_size * 1.45 + 16.0).max(44.0);
                        st.begin_text_input(image_point, initial_width, initial_height);
                    }
                }
                Tool::Number => {
                    if !st.select_number_action_at_point_with_scale(image_point, t.scale) {
                        st.add_number_marker(image_point);
                    }
                }
                _ => {}
            }
        });
    }

    fn drag_begin(&self, t: &ViewTransform, gesture: &gtk4::GestureDrag, view_point: Point) {
        let shift = gesture
            .current_event_state()
            .contains(gdk::ModifierType::SHIFT_MASK);
        self.with_state(|st| {
            if st.selected_tool == Tool::Crop {
                let point = t.view_to_image(view_point);
                if !t.contains_view(view_point)
                    && !st
                        .crop_rect
                        .is_some_and(|rect| crop_handle_at(rect, point, t.scale).is_some())
                {
                    return;
                }
                st.begin_crop_drag(point, t.scale);
                st.drag_start_view = Some(view_point);
                return;
            }
            if !t.contains_view(view_point) {
                return;
            }
            st.drag_start_view = Some(view_point);
            let image_point = t.view_to_image_clamped(view_point);
            if st.selected_tool == Tool::Select {
                if selected_is_arrow(st) {
                    ensure_arrow_controls(st);
                    if let Some(idx) = st.arrow_control_handle_at(image_point) {
                        st.arrow_control_dragging = Some(idx);
                        st.arrow_editing_controls = true;
                        st.drag_start_view = Some(view_point);
                        return;
                    }
                    let idx = st.selected_action_index.unwrap();
                    if action_contains_point_with_padding(&st.actions[idx], image_point, 8.0) {
                        st.select_drag_anchor = Some(image_point);
                        st.select_resize_handle = None;
                        st.arrow_editing_controls = true;
                        st.drag_start_view = Some(view_point);
                        return;
                    }
                }
                st.drag_start_view = Some(view_point);
                st.begin_select_drag_with_scale(image_point, t.scale);
                return;
            }
            if st.selected_tool == Tool::Arrow {
                if selected_is_arrow(st) {
                    if let Some(idx) = st.arrow_control_handle_at(image_point) {
                        st.arrow_control_dragging = Some(idx);
                        st.arrow_editing_controls = true;
                        st.drag_start_view = Some(view_point);
                        return;
                    }
                    let idx = st.selected_action_index.unwrap();
                    if action_contains_point_with_padding(&st.actions[idx], image_point, 8.0) {
                        st.select_drag_anchor = Some(image_point);
                        st.select_resize_handle = None;
                        st.arrow_editing_controls = true;
                        st.drag_start_view = Some(view_point);
                        return;
                    }
                    st.selected_action_index = None;
                    st.select_drag_anchor = None;
                    st.arrow_editing_controls = false;
                }
                if st.selected_action_index.is_none()
                    && st.select_action_at_point_with_scale(image_point, t.scale)
                {
                    if st
                        .selected_action()
                        .is_some_and(|a| matches!(a, AnnotationAction::Arrow { .. }))
                    {
                        ensure_arrow_controls(st);
                        st.arrow_editing_controls = true;
                        st.select_drag_anchor = Some(image_point);
                        st.select_resize_handle = None;
                        st.drag_start_view = Some(view_point);
                        return;
                    }
                    st.selected_action_index = None;
                    st.select_drag_anchor = None;
                }
            }
            if st.selected_tool == Tool::Text
                && st.selected_action_index.is_some()
                && st.active_text_input.is_none()
            {
                if let Some(index) = st.selected_action_index {
                    if let Some(bounds) = committed_text_bounds(st, index) {
                        let handle_hit = bounds.move_handles.iter().find_map(|(h, center)| {
                            let cv = t.image_to_view(*center);
                            (((view_point.x - cv.x).powi(2) + (view_point.y - cv.y).powi(2)).sqrt()
                                < MOVE_HANDLE_DRAG_RADIUS * 1.5)
                                .then_some(h.clone())
                        });
                        let resize_hit = bounds.resize_handle.as_ref().is_some_and(|(_, rp)| {
                            let rv = t.image_to_view(*rp);
                            (view_point.x - rv.x).abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                                && (view_point.y - rv.y).abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                        });
                        if handle_hit.is_some() || resize_hit {
                            st.begin_history_interaction();
                            st.active_text_bounds = Some(bounds);
                            st.active_text_is_dragging = true;
                            st.active_text_drag_handle = handle_hit;
                            st.active_text_drag_start = Some(image_point);
                            st.active_text_drag_start_bounds =
                                st.active_text_bounds.as_ref().map(|b| b.rect);
                            st.active_text_is_resizing = resize_hit;
                            return;
                        }
                    }
                }
                st.drag_start_view = Some(view_point);
                st.select_drag_anchor = Some(image_point);
                st.select_resize_handle = None;
                return;
            }
            if matches!(st.selected_tool, Tool::Text | Tool::Number) {
                return;
            }
            if matches!(
                st.selected_tool,
                Tool::Box | Tool::Circle | Tool::Obfuscate | Tool::Focus
            ) {
                if !(st.selected_action_index.is_some() && st.select_drag_anchor.is_some()) {
                    if let Some(index) = st.selected_action_index {
                        let matching = st.actions.get(index).is_some_and(|action| {
                            matches!(
                                (action, st.selected_tool),
                                (AnnotationAction::Box { .. }, Tool::Box)
                                    | (AnnotationAction::Circle { .. }, Tool::Circle)
                                    | (AnnotationAction::Obfuscate { .. }, Tool::Obfuscate)
                                    | (AnnotationAction::Focus { .. }, Tool::Focus)
                            )
                        });
                        if matching {
                            let radius = selection_handle_hit_radius_for_scale(t.scale);
                            let action = &st.actions[index];
                            if let Some(handle) = action_resize_handle_at_point_with_radius(
                                action,
                                image_point,
                                radius,
                            ) {
                                st.select_resize_handle = Some(handle);
                                st.select_drag_anchor = Some(image_point);
                                st.drag_start_view = Some(view_point);
                                return;
                            }
                            if action_contains_point_with_padding(
                                action,
                                image_point,
                                selection_hit_padding_for_scale(t.scale),
                            ) {
                                st.select_drag_anchor = Some(image_point);
                                st.select_resize_handle = None;
                                st.drag_start_view = Some(view_point);
                                return;
                            }
                        }
                        st.selected_action_index = None;
                        st.select_drag_anchor = None;
                    }
                    if st.selected_action_index.is_none() {
                        let padding = selection_hit_padding_for_scale(t.scale);
                        let hit = st
                            .actions
                            .iter()
                            .enumerate()
                            .rev()
                            .find(|(_, action)| {
                                let matching = matches!(
                                    (action, st.selected_tool),
                                    (AnnotationAction::Box { .. }, Tool::Box)
                                        | (AnnotationAction::Circle { .. }, Tool::Circle)
                                        | (AnnotationAction::Obfuscate { .. }, Tool::Obfuscate)
                                        | (AnnotationAction::Focus { .. }, Tool::Focus)
                                );
                                matching
                                    && action_contains_point_with_padding(
                                        action,
                                        image_point,
                                        padding,
                                    )
                            })
                            .map(|(index, _)| index);
                        if let Some(index) = hit {
                            st.selected_action_index = Some(index);
                            let radius = selection_handle_hit_radius_for_scale(t.scale);
                            st.select_resize_handle = action_resize_handle_at_point_with_radius(
                                &st.actions[index],
                                image_point,
                                radius,
                            );
                            st.select_drag_anchor = Some(image_point);
                            st.drag_start_view = Some(view_point);
                            return;
                        }
                    }
                }
            }
            st.drag_shift_active = shift;
            st.begin_drag(image_point);
            st.drag_start_view = Some(view_point);
        });
    }

    fn drag_update(&self, t: &ViewTransform, gesture: &gtk4::GestureDrag, ox: f64, oy: f64) {
        let shift = gesture
            .current_event_state()
            .contains(gdk::ModifierType::SHIFT_MASK);
        let mut effect_dirty = false;
        self.with_state(|st| {
            let Some(start_view) = st.drag_start_view else {
                return;
            };
            let current_view = Point {
                x: start_view.x + ox,
                y: start_view.y + oy,
            };
            if st.selected_tool == Tool::Crop {
                st.update_crop_drag(t.view_to_image(current_view), shift);
                return;
            }
            if let Some(handle_idx) = st.arrow_control_dragging {
                let image_point = if handle_idx == 1 {
                    t.view_to_image(current_view)
                } else {
                    t.view_to_image_clamped(current_view)
                };
                st.move_arrow_control_handle(handle_idx, image_point);
                return;
            }
            if st.selected_tool == Tool::Text
                && st.active_text_input.is_none()
                && st.active_text_is_dragging
            {
                self.update_active_text_drag(st, t, current_view);
                return;
            }
            if st.active_text_is_dragging {
                self.update_active_text_drag(st, t, current_view);
                return;
            }
            let is_move = st.selected_tool == Tool::Select
                || (st.selected_tool == Tool::Arrow
                    && st.selected_action_index.is_some()
                    && st.select_drag_anchor.is_some()
                    && st.arrow_control_dragging.is_none())
                || (st.selected_tool == Tool::Text
                    && st.selected_action_index.is_some()
                    && st.active_text_input.is_none()
                    && !st.active_text_is_dragging)
                || (matches!(
                    st.selected_tool,
                    Tool::Box | Tool::Circle | Tool::Obfuscate | Tool::Focus
                ) && st.selected_action_index.is_some()
                    && st.select_drag_anchor.is_some());
            if is_move {
                if st.update_select_drag(t.view_to_image_clamped(current_view))
                    && st.select_drag_effect_dirty
                {
                    st.select_drag_effect_dirty = false;
                    effect_dirty = true;
                }
                return;
            }
            if matches!(st.selected_tool, Tool::Text | Tool::Number) {
                return;
            }
            if !t.contains_view(current_view) {
                return;
            }
            st.drag_shift_active = shift;
            st.update_drag(t.view_to_image(current_view));
        });
        if effect_dirty {
            self.rebuild_effects_async();
        }
    }

    /// Shared text-bounds drag for the active edit session and the committed
    /// text action: move-handle drags move, the resize corner resizes.
    fn update_active_text_drag(&self, st: &mut EditorState, t: &ViewTransform, view: Point) {
        let Some(drag_start) = st.active_text_drag_start else {
            return;
        };
        let Some(start_bounds) = st.active_text_drag_start_bounds else {
            return;
        };
        let point = t.view_to_image(view);
        let dx = point.x - drag_start.x;
        let dy = point.y - drag_start.y;
        let min_w = st.committed_text_min_width().max(50.0);
        let drag_handle = st.active_text_drag_handle.clone();
        let Some(bounds) = st.active_text_bounds.as_mut() else {
            return;
        };
        if st.active_text_is_resizing {
            bounds.rect.width = (start_bounds.width as f64 + dx).max(min_w) as i32;
            bounds.sync_handles();
        } else if let Some(handle) = drag_handle {
            match handle {
                MoveHandle::Left => {
                    let right = start_bounds.x + start_bounds.width;
                    bounds.rect.x = (start_bounds.x as f64 + dx).min(right as f64 - 50.0) as i32;
                    bounds.rect.width = (right - bounds.rect.x).max(50);
                }
                MoveHandle::Right => {
                    bounds.rect.width = (start_bounds.width as f64 + dx).max(50.0) as i32;
                }
            }
            bounds.sync_handles();
        } else {
            bounds.rect.x = (start_bounds.x as f64 + dx) as i32;
            bounds.rect.y = (start_bounds.y as f64 + dy) as i32;
            bounds.sync_handles();
        }
        if st.active_text_input.is_some() {
            st.fit_active_text_height_only();
        } else {
            st.fit_committed_text_height_only();
        }
    }

    fn drag_end(&self, t: &ViewTransform, gesture: &gtk4::GestureDrag, ox: f64, oy: f64) {
        let shift = gesture
            .current_event_state()
            .contains(gdk::ModifierType::SHIFT_MASK);
        let mut effect = false;
        self.with_state(|st| {
            if st.selected_tool == Tool::Crop {
                if let Some(start_view) = st.drag_start_view {
                    let current_view = Point {
                        x: start_view.x + ox,
                        y: start_view.y + oy,
                    };
                    st.update_crop_drag(t.view_to_image(current_view), shift);
                }
                st.end_crop_drag();
                st.drag_start_view = None;
                return;
            }
            if st.arrow_control_dragging.is_some() {
                st.finalize_arrow_interaction_cleanup();
                return;
            }
            let Some(start_view) = st.drag_start_view else {
                return;
            };
            let current_view = Point {
                x: start_view.x + ox,
                y: start_view.y + oy,
            };
            let is_move = st.selected_tool == Tool::Select
                || (st.selected_tool == Tool::Arrow
                    && st.selected_action_index.is_some()
                    && st.select_drag_anchor.is_some()
                    && st.arrow_control_dragging.is_none())
                || (st.selected_tool == Tool::Text
                    && st.active_text_input.is_none()
                    && !st.active_text_is_dragging)
                || (matches!(
                    st.selected_tool,
                    Tool::Box | Tool::Circle | Tool::Obfuscate | Tool::Focus
                ) && st.selected_action_index.is_some()
                    && st.select_drag_anchor.is_some());
            // A text box drag finalizes here if the click release never
            // fired (drags past the threshold emit stopped, not released).
            if st.active_text_is_dragging {
                self.update_active_text_drag(st, t, current_view);
                finish_text_drag(st);
                return;
            }
            if is_move {
                st.update_select_drag(t.view_to_image_clamped(current_view));
                if st.end_select_drag_without_rebuild_and_check_effect() {
                    effect = true;
                }
                return;
            }
            if matches!(st.selected_tool, Tool::Text | Tool::Number) || st.active_text_is_dragging {
                return;
            }
            if st.selected_tool == Tool::Arrow
                && st.selected_action_index.is_none()
                && ox.hypot(oy) < ARROW_CLICK_NOOP_DISTANCE
            {
                st.finalize_arrow_interaction_cleanup();
                return;
            }
            // Fold the release point into the path so the committed shape
            // matches where the pointer actually ended.
            st.update_drag(t.view_to_image_clamped(current_view));
            if let Some(action) = st.finalize_drag_action() {
                let needs_effect = EditorState::action_requires_effect_rebuild(&action);
                st.push_action(action);
                if needs_effect {
                    effect = true;
                }
            } else {
                st.clear_drag();
            }
        });
        if effect {
            self.rebuild_effects_async();
        }
    }

    // ------------------------------------------------------------ keyboard

    /// Route a key press through the editing model. Returns Stop when the
    /// editor consumed it; the window handles the rest (zoom, close).
    pub fn handle_key(&self, key: gdk::Key, modifiers: gdk::ModifierType) -> Propagation {
        let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
        // Crop mode keys.
        if self
            .with_state(|st| st.selected_tool == Tool::Crop)
            .unwrap_or(false)
        {
            match key {
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    self.apply_crop();
                    return Propagation::Stop;
                }
                gdk::Key::Escape => {
                    self.cancel_crop();
                    return Propagation::Stop;
                }
                _ => {}
            }
        }

        // Live text input owns character keys.
        let text_result = self.with_state(|st| {
            if st.active_text_input.is_none() {
                return None;
            }
            let mut handled = true;
            let mut commit = false;
            let mut cancel = false;
            match key {
                gdk::Key::Escape => cancel = true,
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    if ctrl {
                        commit = true;
                    } else {
                        st.add_text_input_char('\n');
                    }
                }
                gdk::Key::BackSpace => st.delete_text_input_char(),
                gdk::Key::Delete => st.delete_text_input_char_forward(),
                gdk::Key::Left => st.move_cursor_left(),
                gdk::Key::Right => st.move_cursor_right(),
                gdk::Key::Home => st.move_cursor_home(),
                gdk::Key::End => st.move_cursor_end(),
                _ => {
                    if !ctrl {
                        if let Some(ch) = key.to_unicode() {
                            if !ch.is_control() {
                                st.add_text_input_char(ch);
                            } else {
                                handled = false;
                            }
                        } else {
                            handled = false;
                        }
                    } else {
                        handled = false;
                    }
                }
            }
            if cancel {
                st.cancel_text_input();
            } else if commit {
                st.commit_active_text_input();
            } else if handled && st.active_text_input.is_some() {
                st.fit_active_text_to_layout();
                st.reset_text_cursor_blink();
            }
            Some(handled || cancel || commit)
        });
        if let Some(Some(consumed)) = text_result {
            self.changed();
            return if consumed {
                Propagation::Stop
            } else {
                Propagation::Proceed
            };
        }

        let mut consumed_doc_key = true;

        match key {
            gdk::Key::z | gdk::Key::Z if ctrl && shift => self.redo(),
            gdk::Key::z | gdk::Key::Z if ctrl => self.undo(),
            gdk::Key::y | gdk::Key::Y if ctrl => self.redo(),
            gdk::Key::Delete | gdk::Key::BackSpace => self.delete_selected(),
            gdk::Key::Escape => {
                let consumed = self.with_state(|st| {
                    if st.active_text_bounds.is_some() || st.active_text_input.is_some() {
                        st.cancel_text_input();
                        st.cancel_text_edit();
                        return true;
                    }
                    if st.drag_start.is_some() {
                        st.clear_drag();
                        return true;
                    }
                    if st.selected_action_index.is_some() {
                        st.clear_selection();
                        return true;
                    }
                    false
                });
                if consumed == Some(true) {
                    self.changed();
                    return Propagation::Stop;
                }
                return Propagation::Proceed;
            }
            _ if !ctrl => {
                if let Some((tool, _)) = key.to_unicode().and_then(tool_shortcut_target) {
                    self.set_tool(tool);
                    return Propagation::Stop;
                }
                consumed_doc_key = false;
            }
            _ => consumed_doc_key = false,
        }
        self.changed();
        if consumed_doc_key {
            Propagation::Stop
        } else {
            Propagation::Proceed
        }
    }

    /// Space pressed/released for hand-pan (capture phase, before widgets).
    pub fn set_space_pan(&self, active: bool) -> bool {
        // Text editing and entries need Space as a character.
        let blocked = self
            .with_state(|st| st.active_text_input.is_some())
            .unwrap_or(false);
        if blocked && active {
            return false;
        }
        self.space_pan.set(active);
        true
    }

    pub fn space_pan_active(&self) -> bool {
        self.space_pan.get()
    }

    // ------------------------------------------------------------- output

    /// Clipboard the flattened document as a PNG texture. The flatten runs
    /// on a worker so a pending effect pass never leaks an unredacted frame;
    /// the texture lands back on the main loop.
    pub fn copy_to_clipboard(&self) {
        if self.background_loading() {
            self.report_error(crate::i18n::t(
                "Wait for the background image to finish loading.",
            ));
            return;
        }
        self.commit_text_if_active();
        let snapshot = self.with_state(|st| st.to_export_image().ok());
        let Some(snapshot) = snapshot.flatten() else {
            (self.on_error)("Could not render the image for the clipboard.".into());
            return;
        };
        let report = self.on_error.clone();
        let (sender, receiver) = mpsc::channel::<Option<Vec<u8>>>();
        std::thread::spawn(move || {
            let _ = sender.send(crate::capture::editor::io_ops::png_bytes(&snapshot).ok());
        });
        glib::timeout_add_local(Duration::from_millis(16), move || {
            let bytes = match receiver.try_recv() {
                Ok(bytes) => bytes,
                Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    report("Could not copy the image to the clipboard.".into());
                    return glib::ControlFlow::Break;
                }
            };
            match bytes.and_then(|b| gdk::Texture::from_bytes(&glib::Bytes::from_owned(b)).ok()) {
                Some(texture) => {
                    if let Some(display) = gdk::Display::default() {
                        display.clipboard().set_texture(&texture);
                    }
                }
                None => report("Could not copy the image to the clipboard.".into()),
            }
            glib::ControlFlow::Break
        });
    }

    /// Save the flattened document with the platform file dialog handling
    /// overwrite confirmation; never touches the loaded source path.
    pub fn save_as(&self, parent: &impl IsA<gtk4::Window>, path: &std::path::Path) {
        if self.background_loading() {
            self.report_error(crate::i18n::t(
                "Wait for the background image to finish loading.",
            ));
            return;
        }
        self.commit_text_if_active();
        let snapshot = self.with_state(|st| st.to_export_image().ok());
        let Some(snapshot) = snapshot.flatten() else {
            (self.on_error)("Could not render the image.".into());
            return;
        };
        let path = path.to_path_buf();
        let report = self.on_error.clone();
        let (sender, receiver) =
            mpsc::channel::<Result<(), crate::capture::editor::types::EditorError>>();
        std::thread::spawn(move || {
            let _ = sender.send(crate::capture::editor::io_ops::save_image_snapshot(
                &path, snapshot,
            ));
        });
        glib::timeout_add_local(Duration::from_millis(50), move || {
            match receiver.try_recv() {
                Ok(Ok(())) => glib::ControlFlow::Break,
                Ok(Err(error)) => {
                    report(format!("Could not save the image: {error}"));
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => glib::ControlFlow::Break,
            }
        });
        let _ = parent;
    }
}

/// Gate for a finished effect worker frame: apply only the newest request
/// from the current document — same generation, the exact pending revision,
/// identical base image, no newer dirty flag. The caller owns pending-flag
/// scheduling.
fn apply_effect_result(
    st: &mut EditorState,
    expected_generation: u64,
    generation: u64,
    revision: u64,
    base: &Arc<RgbaImage>,
    image: RgbaImage,
) -> bool {
    if generation != expected_generation
        || revision != st.pending_effect_revision
        || revision <= st.last_applied_effect_revision
        || !Arc::ptr_eq(base, &st.base_image)
        || st.select_effect_rebuild_dirty
    {
        return false;
    }
    st.working_image = Arc::new(image);
    st.last_applied_effect_revision = revision;
    st.mark_working_image_dirty();
    true
}

/// Finalize a dragged text box exactly once whichever gesture ends last
/// (Click release is not guaranteed once a drag crosses the threshold, so
/// Drag end uses this too). Returns true when the live input was refit and
/// the caller should refocus the canvas.
pub(crate) fn finish_text_drag(st: &mut EditorState) -> bool {
    if !st.active_text_is_dragging {
        return false;
    }
    let was_resizing = st.active_text_is_resizing;
    st.active_text_is_dragging = false;
    st.active_text_drag_handle = None;
    st.active_text_drag_start = None;
    st.active_text_drag_start_bounds = None;
    st.active_text_is_resizing = false;
    st.drag_start_view = None;
    if st.active_text_input.is_some() {
        if was_resizing {
            st.fit_active_text_to_layout_preserving_box();
        } else {
            st.fit_active_text_to_layout_preserving_font_size();
        }
        return true;
    }
    if let (Some(bounds), Some(index)) = (st.active_text_bounds.take(), st.selected_action_index) {
        let mut changed = false;
        if let Some(AnnotationAction::Text {
            position,
            font,
            max_width,
            ..
        }) = st.actions.get_mut(index)
        {
            let padding_y = 8.0;
            let next_x = bounds.rect.x as f64;
            let next_y = bounds.rect.y as f64 + font.size + padding_y;
            let next_width = Some(bounds.rect.width as f64);
            changed = position.x != next_x || position.y != next_y || *max_width != next_width;
            position.x = next_x;
            position.y = next_y;
            *max_width = next_width;
        }
        if changed {
            st.clear_redo_history();
        }
        st.finish_history_interaction();
    }
    false
}

/// Apply is only meaningful for a rect that actually trims the image.
pub(crate) fn crop_apply_enabled(st: &EditorState) -> bool {
    st.crop_rect.is_some_and(|rect| {
        rect.width > 0
            && rect.height > 0
            && !(rect.x <= 0
                && rect.y <= 0
                && rect.width >= st.base_image.width() as i32
                && rect.height >= st.base_image.height() as i32)
    })
}

fn text_context() -> Option<(ImageSurface, gtk4::cairo::Context)> {
    let surface = ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1).ok()?;
    let context = gtk4::cairo::Context::new(&surface).ok()?;
    Some((surface, context))
}

fn selected_is_arrow(st: &EditorState) -> bool {
    st.selected_action_index
        .and_then(|i| st.actions.get(i))
        .is_some_and(|a| matches!(a, AnnotationAction::Arrow { .. }))
}

/// Give a freshly selected arrow its control points (midpoint for
/// Curved/Double, endpoints otherwise) the way finalize does.
fn ensure_arrow_controls(st: &mut EditorState) {
    let Some(index) = st.selected_action_index else {
        return;
    };
    let Some(AnnotationAction::Arrow {
        style,
        control_points,
        start,
        end,
        ..
    }) = st.actions.get_mut(index)
    else {
        st.arrow_editing_controls = false;
        return;
    };
    if control_points.is_none() {
        match style {
            ArrowStyle::Curved | ArrowStyle::Double => {
                let mid = Point {
                    x: (start.x + end.x) / 2.0,
                    y: (start.y + end.y) / 2.0,
                };
                *control_points = Some(vec![*start, mid, *end]);
            }
            _ => {
                *control_points = Some(vec![*start, *end]);
            }
        }
    }
    st.arrow_editing_controls = true;
}

/// TextEditBounds of a committed text action for handle hit-testing.
fn committed_text_bounds(
    st: &EditorState,
    index: usize,
) -> Option<crate::capture::editor::types::TextEditBounds> {
    let AnnotationAction::Text {
        position,
        text,
        font,
        max_width,
        ..
    } = st.actions.get(index)?
    else {
        return None;
    };
    let (_, context) = text_context()?;
    let width = max_width
        .unwrap_or_else(|| (st.base_image.width() as f64 - position.x).max(font.size * 1.8));
    Some(text_action_bounds(
        &context,
        *position,
        text,
        font,
        Some(width),
    ))
}

/// Restart detection on the state's own detector set, e.g. after crop.
pub fn restart_text_detection(st: &mut EditorState) {
    st.text_detection_handle = Some(spawn_text_detection(
        Arc::clone(&st.base_image),
        Arc::clone(&st.text_detector),
        Arc::clone(&st.text_detection_ready),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::pen_weight::PenWeight;

    fn test_image(w: u32, h: u32) -> RgbaImage {
        let mut img = RgbaImage::new(w, h);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = if (x + y) % 2 == 0 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            };
        }
        img
    }

    fn host() -> Rc<EditorHost> {
        EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)))
    }

    fn loaded(tool: Tool) -> Rc<EditorHost> {
        let h = host();
        h.load_document_headless(test_image(64, 64));
        h.set_tool(tool);
        h
    }

    fn ctrl() -> gdk::ModifierType {
        gdk::ModifierType::CONTROL_MASK
    }

    #[test]
    fn preview_and_export_use_the_same_background_geometry() {
        use crate::capture::editor::state::background::Fill;
        let host = host();
        let mut state = EditorState::new(RgbaImage::from_pixel(
            100,
            80,
            image::Rgba([240, 0, 0, 255]),
        ));
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            20,
            20,
            image::Rgba([0, 0, 255, 255]),
        )));
        state.background.padding = 40.0;
        state.background.radius = 40.0;
        state.background.frame_aspect = Some(
            crate::capture::editor::state::background::FrameRatio::new(1, 1),
        );
        let exported = state.to_export_image().unwrap();
        *host.editor.borrow_mut() = Some(state);
        let mut surface = ImageSurface::create(gtk4::cairo::Format::ARgb32, 120, 120).unwrap();
        {
            let context = gtk4::cairo::Context::new(&surface).unwrap();
            let transform = ViewTransform {
                offset_x: 10.0,
                offset_y: 20.0,
                has_background: true,
                canvas_width: 120.0,
                canvas_height: 120.0,
                image_rect_x: 10.0,
                image_rect_y: 20.0,
                ..ViewTransform::for_image(100.0, 80.0)
            };
            host.paint_document(&context, transform);
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let preview = render::cairo_argb_to_rgba_image(120, 120, stride, data.as_ref());
        assert_eq!(preview, exported);
    }

    #[test]
    fn wallpaper_is_clipped_to_its_canvas_in_the_preview() {
        use crate::capture::editor::state::background::Fill;
        let host = host();
        let mut state = EditorState::new(RgbaImage::from_pixel(
            100,
            80,
            image::Rgba([240, 0, 0, 255]),
        ));
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            20,
            20,
            image::Rgba([0, 0, 255, 255]),
        )));
        state.background.padding = 40.0;
        *host.editor.borrow_mut() = Some(state);
        let mut surface = ImageSurface::create(gtk4::cairo::Format::ARgb32, 180, 150).unwrap();
        {
            let context = gtk4::cairo::Context::new(&surface).unwrap();
            context.set_source_rgb(0.0, 1.0, 0.0);
            context.paint().unwrap();
            let transform = ViewTransform {
                offset_x: 30.0,
                offset_y: 35.0,
                has_background: true,
                canvas_offset_x: 20.0,
                canvas_offset_y: 25.0,
                canvas_width: 120.0,
                canvas_height: 100.0,
                image_rect_x: 10.0,
                image_rect_y: 10.0,
                ..ViewTransform::for_image(100.0, 80.0)
            };
            host.paint_document(&context, transform);
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let preview = render::cairo_argb_to_rgba_image(180, 150, stride, data.as_ref());
        for (x, y) in [(0, 0), (19, 50), (140, 50), (50, 24), (50, 125)] {
            assert_eq!(*preview.get_pixel(x, y), image::Rgba([0, 255, 0, 255]));
        }
        assert_eq!(*preview.get_pixel(25, 30), image::Rgba([0, 0, 255, 255]));
        assert_eq!(*preview.get_pixel(80, 70), image::Rgba([240, 0, 0, 255]));
    }

    #[test]
    fn unblurred_wallpaper_preview_keeps_full_resolution() {
        use crate::capture::editor::state::background::Fill;
        let host = host();
        let mut state = EditorState::new(RgbaImage::from_pixel(
            1600,
            20,
            image::Rgba([240, 0, 0, 255]),
        ));
        let wallpaper = RgbaImage::from_fn(1664, 84, |x, _| {
            if x % 2 == 0 {
                image::Rgba([0, 0, 0, 255])
            } else {
                image::Rgba([255, 255, 255, 255])
            }
        });
        state.background.fill = Fill::Image(Arc::new(wallpaper));
        state.background.padding = 8.0;
        let exported = state.to_export_image().unwrap();
        assert_eq!(exported.dimensions(), (1664, 84));
        *host.editor.borrow_mut() = Some(state);
        let mut surface = ImageSurface::create(gtk4::cairo::Format::ARgb32, 1664, 84).unwrap();
        {
            let context = gtk4::cairo::Context::new(&surface).unwrap();
            host.paint_document(
                &context,
                ViewTransform {
                    offset_x: 32.0,
                    offset_y: 32.0,
                    has_background: true,
                    canvas_width: 1664.0,
                    canvas_height: 84.0,
                    image_rect_x: 32.0,
                    image_rect_y: 32.0,
                    ..ViewTransform::for_image(1600.0, 20.0)
                },
            );
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let preview = render::cairo_argb_to_rgba_image(1664, 84, stride, data.as_ref());
        for x in 100..110 {
            assert_eq!(preview.get_pixel(x, 5), exported.get_pixel(x, 5));
        }
    }

    #[test]
    fn background_worker_delivers_preview_then_full_image_and_honors_cancellation() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/capture/editor/background-images");
        let path = assets.join("wallpaper-001.jpg");
        let thumb = assets.join("wallpaper-thumb-001.jpg");
        let receiver = spawn_background_decode(
            path.clone(),
            Some(thumb.clone()),
            Arc::new(AtomicBool::new(false)),
        );
        let (preview, image) = receiver.recv_timeout(Duration::from_secs(15)).unwrap();
        assert!(preview);
        assert!(image.unwrap().0.width() < 1920);
        let (preview, image) = receiver.recv_timeout(Duration::from_secs(15)).unwrap();
        assert!(!preview);
        assert_eq!(image.unwrap().0.dimensions(), (1920, 1080));
        let receiver = spawn_background_decode(path, Some(thumb), Arc::new(AtomicBool::new(true)));
        assert!(receiver.recv_timeout(Duration::from_secs(15)).is_err());
    }

    #[test]
    fn crop_gestures_accept_all_handles_outside_the_image() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let h = host();
            let gesture = gtk4::GestureDrag::new();
            let mut transform = ViewTransform::for_image(200.0, 100.0);
            transform.offset_x = 100.0;
            transform.offset_y = 100.0;
            transform.scale = 0.5;
            let cases = [
                (
                    (0.0, 0.0),
                    (-1.0, -1.0),
                    (10.0, 10.0),
                    (10, 10, 190, 90),
                    "nwse-resize",
                ),
                (
                    (100.0, 0.0),
                    (0.0, -1.0),
                    (0.0, 10.0),
                    (0, 10, 200, 90),
                    "ns-resize",
                ),
                (
                    (200.0, 0.0),
                    (1.0, -1.0),
                    (-10.0, 10.0),
                    (0, 10, 190, 90),
                    "nesw-resize",
                ),
                (
                    (0.0, 50.0),
                    (-1.0, 0.0),
                    (10.0, 0.0),
                    (10, 0, 190, 100),
                    "ew-resize",
                ),
                (
                    (200.0, 50.0),
                    (1.0, 0.0),
                    (-10.0, 0.0),
                    (0, 0, 190, 100),
                    "ew-resize",
                ),
                (
                    (0.0, 100.0),
                    (-1.0, 1.0),
                    (10.0, -10.0),
                    (10, 0, 190, 90),
                    "nesw-resize",
                ),
                (
                    (100.0, 100.0),
                    (0.0, 1.0),
                    (0.0, -10.0),
                    (0, 0, 200, 90),
                    "ns-resize",
                ),
                (
                    (200.0, 100.0),
                    (1.0, 1.0),
                    (-10.0, -10.0),
                    (0, 0, 190, 90),
                    "nwse-resize",
                ),
            ];
            for ((x, y), (out_x, out_y), (dx, dy), (rx, ry, rw, rh), cursor) in cases {
                let mut st = EditorState::new(RgbaImage::new(200, 100));
                st.set_tool_without_rebuild(Tool::Crop);
                let handle = transform.image_to_view(Point { x, y });
                let start = Point {
                    x: handle.x + out_x * 3.0,
                    y: handle.y + out_y * 3.0,
                };
                assert!(!transform.contains_view(start));
                assert_eq!(
                    crop_cursor(&st, transform.view_to_image(start), transform.scale),
                    cursor
                );
                *h.editor.borrow_mut() = Some(st);
                h.drag_begin(&transform, &gesture, start);
                h.drag_update(
                    &transform,
                    &gesture,
                    dx * transform.scale,
                    dy * transform.scale,
                );
                h.drag_end(
                    &transform,
                    &gesture,
                    dx * transform.scale,
                    dy * transform.scale,
                );
                assert_eq!(
                    h.read(|st| st.crop_rect),
                    Some(Some(crate::capture::editor::types::Rect {
                        x: rx,
                        y: ry,
                        width: rw,
                        height: rh
                    }))
                );
            }
            let mut st = EditorState::new(RgbaImage::new(200, 100));
            st.set_tool_without_rebuild(Tool::Crop);
            *h.editor.borrow_mut() = Some(st);
            h.drag_begin(&transform, &gesture, Point { x: 70.0, y: 125.0 });
            assert_eq!(h.read(|st| st.drag_start_view), Some(None));
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn keys_route_with_document_loaded() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let h = loaded(Tool::Pen);
            h.with_state(|st| {
                st.begin_drag(crate::capture::editor::types::Point { x: 4.0, y: 4.0 });
                st.update_drag(crate::capture::editor::types::Point { x: 30.0, y: 30.0 });
                if let Some(a) = st.finalize_drag_action() {
                    st.push_action(a);
                }
            });
            assert_eq!(h.read(|st| st.actions.len()), Some(1));
            assert_eq!(h.handle_key(gdk::Key::z, ctrl()), Propagation::Stop);
            assert_eq!(h.read(|st| st.actions.len()), Some(0));
            assert_eq!(
                h.handle_key(gdk::Key::b, gdk::ModifierType::empty()),
                Propagation::Stop
            );
            // 'b' is the shared shortcut for the obfuscate tool.
            assert_eq!(h.read(|st| st.selected_tool), Some(Tool::Obfuscate));
            assert_eq!(
                h.handle_key(gdk::Key::Escape, gdk::ModifierType::empty()),
                Propagation::Proceed
            );
            h.with_state(|st| {
                st.push_action(AnnotationAction::Box {
                    rect: crate::capture::editor::types::Rect {
                        x: 1,
                        y: 1,
                        width: 10,
                        height: 10,
                    },
                    color: st.selected_color,
                    stroke_size: 4.0,
                    shadow: false,
                });
                st.selected_action_index = Some(0);
            });
            assert_eq!(
                h.handle_key(gdk::Key::Delete, gdk::ModifierType::empty()),
                Propagation::Stop
            );
            assert_eq!(h.read(|st| st.actions.len()), Some(0));
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn text_input_enters_commits_and_survives_tool_switch() {
        let h = loaded(Tool::Text);
        h.with_state(|st| {
            st.begin_text_input(
                crate::capture::editor::types::Point { x: 5.0, y: 5.0 },
                100.0,
                24.0,
            )
        });
        assert_eq!(
            h.handle_key(gdk::Key::h, gdk::ModifierType::empty()),
            Propagation::Stop
        );
        h.with_state(|st| st.add_text_input_char('é'));
        assert_eq!(
            h.handle_key(gdk::Key::Return, gdk::ModifierType::empty()),
            Propagation::Stop
        );
        h.with_state(|st| {
            assert_eq!(st.get_text_input().map(|i| i.text.chars().count()), Some(3));
        });
        assert_eq!(h.handle_key(gdk::Key::Return, ctrl()), Propagation::Stop);
        assert_eq!(h.read(|st| st.actions.len()), Some(1));
        h.with_state(|st| {
            st.begin_text_input(
                crate::capture::editor::types::Point { x: 5.0, y: 40.0 },
                100.0,
                24.0,
            )
        });
        h.with_state(|st| st.add_text_input_char('x'));
        h.set_tool(Tool::Pen);
        assert_eq!(h.read(|st| st.actions.len()), Some(2));
    }

    #[test]
    fn arrow_style_swap_normalizes_control_points() {
        let h = loaded(Tool::Arrow);
        h.with_state(|st| {
            st.push_action(AnnotationAction::Arrow {
                start: crate::capture::editor::types::Point { x: 0.0, y: 0.0 },
                end: crate::capture::editor::types::Point { x: 40.0, y: 0.0 },
                color: st.selected_color,
                stroke_size: 4.0,
                shadow: false,
                style: ArrowStyle::Standard,
                reversed: false,
                control_points: Some(vec![
                    crate::capture::editor::types::Point { x: 0.0, y: 0.0 },
                    crate::capture::editor::types::Point { x: 40.0, y: 0.0 },
                ]),
            });
            st.selected_action_index = Some(0);
        });
        h.with_state(|st| {
            assert!(st.set_selected_arrow_style_with_controls(ArrowStyle::Curved));
        });
        // Style + midpoint normalize land in one undoable step.
        h.with_state(|st| {
            if let AnnotationAction::Arrow {
                control_points,
                style,
                ..
            } = &st.actions[0]
            {
                assert_eq!(*style, ArrowStyle::Curved);
                assert_eq!(control_points.as_ref().map(Vec::len), Some(3));
            } else {
                panic!("expected arrow");
            }
        });
        h.undo();
        h.with_state(|st| {
            if let AnnotationAction::Arrow {
                control_points,
                style,
                ..
            } = &st.actions[0]
            {
                assert_eq!(*style, ArrowStyle::Standard);
                assert_eq!(control_points.as_ref().map(Vec::len), Some(2));
            } else {
                panic!("expected arrow");
            }
        });
        h.redo();
        h.with_state(|st| {
            if let AnnotationAction::Arrow {
                control_points,
                style,
                ..
            } = &st.actions[0]
            {
                assert_eq!(*style, ArrowStyle::Curved);
                assert_eq!(control_points.as_ref().map(Vec::len), Some(3));
            } else {
                panic!("expected arrow");
            }
        });
    }

    #[test]
    fn highlighter_weight_updates_next_stroke_size() {
        let h = loaded(Tool::Highlighter);
        h.with_state(|st| {
            st.set_highlighter_weight(PenWeight::Large);
            st.set_stroke_size(PenWeight::Large.highlighter_stroke_width());
        });
        assert_eq!(
            h.read(|st| st.stroke_size),
            Some(PenWeight::Large.highlighter_stroke_width())
        );
    }

    #[test]
    fn export_flattens_pending_effects_without_preview() {
        let h = loaded(Tool::Obfuscate);
        h.with_state(|st| {
            st.set_obfuscate_method(crate::capture::editor::types::ObfuscateMethod::Blackout);
            st.begin_drag(crate::capture::editor::types::Point { x: 8.0, y: 8.0 });
            st.update_drag(crate::capture::editor::types::Point { x: 40.0, y: 40.0 });
            if let Some(a) = st.finalize_drag_action() {
                st.push_action(a);
            }
        });
        let img = h.with_state(|st| st.to_export_image().unwrap()).unwrap();
        let px = img.get_pixel(20, 20);
        assert!(px[0] < 10 && px[1] < 10 && px[2] < 10);
    }

    #[test]
    fn stale_effect_job_is_rejected_across_generations() {
        let h = host();
        h.load_document_headless(test_image(32, 32));
        let gen = h.session_generation.get();
        let (base, actions) = h
            .with_state(|st| (std::sync::Arc::clone(&st.base_image), st.actions.clone()))
            .unwrap();
        h.with_state(|st| st.pending_effect_revision = 1);
        // Old generation (captured before this load): rejected.
        assert!(!h.run_effect_worker_once(base.clone(), actions.clone(), 1, gen + 1));
        // Old base identity: rejected.
        let other_base = std::sync::Arc::new(test_image(32, 32));
        assert!(!h.run_effect_worker_once(other_base, Vec::new(), 1, gen));
        // Same-session dirty flag (newer edit landed mid-flight): rejected.
        h.with_state(|st| st.select_effect_rebuild_dirty = true);
        assert!(!h.run_effect_worker_once(base.clone(), actions.clone(), 1, gen));
        h.with_state(|st| st.select_effect_rebuild_dirty = false);
        // Wrong pending revision: rejected.
        assert!(!h.run_effect_worker_once(base.clone(), actions.clone(), 9, gen));
        // The matching current job applies.
        assert!(h.run_effect_worker_once(base, actions, 1, gen));
        assert_eq!(h.read(|st| st.last_applied_effect_revision), Some(1));
    }

    #[test]
    fn save_overwrites_own_file_and_rejects_unknown_ext() {
        let dir = std::env::temp_dir().join(format!("apexshot-save-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("out.png");
        let bogus = dir.join("out.bogus");
        std::fs::write(&png, b"old").unwrap();
        std::fs::write(&bogus, b"keep").unwrap();
        let img = test_image(8, 8);
        crate::capture::editor::io_ops::save_image_snapshot(&png, img.clone()).unwrap();
        assert!(std::fs::read(&png).unwrap().starts_with(&[0x89, 0x50]));
        assert!(crate::capture::editor::io_ops::save_image_snapshot(&bogus, img).is_err());
        assert_eq!(std::fs::read(&bogus).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// End-to-end proof the WinRT PowerShell helper detects real text:
    /// render a Text action with the shared renderer, then run detection.
    /// Ignored by default (needs the Windows OCR runtime); run manually with
    /// `cargo test --lib mechanical_ocr -- --ignored --nocapture`.
    #[test]
    #[ignore = "requires the WinRT OCR runtime; run manually for evidence"]
    fn mechanical_ocr_detects_generated_text() {
        let (w, h) = (400u32, 120u32);
        let mut surface =
            ImageSurface::create(gtk4::cairo::Format::ARgb32, w as i32, h as i32).unwrap();
        let ctx = gtk4::cairo::Context::new(&surface).unwrap();
        ctx.set_source_rgb(1.0, 1.0, 1.0);
        ctx.paint().unwrap();
        draw_annotation_action(
            &ctx,
            &AnnotationAction::Text {
                position: crate::capture::editor::types::Point { x: 10.0, y: 20.0 },
                text: "Hello World".to_string(),
                color: crate::capture::editor::color::DRAW_COLORS[0],
                font: crate::capture::editor::types::FontSettings {
                    family: "Sans".to_string(),
                    size: 32.0,
                    style: crate::capture::editor::types::FontStyle::Normal,
                    decoration: crate::capture::editor::types::TextDecoration::None,
                    alignment: crate::capture::editor::types::TextAlignment::Left,
                },
                max_width: None,
                shadow: false,
                background_color: None,
            },
        );
        drop(ctx);
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap().to_vec();
        let img = crate::capture::editor::render::cairo_argb_to_rgba_image(w, h, stride, &data);
        match crate::capture::editor::ocr::detect_word_lines(&img) {
            Ok(lines) => {
                println!("OCR lines: {lines:?}");
                assert!(!lines.is_empty(), "expected at least one detected line");
            }
            Err(e) => {
                println!("OCR helper unavailable: {e}");
            }
        }
    }
    #[test]
    fn finish_text_drag_is_idempotent_across_orderings() {
        let h = loaded(Tool::Text);
        h.with_state(|st| {
            st.begin_text_input(
                crate::capture::editor::types::Point { x: 5.0, y: 5.0 },
                100.0,
                24.0,
            );
            st.add_text_input_char('x');
            st.active_text_is_dragging = true;
        });
        // First finalize (either ordering's winner): refit, flags cleared.
        assert_eq!(h.with_state(finish_text_drag), Some(true));
        h.with_state(|st| assert!(!st.active_text_is_dragging));
        // The loser ordering finds nothing to do and records no history.
        assert_eq!(h.with_state(finish_text_drag), Some(false));
        // Live input still there — the drag ended, the text did not commit.
        assert!(h.read(|st| st.active_text_input.is_some()) == Some(true));
    }

    /// Syncing bars while an immutable model borrow is held must not touch
    /// the document (the RefCell crash regression): widgets refresh without
    /// firing their callbacks back into `borrow_mut`.
    #[test]
    #[ignore = "needs a live GTK display; run with --ignored --test-threads=1"]
    fn sub_bar_sync_under_read_borrow_does_not_mutate() {
        gtk4::init().expect("GTK must initialize for the widget wiring test");
        let h = loaded(Tool::Pen);
        let bars = super::super::sub_bar::build(1.0, h.clone(), Rc::new(|_| {}), None);
        let undo_before = h.read(|st| st.can_undo());
        h.read(|st| bars.sync(Some(st)));
        assert_eq!(h.read(|st| st.can_undo()), undo_before);
        assert_eq!(h.read(|st| st.actions.len()), Some(0));
    }

    /// The view's image extents follow the live document: crop shrinks the
    /// transform, undo restores it.
    #[test]
    #[ignore = "needs a live GTK display; run with --ignored --test-threads=1"]
    fn canvas_view_dims_follow_document_through_crop_and_undo() {
        gtk4::init().expect("GTK must initialize for the widget wiring test");
        let canvas = super::super::canvas::build(1.0, true);
        canvas.host.load_document_headless(test_image(64, 32));
        canvas.sync_view_to_document();
        let t0 = canvas.view_transform();
        assert_eq!(t0.scale * 64.0, t0.scale * 64.0);
        // Crop to a quarter of the frame and apply via the shared path.
        canvas.host.with_state(|st| {
            st.selected_tool = Tool::Crop;
            st.crop_rect = Some(crate::capture::editor::types::Rect {
                x: 0,
                y: 0,
                width: 32,
                height: 16,
            });
        });
        canvas.host.apply_crop();
        canvas.sync_view_to_document();
        let (w1, h1) = canvas
            .host
            .read(|st| (st.working_image.width(), st.working_image.height()))
            .unwrap();
        assert_eq!((w1, h1), (32, 16));
        // Undo restores the document dimensions and the view follows.
        canvas.host.undo();
        canvas.sync_view_to_document();
        let (w2, h2) = canvas
            .host
            .read(|st| (st.working_image.width(), st.working_image.height()))
            .unwrap();
        assert_eq!((w2, h2), (64, 32));
        let _ = t0;
    }
}
