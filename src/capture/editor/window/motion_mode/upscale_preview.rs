//! The before / after sheet for neural export enhancement.
//!
//! An in-window sheet over the editor. Left of the divider is the standard
//! enlargement and right of it the network's, both of the rendered export
//! pixels, so the sheet shows exactly what enhancement changes. Rendering
//! runs on a worker and is kept per mode, so switching back is instant.

use gtk4::cairo::{Context, Filter, Format, ImageSurface, LineCap, LineJoin};
use gtk4::glib::Propagation;
use gtk4::{
    gdk, gio, glib, prelude::*, Adjustment, Align, Box as GtkBox, Button, DrawingArea,
    EventControllerKey, EventControllerMotion, GestureDrag, Image, Label, Orientation, Overflow,
    Overlay, PolicyType, ScrolledWindow, Spinner, ToggleButton,
};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::super::motion_render;
use super::MotionSession;
use crate::capture::editor::{render, state::EditorState, upscale::SourceUpscale};
use crate::i18n::{t, tfmt};
use crate::recording::editor::model::{MotionBackgroundFillType, MotionState};

/// Breathing room around the export inside the stage.
const STAGE_MARGIN: f64 = 24.0;
/// How close to the divider, in screen pixels, the resize cursor shows.
const DIVIDER_REACH: f64 = 14.0;
/// Corner radius of the export's frame on the stage.
const IMAGE_CORNER: f64 = 6.0;
/// A preview projected to take longer than this says so, with the estimate,
/// rather than leaving a bare spinner up for minutes.
const LONG_PREVIEW: Duration = Duration::from_secs(20);
/// How often the stage's progress line is refreshed while enhancing.
const PROGRESS_TICK: Duration = Duration::from_millis(250);

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
    dark: bool,
    hadjustment: Adjustment,
    vadjustment: Adjustment,
}

impl Stage {
    fn placement(&self, area: &DrawingArea) -> Option<Placement> {
        let shown = self.shown.borrow();
        let comparison = shown.as_deref()?;
        let device = f64::from(area.scale_factor().max(1));
        let width = f64::from(area.width().max(1));
        let height = f64::from(area.height().max(1));
        let image_width = f64::from(comparison.after.width());
        let image_height = f64::from(comparison.after.height());
        let scale = match self.zoom.get() {
            // Fit fills the stage, enlarging a small export so the
            // difference is visible without reaching for a zoom.
            Zoom::Fit => ((width - 2.0 * STAGE_MARGIN) / image_width)
                .min((height - 2.0 * STAGE_MARGIN) / image_height)
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

    /// The seam is a fraction of the rendered export, not of the whole area,
    /// so the handle never jumps when the area is wider than the export.
    fn divider_at(&self, area: &DrawingArea, x: f64) -> Option<f64> {
        let placement = self.placement(area)?;
        Some(((x - placement.x) / placement.width).clamp(0.0, 1.0))
    }

    fn near_divider(&self, area: &DrawingArea, x: f64) -> bool {
        self.placement(area).is_some_and(|placement| {
            let split = placement.x + self.divider.get() * placement.width;
            (x - split).abs() <= DIVIDER_REACH
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
        let corner = IMAGE_CORNER.min(width / 2.0).min(height / 2.0);
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
        let rule = 2.0;
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
        context.set_source_rgb(1.0, 1.0, 1.0);
        context.arc(split, middle, 13.0, 0.0, std::f64::consts::TAU);
        context.fill_preserve().ok();
        context.set_source_rgba(0.0, 0.0, 0.0, 0.16);
        context.set_line_width(1.0);
        context.stroke().ok();
        // Outward chevrons: the handle moves either way.
        context.set_source_rgb(0.23, 0.25, 0.29);
        context.set_line_width(1.6);
        context.set_line_cap(LineCap::Round);
        context.set_line_join(LineJoin::Round);
        let (gap, reach, rise) = (2.5, 3.5, 4.0);
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
        let inset = 12.0;
        let tag_top = (top + inset).round();
        let left_edge = (x.max(visible_left) + inset).round();
        let right_edge = ((x + width).min(visible_right) - inset).round();
        let before = Tag::new(context, &t("Standard"));
        if split - left_edge > before.width + inset {
            before.paint(context, left_edge, tag_top);
        }
        let after = Tag::new(context, &mode_label(self.mode.get()));
        let after_x = right_edge - after.width;
        if after_x - split > inset {
            after.paint(context, after_x, tag_top);
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
}

impl Tag {
    const PAD_X: f64 = 8.0;
    const PAD_Y: f64 = 3.0;

    fn new(context: &Context, text: &str) -> Self {
        let layout = pangocairo::functions::create_layout(context);
        let mut font = pango::FontDescription::new();
        font.set_family("Inter,Noto Sans");
        font.set_weight(pango::Weight::Medium);
        font.set_absolute_size(11.0 * f64::from(pango::SCALE));
        layout.set_font_description(Some(&font));
        layout.set_text(text);
        let (text_width, _) = layout.pixel_size();
        Self {
            layout,
            width: (f64::from(text_width) + 2.0 * Self::PAD_X).round(),
        }
    }

    fn paint(&self, context: &Context, x: f64, y: f64) {
        let (_, text_height) = self.layout.pixel_size();
        let height = (f64::from(text_height) + 2.0 * Self::PAD_Y).round();
        render::rounded_rect_path(context, x, y, self.width, height, 5.0);
        context.set_source_rgba(0.07, 0.07, 0.08, 0.78);
        context.fill().ok();
        context.set_source_rgba(1.0, 1.0, 1.0, 0.94);
        context.move_to((x + Self::PAD_X).round(), (y + Self::PAD_Y).round());
        pangocairo::functions::show_layout(context, &self.layout);
    }
}

fn mode_label(mode: SourceUpscale) -> String {
    match mode {
        SourceUpscale::Neural4x => t("Neural 4×"),
        _ => t("Neural 2×"),
    }
}

fn mode_index(mode: SourceUpscale) -> usize {
    usize::from(mode == SourceUpscale::Neural4x)
}

/// What each output size exports at. A Motion export keeps its Frame size,
/// so both modes name the frame there.
fn output_size_label(session: &MotionSession, mode: SourceUpscale, motion_export: bool) -> String {
    let (width, height) = if motion_export {
        let source = source_dimensions(session);
        let (width, height) = motion_render::motion_frame_output_size(
            f64::from(source.0),
            f64::from(source.1),
            &session.motion_state(),
        );
        (width.max(0) as u32, height.max(0) as u32)
    } else {
        output_dimensions(session, mode)
    };
    format!("{width} × {height} px")
}

fn wire_sheet_lifecycle(
    window: &gtk4::ApplicationWindow,
    overlay: &Overlay,
    scrim: &GtkBox,
    keys: &EventControllerKey,
    active: &Rc<RefCell<Option<Arc<AtomicBool>>>>,
) -> Rc<dyn Fn()> {
    OPEN.with(|open| open.set(true));
    window.add_controller(keys.clone());
    let closed = Rc::new(Cell::new(false));
    let cleanup: Rc<dyn Fn()> = {
        let closed = closed.clone();
        let window = window.downgrade();
        let keys = keys.downgrade();
        let active = active.clone();
        Rc::new(move || {
            if closed.replace(true) {
                return;
            }
            if let Some(cancel) = active.borrow().as_ref() {
                cancel.store(true, Ordering::Relaxed);
            }
            OPEN.with(|open| open.set(false));
            if let (Some(window), Some(keys)) = (window.upgrade(), keys.upgrade()) {
                window.remove_controller(&keys);
            }
        })
    };
    scrim.connect_unrealize({
        let cleanup = cleanup.clone();
        move |_| cleanup()
    });
    scrim.connect_parent_notify({
        let cleanup = cleanup.clone();
        move |scrim| {
            if scrim.parent().is_none() {
                cleanup();
            }
        }
    });
    let scrim = scrim.downgrade();
    let overlay = overlay.downgrade();
    Rc::new(move || {
        cleanup();
        if let (Some(scrim), Some(overlay)) = (scrim.upgrade(), overlay.upgrade()) {
            if scrim.parent().as_ref() == Some(overlay.upcast_ref::<gtk4::Widget>()) {
                overlay.remove_overlay(&scrim);
            }
        }
    })
}

pub(super) fn show(window: &gtk4::ApplicationWindow, session: &MotionSession, motion_export: bool) {
    if OPEN.with(Cell::get)
        || unavailable_reason(session, SourceUpscale::Neural2x, motion_export).is_some()
    {
        return;
    }
    let Some(overlay) = window.child().and_downcast::<Overlay>() else {
        return;
    };
    let dark = session.prefers_dark;
    let (source_width, source_height) = source_dimensions(session);
    let current = session.runtime.borrow().motion.appearance.source_upscale;
    let initial = if current == SourceUpscale::Neural4x
        && unavailable_reason(session, SourceUpscale::Neural4x, motion_export).is_none()
    {
        SourceUpscale::Neural4x
    } else {
        SourceUpscale::Neural2x
    };

    let scrim = GtkBox::new(Orientation::Vertical, 0);
    scrim.add_css_class("editor-upscale-scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    scrim.set_can_target(true);

    // The comparison fills the card's left side; the decision lives in a
    // narrow panel on the right.
    let card = GtkBox::new(Orientation::Horizontal, 0);
    card.add_css_class("editor-upscale-card");
    card.add_css_class(if dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    });
    card.set_overflow(Overflow::Hidden);
    card.set_can_target(true);
    // A compact sheet: big enough to judge detail, small enough that the
    // editor stays in view around it.
    let width = (f64::from(window.width()) * 0.66).clamp(680.0, 940.0);
    let height = (f64::from(window.height()) * 0.70).clamp(440.0, 600.0);
    card.set_size_request(width as i32, height as i32);

    // Stage: the export, filling everything left of the panel.
    let drawing = DrawingArea::new();
    drawing.set_hexpand(true);
    drawing.set_vexpand(true);
    drawing.set_tooltip_text(Some(&t("Before / after divider")));
    let scroll = ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .child(&drawing)
        .build();
    scroll.add_css_class("editor-upscale-scroll");
    let spinner = Spinner::new();
    let message = Label::new(None);
    message.add_css_class("editor-upscale-message");
    message.set_wrap(true);
    message.set_max_width_chars(52);
    message.set_justify(gtk4::Justification::Center);
    let status = GtkBox::new(Orientation::Vertical, 12);
    status.set_halign(Align::Center);
    status.set_valign(Align::Center);
    status.append(&spinner);
    status.append(&message);
    let stage_overlay = Overlay::new();
    stage_overlay.set_child(Some(&scroll));
    stage_overlay.add_overlay(&status);
    stage_overlay.set_vexpand(true);
    let stage_box = GtkBox::new(Orientation::Vertical, 0);
    stage_box.add_css_class("editor-upscale-stage");
    stage_box.set_hexpand(true);
    stage_box.set_vexpand(true);
    stage_box.set_overflow(Overflow::Hidden);
    stage_box.append(&stage_overlay);

    // Panel: title, output size, zoom, then the decision at the bottom.
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("editor-upscale-panel");
    panel.set_size_request(272, -1);
    panel.set_hexpand(false);

    let head = GtkBox::new(Orientation::Horizontal, 8);
    let titles = GtkBox::new(Orientation::Vertical, 4);
    titles.set_hexpand(true);
    let title = Label::new(Some(&t("Compare enhancement")));
    title.add_css_class("editor-upscale-title");
    title.set_xalign(0.0);
    let hint = Label::new(Some(&t("Drag the divider across the image.")));
    hint.add_css_class("editor-upscale-hint");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    titles.append(&title);
    titles.append(&hint);
    let close_button = Button::new();
    close_button.add_css_class("editor-upscale-close");
    close_button.set_has_frame(false);
    close_button.set_focusable(false);
    close_button.set_valign(Align::Start);
    close_button.set_tooltip_text(Some(&t("Close")));
    let close_icon = Image::from_icon_name("window-close-symbolic");
    close_icon.set_pixel_size(14);
    close_button.set_child(Some(&close_icon));
    head.append(&titles);
    head.append(&close_button);
    panel.append(&head);

    panel.append(&eyebrow(&t("Output size")));
    let options = GtkBox::new(Orientation::Vertical, 8);
    let mode_tabs = [SourceUpscale::Neural2x, SourceUpscale::Neural4x].map(|mode| {
        option(
            &mode_label(mode),
            &output_size_label(session, mode, motion_export),
        )
    });
    mode_tabs[1].set_group(Some(&mode_tabs[0]));
    mode_tabs[mode_index(initial)].set_active(true);
    if let Some(reason) = unavailable_reason(session, SourceUpscale::Neural4x, motion_export) {
        mode_tabs[1].set_sensitive(false);
        mode_tabs[1].set_tooltip_text(Some(&reason));
    }
    for tab in &mode_tabs {
        options.append(tab);
    }
    panel.append(&options);

    let source_row = GtkBox::new(Orientation::Horizontal, 8);
    source_row.add_css_class("editor-upscale-source");
    let source_name = Label::new(Some(&t("Source")));
    source_name.add_css_class("editor-upscale-source-name");
    source_name.set_xalign(0.0);
    source_name.set_hexpand(true);
    let source_value = Label::new(Some(&format!("{source_width} × {source_height} px")));
    source_value.add_css_class("editor-upscale-source-value");
    source_row.append(&source_name);
    source_row.append(&source_value);
    panel.append(&source_row);

    panel.append(&eyebrow(&t("Zoom")));
    let (zooms, zoom_tabs) = segmented(&["Fit", "100%", "200%"], 0);
    panel.append(&zooms);

    let filler = GtkBox::new(Orientation::Vertical, 0);
    filler.set_vexpand(true);
    panel.append(&filler);

    if !crate::studio_entitlement::studio_unlocked() {
        let note = Label::new(Some(&t(
            "Free preview. Exporting with enhancement needs Studio.",
        )));
        note.add_css_class("editor-upscale-note");
        note.set_xalign(0.0);
        note.set_wrap(true);
        panel.append(&note);
    }

    let cancel = Button::with_label(&t("Cancel"));
    cancel.add_css_class("editor-upscale-cancel");
    cancel.set_has_frame(false);
    cancel.set_hexpand(true);
    let apply = Button::with_label(&t("Use Neural 2×"));
    apply.add_css_class("editor-done-button");
    apply.add_css_class("editor-upscale-apply");
    apply.set_has_frame(false);
    apply.set_hexpand(true);
    apply.set_sensitive(false);
    let actions = GtkBox::new(Orientation::Horizontal, 8);
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
        dark,
        hadjustment: scroll.hadjustment(),
        vadjustment: scroll.vadjustment(),
    });
    let cache: Rc<RefCell<[Option<Rc<Comparison>>; 2]>> = Rc::new(RefCell::new([None, None]));
    let generation = Rc::new(Cell::new(0u64));
    let active: Rc<RefCell<Option<Arc<AtomicBool>>>> = Rc::new(RefCell::new(None));

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
    drawing.connect_scale_factor_notify({
        let stage = stage.clone();
        move |area| resize_canvas(area, &stage)
    });

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
        let session = session.clone();
        let stage = stage.clone();
        let status = status.clone();
        let message = message.clone();
        let spinner = spinner.clone();
        let apply = apply.clone();
        let drawing = drawing.clone();
        let cache = cache.clone();
        let generation = generation.clone();
        let active = active.clone();
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
            if let Some(reason) = unavailable_reason(&session, selected, motion_export) {
                fail(reason);
                return;
            }
            let Some(source) = session.source_snapshot() else {
                fail(t("Load an image before previewing upscaling."));
                return;
            };
            // A render for the other mode keeps running so switching back
            // is instant; only closing the sheet cancels it.
            let cancel = active
                .borrow_mut()
                .get_or_insert_with(|| Arc::new(AtomicBool::new(false)))
                .clone();
            let motion = motion_export.then(|| session.motion_state());
            let prefers_dark = session.prefers_dark;
            let (width, height) = source_dimensions(&session);
            let total = crate::capture::editor::upscale::tile_count(width, height);
            let tiles_done = Arc::new(AtomicUsize::new(0));
            let worker = {
                let cancel = Arc::clone(&cancel);
                let tiles_done = Arc::clone(&tiles_done);
                gio::spawn_blocking(move || {
                    render_comparison(
                        source(),
                        selected,
                        motion.as_ref(),
                        prefers_dark,
                        &cancel,
                        &tiles_done,
                    )
                })
            };
            let id = generation.get();
            // The network runs one tile at a time, so the stage reports how
            // far it is and, once a few tiles have timed it, how long is left.
            let started = Instant::now();
            glib::timeout_add_local(PROGRESS_TICK, {
                let generation = generation.clone();
                let cancel = Arc::clone(&cancel);
                let tiles_done = Arc::clone(&tiles_done);
                let message = message.clone();
                move || {
                    let done = tiles_done.load(Ordering::Relaxed);
                    if generation.get() != id || cancel.load(Ordering::Relaxed) || done >= total {
                        return glib::ControlFlow::Break;
                    }
                    message.set_text(&progress_message(done, total, started.elapsed()));
                    glib::ControlFlow::Continue
                }
            });
            let cache = cache.clone();
            let generation = generation.clone();
            let present = present.clone();
            let fail = fail.clone();
            glib::MainContext::default().spawn_local(async move {
                let result = worker.await.unwrap_or_else(|_| {
                    Err(t("The upscaling preview worker stopped unexpectedly."))
                });
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let comparison = result.and_then(|(before, after)| {
                    match (
                        render::rgba_image_to_surface(&before),
                        render::rgba_image_to_surface(&after),
                    ) {
                        (Some(before), Some(after)) => Ok(Rc::new(Comparison { before, after })),
                        _ => Err(t("Could not prepare the comparison image.")),
                    }
                });
                if let Ok(comparison) = &comparison {
                    cache.borrow_mut()[index] = Some(comparison.clone());
                }
                if generation.get() == id {
                    match comparison {
                        Ok(comparison) => present(comparison),
                        Err(error) => fail(error),
                    }
                }
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
    let close = wire_sheet_lifecycle(window, &overlay, &scrim, &keys, &active);
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
        let session = session.clone();
        move |_| {
            let selected = stage.mode.get();
            let mut runtime = session.runtime.borrow_mut();
            if runtime.motion.appearance.source_upscale != selected {
                runtime.begin_motion_edit();
                runtime.motion.appearance.source_upscale = selected;
            }
            drop(runtime);
            if let Some(state) = session.source_state.borrow().as_ref() {
                state.lock().unwrap().source_upscale = selected;
            }
            close();
        }
    });

    overlay.add_overlay(&scrim);
    request();
}

/// The stage's line while the network runs. Until a few tiles have timed
/// it, it only says it is working; after that it gives the share done and
/// the time left, and a long preview says so plainly, so a large image
/// never looks stuck.
fn progress_message(done: usize, total: usize, elapsed: Duration) -> String {
    const TIMED_TILES: usize = 2;
    if total == 0 || done < TIMED_TILES {
        return t("Preparing before / after preview…");
    }
    let percent = (done * 100 / total).min(99);
    let per_tile = elapsed.as_secs_f64() / done as f64;
    let projected = Duration::from_secs_f64(per_tile * total as f64);
    let left = Duration::from_secs_f64(per_tile * (total - done.min(total)) as f64);
    let left = format_duration(left);
    if projected > LONG_PREVIEW {
        tfmt(
            "This image is large, so enhancing it takes a while on this device. {percent}% done, about {left} left. You can close the preview to stop.",
            &[("percent", &percent.to_string()), ("left", &left)],
        )
    } else {
        tfmt(
            "Enhancing… {percent}%, about {left} left",
            &[("percent", &percent.to_string()), ("left", &left)],
        )
    }
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs().max(1);
    if seconds < 60 {
        tfmt("{seconds} s", &[("seconds", &seconds.to_string())])
    } else {
        let minutes = seconds.div_ceil(60);
        tfmt("{minutes} min", &[("minutes", &minutes.to_string())])
    }
}

/// A small section heading in the panel.
fn eyebrow(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class("editor-upscale-eyebrow");
    label.set_xalign(0.0);
    label.set_margin_top(22);
    label.set_margin_bottom(8);
    label
}

/// One output size: a radio mark, the mode's name, and the size it exports at.
fn option(name: &str, detail: &str) -> ToggleButton {
    let button = ToggleButton::new();
    button.add_css_class("editor-upscale-option");
    button.set_has_frame(false);
    button.set_focusable(false);
    let row = GtkBox::new(Orientation::Horizontal, 12);
    let mark = GtkBox::new(Orientation::Horizontal, 0);
    mark.add_css_class("editor-upscale-radio");
    mark.set_valign(Align::Center);
    mark.set_halign(Align::Center);
    let texts = GtkBox::new(Orientation::Vertical, 2);
    let title = Label::new(Some(name));
    title.add_css_class("editor-upscale-option-title");
    title.set_xalign(0.0);
    let size = Label::new(Some(detail));
    size.add_css_class("editor-upscale-option-detail");
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
    tray.add_css_class("editor-upscale-seg");
    tray.set_homogeneous(true);
    tray.set_valign(Align::Center);
    let mut tabs: Vec<ToggleButton> = Vec::new();
    for label in labels {
        let tab = ToggleButton::with_label(&t(label));
        tab.add_css_class("editor-upscale-seg-button");
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
    let margin = 2.0 * STAGE_MARGIN;
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

pub(super) fn source_dimensions(session: &MotionSession) -> (u32, u32) {
    session
        .source_state
        .borrow()
        .as_ref()
        .map(|source| source.lock().unwrap().working_image.dimensions())
        .unwrap_or((0, 0))
}

pub(super) fn input_unavailable_reason((width, height): (u32, u32)) -> Option<String> {
    if width == 0 || height == 0 {
        return Some(t("Load an image before previewing upscaling."));
    }
    crate::capture::editor::upscale::validate_input_dimensions(width, height, SourceUpscale::Neural2x)
        .err()
        .map(|reason| tfmt("{reason} This image is {width} × {height} pixels. Crop it or choose a smaller image to enable enhancement.", &[("reason", &t(&reason)), ("width", &width.to_string()), ("height", &height.to_string())]))
}

pub(super) fn output_dimensions(session: &MotionSession, mode: SourceUpscale) -> (u32, u32) {
    session
        .source_state
        .borrow()
        .as_ref()
        .map(|source| source.lock().unwrap().export_dimensions(mode))
        .unwrap_or((0, 0))
}

pub(super) fn unavailable_reason(
    session: &MotionSession,
    mode: SourceUpscale,
    motion_export: bool,
) -> Option<String> {
    if let Some(reason) = input_unavailable_reason(source_dimensions(session)) {
        return Some(reason);
    }
    if motion_export {
        return None;
    }
    session.source_state.borrow().as_ref().and_then(|source| {
        source
            .lock()
            .unwrap()
            .validate_upscale_export(mode)
            .err()
            .map(|reason| t(&reason))
    })
}

const MAX_MOTION_COMPARISON_PIXELS: i64 = 8_388_608;
const MAX_MOTION_COMPARISON_EDGE: i32 = 7_680;

fn checkpoint(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("The upscaling preview was cancelled.".into());
    }
    Ok(())
}

fn checked_motion_comparison_size(width: i32, height: i32) -> Result<(i32, i32), String> {
    if width <= 0 || height <= 0 {
        return Err("The Motion comparison frame has no pixels to render.".into());
    }
    if width.max(height) > MAX_MOTION_COMPARISON_EDGE
        || i64::from(width) * i64::from(height) > MAX_MOTION_COMPARISON_PIXELS
    {
        return Err(
            "The Motion comparison frame is too large to preview. Reduce the Frame size.".into(),
        );
    }
    Ok((width, height))
}

fn render_comparison(
    mut state: EditorState,
    mode: SourceUpscale,
    motion: Option<&MotionState>,
    prefers_dark: bool,
    cancel: &AtomicBool,
    tiles_done: &AtomicUsize,
) -> Result<(RgbaImage, RgbaImage), String> {
    checkpoint(cancel)?;
    let frame = motion
        .map(|motion| {
            let (width, height) = motion_render::motion_frame_output_size(
                f64::from(state.working_image.width()),
                f64::from(state.working_image.height()),
                motion,
            );
            checked_motion_comparison_size(width, height).map(|size| (motion, size))
        })
        .transpose()?;
    state.source_upscale = mode;
    let before = state
        .upscaled_export_state(false)
        .map_err(|error| error.to_string())?;
    checkpoint(cancel)?;
    let after = state
        .upscaled_export_state_with_progress(true, &|done| {
            tiles_done.store(done, Ordering::Relaxed);
            !cancel.load(Ordering::Relaxed)
        })
        .map_err(|error| error.to_string())?;
    checkpoint(cancel)?;
    let Some((motion, (width, height))) = frame else {
        return Ok((
            before.to_final_image().map_err(|error| error.to_string())?,
            after.to_final_image().map_err(|error| error.to_string())?,
        ));
    };
    let background_path = match motion.appearance.background_fill_type {
        MotionBackgroundFillType::Wallpaper => motion.appearance.wallpaper_image_name.as_deref(),
        MotionBackgroundFillType::Image => motion.appearance.custom_background_image.as_deref(),
        _ => None,
    };
    let background = background_path.and_then(motion_render::load_motion_background_surface);
    let watermark = motion
        .watermark
        .image_file_name
        .as_deref()
        .and_then(motion_render::load_motion_background_surface);
    let render_card = |state: EditorState| -> Result<RgbaImage, String> {
        let card = state
            .to_motion_card_image()
            .map_err(|error| error.to_string())?;
        let card = render::rgba_image_to_surface(&card)
            .ok_or_else(|| t("Could not prepare the comparison image."))?;
        let mut surface = ImageSurface::create(Format::ARgb32, width, height)
            .map_err(|error| error.to_string())?;
        {
            let context = Context::new(&surface).map_err(|error| error.to_string())?;
            motion_render::draw_motion_output_frame(
                &context,
                width,
                height,
                &card,
                motion,
                background.as_ref(),
                watermark.as_ref(),
                motion.playhead,
                prefers_dark,
                f64::from(mode.factor()),
            );
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().map_err(|error| error.to_string())?;
        Ok(render::cairo_argb_to_rgba_image(
            width as u32,
            height as u32,
            stride,
            &data,
        ))
    };
    Ok((render_card(before)?, render_card(after)?))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use image::RgbaImage;

    use super::{
        checked_motion_comparison_size, checkpoint, progress_message, render_comparison,
        MAX_MOTION_COMPARISON_EDGE, MAX_MOTION_COMPARISON_PIXELS,
    };
    use std::time::Duration;

    /// A large image must say how long it will take instead of spinning;
    /// a quick one only shows its progress.
    #[test]
    fn previews_report_progress_and_flag_long_runs() {
        assert_eq!(
            progress_message(1, 96, Duration::from_secs(2)),
            "Preparing before / after preview…"
        );
        let quick = progress_message(10, 20, Duration::from_secs(2));
        assert!(quick.starts_with("Enhancing… 50%"), "{quick}");
        let long = progress_message(4, 96, Duration::from_secs(8));
        assert!(long.starts_with("This image is large"), "{long}");
        assert!(long.contains("4% done, about 4 min left"), "{long}");
    }
    use crate::capture::editor::{state::EditorState, upscale::SourceUpscale};

    #[test]
    fn motion_comparison_accepts_the_pixel_budget_boundary_without_allocating() {
        assert_eq!(checked_motion_comparison_size(2048, 4096), Ok((2048, 4096)));
        assert!(checked_motion_comparison_size(2049, 4096).is_err());
        assert_eq!(
            i64::from(2048) * i64::from(4096),
            MAX_MOTION_COMPARISON_PIXELS
        );
    }

    #[test]
    fn motion_comparison_accepts_the_edge_budget_boundary() {
        assert_eq!(
            checked_motion_comparison_size(MAX_MOTION_COMPARISON_EDGE, 1),
            Ok((MAX_MOTION_COMPARISON_EDGE, 1))
        );
        assert!(checked_motion_comparison_size(MAX_MOTION_COMPARISON_EDGE + 1, 1).is_err());
        assert!(checked_motion_comparison_size(1, MAX_MOTION_COMPARISON_EDGE + 1).is_err());
    }

    #[test]
    fn motion_comparison_rejects_empty_and_negative_frames() {
        assert!(checked_motion_comparison_size(0, 1080).is_err());
        assert!(checked_motion_comparison_size(1920, 0).is_err());
        assert!(checked_motion_comparison_size(-2, 1080).is_err());
    }

    #[test]
    fn cancelled_previews_stop_before_any_work() {
        assert!(checkpoint(&AtomicBool::new(false)).is_ok());
        assert!(checkpoint(&AtomicBool::new(true)).is_err());
        let state = EditorState::new(RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255])));
        let result = render_comparison(
            state,
            SourceUpscale::Neural2x,
            None,
            false,
            &AtomicBool::new(true),
            &std::sync::atomic::AtomicUsize::new(0),
        );
        assert_eq!(
            result.err().as_deref(),
            Some("The upscaling preview was cancelled.")
        );
    }

    use super::{wire_sheet_lifecycle, EventControllerKey, GtkBox, Orientation, Overlay, OPEN};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    fn pump_events() {
        while gtk4::glib::MainContext::default().iteration(false) {}
    }

    fn sheet_fixture(
        token: Option<Arc<AtomicBool>>,
    ) -> (
        gtk4::ApplicationWindow,
        Overlay,
        GtkBox,
        EventControllerKey,
        Rc<RefCell<Option<Arc<AtomicBool>>>>,
        Rc<dyn Fn()>,
    ) {
        let window = gtk4::ApplicationWindow::new(&crate::test_support::test_app());
        let overlay = Overlay::new();
        window.set_child(Some(&overlay));
        let scrim = GtkBox::new(Orientation::Vertical, 0);
        let keys = EventControllerKey::new();
        let active: Rc<RefCell<Option<Arc<AtomicBool>>>> = Rc::new(RefCell::new(token));
        let close = wire_sheet_lifecycle(&window, &overlay, &scrim, &keys, &active);
        overlay.add_overlay(&scrim);
        window.present();
        pump_events();
        (window, overlay, scrim, keys, active, close)
    }

    #[test]
    fn sheet_lifecycle_explicit_close_is_idempotent() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let token = Arc::new(AtomicBool::new(false));
            let (window, _overlay, scrim, keys, _active, close) =
                sheet_fixture(Some(token.clone()));
            assert!(OPEN.with(Cell::get), "wiring the sheet marks it open");
            close();
            close();
            assert!(!OPEN.with(Cell::get));
            assert!(token.load(Ordering::Relaxed), "close cancels the worker");
            assert!(scrim.parent().is_none(), "close detaches the scrim");
            assert!(keys.widget().is_none(), "close removes the key controller");
            window.destroy();
            pump_events();
        }) else {
            assert!(
                std::env::var_os("APEXSHOT_REQUIRE_PREVIEW_GTK_TESTS").is_none(),
                "GTK initialization is required for preview lifecycle regressions"
            );
            return;
        };
    }

    #[test]
    fn sheet_lifecycle_external_detach_cleans_up() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let token = Arc::new(AtomicBool::new(false));
            let (window, overlay, scrim, keys, _active, _close) =
                sheet_fixture(Some(token.clone()));
            overlay.remove_overlay(&scrim);
            pump_events();
            assert!(!OPEN.with(Cell::get));
            assert!(token.load(Ordering::Relaxed));
            assert!(keys.widget().is_none());
            window.destroy();
            pump_events();
        }) else {
            assert!(
                std::env::var_os("APEXSHOT_REQUIRE_PREVIEW_GTK_TESTS").is_none(),
                "GTK initialization is required for preview lifecycle regressions"
            );
            return;
        };
    }

    #[test]
    fn sheet_lifecycle_parent_destroy_allows_reopen_without_stale_cleanup() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let token = Arc::new(AtomicBool::new(false));
            let (window, _overlay, _scrim, keys, _active, close) =
                sheet_fixture(Some(token.clone()));
            window.destroy();
            pump_events();
            assert!(
                !OPEN.with(Cell::get),
                "destroying the parent resets the sheet"
            );
            assert!(
                token.load(Ordering::Relaxed),
                "destroying the parent cancels the worker"
            );
            assert!(
                keys.widget().is_none(),
                "destroying the parent detaches keys"
            );

            let token2 = Arc::new(AtomicBool::new(false));
            let (window2, _overlay2, _scrim2, keys2, _active2, close2) =
                sheet_fixture(Some(token2.clone()));
            assert!(OPEN.with(Cell::get));
            assert!(keys2.widget().is_some());
            close();
            assert!(
                OPEN.with(Cell::get),
                "a stale close must not clear the new sheet"
            );
            assert!(
                keys2.widget().is_some(),
                "a stale close must not strip the new keys"
            );
            close2();
            assert!(!OPEN.with(Cell::get));
            window2.destroy();
            pump_events();
        }) else {
            assert!(
                std::env::var_os("APEXSHOT_REQUIRE_PREVIEW_GTK_TESTS").is_none(),
                "GTK initialization is required for preview lifecycle regressions"
            );
            return;
        };
    }

    #[test]
    fn sheet_lifecycle_parent_destroy_without_worker_resets() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let (window, _overlay, _scrim, _keys, _active, _close) = sheet_fixture(None);
            window.destroy();
            pump_events();
            assert!(!OPEN.with(Cell::get));
        }) else {
            assert!(
                std::env::var_os("APEXSHOT_REQUIRE_PREVIEW_GTK_TESTS").is_none(),
                "GTK initialization is required for preview lifecycle regressions"
            );
            return;
        };
    }
}
