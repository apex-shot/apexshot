// The Custom Wallpaper popover.
//
// Reached from the Edit pill on the Background panel's Custom page. The card
// hangs off the panel's left edge, level with the row that summarizes the
// fill, so it floats over the video stage instead of covering the sidebar it
// belongs to — a centered dialog would hide both that row and the video
// being changed.
//
// Two sub-tabs. Color is the field / hue bar / hex row from the reference.
// Gradient is the full multi-stop editor. Both write straight into
// `VideoEditState.background` through `on_change` (the editor's `ping`),
// which repaints the video preview and runs the panel refresh.

use std::cell::Cell;
use std::cell::RefCell;
use std::f64::consts::{FRAC_PI_2, PI};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, DrawingArea, Entry, GestureClick, GestureDrag, Grid,
    Image, Label, Orientation, Popover, ToggleButton, Widget,
};

use crate::i18n::t;
use crate::recording::editor::model::background_render::{render_gradient, sample_color_at};
use crate::recording::editor::model::{
    GradientKind, GradientStop, VideoBackground, VideoEditState, VideoGradient, MAX_GRADIENT_STOPS,
};

/// Corner radius shared by the color field, the hue bar, and the hex row, so
/// the popover reads as one set of stacked surfaces.
const FIELD_RADIUS: f64 = 8.0;
/// Radius of a hue-bar handle.
const HANDLE_RADIUS: f64 = 8.0;
/// Widget height for the hue bar: room for the overlapping thumb plus padding.
const SPECTRUM_CONTENT_HEIGHT: i32 = 22;
/// Thickness of the rainbow rail itself, centered in the widget. Deliberately
/// slimmer than the thumb so the handle straddles the bar.
const SPECTRUM_BAR_THICKNESS: f64 = 10.0;
/// Height of the gradient bar widget: the ramp's track with a stop handle
/// straddling it.
const GRADIENT_BAR_HEIGHT: i32 = 30;
/// Side of a stop's square handle.
const GRADIENT_PIN_SIZE: f64 = 20.0;
/// Thickness of the ramp's track.
const GRADIENT_BAR_TRACK: f64 = 14.0;
/// How far the selected handle's ring reaches past the handle. The handles are
/// inset by this much so an end stop's ring is never clipped by the bar's
/// edge.
const GRADIENT_PIN_RING: f64 = 3.0;
/// How far from a handle's centre a press still grabs it. Deliberately roomier
/// than the handle: the stop at either end has the whole ramp beside it, and
/// that is where a press naturally lands.
const GRADIENT_GRAB_RADIUS: f64 = 22.0;
/// Degrees added per press of the gradient's rotate control. Figma's rotate
/// button turns the gradient a quarter turn at a time.
const GRADIENT_ROTATE_STEP: f64 = 90.0;
/// Side of the square the header's glyphs are drawn in. Lucide's artwork is a
/// 24x24 box, so the stroke is scaled from there and the glyph keeps its
/// proportions however the button is sized.
const GRADIENT_ICON_SIZE: i32 = 16;
/// Side of a stop row's color chip.
const STEP_CHIP_SIZE: i32 = 26;
/// Corner radius of a stop row's color chip.
const STEP_CHIP_RADIUS: f64 = 8.0;
/// How far left of the panel's edge the card is anchored. Without it the
/// card's right edge lands on the panel's edge — the gap you see is only the
/// panel's own padding — which reads as butted up against the controls. The
/// reference keeps a clear ~23px between the card and the panel's content.
const POPOVER_SIDEBAR_GAP: i32 = 8;
/// How far clear of the popover's own edge the stop picker's card sits. Applied
/// as a popover layout offset, i.e. on top of the position GTK computes, so it
/// cannot move the card into a different coordinate space.
const PICKER_CARD_GAP: i32 = 8;

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

fn stroke_rounded(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64, light: bool) {
    rounded_rect(
        cr,
        x + 0.5,
        y + 0.5,
        (w - 1.0).max(1.0),
        (h - 1.0).max(1.0),
        r,
    );
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

/// The HSV a picker keeps for the color it just read back.
///
/// `held` is the picker's own triple and `last_written` the RGB it last
/// committed. When those agree the color is the picker's own, so the triple
/// stands even where RGB cannot describe it: a drag that reached black commits
/// `#000000`, which carries neither hue nor saturation, and re-deriving from it
/// would drop the handle into the corner and reset the plane mid-drag. Any
/// other color — a project file, the hex entry, another gradient stop — is
/// adopted as itself.
fn reconciled_hsv(
    held: (f64, f64, f64),
    last_written: Option<(u8, u8, u8)>,
    color: (u8, u8, u8),
) -> (f64, f64, f64) {
    if last_written == Some(color) {
        held
    } else {
        rgb_to_hsv(color.0, color.1, color.2)
    }
}

// ── Popover ──

pub(super) fn build_custom_wallpaper_popover(
    sidebar: &GtkBox,
    edit: &Button,
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) -> Popover {
    let popover = Popover::new();
    popover.add_css_class("recording-editor-custom-popover");
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    // The card belongs to the side panel, not to the Edit pill: it opens off
    // the panel's left edge, so it floats over the video stage instead of
    // covering the panel and the row it is editing.
    popover.set_position(gtk4::PositionType::Left);
    popover.set_parent(sidebar);

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
    let gradient_page = build_gradient_page(&state, &notify, &root);
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
            // Leaving Gradient takes the stop picker's mini card with it: the
            // card is parented to the Gradient page, and one floating beside a
            // hidden page would read as a stray panel.
            if !is_gradient {
                (gradient_page.dismiss)();
            }
            color_page.widget.set_visible(!is_gradient);
            gradient_page.widget.set_visible(is_gradient);
            color_tab.set_active(!is_gradient);
            gradient_tab.set_active(is_gradient);
        })
    };
    // Both pages are appended visible, so sync them to the active tab before
    // the handlers exist. Without this the popover opens showing the Color and
    // Gradient editors stacked until the user clicks a tab.
    set_page(false);
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

    // The stop picker's card is parented to the Gradient page, so it goes down
    // with the popover rather than staying afloat over a closed editor.
    popover.connect_closed({
        let gradient_page = gradient_page.clone();
        move |_| (gradient_page.dismiss)()
    });

    // The Edit pill only sets the card's vertical seat: point at the panel's
    // left edge at that row's height. Pointing at the row itself would park
    // the card back over the panel. Bounds are read on every open so the card
    // still tracks the row when the panel has scrolled.
    edit.connect_clicked({
        let popover = popover.clone();
        let sidebar = sidebar.clone();
        let edit = edit.clone();
        move |_| {
            if let Some(bounds) = edit.compute_bounds(&sidebar) {
                // Anchor `POPOVER_SIDEBAR_GAP` left of the panel's edge so the
                // card opens with the reference's breathing room instead of
                // butting against the panel's controls.
                popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
                    -POPOVER_SIDEBAR_GAP,
                    bounds.y() as i32,
                    1,
                    bounds.height() as i32,
                )));
            }
            popover.popup();
        }
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

/// The picker itself: saturation/value plane, hue rail, swatch + hex row.
///
/// Shared between the Color tab (editing the flat fill) and the Gradient tab's
/// side panel (editing the selected stop), so it reads and writes through
/// closures instead of reaching into `background` directly. The Gradient use
/// deliberately has no Color/Gradient tab row of its own — those tabs stay the
/// popover's, and this is just the picker.
#[derive(Clone)]
struct ColorPicker {
    widget: GtkBox,
    repaint: Rc<dyn Fn()>,
}

fn build_color_picker(
    get: Rc<dyn Fn() -> (u8, u8, u8)>,
    set: Rc<dyn Fn((u8, u8, u8))>,
    notify: Rc<dyn Fn()>,
) -> ColorPicker {
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
    // The rail is slim but the widget stays thumb-tall, so the handle keeps
    // its size and straddles the bar rather than shrinking into it.
    let spectrum = DrawingArea::new();
    spectrum.add_css_class("recording-editor-custom-spectrum");
    spectrum.set_content_height(SPECTRUM_CONTENT_HEIGHT);
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

    // The picker's own HSV, kept between frames instead of re-derived from the
    // committed RGB.
    //
    // RGB cannot round-trip the triple: black carries neither hue nor
    // saturation and white carries no hue, so re-deriving HSV from the stored
    // color threw away the axis the user had just zeroed. Dragging to the
    // plane's bottom edge collapsed the handle into the bottom-left corner —
    // black reads as saturation 0 — and reset the plane's hue to red.
    //
    // `synced` remembers the exact RGB this picker last wrote, which is how
    // `refresh` tells its own writes from a color that arrived from a project
    // file, the hex entry, or another gradient stop.
    let hsv = Rc::new(Cell::new((0.0, 0.0, 0.0)));
    let synced: Rc<Cell<Option<(u8, u8, u8)>>> = Rc::new(Cell::new(None));

    // Every edit goes through here: the triple is what the plane and the bar
    // edit, and the RGB is what the model stores.
    let set_hsv: Rc<dyn Fn(f64, f64, f64)> = {
        let set = set.clone();
        let notify = notify.clone();
        let hsv = hsv.clone();
        let synced = synced.clone();
        Rc::new(move |h, s, v| {
            hsv.set((h, s, v));
            let color = hsv_to_rgb(h, s, v);
            synced.set(Some(color));
            set(color);
            notify();
        })
    };

    // A typed hex carries no hue of its own, so it is adopted wholesale rather
    // than layered onto the triple the plane was holding.
    let adopt_rgb: Rc<dyn Fn((u8, u8, u8))> = {
        let set = set.clone();
        let notify = notify.clone();
        let hsv = hsv.clone();
        let synced = synced.clone();
        Rc::new(move |color: (u8, u8, u8)| {
            hsv.set(rgb_to_hsv(color.0, color.1, color.2));
            synced.set(Some(color));
            set(color);
            notify();
        })
    };

    // Commit the typed hex on Enter or focus-out, matching the image editor's
    // commit-on-activate pattern rather than validating per keystroke.
    {
        let adopt = adopt_rgb.clone();
        let syncing = syncing.clone();
        hex.connect_activate({
            let adopt = adopt.clone();
            let syncing = syncing.clone();
            move |entry| {
                if syncing.get() {
                    return;
                }
                if let Some(color) = parse_hex(&entry.text()) {
                    adopt(color);
                }
            }
        });
        let focus = gtk4::EventControllerFocus::new();
        focus.connect_leave({
            let adopt = adopt.clone();
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
                    adopt(color);
                }
            }
        });
        hex.add_controller(focus);
    }

    // Dragging inside the plane sets saturation (across) and value (down);
    // dragging the bar sets the hue the plane is built from.
    attach_plane_drag(&field, hsv.clone(), set_hsv.clone());
    attach_hue_drag(&spectrum, hsv.clone(), set_hsv.clone());

    let refresh: Rc<dyn Fn()> = {
        let field = field.clone();
        let spectrum = spectrum.clone();
        let swatch = swatch.clone();
        let hex = hex.clone();
        let syncing = syncing.clone();
        let get = get.clone();
        let hsv = hsv.clone();
        let synced = synced.clone();
        Rc::new(move || {
            let color = get();
            // Adopt a color that changed outside this picker — a project load,
            // the hex entry, another gradient stop. Our own writes are already
            // the triple we hold, and re-deriving it would discard whichever of
            // hue and saturation a black or white commit cannot carry back.
            let (hue, saturation, value) = reconciled_hsv(hsv.get(), synced.get(), color);
            hsv.set((hue, saturation, value));
            synced.set(Some(color));
            let plane_hsv = (hue, saturation, value);
            field.set_draw_func({
                let plane_hsv = plane_hsv;
                move |_, cr, width, height| {
                    draw_plane(cr, width as f64, height as f64, plane_hsv);
                }
            });
            field.queue_draw();
            spectrum.set_draw_func({
                let color = color;
                move |_, cr, width, height| {
                    draw_spectrum(cr, width as f64, height as f64, hue, color);
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

    ColorPicker {
        widget,
        repaint: refresh,
    }
}

/// The Color tab: the shared picker bound to the flat background fill.
fn build_color_page(state: &Arc<Mutex<VideoEditState>>, notify: &Rc<dyn Fn()>) -> ColorPage {
    let get: Rc<dyn Fn() -> (u8, u8, u8)> = {
        let state = state.clone();
        Rc::new(move || current_flat_color(&state))
    };
    let set: Rc<dyn Fn((u8, u8, u8))> = {
        let state = state.clone();
        Rc::new(move |color| set_flat_color(&state, color))
    };
    let picker = build_color_picker(get, set, notify.clone());
    ColorPage {
        widget: picker.widget,
        repaint: picker.repaint,
    }
}

/// The saturation/value plane: the current hue across the top, washing to
/// Saturation and value at a point inside the plane's drawn box.
///
/// Saturation runs *backwards* along x, because that is the way the plane is
/// painted: the pure hue fills the left edge and the white overlay grows toward
/// the right, so the right edge is saturation 0. Reading it as if saturation
/// grew to the right is what made a drag commit the mirror of the color under
/// the cursor — the top-right corner, painted white, committed a fully
/// saturated color. Value runs top to bottom, full value at the top.
fn plane_sv_at(x: f64, y: f64, width: f64, height: f64) -> (f64, f64) {
    (
        (1.0 - x / width.max(1.0)).clamp(0.0, 1.0),
        (1.0 - y / height.max(1.0)).clamp(0.0, 1.0),
    )
}

/// Where the plane draws its handle for a saturation and value.
///
/// The exact inverse of `plane_sv_at`, so the dot stays under the cursor that
/// put it there instead of drifting to the mirrored side.
fn plane_handle(saturation: f64, value: f64, width: f64, height: f64) -> (f64, f64) {
    (
        (1.0 - saturation.clamp(0.0, 1.0)) * width,
        (1.0 - value.clamp(0.0, 1.0)) * height,
    )
}

/// white on the right and blackening downward, with a handle on the live
/// color. This is what makes the field a picker rather than a swatch — the
/// reference's red-to-dark wash is this plane, not a flat fill.
fn draw_plane(cr: &gtk4::cairo::Context, w: f64, h: f64, hsv: (f64, f64, f64)) {
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
    let (cx, cy) = plane_handle(saturation, value, w, h);
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

/// Where the hue bar's handle sits for a given hue, kept fully on the bar.
///
/// The handle is drawn at the live color's hue, but a raw `hue * w` parks a
/// red (hue 0) handle at x=0, where half the circle falls off the pill and the
/// round end reads as a clipped crescent rather than a handle. Clamping to the
/// handle's own radius is a nudge, not a lie: the rainbow sweep does start at
/// red, so hue 0 belongs at the left end — it just has to be drawn on top of
/// the bar instead of hanging off it.
fn spectrum_handle_x(hue: f64, w: f64, r: f64) -> f64 {
    (hue * w).clamp(r, (w - r).max(r))
}

/// The rainbow hue bar with a round handle on the current color, matching the
/// reference picker row. The bar is a full-saturation sweep, so dragging the
/// handle sets the field to the pure color under it.
///
/// `hue` is passed in rather than recovered from `color`: a white or black
/// color has no hue to recover, and reading one back would park the handle on
/// red while the picker was really holding, say, green.
fn draw_spectrum(cr: &gtk4::cairo::Context, w: f64, h: f64, hue: f64, color: (u8, u8, u8)) {
    if w < 2.0 || h < 2.0 {
        return;
    }
    // A slim rail centered in the widget: one column per pixel, clipped to a
    // pill so the bar's ends round off, with the tall thumb straddling it.
    let bar_h = SPECTRUM_BAR_THICKNESS.min(h);
    let bar_y = (h - bar_h) / 2.0;
    let _ = cr.save();
    rounded_rect(cr, 0.0, bar_y, w, bar_h, bar_h / 2.0);
    let _ = cr.clip();
    for x in 0..w as i32 {
        let hue = x as f64 / w;
        let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
        cr.set_source_rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
        cr.rectangle(x as f64, bar_y, 1.0, bar_h);
        let _ = cr.fill();
    }
    let _ = cr.restore();

    // The handle, ringed in white so it reads against any hue. It keeps its
    // grabbable size and straddles the slim rail rather than shrinking into it.
    let r = HANDLE_RADIUS;
    let cx = spectrum_handle_x(hue, w, r);
    let cy = h / 2.0;
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
    cr.set_source_rgb(
        color.0 as f64 / 255.0,
        color.1 as f64 / 255.0,
        color.2 as f64 / 255.0,
    );
    cr.arc(cx, cy, (r - 2.5).max(1.0), 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
}

fn current_flat_color(state: &Arc<Mutex<VideoEditState>>) -> (u8, u8, u8) {
    match &state.lock().unwrap().background {
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

/// Resolve the pointer's live position mid-drag: the press point plus the
/// drag's total offset.
///
/// `drag-update` reports the *total* offset from the press, not a per-frame
/// delta, so each update must be resolved fresh from `start_point()` (which
/// never moves during the drag). Folding updates into an accumulator sums
/// totals on top of totals and flings the handle to the edges instead of
/// following the cursor. This is the same `start + offset` pattern the
/// FillSlider, preview, and timeline drags all use.
fn resolve_drag_position(start: (f64, f64), offset: (f64, f64)) -> (f64, f64) {
    (start.0 + offset.0, start.1 + offset.1)
}

/// A widget's drawn size, in the same space as a `GestureDrag`'s x/y and the
/// draw function's width/height.
///
/// GTK's `allocated_width()`/`allocated_height()` are the *margin* box, not the
/// drawn box: the bars here carry 10-12px CSS margins, so the allocation is up
/// to 24px wider than what is painted. Hit-testing against it searches right of
/// the handle — the right-hand gradient pin could only be grabbed by its right
/// edge — and drag mapping lands the handle off the cursor. Always measure with
/// this instead.
fn drawn_size(widget: &Widget) -> (f64, f64) {
    (widget.width().max(1) as f64, widget.height().max(1) as f64)
}

/// The saturation and value a hue drag keeps: the picker's own, rescued when
/// they are degenerate.
///
/// From gray, white, or black (saturation ~0) every hue is the same shade, so
/// keeping saturation would make the rainbow bar a visible no-op until the
/// plane is touched first. Snap saturation to full in that case — and value
/// too when it is so dark no hue could read anyway.
fn hue_adjusted_sv(saturation: f64, value: f64) -> (f64, f64) {
    if saturation < 0.02 {
        (1.0, if value < 0.2 { 1.0 } else { value })
    } else {
        (saturation, value)
    }
}

fn attach_hue_drag(
    spectrum: &DrawingArea,
    hsv: Rc<Cell<(f64, f64, f64)>>,
    set_hsv: Rc<dyn Fn(f64, f64, f64)>,
) {
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let hsv = hsv.clone();
        let set_hsv = set_hsv.clone();
        move |gesture, x, _| {
            apply_hue(&gesture, x, &hsv, &set_hsv);
        }
    });
    drag.connect_drag_update({
        let hsv = hsv.clone();
        let set_hsv = set_hsv.clone();
        move |gesture, offset_x, _| {
            let Some(start) = gesture.start_point() else {
                return;
            };
            let (x, _) = resolve_drag_position(start, (offset_x, 0.0));
            apply_hue(&gesture, x, &hsv, &set_hsv);
        }
    });
    spectrum.add_controller(drag);
}

fn apply_hue(
    gesture: &GestureDrag,
    x: f64,
    hsv: &Rc<Cell<(f64, f64, f64)>>,
    set_hsv: &Rc<dyn Fn(f64, f64, f64)>,
) {
    let Some(widget) = gesture.widget() else {
        return;
    };
    let (width, _) = drawn_size(&widget);
    let hue = (x / width.max(1.0)).clamp(0.0, 1.0);
    let (_, saturation, value) = hsv.get();
    let (saturation, value) = hue_adjusted_sv(saturation, value);
    set_hsv(hue, saturation, value);
}

/// Drag inside the plane: horizontal sets saturation, vertical sets value,
/// both against the hue the picker currently holds.
fn attach_plane_drag(
    field: &DrawingArea,
    hsv: Rc<Cell<(f64, f64, f64)>>,
    set_hsv: Rc<dyn Fn(f64, f64, f64)>,
) {
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let hsv = hsv.clone();
        let set_hsv = set_hsv.clone();
        move |gesture, x, y| {
            apply_plane(&gesture, x, y, &hsv, &set_hsv);
        }
    });
    drag.connect_drag_update({
        let hsv = hsv.clone();
        let set_hsv = set_hsv.clone();
        move |gesture, offset_x, offset_y| {
            let Some(start) = gesture.start_point() else {
                return;
            };
            let (x, y) = resolve_drag_position(start, (offset_x, offset_y));
            apply_plane(&gesture, x, y, &hsv, &set_hsv);
        }
    });
    field.add_controller(drag);
}

fn apply_plane(
    gesture: &GestureDrag,
    x: f64,
    y: f64,
    hsv: &Rc<Cell<(f64, f64, f64)>>,
    set_hsv: &Rc<dyn Fn(f64, f64, f64)>,
) {
    let Some(widget) = gesture.widget() else {
        return;
    };
    let (width, height) = drawn_size(&widget);
    let (saturation, value) = plane_sv_at(x, y, width, height);
    // The hue rides along from the picker's own triple rather than from the
    // committed RGB: black has no hue to recover, so deriving it here reset the
    // plane to red — and the handle to a corner — the moment a drag reached the
    // bottom edge.
    let (hue, _, _) = hsv.get();
    set_hsv(hue, saturation, value);
}

// ── Gradient page ──

#[derive(Clone)]
struct GradientPage {
    widget: GtkBox,
    repaint: Rc<dyn Fn()>,
    /// Closes the stop picker's mini card. The popover that owns this page
    /// calls it on hide and when the Color tab takes over, so the card never
    /// outlives the surface it was opened from.
    dismiss: Rc<dyn Fn()>,
}

/// The two glyphs the gradient header draws itself.
///
/// Transcribed from Lucide's `arrow-left-right` and `rotate-cw-square` (ISC
/// licensed, <https://lucide.dev>) instead of naming symbolic icons: a theme
/// icon resolves to whatever the host desktop ships, so the flip and rotate
/// controls changed shape between Adwaita and every other icon set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GradientIcon {
    /// Lucide `arrow-left-right` — the flip control.
    ArrowLeftRight,
    /// Lucide `rotate-cw-square` — the rotate control.
    RotateCwSquare,
}

/// One stroke of a glyph, in Lucide's 24x24 coordinate space.
///
/// The SVGs' relative commands are folded into absolute points, and their
/// `a`/`A` corners are quarter-turn arcs: SVG's `sweep-flag 0` sweeps the
/// same direction Cairo calls `arc_negative`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum IconSegment {
    Move(f64, f64),
    Line(f64, f64),
    /// A rounded corner: centre, radius, and the angles it sweeps between.
    Corner {
        cx: f64,
        cy: f64,
        r: f64,
        from: f64,
        to: f64,
    },
}

/// The path data of `icon`. Lucide draws these with `fill="none"
/// stroke-width="2"` and round caps and joins; `draw_gradient_icon` sets the
/// same.
fn gradient_icon_path(icon: GradientIcon) -> &'static [IconSegment] {
    match icon {
        // <path d="M8 3 4 7l4 4"/><path d="M4 7h16"/>
        // <path d="m16 21 4-4-4-4"/><path d="M20 17H4"/>
        GradientIcon::ArrowLeftRight => &[
            IconSegment::Move(8.0, 3.0),
            IconSegment::Line(4.0, 7.0),
            IconSegment::Line(8.0, 11.0),
            IconSegment::Move(4.0, 7.0),
            IconSegment::Line(20.0, 7.0),
            IconSegment::Move(16.0, 21.0),
            IconSegment::Line(20.0, 17.0),
            IconSegment::Line(16.0, 13.0),
            IconSegment::Move(20.0, 17.0),
            IconSegment::Line(4.0, 17.0),
        ],
        // <path d="M12 5H6a2 2 0 0 0-2 2v3"/><path d="m9 8 3-3-3-3"/>
        // <path d="M4 14v4a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-2"/>
        GradientIcon::RotateCwSquare => &[
            IconSegment::Move(12.0, 5.0),
            IconSegment::Line(6.0, 5.0),
            IconSegment::Corner {
                cx: 6.0,
                cy: 7.0,
                r: 2.0,
                from: -FRAC_PI_2,
                to: -PI,
            },
            IconSegment::Line(4.0, 10.0),
            IconSegment::Move(4.0, 14.0),
            IconSegment::Line(4.0, 18.0),
            IconSegment::Corner {
                cx: 6.0,
                cy: 18.0,
                r: 2.0,
                from: PI,
                to: FRAC_PI_2,
            },
            IconSegment::Line(18.0, 20.0),
            IconSegment::Corner {
                cx: 18.0,
                cy: 18.0,
                r: 2.0,
                from: FRAC_PI_2,
                to: 0.0,
            },
            IconSegment::Line(20.0, 7.0),
            IconSegment::Corner {
                cx: 18.0,
                cy: 7.0,
                r: 2.0,
                from: 0.0,
                to: -FRAC_PI_2,
            },
            IconSegment::Line(16.0, 5.0),
            // The arrow head on the square's open top-right corner.
            IconSegment::Move(9.0, 8.0),
            IconSegment::Line(12.0, 5.0),
            IconSegment::Line(9.0, 2.0),
        ],
    }
}

/// Strokes a glyph into `cr`, scaled out of Lucide's 24x24 box and painted in
/// `color` — the SVG's `currentColor`, so the light theme and the disabled
/// rotate control need no rules of their own.
fn draw_gradient_icon(
    cr: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    icon: GradientIcon,
    color: gtk4::gdk::RGBA,
) {
    let size = width.min(height).max(1.0);
    let _ = cr.save();
    cr.translate((width - size) / 2.0, (height - size) / 2.0);
    cr.scale(size / 24.0, size / 24.0);
    cr.set_line_width(2.0);
    cr.set_line_cap(gtk4::cairo::LineCap::Round);
    cr.set_line_join(gtk4::cairo::LineJoin::Round);
    cr.set_source_rgba(
        color.red().into(),
        color.green().into(),
        color.blue().into(),
        color.alpha().into(),
    );
    for segment in gradient_icon_path(icon) {
        match *segment {
            IconSegment::Move(x, y) => cr.move_to(x, y),
            IconSegment::Line(x, y) => cr.line_to(x, y),
            IconSegment::Corner {
                cx,
                cy,
                r,
                from,
                to,
            } => cr.arc_negative(cx, cy, r, from, to),
        }
    }
    let _ = cr.stroke();
    let _ = cr.restore();
}

/// A flat icon button for the gradient header.
fn gradient_icon_button(icon: GradientIcon, tooltip: &str) -> Button {
    let button = Button::new();
    button.add_css_class("recording-editor-gradient-icon");
    button.set_has_frame(false);
    button.set_valign(Align::Center);
    button.set_tooltip_text(Some(tooltip));
    let glyph = DrawingArea::new();
    glyph.set_content_width(GRADIENT_ICON_SIZE);
    glyph.set_content_height(GRADIENT_ICON_SIZE);
    glyph.set_halign(Align::Center);
    glyph.set_valign(Align::Center);
    // Clicks — and the hover state behind them — belong to the button.
    glyph.set_can_target(false);
    glyph.set_draw_func(move |glyph, cr, width, height| {
        let color = glyph.style_context().color();
        draw_gradient_icon(cr, f64::from(width), f64::from(height), icon, color);
    });
    button.set_child(Some(&glyph));
    button
}

fn kind_label(kind: GradientKind) -> String {
    match kind {
        GradientKind::Linear => t("Linear"),
        GradientKind::Radial => t("Radial"),
        GradientKind::Angular => t("Angular"),
        GradientKind::Diamond => t("Diamond"),
    }
}

/// The popover's own body: the card the Gradient page is painted on.
///
/// The stop picker is hung off *this* widget rather than off the page, and
/// with no pointing rect, so GTK points the picker at its parent's whole area —
/// a rect it measures itself. Every hand-built rect here landed in the wrong
/// space: a popover hung off a widget inside another popover has its rect
/// measured into that popover's surface, so the card kept opening up over the
/// video no matter which box the rect was taken from.
fn build_gradient_page(
    state: &Arc<Mutex<VideoEditState>>,
    notify: &Rc<dyn Fn()>,
    card_body: &GtkBox,
) -> GradientPage {
    let widget = GtkBox::new(Orientation::Vertical, 0);
    widget.set_hexpand(true);

    // The selected stop: its pin carries a ring and its row a highlight.
    // `usize::MAX` means nothing is selected yet; the first refresh picks the
    // last stop, like a freshly opened gradient editor.
    let selected = Rc::new(Cell::new(usize::MAX));
    // True while the Steps list is being torn down. Entries in that list
    // commit on focus-out, and a destroyed entry must not write through a
    // stale row index.
    let rebuilding = Rc::new(Cell::new(false));

    // ── Header: type dropdown, flip, rotate — Figma's compact control row. ──
    let header = GtkBox::new(Orientation::Horizontal, 6);
    header.add_css_class("recording-editor-gradient-header");

    // The type picker is the app's own dropdown (button + styled popover),
    // not a stock GTK one, so its menu matches the editor rather than picking
    // up the desktop theme. The active type carries a check, like Figma's.
    let type_button = Button::new();
    type_button.set_has_frame(false);
    type_button.add_css_class("recording-editor-dropdown");
    type_button.add_css_class("recording-editor-gradient-type");
    type_button.set_valign(Align::Center);
    let type_row = GtkBox::new(Orientation::Horizontal, 6);
    type_row.set_halign(Align::Start);
    let type_label = Label::new(Some(&kind_label(GradientKind::Linear)));
    type_label.add_css_class("recording-editor-dropdown-label");
    type_row.append(&type_label);
    let type_arrow = Image::from_icon_name("pan-down-symbolic");
    type_arrow.add_css_class("recording-editor-dropdown-arrow");
    type_arrow.set_pixel_size(10);
    type_row.append(&type_arrow);
    type_button.set_child(Some(&type_row));

    let type_popover = Popover::new();
    type_popover.set_has_arrow(false);
    type_popover.add_css_class("recording-editor-dropdown-popover");
    let type_list = GtkBox::new(Orientation::Vertical, 0);
    type_list.add_css_class("recording-editor-dropdown-list");
    type_popover.set_child(Some(&type_list));
    type_popover.set_parent(&type_button);
    {
        let popover = type_popover.clone();
        type_button.connect_clicked(move |_| popover.popup());
    }

    let mut type_checks: Vec<(GradientKind, Image)> = Vec::new();
    for kind in [
        GradientKind::Linear,
        GradientKind::Radial,
        GradientKind::Angular,
        GradientKind::Diamond,
    ] {
        let item = Button::new();
        item.set_has_frame(false);
        item.add_css_class("recording-editor-dropdown-item");
        let row = GtkBox::new(Orientation::Horizontal, 8);
        let check = Image::from_icon_name("object-select-symbolic");
        check.set_pixel_size(12);
        // Hidden with opacity rather than visibility so every label starts at
        // the same x, checked or not.
        check.set_opacity(0.0);
        let label = Label::new(Some(&kind_label(kind)));
        label.set_xalign(0.0);
        row.append(&check);
        row.append(&label);
        item.set_child(Some(&row));
        {
            let state = state.clone();
            let notify = notify.clone();
            let popover = type_popover.clone();
            item.connect_clicked(move |_| {
                with_gradient(&state, |gradient| gradient.kind = kind);
                notify();
                popover.popdown();
            });
        }
        type_list.append(&item);
        type_checks.push((kind, check));
    }

    // Flip reverses the stop order; rotate turns the gradient a quarter turn.
    let flip = gradient_icon_button(GradientIcon::ArrowLeftRight, &t("Flip gradient"));
    {
        let state = state.clone();
        let notify = notify.clone();
        flip.connect_clicked(move |_| {
            with_gradient(&state, |gradient| gradient.reversed = !gradient.reversed);
            notify();
        });
    }
    let rotate = gradient_icon_button(GradientIcon::RotateCwSquare, &t("Rotate gradient"));
    {
        let state = state.clone();
        let notify = notify.clone();
        rotate.connect_clicked(move |_| {
            with_gradient(&state, |gradient| {
                gradient.angle_degrees =
                    (gradient.angle_degrees + GRADIENT_ROTATE_STEP).rem_euclid(360.0);
            });
            notify();
        });
    }

    header.append(&type_button);
    // A compact chip on the left, icon controls pushed to the right.
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    header.append(&spacer);
    header.append(&flip);
    header.append(&rotate);
    widget.append(&header);

    // ── The stop bar: the ramp itself with a pin per stop. ──
    // Drag a pin to move its stop; the Stops + button is how a stop is added.
    let bar = DrawingArea::new();
    bar.add_css_class("recording-editor-gradient-bar");
    bar.set_content_height(GRADIENT_BAR_HEIGHT);
    widget.append(&bar);

    // ── Stops header + list. ──
    let steps_header = GtkBox::new(Orientation::Horizontal, 8);
    steps_header.add_css_class("recording-editor-gradient-steps-header");
    let steps_title = Label::new(Some(&t("Stops")));
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
        let selected = selected.clone();
        add.connect_clicked(move |_| {
            selected.set(add_stop(&state));
            notify();
        });
    }
    steps_header.append(&steps_title);
    steps_header.append(&add);
    widget.append(&steps_header);

    let steps = Grid::new();
    steps.add_css_class("recording-editor-gradient-steps");
    steps.set_column_spacing(8);
    steps.set_row_spacing(6);
    steps.set_hexpand(true);
    widget.append(&steps);

    // ── The stop picker: the Color tab's picker in its own mini card. ──
    // The card is a popover of its own, opened clear of the popover's left
    // edge, so the Gradient tab stays the Color tab's width instead of growing
    // a second column that widens the whole popover. It carries no
    // Color/Gradient tabs of its own — those are the popover's — and it edits
    // whichever stop the last tile click selected, which is why it reads and
    // writes through closures instead of reaching into `background` directly.
    let picker_popover = Popover::new();
    picker_popover.add_css_class("recording-editor-gradient-picker-popover");
    picker_popover.set_has_arrow(false);
    picker_popover.set_position(gtk4::PositionType::Left);
    // An inspector, not a menu: it stays put while the ramp, the type chip and
    // the other tiles are used. An autohiding card would also eat the first
    // click on the next tile while closing itself, so the card never opens on
    // the second try.
    picker_popover.set_autohide(false);
    // Top-aligned rather than centred: GTK anchors a left-positioned popover's
    // corner to the pointing rect's origin, not to the edge's centre, so a
    // centred rect is what kept parking the card ~150px high. With START the
    // card's top-right corner lands on the rect's top-left corner — a seat
    // stated in the body's own coordinates and nothing else.
    picker_popover.set_valign(Align::Start);
    picker_popover.set_parent(card_body);

    let stop_picker = {
        let get: Rc<dyn Fn() -> (u8, u8, u8)> = {
            let state = state.clone();
            let selected = selected.clone();
            Rc::new(move || {
                let guard = state.lock().unwrap();
                if let VideoBackground::Gradient(gradient) = &guard.background {
                    if let Some(stop) = gradient.normalized().stops.get(selected.get()) {
                        return (stop.r, stop.g, stop.b);
                    }
                }
                (0xFF, 0xFF, 0xFF)
            })
        };
        let set: Rc<dyn Fn((u8, u8, u8))> = {
            let state = state.clone();
            let selected = selected.clone();
            Rc::new(move |color| {
                let index = selected.get();
                with_gradient(&state, |gradient| {
                    if let Some(stop) = gradient.stops.get_mut(index) {
                        stop.r = color.0;
                        stop.g = color.1;
                        stop.b = color.2;
                    }
                });
            })
        };
        build_color_picker(get, set, notify.clone())
    };
    // The card is the picker's own surface: a header carrying its close, above
    // the picker. The picker keeps the Color tab's margins, so the two read as
    // the same picker.
    stop_picker
        .widget
        .add_css_class("recording-editor-gradient-picker");
    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("recording-editor-gradient-picker-card");
    let card_header = GtkBox::new(Orientation::Horizontal, 0);
    card_header.add_css_class("recording-editor-gradient-picker-header");
    let card_spacer = GtkBox::new(Orientation::Horizontal, 0);
    card_spacer.set_hexpand(true);
    // The popover behind the card stays open while the card is up, so the card
    // needs a close of its own: clicking the same tile again also closes it,
    // but that is not something the card can advertise.
    let card_close = Button::new();
    card_close.add_css_class("recording-editor-gradient-picker-close");
    card_close.set_has_frame(false);
    card_close.set_tooltip_text(Some(&t("Close")));
    let card_close_icon = Image::from_icon_name("window-close-symbolic");
    card_close_icon.set_pixel_size(13);
    card_close.set_child(Some(&card_close_icon));
    card_header.append(&card_spacer);
    card_header.append(&card_close);
    card.append(&card_header);
    card.append(&stop_picker.widget);
    picker_popover.set_child(Some(&card));
    widget.add_css_class("recording-editor-gradient-editor");

    // Whether the card is currently up. Tracked here rather than read back off
    // the popover: `is_visible()` also reports the ancestors' visibility, so
    // it reads false the moment the Gradient page is hidden, which is exactly
    // when the card still has to be popped down.
    let card_up = Rc::new(Cell::new(false));
    picker_popover.connect_closed({
        let card_up = card_up.clone();
        move |_| card_up.set(false)
    });

    // Show the card. The rect is in the body's own coordinate space — the space
    // GTK measures a popover's rect in, and the space this card's parent lives
    // in — and with `valign` START it is read as a corner, so the seat is
    // simply "the card's top-right corner PICKER_CARD_GAP left of the body's
    // top-left corner". That is measured, not derived: see
    // `the_card_hangs_clear_of_the_popover_beside_it`. Seats built from any
    // other box (the page's, the popover's surface) landed the card up over the
    // video, because a popover hung inside another popover has its rect measured
    // through that popover's surface.
    // `was_selected` lets a second click on the same tile close the card
    // instead of leaving it up.
    let open_picker: Rc<dyn Fn(bool)> = {
        let picker_popover = picker_popover.clone();
        let card_body = card_body.clone();
        let card_up = card_up.clone();
        Rc::new(move |was_selected: bool| {
            if was_selected && card_up.get() {
                card_up.set(false);
                picker_popover.popdown();
                return;
            }
            picker_popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
                -PICKER_CARD_GAP,
                0,
                1,
                card_body.height().max(1),
            )));
            card_up.set(true);
            picker_popover.popup();
        })
    };

    let dismiss: Rc<dyn Fn()> = {
        let picker_popover = picker_popover.clone();
        Rc::new(move || {
            if card_up.replace(false) {
                picker_popover.popdown();
            }
        })
    };
    card_close.connect_clicked({
        let dismiss = dismiss.clone();
        move |_| dismiss()
    });

    attach_stop_drag(&bar, state.clone(), notify.clone(), selected.clone());

    let refresh: Rc<dyn Fn()> = {
        let bar = bar.clone();
        let steps = steps.clone();
        let add = add.clone();
        let rotate = rotate.clone();
        let type_label = type_label.clone();
        let type_checks = type_checks.clone();
        let selected = selected.clone();
        let rebuilding = rebuilding.clone();
        let state = state.clone();
        let notify = notify.clone();
        let picker_repaint = stop_picker.repaint.clone();
        Rc::new(move || {
            let gradient = current_gradient(&state);
            let normalized = gradient.normalized();
            let count = normalized.stops.len();
            // Selection rides along with edits and removals.
            if selected.get() >= count {
                selected.set(count.saturating_sub(1));
            }
            let selected_index = selected.get();

            bar.set_draw_func({
                let gradient = gradient.clone();
                move |_, cr, width, height| {
                    draw_stop_bar(cr, width as f64, height as f64, &gradient, selected_index);
                }
            });
            bar.queue_draw();

            type_label.set_text(&kind_label(gradient.kind));
            for (kind, check) in &type_checks {
                check.set_opacity(if *kind == gradient.kind { 1.0 } else { 0.0 });
            }
            // A radial or diamond gradient has no direction to rotate.
            rotate.set_sensitive(matches!(
                gradient.kind,
                GradientKind::Linear | GradientKind::Angular
            ));
            rotate.set_tooltip_text(Some(&format!(
                "{} ({}°)",
                t("Rotate gradient"),
                gradient.angle_degrees.round() as i64
            )));

            // The Steps list is tiny (2..=8 rows), so rebuilding it is cheaper
            // and far less error-prone than tracking per-row widget state.
            rebuilding.set(true);
            while let Some(child) = steps.first_child() {
                steps.remove(&child);
            }
            add.set_sensitive(count < MAX_GRADIENT_STOPS);
            for (index, stop) in normalized.stops.iter().enumerate() {
                let row_widget = build_stop_row(
                    stop,
                    index,
                    &selected,
                    &state,
                    &notify,
                    &rebuilding,
                    &open_picker,
                );
                steps.attach(&row_widget, 0, index as i32, 1, 1);
            }
            rebuilding.set(false);
            // The mini card follows the selection, so it repaints last: the
            // selected index is already clamped to the current stop count.
            picker_repaint();
        }) as Rc<dyn Fn()>
    };

    GradientPage {
        widget,
        repaint: refresh,
        dismiss,
    }
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

/// Append a stop in the widest gap and return its index in the normalized
/// list, so the caller can select it. Adding over a full gradient is a no-op
/// that reports the last stop.
fn add_stop(state: &Arc<Mutex<VideoEditState>>) -> usize {
    let mut position = 0.5;
    with_gradient(state, |gradient| {
        if gradient.stops.len() >= MAX_GRADIENT_STOPS {
            return;
        }
        // Insert into the widest gap so the new stop is visible and editable
        // rather than landing on top of an existing one.
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
        // The ramp is mirrored when the gradient is flipped, so sample the
        // position the new stop will actually display at.
        let display = if gradient.reversed {
            1.0 - position
        } else {
            position
        };
        let color = sample_color_at(gradient, display);
        gradient
            .stops
            .push(GradientStop::new(position, color.0, color.1, color.2));
    });
    find_stop_at(state, position)
}

/// The model index of the stop sitting at `position`, for reselecting after a
/// list re-sort.
fn find_stop_at(state: &Arc<Mutex<VideoEditState>>, position: f64) -> usize {
    current_gradient(state)
        .stops
        .iter()
        .position(|stop| (stop.position - position).abs() < 1e-6)
        .unwrap_or(0)
}

/// Blit a rendered gradient into a Cairo surface.
///
/// Shared with the Background panel's fill chip, which previews the same fill
/// and so has to convert the rasterizer's RGB exactly the same way.
pub(super) fn bitmap_to_surface(
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

/// The gradient bar's handles in display order: `(position along the bar,
/// index into the model's normalized stops, stop)`.
///
/// Reversal mirrors the bar, so the leftmost pin is the last model stop when
/// the gradient is flipped; each pin keeps the model index a write has to
/// target.
fn bar_handles(gradient: &VideoGradient) -> Vec<(f64, usize, GradientStop)> {
    let normalized = gradient.normalized();
    let mut handles: Vec<(f64, usize, GradientStop)> = normalized
        .stops
        .iter()
        .enumerate()
        .map(|(index, stop)| {
            let position = if normalized.reversed {
                1.0 - stop.position
            } else {
                stop.position
            };
            (position, index, *stop)
        })
        .collect();
    handles.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    handles
}

/// The y of the ramp's track inside the bar widget.
fn gradient_track_top() -> f64 {
    (GRADIENT_BAR_HEIGHT as f64 - GRADIENT_BAR_TRACK) / 2.0
}

/// The y a handle is centred on: the middle of the track, so the handle
/// straddles the ramp instead of floating above it.
fn gradient_pin_center_y() -> f64 {
    gradient_track_top() + GRADIENT_BAR_TRACK / 2.0
}

/// How far a handle's centre stays from the bar's ends. A handle at either end
/// has to leave room for the selection ring (and the stroke's half-width) or
/// the ring is clipped, which read as the handle being boxed into the ramp.
fn gradient_pin_inset() -> f64 {
    GRADIENT_PIN_SIZE / 2.0 + GRADIENT_PIN_RING + 1.0
}

/// x of the handle for `position` along the bar.
fn gradient_pin_x(position: f64, w: f64) -> f64 {
    let inset = gradient_pin_inset();
    let span = (w - inset * 2.0).max(1.0);
    inset + position * span
}

/// The stop position a handle centre at `x` stands for — the inverse of
/// `gradient_pin_x`, so a drag maps the cursor back onto the same scale the
/// ramp was drawn with.
fn gradient_position_at(x: f64, w: f64) -> f64 {
    let inset = gradient_pin_inset();
    let span = (w - inset * 2.0).max(1.0);
    ((x - inset) / span).clamp(0.0, 1.0)
}

/// Border color that keeps a handle readable on the ramp: a light stop gets a
/// dark border, everything else the white one.
fn handle_ring_color(color: (u8, u8, u8)) -> (f64, f64, f64) {
    let luminance =
        (0.2126 * color.0 as f64 + 0.7152 * color.1 as f64 + 0.0722 * color.2 as f64) / 255.0;
    if luminance > 0.6 {
        (0.09, 0.09, 0.11)
    } else {
        (0.97, 0.97, 0.97)
    }
}

/// A stop handle: a rounded square in the stop's colour, centred on the ramp
/// so it sits on the track like a chip. The selected handle wears a ring,
/// which `gradient_pin_inset` leaves room for at the bar's ends.
fn draw_stop_pin(cr: &gtk4::cairo::Context, cx: f64, stop: &GradientStop, selected: bool) {
    let (border_r, border_g, border_b) = handle_ring_color((stop.r, stop.g, stop.b));
    let size = GRADIENT_PIN_SIZE;
    let top = gradient_pin_center_y() - size / 2.0;
    let left = cx - size / 2.0;
    let radius = 6.0;

    let _ = cr.save();
    // The ring goes down first so the handle's own border sits over its
    // inner edge rather than the other way around.
    if selected {
        rounded_rect(
            cr,
            left - GRADIENT_PIN_RING,
            top - GRADIENT_PIN_RING,
            size + GRADIENT_PIN_RING * 2.0,
            size + GRADIENT_PIN_RING * 2.0,
            radius + GRADIENT_PIN_RING,
        );
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.95);
        cr.set_line_width(2.0);
        let _ = cr.stroke();
    }

    rounded_rect(cr, left, top, size, size, radius);
    cr.set_source_rgba(
        stop.r as f64 / 255.0,
        stop.g as f64 / 255.0,
        stop.b as f64 / 255.0,
        stop.a as f64 / 255.0,
    );
    let _ = cr.fill_preserve();
    cr.set_source_rgb(border_r, border_g, border_b);
    cr.set_line_width(1.5);
    let _ = cr.stroke();
    let _ = cr.restore();
}

/// The stop's color chip. A lowered opacity shows over a checkerboard so the
/// row reflects it rather than always painting the pure color.
fn draw_stop_chip(cr: &gtk4::cairo::Context, w: f64, h: f64, color: (u8, u8, u8, u8)) {
    let _ = cr.save();
    rounded_rect(cr, 0.0, 0.0, w, h, STEP_CHIP_RADIUS);
    cr.clip();
    if color.3 < u8::MAX {
        let cell = 6.0;
        let mut y = 0.0;
        while y < h {
            let mut x = 0.0;
            while x < w {
                let dark = ((x / cell) as i32 + (y / cell) as i32) % 2 == 0;
                let value = if dark { 0.34 } else { 0.22 };
                cr.set_source_rgba(value, value, value, 1.0);
                cr.rectangle(x, y, cell.min(w - x), cell.min(h - y));
                let _ = cr.fill();
                x += cell;
            }
            y += cell;
        }
    }
    cr.set_source_rgba(
        color.0 as f64 / 255.0,
        color.1 as f64 / 255.0,
        color.2 as f64 / 255.0,
        color.3 as f64 / 255.0,
    );
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();
    let _ = cr.restore();
    stroke_rounded(cr, 0.0, 0.0, w, h, STEP_CHIP_RADIUS, false);
}

fn draw_stop_bar(
    cr: &gtk4::cairo::Context,
    w: f64,
    h: f64,
    gradient: &VideoGradient,
    selected: usize,
) {
    if w < 2.0 || h < 2.0 {
        return;
    }
    // The bar is the stop editor, so it always runs left-to-right: kind and
    // angle belong to the canvas, and applying them here would slide the pins
    // off the stops they belong to. Reversal does apply — it is a property of
    // the stop order itself.
    let mut flat = gradient.clone();
    flat.kind = GradientKind::Linear;
    flat.angle_degrees = 0.0;
    let track_top = gradient_track_top();
    let track_h = GRADIENT_BAR_TRACK.min(h - track_top).max(1.0);
    // Half resolution is finer than the widget can show and keeps a redraw on
    // every drag step cheap.
    let bitmap = render_gradient(&flat, (w * 0.5) as u32, (track_h * 0.5) as u32);
    let surface = bitmap_to_surface(&bitmap);
    let radius = track_h / 2.0;
    let _ = cr.save();
    rounded_rect(cr, 0.0, track_top, w, track_h, radius);
    cr.clip();
    cr.translate(0.0, track_top);
    cr.scale(2.0, 2.0);
    let _ = cr.set_source_surface(&surface, 0.0, 0.0);
    let _ = cr.paint();
    let _ = cr.restore();

    // Handles are drawn outside the track's clip so end stops stay whole.
    for (position, index, stop) in bar_handles(gradient) {
        draw_stop_pin(cr, gradient_pin_x(position, w), &stop, index == selected);
    }
    stroke_rounded(cr, 0.0, track_top, w, track_h, radius, false);
}

/// Commit an entry on Enter or focus-out, like the Color tab's hex field.
///
/// Rows are rebuilt on every refresh, so a commit arriving from an entry that
/// is being torn down is ignored rather than writing through a stale index.
fn commit_entry(entry: &Entry, rebuilding: &Rc<Cell<bool>>, apply: impl Fn(&str) + 'static) {
    let apply = Rc::new(apply);
    entry.connect_activate({
        let apply = apply.clone();
        let rebuilding = rebuilding.clone();
        move |entry| {
            if !rebuilding.get() {
                apply(&entry.text());
            }
        }
    });
    let focus = gtk4::EventControllerFocus::new();
    focus.connect_leave({
        let apply = apply.clone();
        let rebuilding = rebuilding.clone();
        move |controller| {
            if rebuilding.get() {
                return;
            }
            let Some(entry) = controller.widget().and_then(|w| w.downcast::<Entry>().ok()) else {
                return;
            };
            apply(&entry.text());
        }
    });
    entry.add_controller(focus);
}

/// A stop row: the color chip and its hex, and nothing else.
///
/// Position and opacity are deliberately not shown. A stop's position is the
/// pin on the bar above — the row would only be a second, disagreeing place to
/// type the same number — and alpha is not something the reference's gradient
/// editor exposes per stop.
fn build_stop_row(
    stop: &GradientStop,
    index: usize,
    selected: &Rc<Cell<usize>>,
    state: &Arc<Mutex<VideoEditState>>,
    notify: &Rc<dyn Fn()>,
    rebuilding: &Rc<Cell<bool>>,
    open_picker: &Rc<dyn Fn(bool)>,
) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("recording-editor-gradient-step");
    if selected.get() == index {
        row.add_css_class("selected");
    }
    row.set_hexpand(true);

    // The color chip: clicking it selects this stop and opens the picker's
    // mini card. There is deliberately no modal chooser — the card is the one
    // editing surface, and it edits the stop the click just selected.
    let swatch = DrawingArea::new();
    swatch.add_css_class("recording-editor-gradient-step-swatch");
    swatch.set_content_width(STEP_CHIP_SIZE);
    swatch.set_content_height(STEP_CHIP_SIZE);
    swatch.set_valign(Align::Center);
    let color = (stop.r, stop.g, stop.b, stop.a);
    swatch.set_draw_func(move |_, cr, width, height| {
        draw_stop_chip(cr, width as f64, height as f64, color);
    });
    {
        let gesture = GestureClick::new();
        let selected = selected.clone();
        let notify = notify.clone();
        let open_picker = open_picker.clone();
        gesture.connect_released(move |_, _, _, _| {
            // A tile that already carries the card closes it on a second
            // click; any other tile moves the selection and opens it.
            let was_selected = selected.get() == index;
            if !was_selected {
                selected.set(index);
            }
            open_picker(was_selected);
            if !was_selected {
                notify();
            }
        });
        swatch.add_controller(gesture);
    }

    let hex = Entry::new();
    hex.add_css_class("recording-editor-gradient-step-hex");
    hex.set_valign(Align::Center);
    hex.set_hexpand(true);
    hex.set_text(&hex_string((stop.r, stop.g, stop.b)));
    {
        let state = state.clone();
        let notify = notify.clone();
        commit_entry(&hex, rebuilding, move |text| {
            let Some((r, g, b)) = parse_hex(text) else {
                return;
            };
            with_gradient(&state, |gradient| {
                if let Some(stop) = gradient.stops.get_mut(index) {
                    stop.r = r;
                    stop.g = g;
                    stop.b = b;
                }
            });
            notify();
        });
    }

    row.append(&swatch);
    row.append(&hex);
    row
}

/// Drag a stop along the bar. The nearest handle within a grab radius wins; a
/// press on empty track does nothing, because the only way to add a stop is
/// the Stops + button.
fn attach_stop_drag(
    bar: &DrawingArea,
    state: Arc<Mutex<VideoEditState>>,
    notify: Rc<dyn Fn()>,
    selected: Rc<Cell<usize>>,
) {
    // The model index of the stop being moved. It is re-resolved after every
    // write because `normalized()` re-sorts by position, so a stop dragged
    // past another changes index mid-drag.
    let dragging = Rc::new(Cell::new(usize::MAX));
    // Where inside the handle the press landed. The handle keeps this offset
    // from the cursor for the whole drag, so grabbing it off-centre does not
    // make it jump under the pointer.
    let grab_offset = Rc::new(Cell::new(0.0));
    let drag = GestureDrag::new();
    drag.set_button(1);
    // Keep the sequence even when the pointer strays off the popover's own
    // surface. The event-controller default is `SameNative`, which drops the
    // drag the instant the cursor crosses onto the window beneath — easy to
    // do when the stop being dragged sits near the card's edge, which is why
    // the far handle felt like something was stealing the press.
    drag.set_propagation_limit(gtk4::PropagationLimit::None);
    drag.connect_drag_begin({
        let state = state.clone();
        let notify = notify.clone();
        let dragging = dragging.clone();
        let selected = selected.clone();
        let grab_offset = grab_offset.clone();
        move |gesture, x, _| {
            let Some(widget) = gesture.widget() else {
                return;
            };
            let (width, _) = drawn_size(&widget);
            let gradient = current_gradient(&state);

            // Distance is measured along the bar only: the handle sits on the
            // track, so a press at either end of the ramp still belongs to it.
            let mut best = usize::MAX;
            let mut best_distance = f64::MAX;
            let mut best_x = 0.0;
            for (position, index, _) in bar_handles(&gradient) {
                let cx = gradient_pin_x(position, width);
                let distance = (cx - x).abs();
                if distance < best_distance {
                    best_distance = distance;
                    best_x = cx;
                    best = index;
                }
            }
            // Empty track: nothing to grab, so there is nothing to move. A
            // press here must not create a stop — the + button owns that, so
            // a stray click on the ramp cannot silently rewrite the gradient.
            if best_distance <= GRADIENT_GRAB_RADIUS {
                dragging.set(best);
                grab_offset.set(x - best_x);
                if selected.get() != best {
                    selected.set(best);
                    // Repaint the ring at once, but keep the full refresh out
                    // of `drag-begin`. Running the editor ping and the Stops
                    // rebuild inside the signal cost the gesture its sequence:
                    // the first press on an unselected handle selected it and
                    // then dropped the drag, so it took another press to move.
                    if let Some(widget) = gesture.widget() {
                        widget.queue_draw();
                    }
                    let notify = notify.clone();
                    gtk4::glib::idle_add_local_once(move || notify());
                }
                return;
            }
            dragging.set(usize::MAX);
        }
    });
    drag.connect_drag_update({
        let state = state.clone();
        let notify = notify.clone();
        let dragging = dragging.clone();
        let selected = selected.clone();
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
            let (width, _) = drawn_size(&widget);
            // The handle centre keeps the press's offset from the cursor, and
            // the position comes from the inverse of the handle placement, so
            // the drag tracks what was drawn.
            let center = (start + dx) - grab_offset.get();
            let display = gradient_position_at(center, width);
            let reversed = current_gradient(&state).reversed;
            let position = if reversed { 1.0 - display } else { display };
            with_gradient(&state, |gradient| {
                if let Some(stop) = gradient.stops.get_mut(index) {
                    stop.position = position;
                }
            });
            // Re-resolve: the stop that was at `index` may now be elsewhere.
            let index = find_stop_at(&state, position);
            dragging.set(index);
            selected.set(index);
            notify();
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
    fn the_rail_is_slimmer_than_the_thumb() {
        // The rail is the slim strip; the thumb straddles it. Shrinking the
        // thumb into the rail killed its grabbability, so pin the split: a
        // thin rail, a full-size handle.
        assert!(
            SPECTRUM_BAR_THICKNESS < HANDLE_RADIUS * 2.0,
            "the rail ({}) must be slimmer than the thumb ({})",
            SPECTRUM_BAR_THICKNESS,
            HANDLE_RADIUS * 2.0
        );
        // The widget must be tall enough for the whole thumb to show.
        assert!(
            f64::from(SPECTRUM_CONTENT_HEIGHT) >= HANDLE_RADIUS * 2.0,
            "the widget must fit the full thumb"
        );
    }

    #[test]
    fn a_drag_update_is_a_total_offset_not_a_delta() {
        // `drag-update` reports the total offset from the press, so the live
        // position is the press point plus that offset, resolved fresh on every
        // update. Summing updates into an accumulator adds totals on top of
        // totals, which flung the plane's handle to the edges instead of
        // following the cursor.
        assert_eq!(
            resolve_drag_position((100.0, 40.0), (7.0, 0.0)),
            (107.0, 40.0)
        );
        // The same update repeated is the same position, not further travel.
        assert_eq!(
            resolve_drag_position((100.0, 40.0), (7.0, 0.0)),
            (107.0, 40.0)
        );
        assert_eq!(
            resolve_drag_position((100.0, 40.0), (10.0, -10.0)),
            (110.0, 30.0)
        );
    }

    #[test]
    fn the_hue_bar_wakes_up_from_gray() {
        // Keeping saturation makes the rainbow a no-op from gray, white, or
        // black — every hue is the same shade there, which is why the bar only
        // worked after the plane was touched first. From those it must snap to
        // the vivid hue instead.
        let from = |color: (u8, u8, u8), hue: f64| {
            let (_, saturation, value) = rgb_to_hsv(color.0, color.1, color.2);
            let (saturation, value) = hue_adjusted_sv(saturation, value);
            hsv_to_rgb(hue, saturation, value)
        };
        assert_eq!(from((255, 255, 255), 0.0), (255, 0, 0));
        assert_eq!(from((128, 128, 128), 1.0 / 3.0), (0, 128, 0));
        // Near-black also recovers value, or no hue could read anyway.
        assert_eq!(from((17, 17, 17), 2.0 / 3.0), (0, 0, 255));
        // A real color keeps its own saturation and value — only hue moves.
        assert_eq!(from((255, 128, 128), 1.0 / 3.0), (128, 255, 128));
    }

    #[test]
    fn the_plane_maps_the_pointer_to_the_color_under_it() {
        // The reported bug: the plane paints the pure hue down its left edge and
        // washes it to white on the right, but the drag mapping read saturation
        // as if it grew to the right. The color committed was the mirror of the
        // one under the cursor — the top-right corner, painted white, committed
        // a fully saturated color.
        let (w, h) = (400.0, 200.0);
        assert_eq!(plane_sv_at(w, 0.0, w, h), (0.0, 1.0), "top right is white");
        assert_eq!(
            plane_sv_at(0.0, 0.0, w, h),
            (1.0, 1.0),
            "top left is the hue"
        );
        assert_eq!(
            plane_sv_at(0.0, h, w, h),
            (1.0, 0.0),
            "bottom left is black"
        );
        // A point outside the box clamps instead of picking past the ends.
        assert_eq!(plane_sv_at(w + 50.0, -50.0, w, h), (0.0, 1.0));
        // The handle is the exact inverse, or the dot drifts away from the
        // cursor that placed it.
        for (saturation, value) in [(0.0, 1.0), (1.0, 1.0), (0.25, 0.5), (0.5, 0.75), (1.0, 0.0)] {
            let (x, y) = plane_handle(saturation, value, w, h);
            let (read_s, read_v) = plane_sv_at(x, y, w, h);
            assert!(
                (read_s - saturation).abs() < 1e-9 && (read_v - value).abs() < 1e-9,
                "({saturation}, {value}) drew at ({x}, {y}) and read back as ({read_s}, {read_v})"
            );
        }
        // Both halves have to go through these helpers, or one can be corrected
        // without the other and the mirror comes back.
        let source = include_str!("custom_wallpaper_popover.rs");
        let production = &source[..source.find("\n#[cfg(test)]").expect("tests module")];
        let body = |name: &str| {
            let start = production
                .find(name)
                .unwrap_or_else(|| panic!("{name} exists"));
            let end = production[start..]
                .find("\nfn ")
                .map(|at| start + at)
                .unwrap_or(production.len());
            &production[start..end]
        };
        assert!(
            body("fn apply_plane(").contains("plane_sv_at(x, y, width, height)"),
            "the plane drag must map the pointer through plane_sv_at"
        );
        assert!(
            body("fn draw_plane(").contains("plane_handle(saturation, value, w, h)"),
            "the plane must draw its handle through plane_handle"
        );
        // Neither the drag nor the bar may re-derive a hue from RGB: white and
        // black have none, so doing that parks the handle on red.
        for name in ["fn apply_plane(", "fn draw_spectrum("] {
            assert!(
                !body(name).contains("rgb_to_hue"),
                "{name} must take the hue the picker holds, not recover one from RGB"
            );
        }
    }

    #[test]
    fn the_plane_paints_the_color_the_pointer_would_pick() {
        // The paint and the mapping are two halves of one contract: the pixel
        // under the handle has to be the color a drag there commits. Rendering
        // the plane offscreen holds the two halves against each other instead of
        // against the gradient code's own arithmetic — and it pins the
        // orientation the reference shows, with the hue at the left edge and
        // white at the right, not the mirror of it.
        let (w, h) = (120.0, 90.0);
        let hue = 0.0;
        let mut surface = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::Rgb24, 120, 90)
            .expect("plane surface");
        let cr = gtk4::cairo::Context::new(&surface).expect("cairo context");
        // A mid saturation and value parks the handle in the middle, well clear
        // of the pixels sampled below.
        draw_plane(&cr, w, h, (hue, 0.5, 0.5));
        drop(cr);
        surface.flush();

        let stride = surface.stride() as usize;
        let data = surface.data().expect("plane pixels");
        for (x, y) in [(10.0, 10.0), (110.0, 10.0), (60.0, 80.0), (30.0, 45.0)] {
            let (saturation, value) = plane_sv_at(x, y, w, h);
            let expected = hsv_to_rgb(hue, saturation, value);
            // Cairo's Rgb24 is B, G, R, padding on the little-endian hosts we
            // ship, matching `bitmap_to_surface`.
            let at = y as usize * stride + x as usize * 4;
            let painted = (data[at + 2], data[at + 1], data[at]);
            for (channel, painted, expected) in [
                ("r", painted.0, expected.0),
                ("g", painted.1, expected.1),
                ("b", painted.2, expected.2),
            ] {
                assert!(
                    (painted as i32 - expected as i32).abs() <= 6,
                    "at ({x}, {y}) the plane paints {channel}={painted}, but a drag \
                     there picks {channel}={expected}"
                );
            }
        }
    }

    #[test]
    fn a_drag_to_black_keeps_its_hue_and_saturation() {
        // The second reported bug: dragging the handle to the plane's bottom
        // edge commits black, and black carries no hue or saturation. Deriving
        // the triple back from it collapsed the handle into the bottom-left
        // corner and reset the plane to red at speed. The picker's own triple
        // has to survive its own writes.
        let green = 1.0 / 3.0;
        let held = (green, 0.8, 0.0);
        let black = hsv_to_rgb(green, 0.8, 0.0);
        assert_eq!(black, (0, 0, 0), "the bottom edge is black");
        assert_eq!(
            reconciled_hsv(held, Some(black), black),
            held,
            "the triple must survive a commit that cannot carry it"
        );
        // White loses only the hue, and the saturation the plane was holding
        // still has to stay off the corner.
        let held = (green, 0.0, 1.0);
        let white = hsv_to_rgb(green, 0.0, 1.0);
        assert_eq!(reconciled_hsv(held, Some(white), white), held);
        // A color this picker did not write — a project file, the hex entry,
        // another stop — is adopted as itself.
        assert_eq!(
            reconciled_hsv(held, Some(black), (0x00, 0x90, 0xFF)),
            rgb_to_hsv(0x00, 0x90, 0xFF)
        );
        // Before the first write there is nothing of ours to keep.
        assert_eq!(
            reconciled_hsv((0.0, 0.0, 0.0), None, (0xFF, 0xFF, 0xFF)),
            (0.0, 0.0, 1.0)
        );
    }

    #[test]
    fn the_hue_handle_stays_on_the_bar() {
        // A handle centered at x=0 with radius r extends r pixels off the
        // left end, which rendered the red end of the bar as a clipped
        // crescent. It has to be fully on the bar at every hue.
        let w = 400.0;
        let r = 11.0;
        for hue in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let x = spectrum_handle_x(hue, w, r);
            assert!(
                x >= r && x <= w - r,
                "hue {hue} put the handle at {x}, outside [{r}, {}]",
                w - r
            );
        }
        // A mid-bar hue is untouched, so the clamp only moves the ends.
        assert_eq!(spectrum_handle_x(0.5, w, r), 200.0);
        // A bar narrower than the handle must not produce an inverted range.
        assert!(spectrum_handle_x(0.0, 10.0, 11.0).is_finite());
    }

    #[test]
    fn flipping_the_gradient_mirrors_the_bar_handles() {
        let forward = VideoGradient::default();
        let flipped = VideoGradient {
            reversed: true,
            ..VideoGradient::default()
        };
        let a = bar_handles(&forward);
        let b = bar_handles(&flipped);
        // Forward: blue at the left, white at the right.
        assert_eq!(a[0].0, 0.0);
        assert_eq!((a[0].2.r, a[0].2.g, a[0].2.b), (0x00, 0x90, 0xFF));
        assert_eq!(a[1].0, 1.0);
        // Flipped: the same model stops are drawn white-first.
        assert_eq!(b[0].0, 0.0);
        assert_eq!((b[0].2.r, b[0].2.g, b[0].2.b), (0xFF, 0xFF, 0xFF));
        assert_eq!((b[1].2.r, b[1].2.g, b[1].2.b), (0x00, 0x90, 0xFF));
        // A write aimed at the left pin of a flipped bar has to target the
        // white stop (model index 1), not the blue one at index 0.
        assert_eq!(b[0].1, 1);
        assert_eq!(b[1].1, 0);
    }

    #[test]
    fn a_stop_handle_stays_clear_of_the_bar_ends() {
        // The reported bug: an end stop sat flush against the widget edge, so
        // its selection ring was clipped and the handle read as boxed into the
        // ramp. The travel has to leave room for the whole ring.
        let w = 220.0;
        let reach = GRADIENT_PIN_SIZE / 2.0 + GRADIENT_PIN_RING + 1.0;
        for position in [0.0, 0.5, 1.0] {
            let cx = gradient_pin_x(position, w);
            assert!(
                cx - reach >= 0.0 && cx + reach <= w,
                "handle at {position} leaves the bar at {cx}"
            );
        }
    }

    #[test]
    fn the_end_stops_are_grab_wide() {
        // The other half of the bug: the far stop has no track to its right,
        // so the grab zone has to reach across the whole handle and the ramp
        // beside it, and the drag has to invert the drawn placement so an
        // off-centre grab does not fling the handle.
        let w = 220.0;
        assert_eq!(gradient_position_at(gradient_pin_x(0.0, w), w), 0.0);
        assert_eq!(gradient_position_at(gradient_pin_x(1.0, w), w), 1.0);
        assert!((gradient_position_at(gradient_pin_x(0.5, w), w) - 0.5).abs() < 1e-9);
        // The zones reach past the bar's ends, so a press on the very end of
        // the ramp still picks up the stop sitting there.
        assert!(gradient_pin_x(0.0, w) - GRADIENT_GRAB_RADIUS <= 0.0);
        assert!(gradient_pin_x(1.0, w) + GRADIENT_GRAB_RADIUS >= w);
    }

    #[test]
    fn drag_hit_tests_measure_the_drawn_box_not_the_margin_box() {
        // The bars carry 10-12px CSS margins. GTK's `allocated_width()` returns
        // the margin box (+20-24px), while the draw function and a gesture's
        // x/y use the drawn box, so the right pin was hit-tested 24px right of
        // where it was painted and only its right edge could be grabbed. Every
        // drag must go through `drawn_size`.
        let source = include_str!("custom_wallpaper_popover.rs");
        let production = &source[..source.find("\n#[cfg(test)]").expect("tests module")];
        let drags = &production[production.find("fn apply_hue").expect("hue drag")..];
        assert!(
            !drags.contains("allocated_width") && !drags.contains("allocated_height"),
            "allocated_* is the margin box; measure the drawn box"
        );
        assert_eq!(
            drags.matches("drawn_size(&widget)").count(),
            4,
            "hue, plane, and both stop-drag sites must measure through drawn_size"
        );
    }

    #[test]
    fn every_kind_names_itself() {
        // The picker label and the model must agree; a mismatch would show one
        // type's name while rendering another.
        assert_eq!(kind_label(GradientKind::Linear), t("Linear"));
        assert_eq!(kind_label(GradientKind::Radial), t("Radial"));
        assert_eq!(kind_label(GradientKind::Angular), t("Angular"));
        assert_eq!(kind_label(GradientKind::Diamond), t("Diamond"));
    }

    #[test]
    fn light_stops_get_a_dark_handle_ring() {
        let ring = handle_ring_color((0x00, 0x90, 0xFF));
        assert!(
            ring.0 > 0.9 && ring.1 > 0.9 && ring.2 > 0.9,
            "blue keeps the light ring"
        );
        let dark = handle_ring_color((0xFF, 0xFF, 0xFF));
        assert!(
            dark.0 < 0.2 && dark.1 < 0.2 && dark.2 < 0.2,
            "a white stop with a white ring would vanish into the ramp's end"
        );
    }

    #[test]
    fn the_stop_picker_is_a_mini_card_not_a_second_column() {
        // The Gradient tab has to stay the Color tab's width. It used to grow
        // a second column — the picker beside the ramp — which widened the
        // whole popover the moment the tab was opened. The picker is its own
        // mini card now, opened left of the tile that was clicked; this test
        // builds the real tree, because the card has to parent cleanly beside
        // the page rather than reusing a widget that already has a parent.
        let Some(()) = crate::test_support::with_gtk(|| {
            use crate::recording::editor::model::VideoMetadata;

            fn find(widget: &Widget, class: &str) -> Option<Widget> {
                if widget.has_css_class(class) {
                    return Some(widget.clone());
                }
                let mut child = widget.first_child();
                while let Some(current) = child {
                    if let Some(found) = find(&current, class) {
                        return Some(found);
                    }
                    child = current.next_sibling();
                }
                None
            }

            let state = Arc::new(Mutex::new(VideoEditState::new(VideoMetadata {
                path: std::path::PathBuf::from("/tmp/input.mp4"),
                duration_seconds: 10.0,
                width: 1920,
                height: 1080,
                file_size_bytes: 1024,
                has_audio: false,
                frame_rate: 30.0,
            })));
            let sidebar = GtkBox::new(Orientation::Vertical, 0);
            let edit = Button::new();
            let popover = build_custom_wallpaper_popover(&sidebar, &edit, state, Rc::new(|| {}));
            let root = popover.child().expect("the popover has a body");

            assert!(
                find(&root, "recording-editor-gradient-columns").is_none(),
                "the gradient tab must not lay out as columns, or it widens the popover"
            );
            let editor = find(&root, "recording-editor-gradient-editor")
                .expect("the gradient page is the stop editor");
            assert!(
                find(&editor, "recording-editor-gradient-bar").is_some(),
                "the stop editor must carry the ramp"
            );
            // The card hangs off the popover's body, not off the page, so it is
            // found from the body rather than from the stop editor.
            let picker = find(&root, "recording-editor-gradient-picker")
                .expect("the picker is the stop editor's mini card");
            assert!(
                find(&picker, "recording-editor-custom-field").is_some(),
                "the mini card must carry the reused saturation/value plane"
            );
            // GTK wraps a popover's child in an internal `contents` widget, so
            // the card is the nearest ancestor carrying the card's class rather
            // than the picker's direct parent.
            let mut ancestor = picker.parent();
            let card = loop {
                let Some(widget) = ancestor else {
                    panic!("the picker is not inside a card popover");
                };
                if widget.has_css_class("recording-editor-gradient-picker-popover") {
                    break widget
                        .downcast::<Popover>()
                        .expect("the card class belongs to a popover");
                }
                ancestor = widget.parent();
            };
            assert_eq!(
                card.position(),
                gtk4::PositionType::Left,
                "the card opens left of the stop editor, not over it"
            );
            assert!(
                card.parent().is_some_and(|parent| parent == root),
                "the card hangs off the popover's own body, whose box GTK measures for it"
            );
            // The seat is anchored by the card's corner, which is what GTK
            // actually uses for a left-positioned popover: with the default
            // centring the card's corner landed where its centre belonged, i.e.
            // ~150px up, over the video.
            assert_eq!(
                card.valign(),
                gtk4::Align::Start,
                "a centred card is what opened up over the video"
            );
            assert!(
                find(
                    card.upcast_ref::<Widget>(),
                    "recording-editor-gradient-picker-close"
                )
                .is_some(),
                "the card needs its own close, or the tile toggle is the only way out"
            );
            popover.unparent();
        }) else {
            return;
        };
    }

    #[test]
    fn the_plane_covers_saturation_and_value() {
        // The plane is what makes the big field a picker rather than a flat
        // swatch: saturation runs right to left and value top to bottom, both
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
                within(color.0, back.0) && within(color.1, back.1) && within(color.2, back.2),
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

    #[test]
    fn every_gradient_icon_stroke_stays_inside_the_lucide_box() {
        // Both glyphs are transcribed from Lucide's 24x24 artwork and drawn
        // into a square the button would clip. A coordinate outside the box
        // silently loses an arrow head, and a corner that bulges past it
        // loses a rounded corner, so the whole path is kept inside.
        for icon in [GradientIcon::ArrowLeftRight, GradientIcon::RotateCwSquare] {
            for segment in gradient_icon_path(icon) {
                match *segment {
                    IconSegment::Move(x, y) | IconSegment::Line(x, y) => assert!(
                        (0.0..=24.0).contains(&x) && (0.0..=24.0).contains(&y),
                        "{icon:?} leaves Lucide's box at ({x}, {y})"
                    ),
                    IconSegment::Corner { cx, cy, r, .. } => assert!(
                        cx - r >= 0.0 && cx + r <= 24.0 && cy - r >= 0.0 && cy + r <= 24.0,
                        "{icon:?}'s corner at ({cx}, {cy}) sweeps outside the box"
                    ),
                }
            }
        }
    }

    #[test]
    fn every_rounded_corner_meets_the_stroke_before_it() {
        // An SVG `a` corner starts wherever the previous command ended, while
        // Cairo joins the current point straight to the arc's start. A
        // mistyped angle would show up as a chamfered corner, so each arc has
        // to begin exactly on the stroke that hands over to it. The angles
        // also have to stay a quarter turn: swapping them keeps the endpoints
        // but sweeps three quarters the wrong way, which would paint over the
        // square's inside.
        for icon in [GradientIcon::ArrowLeftRight, GradientIcon::RotateCwSquare] {
            let mut current: Option<(f64, f64)> = None;
            for segment in gradient_icon_path(icon) {
                match *segment {
                    IconSegment::Move(x, y) | IconSegment::Line(x, y) => current = Some((x, y)),
                    IconSegment::Corner {
                        cx,
                        cy,
                        r,
                        from,
                        to,
                    } => {
                        let (px, py) = current.expect("a corner follows a stroke");
                        let start = (cx + r * from.cos(), cy + r * from.sin());
                        assert!(
                            (start.0 - px).abs() < 1e-9 && (start.1 - py).abs() < 1e-9,
                            "{icon:?}: corner starts at {start:?}, stroke ends at ({px:?}, {py:?})"
                        );
                        let sweep = (from - to).rem_euclid(2.0 * PI);
                        assert!(
                            sweep <= PI + 1e-9,
                            "{icon:?}: corner at ({cx}, {cy}) sweeps {sweep} rad the long way"
                        );
                        current = Some((cx + r * to.cos(), cy + r * to.sin()));
                    }
                }
            }
        }
    }

    /// The seat, measured rather than derived.
    ///
    /// This is the test that would have caught every wrong seat: the card is
    /// built exactly as the real one is (hung off the popover's body, pointing
    /// at a rect in that body's own space) and its surface position is read back
    /// from GDK, relative to the popover surface it is a child of. Getting the
    /// space wrong moved the card by ~150px — up over the video — so the bounds
    /// below are wide enough for the host theme's popover margins and shadow and
    /// nowhere near wide enough to pass a wrong seat.
    #[test]
    fn the_card_hangs_clear_of_the_popover_beside_it() {
        let Some(()) = crate::test_support::with_gtk(|| {
            use gtk4::gdk::prelude::PopupExt;

            let window = gtk4::Window::new();
            window.set_default_size(1200, 800);
            let root = GtkBox::new(Orientation::Vertical, 0);
            window.set_child(Some(&root));

            let host = Popover::new();
            host.set_has_arrow(false);
            host.set_autohide(true);
            host.set_position(gtk4::PositionType::Left);
            host.set_parent(&root);
            let body = GtkBox::new(Orientation::Vertical, 0);
            body.set_size_request(236, 300);
            host.set_child(Some(&body));

            let card = Popover::new();
            card.set_has_arrow(false);
            card.set_position(gtk4::PositionType::Left);
            card.set_autohide(false);
            card.set_valign(Align::Start);
            card.set_parent(&body);
            let card_body = GtkBox::new(Orientation::Vertical, 0);
            card_body.set_size_request(240, 290);
            card.set_child(Some(&card_body));

            window.present();
            // Surfaces are positioned by the compositor, so the loop has to run
            // in real time: spinning the context without waiting reads the
            // popup's position before Wayland has answered.
            let pump = || {
                let ctx = gtk4::glib::MainContext::default();
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
                while std::time::Instant::now() < deadline {
                    while ctx.iteration(false) {}
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            };
            pump();
            host.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(300, 100, 1, 200)));
            host.popup();
            pump();

            card.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
                -PICKER_CARD_GAP,
                0,
                1,
                card_body.height().max(1),
            )));
            card.popup();
            pump();

            let surface = card.surface().expect("the card has a surface");
            let popup = surface
                .downcast::<gtk4::gdk::Popup>()
                .expect("a popover's surface is a popup");
            let (card_x, card_y) = (popup.position_x(), popup.position_y());
            let (card_w, card_h) = (popup.width(), popup.height());
            let body_rect = body.compute_bounds(&host);
            card.popdown();
            pump();
            card.unparent();
            host.popdown();
            host.unparent();
            window.destroy();

            if card_w < 100 || card_h < 100 {
                eprintln!("skipping: the compositor did not position the surfaces");
                return;
            }
            let Some(body_rect) = body_rect else {
                eprintln!("skipping: the body has no bounds");
                return;
            };

            // `position_x/y` is relative to the popover surface this card is a
            // child of, which is the space the body's bounds are read in. The
            // gap is asserted as a range, not a number: the host theme's own
            // popover margins are in play here, while the app's CSS strips them
            // and leaves the exact PICKER_CARD_GAP. What matters is that the
            // card is clear of the popover's edge and not flung off somewhere
            // else in the window.
            let gap = body_rect.x() as i32 - (card_x + card_w);
            assert!(
                (0..=24).contains(&gap),
                "the card must sit just clear of the popover's edge, not {gap}px away"
            );
            // The failure this guards is vertical: a centred rect put the
            // card's corner where its centre belonged, ~150px up, which is the
            // "card over the video" the seat kept producing.
            let body_top = body_rect.y() as i32;
            assert!(
                (card_y - body_top).abs() <= 10,
                "the card must be level with the popover: card top {card_y} vs body top {body_top}"
            );
        }) else {
            eprintln!("skipping: no display available");
            return;
        };
    }
}
