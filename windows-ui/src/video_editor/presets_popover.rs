//! The presets button beside Export, and the popover it opens.
//!
//! A preset is a saved look. The popover is three states in one card: an empty
//! invitation, a create form that a rename reuses, and the list of saved looks.
//! A row applies its preset to the open project as one undo step; its `⋯` menu
//! keeps the look as the default for new recordings, updates it from the
//! current look, renames it, shows its file or deletes it.

use super::background;
use super::presets::{self, Preset};
use super::side_panel::{draw_wallpaper_fallback, draw_wallpaper_thumbnail};
use super::timeline::Timeline;
use crate::capture::editor::window::icon_names;
use crate::config::{load_config, save_config, AppConfig};
use crate::i18n::{t, tfmt};
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use gtk4::{
    gdk, glib, pango, Align, Box as GtkBox, Button, CssProvider, DrawingArea, Entry,
    EventControllerKey, EventControllerLegacy, Image, Label, Orientation, PickFlags, PolicyType,
    Popover, PositionType, ScrolledWindow, Stack, StackTransitionType, Switch, Widget,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// The editor's accent, the orange the Export button is filled with.
const ACCENT: (f64, f64, f64) = (1.0, 0.404, 0.051);

/// What the form and the delete page are acting on. The list itself never
/// reads this: each state is a page of the card's stack, so the form and the
/// delete question replace the list instead of sitting beside it.
#[derive(Clone)]
enum Pending {
    Idle,
    Rename(String),
    Delete(String, String),
}

/// The popover and everything its handlers reach. Each handler holds a clone,
/// so they all see the same stack, form and status lines.
#[derive(Clone)]
struct Panel {
    popover: Popover,
    stack: Stack,
    create_title: Label,
    delete_title: Label,
    name: Entry,
    default_card: GtkBox,
    default_switch: Switch,
    form_status: Label,
    form_actions: GtkBox,
    create_button: Button,
    list: GtkBox,
    list_status: Label,
    pending: Rc<RefCell<Pending>>,
    timeline: Timeline,
    applied: Rc<RefCell<Option<String>>>,
    scale: f64,
}

/// The widgets a card is built from, before its handlers are attached. Kept
/// apart from [`attach`] so the state layout can be built and inspected
/// without a timeline.
struct States {
    stack: Stack,
    create_title: Label,
    delete_title: Label,
    name: Entry,
    default_card: GtkBox,
    default_switch: Switch,
    form_status: Label,
    form_actions: GtkBox,
    create_button: Button,
    cancel_create: Button,
    list: GtkBox,
    list_status: Label,
    empty_save: Button,
    add_preset: Button,
    cancel_delete: Button,
    confirm_delete: Button,
}

/// The bookmark button before Export in the title bar. It stays insensitive
/// until a recording is open, as Export does, and fills with the accent while
/// its popover is open.
pub(super) fn title_button(scale: f64) -> Button {
    install_style(scale);
    let button = Button::new();
    button.add_css_class("video-editor-title-presets");
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_valign(Align::Center);
    button.set_margin_end((6.0 * scale).round() as i32);
    button.set_size_request((34.0 * scale).round() as i32, (30.0 * scale).round() as i32);
    button.set_tooltip_text(Some(&t("Presets")));
    button.set_sensitive(false);

    let glyph_size = (18.0 * scale).round() as i32;
    let glyph = DrawingArea::new();
    glyph.set_content_width(glyph_size);
    glyph.set_content_height(glyph_size);
    glyph.set_halign(Align::Center);
    glyph.set_valign(Align::Center);
    glyph.set_can_target(false);
    glyph.set_draw_func(|glyph, context, width, height| {
        let open = glyph
            .parent()
            .is_some_and(|button| button.has_css_class("vep-open"));
        let color = glyph.style_context().color();
        let rgba = if open {
            (ACCENT.0, ACCENT.1, ACCENT.2, 1.0)
        } else {
            (
                f64::from(color.red()),
                f64::from(color.green()),
                f64::from(color.blue()),
                f64::from(color.alpha()),
            )
        };
        draw_bookmark(context, f64::from(width), f64::from(height), rgba, open);
    });
    button.set_child(Some(&glyph));
    button
}

thread_local! {
    static STYLE_INSTALLED: Cell<bool> = const { Cell::new(false) };
}

fn install_style(scale: f64) {
    if STYLE_INSTALLED.with(|installed| installed.replace(true)) {
        return;
    }
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(&style_css(scale));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 7,
    );
}

/// The popover's sheet: the title button, the stripped popover frame, the card
/// and one block per state. Authored at 100% and scaled here, including the
/// hairlines, so the card keeps its proportions at every Windows UI scale.
fn style_css(scale: f64) -> String {
    let px = |value: f64| (value * scale).round().max(1.0) as i32;
    let font = 14.0 * scale;
    let muted = 13.0 * scale;
    let note = 12.0 * scale;
    let eyebrow = 11.0 * scale;
    let pill_font = 11.0 * scale;
    let title = 15.0 * scale;
    let eyebrow_spacing = 0.8 * scale;
    let menu_glyph = 16.0 * scale;
    format!(
        r#"
        button.video-editor-title-presets {{
            min-height: 0; min-width: 0; padding: 0; border: none; box-shadow: none;
            border-radius: {r8}px; background: none; color: #d4d4d4;
        }}
        button.video-editor-title-presets:hover {{ background: alpha(white, 0.08); color: #ffffff; }}
        button.video-editor-title-presets.vep-open {{ background: alpha(#ff670d, 0.14); }}
        button.video-editor-title-presets:disabled {{ color: alpha(#d4d4d4, 0.35); }}
        .editor-theme-light button.video-editor-title-presets {{ color: #3a3f45; }}
        .editor-theme-light button.video-editor-title-presets:hover {{ background: alpha(#111827, 0.08); color: #111827; }}

        popover.video-editor-presets-popover,
        popover.video-editor-presets-popover > contents {{
            background: transparent; border: none; outline: none; box-shadow: none;
            padding: 0; border-radius: 0;
        }}
        .vep-card {{
            background: #171717; color: #e8e8e8;
            border: {rim}px solid rgba(255, 255, 255, 0.06);
            border-radius: {card_radius}px;
            padding: {pad}px;
            font-family: 'Inter', 'Noto Sans', system-ui, sans-serif;
            font-size: {font}px;
            box-shadow: none;
        }}
        .vep-header {{ margin-bottom: {header_gap}px; }}
        .vep-title {{ font-size: {title}px; font-weight: 600; color: #e8e8e8; }}
        button.vep-close {{
            min-width: {close}px; min-height: {close}px; padding: 0;
            border: none; box-shadow: none; border-radius: {close_radius}px;
            background: transparent; color: #999999;
        }}
        button.vep-close:hover {{ background: rgba(255, 255, 255, 0.08); color: #e8e8e8; }}
        button.vep-close:focus-visible {{ outline: 2px solid #ff7c2f; outline-offset: 2px; }}

        button.vep-btn {{
            min-height: {btn_height}px; padding: 0 {btn_pad}px;
            border: none; border-radius: {btn_radius}px; box-shadow: none;
            background-image: none; text-shadow: none;
            font-size: {font}px; font-weight: 500;
        }}
        button.vep-btn:focus {{ outline: none; box-shadow: none; }}
        button.vep-btn:focus-visible {{ outline: 2px solid #ff7c2f; outline-offset: 2px; }}
        button.vep-primary {{ background: #ff670d; color: #ffffff; }}
        button.vep-primary:hover {{ background: #ff7c2f; }}
        button.vep-primary:disabled {{ background: rgba(255, 255, 255, 0.08); color: rgba(255, 255, 255, 0.35); }}
        button.vep-ghost {{ background: transparent; color: #c6c6c6; }}
        button.vep-ghost:hover {{ background: #202020; color: #e8e8e8; }}
        button.vep-danger {{ background: #e5484d; color: #ffffff; }}
        button.vep-danger:hover {{ background: #f2555a; }}

        .vep-muted {{ color: #858585; font-size: {muted}px; }}
        .vep-eyebrow {{
            color: #8a8a8a; font-size: {eyebrow}px; font-weight: 600;
            letter-spacing: {eyebrow_spacing}px;
        }}

        entry.vep-entry {{
            min-height: {entry_height}px; padding: 0 {entry_pad}px;
            border: none; border-radius: {entry_radius}px;
            background: rgba(255, 255, 255, 0.05); color: #e8e8e8;
            box-shadow: none; outline: none; font-size: {font}px;
        }}
        entry.vep-entry:focus, entry.vep-entry:focus-within {{
            box-shadow: inset 0 0 0 1px rgba(255, 122, 47, 0.6);
            outline: none;
        }}

        .vep-toggle-card {{
            background: rgba(255, 255, 255, 0.03);
            border: {rim}px solid rgba(255, 255, 255, 0.06);
            border-radius: {panel_radius}px;
            padding: {panel_pad_y}px {panel_pad_x}px;
        }}
        .vep-toggle-title {{ font-size: {font}px; font-weight: 500; color: #e8e8e8; }}
        .vep-toggle-note {{ font-size: {note}px; }}
        switch.vep-switch {{
            min-width: {switch_w}px; min-height: {switch_h}px; padding: 0;
            background: rgba(255, 255, 255, 0.14); background-image: none;
            border: none; border-radius: 999px; box-shadow: none;
        }}
        switch.vep-switch slider {{
            min-width: {knob}px; min-height: {knob}px; margin: {knob_margin}px;
            background: #ffffff; background-image: none; border: none;
            border-radius: 999px; box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
        }}
        switch.vep-switch:checked {{ background: #ff670d; }}
        switch.vep-switch:focus-visible {{ outline: 2px solid #ff7c2f; outline-offset: 2px; }}

        .vep-empty-tile {{ background: rgba(255, 255, 255, 0.05); border-radius: {tile_radius}px; }}
        .vep-empty-glyph {{ color: #bbbbbb; }}
        .vep-empty-title {{ font-size: {title}px; font-weight: 600; color: #e8e8e8; }}

        .vep-row {{ border-radius: {row_radius}px; }}
        .vep-row:hover {{ background: rgba(255, 255, 255, 0.05); }}
        .vep-row.vep-applied {{ background: rgba(255, 103, 13, 0.10); }}
        .vep-row.vep-applied:hover {{ background: rgba(255, 103, 13, 0.14); }}
        button.vep-pick {{
            padding: {row_pad}px; border: none; box-shadow: none;
            background: transparent; border-radius: {row_radius}px;
            color: #e8e8e8; text-shadow: none;
        }}
        button.vep-pick:hover, button.vep-pick:active, button.vep-pick:focus {{
            background: transparent; box-shadow: none; outline: none;
        }}
        button.vep-pick:focus-visible {{ outline: 2px solid #ff7c2f; outline-offset: -2px; }}
        .vep-row-name {{ font-size: {font}px; font-weight: 500; color: #e8e8e8; }}
        .vep-pill {{
            font-size: {pill_font}px; font-weight: 600; color: #ff9a5c;
            background: rgba(255, 103, 13, 0.14); border-radius: {pill_radius}px;
            padding: {pill_y}px {pill_x}px;
        }}
        button.vep-menu {{
            min-width: {menu}px; min-height: {menu}px; padding: 0;
            border: none; box-shadow: none; border-radius: {close_radius}px;
            background: transparent; color: #999999;
        }}
        button.vep-menu:hover {{ background: #2a2a2a; color: #e8e8e8; }}
        button.vep-menu:focus {{ outline: none; box-shadow: none; }}
        button.vep-menu:focus-visible {{ outline: 2px solid #ff7c2f; outline-offset: 2px; }}
        .vep-menu-glyph {{ font-size: {menu_glyph}px; font-weight: 700; }}

        .vep-divider {{ background: rgba(255, 255, 255, 0.06); min-height: {rim}px; }}
        button.vep-add {{
            padding: {row_pad}px; border: none; box-shadow: none;
            background: transparent; border-radius: {row_radius}px; text-shadow: none;
        }}
        button.vep-add:hover {{ background: rgba(255, 255, 255, 0.05); }}
        button.vep-add:focus {{ outline: none; box-shadow: none; }}
        button.vep-add:focus-visible {{ outline: 2px solid #ff7c2f; outline-offset: 2px; }}
        .vep-add-label {{ color: #c6c6c6; font-size: {font}px; font-weight: 500; }}
        .vep-placeholder {{ color: rgba(255, 255, 255, 0.18); }}

        .vep-status {{ color: #858585; font-size: {muted}px; }}
        .vep-status.vep-error {{ color: #f2555a; }}

        .editor-theme-light .vep-card {{
            background: #ffffff; color: #202020;
            border-color: rgba(17, 24, 39, 0.08);
        }}
        .editor-theme-light .vep-title,
        .editor-theme-light .vep-empty-title,
        .editor-theme-light .vep-toggle-title,
        .editor-theme-light .vep-row-name {{ color: #202020; }}
        .editor-theme-light .vep-eyebrow {{ color: #737373; }}
        .editor-theme-light button.vep-close {{ color: #737373; }}
        .editor-theme-light button.vep-close:hover {{ background: rgba(17, 24, 39, 0.06); color: #202020; }}
        .editor-theme-light .vep-muted,
        .editor-theme-light .vep-status {{ color: #737373; }}
        .editor-theme-light .vep-status.vep-error {{ color: #d9383d; }}
        .editor-theme-light button.vep-ghost {{ color: #454545; }}
        .editor-theme-light button.vep-ghost:hover {{ background: rgba(17, 24, 39, 0.06); color: #202020; }}
        .editor-theme-light button.vep-primary:disabled {{ background: rgba(17, 24, 39, 0.08); color: rgba(17, 24, 39, 0.35); }}
        .editor-theme-light entry.vep-entry {{ background: rgba(17, 24, 39, 0.05); color: #202020; }}
        .editor-theme-light .vep-toggle-card {{
            background: rgba(17, 24, 39, 0.03);
            border-color: rgba(17, 24, 39, 0.08);
        }}
        .editor-theme-light switch.vep-switch {{ background: rgba(17, 24, 39, 0.16); }}
        .editor-theme-light switch.vep-switch:checked {{ background: #ff670d; }}
        .editor-theme-light .vep-empty-tile {{ background: rgba(17, 24, 39, 0.04); }}
        .editor-theme-light .vep-empty-glyph {{ color: #737373; }}
        .editor-theme-light .vep-row:hover {{ background: rgba(17, 24, 39, 0.05); }}
        .editor-theme-light .vep-row.vep-applied {{ background: rgba(255, 103, 13, 0.10); }}
        .editor-theme-light .vep-row.vep-applied:hover {{ background: rgba(255, 103, 13, 0.14); }}
        .editor-theme-light button.vep-menu {{ color: #737373; }}
        .editor-theme-light button.vep-menu:hover {{ background: rgba(17, 24, 39, 0.06); color: #202020; }}
        .editor-theme-light .vep-divider {{ background: rgba(17, 24, 39, 0.08); }}
        .editor-theme-light button.vep-add {{ color: #454545; }}
        .editor-theme-light button.vep-add:hover {{ background: rgba(17, 24, 39, 0.05); }}
        .editor-theme-light .vep-add-label {{ color: #454545; }}
        .editor-theme-light .vep-placeholder {{ color: rgba(17, 24, 39, 0.18); }}
        .editor-theme-light .vep-pill {{ color: #b74809; background: rgba(255, 103, 13, 0.14); }}
        "#,
        font = font,
        muted = muted,
        note = note,
        eyebrow = eyebrow,
        eyebrow_spacing = eyebrow_spacing,
        pill_font = pill_font,
        title = title,
        menu_glyph = menu_glyph,
        rim = px(1.0),
        pad = px(18.0),
        card_radius = px(14.0),
        header_gap = px(14.0),
        close = px(26.0),
        close_radius = px(6.0),
        btn_height = px(36.0),
        btn_radius = px(5.0),
        btn_pad = px(16.0),
        entry_height = px(38.0),
        entry_radius = px(8.0),
        entry_pad = px(12.0),
        panel_radius = px(10.0),
        panel_pad_y = px(10.0),
        panel_pad_x = px(12.0),
        switch_w = px(36.0),
        switch_h = px(20.0),
        knob = px(16.0),
        knob_margin = px(2.0),
        tile_radius = px(10.0),
        row_radius = px(8.0),
        row_pad = px(8.0),
        pill_radius = px(4.0),
        pill_y = px(2.0),
        pill_x = px(6.0),
        menu = px(26.0),
        r8 = px(8.0),
    )
}

/// A bookmark in a twenty-unit box: a rounded tab with a V notch at the
/// bottom, outlined at rest and filled while its popover is open.
fn draw_bookmark(
    context: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    (red, green, blue, alpha): (f64, f64, f64, f64),
    filled: bool,
) {
    let size = width.min(height).max(1.0);
    let _ = context.save();
    context.translate((width - size) * 0.5, (height - size) * 0.5);
    context.scale(size / 20.0, size / 20.0);
    context.set_source_rgba(red, green, blue, alpha);
    context.set_line_width(1.7);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.new_sub_path();
    context.arc(
        6.8,
        4.2,
        1.8,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    context.arc(13.2, 4.2, 1.8, std::f64::consts::PI * 1.5, 0.0);
    context.line_to(15.0, 17.2);
    context.line_to(10.0, 13.4);
    context.line_to(5.0, 17.2);
    context.close_path();
    if filled {
        let _ = context.fill_preserve();
    }
    let _ = context.stroke();
    let _ = context.restore();
}

fn redraw_child(widget: &impl IsA<Widget>) {
    if let Some(child) = widget.as_ref().first_child() {
        child.queue_draw();
    }
}

/// A rounded rectangle as a path, used for the thumbnail mask and the dashed
/// placeholder tile.
fn rounded_rect(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    use std::f64::consts::{FRAC_PI_2, PI};
    let right = x + width;
    let bottom = y + height;
    let radius = radius.max(0.0).min(width.min(height) * 0.5);
    context.new_sub_path();
    context.arc(right - radius, y + radius, radius, -FRAC_PI_2, 0.0);
    context.arc(right - radius, bottom - radius, radius, 0.0, FRAC_PI_2);
    context.arc(x + radius, bottom - radius, radius, FRAC_PI_2, PI);
    context.arc(x + radius, y + radius, radius, PI, PI * 1.5);
    context.close_path();
}

fn style_color(widget: &impl IsA<Widget>) -> (f64, f64, f64, f64) {
    let color = widget.as_ref().style_context().color();
    (
        f64::from(color.red()),
        f64::from(color.green()),
        f64::from(color.blue()),
        f64::from(color.alpha()),
    )
}

/// Close `popover` when a press lands on its window outside it. Autohide
/// alone loses its grab once a nested menu has opened and closed, so the
/// window watches presses itself. Presses inside the popover, inside a
/// popover it owns (the row menu, an entry's context menu), or on `button`
/// (which toggles it) are left alone.
fn close_on_outside_press(popover: &Popover, button: &Button) {
    let Some(root) = button.root() else {
        return;
    };
    let window = root.upcast::<Widget>();
    let watcher = EventControllerLegacy::new();
    watcher.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let popover = popover.clone();
    let button = button.clone();
    let window_for_pick = window.clone();
    watcher.connect_event(move |_, event| {
        if event.event_type() != gdk::EventType::ButtonPress || !popover.is_visible() {
            return glib::Propagation::Proceed;
        }
        let Some(surface) = event.surface() else {
            return glib::Propagation::Proceed;
        };
        if let Some(native) = gtk4::Native::for_surface(&surface) {
            let native = native.upcast::<Widget>();
            if native == *popover.upcast_ref::<Widget>() || native.is_ancestor(&popover) {
                return glib::Propagation::Proceed;
            }
            if native != window_for_pick {
                return glib::Propagation::Proceed;
            }
        }
        if let Some((x, y)) = event.position() {
            let (dx, dy) = window_for_pick
                .downcast_ref::<gtk4::Native>()
                .map(|native| native.surface_transform())
                .unwrap_or((0.0, 0.0));
            if let Some(picked) = window_for_pick.pick(x - dx, y - dy, PickFlags::DEFAULT) {
                if picked == *button.upcast_ref::<Widget>() || picked.is_ancestor(&button) {
                    return glib::Propagation::Proceed;
                }
            }
        }
        popover.popdown();
        glib::Propagation::Proceed
    });
    window.add_controller(watcher);
}

/// The header every page shares: a title on the left, the close button on the
/// right. The title is returned too, because the form and the delete question
/// change it in place.
fn header(title: &str, popover: &Popover, scale: f64) -> (GtkBox, Label) {
    let px = |value: f64| (value * scale).round() as i32;
    let row = GtkBox::new(Orientation::Horizontal, px(8.0));
    row.add_css_class("vep-header");
    let label = Label::new(Some(title));
    label.add_css_class("vep-title");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_valign(Align::Center);
    label.set_ellipsize(pango::EllipsizeMode::End);

    let close = Button::new();
    close.add_css_class("vep-close");
    close.set_has_frame(false);
    close.set_focus_on_click(false);
    close.set_valign(Align::Center);
    close.set_tooltip_text(Some(&t("Close")));
    let icon = Image::from_icon_name("window-close-symbolic");
    icon.set_pixel_size(px(12.0));
    close.set_child(Some(&icon));
    close.connect_clicked({
        let popover = popover.clone();
        move |_| popover.popdown()
    });

    row.append(&label);
    row.append(&close);
    (row, label)
}

/// The 52×34 dashed tile that stands in for a thumbnail on the add row. The
/// dash and the plus both take the widget's colour, so the light theme only
/// has to restate one colour.
fn dashed_placeholder(scale: f64) -> DrawingArea {
    let width = (52.0 * scale).round() as i32;
    let height = (34.0 * scale).round() as i32;
    let area = DrawingArea::new();
    area.add_css_class("vep-placeholder");
    area.set_content_width(width);
    area.set_content_height(height);
    area.set_valign(Align::Center);
    area.set_can_target(false);
    area.set_draw_func({
        let scale = scale;
        move |area, context, width, height| {
            let (red, green, blue, alpha) = style_color(area);
            let (width, height) = (f64::from(width), f64::from(height));
            context.set_source_rgba(red, green, blue, alpha);
            context.set_line_width((1.0 * scale).max(1.0));
            rounded_rect(context, 0.5, 0.5, width - 1.0, height - 1.0, 6.0 * scale);
            context.set_dash(&[3.0 * scale, 3.0 * scale], 0.0);
            let _ = context.stroke();
            context.set_dash(&[], 0.0);
            context.set_line_width((1.6 * scale).max(1.0));
            context.set_line_cap(gtk4::cairo::LineCap::Round);
            let (cx, cy, arm) = (width * 0.5, height * 0.5, 6.0 * scale);
            context.move_to(cx - arm, cy);
            context.line_to(cx + arm, cy);
            context.move_to(cx, cy - arm);
            context.line_to(cx, cy + arm);
            let _ = context.stroke();
        }
    });
    area
}

/// Build the card's four pages and the widgets their handlers need. Nothing
/// is wired here: [`attach`] owns the panel and connects the buttons once the
/// panel exists.
fn build_states(popover: &Popover, scale: f64) -> States {
    let px = |value: f64| (value * scale).round() as i32;
    let stack = Stack::new();
    stack.add_css_class("vep-card");
    stack.set_size_request(px(320.0), -1);
    stack.set_transition_type(StackTransitionType::Crossfade);
    stack.set_transition_duration(150);
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);

    // State 1: nothing saved yet.
    let empty = GtkBox::new(Orientation::Vertical, 0);
    let (empty_header, _) = header(&t("Presets"), popover, scale);
    empty.append(&empty_header);
    let empty_body = GtkBox::new(Orientation::Vertical, 0);
    empty_body.set_margin_top(px(6.0));
    let tile = GtkBox::new(Orientation::Vertical, 0);
    tile.add_css_class("vep-empty-tile");
    tile.set_size_request(px(40.0), px(40.0));
    tile.set_halign(Align::Center);
    tile.set_valign(Align::Center);
    let glyph_size = px(18.0);
    let glyph = DrawingArea::new();
    glyph.add_css_class("vep-empty-glyph");
    glyph.set_content_width(glyph_size);
    glyph.set_content_height(glyph_size);
    glyph.set_halign(Align::Center);
    // A vertical box only centres a child that may take the spare height, so
    // the glyph expands into the tile and centres itself inside it.
    glyph.set_vexpand(true);
    glyph.set_valign(Align::Center);
    glyph.set_can_target(false);
    glyph.set_draw_func(|glyph, context, width, height| {
        draw_bookmark(
            context,
            f64::from(width),
            f64::from(height),
            style_color(glyph),
            false,
        );
    });
    tile.append(&glyph);
    let empty_title = Label::new(Some(&t("No presets yet")));
    empty_title.add_css_class("vep-empty-title");
    empty_title.set_halign(Align::Center);
    empty_title.set_margin_top(px(12.0));
    let empty_hint = Label::new(Some(&t(
        "Save this project's background, cursor, camera and captions, then put them on any recording.",
    )));
    empty_hint.add_css_class("vep-muted");
    empty_hint.set_wrap(true);
    empty_hint.set_justify(gtk4::Justification::Center);
    empty_hint.set_max_width_chars(36);
    empty_hint.set_halign(Align::Center);
    empty_hint.set_margin_top(px(6.0));
    let empty_save = Button::with_label(&t("Save current look"));
    empty_save.add_css_class("vep-btn");
    empty_save.add_css_class("vep-primary");
    empty_save.set_has_frame(false);
    empty_save.set_margin_top(px(18.0));
    empty_save.set_hexpand(true);
    empty_body.append(&tile);
    empty_body.append(&empty_title);
    empty_body.append(&empty_hint);
    empty_body.append(&empty_save);
    empty.append(&empty_body);

    // State 2: create, and the rename that reuses it.
    let form = GtkBox::new(Orientation::Vertical, 0);
    let (form_header, create_title) = header(&t("New preset"), popover, scale);
    form.append(&form_header);
    let eyebrow = Label::new(Some(&t("Name").to_uppercase()));
    eyebrow.add_css_class("vep-eyebrow");
    eyebrow.set_xalign(0.0);
    form.append(&eyebrow);
    let name = Entry::new();
    name.add_css_class("vep-entry");
    name.set_max_length(120);
    name.set_placeholder_text(Some(&t("Preset name")));
    name.set_margin_top(px(6.0));
    form.append(&name);
    let form_status = Label::new(None);
    form_status.add_css_class("vep-status");
    form_status.set_xalign(0.0);
    form_status.set_wrap(true);
    form_status.set_margin_top(px(6.0));
    form_status.set_visible(false);
    form.append(&form_status);

    let default_card = GtkBox::new(Orientation::Horizontal, px(12.0));
    default_card.add_css_class("vep-toggle-card");
    default_card.set_margin_top(px(12.0));
    default_card.set_margin_bottom(px(16.0));
    let default_text = GtkBox::new(Orientation::Vertical, px(2.0));
    default_text.set_hexpand(true);
    default_text.set_valign(Align::Center);
    let default_title = Label::new(Some(&t("Default for new recordings")));
    default_title.add_css_class("vep-toggle-title");
    default_title.set_xalign(0.0);
    let default_note = Label::new(Some(&t("New recordings open with this look")));
    default_note.add_css_class("vep-muted");
    default_note.add_css_class("vep-toggle-note");
    default_note.set_xalign(0.0);
    default_note.set_wrap(true);
    default_text.append(&default_title);
    default_text.append(&default_note);
    let default_switch = Switch::new();
    default_switch.add_css_class("vep-switch");
    default_switch.set_valign(Align::Center);
    default_switch.update_property(&[gtk4::accessible::Property::Label(&t(
        "Default for new recordings",
    ))]);
    default_card.append(&default_text);
    default_card.append(&default_switch);
    form.append(&default_card);

    let form_actions = GtkBox::new(Orientation::Horizontal, px(8.0));
    form_actions.set_halign(Align::End);
    let cancel_create = Button::with_label(&t("Cancel"));
    cancel_create.add_css_class("vep-btn");
    cancel_create.add_css_class("vep-ghost");
    cancel_create.set_has_frame(false);
    let create_button = Button::with_label(&t("Create"));
    create_button.add_css_class("vep-btn");
    create_button.add_css_class("vep-primary");
    create_button.set_has_frame(false);
    create_button.set_sensitive(false);
    form_actions.append(&cancel_create);
    form_actions.append(&create_button);
    form.append(&form_actions);

    // State 3: the list, with the add row fixed under its scrolling rows.
    let list_page = GtkBox::new(Orientation::Vertical, 0);
    let (list_header, _) = header(&t("Presets"), popover, scale);
    list_page.append(&list_header);
    let list = GtkBox::new(Orientation::Vertical, px(2.0));
    let scroll = ScrolledWindow::new();
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(px(320.0));
    scroll.set_child(Some(&list));
    list_page.append(&scroll);
    let divider = GtkBox::new(Orientation::Vertical, 0);
    divider.add_css_class("vep-divider");
    divider.set_size_request(-1, px(1.0));
    divider.set_margin_top(px(10.0));
    divider.set_margin_bottom(px(10.0));
    divider.set_margin_start(px(10.0));
    divider.set_margin_end(px(10.0));
    list_page.append(&divider);
    let add_preset = Button::new();
    add_preset.add_css_class("vep-add");
    add_preset.set_has_frame(false);
    add_preset.set_focus_on_click(false);
    let add_content = GtkBox::new(Orientation::Horizontal, px(12.0));
    add_content.append(&dashed_placeholder(scale));
    let add_label = Label::new(Some(&t("Save current look")));
    add_label.add_css_class("vep-add-label");
    add_label.set_xalign(0.0);
    add_label.set_hexpand(true);
    add_content.append(&add_label);
    add_preset.set_child(Some(&add_content));
    list_page.append(&add_preset);
    let list_status = Label::new(None);
    list_status.add_css_class("vep-status");
    list_status.set_xalign(0.0);
    list_status.set_wrap(true);
    list_status.set_margin_top(px(8.0));
    list_status.set_visible(false);
    list_page.append(&list_status);

    // The delete question: the shared header carries the question itself.
    let delete_page = GtkBox::new(Orientation::Vertical, 0);
    let (delete_header, delete_title) = header("", popover, scale);
    delete_page.append(&delete_header);
    let delete_actions = GtkBox::new(Orientation::Horizontal, px(8.0));
    delete_actions.set_halign(Align::End);
    let cancel_delete = Button::with_label(&t("Cancel"));
    cancel_delete.add_css_class("vep-btn");
    cancel_delete.add_css_class("vep-ghost");
    cancel_delete.set_has_frame(false);
    let confirm_delete = Button::with_label(&t("Delete"));
    confirm_delete.add_css_class("vep-btn");
    confirm_delete.add_css_class("vep-danger");
    confirm_delete.set_has_frame(false);
    delete_actions.append(&cancel_delete);
    delete_actions.append(&confirm_delete);
    delete_page.append(&delete_actions);

    stack.add_named(&empty, Some("empty"));
    stack.add_named(&form, Some("form"));
    stack.add_named(&list_page, Some("list"));
    stack.add_named(&delete_page, Some("delete"));
    stack.set_visible_child_name("empty");

    States {
        stack,
        create_title,
        delete_title,
        name,
        default_card,
        default_switch,
        form_status,
        form_actions,
        create_button,
        cancel_create,
        list,
        list_status,
        empty_save,
        add_preset,
        cancel_delete,
        confirm_delete,
    }
}

/// Wire the title button to its popover for this timeline. The list is read
/// when the popover opens and again after each action, never on a timer.
pub(super) fn attach(button: &Button, timeline: &Timeline, scale: f64) {
    let px = |value: f64| (value * scale).round() as i32;
    let popover = Popover::new();
    popover.add_css_class("video-editor-presets-popover");
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    popover.set_position(PositionType::Bottom);
    popover.set_halign(Align::End);
    popover.set_offset(px(14.0), px(6.0));
    popover.set_parent(button);

    let states = build_states(&popover, scale);
    popover.set_child(Some(&states.stack));

    let panel = Panel {
        popover: popover.clone(),
        stack: states.stack.clone(),
        create_title: states.create_title.clone(),
        delete_title: states.delete_title.clone(),
        name: states.name.clone(),
        default_card: states.default_card.clone(),
        default_switch: states.default_switch.clone(),
        form_status: states.form_status.clone(),
        form_actions: states.form_actions.clone(),
        create_button: states.create_button.clone(),
        list: states.list.clone(),
        list_status: states.list_status.clone(),
        pending: Rc::new(RefCell::new(Pending::Idle)),
        timeline: timeline.clone(),
        applied: Rc::new(RefCell::new(None)),
        scale,
    };

    let watching = Rc::new(Cell::new(false));
    popover.connect_show({
        let panel = panel.clone();
        let button = button.clone();
        let watching = watching.clone();
        move |popover| {
            if !watching.replace(true) {
                close_on_outside_press(popover, &button);
            }
            button.add_css_class("vep-open");
            redraw_child(&button);
            panel.open();
        }
    });
    popover.connect_closed({
        let button = button.clone();
        move |_| {
            button.remove_css_class("vep-open");
            redraw_child(&button);
        }
    });
    button.connect_clicked(move |_| {
        if popover.is_visible() {
            popover.popdown();
        } else {
            popover.popup();
        }
    });

    states.empty_save.connect_clicked({
        let panel = panel.clone();
        move |_| panel.begin_create()
    });
    states.add_preset.connect_clicked({
        let panel = panel.clone();
        move |_| panel.begin_create()
    });
    states.cancel_create.connect_clicked({
        let panel = panel.clone();
        move |_| panel.back()
    });
    states.cancel_delete.connect_clicked({
        let panel = panel.clone();
        move |_| panel.back()
    });
    states.create_button.connect_clicked({
        let panel = panel.clone();
        move |_| panel.submit()
    });
    states.confirm_delete.connect_clicked({
        let panel = panel.clone();
        move |_| panel.confirm_delete()
    });
    states.name.connect_activate({
        let panel = panel.clone();
        move |_| panel.submit()
    });
    states.name.connect_changed({
        let create_button = states.create_button.clone();
        move |entry| create_button.set_sensitive(!entry.text().trim().is_empty())
    });

    // Escape leaves the form or the delete question and returns to the list;
    // anywhere else it is left to the popover, which closes.
    let keys = EventControllerKey::new();
    keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let panel = panel.clone();
        move |_, key, _, _| {
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            let leaving = panel
                .stack
                .visible_child_name()
                .is_some_and(|page| page.as_str() == "form" || page.as_str() == "delete");
            if leaving {
                panel.back();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    });
    panel.stack.add_controller(keys);
}

impl Panel {
    fn open(&self) {
        *self.pending.borrow_mut() = Pending::Idle;
        self.clear_form_status();
        self.name.set_text("");
        self.default_switch.set_active(false);
        self.say_list("");
        // An undo, a document swap or a later edit can leave the highlighted
        // preset no longer applied. The popover has no history listener, so it
        // checks the highlight against the open project whenever it opens.
        let applied = self.applied.borrow().clone();
        if let Some(id) = applied {
            let still_applied =
                presets::load(&id).is_some_and(|preset| self.timeline.look_matches(&preset));
            if !still_applied {
                *self.applied.borrow_mut() = None;
            }
        }
        self.fill();
    }

    fn begin_create(&self) {
        *self.pending.borrow_mut() = Pending::Idle;
        self.create_title.set_text(&t("New preset"));
        self.create_button.set_label(&t("Create"));
        self.name.set_text("");
        self.default_switch.set_active(false);
        self.default_card.set_visible(true);
        self.form_actions.set_margin_top(0);
        self.clear_form_status();
        self.stack.set_visible_child_name("form");
        self.focus_name();
    }

    fn begin_rename(&self, preset: &Preset) {
        *self.pending.borrow_mut() = Pending::Rename(preset.id.clone());
        self.create_title.set_text(&t("Rename"));
        self.create_button.set_label(&t("Rename"));
        self.name.set_text(&preset.name);
        self.default_card.set_visible(false);
        self.form_actions
            .set_margin_top((16.0 * self.scale).round() as i32);
        self.clear_form_status();
        self.stack.set_visible_child_name("form");
        self.focus_name();
    }

    fn begin_delete(&self, preset: &Preset) {
        *self.pending.borrow_mut() = Pending::Delete(preset.id.clone(), preset.name.clone());
        self.delete_title
            .set_text(&tfmt("Delete {name}?", &[("name", &preset.name)]));
        self.stack.set_visible_child_name("delete");
    }

    /// Leave the form or the delete question. The list (or the empty state,
    /// when the last preset is gone) is rebuilt on the way back.
    fn back(&self) {
        *self.pending.borrow_mut() = Pending::Idle;
        self.clear_form_status();
        self.fill();
    }

    fn focus_name(&self) {
        let name = self.name.clone();
        glib::idle_add_local_once(move || {
            name.grab_focus();
        });
    }

    fn clear_form_status(&self) {
        self.form_status.set_text("");
        self.form_status.remove_css_class("vep-error");
        self.form_status.set_visible(false);
    }

    fn form_error(&self, text: &str) {
        self.form_status.set_text(text);
        self.form_status.add_css_class("vep-error");
        self.form_status.set_visible(true);
    }

    fn say_list(&self, text: &str) {
        self.list_status.set_text(text);
        self.list_status.set_visible(!text.is_empty());
        if text.is_empty() {
            return;
        }
        let status = self.list_status.clone();
        let text = text.to_owned();
        glib::timeout_add_local_once(Duration::from_secs(3), move || {
            if status.text().as_str() == text.as_str() {
                status.set_text("");
                status.set_visible(false);
            }
        });
    }

    fn refresh_later(&self) {
        let panel = self.clone();
        glib::idle_add_local_once(move || panel.fill());
    }

    /// Rebuild the rows and show the list, or the empty state when nothing is
    /// saved. Called from an idle callback when a row triggered it, because
    /// the row's own widgets are among the ones being replaced.
    fn fill(&self) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let presets = presets::list();
        let configured = load_config().rec_video_default_preset;
        for preset in &presets {
            let is_default = configured.as_deref() == Some(preset.id.as_str());
            self.list.append(&self.preset_row(preset, is_default));
        }
        self.stack
            .set_visible_child_name(if presets.is_empty() { "empty" } else { "list" });
    }

    fn preset_row(&self, preset: &Preset, is_default: bool) -> GtkBox {
        let px = |value: f64| (value * self.scale).round() as i32;
        let row = GtkBox::new(Orientation::Horizontal, 0);
        row.add_css_class("vep-row");
        if self.applied.borrow().as_deref() == Some(preset.id.as_str()) {
            row.add_css_class("vep-applied");
        }

        let pick = Button::new();
        pick.add_css_class("vep-pick");
        pick.set_has_frame(false);
        pick.set_focus_on_click(false);
        pick.set_hexpand(true);
        pick.set_tooltip_text(Some(&t("Apply to this project")));
        let content = GtkBox::new(Orientation::Horizontal, px(12.0));
        content.append(&thumbnail(preset, self.scale));
        let name = Label::new(Some(&preset.name));
        name.add_css_class("vep-row-name");
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(pango::EllipsizeMode::End);
        content.append(&name);
        if is_default {
            let pill = Label::new(Some(&t("Default")));
            pill.add_css_class("vep-pill");
            pill.set_valign(Align::Center);
            pill.set_margin_start(px(6.0));
            content.append(&pill);
        }
        pick.set_child(Some(&content));
        pick.connect_clicked({
            let panel = self.clone();
            let preset = preset.clone();
            move |_| {
                panel.timeline.apply_preset(&preset);
                *panel.applied.borrow_mut() = Some(preset.id.clone());
                panel.popover.popdown();
            }
        });

        row.append(&pick);
        row.append(&self.menu_button(preset, is_default));
        row
    }

    fn menu_button(&self, preset: &Preset, is_default: bool) -> Button {
        let menu_button = Button::new();
        menu_button.add_css_class("vep-menu");
        menu_button.set_has_frame(false);
        menu_button.set_focus_on_click(false);
        menu_button.set_valign(Align::Center);
        menu_button.set_margin_end((8.0 * self.scale).round() as i32);
        menu_button.set_tooltip_text(Some(&t("Preset options")));
        let glyph = Label::new(Some("\u{22ef}"));
        glyph.add_css_class("vep-menu-glyph");
        menu_button.set_child(Some(&glyph));

        let menu = Popover::new();
        menu.add_css_class("history-action-popover");
        menu.add_css_class("settings-select-popover");
        menu.set_has_arrow(false);
        menu.set_autohide(true);
        menu.set_position(PositionType::Bottom);
        menu.set_halign(Align::End);
        menu.set_offset(0, (4.0 * self.scale).round() as i32);
        menu.set_parent(&menu_button);

        // Drawn at its authored size like the timeline's clip menu, so a
        // high-DPI display does not inflate rows the shared sheet already sizes.
        let menu_scale = self.scale.min(1.0);
        let items = GtkBox::new(Orientation::Vertical, (2.0 * menu_scale).round() as i32);
        items.add_css_class("settings-select-menu");
        items.set_size_request((160.0 * menu_scale).round() as i32, -1);
        let default_label = if is_default {
            t("Remove as default")
        } else {
            t("Set as default")
        };
        let default_action = {
            let panel = self.clone();
            let id = preset.id.clone();
            move || panel.toggle_default(&id, is_default)
        };
        items.append(&menu_item(
            &menu,
            menu_scale,
            icon_names::custom::CHECK_SYMBOLIC,
            &default_label,
            default_action,
        ));
        items.append(&menu_item(
            &menu,
            menu_scale,
            icon_names::custom::WAND_SPARKLES_SYMBOLIC,
            &t("Update with current look"),
            {
                let panel = self.clone();
                let preset = preset.clone();
                move || panel.update(&preset)
            },
        ));
        items.append(&menu_item(
            &menu,
            menu_scale,
            icon_names::custom::PENCIL_SYMBOLIC,
            &t("Rename"),
            {
                let panel = self.clone();
                let preset = preset.clone();
                move || panel.begin_rename(&preset)
            },
        ));
        items.append(&menu_item(
            &menu,
            menu_scale,
            icon_names::shipped::FOLDER_OPEN_REGULAR,
            &t("Show in folder"),
            {
                let panel = self.clone();
                let id = preset.id.clone();
                move || panel.show_in_folder(&id)
            },
        ));
        items.append(&menu_item(
            &menu,
            menu_scale,
            icon_names::custom::USER_TRASH_SYMBOLIC,
            &t("Delete"),
            {
                let panel = self.clone();
                let preset = preset.clone();
                move || panel.begin_delete(&preset)
            },
        ));
        menu.set_child(Some(&items));

        menu_button.connect_clicked(move |_| menu.popup());
        menu_button
    }

    fn submit(&self) {
        let pending = self.pending.borrow().clone();
        match pending {
            Pending::Rename(id) => self.commit_rename(&id),
            _ => self.create(),
        }
    }

    fn create(&self) {
        let name = self.name.text().trim().to_owned();
        if name.is_empty() {
            self.form_error(&t("Name the preset before saving it."));
            return;
        }
        match self.timeline.save_preset(&name) {
            Ok(preset) => {
                let defaulted = if self.default_switch.is_active() {
                    set_default_video_preset(&preset.id)
                } else {
                    Ok(())
                };
                *self.pending.borrow_mut() = Pending::Idle;
                self.name.set_text("");
                self.default_switch.set_active(false);
                self.clear_form_status();
                match defaulted {
                    Ok(()) => self.say_list(&tfmt("{name} saved.", &[("name", &preset.name)])),
                    Err(err) => self.say_list(&err),
                }
                self.refresh_later();
            }
            Err(err) => self.form_error(&err),
        }
    }

    fn commit_rename(&self, id: &str) {
        let name = self.name.text().trim().to_owned();
        if name.is_empty() {
            self.form_error(&t("Name the preset before saving it."));
            return;
        }
        let Some(mut preset) = presets::load(id) else {
            *self.pending.borrow_mut() = Pending::Idle;
            self.clear_form_status();
            self.say_list(&t("That preset is no longer on disk."));
            self.refresh_later();
            return;
        };
        preset.name = name.to_owned();
        match presets::save(&preset) {
            Ok(()) => {
                *self.pending.borrow_mut() = Pending::Idle;
                self.clear_form_status();
                self.say_list(&tfmt("Renamed to {name}.", &[("name", &name)]));
                self.refresh_later();
            }
            Err(err) => self.form_error(&err),
        }
    }

    fn confirm_delete(&self) {
        let target = match &*self.pending.borrow() {
            Pending::Delete(id, name) => Some((id.clone(), name.clone())),
            _ => None,
        };
        let Some((id, name)) = target else {
            return;
        };
        *self.pending.borrow_mut() = Pending::Idle;
        if self.applied.borrow().as_deref() == Some(id.as_str()) {
            *self.applied.borrow_mut() = None;
        }
        match presets::delete(&id).and_then(|()| forget_default(&id)) {
            Ok(()) => self.say_list(&tfmt("{name} deleted.", &[("name", &name)])),
            Err(err) => self.say_list(&err),
        }
        self.refresh_later();
    }

    fn toggle_default(&self, id: &str, is_default: bool) {
        let next = if is_default { "" } else { id };
        if let Err(err) = set_default_video_preset(next) {
            self.say_list(&err);
        }
        self.refresh_later();
    }

    fn update(&self, preset: &Preset) {
        match self.timeline.update_preset(preset) {
            Ok(_) => self.say_list(&tfmt(
                "{name} now holds the current look.",
                &[("name", &preset.name)],
            )),
            Err(err) => self.say_list(&err),
        }
        self.refresh_later();
    }

    fn show_in_folder(&self, id: &str) {
        let Some(path) = presets::path_of(id) else {
            self.say_list(&t("That preset is no longer on disk."));
            return;
        };
        let opened = std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn();
        if opened.is_err() {
            self.say_list(&t("Could not open the preset's folder."));
        }
    }
}

fn menu_item(
    menu: &Popover,
    scale: f64,
    icon: &str,
    label: &str,
    action: impl Fn() + 'static,
) -> Button {
    let button = Button::new();
    button.add_css_class("history-action-btn");
    button.set_has_frame(false);
    button.set_focus_on_click(false);
    button.set_size_request(-1, (32.0 * scale).round() as i32);

    let row = GtkBox::new(Orientation::Horizontal, (10.0 * scale).round() as i32);
    let image = Image::from_icon_name(icon);
    image.set_pixel_size((15.0 * scale).round() as i32);
    let text = Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_hexpand(true);
    row.append(&image);
    row.append(&text);
    button.set_child(Some(&row));

    let menu = menu.clone();
    button.connect_clicked(move |_| {
        menu.popdown();
        action();
    });
    button
}

/// The preset's wallpaper as the Background grid draws it, clipped to the
/// row's six-pixel corners, or a neutral swatch when the look has no wallpaper
/// picked.
fn thumbnail(preset: &Preset, scale: f64) -> DrawingArea {
    let width = (52.0 * scale).round() as i32;
    let height = (34.0 * scale).round() as i32;
    let pixbuf = preset
        .stage
        .selection
        .as_deref()
        .and_then(background::thumb_path)
        .and_then(|path| Pixbuf::from_file_at_scale(&path, width * 2, height * 2, true).ok());
    let area = DrawingArea::new();
    area.set_content_width(width);
    area.set_content_height(height);
    area.set_valign(Align::Center);
    area.set_can_target(false);
    let radius = 6.0 * scale;
    area.set_draw_func(move |_, context, width, height| {
        let _ = context.save();
        rounded_rect(
            context,
            0.0,
            0.0,
            f64::from(width),
            f64::from(height),
            radius,
        );
        context.clip();
        match &pixbuf {
            Some(pixbuf) => draw_wallpaper_thumbnail(context, width, height, pixbuf),
            None => draw_wallpaper_fallback(context, width, height),
        }
        let _ = context.restore();
    });
    area
}

/// Clear the default when it names the preset that was just deleted, so a
/// recording never opens on a look that no longer exists.
fn forget_default(id: &str) -> Result<(), String> {
    if load_config().rec_video_default_preset.as_deref() == Some(id) {
        set_default_video_preset("")
    } else {
        Ok(())
    }
}

/// Remember a preset as the look every new recording opens on. An empty id
/// goes back to the built-in look, which is what the import falls back to.
fn set_default_video_preset(id: &str) -> Result<(), String> {
    let config = AppConfig {
        rec_video_default_preset: (!id.is_empty()).then(|| id.to_owned()),
        ..load_config()
    };
    save_config(&config)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk4::{CssProvider, Label, Popover, Widget};
    use std::cell::Cell;
    use std::rc::Rc;

    fn collect_labels(root: &impl IsA<Widget>) -> Vec<String> {
        let root = root.as_ref();
        let mut found = Vec::new();
        if let Some(label) = root.downcast_ref::<Label>() {
            found.push(label.text().to_string());
        }
        let mut child = root.first_child();
        while let Some(widget) = child {
            found.extend(collect_labels(&widget));
            child = widget.next_sibling();
        }
        found
    }

    #[test]
    fn the_presets_sheet_parses_at_every_windows_scale() {
        let Some(()) = crate::test_support::with_gtk(|| {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let errors = Rc::new(Cell::new(0));
                let provider = CssProvider::new();
                provider.connect_parsing_error({
                    let errors = errors.clone();
                    move |_, _, _| errors.set(errors.get() + 1)
                });
                provider.load_from_data(&style_css(scale));
                assert_eq!(errors.get(), 0, "scale {scale}");
            }
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn the_card_opens_empty_and_carries_one_page_per_state() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let popover = Popover::new();
            let states = build_states(&popover, 1.0);
            assert_eq!(states.stack.visible_child_name().as_deref(), Some("empty"));

            let mut pages = Vec::new();
            let mut child = states.stack.first_child();
            while let Some(page) = child {
                if let Some(name) = states.stack.page(&page).name() {
                    pages.push(name.to_string());
                }
                child = page.next_sibling();
            }
            pages.sort();
            assert_eq!(pages, ["delete", "empty", "form", "list"]);

            assert_eq!(
                states.create_title.text().as_str(),
                t("New preset").as_str()
            );
            assert_eq!(
                states.name.placeholder_text().as_deref(),
                Some(t("Preset name").as_str())
            );
            assert!(!states.default_switch.is_active());
            assert!(states.default_card.is_visible());
            assert!(!states.list_status.is_visible());

            let labels = collect_labels(&states.stack);
            for expected in [
                t("Presets"),
                t("No presets yet"),
                t("Save this project's background, cursor, camera and captions, then put them on any recording."),
                t("Save current look"),
                t("Name").to_uppercase(),
                t("Default for new recordings"),
                t("New recordings open with this look"),
                t("Cancel"),
                t("Create"),
                t("Delete"),
            ] {
                assert!(labels.contains(&expected), "missing label: {expected}");
            }
            assert!(!labels.contains(&t("New preset from this project")));
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
