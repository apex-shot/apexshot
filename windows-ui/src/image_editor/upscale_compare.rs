//! The before / after sheet for neural export enhancement.
//!
//! An in-window sheet over the editor, like the mode-switch warning. Left of
//! the divider is the conventional enlargement and right of it the network's,
//! both of the flattened export pixels, so the sheet shows exactly what
//! enhancement changes. Rendering runs on a worker and is cached per mode.

use gtk4::cairo::{Context, Filter, ImageSurface, LineCap, LineJoin};
use gtk4::glib::{self, Propagation};
use gtk4::{
    gdk, prelude::*, Adjustment, Align, Box as GtkBox, Button, DrawingArea, EventControllerKey,
    EventControllerMotion, GestureDrag, Image, Label, Orientation, Overflow, Overlay, PolicyType,
    ScrolledWindow, Spinner, ToggleButton,
};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use crate::capture::editor::render;
use crate::capture::editor::state::EditorState;
use crate::capture::editor::upscale::{self, SourceUpscale};
use crate::i18n::t;

use super::annotate::EditorHost;
use super::ui_support::px;

/// Breathing room around the export inside the stage, at 100% scale.
const STAGE_MARGIN: f64 = 24.0;
/// How close to the divider, in screen pixels at 100%, the resize cursor shows.
const DIVIDER_REACH: f64 = 14.0;
/// Corner radius of the export's frame on the stage, at 100% scale.
const IMAGE_CORNER: f64 = 6.0;

thread_local! {
    static OPEN: Cell<bool> = const { Cell::new(false) };
}

struct Comparison {
    before: ImageSurface,
    after: ImageSurface,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Zoom {
    Fit,
    Actual,
    Double,
}

/// The export's rectangle in drawing-area coordinates.
#[derive(Clone, Copy)]
struct Placement {
    scale: f64,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// What the draw function and the pointer handlers share.
struct Stage {
    shown: RefCell<Option<Rc<Comparison>>>,
    divider: Cell<f64>,
    zoom: Cell<Zoom>,
    mode: Cell<SourceUpscale>,
    ui_scale: f64,
    dark: bool,
    hadjustment: Adjustment,
    vadjustment: Adjustment,
}

impl Stage {
    fn margin(&self) -> f64 {
        STAGE_MARGIN * self.ui_scale
    }

    fn placement(&self, area: &DrawingArea) -> Option<Placement> {
        let shown = self.shown.borrow();
        let comparison = shown.as_deref()?;
        let device = f64::from(area.scale_factor().max(1));
        let width = f64::from(area.width().max(1));
        let height = f64::from(area.height().max(1));
        let image_width = f64::from(comparison.after.width());
        let image_height = f64::from(comparison.after.height());
        let margin = self.margin();
        let scale = match self.zoom.get() {
            // Fit fills the stage, enlarging a small export so the
            // difference is visible without reaching for a zoom.
            Zoom::Fit => ((width - 2.0 * margin) / image_width)
                .min((height - 2.0 * margin) / image_height)
                .max(0.01),
            Zoom::Actual => 1.0 / device,
            Zoom::Double => 2.0 / device,
        };
        let (display_width, display_height) = (image_width * scale, image_height * scale);
        Some(Placement {
            scale,
            x: ((width - display_width) / 2.0).max(0.0).round(),
            y: ((height - display_height) / 2.0).max(0.0).round(),
            width: display_width,
            height: display_height,
        })
    }

    /// The scrolled window's visible part of the drawing area.
    fn visible(&self, area: &DrawingArea) -> (f64, f64, f64, f64) {
        let span = |adjustment: &Adjustment, fallback: i32| {
            let page = adjustment.page_size();
            if page > 0.0 {
                (adjustment.value(), adjustment.value() + page)
            } else {
                (0.0, f64::from(fallback))
            }
        };
        let (left, right) = span(&self.hadjustment, area.width());
        let (top, bottom) = span(&self.vadjustment, area.height());
        (left, top, right, bottom)
    }

    fn divider_at(&self, area: &DrawingArea, x: f64) -> Option<f64> {
        let placement = self.placement(area)?;
        Some(((x - placement.x) / placement.width).clamp(0.0, 1.0))
    }

    fn near_divider(&self, area: &DrawingArea, x: f64) -> bool {
        self.placement(area).is_some_and(|placement| {
            let split = placement.x + self.divider.get() * placement.width;
            (x - split).abs() <= DIVIDER_REACH * self.ui_scale
        })
    }

    fn draw(&self, area: &DrawingArea, context: &Context) {
        let Some(placement) = self.placement(area) else {
            return;
        };
        let shown = self.shown.borrow();
        let Some(comparison) = shown.as_deref() else {
            return;
        };
        let s = self.ui_scale;
        let Placement {
            x,
            y,
            width,
            height,
            ..
        } = placement;

        // The export sits in a softly rounded frame. The grid marks
        // transparency in the export only, so it stays under the image
        // instead of filling the whole stage.
        let corner = (IMAGE_CORNER * s).min(width / 2.0).min(height / 2.0);
        context.save().ok();
        render::rounded_rect_path(context, x, y, width, height, corner);
        context.clip();
        context.save().ok();
        context.translate(x, y);
        render::draw_canvas_checkerboard_background(
            context,
            width.ceil() as i32,
            height.ceil() as i32,
            None,
            !self.dark,
        );
        context.restore().ok();

        let device = f64::from(area.scale_factor().max(1));
        let filter = if placement.scale * device >= 1.0 {
            Filter::Nearest
        } else {
            Filter::Good
        };
        let split = (x + self.divider.get() * width).round();
        paint_half(
            context,
            &comparison.before,
            placement,
            (x, split - x),
            filter,
        );
        paint_half(
            context,
            &comparison.after,
            placement,
            (split, x + width - split),
            filter,
        );

        // A flat white rule with a one-pixel keyline either side, so it
        // reads over light and dark pixels alike. It stays inside the frame.
        let rule = (2.0 * s).round().max(2.0);
        context.set_source_rgba(0.0, 0.0, 0.0, 0.24);
        context.rectangle(split - rule / 2.0 - 1.0, y, rule + 2.0, height);
        context.fill().ok();
        context.set_source_rgb(1.0, 1.0, 1.0);
        context.rectangle(split - rule / 2.0, y, rule, height);
        context.fill().ok();
        context.restore().ok();

        if self.dark {
            context.set_source_rgba(1.0, 1.0, 1.0, 0.10);
        } else {
            context.set_source_rgba(0.07, 0.09, 0.15, 0.14);
        }
        context.set_line_width(1.0);
        render::rounded_rect_path(
            context,
            x - 0.5,
            y - 0.5,
            width + 1.0,
            height + 1.0,
            corner + 0.5,
        );
        context.stroke().ok();

        let (visible_left, visible_top, visible_right, visible_bottom) = self.visible(area);
        let top = y.max(visible_top);
        let bottom = (y + height).min(visible_bottom);

        // The handle: a flat white disc with a hairline edge, no shadow.
        let middle = ((top + bottom) / 2.0).round();
        let radius = 13.0 * s;
        context.set_source_rgb(1.0, 1.0, 1.0);
        context.arc(split, middle, radius, 0.0, std::f64::consts::TAU);
        context.fill_preserve().ok();
        context.set_source_rgba(0.0, 0.0, 0.0, 0.16);
        context.set_line_width(1.0);
        context.stroke().ok();
        // Outward chevrons: the handle moves either way.
        context.set_source_rgb(0.23, 0.25, 0.29);
        context.set_line_width(1.6 * s);
        context.set_line_cap(LineCap::Round);
        context.set_line_join(LineJoin::Round);
        let (gap, reach, rise) = (2.5 * s, 3.5 * s, 4.0 * s);
        context.move_to(split - gap, middle - rise);
        context.line_to(split - gap - reach, middle);
        context.line_to(split - gap, middle + rise);
        context.stroke().ok();
        context.move_to(split + gap, middle - rise);
        context.line_to(split + gap + reach, middle);
        context.line_to(split + gap, middle + rise);
        context.stroke().ok();

        // Each side's tag rides its own half and steps aside when the
        // divider would cover it.
        let inset = 12.0 * s;
        let tag_top = (top + inset).round();
        let left_edge = (x.max(visible_left) + inset).round();
        let right_edge = ((x + width).min(visible_right) - inset).round();
        let before = Tag::new(context, &t("Standard"), s);
        if split - left_edge > before.width + inset {
            before.paint(context, left_edge, tag_top, s);
        }
        let after = Tag::new(context, &mode_label(self.mode.get()), s);
        let after_x = right_edge - after.width;
        if after_x - split > inset {
            after.paint(context, after_x, tag_top, s);
        }
    }
}

fn paint_half(
    context: &Context,
    surface: &ImageSurface,
    placement: Placement,
    (left, width): (f64, f64),
    filter: Filter,
) {
    if width <= 0.0 {
        return;
    }
    context.save().ok();
    context.rectangle(left, placement.y, width, placement.height);
    context.clip();
    context.translate(placement.x, placement.y);
    context.scale(placement.scale, placement.scale);
    context.set_source_surface(surface, 0.0, 0.0).ok();
    context.source().set_filter(filter);
    context.paint().ok();
    context.restore().ok();
}

/// A small label naming one side of the divider, drawn on the stage so it
/// follows the image when it scrolls. Both sides share one neutral style.
struct Tag {
    layout: pango::Layout,
    width: f64,
    height: f64,
}

impl Tag {
    const PAD_X: f64 = 8.0;
    const PAD_Y: f64 = 3.0;

    fn new(context: &Context, text: &str, s: f64) -> Self {
        let layout = pangocairo::functions::create_layout(context);
        let mut font = pango::FontDescription::new();
        font.set_family("Inter,Noto Sans,Segoe UI");
        font.set_weight(pango::Weight::Medium);
        font.set_absolute_size(11.0 * s * f64::from(pango::SCALE));
        layout.set_font_description(Some(&font));
        layout.set_text(text);
        let (text_width, text_height) = layout.pixel_size();
        Self {
            layout,
            width: (f64::from(text_width) + 2.0 * Self::PAD_X * s).round(),
            height: (f64::from(text_height) + 2.0 * Self::PAD_Y * s).round(),
        }
    }

    fn paint(&self, context: &Context, x: f64, y: f64, s: f64) {
        render::rounded_rect_path(context, x, y, self.width, self.height, 5.0 * s);
        context.set_source_rgba(0.07, 0.07, 0.08, 0.78);
        context.fill().ok();
        context.set_source_rgba(1.0, 1.0, 1.0, 0.94);
        context.move_to((x + Self::PAD_X * s).round(), (y + Self::PAD_Y * s).round());
        pangocairo::functions::show_layout(context, &self.layout);
    }
}

fn mode_label(mode: SourceUpscale) -> String {
    t(if mode == SourceUpscale::Neural4x {
        "Neural 4×"
    } else {
        "Neural 2×"
    })
}

fn mode_index(mode: SourceUpscale) -> usize {
    usize::from(mode == SourceUpscale::Neural4x)
}

pub(super) fn show(
    window: &gtk4::ApplicationWindow,
    host: &Rc<EditorHost>,
    scale: f64,
    initial: SourceUpscale,
    on_apply: Rc<dyn Fn(SourceUpscale)>,
) {
    if OPEN.with(Cell::get) {
        return;
    }
    let Some(overlay) = window.child().and_downcast::<Overlay>() else {
        return;
    };
    OPEN.with(|open| open.set(true));
    let dark = crate::settings::windowing::prefers_dark_glass_theme();
    let (source_width, source_height) = host
        .read(|st| st.working_image.dimensions())
        .unwrap_or((0, 0));
    let size_of = move |mode: SourceUpscale| {
        let factor = mode.factor();
        format!("{} × {} px", source_width * factor, source_height * factor)
    };

    let scrim = GtkBox::new(Orientation::Vertical, 0);
    scrim.add_css_class("image-editor-confirm-scrim");
    scrim.add_css_class("image-editor-upscale-scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    scrim.set_can_target(true);

    // The comparison fills the card's left side; the decision lives in a
    // narrow panel on the right.
    let card = GtkBox::new(Orientation::Horizontal, 0);
    card.add_css_class("image-editor-upscale-card");
    card.add_css_class(if dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    });
    card.set_overflow(Overflow::Hidden);
    card.set_can_target(true);
    // A compact sheet: big enough to judge detail, small enough that the
    // editor stays in view around it.
    let width = (f64::from(window.width()) * 0.66)
        .clamp(f64::from(px(680.0, scale)), f64::from(px(940.0, scale)));
    let height = (f64::from(window.height()) * 0.70)
        .clamp(f64::from(px(440.0, scale)), f64::from(px(600.0, scale)));
    card.set_size_request(width as i32, height as i32);

    // Stage: the export, filling everything left of the panel.
    let drawing = DrawingArea::new();
    drawing.set_hexpand(true);
    drawing.set_vexpand(true);
    let scroll = ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .child(&drawing)
        .build();
    scroll.add_css_class("image-editor-upscale-scroll");
    let spinner = Spinner::new();
    let message = Label::new(None);
    message.add_css_class("image-editor-upscale-message");
    message.set_wrap(true);
    message.set_max_width_chars(52);
    message.set_justify(gtk4::Justification::Center);
    let status = GtkBox::new(Orientation::Vertical, px(12.0, scale));
    status.set_halign(Align::Center);
    status.set_valign(Align::Center);
    status.append(&spinner);
    status.append(&message);
    let stage_overlay = Overlay::new();
    stage_overlay.set_child(Some(&scroll));
    stage_overlay.add_overlay(&status);
    let stage_box = GtkBox::new(Orientation::Vertical, 0);
    stage_box.add_css_class("image-editor-upscale-stage");
    stage_box.set_hexpand(true);
    stage_box.set_vexpand(true);
    stage_box.set_overflow(Overflow::Hidden);
    stage_box.append(&stage_overlay);
    stage_overlay.set_vexpand(true);

    // Panel: title, output size, zoom, then the decision at the bottom.
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("image-editor-upscale-panel");
    panel.set_size_request(px(272.0, scale), -1);
    panel.set_hexpand(false);

    let head = GtkBox::new(Orientation::Horizontal, px(8.0, scale));
    let titles = GtkBox::new(Orientation::Vertical, px(4.0, scale));
    titles.set_hexpand(true);
    let title = Label::new(Some(&t("Compare enhancement")));
    title.add_css_class("image-editor-upscale-title");
    title.set_xalign(0.0);
    let hint = Label::new(Some(&t("Drag the divider across the image.")));
    hint.add_css_class("image-editor-upscale-hint");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    titles.append(&title);
    titles.append(&hint);
    let close_button = Button::new();
    close_button.add_css_class("image-editor-upscale-close");
    close_button.set_has_frame(false);
    close_button.set_focusable(false);
    close_button.set_valign(Align::Start);
    close_button.set_tooltip_text(Some(&t("Close")));
    let close_icon = Image::from_icon_name("window-close-symbolic");
    close_icon.set_pixel_size(px(14.0, scale));
    close_button.set_child(Some(&close_icon));
    head.append(&titles);
    head.append(&close_button);
    panel.append(&head);

    panel.append(&eyebrow(&t("Output size"), scale));
    let options = GtkBox::new(Orientation::Vertical, px(8.0, scale));
    let mode_tabs = [
        option(&t("Neural 2×"), &size_of(SourceUpscale::Neural2x), scale),
        option(&t("Neural 4×"), &size_of(SourceUpscale::Neural4x), scale),
    ];
    mode_tabs[1].set_group(Some(&mode_tabs[0]));
    mode_tabs[mode_index(initial)].set_active(true);
    if let Some(reason) = host
        .read(|st| st.upscale_unavailable_reason(SourceUpscale::Neural4x))
        .flatten()
    {
        mode_tabs[1].set_sensitive(false);
        mode_tabs[1].set_tooltip_text(Some(&t(&reason)));
    }
    for tab in &mode_tabs {
        options.append(tab);
    }
    panel.append(&options);

    let source_row = GtkBox::new(Orientation::Horizontal, px(8.0, scale));
    source_row.add_css_class("image-editor-upscale-source");
    let source_name = Label::new(Some(&t("Source")));
    source_name.add_css_class("image-editor-upscale-source-name");
    source_name.set_xalign(0.0);
    source_name.set_hexpand(true);
    let source_value = Label::new(Some(&format!("{source_width} × {source_height} px")));
    source_value.add_css_class("image-editor-upscale-source-value");
    source_row.append(&source_name);
    source_row.append(&source_value);
    panel.append(&source_row);

    panel.append(&eyebrow(&t("Zoom"), scale));
    let (zooms, zoom_tabs) = segmented(&["Fit", "100%", "200%"], 0);
    panel.append(&zooms);

    let filler = GtkBox::new(Orientation::Vertical, 0);
    filler.set_vexpand(true);
    panel.append(&filler);

    if !crate::studio_entitlement::studio_unlocked() {
        let note = Label::new(Some(&t(
            "Free preview. Exporting with enhancement needs Studio.",
        )));
        note.add_css_class("image-editor-upscale-note");
        note.set_xalign(0.0);
        note.set_wrap(true);
        panel.append(&note);
    }

    let cancel = Button::with_label(&t("Cancel"));
    cancel.add_css_class("image-editor-confirm-cancel");
    cancel.set_has_frame(false);
    cancel.set_hexpand(true);
    let apply = Button::with_label(&t("Use Neural 2×"));
    apply.add_css_class("image-editor-confirm-continue");
    apply.set_has_frame(false);
    apply.set_hexpand(true);
    apply.set_sensitive(false);
    let actions = GtkBox::new(Orientation::Horizontal, px(8.0, scale));
    actions.set_homogeneous(true);
    actions.append(&cancel);
    actions.append(&apply);
    panel.append(&actions);

    card.append(&stage_box);
    card.append(&panel);

    // Equal gaps on every side keep the card at its requested size in the
    // middle of the window.
    let top_gap = GtkBox::new(Orientation::Vertical, 0);
    top_gap.set_vexpand(true);
    let bottom_gap = GtkBox::new(Orientation::Vertical, 0);
    bottom_gap.set_vexpand(true);
    let row = GtkBox::new(Orientation::Horizontal, 0);
    let left_gap = GtkBox::new(Orientation::Horizontal, 0);
    left_gap.set_hexpand(true);
    let right_gap = GtkBox::new(Orientation::Horizontal, 0);
    right_gap.set_hexpand(true);
    row.append(&left_gap);
    row.append(&card);
    row.append(&right_gap);
    scrim.append(&top_gap);
    scrim.append(&row);
    scrim.append(&bottom_gap);

    let stage = Rc::new(Stage {
        shown: RefCell::new(None),
        divider: Cell::new(0.5),
        zoom: Cell::new(Zoom::Fit),
        mode: Cell::new(initial),
        ui_scale: scale,
        dark,
        hadjustment: scroll.hadjustment(),
        vadjustment: scroll.vadjustment(),
    });
    let cache: Rc<RefCell<[Option<Rc<Comparison>>; 2]>> = Rc::new(RefCell::new([None, None]));
    let generation = Rc::new(Cell::new(0u64));
    let closed = Rc::new(Cell::new(false));

    drawing.set_draw_func({
        let stage = stage.clone();
        move |area, context, _, _| stage.draw(area, context)
    });
    for adjustment in [scroll.hadjustment(), scroll.vadjustment()] {
        let drawing = drawing.downgrade();
        adjustment.connect_value_changed(move |_| {
            if let Some(drawing) = drawing.upgrade() {
                drawing.queue_draw();
            }
        });
    }

    // Press anywhere on the stage to put the divider there, then drag it.
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let stage = stage.clone();
        move |gesture, x, _| {
            let Some(area) = gesture.widget().and_downcast::<DrawingArea>() else {
                return;
            };
            if let Some(value) = stage.divider_at(&area, x) {
                stage.divider.set(value);
                area.queue_draw();
            }
        }
    });
    drag.connect_drag_update({
        let stage = stage.clone();
        move |gesture, offset_x, _| {
            let Some(area) = gesture.widget().and_downcast::<DrawingArea>() else {
                return;
            };
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            if let Some(value) = stage.divider_at(&area, start_x + offset_x) {
                stage.divider.set(value);
                area.queue_draw();
            }
        }
    });
    drawing.add_controller(drag);

    let hover = EventControllerMotion::new();
    hover.connect_motion({
        let stage = stage.clone();
        move |controller, x, _| {
            let Some(area) = controller.widget().and_downcast::<DrawingArea>() else {
                return;
            };
            let cursor = stage
                .near_divider(&area, x)
                .then(|| gdk::Cursor::from_name("ew-resize", None))
                .flatten();
            area.set_cursor(cursor.as_ref());
        }
    });
    hover.connect_leave(|controller| {
        if let Some(area) = controller.widget() {
            area.set_cursor(None);
        }
    });
    drawing.add_controller(hover);

    for (index, tab) in zoom_tabs.iter().enumerate() {
        tab.connect_toggled({
            let stage = stage.clone();
            let drawing = drawing.clone();
            move |button| {
                if !button.is_active() {
                    return;
                }
                stage.zoom.set(match index {
                    1 => Zoom::Actual,
                    2 => Zoom::Double,
                    _ => Zoom::Fit,
                });
                resize_canvas(&drawing, &stage);
            }
        });
    }

    let present: Rc<dyn Fn(Rc<Comparison>)> = {
        let stage = stage.clone();
        let spinner = spinner.clone();
        let status = status.clone();
        let apply = apply.clone();
        let drawing = drawing.clone();
        Rc::new(move |comparison| {
            *stage.shown.borrow_mut() = Some(comparison);
            spinner.stop();
            status.set_visible(false);
            apply.set_sensitive(true);
            resize_canvas(&drawing, &stage);
        })
    };

    let fail: Rc<dyn Fn(String)> = {
        let spinner = spinner.clone();
        let message = message.clone();
        Rc::new(move |text| {
            spinner.stop();
            spinner.set_visible(false);
            message.set_text(&text);
        })
    };

    let request: Rc<dyn Fn()> = {
        let host = host.clone();
        let stage = stage.clone();
        let status = status.clone();
        let message = message.clone();
        let spinner = spinner.clone();
        let apply = apply.clone();
        let drawing = drawing.clone();
        let cache = cache.clone();
        let generation = generation.clone();
        let closed = closed.clone();
        let present = present.clone();
        let fail = fail.clone();
        Rc::new(move || {
            let selected = stage.mode.get();
            let index = mode_index(selected);
            apply.set_label(&t(if index == 1 {
                "Use Neural 4×"
            } else {
                "Use Neural 2×"
            }));
            generation.set(generation.get() + 1);
            let cached = cache.borrow()[index].clone();
            if let Some(comparison) = cached {
                present(comparison);
                return;
            }
            *stage.shown.borrow_mut() = None;
            resize_canvas(&drawing, &stage);
            apply.set_sensitive(false);
            status.set_visible(true);
            spinner.set_visible(true);
            spinner.start();
            message.set_text(&t("Preparing before / after preview…"));
            if let Some(reason) = host
                .read(|st| st.upscale_unavailable_reason(selected))
                .flatten()
            {
                fail(t(&reason));
                return;
            }
            let Some((base, actions)) = host.read(|st| st.export_source()) else {
                fail(t("Load an image before previewing upscaling."));
                return;
            };
            let id = generation.get();
            let (sender, receiver) = mpsc::channel::<Result<(RgbaImage, Arc<RgbaImage>), String>>();
            std::thread::spawn(move || {
                let source = EditorState::flatten_effects(&base, &actions);
                let before = upscale::standard_enlarge(&source, selected);
                let result =
                    upscale::enhance_cached(&source, selected).map(|after| (before, after));
                let _ = sender.send(result);
            });
            let cache = cache.clone();
            let generation = generation.clone();
            let closed = closed.clone();
            let present = present.clone();
            let fail = fail.clone();
            glib::timeout_add_local(Duration::from_millis(16), move || {
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Err("The upscaling preview worker stopped unexpectedly.".into())
                    }
                };
                if closed.get() {
                    return glib::ControlFlow::Break;
                }
                let comparison = result.and_then(|(before, after)| {
                    match (
                        render::rgba_image_to_surface(&before),
                        render::rgba_image_to_surface(&after),
                    ) {
                        (Some(before), Some(after)) => Ok(Rc::new(Comparison { before, after })),
                        _ => Err("Could not prepare the comparison image.".into()),
                    }
                });
                // A finished render is kept for its mode even when the user
                // has since switched, so switching back is instant.
                if let Ok(comparison) = &comparison {
                    cache.borrow_mut()[index] = Some(comparison.clone());
                }
                if generation.get() == id {
                    match comparison {
                        Ok(comparison) => present(comparison),
                        Err(error) => fail(t(&error)),
                    }
                }
                glib::ControlFlow::Break
            });
        })
    };

    for (index, tab) in mode_tabs.iter().enumerate() {
        tab.connect_toggled({
            let request = request.clone();
            let stage = stage.clone();
            move |button| {
                if !button.is_active() {
                    return;
                }
                stage.mode.set(if index == 1 {
                    SourceUpscale::Neural4x
                } else {
                    SourceUpscale::Neural2x
                });
                request();
            }
        });
    }

    let keys = EventControllerKey::new();
    keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let close: Rc<dyn Fn()> = {
        let scrim = scrim.downgrade();
        let overlay = overlay.downgrade();
        let window = window.downgrade();
        let keys = keys.downgrade();
        let closed = closed.clone();
        Rc::new(move || {
            closed.set(true);
            OPEN.with(|open| open.set(false));
            if let (Some(scrim), Some(overlay)) = (scrim.upgrade(), overlay.upgrade()) {
                overlay.remove_overlay(&scrim);
            }
            if let (Some(window), Some(keys)) = (window.upgrade(), keys.upgrade()) {
                window.remove_controller(&keys);
            }
        })
    };
    keys.connect_key_pressed({
        let close = close.clone();
        move |_, key, _, _| {
            if key == gdk::Key::Escape {
                close();
                return Propagation::Stop;
            }
            Propagation::Proceed
        }
    });
    window.add_controller(keys);

    close_button.connect_clicked({
        let close = close.clone();
        move |_| close()
    });
    cancel.connect_clicked({
        let close = close.clone();
        move |_| close()
    });
    apply.connect_clicked({
        let stage = stage.clone();
        move |_| {
            on_apply(stage.mode.get());
            close();
        }
    });

    overlay.add_overlay(&scrim);
    request();
}

/// A small section heading in the panel.
fn eyebrow(text: &str, scale: f64) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class("image-editor-upscale-eyebrow");
    label.set_xalign(0.0);
    label.set_margin_top(px(22.0, scale));
    label.set_margin_bottom(px(8.0, scale));
    label
}

/// One output size: a radio mark, the mode's name, and the size it exports at.
fn option(name: &str, detail: &str, scale: f64) -> ToggleButton {
    let button = ToggleButton::new();
    button.add_css_class("image-editor-upscale-option");
    button.set_has_frame(false);
    button.set_focusable(false);
    let row = GtkBox::new(Orientation::Horizontal, px(12.0, scale));
    let mark = GtkBox::new(Orientation::Horizontal, 0);
    mark.add_css_class("image-editor-upscale-radio");
    mark.set_valign(Align::Center);
    mark.set_halign(Align::Center);
    let texts = GtkBox::new(Orientation::Vertical, px(2.0, scale));
    let title = Label::new(Some(name));
    title.add_css_class("image-editor-upscale-option-title");
    title.set_xalign(0.0);
    let size = Label::new(Some(detail));
    size.add_css_class("image-editor-upscale-option-detail");
    size.set_xalign(0.0);
    texts.append(&title);
    texts.append(&size);
    row.append(&mark);
    row.append(&texts);
    button.set_child(Some(&row));
    button
}

/// A compact segmented control: grouped toggles in one tray.
fn segmented(labels: &[&str], active: usize) -> (GtkBox, Vec<ToggleButton>) {
    let tray = GtkBox::new(Orientation::Horizontal, 0);
    tray.add_css_class("image-editor-upscale-seg");
    tray.set_homogeneous(true);
    tray.set_valign(Align::Center);
    let mut tabs: Vec<ToggleButton> = Vec::new();
    for label in labels {
        let tab = ToggleButton::with_label(&t(label));
        tab.add_css_class("image-editor-upscale-seg-button");
        tab.set_has_frame(false);
        tab.set_focusable(false);
        if let Some(first) = tabs.first() {
            tab.set_group(Some(first));
        }
        tray.append(&tab);
        tabs.push(tab);
    }
    if let Some(tab) = tabs.get(active) {
        tab.set_active(true);
    }
    (tray, tabs)
}

/// Fit leaves the area to the stage; the pixel zooms give it the export's
/// display size plus the margin, so the scrolled window can pan it.
fn resize_canvas(area: &DrawingArea, stage: &Stage) {
    let device = f64::from(area.scale_factor().max(1));
    let factor = match stage.zoom.get() {
        Zoom::Fit => None,
        Zoom::Actual => Some(1.0 / device),
        Zoom::Double => Some(2.0 / device),
    };
    let margin = 2.0 * stage.margin();
    let size = factor.and_then(|factor| {
        stage.shown.borrow().as_deref().map(|comparison| {
            (
                (f64::from(comparison.after.width()) * factor + margin).ceil() as i32,
                (f64::from(comparison.after.height()) * factor + margin).ceil() as i32,
            )
        })
    });
    let (width, height) = size.unwrap_or((0, 0));
    area.set_content_width(width);
    area.set_content_height(height);
    area.queue_draw();
}
