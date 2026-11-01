//! The Linux Appearance panel's tool groups below its Background card.
//!
//! After the fill-source card, the Linux static builder stacks five more
//! groups in a fixed order: Export quality, then Transform, Shadow, Style
//! and Frame. This module rebuilds those as shape-only shells: every
//! Background controls drive the static document; the other groups are
//! currently visual shells.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::cairo::Context as CairoContext;
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, DrawingArea, Entry, GestureClick, GestureDrag, Grid, Image,
    Label, Orientation, PolicyType, Revealer, RevealerTransitionType, ScrolledWindow, Stack,
    ToggleButton,
};

use super::annotate::EditorHost;
use super::ui_support::px;
use crate::capture::editor::state::background::{Fill, FrameRatio};
use crate::capture::editor::window::icon_names;
use crate::i18n::{t, tfmt};
use crate::video_editor::side_panel::FilledSlider;

/// Edge of one wallpaper thumbnail. The tile is pinned to it and centred in
/// its grid cell, exactly as the Linux panel pins its tiles: a homogeneous
/// column otherwise stretches the preview and the grid stops reading as a
/// catalog.
const TILE_EDGE: f64 = 56.0;
/// Height of the catalog's own viewport. The grid scrolls inside it, so the
/// shared Padding and Radius rows below stay in reach.
const CATALOG_HEIGHT: f64 = 260.0;
const GRID_GAP: f64 = 8.0;
const WALLPAPER_COLUMNS: usize = 4;
const TILE_RADIUS: f64 = 11.0;

/// One card in the inspector: a heading over the rows it owns, the same
/// raised surface the Linux sidebar gives every tool group.
pub(super) fn section_card(title: &str, scale: f64) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, px(8.0, scale));
    section.add_css_class("image-editor-section");
    let heading = Label::new(Some(&t(title)));
    heading.add_css_class("image-editor-section-title");
    heading.set_xalign(0.0);
    section.append(&heading);
    section
}

/// A hint line under the rows it explains, the Linux editor's quiet copy.
fn hint(text: &str) -> Label {
    let label = Label::new(Some(&t(text)));
    label.add_css_class("image-editor-hint");
    label.set_wrap(true);
    label.set_xalign(0.0);
    label
}

/// A tool group below the fill tabs: a clickable header row (title and
/// chevron) over a revealer, collapsed by default. The fill tabs own the top
/// of the panel; everything else opens on demand, so the sidebar never
/// becomes one long scroll.
fn disclosure_row(title: &str, body: &GtkBox, scale: f64) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, px(8.0, scale));
    card.add_css_class("image-editor-section");

    let header = GtkBox::new(Orientation::Horizontal, px(8.0, scale));
    header.add_css_class("image-editor-disclosure-header");
    let heading = Label::new(Some(&t(title)));
    heading.add_css_class("image-editor-section-title");
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    let chevron = Label::new(Some("›"));
    chevron.add_css_class("image-editor-disclosure-chevron");
    header.append(&heading);
    header.append(&chevron);
    card.append(&header);

    let revealer = Revealer::builder()
        .transition_type(RevealerTransitionType::SlideDown)
        .transition_duration(120)
        .child(body)
        .build();
    revealer.set_hexpand(false);
    revealer.set_halign(Align::Fill);
    revealer.set_reveal_child(false);
    card.append(&revealer);

    let click = GestureClick::new();
    click.connect_pressed({
        let revealer = revealer.clone();
        let chevron = chevron.clone();
        move |_, _, _, _| {
            let revealed = revealer.reveals_child();
            revealer.set_reveal_child(!revealed);
            chevron.set_label(if revealed { "›" } else { "⌄" });
        }
    });
    header.add_controller(click);
    card
}

/// A flat control row styled like the Transform reset button: a subtle fill
/// with no frame, used by Compare and Reset.
fn inline_button(label: &str, scale: f64) -> Button {
    let button = Button::with_label(label);
    button.add_css_class("image-editor-inline-button");
    button.set_has_frame(false);
    button.set_halign(Align::Fill);
    button.set_hexpand(true);
    button.set_size_request(-1, px(30.0, scale));
    button
}

/// One tile registered in a selection namespace: its ring flag and the shape
/// that paints it.
type TileEntry = (Rc<Cell<bool>>, DrawingArea);

/// Selection state shared by a grid of tiles: at most one tile carries its
/// ring, and clicking the selected tile hands selection back (reset).
#[derive(Clone)]
struct TileSelection {
    tiles: Rc<RefCell<Vec<TileEntry>>>,
    selected: Rc<Cell<Option<usize>>>,
}

impl TileSelection {
    fn new() -> Self {
        Self {
            tiles: Rc::new(RefCell::new(Vec::new())),
            selected: Rc::new(Cell::new(None)),
        }
    }

    /// Register a tile so clicking elsewhere can un-site its ring.
    fn push(&self, flag: Rc<Cell<bool>>, shape: &DrawingArea) {
        self.tiles.borrow_mut().push((flag, shape.clone()));
    }

    /// Apply the picker's rule to a click on tile `index`: an already
    /// selected tile resets selection, any other takes the ring.
    fn clicked(&self, index: usize, tile_flag: &Rc<Cell<bool>>, _tile: &DrawingArea) {
        let now = if self.selected.get() == Some(index) {
            None
        } else {
            Some(index)
        };
        self.selected.set(now);
        let on = now == Some(index);
        tile_flag.set(on);
        for (flag, shape) in self.tiles.borrow().iter() {
            let is_this = Rc::ptr_eq(flag, tile_flag);
            flag.set(is_this && on);
            shape.queue_draw();
        }
    }

    fn clear(&self) {
        self.selected.set(None);
        for (flag, shape) in self.tiles.borrow().iter() {
            flag.set(false);
            shape.queue_draw();
        }
    }
}

// ── Export quality ──────────────────────────────────────────────────

/// The select-style source row: a dropdown trigger with check-marked option
/// rows, built from the docked bars' picker so the panel reads as one
/// control vocabulary. The Linux panel's MotionSelect looks the same.
fn export_select(scale: f64) -> Button {
    let labels: Vec<String> = ["Off", "Neural 2×", "Neural 4×"]
        .iter()
        .map(|s| t(s))
        .collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    super::sub_bar::option_pill(scale, labels[0], &labels)
}

/// The Export quality card: an upscale source plus the compare action, the
/// Linux panel's layout with both controls unavailable here.
pub(super) fn export_quality(scale: f64) -> GtkBox {
    let card = section_card("Export quality", scale);
    let unavailable = t("Neural upscaling is not available in the Windows image editor yet");

    let source = export_select(scale);
    source.set_sensitive(false);
    source.set_tooltip_text(Some(&unavailable));
    card.append(&source);

    let compare = inline_button(&t("Compare before / after"), scale);
    compare.set_sensitive(false);
    compare.set_tooltip_text(Some(&unavailable));
    card.append(&compare);

    card.append(&hint("Offline neural enhancement for small images. Image exports grow by 2× or 4×; Motion keeps its Frame size. Compare the export for free before enabling Studio enhancement. Supports up to 1 megapixel. Missing detail cannot always be recovered."));
    card
}

// ── Transform ───────────────────────────────────────────────────────

/// A slider naming its unit in the readout: the Linux angle/percent rows.
fn unit_slider(
    label: &str,
    min: f64,
    max: f64,
    initial: f64,
    suffix: &'static str,
    scale: f64,
) -> FilledSlider {
    let slider = FilledSlider::new(label, min, max, 1.0, 1.0, 0, scale).with_suffix(suffix);
    slider.set_value(initial);
    slider
}

/// The Transform disclosure: the still image's frozen camera pose. Yaw,
/// pitch and roll swing the card, perspective leans it once the first two
/// move off zero, and the pad plus the X/Y sliders place it.
pub(super) fn transform_disclosure(scale: f64) -> GtkBox {
    let body = GtkBox::new(Orientation::Vertical, px(8.0, scale));

    body.append(&hint(
        "Studio preview. A non-default rendered pose needs activation for final images; backgrounds and normal edits stay free.",
    ));

    let yaw = unit_slider(&t("Yaw"), -24.0, 24.0, 0.0, "°", scale);
    let pitch = unit_slider(&t("Pitch"), -24.0, 24.0, 0.0, "°", scale);
    let roll = unit_slider(&t("Roll"), -24.0, 24.0, 0.0, "°", scale);
    let perspective = unit_slider(&t("Perspective"), 0.0, 100.0, 18.0, "%", scale);
    let position_x = unit_slider(&t("X"), -1000.0, 1000.0, 0.0, "", scale);
    let position_y = unit_slider(&t("Y"), -1000.0, 1000.0, 0.0, "", scale);

    // The Linux body is two labelled groups inside the one disclosure:
    // Rotation & Perspective, then the position field, then Reset.
    let rotation_title = subheader("Rotation & Perspective");
    body.append(&rotation_title);
    body.append(&yaw.widget());
    body.append(&pitch.widget());
    body.append(&roll.widget());
    body.append(&perspective.widget());
    body.append(&hint(
        "Tilt the image with Yaw or Pitch before adjusting perspective.",
    ));

    let position_title = subheader("Position");
    body.append(&position_title);
    let (pad, pad_position) = position_pad(scale);
    body.append(&pad);
    body.append(&position_x.widget());
    body.append(&position_y.widget());

    let reset = inline_button(&t("Reset transforms"), scale);
    reset.connect_clicked(move |_| {
        for (slider, value) in [
            (&yaw, 0.0),
            (&pitch, 0.0),
            (&roll, 0.0),
            (&perspective, 18.0),
            (&position_x, 0.0),
            (&position_y, 0.0),
        ] {
            slider.set_value(value);
        }
        pad_position.set((0.0, 0.0));
        pad.queue_draw();
    });
    body.append(&reset);

    disclosure_row("Transform", &body, scale)
}

/// The camera-framing pad: a soft square with a draggable dot, top-right
/// showing top-right. It reports its normalized position through the handle
/// so Reset can park it back at the origin.
fn position_pad(scale: f64) -> (DrawingArea, Rc<Cell<(f64, f64)>>) {
    let area = DrawingArea::new();
    area.set_size_request(-1, px(152.0, scale));
    area.set_hexpand(true);
    area.add_css_class("image-editor-position-pad");
    let position: Rc<Cell<(f64, f64)>> = Rc::new(Cell::new((0.0, 0.0)));
    area.set_draw_func({
        let position = position.clone();
        move |area, context, width, height| {
            paint_position_pad(area, context, width, height, &position.get());
        }
    });
    let apply_point = {
        let area = area.clone();
        let position = position.clone();
        move |point_x: f64, point_y: f64| {
            let width = f64::from(area.allocated_width().max(1));
            let height = f64::from(area.allocated_height().max(1));
            let x = (point_x / width * 2.0 - 1.0).clamp(-1.0, 1.0);
            let y = (point_y / height * 2.0 - 1.0).clamp(-1.0, 1.0);
            position.set((x, y));
            area.queue_draw();
        }
    };
    let click = GestureClick::new();
    click.set_button(1);
    click.connect_pressed({
        let apply_point = apply_point.clone();
        move |_, _, x, y| apply_point(x, y)
    });
    area.add_controller(click);
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let apply_point = apply_point.clone();
        move |gesture, _, _| {
            if let Some((x, y)) = gesture.start_point() {
                apply_point(x, y);
            }
        }
    });
    drag.connect_drag_update(move |gesture, dx, dy| {
        if let Some((start_x, start_y)) = gesture.start_point() {
            apply_point(start_x + dx, start_y + dy);
        }
    });
    area.add_controller(drag);
    area.set_cursor(gtk4::gdk::Cursor::from_name("move", None).as_ref());
    area.set_tooltip_text(Some(&t("Drag to choose what the camera shows")));
    (area, position)
}

/// The pad body: a soft raised square, the origin cross at centre, and the
/// shared soft-white handle at the chosen camera position.
fn paint_position_pad(
    widget: &DrawingArea,
    context: &CairoContext,
    width: i32,
    height: i32,
    position: &(f64, f64),
) {
    let width = f64::from(width.max(1));
    let height = f64::from(height.max(1));
    let light = widget_is_light(widget);
    rounded_rect(context, 0.0, 0.0, width, height, 10.0);
    if light {
        context.set_source_rgba(0.11, 0.13, 0.16, 0.10);
    } else {
        context.set_source_rgba(1.0, 1.0, 1.0, 0.07);
    }
    let _ = context.fill_preserve();
    // Match the idle filled portion of FillSlider across the whole pad.
    if light {
        context.set_source_rgba(0.11, 0.13, 0.16, 0.16);
    } else {
        context.set_source_rgba(1.0, 1.0, 1.0, 0.12);
    }
    let _ = context.fill();

    // Zero, so the field still states its origin without a grid.
    if light {
        context.set_source_rgba(0.11, 0.13, 0.16, 0.16);
    } else {
        context.set_source_rgba(1.0, 1.0, 1.0, 0.16);
    }
    context.set_line_width(1.0);
    for (dx, dy) in [(1.0, 0.0), (0.0, 1.0)] {
        context.move_to(
            width * 0.5 - dx * CENTRE_TICK,
            height * 0.5 - dy * CENTRE_TICK,
        );
        context.line_to(
            width * 0.5 + dx * CENTRE_TICK,
            height * 0.5 + dy * CENTRE_TICK,
        );
    }
    let _ = context.stroke();

    let (pos_x, pos_y) = *position;
    // Leave room for the complete marker at every extreme; the control value
    // still reaches +/- 1 exactly when the pointer reaches an edge.
    let marker_margin = 19.0_f64.min(width * 0.5).min(height * 0.5);
    let x = marker_margin + (pos_x + 1.0) * 0.5 * (width - marker_margin * 2.0);
    let y = marker_margin + (pos_y + 1.0) * 0.5 * (height - marker_margin * 2.0);
    paint_soft_handle(context, x, y);
}

/// Half the handle's side; its soft square matches the Motion pads' shared
/// marker so every point the inspector can choose draws the same way.
const HANDLE_HALF: f64 = 13.0;
/// Corner radius: a soft square, not a disc.
const HANDLE_RADIUS: f64 = 9.0;
/// Arm of the field's centre cross.
const CENTRE_TICK: f64 = 5.0;

/// The marker: one plain white soft square over a blurred shadow. No ring,
/// no core, one colour, so it reads as the chosen point rather than a
/// second widget parked on the pad.
fn paint_soft_handle(context: &CairoContext, x: f64, y: f64) {
    let shadow = gtk4::cairo::RadialGradient::new(
        x,
        y + 2.0,
        HANDLE_HALF * 0.35,
        x,
        y + 2.0,
        HANDLE_HALF * 1.9,
    );
    shadow.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.42);
    shadow.add_color_stop_rgba(0.55, 0.0, 0.0, 0.0, 0.18);
    shadow.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
    context.set_source(&shadow).ok();
    context.arc(x, y + 2.0, HANDLE_HALF * 1.9, 0.0, std::f64::consts::TAU);
    let _ = context.fill();

    rounded_rect(
        context,
        x - HANDLE_HALF,
        y - HANDLE_HALF,
        HANDLE_HALF * 2.0,
        HANDLE_HALF * 2.0,
        HANDLE_RADIUS,
    );
    context.set_source_rgb(1.0, 1.0, 1.0);
    let _ = context.fill();
}

/// Shadows toward the ancestor carrying the theme class, then stops.
fn widget_is_light(widget: &DrawingArea) -> bool {
    let mut current = Some(widget.clone().upcast::<gtk4::Widget>());
    while let Some(node) = current {
        if node.has_css_class("editor-theme-light") {
            return true;
        }
        current = node.parent();
    }
    false
}

// ── Shadow ──────────────────────────────────────────────────────────

/// The four shadow styles the Linux panel offers, in their row order.
const SHADOW_PRESETS: [&str; 4] = ["None", "Spread", "Hug", "Adaptive"];

/// The Shadow disclosure: one shadow cast by the image onto the background.
/// The pills pick a falloff style, the opacity dials it, and the light pad
/// places the light; the shadow falls opposite the dot.
pub(super) fn shadow_disclosure(scale: f64) -> GtkBox {
    let body = GtkBox::new(Orientation::Vertical, px(8.0, scale));

    let pills = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
    pills.add_css_class("image-editor-preset-row");
    pills.set_homogeneous(true);
    let mut first: Option<ToggleButton> = None;
    for (index, label) in SHADOW_PRESETS.into_iter().enumerate() {
        let pill = ToggleButton::with_label(&t(label));
        pill.add_css_class("image-editor-preset-pill");
        pill.set_has_frame(false);
        pill.set_hexpand(true);
        if let Some(first) = &first {
            pill.set_group(Some(first));
        } else {
            first = Some(pill.clone());
        }
        pill.set_active(index == 0);
        pills.append(&pill);
    }
    body.append(&pills);

    let opacity = unit_slider(&t("Opacity"), 0.0, 100.0, 100.0, "%", scale);
    body.append(&opacity.widget());

    let light_title = Label::new(Some(&t("Adjust Light")));
    light_title.add_css_class("image-editor-section-title");
    light_title.set_xalign(0.0);
    body.append(&light_title);
    body.append(&light_pad(px(136.0, scale)));

    disclosure_row("Shadow", &body, scale)
}

/// The "Adjust Light" pad: a dark rounded square with a draggable dot. The
/// dot is the light, so the cast falls on the opposite side of the card.
fn light_pad(edge: i32) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_size_request(edge, edge);
    area.set_hexpand(true);
    area.set_halign(Align::Fill);
    area.add_css_class("image-editor-light-pad");
    let light: Rc<Cell<(f64, f64)>> = Rc::new(Cell::new((0.65, 0.65)));
    area.set_draw_func({
        let light = light.clone();
        move |_, context, width, height| {
            paint_light_pad(context, width, height, &light.get());
        }
    });
    let move_light = {
        let area = area.clone();
        let light = light.clone();
        move |x: f64, y: f64| {
            let width = f64::from(area.allocated_width());
            let height = f64::from(area.allocated_height());
            let size = width.min(height).max(1.0);
            let origin_x = (width - size) * 0.5;
            let origin_y = (height - size) * 0.5;
            let dot = size * 0.072;
            let travel = (size * 0.5 - dot * 2.0).max(1.0);
            light.set((
                ((x - origin_x - size * 0.5) / travel).clamp(-1.0, 1.0),
                ((y - origin_y - size * 0.5) / travel).clamp(-1.0, 1.0),
            ));
            area.queue_draw();
        }
    };
    let drag = GestureDrag::new();
    drag.connect_drag_begin({
        let move_light = move_light.clone();
        move |gesture, _, _| {
            if let Some((x, y)) = gesture.start_point() {
                move_light(x, y);
            }
        }
    });
    drag.connect_drag_update({
        let move_light = move_light.clone();
        move |gesture, dx, dy| {
            if let Some((start_x, start_y)) = gesture.start_point() {
                move_light(start_x + dx, start_y + dy);
            }
        }
    });
    area.add_controller(drag);
    let click = GestureClick::new();
    click.connect_pressed({
        let move_light = move_light.clone();
        move |_, _, x, y| move_light(x, y)
    });
    area.add_controller(click);
    area
}

fn paint_light_pad(context: &CairoContext, width: i32, height: i32, light: &(f64, f64)) {
    let width = f64::from(width);
    let height = f64::from(height);
    let size = width.min(height);
    if size < 8.0 {
        return;
    }
    let pad_x = (width - size) * 0.5;
    let pad_y = (height - size) * 0.5;
    rounded_rect(context, pad_x, pad_y, size, size, size * 0.10);
    context.set_source_rgba(0.0, 0.0, 0.0, 0.35);
    let _ = context.fill();

    let (lx, ly) = *light;
    let dot = size * 0.072;
    let travel = (size * 0.5 - dot * 2.0).max(1.0);
    context.arc(
        pad_x + size * 0.5 + lx.clamp(-1.0, 1.0) * travel,
        pad_y + size * 0.5 + ly.clamp(-1.0, 1.0) * travel,
        dot,
        0.0,
        std::f64::consts::TAU,
    );
    context.set_source_rgba(0.86, 0.86, 0.88, 1.0);
    let _ = context.fill();
}

// ── Style ───────────────────────────────────────────────────────────

/// One drawn accent around the card in a tile: an outer stroke whose gap
/// spaces it from the stroke before it.
#[derive(Clone, Copy)]
struct OuterStroke {
    thickness: f64,
    color: [f64; 4],
    gap: f64,
}

/// One backing sheet behind the card, for the Stack looks: full-size flat
/// fill, offset and rotated, `center_pivot` fanning diagonally.
#[derive(Clone, Copy)]
struct Backing {
    offset_x: f64,
    offset_y: f64,
    rotation_deg: f64,
    center_pivot: bool,
    color: [f64; 4],
}

struct StyleSpec {
    label: &'static str,
    thickness: f64,
    color: [f64; 4],
    inset: bool,
    liquid: bool,
    outer1: Option<OuterStroke>,
    outer2: Option<OuterStroke>,
    backing1: Option<Backing>,
    backing2: Option<Backing>,
}

/// The twelve frame-style presets the Linux Style grid paints, carrying the
/// same resolved border/outer/backing numbers its swatch painter draws.
/// A static, so the tile closures can hold `'static` references into it.
static STYLE_SPECS: [StyleSpec; 12] = [
    StyleSpec {
        label: "Clean",
        thickness: 0.0,
        color: [1.0, 1.0, 1.0, 0.0],
        inset: false,
        liquid: false,
        outer1: None,
        outer2: None,
        backing1: None,
        backing2: None,
    },
    StyleSpec {
        label: "Frost",
        thickness: 2.5,
        color: [1.0, 1.0, 1.0, 0.55],
        inset: false,
        liquid: true,
        outer1: Some(OuterStroke {
            thickness: 1.0,
            color: [1.0, 1.0, 1.0, 0.95],
            gap: 0.0,
        }),
        outer2: None,
        backing1: None,
        backing2: None,
    },
    StyleSpec {
        label: "Smoke",
        thickness: 2.5,
        color: [0.05, 0.05, 0.07, 0.60],
        inset: false,
        liquid: true,
        outer1: Some(OuterStroke {
            thickness: 1.0,
            color: [1.0, 1.0, 1.0, 0.90],
            gap: 0.0,
        }),
        outer2: None,
        backing1: None,
        backing2: None,
    },
    StyleSpec {
        label: "Prism",
        thickness: 2.5,
        color: [1.0, 1.0, 1.0, 0.10],
        inset: false,
        liquid: true,
        outer1: Some(OuterStroke {
            thickness: 1.0,
            color: [1.0, 1.0, 1.0, 0.55],
            gap: 0.0,
        }),
        outer2: None,
        backing1: None,
        backing2: None,
    },
    StyleSpec {
        label: "Pearl",
        thickness: 3.0,
        color: [1.0, 1.0, 1.0, 0.85],
        inset: true,
        liquid: false,
        outer1: None,
        outer2: None,
        backing1: None,
        backing2: None,
    },
    StyleSpec {
        label: "Slate",
        thickness: 3.0,
        color: [0.0, 0.0, 0.0, 0.85],
        inset: true,
        liquid: false,
        outer1: None,
        outer2: None,
        backing1: None,
        backing2: None,
    },
    StyleSpec {
        label: "Halo",
        thickness: 0.0,
        color: [1.0, 1.0, 1.0, 0.0],
        inset: false,
        liquid: false,
        outer1: Some(OuterStroke {
            thickness: 1.0,
            color: [0.7, 0.7, 0.7, 1.0],
            gap: 2.0,
        }),
        outer2: None,
        backing1: None,
        backing2: None,
    },
    StyleSpec {
        label: "Ink",
        thickness: 3.0,
        color: [0.0, 0.0, 0.0, 1.0],
        inset: false,
        liquid: false,
        outer1: None,
        outer2: None,
        backing1: None,
        backing2: None,
    },
    StyleSpec {
        label: "Offset",
        thickness: 3.0,
        color: [0.0, 0.0, 0.0, 1.0],
        inset: false,
        liquid: false,
        outer1: None,
        outer2: None,
        backing1: Some(Backing {
            offset_x: 28.0,
            offset_y: 28.0,
            rotation_deg: 0.0,
            center_pivot: false,
            color: [0.0, 0.0, 0.0, 1.0],
        }),
        backing2: None,
    },
    StyleSpec {
        label: "Bevel",
        thickness: 1.5,
        color: [0.08, 0.09, 0.12, 0.55],
        inset: false,
        liquid: false,
        outer1: Some(OuterStroke {
            thickness: 0.75,
            color: [1.0, 1.0, 1.0, 0.65],
            gap: 0.0,
        }),
        outer2: None,
        backing1: None,
        backing2: None,
    },
    StyleSpec {
        label: "Tilt",
        thickness: 0.0,
        color: [1.0, 1.0, 1.0, 0.0],
        inset: false,
        liquid: false,
        outer1: None,
        outer2: None,
        backing1: Some(Backing {
            offset_x: 0.0,
            offset_y: 0.0,
            rotation_deg: -2.0,
            center_pivot: true,
            color: [0.75, 0.75, 0.77, 1.0],
        }),
        backing2: None,
    },
    StyleSpec {
        label: "Cascade",
        thickness: 0.0,
        color: [1.0, 1.0, 1.0, 0.0],
        inset: false,
        liquid: false,
        outer1: None,
        outer2: None,
        backing1: Some(Backing {
            offset_x: -10.0,
            offset_y: -28.0,
            rotation_deg: -3.0,
            center_pivot: false,
            color: [0.56, 0.56, 0.58, 1.0],
        }),
        backing2: Some(Backing {
            offset_x: -5.0,
            offset_y: -14.0,
            rotation_deg: -1.5,
            center_pivot: false,
            color: [0.78, 0.78, 0.80, 1.0],
        }),
    },
];

/// Swatch body: a gray card with the preset's backings, glass rim and
/// border strokes, scaled down into the tile like the Linux painter.
fn paint_style_swatch(context: &CairoContext, width: f64, height: f64, spec: &StyleSpec) {
    if width < 2.0 || height < 2.0 {
        return;
    }
    let k = width / 56.0;
    let card_w = 34.0 * k;
    let card_h = 22.0 * k;
    let card_x = (width - card_w) / 2.0;
    let card_y = (height - card_h) / 2.0;
    let card_r = 5.0 * k;
    for backing in [spec.backing1, spec.backing2].into_iter().flatten() {
        let _ = context.save();
        if backing.center_pivot {
            context.translate(
                card_x + card_w / 2.0 + backing.offset_x * 0.32 * k,
                card_y + card_h / 2.0 + backing.offset_y * 0.32 * k,
            );
            context.rotate(backing.rotation_deg.to_radians());
            context.translate(-card_w / 2.0, -card_h / 2.0);
        } else {
            context.translate(
                card_x + card_w + backing.offset_x * 0.32 * k,
                card_y + card_h + backing.offset_y * 0.32 * k,
            );
            context.rotate(backing.rotation_deg.to_radians());
            context.translate(-card_w, -card_h);
        }
        context.set_source_rgba(
            backing.color[0],
            backing.color[1],
            backing.color[2],
            backing.color[3],
        );
        rounded_rect(context, 0.0, 0.0, card_w, card_h, card_r);
        let _ = context.fill();
        let _ = context.restore();
    }
    context.set_source_rgba(0.82, 0.82, 0.84, 1.0);
    rounded_rect(context, card_x, card_y, card_w, card_h, card_r);
    let _ = context.fill();

    // Glass presets: a translucent edge band plus a bright rim, the recipe
    // the Cairo previews scale down to.
    if spec.liquid {
        let [r, g, b, a] = spec.color;
        let band = (spec.thickness * 0.64 * k).max(1.0);
        context.set_source_rgba(r, g, b, a);
        rounded_rect(
            context,
            card_x - band,
            card_y - band,
            card_w + band * 2.0,
            card_h + band * 2.0,
            card_r + band,
        );
        let _ = context.fill();
    }
    if !spec.liquid && spec.inset && spec.thickness > 0.0 {
        // Inset styles paint inside the card edge.
        let t = (spec.thickness * 0.32 * k).max(k);
        let [r, g, b, a] = spec.color;
        context.set_source_rgba(r, g, b, a);
        context.set_line_width(t);
        rounded_rect(
            context,
            card_x + t / 2.0,
            card_y + t / 2.0,
            (card_w - t).max(1.0),
            (card_h - t).max(1.0),
            (card_r - t / 2.0).max(0.0),
        );
        let _ = context.stroke();
    }
    if !spec.liquid && !spec.inset && spec.thickness > 0.0 {
        let t = (spec.thickness * 0.32 * k).max(k);
        let [r, g, b, a] = spec.color;
        context.set_source_rgba(r, g, b, a);
        context.set_line_width(t);
        let e = t / 2.0;
        rounded_rect(
            context,
            card_x - e,
            card_y - e,
            card_w + e * 2.0,
            card_h + e * 2.0,
            card_r + e,
        );
        let _ = context.stroke();
    }
    for outer in [spec.outer1, spec.outer2].into_iter().flatten() {
        let t = (outer.thickness * 0.32 * k).max(k);
        let e = outer.gap * 0.32 * k + t / 2.0;
        context.set_source_rgba(
            outer.color[0],
            outer.color[1],
            outer.color[2],
            outer.color[3],
        );
        context.set_line_width(t);
        rounded_rect(
            context,
            card_x - e,
            card_y - e,
            card_w + e * 2.0,
            card_h + e * 2.0,
            card_r + e,
        );
        let _ = context.stroke();
    }
}

/// The Style disclosure: one tile per frame-style preset, three per row.
/// The chosen tile carries the accent ring; the swatch paints the preset.
pub(super) fn style_disclosure(scale: f64) -> GtkBox {
    let body = GtkBox::new(Orientation::Vertical, px(8.0, scale));
    let grid = Grid::new();
    grid.add_css_class("image-editor-frame-grid");
    grid.set_column_homogeneous(true);
    grid.set_column_spacing(px(6.0, scale).max(0) as u32);
    grid.set_row_spacing(px(8.0, scale).max(0) as u32);
    for (index, spec) in STYLE_SPECS.iter().enumerate() {
        let spec: &'static StyleSpec = spec;
        let cell = GtkBox::new(Orientation::Vertical, px(4.0, scale));
        cell.set_hexpand(false);
        cell.set_halign(Align::Center);
        let preview = DrawingArea::builder()
            .content_width(px(56.0, scale))
            .content_height(px(44.0, scale))
            .build();
        preview.set_can_target(false);
        preview.set_draw_func(move |_, context, width, height| {
            paint_style_swatch(context, f64::from(width), f64::from(height), spec);
        });
        // Linux keeps the swatch alone in the hover-tinted button, with the
        // name below it outside the tint.
        let button = Button::new();
        button.set_has_frame(false);
        button.add_css_class("image-editor-frame-tile");
        button.set_child(Some(&preview));
        button.set_tooltip_text(Some(&t(spec.label)));
        button.set_hexpand(false);
        button.set_halign(Align::Center);
        let label = Label::new(Some(&t(spec.label)));
        label.add_css_class("image-editor-frame-tile-label");
        label.set_xalign(0.5);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        label.set_max_width_chars(9);
        cell.append(&button);
        cell.append(&label);
        grid.attach(&cell, (index % 3) as i32, (index / 3) as i32, 1, 1);
    }
    body.append(&grid);
    disclosure_row("Style", &body, scale)
}

// ── Frame ───────────────────────────────────────────────────────────

/// Generic ratio tiles in the expanded picker: the shape is drawn per
/// aspect, capped into a 44×46 box, and selection paints on the shape.
const FRAME_RATIOS: [(&str, FrameRatio, &str); 9] = [
    ("16:9", FrameRatio::new(16, 9), "Widescreen 16:9 output"),
    ("3:2", FrameRatio::new(3, 2), "Photo 3:2 output"),
    ("4:3", FrameRatio::new(4, 3), "Fullscreen 4:3 output"),
    ("5:4", FrameRatio::new(5, 4), "Large format 5:4 output"),
    ("1:1", FrameRatio::new(1, 1), "Square 1:1 output"),
    ("4:5", FrameRatio::new(4, 5), "Portrait 4:5 output"),
    ("3:4", FrameRatio::new(3, 4), "Portrait 3:4 output"),
    ("2:3", FrameRatio::new(2, 3), "Portrait 2:3 output"),
    ("9:16", FrameRatio::new(9, 16), "Vertical 9:16 output"),
];

/// The tiles of one social section: name, drawn aspect, ratio sublabel.
type SocialTiles = &'static [(&'static str, FrameRatio, &'static str)];

/// One social preset row: name, drawn aspect, and the ratio sublabel.
const SOCIAL_SECTIONS: [(&str, SocialTiles); 4] = [
    (
        "Instagram",
        &[
            ("Post", FrameRatio::new(1, 1), "1:1"),
            ("Portrait", FrameRatio::new(4, 5), "4:5"),
            ("Story", FrameRatio::new(9, 16), "9:16"),
        ],
    ),
    (
        "Twitter",
        &[
            ("Tweet", FrameRatio::new(16, 9), "16:9"),
            ("Cover", FrameRatio::new(3, 1), "3:1"),
        ],
    ),
    (
        "YouTube",
        &[
            ("Banner", FrameRatio::new(16, 9), "16:9"),
            ("Thumbnail", FrameRatio::new(16, 9), "16:9"),
            ("Video", FrameRatio::new(16, 9), "16:9"),
        ],
    ),
    (
        "Pinterest",
        &[
            ("Long", FrameRatio::new(10, 21), "10:21"),
            ("Optimal", FrameRatio::new(2, 3), "2:3"),
            ("Square", FrameRatio::new(1, 1), "1:1"),
        ],
    ),
];

/// Draw one preset's shape, filling and ringing it when selected.
fn paint_ratio_shape(context: &CairoContext, width: f64, height: f64, aspect: f64, selected: bool) {
    if width < 2.0 || height < 2.0 {
        return;
    }
    let max_w = width - 2.0;
    let max_h = height - 2.0;
    let (draw_w, draw_h) = if aspect >= 1.0 {
        (max_w, (max_w / aspect).max(10.0).min(max_h))
    } else {
        ((max_h * aspect).max(10.0).min(max_w), max_h)
    };
    let x = (width - draw_w) * 0.5;
    let y = (height - draw_h) * 0.5;
    context.set_source_rgba(1.0, 1.0, 1.0, if selected { 0.18 } else { 0.13 });
    rounded_rect(context, x + 0.5, y + 0.5, draw_w - 1.0, draw_h - 1.0, 8.0);
    let _ = context.fill();
    if selected {
        context.set_source_rgb(0.75, 0.40, 0.25);
        context.set_line_width(2.0);
        rounded_rect(context, x + 1.5, y + 1.5, draw_w - 3.0, draw_h - 3.0, 7.0);
        let _ = context.stroke();
    }
}

fn frame_tile(
    aspect: f64,
    name: &str,
    sublabel: Option<&str>,
    tooltip: &str,
    selection: &TileSelection,
    index: usize,
    scale: f64,
) -> Button {
    let cell = GtkBox::new(Orientation::Vertical, px(4.0, scale));
    cell.set_hexpand(false);
    cell.set_halign(Align::Center);
    let flag = Rc::new(Cell::new(false));
    let shape = DrawingArea::new();
    let (shape_w, shape_h) = if aspect >= 1.0 {
        (44.0, (44.0 / aspect).max(10.0))
    } else {
        ((46.0 * aspect).max(10.0), 46.0)
    };
    shape.set_content_width(px(shape_w.min(44.0).ceil(), scale));
    shape.set_content_height(px(shape_h.min(46.0).ceil(), scale));
    shape.set_can_target(false);
    shape.add_css_class("image-editor-frame-shape");
    shape.set_draw_func({
        let flag = flag.clone();
        move |_, context, width, height| {
            paint_ratio_shape(
                context,
                f64::from(width),
                f64::from(height),
                aspect,
                flag.get(),
            );
        }
    });
    selection.push(flag.clone(), &shape);
    cell.append(&shape);
    let label = Label::new(Some(name));
    label.add_css_class("image-editor-frame-tile-label");
    label.set_xalign(0.5);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_max_width_chars(8);
    cell.append(&label);
    if let Some(second) = sublabel {
        let bottom = Label::new(Some(second));
        bottom.add_css_class("image-editor-frame-sublabel");
        bottom.set_xalign(0.5);
        bottom.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        bottom.set_max_width_chars(8);
        cell.append(&bottom);
    }
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("image-editor-frame-tile");
    button.set_child(Some(&cell));
    button.set_tooltip_text(Some(tooltip));
    button.set_hexpand(false);
    button.set_halign(Align::Center);
    button.connect_clicked({
        let selection = selection.clone();
        let flag = flag.clone();
        let shape = shape.clone();
        move |_| selection.clicked(index, &flag, &shape)
    });
    button
}

/// One platform section header inside the expanded grid, e.g. Instagram.
fn subheader(text: &str) -> Label {
    let label = Label::new(Some(&t(text)));
    label.add_css_class("image-editor-frame-subheader");
    label.set_xalign(0.0);
    label
}

fn frame_separator() -> GtkBox {
    let rule = GtkBox::new(Orientation::Horizontal, 0);
    rule.add_css_class("image-editor-frame-separator");
    rule
}

/// The Frame disclosure: collapsed W/H inputs plus an expandable ratio
/// grid whose tiles change the aspect ratio rather than a fixed pixel size.
fn frame_disclosure(scale: f64, host: Option<&Rc<EditorHost>>) -> (GtkBox, Rc<dyn Fn()>) {
    let body = GtkBox::new(Orientation::Vertical, px(8.0, scale));

    body.append(&hint(
        "Image presets change the aspect ratio, not a fixed pixel size. W/H shows the export size. Click the selected preset again to reset.",
    ));

    let dim_row = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
    dim_row.add_css_class("image-editor-dim-row");
    let mut dimensions = Vec::new();
    for unit in ["W", "H"] {
        let pill = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
        pill.add_css_class("image-editor-dim-pill");
        pill.set_hexpand(true);
        pill.set_halign(Align::Fill);
        let unit_label = Label::new(Some(unit));
        unit_label.add_css_class("image-editor-dim-unit");
        pill.append(&unit_label);
        let entry = Entry::new();
        entry.add_css_class("image-editor-dim-entry");
        // Read-only rather than insensitive, like the Linux static panel:
        // the entries keep normal entry text and only ever summarize.
        entry.set_editable(false);
        entry.set_focusable(false);
        entry.set_width_chars(4);
        entry.set_max_width_chars(4);
        entry.set_tooltip_text(Some(&t(match unit {
            "W" => "Image export width; select a preset to change the aspect ratio",
            _ => "Image export height; select a preset to change the aspect ratio",
        })));
        pill.append(&entry);
        dimensions.push(entry);
        dim_row.append(&pill);
    }
    let expand_button = Button::with_label("⌄");
    expand_button.set_has_frame(false);
    expand_button.add_css_class("image-editor-frame-expand");
    expand_button.set_tooltip_text(Some(&t("More frame sizes")));
    let revealer = Revealer::builder()
        .transition_type(RevealerTransitionType::SlideDown)
        .transition_duration(120)
        .build();
    revealer.set_hexpand(false);
    revealer.set_halign(Align::Fill);
    revealer.set_reveal_child(false);
    expand_button.connect_clicked({
        let revealer = revealer.clone();
        let button = expand_button.clone();
        move |_| {
            let revealed = revealer.reveals_child();
            revealer.set_reveal_child(!revealed);
            button.set_label(if revealed { "⌄" } else { "⌃" });
        }
    });
    dim_row.append(&expand_button);
    body.append(&dim_row);

    let expanded = GtkBox::new(Orientation::Vertical, px(10.0, scale));
    expanded.add_css_class("image-editor-frame-expanded");
    let selection = TileSelection::new();
    // One selection namespace across both grids, so a generic ratio and a
    // social size can never both carry the ring.
    let tile_index: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let next_index = {
        let tile_index = tile_index.clone();
        move || {
            let index = tile_index.get();
            tile_index.set(index + 1);
            index
        }
    };

    let grid = Grid::new();
    grid.add_css_class("image-editor-frame-grid");
    grid.set_column_homogeneous(true);
    grid.set_column_spacing(px(6.0, scale).max(0) as u32);
    grid.set_row_spacing(px(8.0, scale).max(0) as u32);
    for (row, (ratio, aspect, _tooltip)) in FRAME_RATIOS.into_iter().enumerate() {
        // Static frames name the image ratio they pick, not an export size:
        // the tooltip carries the same line the Linux panel builds.
        let tooltip = tfmt(
            "{name}: {ratio} image aspect ratio",
            &[("name", ratio), ("ratio", ratio)],
        );
        let tile = frame_tile(
            aspect.aspect(),
            ratio,
            None,
            &tooltip,
            &selection,
            next_index(),
            scale,
        );
        wire_frame_tile(&tile, aspect, &selection, host);
        grid.attach(&tile, (row % 3) as i32, (row / 3) as i32, 1, 1);
    }
    expanded.append(&grid);

    for (platform, tiles) in SOCIAL_SECTIONS.into_iter() {
        expanded.append(&frame_separator());
        expanded.append(&subheader(platform));
        let grid = Grid::new();
        grid.add_css_class("image-editor-frame-grid");
        grid.set_column_homogeneous(true);
        grid.set_column_spacing(px(6.0, scale).max(0) as u32);
        grid.set_row_spacing(px(8.0, scale).max(0) as u32);
        for (column, (name, aspect, ratio)) in tiles.iter().enumerate() {
            let tooltip = match (platform, *name) {
                ("Pinterest", "Long") => t("10:21 image aspect ratio; long pins may be cropped in feeds. 2:3 is recommended."),
                ("YouTube", "Banner") => t("16:9 image aspect ratio; banners need at least 2048 x 1152 pixels and may be cropped on some devices."),
                _ => tfmt(
                    "{name}: {ratio} image aspect ratio",
                    &[("name", *name), ("ratio", *ratio)],
                ),
            };
            let tile = frame_tile(
                aspect.aspect(),
                name,
                Some(ratio),
                &tooltip,
                &selection,
                next_index(),
                scale,
            );
            wire_frame_tile(&tile, *aspect, &selection, host);
            grid.attach(&tile, column as i32, 0, 1, 1);
        }
        expanded.append(&grid);
    }
    revealer.set_child(Some(&expanded));
    body.append(&revealer);

    let section = disclosure_row("Frame", &body, scale);
    section.set_sensitive(host.is_some_and(|host| host.has_document()));
    let host = host.cloned();
    let generation = Cell::new(u64::MAX);
    let sync = {
        let section = section.clone();
        Rc::new(move || {
            let Some(host) = &host else {
                section.set_sensitive(false);
                return;
            };
            section.set_sensitive(host.has_document());
            if generation.replace(host.session_generation.get()) != host.session_generation.get() {
                selection.clear();
            }
            if let Some((width, height)) = host.read(|st| {
                let layout = st
                    .background
                    .layout(st.working_image.width(), st.working_image.height());
                (layout.width.ceil() as u32, layout.height.ceil() as u32)
            }) {
                dimensions[0].set_text(&width.to_string());
                dimensions[1].set_text(&height.to_string());
            } else {
                dimensions[0].set_text("");
                dimensions[1].set_text("");
            }
        }) as Rc<dyn Fn()>
    };
    (section, sync)
}

fn wire_frame_tile(
    tile: &Button,
    aspect: FrameRatio,
    selection: &TileSelection,
    host: Option<&Rc<EditorHost>>,
) {
    if let Some(host) = host {
        let host = host.clone();
        let selection = selection.clone();
        tile.connect_clicked(move |_| {
            let aspect = selection.selected.get().map(|_| aspect);
            host.mutate(|st| {
                st.background.frame_aspect = aspect;
                st.background.changed();
            });
        });
    }
}

/// A rounded stroke-and-fill path helper, same corner recipe everywhere.
fn rounded_rect(context: &CairoContext, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    context.new_sub_path();
    context.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    context.close_path();
}
fn background_images_dir() -> Option<PathBuf> {
    crate::video_editor::background::asset_directory()
}

fn wallpaper_thumbs() -> Vec<PathBuf> {
    let Some(dir) = background_images_dir() else {
        return Vec::new();
    };
    let mut thumbs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name.starts_with("wallpaper-thumb-")
                                || name.starts_with("gradient-thumb-")
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    thumbs.sort();
    thumbs
}

/// Cover-fit the thumbnail into the tile, clipped to the tile's rounded
/// corners, the way the Linux panel paints its cairo previews.
fn paint_thumbnail(
    context: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    thumbnail: &Pixbuf,
    radius: f64,
) {
    let width = f64::from(width);
    let height = f64::from(height);
    if width < 1.0 || height < 1.0 {
        return;
    }
    rounded_rect(context, 0.0, 0.0, width, height, radius);
    context.clip();
    let source_width = f64::from(thumbnail.width().max(1));
    let source_height = f64::from(thumbnail.height().max(1));
    let cover = (width / source_width).max(height / source_height);
    let draw_width = source_width * cover;
    let draw_height = source_height * cover;
    context.translate((width - draw_width) * 0.5, (height - draw_height) * 0.5);
    context.scale(cover, cover);
    context.set_source_pixbuf(thumbnail, 0.0, 0.0);
    let _ = context.paint();
}

type ResetAction = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// Every tile and the None row share one selection.
#[derive(Clone)]
struct Selection {
    tiles: Rc<RefCell<Vec<Button>>>,
    none: Button,
    clear_image: ResetAction,
    confirmed: Rc<RefCell<Option<Button>>>,
    confirmed_none: Rc<Cell<bool>>,
}

impl Selection {
    fn refresh_tile(tile: &Button) {
        if let Some(child) = tile.child() {
            child.queue_draw();
        }
    }

    fn clear_tiles(&self) {
        for tile in self.tiles.borrow().iter() {
            tile.remove_css_class("active-background-option");
            Self::refresh_tile(tile);
        }
        self.none.remove_css_class("active-background-option");
        *self.confirmed.borrow_mut() = None;
        self.confirmed_none.set(false);
    }

    fn clear(&self) {
        for tile in self.tiles.borrow().iter() {
            tile.remove_css_class("active-background-option");
            Self::refresh_tile(tile);
        }
        self.none.add_css_class("active-background-option");
        *self.confirmed.borrow_mut() = None;
        self.confirmed_none.set(true);
        self.reset_image();
    }

    fn select(&self, tile: &Button) {
        for other in self.tiles.borrow().iter() {
            other.remove_css_class("active-background-option");
            Self::refresh_tile(other);
        }
        tile.add_css_class("active-background-option");
        Self::refresh_tile(tile);
        self.none.remove_css_class("active-background-option");
    }

    fn confirm(&self, tile: &Button) {
        *self.confirmed.borrow_mut() = Some(tile.clone());
        self.confirmed_none.set(false);
        self.reset_image();
    }

    fn restore(&self) {
        let tile = self.confirmed.borrow().clone();
        if let Some(tile) = tile {
            self.select(&tile);
        } else {
            for tile in self.tiles.borrow().iter() {
                tile.remove_css_class("active-background-option");
                Self::refresh_tile(tile);
            }
            if self.confirmed_none.get() {
                self.none.add_css_class("active-background-option");
            }
        }
    }

    fn reset_image(&self) {
        if let Some(clear) = self.clear_image.borrow().as_ref() {
            clear();
        }
    }
}

/// One wallpaper thumbnail tile: a fixed square preview whose ring is owned by
/// the panel's single selection rule, like the Linux grid.
fn wallpaper_tile(
    scale: f64,
    path: &std::path::Path,
    selection: &Selection,
    host: Option<&Rc<EditorHost>>,
    clear_custom: &ResetAction,
) -> Button {
    let edge = px(TILE_EDGE, scale);
    let preview = DrawingArea::new();
    preview.set_content_width(edge);
    preview.set_content_height(edge);
    preview.set_size_request(edge, edge);
    preview.set_can_target(false);
    let tile = Button::builder()
        .has_frame(false)
        .focusable(false)
        .tooltip_text(
            path.file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or(""),
        )
        .build();
    let thumbnail: Rc<RefCell<Option<Pixbuf>>> = Rc::new(RefCell::new(None));
    preview.set_draw_func({
        let thumbnail = thumbnail.clone();
        let tile = tile.downgrade();
        move |_, context, width, height| {
            if let Some(pixbuf) = thumbnail.borrow().as_ref() {
                let _ = context.save();
                paint_thumbnail(context, width, height, pixbuf, TILE_RADIUS * scale);
                let _ = context.restore();
            }
            if tile
                .upgrade()
                .is_some_and(|tile| tile.has_css_class("active-background-option"))
            {
                let inset = 1.5 * scale;
                context.set_source_rgb(1.0, 0.486, 0.184);
                context.set_line_width(2.0 * scale);
                rounded_rect(
                    context,
                    inset,
                    inset,
                    f64::from(width) - 2.0 * inset,
                    f64::from(height) - 2.0 * inset,
                    TILE_RADIUS * scale - inset,
                );
                let _ = context.stroke();
            }
        }
    });
    // Defer the thumbnail catalog so opening the editor stays responsive.
    {
        let preview = preview.clone();
        let path = path.to_path_buf();
        gtk4::glib::idle_add_local_once(move || {
            let Some(pixbuf) = Pixbuf::from_file_at_scale(&path, edge * 2, edge * 2, true).ok()
            else {
                return;
            };
            *thumbnail.borrow_mut() = Some(pixbuf);
            preview.queue_draw();
        });
    }

    tile.add_css_class("image-editor-wallpaper-tile");
    tile.set_size_request(edge, edge);
    tile.set_hexpand(false);
    tile.set_halign(Align::Center);
    tile.set_valign(Align::Start);
    tile.set_child(Some(&preview));
    tile.connect_clicked({
        let selection = selection.clone();
        let tile = tile.clone();
        let host = host.cloned();
        let clear_custom = clear_custom.clone();
        let preview_path = path.to_path_buf();
        let path = path.with_file_name(
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .replace("-thumb-", "-"),
        );
        move |_| {
            let Some(host) = &host else {
                return;
            };
            if tile.has_css_class("active-background-option") {
                return;
            }
            selection.select(&tile);
            host.load_background(
                path.clone(),
                Some(preview_path.clone()),
                {
                    let clear_custom = clear_custom.clone();
                    let selection = selection.clone();
                    let tile = tile.clone();
                    move |_| {
                        if let Some(clear) = clear_custom.borrow().as_ref() {
                            clear();
                        }
                        selection.select(&tile);
                        selection.confirm(&tile);
                    }
                },
                {
                    let selection = selection.clone();
                    move || selection.restore()
                },
            );
        }
    });
    tile
}

/// The Wallpapers page: the catalog in its own fixed-height viewport, four
/// pinned tiles per row.
fn build_wallpaper_page(
    scale: f64,
    selection: &Selection,
    host: Option<&Rc<EditorHost>>,
    clear_custom: &ResetAction,
) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.set_hexpand(true);

    let grid = Grid::new();
    grid.add_css_class("image-editor-wallpaper-grid");
    // The tile natural sizes stay the grid's: without this a homogeneous
    // column stretches every thumbnail to the panel's width.
    grid.set_hexpand(false);
    grid.set_halign(Align::Fill);
    grid.set_column_homogeneous(true);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        grid.set_column_spacing(px(GRID_GAP, scale).max(0) as u32);
        grid.set_row_spacing(px(GRID_GAP, scale).max(0) as u32);
    }

    for (index, thumb) in wallpaper_thumbs().into_iter().enumerate() {
        let tile = wallpaper_tile(scale, &thumb, selection, host, clear_custom);
        selection.tiles.borrow_mut().push(tile.clone());
        #[allow(clippy::cast_possible_wrap)]
        grid.attach(
            &tile,
            (index % WALLPAPER_COLUMNS) as i32,
            (index / WALLPAPER_COLUMNS) as i32,
            1,
            1,
        );
    }

    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .propagate_natural_width(false)
        .propagate_natural_height(false)
        .height_request(px(CATALOG_HEIGHT, scale))
        .child(&grid)
        .build();
    scroller.add_css_class("image-editor-wallpaper-scroll");
    page.append(&scroller);
    page
}

/// The Custom page: the current fill summarized by its swatch, its kind and a
/// trailing Edit pill. The editors behind it live in the video editor's Custom
/// Wallpaper popover on Linux, so this row only ever summarizes.
fn build_custom_page(scale: f64) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.set_hexpand(true);

    let row = GtkBox::new(Orientation::Horizontal, px(10.0, scale));
    row.add_css_class("image-editor-bg-custom-row");
    row.set_hexpand(true);

    let swatch = GtkBox::new(Orientation::Horizontal, 0);
    swatch.set_size_request(px(28.0, scale), px(28.0, scale));
    swatch.set_valign(Align::Center);
    swatch.add_css_class("image-editor-bg-custom-swatch");

    let kind = Label::new(Some(&t("Color")));
    kind.set_xalign(0.0);
    kind.set_hexpand(true);
    kind.set_valign(Align::Center);
    kind.add_css_class("image-editor-bg-custom-kind");

    let edit = Button::with_label(&t("Edit"));
    edit.add_css_class("image-editor-bg-custom-edit");
    edit.set_has_frame(false);
    edit.set_valign(Align::Center);
    edit.set_sensitive(false);
    edit.set_tooltip_text(Some(&t("Edit custom fill")));

    row.append(&swatch);
    row.append(&kind);
    row.append(&edit);
    page.append(&row);
    page
}

/// The Image page: one full-width chooser row carrying a preview chip, the
/// picker label and the folder affordance.
fn build_image_page(
    scale: f64,
    host: Option<&Rc<EditorHost>>,
    selection: &Selection,
    clear_custom: &ResetAction,
    window: &gtk4::ApplicationWindow,
) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, px(6.0, scale));
    page.set_hexpand(true);

    let row = Button::new();
    row.add_css_class("image-editor-bg-image-row");
    row.set_has_frame(false);
    row.set_hexpand(true);
    row.set_sensitive(host.is_some());

    let inner = GtkBox::new(Orientation::Horizontal, px(10.0, scale));
    let thumb = DrawingArea::new();
    thumb.set_content_width(px(28.0, scale));
    thumb.set_content_height(px(28.0, scale));
    thumb.set_valign(Align::Center);
    thumb.add_css_class("image-editor-bg-custom-swatch");
    thumb.add_css_class("image-editor-bg-image-thumb");
    let label = Label::new(Some(&t("Select image...")));
    label.set_hexpand(true);
    label.set_xalign(0.0);
    label.set_valign(Align::Center);
    let icon = Image::from_icon_name(icon_names::shipped::FOLDER_OPEN_REGULAR);
    icon.set_pixel_size(px(13.0, scale));
    icon.set_valign(Align::Center);
    icon.add_css_class("image-editor-bg-image-icon");
    inner.append(&thumb);
    inner.append(&label);
    inner.append(&icon);
    row.set_child(Some(&inner));
    let preview: Rc<RefCell<Option<Pixbuf>>> = Rc::new(RefCell::new(None));
    thumb.set_draw_func({
        let preview = preview.clone();
        move |_, context, width, height| {
            if let Some(image) = preview.borrow().as_ref() {
                paint_thumbnail(context, width, height, image, 7.0 * scale);
            }
        }
    });
    *selection.clear_image.borrow_mut() = Some({
        let label = label.clone();
        let thumb = thumb.clone();
        let preview = preview.clone();
        Rc::new(move || {
            label.set_text(&t("Select image..."));
            *preview.borrow_mut() = None;
            thumb.queue_draw();
        })
    });
    if let Some(host) = host {
        let host = host.clone();
        let window = window.clone();
        let selection = selection.clone();
        let clear_custom = clear_custom.clone();
        row.connect_clicked(move |_| {
            let session = host.session_generation.get();
            let chooser = gtk4::FileChooserNative::new(
                Some(&t("Select image")),
                Some(&window),
                gtk4::FileChooserAction::Open,
                Some(&t("Open")),
                Some(&t("Cancel")),
            );
            let filter = gtk4::FileFilter::new();
            filter.set_name(Some(&t("Images")));
            for pattern in ["*.png", "*.jpg", "*.jpeg", "*.webp"] {
                filter.add_pattern(pattern);
            }
            chooser.add_filter(&filter);
            let host = host.clone();
            let selection = selection.clone();
            let clear_custom = clear_custom.clone();
            let label = label.clone();
            let thumb = thumb.clone();
            let preview = preview.clone();
            chooser.connect_response(move |chooser, response| {
                if response == gtk4::ResponseType::Accept {
                    if let Some(path) = chooser.file().and_then(|file| file.path()) {
                        if host.session_generation.get() == session {
                            let selection = selection.clone();
                            let clear_custom = clear_custom.clone();
                            let label = label.clone();
                            let thumb = thumb.clone();
                            let preview = preview.clone();
                            let failure_selection = selection.clone();
                            let name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            host.load_background(
                                path,
                                None,
                                move |image| {
                                    if let Some(clear) = clear_custom.borrow().as_ref() {
                                        clear();
                                    }
                                    selection.clear_tiles();
                                    label.set_text(&name);
                                    let small = image::imageops::thumbnail(image, 56, 56);
                                    let (width, height) = small.dimensions();
                                    let bytes = gtk4::glib::Bytes::from_owned(small.into_raw());
                                    *preview.borrow_mut() = Some(Pixbuf::from_bytes(
                                        &bytes,
                                        gtk4::gdk_pixbuf::Colorspace::Rgb,
                                        true,
                                        8,
                                        width as i32,
                                        height as i32,
                                        (width * 4) as i32,
                                    ));
                                    thumb.queue_draw();
                                },
                                move || failure_selection.restore(),
                            );
                        }
                    }
                }
                chooser.hide();
            });
            chooser.show();
        });
    }
    page.append(&row);
    page
}

/// One pill in the fill-source tray, reusing the video editor's Background tab
/// look so the two panels read identically.
fn source_tab(label: &str) -> ToggleButton {
    let button = ToggleButton::with_label(&t(label));
    button.add_css_class("image-editor-bg-tab");
    button.set_has_frame(false);
    button.set_hexpand(true);
    button
}

fn build_source_pages(
    scale: f64,
    selection: &Selection,
    host: Option<&Rc<EditorHost>>,
    clear_custom: &ResetAction,
    column: &GtkBox,
    window: &gtk4::ApplicationWindow,
) -> Stack {
    let pages = Stack::new();
    pages.set_hhomogeneous(false);
    pages.set_vhomogeneous(false);
    pages.set_hexpand(true);
    pages.add_named(
        &build_wallpaper_page(scale, selection, host, clear_custom),
        Some("wallpapers"),
    );
    if let Some(host) = host {
        let custom = crate::video_editor::background::Background::default();
        let on_selected = {
            let selection = selection.clone();
            Rc::new(move || {
                selection.clear_tiles();
                selection.reset_image();
            })
        };
        let page = crate::video_editor::custom_fill::build_custom_page(
            scale,
            column,
            &custom,
            on_selected,
        );
        *clear_custom.borrow_mut() = Some(page.clear);
        let state = custom.state();
        let host = host.clone();
        custom.connect(Rc::new(move || {
            if let Some(painter) = state.custom_fill() {
                host.cancel_background_load();
                host.mutate(|st| {
                    st.background.set_fill(Fill::Custom(painter));
                    st.background.changed();
                });
            }
        }));
        pages.add_named(&page.widget, Some("custom"));
    } else {
        pages.add_named(&build_custom_page(scale), Some("custom"));
    }
    pages.add_named(
        &build_image_page(scale, host, selection, clear_custom, window),
        Some("image"),
    );
    pages.set_visible_child_name("wallpapers");
    pages
}

/// ── Shared Appearance panel ─────────────────────────────────────────
///
/// The Linux builder is one function for both editors: the static editor
/// attaches its Transform extension, Motion shows the same groups without
/// it.
pub(super) fn appearance_panel(
    scale: f64,
    include_transform: bool,
    host: Option<&Rc<EditorHost>>,
    column: &GtkBox,
    window: &gtk4::ApplicationWindow,
) -> (GtkBox, Rc<dyn Fn()>) {
    let body = GtkBox::new(Orientation::Vertical, px(12.0, scale));
    body.add_css_class("image-editor-inspector-body");

    let title = Label::new(Some(&t("Appearance")));
    title.add_css_class("image-editor-inspector-title");
    title.set_xalign(0.0);
    body.append(&title);

    let (background, sync) = background_card(scale, host, column, window);
    body.append(&background);
    body.append(&export_quality(scale));
    if include_transform {
        body.append(&transform_disclosure(scale));
    }
    body.append(&shadow_disclosure(scale));
    body.append(&style_disclosure(scale));
    let (frame, frame_sync) = frame_disclosure(scale, host);
    body.append(&frame);
    let sync = Rc::new(move || {
        sync();
        frame_sync();
    });
    (body, sync)
}

/// The Background card: the fill-source tray and the shared sliders, the
/// Linux panel's opening group. `selection` owns the one ring both here and
/// the catalog carry.
fn background_card(
    scale: f64,
    host: Option<&Rc<EditorHost>>,
    column: &GtkBox,
    window: &gtk4::ApplicationWindow,
) -> (GtkBox, Rc<dyn Fn()>) {
    let section = section_card("Background", scale);

    // "None" clears the fill outright. It is not a fourth source tab, so it
    // sits under the tray on every tab rather than inside it, and it carries
    // the ring exactly while no tile is chosen.
    let none_row = Button::with_label(&t("None"));
    none_row.add_css_class("image-editor-bg-none-row");
    none_row.set_has_frame(false);
    none_row.set_hexpand(true);
    none_row.set_halign(Align::Fill);
    none_row.add_css_class("active-background-option");
    let selection = Selection {
        tiles: Rc::new(RefCell::new(Vec::new())),
        none: none_row.clone(),
        clear_image: Rc::new(RefCell::new(None)),
        confirmed: Rc::new(RefCell::new(None)),
        confirmed_none: Rc::new(Cell::new(true)),
    };
    let clear_custom: ResetAction = Rc::new(RefCell::new(None));
    none_row.connect_clicked({
        let selection = selection.clone();
        let host = host.cloned();
        let clear_custom = clear_custom.clone();
        move |_| {
            if let Some(clear) = clear_custom.borrow().as_ref() {
                clear();
            }
            selection.clear();
            if let Some(host) = &host {
                host.cancel_background_load();
                host.mutate(|st| {
                    st.background.fill = Fill::None;
                    st.background.changed();
                });
            }
        }
    });

    // Fill-source tabs: one source at a time, matching the Linux panel.
    let tabs = GtkBox::new(Orientation::Horizontal, 0);
    tabs.add_css_class("image-editor-bg-tabs");
    tabs.set_hexpand(true);
    tabs.set_homogeneous(true);
    let pages = build_source_pages(scale, &selection, host, &clear_custom, column, window);
    let mut first_tab: Option<ToggleButton> = None;
    for (label, page_name, active) in [
        ("Wallpapers", "wallpapers", true),
        ("Custom", "custom", false),
        ("Image", "image", false),
    ] {
        let tab = source_tab(label);
        if let Some(first) = &first_tab {
            tab.set_group(Some(first));
        } else {
            first_tab = Some(tab.clone());
        }
        tab.set_active(active);
        tab.connect_toggled({
            let pages = pages.clone();
            move |button| {
                if button.is_active() {
                    pages.set_visible_child_name(page_name);
                }
            }
        });
        tabs.append(&tab);
    }
    section.append(&tabs);
    section.append(&none_row);
    section.append(&pages);

    // Padding, blur, radius and noise are shared by every source tab: they
    // sit inside the card, right under the open page, in the Linux panel's
    // row order.
    let options = GtkBox::new(Orientation::Vertical, px(8.0, scale));
    options.add_css_class("image-editor-bg-options");
    let padding = FilledSlider::new(&t("Padding"), 0.0, 80.0, 0.0, 1.0, 0, scale);
    let blur =
        FilledSlider::new(&t("Background blur"), 0.0, 100.0, 0.0, 1.0, 0, scale).with_suffix("%");
    let radius = FilledSlider::new(&t("Border Radius"), 0.0, 40.0, 0.0, 1.0, 0, scale);
    let noise =
        FilledSlider::new(&t("Background noise"), 0.0, 100.0, 0.0, 1.0, 0, scale).with_suffix("%");
    for (index, slider) in [&padding, &blur, &radius, &noise].into_iter().enumerate() {
        options.append(&slider.widget());
        if let Some(host) = host {
            let host = host.clone();
            slider.on_change(Rc::new(move |value| {
                host.mutate(|st| {
                    match index {
                        0 => st.background.set_padding(value),
                        1 => st.background.blur = value / 100.0,
                        2 => st.background.radius = value,
                        _ => st.background.noise = value / 100.0,
                    }
                    st.background.changed();
                });
            }));
        }
    }
    section.append(&options);
    let generation = Cell::new(u64::MAX);
    let host = host.cloned();
    let sync = {
        let section = section.clone();
        Rc::new(move || {
            let Some(host) = &host else {
                section.set_sensitive(false);
                return;
            };
            section.set_sensitive(host.has_document());
            if generation.replace(host.session_generation.get()) != host.session_generation.get() {
                if let Some(clear) = clear_custom.borrow().as_ref() {
                    clear();
                }
                selection.clear();
                for slider in [&padding, &blur, &radius, &noise] {
                    slider.set_value(0.0);
                }
            }
            if let Some(value) = host.read(|st| st.background.padding) {
                padding.set_value(value);
            }
        }) as Rc<dyn Fn()>
    };
    (section, sync)
}

/// The Watermark tool's Motion panel: a picture picker, the Remove action
/// and the placement dials. The shell keeps its local image and slider
/// values; nothing a drawing backend could read exists on this platform
/// yet.
pub(super) fn watermark_panel(scale: f64) -> GtkBox {
    let card = section_card("Watermark", scale);
    let image: Rc<RefCell<Option<Pixbuf>>> = Rc::new(RefCell::new(None));

    // The picker row: preview chip, chosen name and the folder affordance.
    let picker = Button::new();
    picker.add_css_class("image-editor-bg-image-row");
    picker.set_has_frame(false);
    picker.set_hexpand(true);
    let default_tooltip = t("Choose Watermark Image");
    picker.set_tooltip_text(Some(&default_tooltip));
    let inner = GtkBox::new(Orientation::Horizontal, px(10.0, scale));
    let thumbnail = DrawingArea::new();
    thumbnail.set_content_width(px(28.0, scale));
    thumbnail.set_content_height(px(28.0, scale));
    thumbnail.set_valign(Align::Center);
    thumbnail.set_can_target(false);
    thumbnail.add_css_class("image-editor-bg-custom-swatch");
    thumbnail.add_css_class("image-editor-bg-image-thumb");
    {
        let image = image.clone();
        thumbnail.set_draw_func(move |_, context, width, height| {
            let image = image.borrow().clone();
            let Some(image) = image else {
                return;
            };
            let width = f64::from(width);
            let height = f64::from(height);
            if width < 1.0 || height < 1.0 {
                return;
            }
            let source_w = f64::from(image.width().max(1));
            let source_h = f64::from(image.height().max(1));
            let fit = (width / source_w).min(height / source_h);
            let draw_w = source_w * fit;
            let draw_h = source_h * fit;
            rounded_rect(context, 0.0, 0.0, width, height, 7.0);
            context.clip();
            context.translate((width - draw_w) * 0.5, (height - draw_h) * 0.5);
            context.scale(fit, fit);
            context.set_source_pixbuf(&image, 0.0, 0.0);
            let _ = context.paint();
        });
    }
    let name = Label::new(Some(&t("Select image...")));
    name.set_hexpand(true);
    name.set_xalign(0.0);
    name.set_valign(Align::Center);
    name.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    let icon = gtk4::Image::from_icon_name(icon_names::shipped::FOLDER_OPEN_REGULAR);
    icon.set_pixel_size(px(13.0, scale));
    icon.set_valign(Align::Center);
    icon.add_css_class("image-editor-bg-image-icon");
    inner.append(&thumbnail);
    inner.append(&name);
    inner.append(&icon);
    picker.set_child(Some(&inner));
    card.append(&picker);

    // The chooser hands back a thumbnail for the chip and names the file; a
    // drawing backend reads it later.
    picker.connect_clicked({
        let image = image.clone();
        let thumbnail = thumbnail.clone();
        let name = name.clone();
        let picker = picker.clone();
        move |_| {
            let chooser = gtk4::FileChooserNative::new(
                Some(&t("Choose Watermark Image")),
                None::<&gtk4::Window>,
                gtk4::FileChooserAction::Open,
                Some(&t("Choose")),
                Some(&t("Cancel")),
            );
            let filter = gtk4::FileFilter::new();
            filter.set_name(Some(&t("Images")));
            for pattern in ["*.png", "*.jpg", "*.jpeg", "*.webp"] {
                filter.add_pattern(pattern);
            }
            chooser.add_filter(&filter);
            let image = image.clone();
            let thumbnail = thumbnail.clone();
            let name = name.clone();
            let picker = picker.clone();
            chooser.connect_response(move |dialog, response| {
                if response != gtk4::ResponseType::Accept {
                    return;
                }
                let Some(path) = dialog.file().and_then(|file| file.path()) else {
                    return;
                };
                let decoded = Pixbuf::from_file_at_scale(&path, 128, 128, true).ok();
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                *image.borrow_mut() = decoded;
                name.set_text(&file_name);
                picker.set_tooltip_text(Some(&file_name));
                thumbnail.queue_draw();
                dialog.destroy();
            });
            chooser.show();
        }
    });

    let remove = inline_button(&t("Remove"), scale);
    let default_tooltip = default_tooltip.clone();
    remove.connect_clicked({
        let image = image.clone();
        let thumbnail = thumbnail.clone();
        let name = name.clone();
        let picker = picker.clone();
        move |_| {
            *image.borrow_mut() = None;
            name.set_text(&t("Select image..."));
            picker.set_tooltip_text(Some(&default_tooltip));
            thumbnail.queue_draw();
        }
    });
    card.append(&remove);

    // The four placement dials, in the Linux panel's order: size, inset,
    // then position, all normalized against the card.
    card.append(
        &FilledSlider::new(&t("Watermark size"), 2.0, 80.0, 18.0, 1.0, 0, scale)
            .with_suffix("%")
            .widget(),
    );
    card.append(
        &FilledSlider::new(&t("Watermark inset"), 0.0, 45.0, 4.0, 1.0, 0, scale)
            .with_suffix("%")
            .widget(),
    );
    card.append(
        &FilledSlider::new(&t("Watermark position X"), 0.0, 100.0, 90.0, 1.0, 0, scale)
            .with_suffix("%")
            .widget(),
    );
    card.append(
        &FilledSlider::new(&t("Watermark position Y"), 0.0, 100.0, 90.0, 1.0, 0, scale)
            .with_suffix("%")
            .widget(),
    );
    card
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::state::EditorState;
    use image::{Rgba, RgbaImage};
    use std::sync::Arc;

    fn collect_class(widget: &gtk4::Widget, class: &str, found: &mut Vec<gtk4::Widget>) {
        if widget.has_css_class(class) {
            found.push(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            collect_class(&current, class, found);
            child = current.next_sibling();
        }
    }

    #[test]
    fn frame_tiles_update_dimensions_reset_and_follow_new_documents() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
            let mut document =
                EditorState::new(RgbaImage::from_pixel(100, 80, Rgba([240, 0, 0, 255])));
            document.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
                1,
                1,
                Rgba([0, 0, 255, 255]),
            )));
            document.background.padding = 40.0;
            *host.editor.borrow_mut() = Some(document);
            let (section, sync) = frame_disclosure(1.0, Some(&host));
            sync();
            let mut tiles = Vec::new();
            let mut entries = Vec::new();
            collect_class(section.upcast_ref(), "image-editor-frame-tile", &mut tiles);
            collect_class(section.upcast_ref(), "image-editor-dim-entry", &mut entries);
            let square = tiles[4].clone().downcast::<Button>().unwrap();
            let width = entries[0].clone().downcast::<Entry>().unwrap();
            let height = entries[1].clone().downcast::<Entry>().unwrap();
            assert_eq!(
                (width.text().as_str(), height.text().as_str()),
                ("120", "100")
            );

            square.emit_clicked();
            sync();
            assert_eq!(
                host.read(|st| st.background.frame_aspect),
                Some(Some(FrameRatio::new(1, 1)))
            );
            assert_eq!(
                (width.text().as_str(), height.text().as_str()),
                ("120", "120")
            );

            square.emit_clicked();
            sync();
            assert_eq!(host.read(|st| st.background.frame_aspect), Some(None));
            assert_eq!(
                (width.text().as_str(), height.text().as_str()),
                ("120", "100")
            );

            square.emit_clicked();
            *host.editor.borrow_mut() = Some(EditorState::new(RgbaImage::new(60, 40)));
            host.session_generation
                .set(host.session_generation.get() + 1);
            sync();
            assert_eq!(
                (width.text().as_str(), height.text().as_str()),
                ("60", "40")
            );
            square.emit_clicked();
            assert_eq!(
                host.read(|st| st.background.frame_aspect),
                Some(Some(FrameRatio::new(1, 1)))
            );
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn every_frame_tile_exports_its_labeled_ratio_with_or_without_a_fill() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
            *host.editor.borrow_mut() = Some(EditorState::new(RgbaImage::from_pixel(
                101,
                61,
                Rgba([240, 0, 0, 255]),
            )));
            let (section, sync) = frame_disclosure(1.0, Some(&host));
            sync();
            let mut tiles = Vec::new();
            let mut entries = Vec::new();
            collect_class(section.upcast_ref(), "image-editor-frame-tile", &mut tiles);
            collect_class(section.upcast_ref(), "image-editor-dim-entry", &mut entries);
            let width = entries[0].clone().downcast::<Entry>().unwrap();
            let height = entries[1].clone().downcast::<Entry>().unwrap();
            let presets: Vec<_> =
                FRAME_RATIOS
                    .iter()
                    .map(|(label, ratio, _)| (*label, *ratio))
                    .chain(SOCIAL_SECTIONS.iter().flat_map(|(_, items)| {
                        items.iter().map(|(_, ratio, label)| (*label, *ratio))
                    }))
                    .collect();
            assert_eq!(tiles.len(), presets.len());
            for fill in [false, true] {
                host.mutate(|st| {
                    st.background.fill = if fill {
                        Fill::Image(Arc::new(RgbaImage::from_pixel(
                            1,
                            1,
                            Rgba([0, 0, 255, 255]),
                        )))
                    } else {
                        Fill::None
                    };
                    st.background.padding = 1.0;
                });
                for (tile, (label, ratio)) in tiles.iter().zip(&presets) {
                    let (a, b) = label.split_once(':').unwrap();
                    assert_eq!(
                        (a.parse::<u32>().unwrap(), b.parse::<u32>().unwrap()),
                        (ratio.width, ratio.height)
                    );
                    tile.clone().downcast::<Button>().unwrap().emit_clicked();
                    sync();
                    host.read(|st| {
                        assert_eq!(st.background.frame_aspect, Some(*ratio));
                        let image = st.to_export_image().unwrap();
                        assert_eq!(
                            u64::from(image.width()) * u64::from(ratio.height),
                            u64::from(image.height()) * u64::from(ratio.width),
                            "{label}"
                        );
                        assert_eq!(width.text().as_str(), image.width().to_string());
                        assert_eq!(height.text().as_str(), image.height().to_string());
                        let layout = st.background.layout(101, 61);
                        assert_eq!(
                            *image
                                .get_pixel(layout.image_x as u32 + 50, layout.image_y as u32 + 30),
                            Rgba([240, 0, 0, 255])
                        );
                    })
                    .unwrap();
                }
            }
        }) else {
            return;
        };
    }

    #[test]
    fn social_frame_presets_match_supported_formats() {
        let actual: Vec<_> = SOCIAL_SECTIONS
            .iter()
            .flat_map(|(group, tiles)| {
                tiles
                    .iter()
                    .map(move |(name, ratio, _)| (*group, *name, *ratio))
            })
            .collect();
        assert_eq!(
            actual,
            vec![
                ("Instagram", "Post", FrameRatio::new(1, 1)),
                ("Instagram", "Portrait", FrameRatio::new(4, 5)),
                ("Instagram", "Story", FrameRatio::new(9, 16)),
                ("Twitter", "Tweet", FrameRatio::new(16, 9)),
                ("Twitter", "Cover", FrameRatio::new(3, 1)),
                ("YouTube", "Banner", FrameRatio::new(16, 9)),
                ("YouTube", "Thumbnail", FrameRatio::new(16, 9)),
                ("YouTube", "Video", FrameRatio::new(16, 9)),
                ("Pinterest", "Long", FrameRatio::new(10, 21)),
                ("Pinterest", "Optimal", FrameRatio::new(2, 3)),
                ("Pinterest", "Square", FrameRatio::new(1, 1)),
            ]
        );
    }

    #[test]
    fn wallpaper_selection_is_immediate_and_can_restore_previous_selection() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
            *host.editor.borrow_mut() = Some(EditorState::new(RgbaImage::new(100, 80)));
            let selection = Selection {
                tiles: Rc::new(RefCell::new(Vec::new())),
                none: Button::new(),
                clear_image: Rc::new(RefCell::new(None)),
                confirmed: Rc::new(RefCell::new(None)),
                confirmed_none: Rc::new(Cell::new(true)),
            };
            selection.none.add_css_class("active-background-option");
            let clear_custom = Rc::new(RefCell::new(None));
            let tile = wallpaper_tile(
                1.0,
                std::path::Path::new("missing-thumb-wallpaper.jpg"),
                &selection,
                Some(&host),
                &clear_custom,
            );
            selection.tiles.borrow_mut().push(tile.clone());
            tile.emit_clicked();
            assert!(tile.has_css_class("active-background-option"));
            assert!(!selection.none.has_css_class("active-background-option"));
            assert!(host.background_loading());
            host.cancel_background_load();
            selection.restore();
            assert!(!host.background_loading());
            assert!(!tile.has_css_class("active-background-option"));
            assert!(selection.none.has_css_class("active-background-option"));
            assert!(matches!(
                host.read(|st| st.background.fill.clone()),
                Some(Fill::None)
            ));
        }) else {
            return;
        };
    }
}
