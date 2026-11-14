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
/// A new box or circle needs a deliberate drag on screen; anything shorter is
/// a click that jittered, and committing it would leave a dot behind.
const MIN_SHAPE_DRAG_DISTANCE: f64 = 16.0;
const PREVIEW_CARD_MAX_EDGE: u32 = 1600;
static BACKGROUND_DECODE_LOCK: Mutex<()> = Mutex::new(());

fn preview_card_size(width: u32, height: u32) -> (u32, u32) {
    let factor = (f64::from(PREVIEW_CARD_MAX_EDGE) / f64::from(width.max(height).max(1))).min(1.0);
    (
        (f64::from(width) * factor).round().max(1.0) as u32,
        (f64::from(height) * factor).round().max(1.0) as u32,
    )
}

type DecodedBackground = (RgbaImage, Vec<u8>);

/// How long the full wallpaper may take before its thumbnail stands in.
/// Most decodes finish inside it, so a switch goes straight to the sharp
/// image instead of flashing a blurry thumbnail first.
const BACKGROUND_PREVIEW_GRACE: Duration = Duration::from_millis(300);

fn spawn_background_decode(
    path: std::path::PathBuf,
    preview_path: Option<std::path::PathBuf>,
    cancel: Arc<AtomicBool>,
    grace: Duration,
) -> mpsc::Receiver<(bool, image::ImageResult<DecodedBackground>)> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let prepare = |image: image::DynamicImage| {
            let image = image.to_rgba8();
            let bytes = render::rgba_to_cairo_argb_bytes(&image);
            (image, bytes)
        };
        let (full_sender, full_receiver) = mpsc::channel();
        {
            let cancel = Arc::clone(&cancel);
            std::thread::spawn(move || {
                let Ok(_guard) = BACKGROUND_DECODE_LOCK.lock() else {
                    return;
                };
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let _ = full_sender.send(image::open(path));
            });
        }
        let send_full = |image: image::ImageResult<image::DynamicImage>| {
            if !cancel.load(Ordering::Relaxed) {
                let _ = sender.send((false, image.map(prepare)));
            }
        };
        match full_receiver.recv_timeout(grace) {
            Ok(image) => {
                send_full(image);
                return;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Some(path) = preview_path {
            if let Ok(image) = image::open(path) {
                if cancel.load(Ordering::Relaxed)
                    || sender.send((true, Ok(prepare(image)))).is_err()
                {
                    return;
                }
            }
        }
        if let Ok(image) = full_receiver.recv() {
            send_full(image);
        }
    });
    receiver
}

type CachedCard = (u64, Vec<AnnotationAction>, f64, ImageSurface);
/// The working image with its committed annotations, baked once per drag so
/// pointer updates only redraw the in-progress stroke.
type CachedAnnotations = (u64, usize, Option<usize>, ImageSurface);
/// Preview wallpaper: revision, image width and height, raster edge, surface.
type CachedBackground = (u64, u32, u32, u32, ImageSurface);
type CachedShadow = (
    crate::capture::editor::render::scene_shadow::SceneShadowGeometry,
    bool,
    f64,
    ImageSurface,
);
#[derive(Clone, Copy, PartialEq)]
enum MotionPaintPart {
    Complete,
    Background,
    Foreground,
    /// Everything under the annotations, for the cached base layer.
    Base,
}

/// Everything the cached base layer depends on. Annotations, the caret and
/// the text box are drawn over it, so typing and text drags never rebuild it.
#[derive(Clone, PartialEq)]
struct BaseKey {
    working_revision: u64,
    image_size: (u32, u32),
    background_revision: u64,
    fill: usize,
    padding: f64,
    blur: f64,
    radius: f64,
    noise: f64,
    frame_aspect: Option<crate::capture::editor::state::background::FrameRatio>,
    frame_style: crate::capture::editor::types::FrameStyle,
    shadow: crate::recording::editor::model::MotionSceneShadow,
    layout: crate::capture::editor::state::background::Layout,
    scale: f64,
    canvas_scale: f64,
    device_scale: f64,
}

/// A pen-drag frame: base key, action count, re-edited action, raster.
type CachedDragFrame = (BaseKey, usize, Option<usize>, ImageSurface);

pub struct EditorHost {
    pub editor: Shared,
    #[cfg(test)]
    pub(super) document_paints: Cell<usize>,
    /// Called after any document or selection mutation: redraw + sync bars.
    on_change: Rc<dyn Fn()>,
    /// Toast/banner reporter for OCR failures and export errors.
    on_error: Rc<dyn Fn(String)>,
    /// Prepared Cairo surface for the working image, keyed by its revision.
    surface_cache: RefCell<Option<(u64, ImageSurface)>>,
    card_cache: RefCell<Option<CachedCard>>,
    /// The card at the working image's own resolution, for the video encoder.
    /// Kept apart from `card_cache` so a preview-sized card is never painted
    /// into an export frame.
    export_card_cache: RefCell<Option<CachedCard>>,
    /// Built on the first paint of a pen/highlighter drag; dropped when the
    /// drag ends. Only used while no card transform is active.
    annotation_cache: RefCell<Option<CachedAnnotations>>,
    shadow_cache: RefCell<Option<CachedShadow>>,
    background_cache: RefCell<Option<CachedBackground>>,
    /// The base layer rastered at the on-screen size; see [`BaseKey`].
    base_cache: RefCell<Option<(BaseKey, ImageSurface)>>,
    #[cfg(test)]
    pub(super) base_builds: Cell<usize>,
    /// During a pen or highlighter drag: the base plus every committed
    /// annotation at the on-screen size, keyed by the base key, the action
    /// count and the action being re-edited.
    drag_frame_cache: RefCell<Option<CachedDragFrame>>,
    #[cfg(test)]
    pub(super) drag_frame_builds: Cell<usize>,
    studio_unlock_cache: RefCell<Option<(std::time::Instant, bool)>>,
    studio_text_ratio: Cell<Option<f64>>,
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
    /// Input method for the live text box. `install` creates it because it
    /// needs GTK, so headless hosts have none.
    text_input_method: RefCell<Option<gtk4::IMMulticontext>>,
    text_input_area: RefCell<Option<glib::WeakRef<DrawingArea>>>,
    text_input_transform: RefCell<Option<Rc<dyn Fn() -> ViewTransform>>>,
    text_input_focused: Cell<bool>,
}

impl EditorHost {
    pub fn new(
        on_change: Rc<dyn Fn()>,
        on_error: Rc<dyn Fn(String)>,
        view_pan: Rc<dyn Fn() -> (f64, f64)>,
    ) -> Rc<Self> {
        Rc::new(Self {
            editor: Rc::new(RefCell::new(None)),
            #[cfg(test)]
            document_paints: Cell::new(0),
            on_change,
            on_error,
            surface_cache: RefCell::new(None),
            card_cache: RefCell::new(None),
            export_card_cache: RefCell::new(None),
            annotation_cache: RefCell::new(None),
            shadow_cache: RefCell::new(None),
            background_cache: RefCell::new(None),
            base_cache: RefCell::new(None),
            #[cfg(test)]
            base_builds: Cell::new(0),
            drag_frame_cache: RefCell::new(None),
            #[cfg(test)]
            drag_frame_builds: Cell::new(0),
            studio_unlock_cache: RefCell::new(None),
            studio_text_ratio: Cell::new(None),
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
            text_input_method: RefCell::new(None),
            text_input_area: RefCell::new(None),
            text_input_transform: RefCell::new(None),
            text_input_focused: Cell::new(false),
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
        *self.card_cache.borrow_mut() = None;
        *self.export_card_cache.borrow_mut() = None;
        *self.annotation_cache.borrow_mut() = None;
        *self.shadow_cache.borrow_mut() = None;
        *self.background_cache.borrow_mut() = None;
        *self.base_cache.borrow_mut() = None;
        *self.drag_frame_cache.borrow_mut() = None;
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
        self.sync_text_input_method();
        (self.on_change)();
    }

    /// Keep the input method focused only while a text box is live, and park
    /// its composition window just under that box.
    fn sync_text_input_method(&self) {
        let Some(im) = self.text_input_method.borrow().clone() else {
            return;
        };
        let Ok(borrow) = self.editor.try_borrow() else {
            return;
        };
        let active = borrow.as_ref().and_then(|st| {
            st.active_text_input.as_ref()?;
            Some(st.active_text_bounds.as_ref().map(|bounds| bounds.rect))
        });
        drop(borrow);
        let Some(rect) = active else {
            if self.text_input_focused.replace(false) {
                im.focus_out();
            }
            return;
        };
        if !self.text_input_focused.replace(true) {
            im.focus_in();
        }
        let transform_of = self.text_input_transform.borrow().clone();
        if let (Some(rect), Some(transform_of)) = (rect, transform_of) {
            let t = transform_of();
            let top_left = t.image_to_view(Point {
                x: f64::from(rect.x),
                y: f64::from(rect.y),
            });
            let bottom_right = t.image_to_view(Point {
                x: f64::from(rect.x + rect.width),
                y: f64::from(rect.y + rect.height),
            });
            im.set_cursor_location(&gdk::Rectangle::new(
                top_left.x.round() as i32,
                bottom_right.y.round() as i32,
                (bottom_right.x - top_left.x).round().max(1.0) as i32,
                1,
            ));
        }
    }

    /// Offer a key event to the input method while a text box is live.
    /// Returns true when the input method keeps the key; every key typed
    /// during a composition belongs to it.
    pub fn filter_text_input_key(&self, event: &gdk::Event) -> bool {
        let Some(im) = self.text_input_method.borrow().clone() else {
            return false;
        };
        if !self
            .read(|st| st.active_text_input.is_some())
            .unwrap_or(false)
        {
            return false;
        }
        let composing = !im.preedit_string().0.is_empty();
        im.filter_keypress(event) || composing
    }

    /// Insert committed input-method text at the caret of the live text box.
    fn insert_text(&self, text: &str) {
        let inserted = self
            .editor
            .try_borrow_mut()
            .ok()
            .and_then(|mut borrow| borrow.as_mut().map(|st| insert_into_active_text(st, text)))
            .unwrap_or(false);
        if inserted {
            self.changed();
        }
    }

    /// Read clipboard text into the live text box. The read is async, so the
    /// text lands only if a box is still live in the same document.
    fn paste_text_from_clipboard(&self) {
        let Some(area) = self
            .text_input_area
            .borrow()
            .as_ref()
            .and_then(|area| area.upgrade())
        else {
            return;
        };
        let editor = self.editor.clone();
        let on_change = self.on_change.clone();
        let generation = self.session_generation.clone();
        let started = generation.get();
        area.clipboard()
            .read_text_async(gtk4::gio::Cancellable::NONE, move |result| {
                let Ok(Some(text)) = result else {
                    return;
                };
                if generation.get() != started {
                    return;
                }
                let inserted = editor
                    .try_borrow_mut()
                    .ok()
                    .and_then(|mut borrow| {
                        borrow.as_mut().map(|st| insert_into_active_text(st, &text))
                    })
                    .unwrap_or(false);
                if inserted {
                    on_change();
                }
            });
    }

    /// Route input-method commits into the live text box. The canvas draws
    /// no inline preedit, so the input method shows its own composition
    /// window at the cursor location `sync_text_input_method` sets.
    fn install_text_input_method(
        self: &Rc<Self>,
        area: &DrawingArea,
        transform_of: Rc<dyn Fn() -> ViewTransform>,
    ) {
        let im = gtk4::IMMulticontext::new();
        im.set_use_preedit(false);
        // The input method needs the canvas window, which exists only while
        // the canvas is realized.
        area.connect_realize({
            let host = Rc::downgrade(self);
            let im = im.downgrade();
            move |area| {
                let (Some(host), Some(im)) = (host.upgrade(), im.upgrade()) else {
                    return;
                };
                im.set_client_widget(Some(area));
                host.text_input_focused.set(false);
                host.sync_text_input_method();
            }
        });
        area.connect_unrealize({
            let host = Rc::downgrade(self);
            let im = im.downgrade();
            move |_| {
                let (Some(host), Some(im)) = (host.upgrade(), im.upgrade()) else {
                    return;
                };
                host.text_input_focused.set(false);
                im.set_client_widget(None::<&gtk4::Widget>);
            }
        });
        if area.is_realized() {
            im.set_client_widget(Some(area));
        }
        im.connect_commit({
            let host = Rc::downgrade(self);
            move |_, text| {
                if let Some(host) = host.upgrade() {
                    host.insert_text(text);
                }
            }
        });
        *self.text_input_area.borrow_mut() = Some(area.downgrade());
        *self.text_input_transform.borrow_mut() = Some(transform_of);
        *self.text_input_method.borrow_mut() = Some(im);
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

    /// The licence state, re-read at most once a second so painting never
    /// touches the entitlement file per frame.
    fn studio_unlocked_cached(&self) -> bool {
        let now = std::time::Instant::now();
        if let Some((at, value)) = *self.studio_unlock_cache.borrow() {
            if now.duration_since(at) < std::time::Duration::from_secs(1) {
                return value;
            }
        }
        let value = crate::studio_entitlement::studio_unlocked();
        *self.studio_unlock_cache.borrow_mut() = Some((now, value));
        value
    }

    /// Forget the cached licence state, e.g. right after activation.
    pub(crate) fn invalidate_studio_cache(&self) {
        *self.studio_unlock_cache.borrow_mut() = None;
    }

    /// Whether the mark is due: the document uses a Studio style and no
    /// licence covers this build.
    pub(crate) fn studio_mark_active(&self) -> bool {
        self.read(|st| st.uses_studio_style()).unwrap_or(false) && !self.studio_unlocked_cached()
    }

    /// Paint the Studio preview mark in screen pixels over the visible canvas.
    pub(crate) fn paint_studio_mark(
        &self,
        context: &gtk4::cairo::Context,
        canvas: crate::capture::editor::studio_watermark::ScreenRect,
        image: crate::capture::editor::studio_watermark::ScreenRect,
        visible: crate::capture::editor::studio_watermark::ScreenRect,
    ) {
        use crate::capture::editor::studio_watermark as mark;
        if !self.studio_mark_active() {
            return;
        }
        let Some(region) = canvas.intersect(visible) else {
            return;
        };
        let ratio = self.studio_text_ratio.get().unwrap_or_else(|| {
            let ratio = mark::text_ratio(context);
            self.studio_text_ratio.set(Some(ratio));
            ratio
        });
        let stage = mark::Stage {
            canvas,
            image,
            visible,
        };
        let Some(rect) = mark::place(stage, ratio) else {
            return;
        };
        let ink = self
            .read(|st| mark::ink_for_image(&st.working_image, rect, image))
            .unwrap_or_else(mark::Ink::light);
        let _ = context.save();
        context.rectangle(region.x, region.y, region.width, region.height);
        context.clip();
        mark::paint(context, rect, ink);
        let _ = context.restore();
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
        let receiver =
            spawn_background_decode(path, preview_path, cancel, BACKGROUND_PREVIEW_GRACE);
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
                    host.apply_decoded_background(preview, image, bytes, &mut on_selected)
                }
                Ok((_, Err(_))) | Err(mpsc::TryRecvError::Disconnected) => {
                    host.background_loading.set(false);
                    if let Some(previous) = host.background_previous.borrow_mut().take() {
                        host.mutate(|st| {
                            st.background.fill = previous;
                            st.background.changed();
                        });
                    }
                    // A failed load is not an edit: closing the interaction
                    // drops its snapshot without recording a step.
                    host.with_state(|st| st.finish_history_interaction());
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

    fn apply_decoded_background(
        &self,
        preview: bool,
        image: RgbaImage,
        bytes: Vec<u8>,
        on_selected: &mut Option<impl FnOnce(&RgbaImage)>,
    ) -> glib::ControlFlow {
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
        *self.background_source.borrow_mut() = surface.map(|surface| (Arc::clone(&image), surface));
        if !preview {
            self.background_loading.set(false);
            self.background_previous.borrow_mut().take();
        }
        self.mutate(|st| {
            // The first frame of a load opens the step and the final image
            // closes it, so a preview followed by the sharp image undoes as
            // one edit.
            st.begin_history_interaction();
            st.background
                .set_fill(crate::capture::editor::state::background::Fill::Image(
                    image,
                ));
            st.background.changed();
            if !preview {
                st.finish_history_interaction();
            }
        });
        if preview {
            glib::ControlFlow::Continue
        } else {
            glib::ControlFlow::Break
        }
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
        let leaving_crop = self
            .read(|st| st.selected_tool == Tool::Crop && tool == Tool::Crop)
            .unwrap_or(false);
        if leaving_crop {
            // Re-clicking Crop leaves crop mode and discards the uncommitted
            // rect the same way Escape does.
            self.cancel_crop();
            return;
        }
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
        self.paint_document_inner(context, t, None, MotionPaintPart::Complete, false, false);
    }

    pub(super) fn motion_thumbnail(&self) -> Option<ImageSurface> {
        let borrow = self.editor.try_borrow().ok()?;
        let st = borrow.as_ref()?;
        let layout = st
            .background
            .layout(st.working_image.width(), st.working_image.height());
        if let Some(surface) = self
            .card_cache
            .borrow()
            .as_ref()
            .filter(|(revision, actions, radius, _)| {
                *revision == st.working_image_revision
                    && *actions == st.actions
                    && *radius == layout.radius
            })
            .map(|(_, _, _, surface)| surface.clone())
        {
            return Some(surface);
        }
        let source = self.working_surface(st)?;
        let (width, height) =
            preview_card_size(st.working_image.width(), st.working_image.height());
        let surface = st
            .card_surface_from_source(&source, layout.radius, width, height)
            .ok()?;
        *self.card_cache.borrow_mut() = Some((
            st.working_image_revision,
            st.actions.clone(),
            layout.radius,
            surface.clone(),
        ));
        Some(surface)
    }

    /// Motion animates the card independently of the background. `live`
    /// marks a frame painted while the head is moving (scrub, hover, or
    /// playback): the shadow keeps the cheap cached silhouette then, and the
    /// parked frame is painted again at full quality on release.
    pub(super) fn paint_motion_preview(
        &self,
        context: &gtk4::cairo::Context,
        t: ViewTransform,
        motion: crate::recording::editor::model::MotionTransform,
        anchor: (f64, f64),
        live: bool,
    ) {
        self.paint_document_inner(
            context,
            t,
            Some((motion, anchor)),
            MotionPaintPart::Complete,
            live,
            false,
        );
    }

    pub(super) fn paint_motion_background(
        &self,
        context: &gtk4::cairo::Context,
        t: ViewTransform,
        live: bool,
    ) {
        self.paint_document_inner(context, t, None, MotionPaintPart::Background, live, false);
    }

    /// `export` paints the video encoder's frame: the card comes from the
    /// working image at its own resolution and with the export resampling,
    /// and the shadow uses the parked mask instead of the interactive one.
    pub(super) fn paint_motion_foreground(
        &self,
        context: &gtk4::cairo::Context,
        t: ViewTransform,
        motion: crate::recording::editor::model::MotionTransform,
        anchor: (f64, f64),
        live: bool,
        export: bool,
    ) {
        self.paint_document_inner(
            context,
            t,
            Some((motion, anchor)),
            MotionPaintPart::Foreground,
            live,
            export,
        );
    }

    fn paint_document_inner(
        &self,
        context: &gtk4::cairo::Context,
        t: ViewTransform,
        motion_pose: Option<(crate::recording::editor::model::MotionTransform, (f64, f64))>,
        part: MotionPaintPart,
        live: bool,
        export: bool,
    ) {
        #[cfg(test)]
        if part != MotionPaintPart::Base {
            self.document_paints.set(self.document_paints.get() + 1);
        }
        let motion = motion_pose.is_some();
        let Some(borrow) = self.editor.try_borrow().ok() else {
            return;
        };
        let Some(st) = borrow.as_ref() else {
            return;
        };
        if st.drag_start.is_none() {
            let _ = self.annotation_cache.borrow_mut().take();
            let _ = self.drag_frame_cache.borrow_mut().take();
        }
        let Some(surface) = self.working_surface(st) else {
            return;
        };

        let layout = st
            .background
            .layout(st.working_image.width(), st.working_image.height());
        let pose = if let Some((transform, _)) = motion_pose {
            crate::capture::editor::image_transform::StaticImageTransform {
                yaw: transform.rotation_y,
                pitch: transform.rotation_x,
                roll: transform.rotation_z,
                perspective: transform.perspective,
                pos_x: transform.pos_x / transform.scale.max(0.01),
                pos_y: transform.pos_y / transform.scale.max(0.01),
            }
        } else if st.selected_tool == Tool::Background {
            st.static_image_transform
        } else {
            Default::default()
        };
        let pen_drag = !motion
            && st.drag_start.is_some()
            && matches!(st.selected_tool, Tool::Pen | Tool::Highlighter)
            && !st.auto_expand_canvas;
        // Appearance drags change the base every frame; they keep painting it
        // directly instead of rebuilding the cache per pointer event.
        let appearance_drag =
            st.selected_tool == Tool::Background && st.history_interaction_before.is_some();
        // Each pointer event changes the base, so it is painted with the cheap
        // live filters; the release repaints it at full quality from the cache.
        let live = live || appearance_drag;
        let cached_base = part == MotionPaintPart::Complete
            && !motion
            && !live
            && pose.is_identity()
            && self.paint_cached_base(context, st, t, layout, pen_drag);
        let mut drag_annotations: Option<ImageSurface> = None;
        if !cached_base {
            if part != MotionPaintPart::Foreground {
                if let crate::capture::editor::state::background::Fill::Image(image) =
                    &st.background.fill
                {
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
                            // A moving preview downscales the same wallpaper on
                            // every pointer event or playback frame; Cairo's Good
                            // filter convolves the whole source each time. The
                            // parked frame is painted again with Good on release.
                            context.source().set_filter(if live {
                                gtk4::cairo::Filter::Bilinear
                            } else {
                                gtk4::cairo::Filter::Good
                            });
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
                        self.paint_composited_background(context, st, t, layout, live);
                    }
                } else if st.background.has_fill() {
                    self.paint_composited_background(context, st, t, layout, live);
                }
            }
            if part == MotionPaintPart::Background {
                return;
            }
            if let Some((transform, anchor)) = motion_pose {
                let _ = context.save();
                context.rectangle(
                    t.canvas_offset_x,
                    t.canvas_offset_y,
                    t.canvas_width * t.canvas_scale,
                    t.canvas_height * t.canvas_scale,
                );
                context.clip();
                let width = f64::from(st.working_image.width());
                let height = f64::from(st.working_image.height());
                let anchor_pose = crate::capture::editor::image_transform::StaticImageTransform {
                    pos_x: 0.0,
                    pos_y: 0.0,
                    ..pose
                };
                let project = crate::capture::editor::transform_render::projector(
                    layout,
                    width,
                    height,
                    anchor_pose,
                );
                let (x, y) = project((anchor.0 - 0.5) * width, (anchor.1 - 0.5) * height);
                let cx = t.canvas_offset_x + x * t.canvas_scale;
                let cy = t.canvas_offset_y + y * t.canvas_scale;
                context.translate(cx, cy);
                context.scale(transform.scale, transform.scale);
                context.translate(-cx, -cy);
            }
            if let Some(shadow) = crate::capture::editor::shadow_render::geometry(
                layout,
                f64::from(st.working_image.width()),
                f64::from(st.working_image.height()),
                pose,
                &st.scene_shadow,
            ) {
                let interactive = appearance_drag
                    || (part == MotionPaintPart::Foreground && !export)
                    || (motion && live);
                // During a light/opacity drag, move a cached unoffset silhouette
                // instead of blurring the same card again for every pointer event.
                let mut mask_geometry = shadow;
                if interactive {
                    let margin = shadow.blur * 1.5
                        + f64::from(st.working_image.width().min(st.working_image.height())) * 0.1;
                    let min_x = shadow.corners.iter().map(|p| p.0).fold(0.0, f64::min);
                    let min_y = shadow.corners.iter().map(|p| p.1).fold(0.0, f64::min);
                    let max_x = shadow
                        .corners
                        .iter()
                        .map(|p| p.0)
                        .fold(layout.width, f64::max);
                    let max_y = shadow
                        .corners
                        .iter()
                        .map(|p| p.1)
                        .fold(layout.height, f64::max);
                    mask_geometry.bounds = (
                        min_x - margin,
                        min_y - margin,
                        max_x - min_x + margin * 2.0,
                        max_y - min_y + margin * 2.0,
                    );
                    mask_geometry.offset = (0.0, 0.0);
                    mask_geometry.opacity = 1.0;
                }
                // A live frame keeps a small mask; the parked frame gets the
                // full-resolution one. The edge is part of the key so a live mask
                // is never reused as a sharp one.
                let shadow_edge = if live {
                    96.0
                } else if interactive {
                    192.0
                } else {
                    720.0
                };
                let cached = self.shadow_cache.borrow();
                let mask = cached
                    .as_ref()
                    .filter(|(geometry, cached_interactive, edge, _)| {
                        *geometry == mask_geometry
                            && *cached_interactive == interactive
                            && *edge == shadow_edge
                    })
                    .map(|(_, _, _, surface)| surface.clone());
                drop(cached);
                let mask = mask.or_else(|| {
                    let surface = render::scene_shadow::shadow_mask(mask_geometry, shadow_edge)?;
                    *self.shadow_cache.borrow_mut() =
                        Some((mask_geometry, interactive, shadow_edge, surface.clone()));
                    Some(surface)
                });
                if let Some(mask) = mask {
                    let _ = context.save();
                    context.translate(t.canvas_offset_x, t.canvas_offset_y);
                    context.scale(t.canvas_scale, t.canvas_scale);
                    context.rectangle(0.0, 0.0, layout.width, layout.height);
                    context.clip();
                    if interactive {
                        context.translate(shadow.offset.0, shadow.offset.1);
                    }
                    render::scene_shadow::paint_shadow_mask(
                        context,
                        &mask,
                        mask_geometry.bounds,
                        if interactive { shadow.opacity } else { 1.0 },
                    );
                    let _ = context.restore();
                }
            }
            let mut style = crate::capture::editor::style_render::StyleGeometry::new(
                layout,
                f64::from(st.working_image.width()),
                f64::from(st.working_image.height()),
                pose,
                st.background.frame_style,
            );
            style.over_fill = st.background.has_fill();
            let _ = context.save();
            context.translate(t.canvas_offset_x, t.canvas_offset_y);
            context.scale(t.canvas_scale, t.canvas_scale);
            style.paint_backings(context);
            style.paint_underlay(context);
            let _ = context.restore();
            if !pose.is_identity() {
                // The encoder's card is built from the full working image once
                // and then reused for every frame; the preview keeps its
                // capped-size card so playback stays cheap.
                let cache = if export {
                    &self.export_card_cache
                } else {
                    &self.card_cache
                };
                let cached = cache.borrow();
                let card = cached
                    .as_ref()
                    .filter(|(revision, actions, radius, _)| {
                        *revision == st.working_image_revision
                            && *actions == st.actions
                            && *radius == layout.radius
                    })
                    .map(|(_, _, _, surface)| surface.clone());
                drop(cached);
                let card = card.or_else(|| {
                    let (width, height) = if export {
                        (st.working_image.width(), st.working_image.height())
                    } else {
                        preview_card_size(st.working_image.width(), st.working_image.height())
                    };
                    let surface = st
                        .card_surface_from_source(&surface, layout.radius, width, height)
                        .ok()?;
                    *cache.borrow_mut() = Some((
                        st.working_image_revision,
                        st.actions.clone(),
                        layout.radius,
                        surface.clone(),
                    ));
                    Some(surface)
                });
                if let Some(card) = card {
                    let _ = context.save();
                    context.translate(t.canvas_offset_x, t.canvas_offset_y);
                    context.scale(t.canvas_scale, t.canvas_scale);
                    crate::capture::editor::transform_render::paint_card(
                        context,
                        &card,
                        layout,
                        f64::from(st.working_image.width()),
                        f64::from(st.working_image.height()),
                        pose,
                        if export {
                            crate::capture::editor::transform_render::Quality::Export
                        } else if live {
                            crate::capture::editor::transform_render::Quality::Live
                        } else {
                            crate::capture::editor::transform_render::Quality::Preview
                        },
                    );
                    style.paint_frame(context);
                    let _ = context.restore();
                }
                if motion {
                    let _ = context.restore();
                }
                return;
            }
            let _ = context.save();
            context.translate(t.offset_x, t.offset_y);
            context.scale(t.scale, t.scale);
            drag_annotations = if pen_drag && !t.has_background {
                self.drag_annotation_frame(st)
            } else {
                None
            };
            if let Some(cached) = &drag_annotations {
                // The cached frame already has the rounded image baked in; its
                // annotations are only clipped to the image rectangle, exactly
                // like the direct path below.
                context.rectangle(
                    0.0,
                    0.0,
                    f64::from(st.working_image.width()),
                    f64::from(st.working_image.height()),
                );
                context.clip();
                context.set_source_surface(cached, 0.0, 0.0).ok();
            } else {
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
            }
            // A live frame resamples the screenshot under a moving scale; the
            // quality convolution costs tens of milliseconds there. The parked
            // frame is painted again with the full filter.
            context.source().set_filter(if live {
                render::editor_interactive_image_filter()
            } else {
                render::editor_image_filter_for_scale(t.scale)
            });
            let _ = context.paint();
            let _ = context.restore();

            let _ = context.save();
            context.translate(t.canvas_offset_x, t.canvas_offset_y);
            context.scale(t.canvas_scale, t.canvas_scale);
            style.paint_frame(context);
            let _ = context.restore();
            if part == MotionPaintPart::Base {
                return;
            }
        }

        let _ = context.save();
        context.translate(t.offset_x, t.offset_y);
        context.scale(t.scale, t.scale);
        let _ = context.save();
        clip_annotation_content_to_canvas(context, t);

        let editing = if motion {
            None
        } else {
            st.active_text_input
                .as_ref()
                .and_then(|input| input.editing_action_index)
        };
        if drag_annotations.is_none() && !(cached_base && pen_drag) {
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
        }
        if !motion {
            if let Some(draft) = st.draft_action() {
                draw_draft_action(context, &draft);
            }
        }
        let _ = context.restore();

        if !motion {
            self.paint_chrome(context, st, t);
        }
        let _ = context.restore();
        if motion {
            let _ = context.restore();
        }
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

    /// Paint the base layer from a raster kept at the on-screen size, built
    /// with the same quality filters as a direct paint. Returns false when no
    /// raster can be made, so the caller paints the base directly.
    /// With `with_annotations`, the raster also carries every committed
    /// annotation, for the length of a pen or highlighter drag.
    fn paint_cached_base(
        &self,
        context: &gtk4::cairo::Context,
        st: &EditorState,
        t: ViewTransform,
        layout: crate::capture::editor::state::background::Layout,
        with_annotations: bool,
    ) -> bool {
        let (device_x, device_y) = context.target().device_scale();
        let device_scale = device_x.max(device_y).max(1.0);
        let width = (layout.width * t.canvas_scale * device_scale).ceil();
        let height = (layout.height * t.canvas_scale * device_scale).ceil();
        if !(width >= 1.0 && height >= 1.0) || width * height > 40_000_000.0 {
            return false;
        }
        let fill = match &st.background.fill {
            crate::capture::editor::state::background::Fill::None => 0,
            crate::capture::editor::state::background::Fill::Image(image) => {
                Arc::as_ptr(image) as usize
            }
            crate::capture::editor::state::background::Fill::Custom(painter) => {
                Rc::as_ptr(painter) as *const () as usize
            }
        };
        let key = BaseKey {
            working_revision: st.working_image_revision,
            image_size: st.working_image.dimensions(),
            background_revision: st.background.revision,
            fill,
            padding: st.background.padding,
            blur: st.background.blur,
            radius: st.background.radius,
            noise: st.background.noise,
            frame_aspect: st.background.frame_aspect,
            frame_style: st.background.frame_style,
            shadow: st.scene_shadow.clone(),
            layout,
            scale: t.scale,
            canvas_scale: t.canvas_scale,
            device_scale,
        };
        let cached = self
            .base_cache
            .borrow()
            .as_ref()
            .filter(|(cached_key, _)| *cached_key == key)
            .map(|(_, surface)| surface.clone());
        let surface = match cached {
            Some(surface) => surface,
            None => {
                let Ok(surface) =
                    ImageSurface::create(gtk4::cairo::Format::ARgb32, width as i32, height as i32)
                else {
                    return false;
                };
                {
                    let Ok(base) = gtk4::cairo::Context::new(&surface) else {
                        return false;
                    };
                    base.scale(device_scale, device_scale);
                    let local = ViewTransform {
                        offset_x: t.offset_x - t.canvas_offset_x,
                        offset_y: t.offset_y - t.canvas_offset_y,
                        canvas_offset_x: 0.0,
                        canvas_offset_y: 0.0,
                        ..t
                    };
                    self.paint_document_inner(
                        &base,
                        local,
                        None,
                        MotionPaintPart::Base,
                        false,
                        false,
                    );
                }
                surface.flush();
                #[cfg(test)]
                self.base_builds.set(self.base_builds.get() + 1);
                *self.base_cache.borrow_mut() = Some((key.clone(), surface.clone()));
                surface
            }
        };
        let surface = if with_annotations {
            let editing = st
                .active_text_input
                .as_ref()
                .and_then(|input| input.editing_action_index);
            let cached = self
                .drag_frame_cache
                .borrow()
                .as_ref()
                .filter(|(cached_key, actions, cached_editing, _)| {
                    *cached_key == key && *actions == st.actions.len() && *cached_editing == editing
                })
                .map(|(_, _, _, frame)| frame.clone());
            match cached {
                Some(frame) => frame,
                None => {
                    let Ok(frame) = ImageSurface::create(
                        gtk4::cairo::Format::ARgb32,
                        width as i32,
                        height as i32,
                    ) else {
                        return false;
                    };
                    {
                        let Ok(drawing) = gtk4::cairo::Context::new(&frame) else {
                            return false;
                        };
                        let _ = drawing.set_source_surface(&surface, 0.0, 0.0);
                        drawing.source().set_filter(gtk4::cairo::Filter::Fast);
                        let _ = drawing.paint();
                        drawing.scale(device_scale, device_scale);
                        let local = ViewTransform {
                            offset_x: t.offset_x - t.canvas_offset_x,
                            offset_y: t.offset_y - t.canvas_offset_y,
                            canvas_offset_x: 0.0,
                            canvas_offset_y: 0.0,
                            ..t
                        };
                        drawing.translate(local.offset_x, local.offset_y);
                        drawing.scale(local.scale, local.scale);
                        clip_annotation_content_to_canvas(&drawing, local);
                        for (index, action) in st.actions.iter().enumerate() {
                            if Some(index) == editing
                                || matches!(
                                    action,
                                    AnnotationAction::Obfuscate { .. }
                                        | AnnotationAction::Focus { .. }
                                )
                            {
                                continue;
                            }
                            draw_annotation_action(&drawing, action);
                        }
                    }
                    frame.flush();
                    #[cfg(test)]
                    self.drag_frame_builds.set(self.drag_frame_builds.get() + 1);
                    *self.drag_frame_cache.borrow_mut() =
                        Some((key, st.actions.len(), editing, frame.clone()));
                    frame
                }
            }
        } else {
            surface
        };
        let _ = context.save();
        context.translate(
            (t.canvas_offset_x * device_scale).round() / device_scale,
            (t.canvas_offset_y * device_scale).round() / device_scale,
        );
        context.scale(1.0 / device_scale, 1.0 / device_scale);
        let _ = context.set_source_surface(&surface, 0.0, 0.0);
        context.source().set_filter(gtk4::cairo::Filter::Fast);
        let _ = context.paint();
        let _ = context.restore();
        true
    }

    fn paint_composited_background(
        &self,
        context: &gtk4::cairo::Context,
        st: &EditorState,
        t: ViewTransform,
        layout: crate::capture::editor::state::background::Layout,
        live: bool,
    ) {
        // Raster at the canvas's on-screen size, rounded up to 256 px steps so
        // small zoom changes reuse the cache; never above 4096.
        let edge = ((layout.width.max(layout.height) * t.canvas_scale / 256.0).ceil() * 256.0)
            .clamp(256.0, 4096.0) as u32;
        let key = (
            st.background.revision,
            st.working_image.width(),
            st.working_image.height(),
            edge,
        );
        let cached = self.background_cache.borrow();
        let background = cached
            .as_ref()
            .filter(|(revision, width, height, size, _)| (*revision, *width, *height, *size) == key)
            .map(|(_, _, _, _, surface)| surface.clone());
        drop(cached);
        let background = background.or_else(|| {
            let surface = st.background.surface(layout, f64::from(edge))?;
            *self.background_cache.borrow_mut() =
                Some((key.0, key.1, key.2, key.3, surface.clone()));
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
            // Same rule as the wallpaper branch: a moving frame trades the
            // quality resample for a cheap one and is repainted on release.
            context.source().set_filter(if live {
                render::editor_interactive_image_filter()
            } else {
                render::editor_image_filter_for_scale(t.canvas_scale)
            });
            let _ = context.paint();
            let _ = context.restore();
        }
    }

    /// The working image with every committed annotation already composited,
    /// for the length of a pen/highlighter drag. Rebuilding it per frame is
    /// what makes long strokes lag once a few are on the canvas.
    fn drag_annotation_frame(&self, st: &EditorState) -> Option<ImageSurface> {
        let editing = st
            .active_text_input
            .as_ref()
            .and_then(|input| input.editing_action_index);
        let cached = self.annotation_cache.borrow();
        if let Some((revision, actions, cached_editing, surface)) = cached.as_ref() {
            if *revision == st.working_image_revision
                && *actions == st.actions.len()
                && *cached_editing == editing
            {
                return Some(surface.clone());
            }
        }
        drop(cached);

        let source = self.working_surface(st)?;
        let surface = ImageSurface::create(
            gtk4::cairo::Format::ARgb32,
            st.working_image.width() as i32,
            st.working_image.height() as i32,
        )
        .ok()?;
        let context = gtk4::cairo::Context::new(&surface).ok()?;
        let layout = st
            .background
            .layout(st.working_image.width(), st.working_image.height());
        let _ = context.save();
        render::rounded_rect_path(
            &context,
            0.0,
            0.0,
            f64::from(st.working_image.width()),
            f64::from(st.working_image.height()),
            layout.radius,
        );
        context.clip();
        context.set_source_surface(&source, 0.0, 0.0).ok()?;
        context.paint().ok()?;
        let _ = context.restore();
        for (index, action) in st.actions.iter().enumerate() {
            if Some(index) == editing
                || matches!(
                    action,
                    AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. }
                )
            {
                continue;
            }
            draw_annotation_action(&context, action);
        }
        surface.flush();
        *self.annotation_cache.borrow_mut() = Some((
            st.working_image_revision,
            st.actions.len(),
            editing,
            surface.clone(),
        ));
        Some(surface)
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
                        max_width.unwrap_or_else(|| default_text_wrap_width(st, *position, font));
                    let mut bounds =
                        text_action_bounds(context, *position, text, font, Some(width));
                    clamp_text_bounds_to_canvas(st, &mut bounds);
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
                        max_width.unwrap_or_else(|| default_text_wrap_width(st, *position, font));
                    let mut bounds =
                        text_action_bounds(context, *position, text, font, Some(width));
                    clamp_text_bounds_to_canvas(st, &mut bounds);
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
            clamp_text_bounds_to_canvas(st, &mut bounds);
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
        hover: Rc<super::canvas::Hover>,
        pan_to: Rc<dyn Fn(f64, f64)>,
    ) {
        self.install_text_input_method(area, transform_of.clone());
        // ---- pointer motion: text hover + handle cursors ----
        let motion = gtk4::EventControllerMotion::new();
        motion.connect_motion({
            let host = Rc::downgrade(self);
            let transform_of = transform_of.clone();
            let area = area.downgrade();
            let cursor = cursor.clone();
            let hover = hover.clone();
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
                let brush = host.with_state(|st| {
                    // The pen and the highlighter draw their own brush preview;
                    // every other tool keeps a theme cursor.
                    if let Some(brush) = super::cursor::brush_cursor(st, t, Point { x, y }) {
                        return Some(brush);
                    }
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
                    None
                });
                hover.point.set(Some(Point { x, y }));
                match brush.flatten() {
                    Some(super::cursor::CanvasCursor::Name(name)) => cursor(Some(name)),
                    Some(super::cursor::CanvasCursor::Custom(brush)) => {
                        area.set_cursor(Some(&brush));
                    }
                    None => cursor(cursor_name),
                }
            }
        });
        motion.connect_leave({
            let hover = hover.clone();
            move |_| hover.point.set(None)
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
                let refocus = host.with_state(|st| {
                    // A click that never became a drag arms an arrow handle on
                    // press but emits no drag end, so clear the stale handle
                    // drag here.
                    if st.drag_start_view.is_none() {
                        st.arrow_control_dragging = None;
                    }
                    finish_text_drag(st)
                });
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
                    let live_bars = host
                        .read(|st| {
                            st.selected_tool == Tool::Crop
                                || matches!(
                                    st.selected_action(),
                                    Some(
                                        AnnotationAction::Obfuscate { .. }
                                            | AnnotationAction::Focus { .. }
                                    )
                                )
                        })
                        .unwrap_or(false);
                    if live_bars {
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
                #[allow(clippy::collapsible_match)]
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
        *self.annotation_cache.borrow_mut() = None;
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
            ) && !(st.selected_action_index.is_some() && st.select_drag_anchor.is_some())
            {
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
                        if let Some(handle) =
                            action_resize_handle_at_point_with_radius(action, image_point, radius)
                        {
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
                                && action_contains_point_with_padding(action, image_point, padding)
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
    /// text action. Move handles move or stretch the box and the corner
    /// resizes it; the text follows live, inside the composed canvas.
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
        let resizing = st.active_text_is_resizing;
        let min_w = if resizing {
            50.0
        } else {
            st.committed_text_min_width().max(50.0)
        };
        let min_h = 44.0;
        let (min_x, _, max_x, max_y) = st.annotation_canvas_bounds();
        let (min_x, max_x, max_y) = (
            min_x.round() as i32,
            max_x.round() as i32,
            max_y.round() as i32,
        );
        let drag_handle = st.active_text_drag_handle.clone();
        let Some(bounds) = st.active_text_bounds.as_mut() else {
            return;
        };
        if resizing {
            bounds.rect.x = start_bounds.x;
            bounds.rect.y = start_bounds.y;
            bounds.rect.width = (f64::from(start_bounds.width) + dx)
                .clamp(min_w, f64::from(max_x - start_bounds.x).max(min_w))
                .round() as i32;
            bounds.rect.height = (f64::from(start_bounds.height) + dy)
                .clamp(min_h, f64::from(max_y - start_bounds.y).max(min_h))
                .round() as i32;
        } else {
            match drag_handle {
                Some(MoveHandle::Left) => {
                    let right = start_bounds.x + start_bounds.width;
                    let x = (start_bounds.x + dx.round() as i32)
                        .clamp(min_x, (right - min_w as i32).max(min_x));
                    bounds.rect.x = x;
                    bounds.rect.y = start_bounds.y;
                    bounds.rect.width = (right - x).max(min_w as i32);
                }
                Some(MoveHandle::Right) => {
                    bounds.rect.x = start_bounds.x;
                    bounds.rect.y = start_bounds.y;
                    bounds.rect.width = (f64::from(start_bounds.width) + dx)
                        .clamp(min_w, f64::from(max_x - start_bounds.x).max(min_w))
                        .round() as i32;
                }
                None => {
                    bounds.rect.x = (f64::from(start_bounds.x) + dx).round() as i32;
                    bounds.rect.y = (f64::from(start_bounds.y) + dy).round() as i32;
                }
            }
        }
        let mut clamped = bounds.clone();
        clamp_text_bounds_to_canvas(st, &mut clamped);
        st.active_text_bounds = Some(clamped);
        if st.active_text_input.is_some() {
            if resizing {
                st.fit_active_text_to_layout_preserving_box();
            } else {
                st.fit_active_text_height_only();
            }
        } else {
            if !resizing {
                st.fit_committed_text_height_only();
            }
            apply_text_bounds_to_selected_action(st, resizing);
        }
    }

    fn drag_end(&self, t: &ViewTransform, gesture: &gtk4::GestureDrag, ox: f64, oy: f64) {
        *self.annotation_cache.borrow_mut() = None;
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
            // Moving an existing shape is handled above; reaching here means a
            // new box or circle, which has to be a deliberate drag.
            if matches!(st.selected_tool, Tool::Box | Tool::Circle)
                && ox.abs().max(oy.abs()) < MIN_SHAPE_DRAG_DISTANCE
            {
                st.clear_drag();
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
            st.active_text_input.as_ref()?;
            let mut handled = true;
            let mut commit = false;
            let mut cancel = false;
            let mut paste = false;
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
                gdk::Key::v | gdk::Key::V if ctrl => paste = true,
                gdk::Key::Insert if shift => paste = true,
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
            Some((handled || cancel || commit, paste))
        });
        if let Some(Some((consumed, paste))) = text_result {
            self.changed();
            if paste {
                self.paste_text_from_clipboard();
            }
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
        let snapshot = self.with_state(|st| st.to_final_image().ok());
        let Some(snapshot) = snapshot.flatten() else {
            report_output_failure(
                &*self.on_error,
                &crate::i18n::t("Copy failed"),
                "Could not render the image for the clipboard.",
            );
            return;
        };
        self.write_snapshot_to_clipboard(snapshot);
    }

    /// Clipboard an already rendered frame, the current Motion pose, through
    /// the same BMP and PNG provider the flattened document uses.
    pub fn copy_frame_to_clipboard(&self, frame: image::RgbaImage) {
        if self.background_loading() {
            self.report_error(crate::i18n::t(
                "Wait for the background image to finish loading.",
            ));
            return;
        }
        self.write_snapshot_to_clipboard(frame);
    }

    fn write_snapshot_to_clipboard(&self, snapshot: image::RgbaImage) {
        let report = self.on_error.clone();
        let (sender, receiver) = mpsc::channel::<Option<(Vec<u8>, Vec<u8>)>>();
        std::thread::spawn(move || {
            let bmp = crate::capture::editor::io_ops::bmp_bytes(&snapshot).ok();
            let png = crate::capture::editor::io_ops::png_bytes(&snapshot).ok();
            let _ = sender.send(bmp.zip(png));
        });
        glib::timeout_add_local(Duration::from_millis(16), move || {
            let encoded = match receiver.try_recv() {
                Ok(encoded) => encoded,
                Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    report_output_failure(
                        &*report,
                        &crate::i18n::t("Copy failed"),
                        "Could not copy the image to the clipboard.",
                    );
                    return glib::ControlFlow::Break;
                }
            };
            let copied = encoded.is_some_and(|(bmp, png)| {
                let bmp = glib::Bytes::from_owned(bmp);
                let png = glib::Bytes::from_owned(png);
                let provider = gdk::ContentProvider::new_union(&[
                    gdk::ContentProvider::for_bytes("image/bmp", &bmp),
                    gdk::ContentProvider::for_bytes("image/png", &png),
                ]);
                gdk::Display::default()
                    .is_some_and(|display| display.clipboard().set_content(Some(&provider)).is_ok())
            });
            if copied {
                report(crate::i18n::t("Image copied to clipboard"));
            } else {
                report_output_failure(
                    &*report,
                    &crate::i18n::t("Copy failed"),
                    "Could not copy the image to the clipboard.",
                );
            }
            glib::ControlFlow::Break
        });
    }

    /// The flattened document for a Cloud upload. Commits inline text first
    /// and refuses while the background image is still loading, exactly like
    /// Copy and Save As. A failed flatten carries its render error text so
    /// the caller can report it instead of silently doing nothing.
    pub fn upload_snapshot(&self) -> Result<image::RgbaImage, String> {
        if self.background_loading() {
            return Err(crate::i18n::t(
                "Wait for the background image to finish loading.",
            ));
        }
        self.commit_text_if_active();
        match self.with_state(|st| st.to_final_image().map_err(|error| error.to_string())) {
            Some(result) => result,
            None => Err(crate::i18n::t("Could not render the image.")),
        }
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
        let snapshot = self.with_state(|st| st.to_final_image().ok());
        let Some(snapshot) = snapshot.flatten() else {
            report_output_failure(
                &*self.on_error,
                &crate::i18n::t("Export failed"),
                "Could not render the image.",
            );
            return;
        };
        self.save_snapshot(path, snapshot);
        let _ = parent;
    }

    fn save_snapshot(&self, path: &std::path::Path, snapshot: image::RgbaImage) {
        let path = path.to_path_buf();
        let report = self.on_error.clone();
        let reported_path = path.clone();
        let (sender, receiver) =
            mpsc::channel::<Result<(), crate::capture::editor::types::EditorError>>();
        std::thread::spawn(move || {
            let _ = sender.send(crate::capture::editor::io_ops::save_image_snapshot(
                &path, snapshot,
            ));
        });
        glib::timeout_add_local(Duration::from_millis(50), move || {
            match receiver.try_recv() {
                Ok(Ok(())) => {
                    crate::utils::notify::desktop_notification(
                        &crate::i18n::t("Export complete"),
                        &reported_path.display().to_string(),
                    );
                    glib::ControlFlow::Break
                }
                Ok(Err(error)) => {
                    report_output_failure(
                        &*report,
                        &crate::i18n::t("Export failed"),
                        &format!("Could not save the image: {error}"),
                    );
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => glib::ControlFlow::Break,
            }
        });
    }
}

/// Report a failed copy or export on both surfaces: the canvas toast for the
/// window the user is looking at, and a system notification for the copy that
/// window no longer covers by the time the worker finishes.
fn report_output_failure(report: &dyn Fn(String), summary: &str, message: &str) {
    crate::utils::notify::desktop_notification_important(summary, message);
    report(message.to_string());
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
    let committed = st.active_text_bounds.is_some() && st.selected_action_index.is_some();
    if committed && apply_text_bounds_to_selected_action(st, was_resizing) {
        st.clear_redo_history();
    }
    st.active_text_bounds = None;
    if committed {
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

/// Insert text at the live caret, turning CRLF and lone CR line breaks into
/// newlines. Returns false when no text box is live.
fn insert_into_active_text(st: &mut EditorState, text: &str) -> bool {
    if st.active_text_input.is_none() {
        return false;
    }
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    st.insert_text_input_str(&text);
    st.fit_active_text_to_layout();
    st.reset_text_cursor_blink();
    true
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

/// Keep a text box inside the composed canvas, background padding included.
fn clamp_text_bounds_to_canvas(
    st: &EditorState,
    bounds: &mut crate::capture::editor::types::TextEditBounds,
) {
    let (min_x, min_y, max_x, max_y) = st.annotation_canvas_bounds();
    let (min_x, min_y) = (min_x.round() as i32, min_y.round() as i32);
    let (max_x, max_y) = (max_x.round() as i32, max_y.round() as i32);
    bounds.rect.x = bounds
        .rect
        .x
        .clamp(min_x, (max_x - bounds.rect.width).max(min_x));
    bounds.rect.y = bounds
        .rect
        .y
        .clamp(min_y, (max_y - bounds.rect.height).max(min_y));
    bounds.sync_handles();
}

/// Wrap width for a text action saved without one: up to the canvas edge.
fn default_text_wrap_width(st: &EditorState, position: Point, font: &FontSettings) -> f64 {
    let (_, _, max_x, _) = st.annotation_canvas_bounds();
    (max_x - position.x).max(font.size * 1.8)
}

/// Largest offered text size whose wrapped text fits inside `rect`.
fn fitted_text_size(
    text: &str,
    font: &FontSettings,
    rect: crate::capture::editor::types::Rect,
) -> f64 {
    let Some((_surface, context)) = text_context() else {
        return font.size;
    };
    let sizes = crate::capture::editor::color::OFFERED_TEXT_SIZES;
    let mut best = sizes[0];
    for size in sizes {
        let mut candidate = font.clone();
        candidate.size = size;
        let measured = text_action_bounds(
            &context,
            Point { x: 0.0, y: size },
            text,
            &candidate,
            Some(f64::from(rect.width)),
        );
        if measured.rect.width <= rect.width && measured.rect.height <= rect.height {
            best = size;
        }
    }
    best
}

/// Write the dragged box back to the selected committed text so it reflows
/// while the pointer moves; the resize corner also refits its font size.
/// Returns whether the action changed.
fn apply_text_bounds_to_selected_action(st: &mut EditorState, fit_font: bool) -> bool {
    let (Some(rect), Some(index)) = (
        st.active_text_bounds.as_ref().map(|bounds| bounds.rect),
        st.selected_action_index,
    ) else {
        return false;
    };
    let Some(AnnotationAction::Text {
        position,
        text,
        font,
        max_width,
        ..
    }) = st.actions.get_mut(index)
    else {
        return false;
    };
    let next_size = if fit_font {
        fitted_text_size(text, font, rect)
    } else {
        font.size
    };
    let padding_y = 8.0;
    let next_x = f64::from(rect.x);
    let next_y = f64::from(rect.y) + next_size + padding_y;
    let next_width = Some(f64::from(rect.width));
    let changed = position.x != next_x
        || position.y != next_y
        || *max_width != next_width
        || font.size != next_size;
    font.size = next_size;
    position.x = next_x;
    position.y = next_y;
    *max_width = next_width;
    if fit_font {
        st.text_size = next_size;
    }
    changed
}

/// Set the text size and refit the live text box at once, scaling its width
/// with the font so it grows and shrinks with the text instead of waiting
/// for the next keystroke.
pub(crate) fn set_text_size_and_fit(st: &mut EditorState, size: f64) {
    let previous = st.text_size;
    if !st.set_text_size(size) || st.active_text_input.is_none() || previous <= 0.0 {
        return;
    }
    let ratio = st.text_size / previous;
    if let Some(bounds) = st.active_text_bounds.as_mut() {
        bounds.rect.width = (f64::from(bounds.rect.width) * ratio).round().max(50.0) as i32;
        bounds.sync_handles();
    }
    st.fit_active_text_to_layout_preserving_font_size();
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
    let width = max_width.unwrap_or_else(|| default_text_wrap_width(st, *position, font));
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

    #[test]
    fn re_arming_crop_leaves_crop_mode_and_discards_the_rect() {
        let h = host();
        let mut state = EditorState::new(RgbaImage::new(200, 100));
        state.set_tool_without_rebuild(Tool::Crop);
        state.crop_rect = Some(crate::capture::editor::types::Rect {
            x: 10,
            y: 10,
            width: 50,
            height: 40,
        });
        *h.editor.borrow_mut() = Some(state);

        h.activate_tool(Tool::Crop);

        assert_eq!(h.read(|st| st.selected_tool), Some(Tool::Select));
        assert_eq!(h.read(|st| st.crop_rect), Some(None));
    }

    #[test]
    fn drag_annotation_cache_matches_direct_paint() {
        let host = host();
        let mut state = EditorState::new(RgbaImage::from_pixel(
            160,
            100,
            image::Rgba([240, 240, 240, 255]),
        ));
        state.selected_tool = Tool::Highlighter;
        state.actions = vec![AnnotationAction::Highlighter {
            points: vec![
                Point { x: 20.0, y: 30.0 },
                Point { x: 60.0, y: 40.0 },
                Point { x: 100.0, y: 25.0 },
                Point { x: 140.0, y: 60.0 },
            ],
            color: crate::capture::editor::types::DrawColor::new(1.0, 0.85, 0.1, 1.0),
            stroke_size: 14.0,
        }];
        state.drag_start = Some(Point { x: 10.0, y: 10.0 });
        *host.editor.borrow_mut() = Some(state);

        let transform = ViewTransform::for_image(160.0, 100.0);
        let render = |dragging: bool| {
            *host.annotation_cache.borrow_mut() = None;
            host.mutate(|state| {
                state.drag_start = dragging.then_some(Point { x: 10.0, y: 10.0 });
            });
            let mut surface = ImageSurface::create(gtk4::cairo::Format::ARgb32, 160, 100).unwrap();
            let context = gtk4::cairo::Context::new(&surface).unwrap();
            host.paint_document(&context, transform);
            drop(context);
            surface.flush();
            let data = surface.data().unwrap().to_vec();
            drop(surface);
            data
        };

        assert_eq!(render(true), render(false));
    }

    #[test]
    fn cached_base_matches_a_direct_paint_and_survives_typing() {
        let host = host();
        let mut state = EditorState::new(RgbaImage::from_fn(240, 160, |x, y| {
            image::Rgba([(x % 200) as u8, (y % 150) as u8, 120, 255])
        }));
        state.background.fill = crate::capture::editor::state::background::Fill::Image(Arc::new(
            RgbaImage::from_pixel(8, 8, image::Rgba([20, 90, 200, 255])),
        ));
        state.background.set_padding(40.0);
        state.background.noise = 0.2;
        state.background.radius = 12.0;
        state.background.changed();
        state.selected_tool = Tool::Text;
        *host.editor.borrow_mut() = Some(state);
        let layout = host
            .read(|st| st.background.layout(240, 160))
            .expect("document");
        let t = ViewTransform {
            scale: 0.5,
            offset_x: 10.0 + layout.image_x * 0.5,
            offset_y: 6.0 + layout.image_y * 0.5,
            has_background: true,
            canvas_offset_x: 10.0,
            canvas_offset_y: 6.0,
            canvas_scale: 0.5,
            canvas_width: layout.width,
            canvas_height: layout.height,
            image_rect_x: layout.image_x,
            image_rect_y: layout.image_y,
            ..ViewTransform::for_image(240.0, 160.0)
        };
        let render = |part: MotionPaintPart| {
            let mut surface = ImageSurface::create(gtk4::cairo::Format::ARgb32, 260, 200).unwrap();
            let context = gtk4::cairo::Context::new(&surface).unwrap();
            host.paint_document_inner(&context, t, None, part, false, false);
            drop(context);
            surface.flush();
            let data = surface.data().unwrap().to_vec();
            drop(surface);
            data
        };
        let cached = render(MotionPaintPart::Complete);
        let direct = render(MotionPaintPart::Base);
        assert_eq!(host.base_builds.get(), 1);
        let worst = cached
            .iter()
            .zip(&direct)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(
            worst <= 2,
            "cached base differs from a direct paint by {worst}"
        );

        host.with_state(|st| {
            st.begin_text_input(
                crate::capture::editor::types::Point { x: 20.0, y: 40.0 },
                220.0,
                44.0,
            );
            st.insert_text_input_str("typing never rebuilds the base");
            st.fit_active_text_to_layout();
        });
        render(MotionPaintPart::Complete);
        assert_eq!(host.base_builds.get(), 1, "typing reused the cached base");

        host.with_state(|st| {
            st.background.set_padding(60.0);
            st.background.changed();
        });
        render(MotionPaintPart::Complete);
        assert_eq!(
            host.base_builds.get(),
            2,
            "an appearance change rebuilds it"
        );
    }

    #[test]
    fn pen_drag_frame_matches_a_direct_paint_and_is_built_once() {
        let host = host();
        let mut state = EditorState::new(RgbaImage::from_fn(240, 160, |x, y| {
            image::Rgba([(x % 200) as u8, (y % 150) as u8, 120, 255])
        }));
        state.selected_tool = Tool::Highlighter;
        state.actions = vec![AnnotationAction::Highlighter {
            points: vec![
                Point { x: 20.0, y: 30.0 },
                Point { x: 120.0, y: 60.0 },
                Point { x: 200.0, y: 40.0 },
            ],
            color: crate::capture::editor::types::DrawColor::new(1.0, 0.85, 0.1, 1.0),
            stroke_size: 14.0,
        }];
        *host.editor.borrow_mut() = Some(state);
        let t = ViewTransform {
            scale: 0.5,
            offset_x: 10.0,
            offset_y: 6.0,
            canvas_offset_x: 10.0,
            canvas_offset_y: 6.0,
            canvas_scale: 0.5,
            ..ViewTransform::for_image(240.0, 160.0)
        };
        let render = |dragging: bool| {
            host.with_state(|st| {
                st.drag_start = dragging.then_some(Point { x: 10.0, y: 10.0 });
            });
            let mut surface = ImageSurface::create(gtk4::cairo::Format::ARgb32, 160, 120).unwrap();
            let context = gtk4::cairo::Context::new(&surface).unwrap();
            host.paint_document_inner(
                &context,
                t,
                None,
                MotionPaintPart::Complete,
                false,
                false,
            );
            drop(context);
            surface.flush();
            let data = surface.data().unwrap().to_vec();
            drop(surface);
            data
        };
        let direct = render(false);
        let dragged = render(true);
        render(true);
        assert_eq!(
            host.drag_frame_builds.get(),
            1,
            "the frame is reused across a drag"
        );
        let worst = direct
            .iter()
            .zip(&dragged)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(
            worst <= 2,
            "pen drag frame differs from a direct paint by {worst}"
        );
        render(false);
        render(true);
        assert_eq!(
            host.drag_frame_builds.get(),
            2,
            "a new drag rebuilds the frame"
        );
    }

    #[test]
    #[ignore = "manual highlighter paint measurement"]
    fn measure_highlighter_paint() {
        let host = host();
        let mut state = EditorState::new(RgbaImage::from_pixel(
            1920,
            1080,
            image::Rgba([230, 230, 230, 255]),
        ));
        state.selected_tool = Tool::Highlighter;
        state.drag_start = Some(Point { x: 10.0, y: 10.0 });
        *host.editor.borrow_mut() = Some(state);
        let surface = ImageSurface::create(gtk4::cairo::Format::ARgb32, 1920, 1080).unwrap();
        let context = gtk4::cairo::Context::new(&surface).unwrap();
        let transform = ViewTransform::for_image(1920.0, 1080.0);
        for count in [0usize, 1, 3, 6, 10] {
            host.mutate(|state| {
                state.actions = (0..count)
                    .map(|index| {
                        let points = (0..500)
                            .map(|i| Point {
                                x: 40.0 + i as f64 * 1.8,
                                y: 120.0 + index as f64 * 90.0 + (i as f64 * 0.05).sin() * 50.0,
                            })
                            .collect();
                        AnnotationAction::Highlighter {
                            points,
                            color: crate::capture::editor::types::DrawColor::new(
                                1.0, 0.9, 0.1, 1.0,
                            ),
                            stroke_size: 40.0,
                        }
                    })
                    .collect();
            });
            host.paint_document(&context, transform);
            let start = std::time::Instant::now();
            for _ in 0..10 {
                host.paint_document(&context, transform);
            }
            let cached = start.elapsed() / 10;
            host.mutate(|state| state.drag_start = None);
            host.paint_document(&context, transform);
            let start = std::time::Instant::now();
            for _ in 0..10 {
                host.paint_document(&context, transform);
            }
            let direct = start.elapsed() / 10;
            host.mutate(|state| state.drag_start = Some(Point { x: 10.0, y: 10.0 }));
            eprintln!(
                "{count} committed strokes: cached {cached:?}/frame, direct {direct:?}/frame"
            );
        }
    }

    #[test]
    #[ignore = "manual transform preview performance measurement"]
    fn measure_large_transform_preview() {
        let host = host();
        let mut state = EditorState::new(RgbaImage::from_pixel(
            3840,
            2160,
            image::Rgba([120, 80, 40, 255]),
        ));
        state.selected_tool = Tool::Background;
        state.static_image_transform.yaw = 18.0;
        *host.editor.borrow_mut() = Some(state);
        let surface = ImageSurface::create(gtk4::cairo::Format::ARgb32, 960, 540).unwrap();
        let context = gtk4::cairo::Context::new(&surface).unwrap();
        let transform = ViewTransform {
            scale: 0.25,
            canvas_scale: 0.25,
            canvas_width: 3840.0,
            canvas_height: 2160.0,
            ..ViewTransform::for_image(3840.0, 2160.0)
        };
        let start = std::time::Instant::now();
        host.paint_document(&context, transform);
        let first = start.elapsed();
        let start = std::time::Instant::now();
        for frame in 0..20 {
            host.mutate(|state| state.static_image_transform.yaw = 10.0 + f64::from(frame) / 2.0);
            host.paint_document(&context, transform);
        }
        eprintln!(
            "transform preview: first={first:?}, 20 frames={:?}",
            start.elapsed()
        );
        host.mutate(|state| {
            state.scene_shadow.preset =
                crate::recording::editor::model::MotionSceneShadowPreset::Spread;
            state.begin_history_interaction();
        });
        let start = std::time::Instant::now();
        for frame in 0..20 {
            host.mutate(|state| state.scene_shadow.light.0 = f64::from(frame) / 10.0 - 1.0);
            host.paint_document(&context, transform);
        }
        eprintln!("shadow preview: 20 frames={:?}", start.elapsed());
        host.mutate(|state| {
            state.finish_history_interaction();
            state.static_image_transform = Default::default();
            state.background.fill =
                crate::capture::editor::state::background::Fill::Image(Arc::new(
                    RgbaImage::from_pixel(1, 1, image::Rgba([90, 130, 190, 255])),
                ));
            state.background.padding = 40.0;
            state.set_frame_style(crate::capture::editor::types::FrameStyle::Liquid);
        });
        let layout = host
            .read(|state| state.background.layout(3840, 2160))
            .unwrap();
        let zoom = 960.0 / layout.width;
        let transform = ViewTransform {
            scale: zoom,
            canvas_scale: zoom,
            offset_x: layout.image_x * zoom,
            offset_y: layout.image_y * zoom,
            canvas_width: layout.width,
            canvas_height: layout.height,
            image_rect_x: layout.image_x,
            image_rect_y: layout.image_y,
            has_background: true,
            ..ViewTransform::for_image(3840.0, 2160.0)
        };
        let start = std::time::Instant::now();
        host.paint_document(&context, transform);
        let first = start.elapsed();
        let start = std::time::Instant::now();
        for _ in 0..20 {
            host.paint_document(&context, transform);
        }
        eprintln!(
            "glass preview: first={first:?}, 20 cached frames={:?}",
            start.elapsed()
        );
        host.mutate(|state| {
            state.background.frame_style = crate::capture::editor::types::FrameStyle::Default;
            state.scene_shadow.preset =
                crate::recording::editor::model::MotionSceneShadowPreset::None;
        });
        let start = std::time::Instant::now();
        for _ in 0..20 {
            host.paint_document(&context, transform);
        }
        eprintln!("flat preview: 20 frames={:?}", start.elapsed());
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
    fn motion_preview_shares_appearance_but_not_static_pose_or_selection() {
        use crate::capture::editor::state::background::Fill;
        use crate::capture::editor::types::DrawColor;
        let host = host();
        let mut state = EditorState::new(RgbaImage::from_pixel(
            100,
            80,
            image::Rgba([240, 0, 0, 255]),
        ));
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([0, 0, 255, 255]),
        )));
        state.background.padding = 40.0;
        state.actions.push(AnnotationAction::Line {
            start: Point { x: 20.0, y: 40.0 },
            end: Point { x: 80.0, y: 40.0 },
            color: DrawColor::new(0.0, 1.0, 0.0, 1.0),
            stroke_size: 4.0,
            shadow: false,
        });
        state.selected_action_index = Some(0);
        state.selected_tool = Tool::Background;
        state.static_image_transform.yaw = 20.0;
        *host.editor.borrow_mut() = Some(state);
        let transform = ViewTransform {
            offset_x: 10.0,
            offset_y: 10.0,
            canvas_width: 120.0,
            canvas_height: 100.0,
            image_rect_x: 10.0,
            image_rect_y: 10.0,
            ..ViewTransform::for_image(100.0, 80.0)
        };
        let paint = |zoom| {
            let mut surface = ImageSurface::create(gtk4::cairo::Format::ARgb32, 120, 100).unwrap();
            {
                let context = gtk4::cairo::Context::new(&surface).unwrap();
                host.paint_motion_preview(
                    &context,
                    transform,
                    crate::recording::editor::model::MotionTransform {
                        scale: zoom,
                        ..Default::default()
                    },
                    (0.5, 0.5),
                    false,
                );
            }
            surface.flush();
            let stride = surface.stride() as usize;
            let data = surface.data().unwrap();
            render::cairo_argb_to_rgba_image(120, 100, stride, data.as_ref())
        };
        let preview = paint(1.0);
        assert_eq!(*preview.get_pixel(0, 0), image::Rgba([0, 0, 255, 255]));
        assert_eq!(*preview.get_pixel(15, 15), image::Rgba([240, 0, 0, 255]));
        assert!(preview.get_pixel(60, 50)[1] > 240);
        let zoomed = paint(1.2);
        assert_eq!(*zoomed.get_pixel(0, 0), image::Rgba([0, 0, 255, 255]));
        assert_eq!(*preview.get_pixel(6, 50), image::Rgba([0, 0, 255, 255]));
        assert_eq!(*zoomed.get_pixel(6, 50), image::Rgba([240, 0, 0, 255]));
        host.mutate(|state| {
            state.static_image_transform = Default::default();
            state.selected_tool = Tool::Select;
            state.selected_action_index = None;
        });
        assert_eq!(paint(1.0), preview);
        host.mutate(|state| {
            state.background.fill = Fill::None;
            state.background.changed();
        });
        assert_ne!(paint(1.0), preview);
    }

    #[test]
    fn motion_preview_projects_position_rotation_and_zoom_anchor() {
        use crate::capture::editor::state::background::Fill;
        use crate::recording::editor::model::MotionTransform;
        let host = host();
        let mut state = EditorState::new(RgbaImage::from_pixel(
            100,
            80,
            image::Rgba([240, 0, 0, 255]),
        ));
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([0, 0, 255, 255]),
        )));
        state.background.padding = 40.0;
        let layout = state.background.layout(100, 80);
        let transform = ViewTransform {
            offset_x: layout.image_x,
            offset_y: layout.image_y,
            canvas_width: layout.width,
            canvas_height: layout.height,
            image_rect_x: layout.image_x,
            image_rect_y: layout.image_y,
            ..ViewTransform::for_image(100.0, 80.0)
        };
        *host.editor.borrow_mut() = Some(state);
        let paint = |pose: MotionTransform, anchor| {
            let width = layout.width.ceil() as i32;
            let height = layout.height.ceil() as i32;
            let mut surface =
                ImageSurface::create(gtk4::cairo::Format::ARgb32, width, height).unwrap();
            {
                let context = gtk4::cairo::Context::new(&surface).unwrap();
                host.paint_motion_preview(&context, transform, pose, anchor, false);
            }
            surface.flush();
            let stride = surface.stride() as usize;
            let data = surface.data().unwrap();
            render::cairo_argb_to_rgba_image(width as u32, height as u32, stride, data.as_ref())
        };
        let default = MotionTransform::default();
        let baseline = paint(default, (0.5, 0.5));
        let moved = paint(
            MotionTransform {
                pos_x: 0.5,
                ..default
            },
            (0.5, 0.5),
        );
        let x = (layout.image_x + 85.0) as u32;
        let y = (layout.image_y + 40.0) as u32;
        assert_eq!(*baseline.get_pixel(x, y), image::Rgba([240, 0, 0, 255]));
        assert_eq!(*moved.get_pixel(x, y), image::Rgba([0, 0, 255, 255]));
        let rotated = paint(
            MotionTransform {
                rotation_y: 20.0,
                perspective: 0.8,
                ..default
            },
            (0.5, 0.5),
        );
        assert_ne!(rotated, baseline);
        let centered = paint(
            MotionTransform {
                scale: 1.4,
                ..default
            },
            (0.5, 0.5),
        );
        let anchored = paint(
            MotionTransform {
                scale: 1.4,
                ..default
            },
            (0.0, 0.5),
        );
        let left = (layout.image_x - 10.0) as u32;
        assert_eq!(*centered.get_pixel(left, y), image::Rgba([240, 0, 0, 255]));
        assert_eq!(*anchored.get_pixel(left, y), image::Rgba([0, 0, 255, 255]));
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
    fn transformed_preview_matches_export_and_drawing_pauses_pose() {
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
        state.background.radius = 12.0;
        state.static_image_transform.yaw = 18.0;
        state.static_image_transform.pos_x = 0.2;
        state.selected_tool = Tool::Background;
        let exported = state.to_export_image().unwrap();
        *host.editor.borrow_mut() = Some(state);
        let transform = ViewTransform {
            offset_x: 10.0,
            offset_y: 10.0,
            has_background: true,
            canvas_width: 120.0,
            canvas_height: 100.0,
            image_rect_x: 10.0,
            image_rect_y: 10.0,
            ..ViewTransform::for_image(100.0, 80.0)
        };
        let paint = || {
            let mut surface = ImageSurface::create(gtk4::cairo::Format::ARgb32, 120, 100).unwrap();
            {
                let context = gtk4::cairo::Context::new(&surface).unwrap();
                host.paint_document(&context, transform);
            }
            surface.flush();
            let stride = surface.stride() as usize;
            let data = surface.data().unwrap();
            render::cairo_argb_to_rgba_image(120, 100, stride, data.as_ref())
        };
        let preview = paint();
        let red_bounds = |image: &RgbaImage| {
            image
                .enumerate_pixels()
                .filter(|(_, _, pixel)| pixel[0] > 180 && pixel[2] < 80)
                .map(|(x, y, _)| (x, y))
                .fold(None::<(u32, u32, u32, u32)>, |bounds, (x, y)| {
                    Some(match bounds {
                        Some((min_x, min_y, max_x, max_y)) => {
                            (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                        }
                        None => (x, y, x, y),
                    })
                })
                .unwrap()
        };
        let actual = red_bounds(&preview);
        let expected = red_bounds(&exported);
        for (a, b) in [
            (actual.0, expected.0),
            (actual.1, expected.1),
            (actual.2, expected.2),
            (actual.3, expected.3),
        ] {
            assert!(
                a.abs_diff(b) <= 1,
                "preview {actual:?} differs from export {expected:?}"
            );
        }
        assert_eq!(preview.get_pixel(0, 0), exported.get_pixel(0, 0));
        assert_eq!(
            host.read(|state| state.to_export_image().unwrap()),
            Some(exported.clone())
        );
        host.mutate(|state| state.selected_tool = Tool::Pen);
        assert_ne!(paint(), exported);
        assert_eq!(
            host.read(|state| state.static_image_transform.yaw),
            Some(18.0)
        );
    }

    #[test]
    fn shadow_preview_matches_export_and_reuses_interactive_mask() {
        use crate::capture::editor::state::background::Fill;
        use crate::recording::editor::model::MotionSceneShadowPreset;
        let host = host();
        let mut state = EditorState::new(RgbaImage::from_pixel(
            100,
            80,
            image::Rgba([240, 0, 0, 255]),
        ));
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([240, 240, 240, 255]),
        )));
        state.background.padding = 80.0;
        state.background.radius = 10.0;
        state.scene_shadow.preset = MotionSceneShadowPreset::Spread;
        state.selected_tool = Tool::Background;
        let exported = state.to_export_image().unwrap();
        let layout = state.background.layout(100, 80);
        *host.editor.borrow_mut() = Some(state);
        let transform = ViewTransform {
            offset_x: layout.image_x,
            offset_y: layout.image_y,
            has_background: true,
            canvas_width: layout.width,
            canvas_height: layout.height,
            image_rect_x: layout.image_x,
            image_rect_y: layout.image_y,
            ..ViewTransform::for_image(100.0, 80.0)
        };
        let paint = || {
            let mut surface = ImageSurface::create(
                gtk4::cairo::Format::ARgb32,
                layout.width as i32,
                layout.height as i32,
            )
            .unwrap();
            {
                let context = gtk4::cairo::Context::new(&surface).unwrap();
                host.paint_document(&context, transform);
            }
            surface.flush();
            let stride = surface.stride() as usize;
            let data = surface.data().unwrap();
            render::cairo_argb_to_rgba_image(
                layout.width as u32,
                layout.height as u32,
                stride,
                data.as_ref(),
            )
        };
        assert_eq!(paint(), exported);
        host.mutate(|state| state.begin_history_interaction());
        let first_frame = paint();
        let cached = host.shadow_cache.borrow().as_ref().unwrap().3.clone();
        host.mutate(|state| {
            state.scene_shadow.light = (-1.0, -0.5);
            state.scene_shadow.opacity = 0.4;
        });
        assert_ne!(paint(), first_frame);
        assert_eq!(
            cached.to_raw_none(),
            host.shadow_cache.borrow().as_ref().unwrap().3.to_raw_none()
        );
        host.mutate(|state| state.finish_history_interaction());
        paint();
        assert_ne!(
            cached.to_raw_none(),
            host.shadow_cache.borrow().as_ref().unwrap().3.to_raw_none()
        );
    }

    #[test]
    fn frame_style_preview_matches_flat_export() {
        use crate::capture::editor::state::background::Fill;
        use crate::capture::editor::types::FrameStyle;
        let host = host();
        let mut state = EditorState::new(RgbaImage::from_pixel(
            100,
            80,
            image::Rgba([240, 0, 0, 255]),
        ));
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([80, 140, 210, 255]),
        )));
        state.background.padding = 120.0;
        state.background.radius = 20.0;
        state.selected_tool = Tool::Background;
        *host.editor.borrow_mut() = Some(state);
        for style in FrameStyle::ALL {
            host.mutate(|state| {
                state.set_frame_style(style);
            });
            let (layout, exported) = host
                .read(|state| {
                    (
                        state.background.layout(100, 80),
                        state.to_export_image().unwrap(),
                    )
                })
                .unwrap();
            let transform = ViewTransform {
                offset_x: layout.image_x,
                offset_y: layout.image_y,
                has_background: true,
                canvas_width: layout.width,
                canvas_height: layout.height,
                image_rect_x: layout.image_x,
                image_rect_y: layout.image_y,
                ..ViewTransform::for_image(100.0, 80.0)
            };
            let mut surface = ImageSurface::create(
                gtk4::cairo::Format::ARgb32,
                layout.width as i32,
                layout.height as i32,
            )
            .unwrap();
            {
                let context = gtk4::cairo::Context::new(&surface).unwrap();
                host.paint_document(&context, transform);
            }
            surface.flush();
            let stride = surface.stride() as usize;
            let data = surface.data().unwrap();
            let preview = render::cairo_argb_to_rgba_image(
                layout.width as u32,
                layout.height as u32,
                stride,
                data.as_ref(),
            );
            assert_eq!(preview, exported, "preview differs for {style:?}");
        }
    }

    #[test]
    fn glass_preview_matches_export() {
        use crate::capture::editor::state::background::Fill;
        use crate::capture::editor::types::FrameStyle;
        let host = host();
        let mut state = EditorState::new(RgbaImage::from_pixel(
            400,
            300,
            image::Rgba([240, 0, 0, 255]),
        ));
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([80, 140, 210, 255]),
        )));
        state.background.padding = 60.0;
        state.background.radius = 20.0;
        state.selected_tool = Tool::Background;
        *host.editor.borrow_mut() = Some(state);
        for style in [
            FrameStyle::GlassLight,
            FrameStyle::GlassDark,
            FrameStyle::Liquid,
        ] {
            host.mutate(|state| {
                state.set_frame_style(style);
            });
            let (layout, exported) = host
                .read(|state| {
                    (
                        state.background.layout(400, 300),
                        state.to_export_image().unwrap(),
                    )
                })
                .unwrap();
            let transform = ViewTransform {
                offset_x: layout.image_x,
                offset_y: layout.image_y,
                has_background: true,
                canvas_width: layout.width,
                canvas_height: layout.height,
                image_rect_x: layout.image_x,
                image_rect_y: layout.image_y,
                ..ViewTransform::for_image(400.0, 300.0)
            };
            let paint = || {
                let mut surface = ImageSurface::create(
                    gtk4::cairo::Format::ARgb32,
                    layout.width as i32,
                    layout.height as i32,
                )
                .unwrap();
                {
                    let context = gtk4::cairo::Context::new(&surface).unwrap();
                    host.paint_document(&context, transform);
                }
                surface.flush();
                let stride = surface.stride() as usize;
                let data = surface.data().unwrap();
                render::cairo_argb_to_rgba_image(
                    layout.width as u32,
                    layout.height as u32,
                    stride,
                    data.as_ref(),
                )
            };
            assert_eq!(paint(), exported, "glass preview differs for {style:?}");
        }
    }

    #[test]
    fn preview_card_is_bounded_and_reused_until_document_changes() {
        assert_eq!(preview_card_size(3840, 2160), (1600, 900));
        assert_eq!(preview_card_size(100, 80), (100, 80));
        assert_eq!(preview_card_size(1, 3840), (1, 1600));

        let host = host();
        let mut state = EditorState::new(RgbaImage::from_pixel(
            2000,
            1000,
            image::Rgba([240, 0, 0, 255]),
        ));
        state.static_image_transform.yaw = 12.0;
        state.selected_tool = Tool::Background;
        *host.editor.borrow_mut() = Some(state);
        let surface = ImageSurface::create(gtk4::cairo::Format::ARgb32, 200, 100).unwrap();
        let context = gtk4::cairo::Context::new(&surface).unwrap();
        let transform = ViewTransform {
            scale: 0.1,
            canvas_scale: 0.1,
            canvas_width: 2000.0,
            canvas_height: 1000.0,
            ..ViewTransform::for_image(2000.0, 1000.0)
        };
        host.paint_document(&context, transform);
        let cached = host.card_cache.borrow().as_ref().unwrap().3.clone();
        assert_eq!((cached.width(), cached.height()), (1600, 800));
        host.mutate(|state| state.static_image_transform.pitch = 8.0);
        host.paint_document(&context, transform);
        assert_eq!(
            cached.to_raw_none(),
            host.card_cache.borrow().as_ref().unwrap().3.to_raw_none()
        );
        host.mutate(|state| state.mark_working_image_dirty());
        host.paint_document(&context, transform);
        assert_ne!(
            cached.to_raw_none(),
            host.card_cache.borrow().as_ref().unwrap().3.to_raw_none()
        );
        let rebuilt = host.card_cache.borrow().as_ref().unwrap().3.clone();
        host.mutate(|state| {
            state.actions.push(AnnotationAction::Box {
                rect: crate::capture::editor::types::Rect {
                    x: 20,
                    y: 20,
                    width: 100,
                    height: 100,
                },
                color: crate::capture::editor::types::DrawColor::new(1.0, 1.0, 1.0, 1.0),
                stroke_size: 4.0,
                shadow: false,
            });
        });
        host.paint_document(&context, transform);
        assert_ne!(
            rebuilt.to_raw_none(),
            host.card_cache.borrow().as_ref().unwrap().3.to_raw_none()
        );
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
            Duration::ZERO,
        );
        let (preview, image) = receiver.recv_timeout(Duration::from_secs(15)).unwrap();
        assert!(preview);
        assert!(image.unwrap().0.width() < 1920);
        let (preview, image) = receiver.recv_timeout(Duration::from_secs(15)).unwrap();
        assert!(!preview);
        assert_eq!(image.unwrap().0.dimensions(), (1920, 1080));
        let receiver = spawn_background_decode(
            path,
            Some(thumb),
            Arc::new(AtomicBool::new(true)),
            Duration::ZERO,
        );
        assert!(receiver.recv_timeout(Duration::from_secs(15)).is_err());
    }

    #[test]
    fn a_quick_full_decode_skips_the_thumbnail() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/capture/editor/background-images");
        let receiver = spawn_background_decode(
            assets.join("wallpaper-001.jpg"),
            Some(assets.join("wallpaper-thumb-001.jpg")),
            Arc::new(AtomicBool::new(false)),
            Duration::from_secs(30),
        );
        let (preview, image) = receiver.recv_timeout(Duration::from_secs(30)).unwrap();
        assert!(!preview, "the full image arrives first");
        assert_eq!(image.unwrap().0.dimensions(), (1920, 1080));
        assert!(receiver.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn wallpaper_selection_keeps_the_full_image_after_the_thumbnail_preview() {
        let host = host();
        *host.editor.borrow_mut() = Some(EditorState::new(RgbaImage::new(100, 80)));
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/capture/editor/background-images");
        let receiver = spawn_background_decode(
            assets.join("wallpaper-001.jpg"),
            Some(assets.join("wallpaper-thumb-001.jpg")),
            Arc::new(AtomicBool::new(false)),
            Duration::ZERO,
        );
        let mut on_selected = Some(|_: &RgbaImage| {});
        let (preview, result) = receiver.recv_timeout(Duration::from_secs(15)).unwrap();
        assert!(preview);
        let (image, bytes) = result.unwrap();
        let flow = host.apply_decoded_background(preview, image, bytes, &mut on_selected);
        assert!(matches!(flow, gtk4::glib::ControlFlow::Continue));
        let (preview, result) = receiver.recv_timeout(Duration::from_secs(15)).unwrap();
        assert!(!preview);
        let (image, bytes) = result.unwrap();
        let flow = host.apply_decoded_background(preview, image, bytes, &mut on_selected);
        assert!(matches!(flow, gtk4::glib::ControlFlow::Break));
        let dimensions = host
            .read(|state| match &state.background.fill {
                crate::capture::editor::state::background::Fill::Image(image) => {
                    Some(image.dimensions())
                }
                _ => None,
            })
            .flatten();
        assert_eq!(dimensions, Some((1920, 1080)));
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
    fn a_jittered_shape_drag_leaves_no_dot() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let h = loaded(Tool::Box);
            let transform = ViewTransform::for_image(64.0, 64.0);
            let gesture = gtk4::GestureDrag::new();

            h.drag_begin(&transform, &gesture, Point { x: 20.0, y: 20.0 });
            h.drag_end(&transform, &gesture, 5.0, 5.0);
            assert_eq!(
                h.read(|st| st.actions.len()),
                Some(0),
                "a short drag must not leave a dot"
            );

            h.drag_begin(&transform, &gesture, Point { x: 20.0, y: 20.0 });
            h.drag_end(&transform, &gesture, 20.0, 20.0);
            assert_eq!(
                h.read(|st| st.actions.len()),
                Some(1),
                "a deliberate drag still commits"
            );
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

    fn text_host(width: u32, height: u32) -> Rc<EditorHost> {
        let h = host();
        h.load_document_headless(test_image(width, height));
        h.set_tool(Tool::Text);
        h
    }

    fn start_text_drag(
        st: &mut EditorState,
        handle: Option<MoveHandle>,
        resizing: bool,
        at: Point,
    ) {
        st.active_text_is_dragging = true;
        st.active_text_drag_handle = handle;
        st.active_text_is_resizing = resizing;
        st.active_text_drag_start = Some(at);
        st.active_text_drag_start_bounds = st.active_text_bounds.as_ref().map(|b| b.rect);
    }

    #[test]
    fn resize_corner_scales_the_live_text_while_dragging() {
        let h = text_host(800, 600);
        let t = ViewTransform::for_image(800.0, 600.0);
        h.with_state(|st| {
            st.begin_text_input(Point { x: 20.0, y: 60.0 }, 220.0, 44.0);
            st.insert_text_input_str("Hi");
            st.fit_active_text_to_layout();
            let before = st.text_size;
            let rect = st.active_text_bounds.as_ref().unwrap().rect;
            let corner = Point {
                x: f64::from(rect.x + rect.width),
                y: f64::from(rect.y + rect.height),
            };
            start_text_drag(st, None, true, corner);
            h.update_active_text_drag(
                st,
                &t,
                Point {
                    x: corner.x + 300.0,
                    y: corner.y + 200.0,
                },
            );
            assert!(st.active_text_is_dragging, "still mid-drag");
            assert!(st.text_size > before, "{} -> {}", before, st.text_size);
        });
    }

    #[test]
    fn dragging_committed_text_handles_reflows_it_live() {
        let h = text_host(800, 600);
        let t = ViewTransform::for_image(800.0, 600.0);
        h.with_state(|st| {
            st.begin_text_input(Point { x: 20.0, y: 60.0 }, 400.0, 44.0);
            st.insert_text_input_str("one two three four");
            st.fit_active_text_to_layout();
            assert!(st.commit_active_text_input());
            let index = st.actions.len() - 1;
            st.selected_action_index = Some(index);
            st.active_text_bounds = committed_text_bounds(st, index);
            let rect = st.active_text_bounds.as_ref().unwrap().rect;
            let right = Point {
                x: f64::from(rect.x + rect.width),
                y: f64::from(rect.y),
            };
            start_text_drag(st, Some(MoveHandle::Right), false, right);
            h.update_active_text_drag(
                st,
                &t,
                Point {
                    x: right.x + 120.0,
                    y: right.y,
                },
            );
            let Some(AnnotationAction::Text { max_width, .. }) = st.actions.get(index) else {
                panic!("text action");
            };
            assert_eq!(*max_width, Some(f64::from(rect.width + 120)));

            let size = |st: &EditorState| match st.actions.get(index) {
                Some(AnnotationAction::Text { font, .. }) => font.size,
                _ => 0.0,
            };
            let before = size(st);
            st.active_text_drag_start_bounds = st.active_text_bounds.as_ref().map(|b| b.rect);
            let rect = st.active_text_bounds.as_ref().unwrap().rect;
            let corner = Point {
                x: f64::from(rect.x + rect.width),
                y: f64::from(rect.y + rect.height),
            };
            st.active_text_drag_start = Some(corner);
            st.active_text_drag_handle = None;
            st.active_text_is_resizing = true;
            h.update_active_text_drag(
                st,
                &t,
                Point {
                    x: corner.x + 200.0,
                    y: corner.y + 200.0,
                },
            );
            assert!(size(st) > before, "{} -> {}", before, size(st));
        });
    }

    #[test]
    fn text_size_changes_resize_the_live_box_both_ways() {
        let h = text_host(800, 600);
        h.with_state(|st| {
            st.set_text_size(24.0);
            st.begin_text_input(Point { x: 20.0, y: 60.0 }, 220.0, 44.0);
            st.insert_text_input_str("Hello\nthere");
            st.fit_active_text_to_layout();
            let width = |st: &EditorState| st.active_text_bounds.as_ref().unwrap().rect.width;
            let height = |st: &EditorState| st.active_text_bounds.as_ref().unwrap().rect.height;
            let (w24, h24) = (width(st), height(st));
            set_text_size_and_fit(st, 48.0);
            assert!(width(st) > w24 && height(st) > h24, "grows at once");
            set_text_size_and_fit(st, 16.0);
            assert!(width(st) < w24 && height(st) < h24, "shrinks at once");
        });
    }

    #[test]
    fn a_text_box_can_grow_into_the_background_padding() {
        let h = text_host(200, 100);
        h.with_state(|st| {
            st.background.fill = crate::capture::editor::state::background::Fill::Image(Arc::new(
                RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 255, 255])),
            ));
            st.background.set_padding(60.0);
            st.begin_text_input(Point { x: 150.0, y: 40.0 }, 220.0, 44.0);
            st.insert_text_input_str("a much longer line of text");
            st.fit_active_text_to_layout();
            let rect = st.active_text_bounds.as_ref().unwrap().rect;
            assert!(
                rect.x + rect.width > 200,
                "box stops at the screenshot: {rect:?}"
            );
        });
    }

    #[test]
    fn pasted_text_lands_at_the_caret_with_newline_breaks() {
        let h = loaded(Tool::Text);
        h.with_state(|st| {
            st.begin_text_input(
                crate::capture::editor::types::Point { x: 5.0, y: 5.0 },
                100.0,
                24.0,
            );
            st.insert_text_input_str("ab");
            st.move_cursor_left();
            assert!(insert_into_active_text(st, "Tiếng\r\nViệt\r"));
            assert_eq!(
                st.get_text_input().map(|input| input.text.clone()),
                Some("aTiếng\nViệt\nb".to_string())
            );
            st.cancel_text_input();
            assert!(!insert_into_active_text(st, "x"));
        });
    }

    #[test]
    fn paste_shortcuts_stay_with_the_live_text_box() {
        let h = loaded(Tool::Text);
        h.with_state(|st| {
            st.begin_text_input(
                crate::capture::editor::types::Point { x: 5.0, y: 5.0 },
                100.0,
                24.0,
            )
        });
        assert_eq!(h.handle_key(gdk::Key::v, ctrl()), Propagation::Stop);
        assert_eq!(
            h.handle_key(gdk::Key::Insert, gdk::ModifierType::SHIFT_MASK),
            Propagation::Stop
        );
        assert_eq!(
            h.read(|st| st.get_text_input().map(|input| input.text.is_empty())),
            Some(Some(true))
        );
    }

    #[test]
    fn input_method_commits_into_the_live_text_box_only() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let h = loaded(Tool::Text);
            let area = DrawingArea::new();
            h.install_text_input_method(&area, Rc::new(|| ViewTransform::for_image(64.0, 64.0)));
            let im = h.text_input_method.borrow().clone().expect("input method");
            im.emit_by_name::<()>("commit", &[&"dropped"]);
            assert_eq!(h.read(|st| st.active_text_input.is_none()), Some(true));
            h.with_state(|st| {
                st.begin_text_input(
                    crate::capture::editor::types::Point { x: 5.0, y: 5.0 },
                    100.0,
                    24.0,
                )
            });
            h.changed();
            assert!(h.text_input_focused.get());
            im.emit_by_name::<()>("commit", &[&"Tiếng Việt"]);
            assert_eq!(
                h.read(|st| st.get_text_input().map(|input| input.text.clone())),
                Some(Some("Tiếng Việt".to_string()))
            );
            h.with_state(|st| st.cancel_text_input());
            h.changed();
            assert!(!h.text_input_focused.get());
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn arrow_style_swap_normalizes_control_points() {
        // Undo rebuilds the effects through a GLib timeout, which needs the
        // default main context: run on the test binary's GTK thread, which
        // owns it, instead of racing that thread for it.
        let Some(()) = crate::test_support::with_gtk(|| {
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
        }) else {
            return;
        };
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

    /// Manual probe: the editor's Copy must advertise a native Windows image
    /// format while the owning process is still alive.
    #[test]
    #[ignore = "clobbers the live clipboard; run manually with --ignored --nocapture"]
    fn copy_to_clipboard_advertises_windows_image_format() {
        gtk4::init().expect("GTK must initialize for the clipboard probe");
        let messages: Rc<std::cell::RefCell<Vec<String>>> =
            Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = messages.clone();
        let h = EditorHost::new(
            Rc::new(|| {}),
            Rc::new(move |message: String| sink.borrow_mut().push(message)),
            Rc::new(|| (0.0, 0.0)),
        );
        h.load_document_headless(test_image(64, 64));
        h.copy_to_clipboard();

        let main_loop = glib::MainLoop::new(None, false);
        let quit = main_loop.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || quit.quit());
        main_loop.run();

        let mut formats = Vec::new();
        unsafe {
            use windows_sys::Win32::System::DataExchange::{
                CloseClipboard, EnumClipboardFormats, GetClipboardFormatNameW, OpenClipboard,
            };
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                let mut format = 0;
                loop {
                    format = EnumClipboardFormats(format);
                    if format == 0 {
                        break;
                    }
                    let mut name = [0u16; 128];
                    let len = GetClipboardFormatNameW(format, name.as_mut_ptr(), name.len() as i32);
                    let name = if len > 0 {
                        String::from_utf16_lossy(&name[..len as usize])
                    } else {
                        match format {
                            1 => "CF_TEXT".to_string(),
                            2 => "CF_BITMAP".to_string(),
                            8 => "CF_DIB".to_string(),
                            13 => "CF_UNICODETEXT".to_string(),
                            17 => "CF_DIBV5".to_string(),
                            other => format!("predefined {other}"),
                        }
                    };
                    formats.push((format, name));
                }
                CloseClipboard();
            }
        }
        assert!(
            formats
                .iter()
                .any(|(format, _)| *format == 8 || *format == 17),
            "no CF_DIB/CF_DIBV5 image on the Windows clipboard: {formats:?}"
        );
        assert!(
            !messages
                .borrow()
                .iter()
                .any(|message| message.contains("Could not copy")),
            "copy reported a failure: {:?}",
            messages.borrow()
        );
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
