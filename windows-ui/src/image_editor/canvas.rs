//! The editor's display canvas.
//!
//! The canvas owns the checkerboard, the decoded image, the empty drop state,
//! the zoom pill and the view transform. The annotation controller handles
//! drawing and editing in image coordinates through that same transform.

use gtk4::cairo::Context;
use gtk4::gdk;
use gtk4::gdk_pixbuf::{Colorspace, Pixbuf};
use gtk4::glib::{self, Propagation};
use gtk4::prelude::*;
use gtk4::{
    gio, Align, Box as GtkBox, Button, DrawingArea, DropTarget, EventControllerScroll,
    EventControllerScrollFlags, FileChooserAction, FileChooserNative, FileFilter, Image, Label,
    Orientation, Overlay, ResponseType, Spinner,
};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use super::ui_support::{is_supported_image, px, TOP_CHROME_HEIGHT};
use crate::i18n::{t, tfmt};

const ZOOM_STEP: f64 = 1.15;
const MIN_ZOOM: f64 = 0.05;
const MAX_ZOOM: f64 = 16.0;
/// Breathing room below the top chrome and above the zoom pill.
const CONTENT_MARGIN: f64 = 16.0;
const DOCKED_BAR_INSET: f64 = 48.0;
const CROP_HANDLE_MARGIN: f64 = 20.0;
const CROP_BOTTOM_CHROME: f64 = 64.0;
const TOAST_LIFETIME: Duration = Duration::from_millis(4200);

struct SavedCropView {
    width: i32,
    height: i32,
    manual_zoom: Option<f64>,
    pan: (f64, f64),
}

struct PaintOptions {
    scale: f64,
    dark: bool,
    docked_inset: f64,
    crop_mode: bool,
}

struct ImageState {
    pixbuf: Option<Pixbuf>,
    width: i32,
    height: i32,
    canvas_width: i32,
    canvas_height: i32,
    image_x: f64,
    image_y: f64,
    has_background: bool,
    frame_aspect: Option<crate::capture::editor::state::background::FrameRatio>,
    /// `None` keeps the image fitted while the window resizes.
    manual_zoom: Option<f64>,
    pan: (f64, f64),
    saved_crop_view: Option<SavedCropView>,
}

impl Default for ImageState {
    fn default() -> Self {
        Self {
            pixbuf: None,
            width: 0,
            height: 0,
            canvas_width: 0,
            canvas_height: 0,
            image_x: 0.0,
            image_y: 0.0,
            has_background: false,
            frame_aspect: None,
            manual_zoom: None,
            pan: (0.0, 0.0),
            saved_crop_view: None,
        }
    }
}

struct DecodedImage {
    bytes: Vec<u8>,
    width: i32,
    height: i32,
}

/// Where the pointer last was over the canvas, so a tool, brush or detection
/// change can refresh its cursor without waiting for the pointer to move.
#[derive(Default)]
pub(super) struct Hover {
    pub(super) point: Cell<Option<crate::capture::editor::types::Point>>,
}

pub(super) struct Canvas {
    pub root: Overlay,
    pub area: DrawingArea,
    pub open_button: Button,
    pub motion_button: Button,
    empty: GtkBox,
    loading_row: GtkBox,
    spinner: Spinner,
    status: Label,
    toast: Label,
    zoom_button: Button,
    zoom_in: Button,
    zoom_out: Button,
    state: Rc<RefCell<ImageState>>,
    scale: f64,
    /// Extra logical pixels the canvas gives up below the top chrome while a
    /// tool's option bar is docked, like the Linux docked-bar inset.
    docked_inset: Rc<Cell<f64>>,
    crop_mode: Rc<Cell<bool>>,
    /// Bumped for each open so a late decode cannot replace a newer image.
    generation: Rc<Cell<u64>>,
    toast_generation: Rc<Cell<u64>>,
    /// The annotation document and interaction controller.
    pub host: Rc<super::annotate::EditorHost>,
    hover: Rc<Hover>,
    /// Subscribed change listeners (sub-bars, history pill) fired by the host.
    listeners: Rc<RefCell<Vec<Rc<dyn Fn()>>>>,
}

pub(super) fn build(scale: f64, dark: bool) -> Rc<Canvas> {
    let state = Rc::new(RefCell::new(ImageState::default()));
    let docked_inset = Rc::new(Cell::new(0.0));
    let crop_mode = Rc::new(Cell::new(false));
    let last_zoom_text = Rc::new(RefCell::new(String::new()));

    let (empty, open_button, loading_row, spinner, status) = build_empty_state(scale);
    let (zoom_pill, zoom_button, zoom_out, zoom_in) = build_zoom_pill(scale);

    let toast = Label::new(None);
    toast.add_css_class("image-editor-toast");
    toast.set_halign(Align::Center);
    toast.set_valign(Align::End);
    toast.set_margin_bottom(px(20.0, scale));
    toast.set_wrap(true);
    toast.set_max_width_chars(60);
    toast.set_visible(false);

    let area = DrawingArea::new();
    area.set_focusable(true);
    let listeners: Rc<RefCell<Vec<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(Vec::new()));
    let host = super::annotate::EditorHost::new(
        {
            let listeners = listeners.clone();
            let weak_area = area.downgrade();
            Rc::new(move || {
                if let Some(area) = weak_area.upgrade() {
                    area.queue_draw();
                }
                for listener in listeners.borrow().iter() {
                    listener();
                }
            })
        },
        {
            let toast = toast.clone();
            let toast_generation = Rc::new(Cell::new(0u64));
            Rc::new(move |message| {
                toast.set_text(&message);
                toast.set_visible(true);
                let token = toast_generation.get() + 1;
                toast_generation.set(token);
                let toast = toast.clone();
                let gen = toast_generation.clone();
                glib::timeout_add_local_once(TOAST_LIFETIME, move || {
                    if gen.get() == token {
                        toast.set_visible(false);
                    }
                });
            })
        },
        {
            let state = state.clone();
            Rc::new(move || state.borrow().pan)
        },
    );

    area.add_css_class("image-editor-canvas");
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_draw_func({
        let state = state.clone();
        let host = host.clone();
        let docked_inset = docked_inset.clone();
        let crop_mode = crop_mode.clone();
        let last_zoom_text = last_zoom_text.clone();
        let zoom_button = zoom_button.downgrade();
        move |area, context, _width, _height| {
            let Some(zoom) = paint(
                area,
                context,
                &state.borrow(),
                PaintOptions {
                    scale,
                    dark,
                    docked_inset: docked_inset.get(),
                    crop_mode: crop_mode.get(),
                },
                &host,
            ) else {
                return;
            };
            let text = zoom_text(zoom);
            let mut last = last_zoom_text.borrow_mut();
            if *last == text {
                return;
            }
            *last = text.clone();
            let button = zoom_button.clone();
            glib::idle_add_local_once(move || {
                if let Some(button) = button.upgrade() {
                    button.set_label(&text);
                }
            });
        }
    });

    // Bottom-centre mode switch, mirroring the Linux floating motion control.
    let motion = GtkBox::new(Orientation::Horizontal, 0);
    motion.add_css_class("image-editor-motion-pill");
    motion.set_halign(Align::Center);
    motion.set_valign(Align::End);
    motion.set_margin_bottom(px(16.0, scale));
    let motion_button = Button::with_label(&t("Motion"));
    motion_button.add_css_class("image-editor-motion-button");
    motion_button.set_focusable(false);
    motion_button.set_sensitive(false);
    motion_button.set_tooltip_text(Some(&t(
        "Switch to Motion: not available in the Windows image editor yet",
    )));
    motion.append(&motion_button);

    let root = Overlay::new();
    root.set_hexpand(true);
    root.set_vexpand(true);
    root.set_child(Some(&area));
    root.add_overlay(&empty);
    root.add_overlay(&zoom_pill);
    root.add_overlay(&motion);
    root.add_overlay(&toast);

    let canvas = Rc::new(Canvas {
        root,
        area,
        open_button,
        motion_button,
        empty,
        loading_row,
        spinner,
        status,
        toast,
        zoom_button,
        zoom_in,
        zoom_out,
        state,
        scale,
        docked_inset,
        crop_mode,
        generation: Rc::new(Cell::new(0)),
        toast_generation: Rc::new(Cell::new(0)),
        host,
        hover: Rc::new(Hover::default()),
        listeners,
    });

    canvas.install_gestures();
    canvas.install_zoom_controls();
    canvas.install_model_timers();
    canvas.area.connect_resize({
        let canvas = Rc::downgrade(&canvas);
        move |_, _, _| {
            if let Some(canvas) = canvas.upgrade() {
                canvas.host.changed();
            }
        }
    });
    canvas
}

impl Canvas {
    /// Open the file chooser, then load the chosen image in this window.
    pub(super) fn choose_image(self: &Rc<Self>, parent: &impl IsA<gtk4::Window>) {
        let chooser = FileChooserNative::new(
            Some(&t("Open image")),
            Some(parent),
            FileChooserAction::Open,
            Some(&t("Open")),
            Some(&t("Cancel")),
        );

        let filter = FileFilter::new();
        filter.set_name(Some(&t("Images")));
        for pattern in ["*.png", "*.jpg", "*.jpeg", "*.webp"] {
            filter.add_pattern(pattern);
        }
        for mime in ["image/png", "image/jpeg", "image/webp"] {
            filter.add_mime_type(mime);
        }
        chooser.add_filter(&filter);

        chooser.connect_response({
            let canvas = Rc::downgrade(self);
            move |chooser, response| {
                if response == ResponseType::Accept {
                    if let Some(path) = chooser.file().and_then(|file| file.path()) {
                        if let Some(canvas) = canvas.upgrade() {
                            canvas.load(&path);
                        }
                    }
                }
                chooser.hide();
            }
        });
        chooser.show();
    }

    /// Decode `path` off the main thread and swap it into the canvas.
    pub(super) fn load(self: &Rc<Self>, path: &Path) {
        if !is_supported_image(path) {
            self.show_error(&t(
                "That file is not a supported image. Choose a PNG, JPEG, or WebP.",
            ));
            return;
        }

        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.toast.set_visible(false);
        self.open_button.set_sensitive(false);
        if self.state.borrow().pixbuf.is_none() {
            self.empty.set_visible(true);
            self.loading_row.set_visible(true);
            self.spinner.start();
            self.spinner.set_visible(true);
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            self.status
                .set_text(&tfmt("Opening {name}…", &[("name", &name)]));
        }

        let (sender, receiver) = mpsc::channel::<Result<DecodedImage, String>>();
        let path_thread = path.to_path_buf();
        std::thread::spawn(move || {
            let _ = sender.send(decode_image(&path_thread));
        });

        let canvas = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(16), move || {
            let Some(canvas) = canvas.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if canvas.generation.get() != generation {
                return glib::ControlFlow::Break;
            }
            match receiver.try_recv() {
                Ok(Ok(decoded)) => {
                    canvas.finish_load(decoded);
                    glib::ControlFlow::Break
                }
                Ok(Err(error)) => {
                    canvas.finish_error(&error);
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    canvas.finish_error(&t("The image could not be read."));
                    glib::ControlFlow::Break
                }
            }
        });
    }

    /// Step the zoom in, as the pill's plus and Ctrl+plus do.
    pub(super) fn zoom_in(&self) {
        self.zoom_by(ZOOM_STEP);
    }

    /// Step the zoom out, as the pill's minus and Ctrl+minus do.
    pub(super) fn zoom_out(&self) {
        self.zoom_by(1.0 / ZOOM_STEP);
    }

    /// Reserve or release the docked-bar band below the top chrome; the image
    /// refits into the space left, the way the Linux canvas reflows.
    pub(super) fn set_docked(&self, docked: bool) {
        let inset = if docked { DOCKED_BAR_INSET } else { 0.0 };
        if self.docked_inset.replace(inset) == inset {
            return;
        }
        self.area.queue_draw();
    }

    /// Fit crop handles inside the viewport without losing the previous view.
    pub(super) fn set_crop_mode(&self, enabled: bool) {
        if self.crop_mode.replace(enabled) == enabled {
            return;
        }
        let mut state = self.state.borrow_mut();
        if enabled {
            state.saved_crop_view = Some(SavedCropView {
                width: state.width,
                height: state.height,
                manual_zoom: state.manual_zoom,
                pan: state.pan,
            });
            state.manual_zoom = None;
            state.pan = (0.0, 0.0);
        } else if let Some(saved) = state.saved_crop_view.take() {
            if (saved.width, saved.height) == (state.width, state.height) {
                state.manual_zoom = saved.manual_zoom;
                let (_, _, width, height) =
                    content_rect(&self.area, self.scale, self.docked_inset.get(), false);
                let zoom = effective_zoom(&state, width, height, false);
                state.pan = clamp_pan(
                    saved.pan,
                    state.canvas_width,
                    state.canvas_height,
                    zoom,
                    width,
                    height,
                );
            }
        }
        drop(state);
        self.area.queue_draw();
    }

    /// Reset the view to fit the viewport, as Ctrl+0 and the zoom label do.
    pub(super) fn fit(&self) {
        if self.state.borrow().pixbuf.is_none() {
            return;
        }
        self.state.borrow_mut().manual_zoom = None;
        self.state.borrow_mut().pan = (0.0, 0.0);
        self.host.changed();
    }

    fn zoom_by(&self, factor: f64) {
        let mut state = self.state.borrow_mut();
        if state.pixbuf.is_none() {
            return;
        }
        let (_, _, avail_width, avail_height) = content_rect(
            &self.area,
            self.scale,
            self.docked_inset.get(),
            self.crop_mode.get(),
        );
        let crop_mode = self.crop_mode.get();
        let current = effective_zoom(&state, avail_width, avail_height, crop_mode);
        let min_zoom = if crop_mode {
            MIN_ZOOM.min(crop_fit_zoom(
                state.canvas_width,
                state.canvas_height,
                avail_width,
                avail_height,
            ))
        } else {
            MIN_ZOOM
        };
        let zoom = (current * factor).clamp(min_zoom, MAX_ZOOM);
        state.manual_zoom = Some(zoom);
        state.pan = clamp_pan(
            state.pan,
            state.canvas_width,
            state.canvas_height,
            zoom,
            avail_width,
            avail_height,
        );
        drop(state);
        self.host.changed();
    }

    fn finish_load(self: &Rc<Self>, decoded: DecodedImage) {
        if decoded.width <= 0 || decoded.height <= 0 {
            self.finish_error(&t("The image is empty."));
            return;
        }

        let stride = decoded.width * 4;
        let bytes = glib::Bytes::from_owned(decoded.bytes);
        let pixbuf = Pixbuf::from_bytes(
            &bytes,
            Colorspace::Rgb,
            true,
            8,
            decoded.width,
            decoded.height,
            stride,
        );

        {
            let mut state = self.state.borrow_mut();
            state.pixbuf = Some(pixbuf);
            state.width = decoded.width;
            state.height = decoded.height;
            state.canvas_width = decoded.width;
            state.canvas_height = decoded.height;
            state.image_x = 0.0;
            state.image_y = 0.0;
            state.has_background = false;
            state.frame_aspect = None;
            state.manual_zoom = None;
            state.pan = (0.0, 0.0);
            state.saved_crop_view = None;
        }

        // Hand the decoded pixels to the annotation document. The pixbuf
        // stays as the "image present" marker for zoom/fit bookkeeping.
        let image = image::RgbaImage::from_raw(
            decoded.width as u32,
            decoded.height as u32,
            bytes.as_ref().to_vec(),
        );
        if let Some(image) = image {
            self.host.load_document(image, &self.area);
        }

        self.empty.set_visible(false);
        self.loading_row.set_visible(false);
        self.spinner.stop();
        self.open_button.set_sensitive(true);
        // The Motion mode opens for any loaded image, like the Linux editor.
        self.motion_button.set_sensitive(true);
        self.motion_button.set_tooltip_text(None);
        self.area.set_cursor_from_name(Some("grab"));
        self.area.queue_draw();
    }

    fn finish_error(self: &Rc<Self>, message: &str) {
        let has_image = self.state.borrow().pixbuf.is_some();
        self.loading_row.set_visible(false);
        self.spinner.stop();
        self.open_button.set_sensitive(true);
        self.empty.set_visible(!has_image);
        self.show_error(message);
    }

    /// Show a short, self-dismissing message over the canvas.
    fn show_error(self: &Rc<Self>, message: &str) {
        eprintln!("[image editor] {message}");
        self.toast.set_text(message);
        self.toast.set_visible(true);

        let token = self.toast_generation.get() + 1;
        self.toast_generation.set(token);
        let canvas = Rc::downgrade(self);
        glib::timeout_add_local_once(TOAST_LIFETIME, move || {
            if let Some(canvas) = canvas.upgrade() {
                if canvas.toast_generation.get() == token {
                    canvas.toast.set_visible(false);
                }
            }
        });
    }

    /// Text-caret blink and the OCR completion poll: both tick on the UI
    /// clock, read the shared document, and redraw only when something moved.
    fn install_model_timers(self: &Rc<Self>) {
        // Caret blink while a text session is live.
        let canvas = Rc::downgrade(self);
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            let Some(canvas) = canvas.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let flipped = canvas.host.editor.borrow_mut().as_mut().map(|st| {
                if let Some(input) = st.active_text_input.as_mut() {
                    input.cursor_visible = !input.cursor_visible;
                    return true;
                }
                false
            });
            // No document yet: stay alive so the next load's caret blinks;
            // only repaint when a visible toggle actually happened.
            match flipped {
                Some(true) => {
                    canvas.area.queue_draw();
                    glib::ControlFlow::Continue
                }
                _ => glib::ControlFlow::Continue,
            }
        });

        // OCR readiness: when detection lands, refresh so the text-aware
        // highlighter can use it; surface a one-time failure.
        let canvas = Rc::downgrade(self);
        let generation = self.host.session_generation.clone();
        let consumed_for = Rc::new(Cell::new((usize::MAX, u64::MAX)));
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
            let Some(canvas) = canvas.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let gen = generation.get();
            let outcome = canvas.host.read(|st| {
                let detector_id = Arc::as_ptr(&st.text_detector) as usize;
                use std::sync::atomic::Ordering;
                let ready = st.text_detection_ready.load(Ordering::Acquire);
                let failed = st
                    .text_detector
                    .lock()
                    .ok()
                    .and_then(|det| match det.status() {
                        crate::capture::editor::text_detect::DetectionStatus::Failed(e) => {
                            Some(e.clone())
                        }
                        _ => None,
                    });
                (ready, failed, detector_id)
            });
            // Failed is checked before ready: the worker marks ready even
            // when detection itself failed, so ready-first would never
            // surface the error. Identity keys on (detector, generation) so
            // a post-crop restart in the same session is still observed.
            match outcome {
                Some((_, Some(error), id)) if consumed_for.get() != (id, gen) => {
                    consumed_for.set((id, gen));
                    canvas.host.report_error(format!(
                        "Text detection is unavailable: {error}. The highlighter falls back to freehand."
                    ));
                }
                Some((true, _, id)) if consumed_for.get() != (id, gen) => {
                    consumed_for.set((id, gen));
                    canvas.refresh_cursor();
                    canvas.area.queue_draw();
                }
                _ => {}
            }
            glib::ControlFlow::Continue
        });
    }

    /// Re-derive the view's image extents from the live document so crop
    /// and undo can't leave painting/hit-testing pointed at stale
    /// dimensions; a real size change drops manual zoom back to fit.
    pub(super) fn sync_view_to_document(&self) {
        let Some((w, h, layout, has_background, frame_aspect)) = self.host.read(|st| {
            let (w, h) = st.working_image.dimensions();
            (
                w,
                h,
                st.background.layout(w, h),
                st.background.has_fill()
                    || st.background.frame_aspect.is_some()
                    || crate::capture::editor::types::frame_needs_canvas(
                        st.background.frame_style,
                        st.background.frame_style.spec().border_thickness,
                    ),
                st.background.frame_aspect,
            )
        }) else {
            return;
        };
        let mut state = self.state.borrow_mut();
        if state.width != w as i32 || state.height != h as i32 {
            state.width = w as i32;
            state.height = h as i32;
            state.manual_zoom = None;
            state.pan = (0.0, 0.0);
        }
        if state.frame_aspect != frame_aspect
            || (state.has_background != has_background && frame_aspect.is_some())
        {
            state.manual_zoom = None;
            state.pan = (0.0, 0.0);
            state.frame_aspect = frame_aspect;
        }
        state.canvas_width = layout.width.ceil() as i32;
        state.canvas_height = layout.height.ceil() as i32;
        state.image_x = layout.image_x;
        state.image_y = layout.image_y;
        state.has_background = has_background;
        drop(state);
        self.area.queue_draw();
    }

    /// Subscribe to document/selection changes (bars, history pill).
    pub(super) fn add_change_listener(&self, listener: Rc<dyn Fn()>) {
        self.listeners.borrow_mut().push(listener);
    }

    /// The view transform the paint pass uses, rebuilt from live layout so
    /// painting and hit testing agree exactly.
    pub(super) fn view_transform(&self) -> crate::capture::editor::types::ViewTransform {
        use crate::capture::editor::types::ViewTransform;
        let state = self.state.borrow();
        let (left, top, avail_width, avail_height) = content_rect(
            &self.area,
            self.scale,
            self.docked_inset.get(),
            self.crop_mode.get(),
        );
        let zoom = effective_zoom(&state, avail_width, avail_height, self.crop_mode.get());
        let draw_width = f64::from(state.canvas_width) * zoom;
        let draw_height = f64::from(state.canvas_height) * zoom;
        let canvas_x = left + (avail_width - draw_width) / 2.0 + state.pan.0;
        let canvas_y = image_y(
            top,
            avail_height,
            f64::from(self.area.height()),
            self.scale,
            self.crop_mode.get(),
            draw_height,
        ) + state.pan.1;
        ViewTransform {
            scale: zoom,
            offset_x: canvas_x + state.image_x * zoom,
            offset_y: canvas_y + state.image_y * zoom,
            has_background: state.has_background,
            canvas_offset_x: canvas_x,
            canvas_offset_y: canvas_y,
            canvas_scale: zoom,
            canvas_width: f64::from(state.canvas_width),
            canvas_height: f64::from(state.canvas_height),
            image_rect_x: state.image_x,
            image_rect_y: state.image_y,
            ..ViewTransform::for_image(f64::from(state.width), f64::from(state.height))
        }
    }

    /// Re-apply the cursor at the pointer's last position, so a tool switch,
    /// a brush-size change or a finished detection shows without motion.
    pub(super) fn refresh_cursor(&self) {
        let Some(point) = self.hover.point.get() else {
            return;
        };
        let transform = self.view_transform();
        match self
            .host
            .read(|st| super::cursor::brush_cursor(st, transform, point))
            .flatten()
        {
            Some(super::cursor::CanvasCursor::Name(name)) => {
                self.area.set_cursor_from_name(Some(name));
            }
            Some(super::cursor::CanvasCursor::Custom(cursor)) => {
                self.area.set_cursor(Some(&cursor));
            }
            // Other tools keep the cursor the motion handler last picked.
            None => {}
        }
    }

    fn install_gestures(self: &Rc<Self>) {
        let pan_to: Rc<dyn Fn(f64, f64)> = {
            let canvas = Rc::downgrade(self);
            Rc::new(move |px, py| {
                let Some(canvas) = canvas.upgrade() else {
                    return;
                };
                let mut state = canvas.state.borrow_mut();
                let (_, _, avail_width, avail_height) = content_rect(
                    &canvas.area,
                    canvas.scale,
                    canvas.docked_inset.get(),
                    canvas.crop_mode.get(),
                );
                let zoom =
                    effective_zoom(&state, avail_width, avail_height, canvas.crop_mode.get());
                state.pan = clamp_pan(
                    (px, py),
                    state.canvas_width,
                    state.canvas_height,
                    zoom,
                    avail_width,
                    avail_height,
                );
                drop(state);
                canvas.host.changed();
            })
        };
        self.host.install(
            &self.area,
            {
                let canvas = Rc::downgrade(self);
                Rc::new(move || {
                    canvas
                        .upgrade()
                        .map(|c| c.view_transform())
                        .unwrap_or_else(|| {
                            crate::capture::editor::types::ViewTransform::for_image(1.0, 1.0)
                        })
                })
            },
            {
                let area = self.area.downgrade();
                Rc::new(move |name| {
                    if let Some(area) = area.upgrade() {
                        area.set_cursor_from_name(name);
                    }
                })
            },
            self.hover.clone(),
            pan_to,
        );

        let scroll = EventControllerScroll::new(EventControllerScrollFlags::BOTH_AXES);
        scroll.connect_scroll({
            let canvas = Rc::downgrade(self);
            move |controller, _delta_x, delta_y| {
                if !controller
                    .current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK)
                {
                    return Propagation::Proceed;
                }
                let Some(canvas) = canvas.upgrade() else {
                    return Propagation::Proceed;
                };
                if canvas.state.borrow().pixbuf.is_none() {
                    return Propagation::Proceed;
                }
                if delta_y < 0.0 {
                    canvas.zoom_by(ZOOM_STEP);
                } else if delta_y > 0.0 {
                    canvas.zoom_by(1.0 / ZOOM_STEP);
                }
                Propagation::Stop
            }
        });
        self.area.add_controller(scroll);

        let drop = DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
        drop.connect_drop({
            let canvas = Rc::downgrade(self);
            move |_, value, _, _| {
                let Ok(file) = value.get::<gio::File>() else {
                    return false;
                };
                let Some(path) = file.path() else {
                    return false;
                };
                let Some(canvas) = canvas.upgrade() else {
                    return false;
                };
                if !is_supported_image(&path) {
                    canvas.show_error(&t(
                        "That file is not a supported image. Choose a PNG, JPEG, or WebP.",
                    ));
                    return false;
                }
                canvas.load(&path);
                true
            }
        });
        self.root.add_controller(drop);
    }

    fn install_zoom_controls(self: &Rc<Self>) {
        self.zoom_out.connect_clicked({
            let canvas = Rc::downgrade(self);
            move |_| {
                if let Some(canvas) = canvas.upgrade() {
                    canvas.zoom_out();
                }
            }
        });
        self.zoom_in.connect_clicked({
            let canvas = Rc::downgrade(self);
            move |_| {
                if let Some(canvas) = canvas.upgrade() {
                    canvas.zoom_in();
                }
            }
        });
        self.zoom_button.connect_clicked({
            let canvas = Rc::downgrade(self);
            move |_| {
                if let Some(canvas) = canvas.upgrade() {
                    canvas.fit();
                }
            }
        });
    }
}

fn build_empty_state(scale: f64) -> (GtkBox, Button, GtkBox, Spinner, Label) {
    // Centred on the whole canvas pane, like the Linux drop zone.
    let root = GtkBox::new(Orientation::Vertical, px(14.0, scale));
    root.set_halign(Align::Center);
    root.set_valign(Align::Center);

    let icon = Image::from_icon_name(
        crate::capture::editor::window::icon_names::custom::IMAGE_ALT_SYMBOLIC,
    );
    icon.set_pixel_size(px(42.0, scale));
    icon.add_css_class("image-editor-empty-icon");
    icon.set_halign(Align::Center);

    let title = Label::new(Some(&t("Open an image to annotate")));
    title.add_css_class("image-editor-empty-title");
    title.set_halign(Align::Center);

    let detail = Label::new(Some(&t(
        "Drop a PNG, JPEG, or WebP here, or choose one from disk.",
    )));
    detail.add_css_class("image-editor-empty-detail");
    detail.set_halign(Align::Center);
    detail.set_wrap(true);
    detail.set_justify(gtk4::Justification::Center);
    detail.set_max_width_chars(46);

    let loading_row = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
    loading_row.set_halign(Align::Center);
    loading_row.set_visible(false);
    let spinner = Spinner::new();
    spinner.set_size_request(px(14.0, scale), px(14.0, scale));
    let status = Label::new(None);
    status.add_css_class("image-editor-empty-loading");
    loading_row.append(&spinner);
    loading_row.append(&status);

    let open_button = Button::with_label(&t("Open Image"));
    open_button.add_css_class("image-editor-primary-button");
    open_button.set_halign(Align::Center);
    open_button.set_margin_top(px(6.0, scale));

    root.append(&icon);
    root.append(&title);
    root.append(&detail);
    root.append(&loading_row);
    root.append(&open_button);

    (root, open_button, loading_row, spinner, status)
}

fn build_zoom_pill(scale: f64) -> (GtkBox, Button, Button, Button) {
    let pill = GtkBox::new(Orientation::Horizontal, 0);
    pill.add_css_class("image-editor-zoom-pill");
    pill.set_halign(Align::Start);
    pill.set_valign(Align::End);
    pill.set_margin_start(px(16.0, scale));
    pill.set_margin_bottom(px(16.0, scale));

    let zoom_out = Button::with_label("−");
    zoom_out.add_css_class("image-editor-zoom-step");
    zoom_out.set_focusable(false);
    zoom_out.set_tooltip_text(Some(&t("Zoom Out")));

    let label = Button::with_label("100%");
    label.add_css_class("image-editor-zoom-label");
    label.set_focusable(false);
    label.set_tooltip_text(Some(&t("Fit to window")));

    let zoom_in = Button::with_label("+");
    zoom_in.add_css_class("image-editor-zoom-step");
    zoom_in.set_focusable(false);
    zoom_in.set_tooltip_text(Some(&t("Zoom In")));

    pill.append(&zoom_out);
    pill.append(&label);
    pill.append(&zoom_in);

    (pill, label, zoom_out, zoom_in)
}

/// The viewport left for the image: below the floating toolbar (plus whatever
/// a docked option bar claims), above the pill.
fn content_rect(
    area: &DrawingArea,
    scale: f64,
    docked_inset: f64,
    crop_mode: bool,
) -> (f64, f64, f64, f64) {
    viewport_rect(
        f64::from(area.width()),
        f64::from(area.height()),
        scale,
        docked_inset,
        crop_mode,
    )
}

fn viewport_rect(
    width: f64,
    height: f64,
    scale: f64,
    docked_inset: f64,
    crop_mode: bool,
) -> (f64, f64, f64, f64) {
    let width = width.max(1.0);
    let height = height.max(1.0);
    let margin = if crop_mode {
        CROP_HANDLE_MARGIN * scale
    } else {
        0.0
    };
    let top = (TOP_CHROME_HEIGHT + docked_inset) * scale + margin;
    let bottom = (if crop_mode {
        CROP_BOTTOM_CHROME
    } else {
        CONTENT_MARGIN
    }) * scale
        + margin;
    (
        margin,
        top,
        (width - 2.0 * margin).max(1.0),
        (height - top - bottom).max(1.0),
    )
}

fn image_y(
    top: f64,
    avail_height: f64,
    canvas_height: f64,
    scale: f64,
    crop_mode: bool,
    draw_height: f64,
) -> f64 {
    if draw_height > avail_height {
        return top + (avail_height - draw_height) / 2.0;
    }
    let bottom = (if crop_mode {
        CROP_BOTTOM_CHROME + CROP_HANDLE_MARGIN
    } else {
        CONTENT_MARGIN
    }) * scale;
    // Keep a stable canvas-centred position unless the tool bar needs the space.
    ((canvas_height - bottom - draw_height) / 2.0).clamp(top, top + avail_height - draw_height)
}

fn fit_zoom(image_width: i32, image_height: i32, avail_width: f64, avail_height: f64) -> f64 {
    if image_width <= 0 || image_height <= 0 {
        return 1.0;
    }
    let zoom = (avail_width / f64::from(image_width)).min(avail_height / f64::from(image_height));
    zoom.clamp(MIN_ZOOM, MAX_ZOOM)
}

fn crop_fit_zoom(image_width: i32, image_height: i32, avail_width: f64, avail_height: f64) -> f64 {
    if image_width <= 0 || image_height <= 0 {
        return 1.0;
    }
    (avail_width / f64::from(image_width))
        .min(avail_height / f64::from(image_height))
        .clamp(0.0001, MAX_ZOOM)
}

fn effective_zoom(state: &ImageState, avail_width: f64, avail_height: f64, crop_mode: bool) -> f64 {
    let crop_fit = crop_fit_zoom(
        state.canvas_width,
        state.canvas_height,
        avail_width,
        avail_height,
    );
    match state.manual_zoom {
        Some(zoom) => zoom.clamp(
            if crop_mode {
                MIN_ZOOM.min(crop_fit)
            } else {
                MIN_ZOOM
            },
            MAX_ZOOM,
        ),
        None if crop_mode => crop_fit,
        None => fit_zoom(
            state.canvas_width,
            state.canvas_height,
            avail_width,
            avail_height,
        ),
    }
}

fn clamp_pan(
    pan: (f64, f64),
    image_width: i32,
    image_height: i32,
    zoom: f64,
    avail_width: f64,
    avail_height: f64,
) -> (f64, f64) {
    let max_x = (f64::from(image_width) * zoom - avail_width).max(0.0) / 2.0;
    let max_y = (f64::from(image_height) * zoom - avail_height).max(0.0) / 2.0;
    (pan.0.clamp(-max_x, max_x), pan.1.clamp(-max_y, max_y))
}

fn zoom_text(zoom: f64) -> String {
    format!("{}%", (zoom * 100.0).round().clamp(1.0, 6400.0) as i32)
}

/// Paint the canvas and return the zoom the image was drawn at.
fn paint(
    area: &DrawingArea,
    context: &Context,
    state: &ImageState,
    options: PaintOptions,
    host: &super::annotate::EditorHost,
) -> Option<f64> {
    let PaintOptions {
        scale,
        dark,
        docked_inset,
        crop_mode,
    } = options;
    let (left, top, avail_width, avail_height) = content_rect(area, scale, docked_inset, crop_mode);

    // The whole canvas gets the transparency checkerboard, image or not, like
    // the Linux canvas.
    draw_canvas_checkerboard(
        context,
        f64::from(area.width()).max(1.0),
        f64::from(area.height()).max(1.0),
        scale,
        dark,
    );

    let pixbuf = state.pixbuf.as_ref()?;
    let zoom = effective_zoom(state, avail_width, avail_height, crop_mode);
    let draw_width = f64::from(state.canvas_width) * zoom;
    let draw_height = f64::from(state.canvas_height) * zoom;
    let x = left + (avail_width - draw_width) / 2.0 + state.pan.0;
    let y = image_y(
        top,
        avail_height,
        f64::from(area.height()),
        scale,
        crop_mode,
        draw_height,
    ) + state.pan.1;

    if host.has_document() {
        let transform = crate::capture::editor::types::ViewTransform {
            scale: zoom,
            offset_x: x + state.image_x * zoom,
            offset_y: y + state.image_y * zoom,
            has_background: state.has_background,
            canvas_offset_x: x,
            canvas_offset_y: y,
            canvas_scale: zoom,
            canvas_width: f64::from(state.canvas_width),
            canvas_height: f64::from(state.canvas_height),
            image_rect_x: state.image_x,
            image_rect_y: state.image_y,
            ..crate::capture::editor::types::ViewTransform::for_image(
                f64::from(state.width),
                f64::from(state.height),
            )
        };
        host.paint_document(context, transform);
    } else {
        let _ = context.save();
        context.translate(x, y);
        context.scale(zoom, zoom);
        context.set_source_pixbuf(pixbuf, 0.0, 0.0);
        context.source().set_filter(gtk4::cairo::Filter::Good);
        let _ = context.paint();
        let _ = context.restore();
    }

    context.set_line_width(1.0);
    if dark {
        context.set_source_rgba(1.0, 1.0, 1.0, 0.14);
    } else {
        context.set_source_rgba(0.067, 0.094, 0.153, 0.16);
    }
    context.rectangle(
        x + 0.5,
        y + 0.5,
        (draw_width - 1.0).max(0.0),
        (draw_height - 1.0).max(0.0),
    );
    let _ = context.stroke();

    Some(zoom)
}

/// The checkerboard the Linux canvas fills its drawing area with: 14px tiles
/// in a repeating surface pattern, light or dark.
pub(super) fn draw_canvas_checkerboard(
    context: &Context,
    width: f64,
    height: f64,
    scale: f64,
    dark: bool,
) {
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let tile = (14.0 * scale).max(1.0);
    let (base, alternate) = if dark {
        ((0.078, 0.078, 0.078), (0.114, 0.114, 0.114))
    } else {
        ((0.965, 0.969, 0.984), (0.941, 0.945, 0.961))
    };

    let side = (tile * 2.0).ceil().max(2.0) as i32;
    let Ok(surface) = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::Rgb24, side, side)
    else {
        return;
    };
    let Ok(pattern_ctx) = Context::new(&surface) else {
        return;
    };
    pattern_ctx.set_source_rgb(base.0, base.1, base.2);
    let _ = pattern_ctx.paint();
    pattern_ctx.set_source_rgb(alternate.0, alternate.1, alternate.2);
    pattern_ctx.rectangle(0.0, 0.0, tile, tile);
    pattern_ctx.rectangle(tile, tile, tile, tile);
    let _ = pattern_ctx.fill();

    let pattern = gtk4::cairo::SurfacePattern::create(&surface);
    pattern.set_extend(gtk4::cairo::Extend::Repeat);
    let _ = context.set_source(&pattern);
    context.rectangle(0.0, 0.0, width, height);
    let _ = context.fill();
}

fn decode_image(path: &Path) -> Result<DecodedImage, String> {
    let decoded = image::open(path)
        .map_err(|error| error.to_string())?
        .to_rgba8();
    let (width, height) = decoded.dimensions();
    Ok(DecodedImage {
        bytes: decoded.into_raw(),
        width: width as i32,
        height: height as i32,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_pan, crop_fit_zoom, fit_zoom, image_y, viewport_rect, zoom_text, CROP_BOTTOM_CHROME,
        CROP_HANDLE_MARGIN, DOCKED_BAR_INSET, MAX_ZOOM, MIN_ZOOM,
    };

    #[test]
    fn fit_zoom_uses_the_tighter_axis() {
        assert!((fit_zoom(1000, 500, 500.0, 500.0) - 0.5).abs() < 1e-9);
        assert!((fit_zoom(500, 1000, 500.0, 500.0) - 0.5).abs() < 1e-9);
        assert!((fit_zoom(1000, 1000, 800.0, 400.0) - 0.4).abs() < 1e-9);
    }

    #[test]
    fn fit_zoom_is_clamped_and_handles_empty_images() {
        assert_eq!(fit_zoom(0, 0, 500.0, 500.0), 1.0);
        assert_eq!(fit_zoom(10_000_000, 10_000_000, 1.0, 1.0), MIN_ZOOM);
        assert_eq!(fit_zoom(1, 1, 10_000.0, 10_000.0), MAX_ZOOM);
    }

    #[test]
    fn pan_is_clamped_to_the_hidden_image_edges() {
        // Fitted image: nothing to pan.
        assert_eq!(
            clamp_pan((40.0, -40.0), 1000, 500, 0.5, 500.0, 250.0),
            (0.0, 0.0)
        );
        // 2x image in a 500x250 viewport: 1000px of width can travel 250px.
        assert_eq!(
            clamp_pan((900.0, -900.0), 1000, 500, 1.0, 500.0, 250.0),
            (250.0, -125.0)
        );
    }

    #[test]
    fn zoom_text_rounds_and_floors_at_one_percent() {
        assert_eq!(zoom_text(1.0), "100%");
        assert_eq!(zoom_text(0.333), "33%");
        assert_eq!(zoom_text(0.0), "1%");
    }

    #[test]
    fn opening_a_bar_does_not_push_a_centered_landscape_image_down() {
        let (_, top, _, height) = viewport_rect(1168.0, 940.0, 1.25, 0.0, false);
        let (_, docked_top, _, docked_height) =
            viewport_rect(1168.0, 940.0, 1.25, DOCKED_BAR_INSET, false);
        let initial = image_y(top, height, 940.0, 1.25, false, 610.0);
        let docked = image_y(docked_top, docked_height, 940.0, 1.25, false, 610.0);
        assert_eq!(docked, initial);
        assert!(docked - docked_top < 50.0);

        let tall_image = docked_height;
        assert_eq!(
            image_y(docked_top, docked_height, 940.0, 1.25, false, tall_image),
            docked_top
        );
    }

    #[test]
    fn crop_fit_leaves_handle_and_bottom_chrome_clearance() {
        for scale in [1.0, 1.25, 2.0] {
            let (left, top, width, height) =
                viewport_rect(1000.0, 800.0, scale, DOCKED_BAR_INSET, true);
            for (image_width, image_height) in [(2000, 600), (600, 2000), (2000, 2000)] {
                let zoom = crop_fit_zoom(image_width, image_height, width, height);
                let draw_w = f64::from(image_width) * zoom;
                let draw_h = f64::from(image_height) * zoom;
                let x = left + (width - draw_w) / 2.0;
                let y = image_y(top, height, 800.0, scale, true, draw_h);
                assert!(x >= CROP_HANDLE_MARGIN * scale - 1e-9);
                assert!(x + draw_w <= 1000.0 - CROP_HANDLE_MARGIN * scale + 1e-9);
                assert!(y >= top - 1e-9);
                assert!(
                    y + draw_h <= 800.0 - (CROP_BOTTOM_CHROME + CROP_HANDLE_MARGIN) * scale + 1e-9
                );
            }
        }
        assert!(crop_fit_zoom(100_000, 100_000, 800.0, 600.0) < MIN_ZOOM);
    }

    #[test]
    fn entering_crop_temporarily_fits_and_restores_the_previous_view() {
        let Some(()) = crate::test_support::with_gtk(|| {
            use gtk4::prelude::*;
            let canvas = super::build(1.0, true);
            canvas.area.measure(gtk4::Orientation::Horizontal, -1);
            canvas.area.measure(gtk4::Orientation::Vertical, 800);
            canvas.area.allocate(800, 600, -1, None);
            {
                let mut st = canvas.state.borrow_mut();
                st.width = 1000;
                st.height = 800;
                st.canvas_width = 1000;
                st.canvas_height = 800;
                st.manual_zoom = Some(1.0);
                st.pan = (10.0, 20.0);
            }
            canvas.set_docked(true);
            canvas.set_crop_mode(true);
            {
                let st = canvas.state.borrow();
                assert_eq!(st.manual_zoom, None);
                assert_eq!(st.pan, (0.0, 0.0));
            }
            canvas.set_docked(false);
            canvas.set_crop_mode(false);
            {
                let st = canvas.state.borrow();
                assert_eq!(st.manual_zoom, Some(1.0));
                assert_eq!(st.pan, (10.0, 20.0));
            }
            canvas.set_crop_mode(true);
            canvas.state.borrow_mut().width = 800;
            canvas.set_crop_mode(false);
            let st = canvas.state.borrow();
            assert_eq!(st.manual_zoom, None);
            assert_eq!(st.pan, (0.0, 0.0));
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn changing_frame_ratio_refits_without_resetting_on_padding_changes() {
        let Some(()) = crate::test_support::with_gtk(|| {
            use crate::capture::editor::state::background::Fill;
            use crate::capture::editor::state::EditorState;
            use image::{Rgba, RgbaImage};
            use std::sync::Arc;
            let canvas = super::build(1.0, true);
            let mut document = EditorState::new(RgbaImage::new(1000, 500));
            document.background.fill =
                Fill::Image(Arc::new(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]))));
            *canvas.host.editor.borrow_mut() = Some(document);
            canvas.sync_view_to_document();
            canvas.state.borrow_mut().manual_zoom = Some(0.68);
            canvas.host.mutate(|st| st.background.padding = 10.0);
            canvas.sync_view_to_document();
            assert_eq!(canvas.state.borrow().manual_zoom, Some(0.68));
            canvas.host.mutate(|st| {
                st.background.frame_aspect = Some(
                    crate::capture::editor::state::background::FrameRatio::new(10, 21),
                )
            });
            canvas.sync_view_to_document();
            let state = canvas.state.borrow();
            assert_eq!(state.manual_zoom, None);
            assert_eq!(state.pan, (0.0, 0.0));
            assert!(state.canvas_height > state.canvas_width);
            let zoom = super::effective_zoom(&state, 1000.0, 600.0, false);
            assert!(f64::from(state.canvas_height) * zoom <= 600.0 + 1e-6);
        }) else {
            return;
        };
    }
}
