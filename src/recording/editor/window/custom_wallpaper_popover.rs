// The Custom Wallpaper popover.
//
// Reached from the Edit pill on the Background panel's Custom page. It opens
// beside the sidebar rather than centered on the window: the controls belong
// next to the row that summarizes the fill, and a centered dialog would hide
// both that row and the video being changed.
//
// Two sub-tabs. Color is the field / hue bar / hex row from the reference.
// Gradient is the full multi-stop editor. Both write straight into
// `VideoEditState.background` through `on_change` (the editor's `ping`),
// which repaints the video preview and runs the panel refresh.

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, ColorChooserDialog, DrawingArea, Entry, Grid,
    GestureClick, GestureDrag, Image, Label, Orientation, Popover, ToggleButton, Widget, Window,
};

use crate::i18n::t;
use crate::recording::editor::model::background_render::render_gradient;
use crate::recording::editor::model::{
    GradientStop, VideoBackground, VideoEditState, VideoGradient, MAX_GRADIENT_STOPS,
    MIN_GRADIENT_STOPS,
};

/// Corner radius shared by the color field, the hue bar, and the hex row, so
/// the popover reads as one set of stacked surfaces.
const FIELD_RADIUS: f64 = 8.0;
/// Radius of a hue-bar handle and a gradient stop handle.
const HANDLE_RADIUS: f64 = 8.0;

/// Paints `rounded_rect` and clips to it.
fn fill_rounded(
    cr: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    r: f64,
    color: (u8, u8, u8),
) {
    rounded_rect(cr, x, y, w, h, r);
    cr.set_source_rgb(
        color.0 as f64 / 255.0,
        color.1 as f64 / 255.0,
        color.2 as f64 / 255.0,
    );
    let _ = cr.fill();
}

fn stroke_rounded(
    cr: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    r: f64,
    light: bool,
) {
    rounded_rect(cr, x + 0.5, y + 0.5, (w - 1.0).max(1.0), (h - 1.0).max(1.0), r);
    if light {
        cr.set_source_rgba(0.11, 0.13, 0.16, 0.20);
    } else {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.30);
    }
    cr.set_line_width(1.0);
    let _ = cr.stroke();
}

fn rounded_rect(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
}

// ── Color model ──
// The reference picker is the standard three-part one: a saturation/value
// plane for the current hue, a hue bar underneath, and a hex readout. Hue
// comes from the bar, saturation from the plane's horizontal axis, and value
// from its vertical axis.

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let h = h.rem_euclid(1.0) * 6.0;
    let sector = h.floor();
    let f = h - sector;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match sector as i32 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let to_u8 = |c: f64| (c * 255.0).round().clamp(0.0, 255.0) as u8;
    (to_u8(r), to_u8(g), to_u8(b))
}

/// Hue of an RGB color, 0..=1. Used to place the handle on the bar when the
/// color arrived from somewhere other than the bar (a preset, a project file).
fn rgb_to_hue(r: u8, g: u8, b: u8) -> f64 {
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if delta <= f64::EPSILON {
        return 0.0;
    }
    let hue = if (max - r).abs() < f64::EPSILON {
        (g - b) / delta
    } else if (max - g).abs() < f64::EPSILON {
        2.0 + (b - r) / delta
    } else {
        4.0 + (r - g) / delta
    };
    (hue / 6.0).rem_euclid(1.0)
}

fn hex_string(color: (u8, u8, u8)) -> String {
    format!("#{:02X}{:02X}{:02X}", color.0, color.1, color.2)
}

/// Parse `#RGB`, `#RRGGBB`, or the same without the `#`. Returns None on
/// anything else so a partial edit does not commit a wrong color.
fn parse_hex(text: &str) -> Option<(u8, u8, u8)> {
    let digits: String = text
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '#')
        .collect();
    let nibble = |c: char| c.to_digit(16).map(|d| d as u8);
    match digits.len() {
        3 => {
            let mut out = [0u8; 3];
            for (index, c) in digits.chars().enumerate() {
                let d = nibble(c)?;
                // `#ABC` means `#AABBCC`.
                out[index] = d * 17;
            }
            Some((out[0], out[1], out[2]))
        }
        6 => {
            let bytes = digits.as_bytes();
            let mut out = [0u8; 3];
            for slot in 0..3 {
                let i = slot * 2;
                let Some(hi) = nibble(bytes[i] as char) else {
                    return None;
                };
                let Some(lo) = nibble(bytes[i + 1] as char) else {
                    return None;
                };
                out[slot] = hi * 16 + lo;
            }
            Some((out[0], out[1], out[2]))
        }
        _ => None,
    }
}

fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let (rf, gf, bf) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;
    if delta <= f64::EPSILON {
        return (0.0, 0.0, max);
    }
    let hue = rgb_to_hue(r, g, b);
    (hue, delta / max, max)
}

// ── Popover ──

pub(super) fn build_custom_wallpaper_popover(
    anchor: &Button,
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) -> Popover {
    let popover = Popover::new();
    popover.add_css_class("recording-editor-custom-popover");
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    // Opens to the left of the Edit pill, so it floats over the video stage
    // and never covers the sidebar row it is editing.
    popover.set_position(gtk4::PositionType::Left);
    popover.set_parent(anchor);

    // A single "tell everyone to redraw" hook. Mutations call `notify`, which
    // pings the editor and then repaints both popover pages, so the Color tab
    // and the Gradient tab never disagree about the current fill.
    let redraw: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let notify: Rc<dyn Fn()> = {
        let on_change = on_change.clone();
        let redraw = redraw.clone();
        Rc::new(move || {
            on_change();
            if let Some(repaint) = redraw.borrow().as_ref() {
                repaint();
            }
        })
    };

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("recording-editor-custom-body");

    // ── Header: title + close, like the reference. ──
    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.add_css_class("recording-editor-custom-header");
    let title = Label::new(Some(&t("Custom Wallpaper")));
    title.add_css_class("recording-editor-custom-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    let close = Button::new();
    close.add_css_class("recording-editor-custom-close");
    close.set_has_frame(false);
    close.set_tooltip_text(Some(&t("Close")));
    let close_icon = Image::from_icon_name("window-close-symbolic");
    close_icon.set_pixel_size(13);
    close.set_child(Some(&close_icon));
    {
        let popover = popover.clone();
        close.connect_clicked(move |_| popover.popdown());
    }
    header.append(&title);
    header.append(&close);
    root.append(&header);

    // ── Sub-tabs. ──
    let tabs = GtkBox::new(Orientation::Horizontal, 0);
    tabs.add_css_class("recording-editor-bg-tabs");
    tabs.set_homogeneous(true);
    let make_tab = |label: &str| {
        let b = ToggleButton::with_label(label);
        b.add_css_class("recording-editor-bg-tab");
        b.set_has_frame(false);
        b.set_hexpand(true);
        b
    };
    let color_tab = make_tab(&t("Color"));
    let gradient_tab = make_tab(&t("Gradient"));
    color_tab.set_group(Some(&gradient_tab));
    color_tab.set_active(true);
    tabs.append(&color_tab);
    tabs.append(&gradient_tab);
    root.append(&tabs);

    let pages = GtkBox::new(Orientation::Vertical, 0);
    pages.set_hexpand(true);
    root.append(&pages);

    let color_page = build_color_page(&state, &notify);
    let gradient_page = build_gradient_page(&state, &notify);
    pages.append(&color_page.widget);
    pages.append(&gradient_page.widget);

    // Switching tabs is a view change and must not write a fill, or opening
    // Gradient would silently replace a color the swatch is showing.
    let set_page = {
        let color_page = color_page.clone();
        let gradient_page = gradient_page.clone();
        let color_tab = color_tab.clone();
        let gradient_tab = gradient_tab.clone();
        Rc::new(move |is_gradient: bool| {
            color_page.widget.set_visible(!is_gradient);
            gradient_page.widget.set_visible(is_gradient);
            color_tab.set_active(!is_gradient);
            gradient_tab.set_active(is_gradient);
        })
    };
    color_tab.connect_toggled({
        let set_page = set_page.clone();
        move |b| {
            if b.is_active() {
                set_page(false);
            }
        }
    });
    gradient_tab.connect_toggled(move |b| {
        if b.is_active() {
            set_page(true);
        }
    });

    let refresh: Rc<dyn Fn()> = {
        let color_page = color_page.clone();
        let gradient_page = gradient_page.clone();
        Rc::new(move || {
            (color_page.repaint)();
            (gradient_page.repaint)();
        }) as Rc<dyn Fn()>
    };
    *redraw.borrow_mut() = Some(refresh.clone());

    // Paint once on open, and again whenever the panel's own refresh runs so
    // an external change (undo, a project load) is reflected while open.
    popover.connect_show({
        let refresh = refresh.clone();
        move |_| refresh()
    });

    popover.set_child(Some(&root));
    popover
}

// ── Color page ──

#[derive(Clone)]
struct ColorPage {
    widget: GtkBox,
    repaint: Rc<dyn Fn()>,
}

fn build_color_page(
    state: &Arc<Mutex<VideoEditState>>,
    notify: &Rc<dyn Fn()>,
) -> ColorPage {
    let widget = GtkBox::new(Orientation::Vertical, 0);
    widget.set_hexpand(true);

    // The large saturation/value plane, as in the reference: the current hue
    // washing to white on the right and darkening downward, with a handle
    // showing where the live color sits on it.
    let field = DrawingArea::new();
    field.add_css_class("recording-editor-custom-field");
    // The reference's plane is the tallest thing in the popover, roughly
    // square against the popover's width.
    field.set_content_height(190);
    field.set_hexpand(true);
    field.set_cursor(gtk4::gdk::Cursor::from_name("crosshair", None).as_ref());
    widget.append(&field);

    // The rainbow hue bar with a round handle, matching the reference row.
    let spectrum = DrawingArea::new();
    spectrum.add_css_class("recording-editor-custom-spectrum");
    spectrum.set_content_height(26);
    widget.append(&spectrum);

    // Swatch + hex + "rgb", on one rounded bar.
    let value_row = GtkBox::new(Orientation::Horizontal, 8);
    value_row.add_css_class("recording-editor-custom-value-row");
    let swatch = DrawingArea::new();
    swatch.add_css_class("recording-editor-custom-swatch");
    swatch.set_content_width(22);
    swatch.set_content_height(22);
    swatch.set_valign(Align::Center);
    swatch.set_can_target(false);
    let hex = Entry::new();
    hex.add_css_class("recording-editor-custom-hex");
    hex.set_valign(Align::Center);
    hex.set_hexpand(true);
    let unit = Label::new(Some(&t("rgb")));
    unit.add_css_class("recording-editor-custom-unit");
    unit.set_valign(Align::Center);
    value_row.append(&swatch);
    value_row.append(&hex);
    value_row.append(&unit);
    widget.append(&value_row);

    // A guard so pushing state into the entry does not look like a user edit
    // and re-commit the value we just set.
    let syncing = Rc::new(Cell::new(false));

    // Commit the typed hex on Enter or focus-out, matching the image editor's
    // commit-on-activate pattern rather than validating per keystroke.
    {
        let state = state.clone();
        let notify = notify.clone();
        let syncing = syncing.clone();
        hex.connect_activate({
            let state = state.clone();
            let notify = notify.clone();
            let syncing = syncing.clone();
            move |entry| {
                if syncing.get() {
                    return;
                }
                if let Some(color) = parse_hex(&entry.text()) {
                    set_flat_color(&state, color);
                    notify();
                }
            }
        });
        let focus = gtk4::EventControllerFocus::new();
        focus.connect_leave({
            let state = state.clone();
            let notify = notify.clone();
            let syncing = syncing.clone();
            move |controller| {
                if syncing.get() {
                    return;
                }
                let Some(entry) = controller.widget().and_then(|w| w.downcast::<Entry>().ok())
                else {
                    return;
                };
                if let Some(color) = parse_hex(&entry.text()) {
                    set_flat_color(&state, color);
                    notify();
                }
            }
        });
        hex.add_controller(focus);
    }

    // Dragging inside the plane sets saturation (across) and value (down);
    // dragging the bar sets the hue the plane is built from.
    attach_plane_drag(&field, state.clone(), notify.clone());
    attach_hue_drag(&spectrum, state.clone(), notify.clone());

    let refresh: Rc<dyn Fn()> = {
        let field = field.clone();
        let spectrum = spectrum.clone();
        let swatch = swatch.clone();
        let hex = hex.clone();
        let syncing = syncing.clone();
        let state = state.clone();
        Rc::new(move || {
            let color = current_flat_color(&state);
            // The plane and the bar both draw from the hue of the live color,
            // so a color that arrived from a project file or the hex entry
            // still shows the right hue rather than a stale one.
            let hsv = rgb_to_hsv(color.0, color.1, color.2);
            field.set_draw_func({
                let hsv = hsv;
                move |_, cr, width, height| {
                    draw_plane(cr, width as f64, height as f64, hsv);
                }
            });
            field.queue_draw();
            spectrum.set_draw_func({
                let color = color;
                move |_, cr, width, height| {
                    draw_spectrum(cr, width as f64, height as f64, color);
                }
            });
            spectrum.queue_draw();
            swatch.set_draw_func({
                let color = color;
                move |_, cr, width, height| {
                    fill_rounded(cr, 0.0, 0.0, width as f64, height as f64, 5.0, color);
                }
            });
            swatch.queue_draw();
            syncing.set(true);
            hex.set_text(&hex_string(color));
            syncing.set(false);
        }) as Rc<dyn Fn()>
    };

    ColorPage { widget, repaint: refresh }
}

/// The saturation/value plane: the current hue across the top, washing to
/// white on the right and blackening downward, with a handle on the live
/// color. This is what makes the field a picker rather than a swatch — the
/// reference's red-to-dark wash is this plane, not a flat fill.
fn draw_plane(
    cr: &gtk4::cairo::Context,
    w: f64,
    h: f64,
    hsv: (f64, f64, f64),
) {
    if w < 2.0 || h < 2.0 {
        return;
    }
    let (hue, saturation, value) = hsv;
    let _ = cr.save();
    rounded_rect(cr, 0.0, 0.0, w, h, FIELD_RADIUS);
    let _ = cr.clip();

    // The pure hue fills the left edge, then white overlays from the right.
    let base = hsv_to_rgb(hue, 1.0, 1.0);
    let white = gtk4::cairo::LinearGradient::new(0.0, 0.0, w, 0.0);
    let _ = white.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.0);
    let _ = white.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 1.0);
    // Black overlays from the bottom.
    let shade = gtk4::cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
    let _ = shade.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
    let _ = shade.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 1.0);

    cr.set_source_rgb(
        base.0 as f64 / 255.0,
        base.1 as f64 / 255.0,
        base.2 as f64 / 255.0,
    );
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();
    let _ = cr.set_source(&white);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();
    let _ = cr.set_source(&shade);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();
    let _ = cr.restore();

    // The handle, ringed so it reads against any part of the plane.
    let cx = saturation.clamp(0.0, 1.0) * w;
    let cy = (1.0 - value.clamp(0.0, 1.0)) * h;
    let r = 7.0;
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
    let color = hsv_to_rgb(hue, saturation, value);
    cr.set_source_rgb(
        color.0 as f64 / 255.0,
        color.1 as f64 / 255.0,
        color.2 as f64 / 255.0,
    );
    cr.arc(cx, cy, r - 2.0, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
}

/// The rainbow hue bar with a round handle on the current color, matching the
/// reference picker row. The bar is a full-saturation sweep, so dragging the
/// handle sets the field to the pure color under it.
fn draw_spectrum(cr: &gtk4::cairo::Context, w: f64, h: f64, color: (u8, u8, u8)) {
    if w < 2.0 || h < 2.0 {
        return;
    }
    // One column per pixel, clipped to a pill so the bar's ends round off.
    let _ = cr.save();
    rounded_rect(cr, 0.0, 0.0, w, h, h / 2.0);
    let _ = cr.clip();
    for x in 0..w as i32 {
        let hue = x as f64 / w;
        let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
        cr.set_source_rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
        cr.rectangle(x as f64, 0.0, 1.0, h);
        let _ = cr.fill();
    }
    let _ = cr.restore();

    // The handle, ringed in white so it reads against any hue.
    let hue = rgb_to_hue(color.0, color.1, color.2);
    let cx = hue * w;
    let cy = h / 2.0;
    let r = (h * 0.42).min(11.0);
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
    cr.set_source_rgb(
        color.0 as f64 / 255.0,
        color.1 as f64 / 255.0,
        color.2 as f64 / 255.0,
    );
    cr.arc(cx, cy, (r - 2.0).max(1.0), 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
}

fn current_flat_color(state: &Arc<Mutex<VideoEditState>>) -> (u8, u8, u8) {    match &state.lock().unwrap().background {
        VideoBackground::Plain { r, g, b } => (*r, *g, *b),
        _ => (17, 17, 17),
    }
}

fn set_flat_color(state: &Arc<Mutex<VideoEditState>>, color: (u8, u8, u8)) {
    state.lock().unwrap().background = VideoBackground::Plain {
        r: color.0,
        g: color.1,
        b: color.2,
    };
}

fn attach_hue_drag(
    spectrum: &DrawingArea,
    state: Arc<Mutex<VideoEditState>>,
    notify: Rc<dyn Fn()>,
) {
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let state = state.clone();
        let notify = notify.clone();
        move |gesture, x, _| {
            apply_hue(&gesture, x, &state, &notify);
        }
    });
    drag.connect_drag_update({
        let state = state.clone();
        let notify = notify.clone();
        move |gesture, dx, _| {
            let Some((start, _)) = gesture.start_point() else {
                return;
            };
            let _ = dx;
            apply_hue(&gesture, start, &state, &notify);
        }
    });
    spectrum.add_controller(drag);
}

fn apply_hue(
    gesture: &GestureDrag,
    x: f64,
    state: &Arc<Mutex<VideoEditState>>,
    notify: &Rc<dyn Fn()>,
) {
    let Some(widget) = gesture.widget() else {
        return;
    };
    let width = widget.allocated_width().max(1) as f64;
    let hue = (x / width).clamp(0.0, 1.0);
    let (s, v) = {
        let color = current_flat_color(state);
        let (_, s, v) = rgb_to_hsv(color.0, color.1, color.2);
        (s, v)
    };
    set_flat_color(state, hsv_to_rgb(hue, s.max(0.0), v.max(0.0)));
    notify();
}

/// Drag inside the plane: horizontal sets saturation, vertical sets value,
/// both against the hue the bar currently holds.
fn attach_plane_drag(
    field: &DrawingArea,
    state: Arc<Mutex<VideoEditState>>,
    notify: Rc<dyn Fn()>,
) {
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let state = state.clone();
        let notify = notify.clone();
        move |gesture, x, y| {
            apply_plane(&gesture, x, y, &state, &notify);
        }
    });
    drag.connect_drag_update({
        let state = state.clone();
        let notify = notify.clone();
        move |gesture, dx, dy| {
            let Some((start_x, start_y)) = gesture.start_point() else {
                return;
            };
            let _ = (dx, dy);
            apply_plane(&gesture, start_x, start_y, &state, &notify);
        }
    });
    field.add_controller(drag);
}

fn apply_plane(
    gesture: &GestureDrag,
    x: f64,
    y: f64,
    state: &Arc<Mutex<VideoEditState>>,
    notify: &Rc<dyn Fn()>,
) {
    let Some(widget) = gesture.widget() else {
        return;
    };
    let width = widget.allocated_width().max(1) as f64;
    let height = widget.allocated_height().max(1) as f64;
    let saturation = (x / width).clamp(0.0, 1.0);
    // The plane darkens downward, so the top is full value.
    let value = (1.0 - y / height).clamp(0.0, 1.0);
    let hue = {
        let color = current_flat_color(state);
        rgb_to_hue(color.0, color.1, color.2)
    };
    set_flat_color(state, hsv_to_rgb(hue, saturation, value));
    notify();
}

// ── Gradient page ──

#[derive(Clone)]
struct GradientPage {
    widget: GtkBox,
    repaint: Rc<dyn Fn()>,
}

fn build_gradient_page(
    state: &Arc<Mutex<VideoEditState>>,
    notify: &Rc<dyn Fn()>,
) -> GradientPage {
    let widget = GtkBox::new(Orientation::Vertical, 0);
    widget.set_hexpand(true);

    let preview = DrawingArea::new();
    preview.add_css_class("recording-editor-custom-field");
    preview.set_content_height(120);
    preview.set_can_target(false);
    widget.append(&preview);

    // Angle + reverse, on one row under the preview.
    let controls = GtkBox::new(Orientation::Horizontal, 8);
    controls.add_css_class("recording-editor-gradient-controls");
    let angle = DrawingArea::new();
    angle.add_css_class("recording-editor-gradient-angle");
    angle.set_content_height(28);
    angle.set_hexpand(true);
    let reverse = Button::new();
    reverse.add_css_class("recording-editor-gradient-reverse");
    reverse.set_has_frame(false);
    reverse.set_valign(Align::Center);
    reverse.set_tooltip_text(Some(&t("Reverse")));
    let reverse_icon = Image::from_icon_name("view-refresh-symbolic");
    reverse_icon.set_pixel_size(14);
    reverse.set_child(Some(&reverse_icon));
    {
        let state = state.clone();
        let notify = notify.clone();
        reverse.connect_clicked(move |_| {
            with_gradient(&state, |g| g.reversed = !g.reversed);
            notify();
        });
    }
    controls.append(&angle);
    controls.append(&reverse);
    widget.append(&controls);

    // The stop bar: drag a handle to move a stop, click empty track to add.
    let bar = DrawingArea::new();
    bar.add_css_class("recording-editor-gradient-bar");
    bar.set_content_height(40);
    widget.append(&bar);

    // Steps header + list.
    let steps_header = GtkBox::new(Orientation::Horizontal, 8);
    steps_header.add_css_class("recording-editor-gradient-steps-header");
    let steps_title = Label::new(Some(&t("Steps")));
    steps_title.set_xalign(0.0);
    steps_title.set_hexpand(true);
    let add = Button::new();
    add.add_css_class("recording-editor-gradient-add");
    add.set_has_frame(false);
    add.set_valign(Align::Center);
    add.set_tooltip_text(Some(&t("Add stop")));
    let add_label = Label::new(Some("+"));
    add.set_child(Some(&add_label));
    {
        let state = state.clone();
        let notify = notify.clone();
        add.connect_clicked(move |_| {
            add_stop(state.clone(), notify.clone());
        });
    }
    steps_header.append(&steps_title);
    steps_header.append(&add);
    widget.append(&steps_header);

    let steps = Grid::new();
    steps.add_css_class("recording-editor-gradient-steps");
    steps.set_column_spacing(8);
    steps.set_row_spacing(4);
    steps.set_hexpand(true);
    widget.append(&steps);

    attach_angle_drag(&angle, state.clone(), notify.clone());
    attach_stop_drag(&bar, state.clone(), notify.clone());

    let refresh: Rc<dyn Fn()> = {
        let preview = preview.clone();
        let angle = angle.clone();
        let bar = bar.clone();
        let steps = steps.clone();
        let steps_title = steps_title.clone();
        let add = add.clone();
        let reverse = reverse.clone();
        let state = state.clone();
        let notify = notify.clone();
        Rc::new(move || {
            let gradient = current_gradient(&state);
            preview.set_draw_func({
                let gradient = gradient.clone();
                move |_, cr, width, height| {
                    draw_gradient_field(cr, width as f64, height as f64, &gradient);
                }
            });
            preview.queue_draw();

            bar.set_draw_func({
                let gradient = gradient.clone();
                move |_, cr, width, height| {
                    draw_stop_bar(cr, width as f64, height as f64, &gradient);
                }
            });
            bar.queue_draw();

            angle.set_draw_func({
                let gradient = gradient.clone();
                move |_, cr, width, height| {
                    draw_angle(cr, width as f64, height as f64, &gradient);
                }
            });
            angle.queue_draw();

            // The Steps list is tiny (2..=8 rows), so rebuilding it is cheaper
            // and far less error-prone than tracking per-row widget state.
            while let Some(child) = steps.first_child() {
                steps.remove(&child);
            }
            let normalized = gradient.normalized();
            steps_title.set_text(&format!("{} ({})", t("Steps"), normalized.stops.len()));
            add.set_sensitive(normalized.stops.len() < MAX_GRADIENT_STOPS);
            reverse.set_sensitive(normalized.stops.len() >= MIN_GRADIENT_STOPS);
            for (row, stop) in normalized.stops.iter().enumerate() {
                let row_widget = build_stop_row(stop, row, normalized.stops.len(), &state, &notify);
                steps.attach(&row_widget, 0, row as i32, 1, 1);
            }
        }) as Rc<dyn Fn()>
    };

    GradientPage { widget, repaint: refresh }
}

fn current_gradient(state: &Arc<Mutex<VideoEditState>>) -> VideoGradient {
    match &state.lock().unwrap().background {
        VideoBackground::Gradient(gradient) => gradient.normalized(),
        _ => VideoGradient::default(),
    }
}

/// Apply `edit` to the gradient, promoting a flat color or no-fill to a real
/// gradient first so an edit on the Gradient tab is never dropped.
fn with_gradient(state: &Arc<Mutex<VideoEditState>>, edit: impl FnOnce(&mut VideoGradient)) {
    let mut guard = state.lock().unwrap();
    if !matches!(guard.background, VideoBackground::Gradient(_)) {
        let seed = match &guard.background {
            VideoBackground::Plain { r, g, b } => VideoGradient {
                stops: vec![
                    GradientStop::new(0.0, *r, *g, *b),
                    GradientStop::new(1.0, *r, *g, *b),
                ],
                ..VideoGradient::default()
            },
            _ => VideoGradient::default(),
        };
        guard.background = VideoBackground::Gradient(seed);
    }
    if let VideoBackground::Gradient(gradient) = &mut guard.background {
        let mut normalized = gradient.normalized();
        edit(&mut normalized);
        *gradient = normalized.normalized();
    }
}

fn add_stop(state: Arc<Mutex<VideoEditState>>, notify: Rc<dyn Fn()>) {
    with_gradient(&state, |gradient| {
        if gradient.stops.len() >= MAX_GRADIENT_STOPS {
            return;
        }
        // Insert into the widest gap so the new stop is visible and editable
        // rather than landing on top of an existing one.
        let mut position = 0.5;
        let mut widest: f64 = -1.0;
        for pair in gradient.stops.windows(2) {
            let gap = pair[1].position - pair[0].position;
            if gap > widest {
                widest = gap;
                position = (pair[0].position + pair[1].position) / 2.0;
            }
        }
        if widest <= f64::EPSILON {
            position = 0.5;
        }
        let color = sample_color(gradient, position);
        gradient
            .stops
            .push(GradientStop::new(position, color.0, color.1, color.2));
    });
    notify();
}

fn remove_stop(state: Arc<Mutex<VideoEditState>>, notify: Rc<dyn Fn()>, index: usize) {
    with_gradient(&state, |gradient| {
        // Two stops is a gradient's floor; removing past it would leave a
        // single color with no direction.
        if gradient.stops.len() <= MIN_GRADIENT_STOPS || index >= gradient.stops.len() {
            return;
        }
        gradient.stops.remove(index);
    });
    notify();
}

/// The color the gradient shows at `position`, used to seed a new stop with
/// the color already under it so adding one is a visible no-op until dragged.
fn sample_color(gradient: &VideoGradient, position: f64) -> (u8, u8, u8) {
    let bitmap = render_gradient(gradient, 256, 1);
    let x = (position * 255.0).round().clamp(0.0, 255.0) as u32;
    bitmap.pixel(x, 0)
}

fn bitmap_to_surface(
    bitmap: &crate::recording::editor::model::background_render::GradientBitmap,
) -> gtk4::cairo::ImageSurface {
    let width = bitmap.width;
    let height = bitmap.height;
    let stride = gtk4::cairo::Format::Rgb24
        .stride_for_width(width)
        .unwrap_or((width * 4) as i32);
    // Cairo's Rgb24 is native-endian with a padding byte, so the RGB bytes
    // are laid out B, G, R, 0 on a little-endian host.
    let mut data = Vec::with_capacity(stride as usize * height as usize);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let i = (y * width as usize + x) * 3;
            data.extend_from_slice(&[
                bitmap.pixels[i + 2],
                bitmap.pixels[i + 1],
                bitmap.pixels[i],
                0x00,
            ]);
        }
    }
    gtk4::cairo::ImageSurface::create_for_data(
        data,
        gtk4::cairo::Format::Rgb24,
        width as i32,
        height as i32,
        stride,
    )
    .expect("gradient surface allocates")
}

fn draw_gradient_field(
    cr: &gtk4::cairo::Context,
    w: f64,
    h: f64,
    gradient: &VideoGradient,
) {
    if w < 2.0 || h < 2.0 {
        return;
    }
    // Half resolution is finer than the widget can show and keeps a redraw on
    // every drag step cheap.
    let bitmap = render_gradient(gradient, (w * 0.5) as u32, (h * 0.5) as u32);
    let surface = bitmap_to_surface(&bitmap);
    let _ = cr.save();
    rounded_rect(cr, 0.0, 0.0, w, h, FIELD_RADIUS);
    cr.clip();
    cr.scale(2.0, 2.0);
    let _ = cr.set_source_surface(&surface, 0.0, 0.0);
    let _ = cr.paint();
    let _ = cr.restore();
    stroke_rounded(cr, 0.0, 0.0, w, h, FIELD_RADIUS, false);
}

fn draw_stop_bar(
    cr: &gtk4::cairo::Context,
    w: f64,
    h: f64,
    gradient: &VideoGradient,
) {
    if w < 2.0 || h < 2.0 {
        return;
    }
    let bitmap = render_gradient(gradient, (w * 0.5) as u32, (h * 0.5) as u32);
    let surface = bitmap_to_surface(&bitmap);
    let _ = cr.save();
    rounded_rect(cr, 0.0, 0.0, w, h, 6.0);
    cr.clip();
    cr.scale(2.0, 2.0);
    let _ = cr.set_source_surface(&surface, 0.0, 0.0);
    let _ = cr.paint();
    let _ = cr.restore();

    // Handles are drawn outside the clip so the end stops stay fully visible
    // at the very edges of the bar.
    let inset = HANDLE_RADIUS;
    let span = (w - inset * 2.0).max(1.0);
    let cy = h / 2.0;
    for stop in &gradient.normalized().stops {
        let cx = inset + stop.position * span;
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.96);
        cr.arc(cx, cy, HANDLE_RADIUS, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
        cr.set_source_rgb(
            stop.r as f64 / 255.0,
            stop.g as f64 / 255.0,
            stop.b as f64 / 255.0,
        );
        cr.arc(cx, cy, HANDLE_RADIUS - 2.5, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
}

fn draw_angle(
    cr: &gtk4::cairo::Context,
    w: f64,
    h: f64,
    gradient: &VideoGradient,
) {
    let light = false;
    fill_rounded(cr, 0.0, 0.0, w, h, 6.0, (30, 30, 30));
    if light {
        let _ = light;
    }
    rounded_rect(cr, 0.0, 0.0, w, h, 6.0);
    if gradient.angle_degrees > f64::EPSILON {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.12);
        let _ = cr.fill();
    } else {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.18);
        let _ = cr.fill();
    }
    cr.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(11.0);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.72);
    cr.move_to(10.0, h * 0.68);
    let _ = cr.show_text(&format!("{}°", gradient.angle_degrees.round() as i64));
    let _ = w;
}

fn build_stop_row(
    stop: &GradientStop,
    index: usize,
    count: usize,
    state: &Arc<Mutex<VideoEditState>>,
    notify: &Rc<dyn Fn()>,
) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("recording-editor-gradient-step");

    let swatch = DrawingArea::new();
    swatch.add_css_class("recording-editor-gradient-step-swatch");
    swatch.set_content_width(20);
    swatch.set_content_height(20);
    swatch.set_valign(Align::Center);
    swatch.set_can_target(false);
    let color = (stop.r, stop.g, stop.b);
    swatch.set_draw_func(move |_, cr, width, height| {
        fill_rounded(cr, 0.0, 0.0, width as f64, height as f64, 5.0, color);
    });
    // A click gesture rather than a button so the swatch stays a pure chip;
    // the release handler opens the per-stop color chooser.
    {
        let gesture = GestureClick::new();
        let anchor = swatch.clone();
        let state = state.clone();
        let notify = notify.clone();
        gesture.connect_released(move |_, _, _, _| {
            open_stop_color_picker(&anchor, index, state.clone(), notify.clone());
        });
        swatch.add_controller(gesture);
    }

    let label = Label::new(Some(&hex_string((stop.r, stop.g, stop.b))));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_valign(Align::Center);

    let remove = Button::new();
    remove.add_css_class("recording-editor-gradient-step-remove");
    remove.set_has_frame(false);
    remove.set_valign(Align::Center);
    // Two stops is a hard floor, so the control says so instead of silently
    // ignoring the click.
    remove.set_sensitive(count > MIN_GRADIENT_STOPS);
    remove.set_tooltip_text(Some(&t("Remove stop")));
    let remove_icon = Image::from_icon_name("user-trash-symbolic");
    remove_icon.set_pixel_size(13);
    remove.set_child(Some(&remove_icon));
    {
        let state = state.clone();
        let notify = notify.clone();
        remove.connect_clicked(move |_| remove_stop(state.clone(), notify.clone(), index));
    }

    row.append(&swatch);
    row.append(&label);
    row.append(&remove);
    row
}

fn open_stop_color_picker(
    anchor: &impl IsA<Widget>,
    index: usize,
    state: Arc<Mutex<VideoEditState>>,
    notify: Rc<dyn Fn()>,
) {
    let color = {
        let VideoBackground::Gradient(gradient) = &state.lock().unwrap().background else {
            return;
        };
        let normalized = gradient.normalized();
        let Some(stop) = normalized.stops.get(index) else {
            return;
        };
        (stop.r, stop.g, stop.b)
    };
    let initial = gtk4::gdk::RGBA::new(
        color.0 as f32 / 255.0,
        color.1 as f32 / 255.0,
        color.2 as f32 / 255.0,
        1.0,
    );
    // Parented to the editor window so the dialog centers on the editor
    // rather than the screen. A swatch is a DrawingArea, so its root reaches
    // the window through the popover.
    let parent = anchor.root().and_then(|root| root.downcast::<Window>().ok());
    let dialog = ColorChooserDialog::new(Some(&t("Stop color")), parent.as_ref());
    dialog.set_modal(true);
    dialog.set_use_alpha(false);
    dialog.set_rgba(&initial);
    dialog.connect_response(move |dialog, response| {
        if response == gtk4::ResponseType::Ok {
            let c = dialog.rgba();
            let rgb = (
                (c.red() * 255.0).round().clamp(0.0, 255.0) as u8,
                (c.green() * 255.0).round().clamp(0.0, 255.0) as u8,
                (c.blue() * 255.0).round().clamp(0.0, 255.0) as u8,
            );
            with_gradient(&state, |gradient| {
                if let Some(stop) = gradient.stops.get_mut(index) {
                    stop.r = rgb.0;
                    stop.g = rgb.1;
                    stop.b = rgb.2;
                }
            });
            notify();
        }
        dialog.close();
    });
    dialog.present();
}

fn attach_angle_drag(
    angle: &DrawingArea,
    state: Arc<Mutex<VideoEditState>>,
    notify: Rc<dyn Fn()>,
) {
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let state = state.clone();
        let notify = notify.clone();
        move |gesture, x, _| {
            apply_angle(&gesture, x, &state, &notify);
        }
    });
    drag.connect_drag_update({
        let state = state.clone();
        let notify = notify.clone();
        move |gesture, dx, _| {
            let Some((start, _)) = gesture.start_point() else {
                return;
            };
            let _ = dx;
            apply_angle(&gesture, start, &state, &notify);
        }
    });
    angle.add_controller(drag);
}

fn apply_angle(
    gesture: &GestureDrag,
    x: f64,
    state: &Arc<Mutex<VideoEditState>>,
    notify: &Rc<dyn Fn()>,
) {
    let Some(widget) = gesture.widget() else {
        return;
    };
    let width = widget.allocated_width().max(1) as f64;
    // The first ~40px is the label gutter, so the track starts after it.
    let gutter = 40.0;
    let t = ((x - gutter) / (width - gutter).max(1.0)).clamp(0.0, 1.0);
    let degrees = t * 360.0;
    with_gradient(state, |gradient| gradient.angle_degrees = degrees);
    notify();
}

/// Drag a stop along the bar. The nearest handle within a grab radius wins; a
/// drag starting on empty track adds a stop there and moves that one instead.
fn attach_stop_drag(
    bar: &DrawingArea,
    state: Arc<Mutex<VideoEditState>>,
    notify: Rc<dyn Fn()>,
) {
    // The index of the stop being moved. It is re-resolved after every write
    // because `normalized()` re-sorts by position, so a stop dragged past
    // another changes index mid-drag.
    let dragging = Rc::new(Cell::new(usize::MAX));
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let state = state.clone();
        let notify = notify.clone();
        let dragging = dragging.clone();
        move |gesture, x, y| {
            let Some(widget) = gesture.widget() else {
                return;
            };
            let width = widget.allocated_width().max(1) as f64;
            let height = widget.allocated_height().max(1) as f64;
            let inset = HANDLE_RADIUS;
            let span = (width - inset * 2.0).max(1.0);
            let cy = height / 2.0;
            let stops = current_gradient(&state).normalized().stops;

            let mut best = usize::MAX;
            let mut best_distance = f64::MAX;
            for (index, stop) in stops.iter().enumerate() {
                let cx = inset + stop.position * span;
                let distance = (cx - x).hypot(cy - y);
                if distance < best_distance {
                    best_distance = distance;
                    best = index;
                }
            }
            if best_distance <= HANDLE_RADIUS * 2.5 {
                dragging.set(best);
                return;
            }
            // Empty track: add a stop at the click, then drag that one.
            let position = ((x - inset) / span).clamp(0.0, 1.0);
            let color = sample_color(&current_gradient(&state), position);
            with_gradient(&state, |gradient| {
                if gradient.stops.len() >= MAX_GRADIENT_STOPS {
                    return;
                }
                gradient
                    .stops
                    .push(GradientStop::new(position, color.0, color.1, color.2));
            });
            notify();
            // The list is now sorted, so find where the new stop landed.
            dragging.set(
                current_gradient(&state)
                    .normalized()
                    .stops
                    .iter()
                    .position(|stop| (stop.position - position).abs() < 1e-6)
                    .unwrap_or(usize::MAX),
            );
        }
    });
    drag.connect_drag_update({
        let state = state.clone();
        let notify = notify.clone();
        let dragging = dragging.clone();
        move |gesture, dx, _| {
            let index = dragging.get();
            if index == usize::MAX {
                return;
            }
            let Some(widget) = gesture.widget() else {
                return;
            };
            let Some((start, _)) = gesture.start_point() else {
                return;
            };
            let width = widget.allocated_width().max(1) as f64;
            let inset = HANDLE_RADIUS;
            let span = (width - inset * 2.0).max(1.0);
            let position = (((start + dx) - inset) / span).clamp(0.0, 1.0);
            // Capture the position being moved so it can be found again after
            // the list re-sorts.
            let before = current_gradient(&state)
                .normalized()
                .stops
                .get(index)
                .map(|s| s.position);
            with_gradient(&state, |gradient| {
                if let Some(stop) = gradient.stops.get_mut(index) {
                    stop.position = position;
                }
            });
            let _ = before;
            notify();
            // Re-resolve: the stop that was at `index` may now be elsewhere.
            dragging.set(
                current_gradient(&state)
                    .normalized()
                    .stops
                    .iter()
                    .position(|stop| (stop.position - position).abs() < 1e-9)
                    .unwrap_or(usize::MAX),
            );
        }
    });
    drag.connect_drag_end({
        let notify = notify.clone();
        let dragging = dragging.clone();
        move |_, _, _| {
            dragging.set(usize::MAX);
            notify();
        }
    });
    bar.add_controller(drag);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let color = (0x18, 0x16, 0x16);
        assert_eq!(parse_hex(&hex_string(color)), Some(color));
    }

    #[test]
    fn hex_parses_short_and_bare_forms() {
        assert_eq!(parse_hex("#ABC"), Some((0xAA, 0xBB, 0xCC)));
        assert_eq!(parse_hex("abc"), Some((0xAA, 0xBB, 0xCC)));
        assert_eq!(parse_hex("#181616"), Some((0x18, 0x16, 0x16)));
    }

    #[test]
    fn a_partial_or_invalid_hex_is_rejected() {
        // Committing a half-typed hex would set a wrong color, so anything
        // that is not a complete 3 or 6 digit value is refused.
        assert_eq!(parse_hex("#18"), None);
        assert_eq!(parse_hex("#18161"), None);
        assert_eq!(parse_hex("zzz"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn the_spectrum_bar_is_a_pure_hue_sweep() {
        // The bar sweeps hue at full saturation and value, so every hue is
        // reachable by dragging it.
        let red = hsv_to_rgb(0.0, 1.0, 1.0);
        assert_eq!(red, (255, 0, 0));
        let green = hsv_to_rgb(1.0 / 3.0, 1.0, 1.0);
        assert_eq!(green, (0, 255, 0));
        let blue = hsv_to_rgb(2.0 / 3.0, 1.0, 1.0);
        assert_eq!(blue, (0, 0, 255));
    }

    #[test]
    fn the_plane_covers_saturation_and_value() {
        // The plane is what makes the big field a picker rather than a flat
        // swatch: saturation runs left to right and value top to bottom, both
        // against the hue the bar holds. The reference's red-to-dark wash is
        // exactly this.
        let hue = 0.0;
        // Left edge is the pure hue, right edge washes to white.
        assert_eq!(hsv_to_rgb(hue, 0.0, 1.0), (255, 255, 255));
        assert_eq!(hsv_to_rgb(hue, 1.0, 1.0), (255, 0, 0));
        // The top is full value, the bottom is black.
        assert_eq!(hsv_to_rgb(hue, 1.0, 0.0), (0, 0, 0));
    }

    #[test]
    fn a_color_survives_a_round_trip_through_hsv() {
        // The plane's handle position is derived from the stored color, so
        // picking a hue then a saturation then a value has to land back on
        // the same color rather than drifting on each rebuild.
        for (h, s, v) in [(0.0, 1.0, 1.0), (0.33, 0.5, 0.8), (0.72, 1.0, 0.25)] {
            let color = hsv_to_rgb(h, s, v);
            let (rh, rs, rv) = rgb_to_hsv(color.0, color.1, color.2);
            let back = hsv_to_rgb(rh, rs, rv);
            let within = |a: u8, b: u8| (a as i32 - b as i32).abs() <= 2;
            assert!(
                within(color.0, back.0)
                    && within(color.1, back.1)
                    && within(color.2, back.2),
                "{color:?} came back as {back:?}"
            );
        }
    }

    #[test]
    fn hue_survives_a_round_trip_through_rgb() {
        // The handle's position comes from the stored color, so a color that
        // came from a project file must land back on the right hue.
        for step in 0..12 {
            let hue = step as f64 / 12.0;
            let color = hsv_to_rgb(hue, 1.0, 1.0);
            let recovered = rgb_to_hue(color.0, color.1, color.2);
            assert!(
                (recovered - hue).abs() < 0.02 || (recovered - hue).abs() > 0.98,
                "hue {hue} came back as {recovered}"
            );
        }
    }
}
