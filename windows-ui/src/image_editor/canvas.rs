//! The editor's display canvas.
//!
//! The canvas owns the checkerboard, the decoded image, the empty drop state,
//! the zoom pill and the pan/zoom gestures. Annotation rendering and editing
//! arrive in their own slices; this one puts a screenshot on screen at the
//! right scale and keeps it there.

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
use std::sync::mpsc;
use std::time::Duration;

use super::ui_support::{is_supported_image, px, TOP_CHROME_HEIGHT};
use crate::i18n::{t, tfmt};

const ZOOM_STEP: f64 = 1.15;
const MIN_ZOOM: f64 = 0.05;
const MAX_ZOOM: f64 = 16.0;
/// Breathing room below the top chrome and above the zoom pill.
const CONTENT_MARGIN: f64 = 16.0;
const TOAST_LIFETIME: Duration = Duration::from_millis(4200);

struct ImageState {
    pixbuf: Option<Pixbuf>,
    width: i32,
    height: i32,
    /// `None` keeps the image fitted while the window resizes.
    manual_zoom: Option<f64>,
    pan: (f64, f64),
}

impl Default for ImageState {
    fn default() -> Self {
        Self {
            pixbuf: None,
            width: 0,
            height: 0,
            manual_zoom: None,
            pan: (0.0, 0.0),
        }
    }
}

struct DecodedImage {
    bytes: Vec<u8>,
    width: i32,
    height: i32,
}

pub(super) struct Canvas {
    pub root: Overlay,
    pub area: DrawingArea,
    pub open_button: Button,
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
    /// Bumped for each open so a late decode cannot replace a newer image.
    generation: Rc<Cell<u64>>,
    toast_generation: Rc<Cell<u64>>,
}

pub(super) fn build(scale: f64, dark: bool) -> Rc<Canvas> {
    let state = Rc::new(RefCell::new(ImageState::default()));
    let docked_inset = Rc::new(Cell::new(0.0));
    let last_zoom_text = Rc::new(RefCell::new(String::new()));

    let (empty, open_button, loading_row, spinner, status) = build_empty_state(scale);
    let (zoom_pill, zoom_button, zoom_out, zoom_in) = build_zoom_pill(scale);

    let area = DrawingArea::new();
    area.add_css_class("image-editor-canvas");
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_draw_func({
        let state = state.clone();
        let docked_inset = docked_inset.clone();
        let last_zoom_text = last_zoom_text.clone();
        let zoom_button = zoom_button.downgrade();
        move |area, context, _width, _height| {
            let Some(zoom) = paint(
                area,
                context,
                &state.borrow(),
                scale,
                dark,
                docked_inset.get(),
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

    let toast = Label::new(None);
    toast.add_css_class("image-editor-toast");
    toast.set_halign(Align::Center);
    toast.set_valign(Align::End);
    toast.set_margin_bottom(px(20.0, scale));
    toast.set_wrap(true);
    toast.set_max_width_chars(60);
    toast.set_visible(false);

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
        generation: Rc::new(Cell::new(0)),
        toast_generation: Rc::new(Cell::new(0)),
    });

    canvas.install_gestures();
    canvas.install_zoom_controls();
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
        let inset = if docked { 56.0 } else { 0.0 };
        if self.docked_inset.replace(inset) == inset {
            return;
        }
        self.area.queue_draw();
    }

    /// Reset the view to fit the viewport, as Ctrl+0 and the zoom label do.
    pub(super) fn fit(&self) {
        if self.state.borrow().pixbuf.is_none() {
            return;
        }
        self.state.borrow_mut().manual_zoom = None;
        self.state.borrow_mut().pan = (0.0, 0.0);
        self.area.queue_draw();
    }

    fn zoom_by(&self, factor: f64) {
        let mut state = self.state.borrow_mut();
        if state.pixbuf.is_none() {
            return;
        }
        let (_, _, avail_width, avail_height) =
            content_rect(&self.area, self.scale, self.docked_inset.get());
        let current = effective_zoom(&state, avail_width, avail_height);
        let zoom = (current * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        state.manual_zoom = Some(zoom);
        state.pan = clamp_pan(
            state.pan,
            state.width,
            state.height,
            zoom,
            avail_width,
            avail_height,
        );
        drop(state);
        self.area.queue_draw();
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
            state.manual_zoom = None;
            state.pan = (0.0, 0.0);
        }

        self.empty.set_visible(false);
        self.loading_row.set_visible(false);
        self.spinner.stop();
        self.open_button.set_sensitive(true);
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

    fn install_gestures(self: &Rc<Self>) {
        let drag = gtk4::GestureDrag::new();
        let pan_start = Rc::new(Cell::new((0.0, 0.0)));
        drag.connect_drag_begin({
            let canvas = Rc::downgrade(self);
            let pan_start = pan_start.clone();
            move |_, _, _| {
                let Some(canvas) = canvas.upgrade() else {
                    return;
                };
                let state = canvas.state.borrow();
                if state.pixbuf.is_none() {
                    return;
                }
                pan_start.set(state.pan);
                drop(state);
                canvas.area.set_cursor_from_name(Some("grabbing"));
            }
        });
        drag.connect_drag_update({
            let canvas = Rc::downgrade(self);
            let pan_start = pan_start.clone();
            move |_, offset_x, offset_y| {
                let Some(canvas) = canvas.upgrade() else {
                    return;
                };
                let mut state = canvas.state.borrow_mut();
                if state.pixbuf.is_none() {
                    return;
                }
                let (_, _, avail_width, avail_height) =
                    content_rect(&canvas.area, canvas.scale, canvas.docked_inset.get());
                let zoom = effective_zoom(&state, avail_width, avail_height);
                let start = pan_start.get();
                state.pan = clamp_pan(
                    (start.0 + offset_x, start.1 + offset_y),
                    state.width,
                    state.height,
                    zoom,
                    avail_width,
                    avail_height,
                );
                drop(state);
                canvas.area.queue_draw();
            }
        });
        drag.connect_drag_end({
            let canvas = Rc::downgrade(self);
            move |_, _, _| {
                if let Some(canvas) = canvas.upgrade() {
                    let cursor = if canvas.state.borrow().pixbuf.is_some() {
                        "grab"
                    } else {
                        "default"
                    };
                    canvas.area.set_cursor_from_name(Some(cursor));
                }
            }
        });
        self.area.add_controller(drag);

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
fn content_rect(area: &DrawingArea, scale: f64, docked_inset: f64) -> (f64, f64, f64, f64) {
    let width = f64::from(area.width()).max(1.0);
    let height = f64::from(area.height()).max(1.0);
    let top = (TOP_CHROME_HEIGHT + docked_inset) * scale;
    let bottom = CONTENT_MARGIN * scale;
    (0.0, top, width, (height - top - bottom).max(1.0))
}

fn fit_zoom(image_width: i32, image_height: i32, avail_width: f64, avail_height: f64) -> f64 {
    if image_width <= 0 || image_height <= 0 {
        return 1.0;
    }
    let zoom = (avail_width / f64::from(image_width)).min(avail_height / f64::from(image_height));
    zoom.clamp(MIN_ZOOM, MAX_ZOOM)
}

fn effective_zoom(state: &ImageState, avail_width: f64, avail_height: f64) -> f64 {
    match state.manual_zoom {
        Some(zoom) => zoom.clamp(MIN_ZOOM, MAX_ZOOM),
        None => fit_zoom(state.width, state.height, avail_width, avail_height),
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
    scale: f64,
    dark: bool,
    docked_inset: f64,
) -> Option<f64> {
    let (_, top, avail_width, avail_height) = content_rect(area, scale, docked_inset);

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
    let zoom = effective_zoom(state, avail_width, avail_height);
    let draw_width = f64::from(state.width) * zoom;
    let draw_height = f64::from(state.height) * zoom;
    let x = (avail_width - draw_width) / 2.0 + state.pan.0;
    let y = top + (avail_height - draw_height) / 2.0 + state.pan.1;

    let _ = context.save();
    context.translate(x, y);
    context.scale(zoom, zoom);
    context.set_source_pixbuf(pixbuf, 0.0, 0.0);
    context.source().set_filter(gtk4::cairo::Filter::Good);
    let _ = context.paint();
    let _ = context.restore();

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
fn draw_canvas_checkerboard(context: &Context, width: f64, height: f64, scale: f64, dark: bool) {
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
    use super::{clamp_pan, fit_zoom, zoom_text, MAX_ZOOM, MIN_ZOOM};

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
}
