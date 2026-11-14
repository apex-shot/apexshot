//! The presets button beside Export, and the popover it opens.
//!
//! A preset is a saved look. Clicking one puts it on the open project as one
//! undo step; the gear beside it keeps it as the default for new recordings,
//! updates it from the current look, renames it, shows its file or deletes it.

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
    gdk, glib, Align, Box as GtkBox, Button, CssProvider, DrawingArea, Entry, EventControllerFocus,
    EventControllerKey, EventControllerLegacy, Image, Label, Orientation, PickFlags, Popover,
    PositionType, Switch, Widget,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The row the list is waiting on: one rename or one delete confirmation at a
/// time, named by the id of the preset it belongs to.
#[derive(Clone, PartialEq)]
enum Pending {
    Idle,
    Rename(String),
    Delete(String),
}

/// The popover and everything its handlers reach. Each handler holds a clone,
/// so they all see the same list, form and status line.
#[derive(Clone)]
struct Panel {
    popover: Popover,
    form: GtkBox,
    plus: Button,
    name: Entry,
    default_switch: Switch,
    list: GtkBox,
    status: Label,
    pending: Rc<RefCell<Pending>>,
    timeline: Timeline,
    applied: Rc<RefCell<Option<String>>>,
    scale: f64,
}

/// The editor's accent, the orange the Export button is filled with.
const ACCENT: (f64, f64, f64) = (1.0, 0.404, 0.051);

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
    let px = |value: f64| (value * scale).round() as i32;
    let css = format!(
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
            background: #1c1c1c; color: #ececec;
            border: 1px solid alpha(white, 0.10); border-radius: {r10}px;
            box-shadow: none; padding: 0 0 {p8}px 0;
        }}
        .vep-header {{ padding: {p8}px {p8}px {p6}px {p14}px; }}
        .vep-title {{ font-size: {f13}px; font-weight: 600; color: alpha(white, 0.92); }}
        button.vep-icon-tile {{
            min-width: {s24}px; min-height: {s24}px; padding: 0; border: none; box-shadow: none;
            border-radius: {r6}px; background: transparent; color: alpha(white, 0.78);
        }}
        button.vep-icon-tile:hover {{ background: alpha(white, 0.10); color: #ffffff; }}
        button.vep-icon-tile.vep-open {{ background: alpha(#ff670d, 0.20); color: #ff9a5c; }}
        button.vep-row {{
            min-height: 0; padding: {p6}px; border: none; box-shadow: none;
            border-radius: {r10}px; background: transparent; color: #ececec;
        }}
        button.vep-row:hover {{ background: alpha(white, 0.06); }}
        button.vep-row.vep-applied {{ background: alpha(#ff670d, 0.18); }}
        button.vep-row.vep-applied:hover {{ background: alpha(#ff670d, 0.24); }}
        .vep-name {{ font-size: {f14}px; font-weight: 600; }}
        .vep-pill {{
            font-size: {f11}px; font-weight: 700; color: #ff9a5c;
            background: alpha(#ff670d, 0.18); border-radius: {r6}px; padding: {p3}px {p8}px;
        }}
        button.vep-gear {{
            min-width: {s30}px; min-height: {s30}px; padding: 0; border: none; box-shadow: none;
            border-radius: {r8}px; background: transparent; color: #8c8c8c;
        }}
        button.vep-gear:hover {{ background: alpha(white, 0.08); color: #ececec; }}
        .vep-muted {{ color: #8f8f8f; font-size: {f12}px; }}
        .vep-empty-title {{ font-size: {f14}px; font-weight: 600; }}
        .vep-form {{ margin: 0 {p8}px {p8}px {p8}px; }}
        .vep-form-label {{ font-size: {f13}px; color: alpha(white, 0.78); }}
        entry.vep-entry {{
            min-height: {s34}px; padding: 0 {p10}px; border-radius: {r8}px;
            background: alpha(white, 0.06); color: alpha(white, 0.92); border: 1px solid transparent;
            box-shadow: none; outline: none; font-size: {f13}px;
        }}
        entry.vep-entry:focus-within {{ border-color: alpha(white, 0.22); background: alpha(white, 0.08); }}
        button.vep-secondary, button.vep-primary, button.vep-danger {{
            min-height: {s32}px; padding: 0 {p18}px; border: none; box-shadow: none;
            border-radius: {r8}px; font-weight: 600; font-size: {f13}px;
        }}
        button.vep-secondary {{ background: transparent; color: alpha(white, 0.78); }}
        button.vep-secondary:hover {{ background: alpha(white, 0.08); color: #ffffff; }}
        button.vep-primary {{ background: #ff670d; color: #ffffff; }}
        button.vep-primary:hover {{ background: #ff7c2f; }}
        button.vep-primary:disabled {{ background: alpha(white, 0.06); color: alpha(white, 0.35); }}
        button.vep-danger {{ background: #e5484d; color: #ffffff; }}
        button.vep-danger:hover {{ background: #ef5c60; }}
        .vep-confirm {{ background: alpha(#e5484d, 0.10); border-radius: {r8}px; padding: {p10}px; margin: {p2}px {p8}px; }}
        .vep-status {{ color: #a4a4a4; font-size: {f12}px; }}

        .editor-theme-light .vep-card {{
            background: #f2f3f4; color: #1f2328; border-color: alpha(#111827, 0.14);
        }}
        .editor-theme-light .vep-title {{ color: #1f2328; }}
        .editor-theme-light button.vep-icon-tile {{ background: transparent; color: #3a3f45; }}
        .editor-theme-light button.vep-icon-tile:hover {{ background: alpha(#111827, 0.08); }}
        .editor-theme-light button.vep-row {{ color: #1f2328; }}
        .editor-theme-light button.vep-row:hover {{ background: alpha(#111827, 0.05); }}
        .editor-theme-light button.vep-row.vep-applied {{ background: alpha(#ff670d, 0.14); }}
        .editor-theme-light .vep-pill {{ color: #b74809; background: alpha(#ff670d, 0.14); }}
        .editor-theme-light button.vep-gear {{ color: #6b7076; }}
        .editor-theme-light button.vep-gear:hover {{ background: alpha(#111827, 0.06); color: #1f2328; }}
        .editor-theme-light .vep-muted, .editor-theme-light .vep-status {{ color: #6b7076; }}
        .editor-theme-light .vep-form-label {{ color: #3a3f45; }}
        .editor-theme-light entry.vep-entry {{ background: alpha(#111827, 0.06); color: #1f2328; }}
        .editor-theme-light entry.vep-entry:focus-within {{ border-color: alpha(#111827, 0.32); }}
        .editor-theme-light button.vep-primary:disabled {{ background: alpha(#111827, 0.06); color: alpha(#111827, 0.35); }}
        .editor-theme-light button.vep-secondary {{ background: transparent; color: #3a3f45; }}
        .editor-theme-light button.vep-secondary:hover {{ background: alpha(#111827, 0.08); color: #1f2328; }}
        "#,
        p2 = px(2.0),
        p3 = px(3.0),
        p6 = px(6.0),
        p8 = px(8.0),
        p10 = px(10.0),
        p14 = px(14.0),
        p18 = px(18.0),
        r6 = px(6.0),
        r8 = px(8.0),
        r10 = px(10.0),
        s24 = px(24.0),
        s30 = px(30.0),
        s32 = px(32.0),
        s34 = px(34.0),
        f11 = px(11.0),
        f12 = px(12.0),
        f13 = px(13.0),
        f14 = px(14.0),
    );
    let provider = CssProvider::new();
    provider.load_from_data(&css);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 7,
    );
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

/// A plus sign drawn in the widget's own colour, so it follows hover and the
/// open state of the tile it sits on.
fn plus_glyph(scale: f64) -> DrawingArea {
    let size = (12.0 * scale).round() as i32;
    let glyph = DrawingArea::new();
    glyph.set_content_width(size);
    glyph.set_content_height(size);
    glyph.set_halign(Align::Center);
    glyph.set_valign(Align::Center);
    glyph.set_can_target(false);
    glyph.set_draw_func(|glyph, context, width, height| {
        let color = glyph.style_context().color();
        let (width, height) = (f64::from(width), f64::from(height));
        let size = width.min(height);
        context.set_source_rgba(
            f64::from(color.red()),
            f64::from(color.green()),
            f64::from(color.blue()),
            f64::from(color.alpha()),
        );
        context.set_line_width((size / 8.0).max(1.4));
        context.set_line_cap(gtk4::cairo::LineCap::Round);
        let (cx, cy, arm) = (width * 0.5, height * 0.5, size * 0.42);
        context.move_to(cx - arm, cy);
        context.line_to(cx + arm, cy);
        context.move_to(cx, cy - arm);
        context.line_to(cx, cy + arm);
        let _ = context.stroke();
    });
    glyph
}

fn redraw_child(widget: &impl IsA<Widget>) {
    if let Some(child) = widget.as_ref().first_child() {
        child.queue_draw();
    }
}

/// Close `popover` when a press lands on its window outside it. Autohide
/// alone loses its grab once the nested gear menu has opened and closed, so
/// the window watches presses itself. Presses inside the popover, inside a
/// popover it owns (the gear menu, an entry's context menu), or on `button`
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

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("vep-card");
    body.set_size_request(px(280.0), -1);

    let header = GtkBox::new(Orientation::Horizontal, px(8.0));
    header.add_css_class("vep-header");
    let title = Label::new(Some(&t("Presets")));
    title.add_css_class("vep-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_valign(Align::Center);
    let plus = Button::new();
    plus.add_css_class("vep-icon-tile");
    plus.set_has_frame(false);
    plus.set_focus_on_click(false);
    plus.set_valign(Align::Center);
    plus.set_tooltip_text(Some(&t("New preset from this project")));
    plus.set_child(Some(&plus_glyph(scale)));
    let close = Button::new();
    close.add_css_class("vep-icon-tile");
    close.set_has_frame(false);
    close.set_focus_on_click(false);
    close.set_valign(Align::Center);
    close.set_tooltip_text(Some(&t("Close")));
    let close_icon = Image::from_icon_name("window-close-symbolic");
    close_icon.set_pixel_size(px(13.0));
    close.set_child(Some(&close_icon));
    close.connect_clicked({
        let popover = popover.clone();
        move |_| popover.popdown()
    });
    header.append(&title);
    header.append(&plus);
    header.append(&close);
    body.append(&header);

    let form = GtkBox::new(Orientation::Vertical, px(12.0));
    form.add_css_class("vep-form");
    form.set_visible(false);
    let name = Entry::new();
    name.add_css_class("vep-entry");
    name.set_max_length(120);
    name.set_placeholder_text(Some(&t("Preset name")));
    form.append(&name);

    let default_row = GtkBox::new(Orientation::Horizontal, px(8.0));
    let default_label = Label::new(Some(&t("Default for new recordings")));
    default_label.add_css_class("vep-form-label");
    default_label.set_xalign(0.0);
    default_label.set_hexpand(true);
    let default_switch = Switch::new();
    default_switch.add_css_class("recording-editor-zoom-switch");
    default_switch.set_valign(Align::Center);
    default_row.append(&default_label);
    default_row.append(&default_switch);
    form.append(&default_row);

    let form_buttons = GtkBox::new(Orientation::Horizontal, 0);
    form_buttons.set_margin_top(px(4.0));
    let cancel = Button::with_label(&t("Cancel"));
    cancel.add_css_class("vep-secondary");
    cancel.set_has_frame(false);
    cancel.set_halign(Align::Start);
    cancel.set_hexpand(true);
    let create = Button::with_label(&t("Create"));
    create.add_css_class("vep-primary");
    create.set_has_frame(false);
    create.set_sensitive(false);
    create.set_halign(Align::End);
    form_buttons.append(&cancel);
    form_buttons.append(&create);
    form.append(&form_buttons);
    body.append(&form);

    let list = GtkBox::new(Orientation::Vertical, px(2.0));
    list.set_margin_start(px(8.0));
    list.set_margin_end(px(8.0));
    body.append(&list);

    let status = Label::new(None);
    status.add_css_class("vep-status");
    status.set_xalign(0.0);
    status.set_margin_start(px(14.0));
    status.set_margin_end(px(14.0));
    status.set_margin_top(px(8.0));
    status.set_wrap(true);
    status.set_max_width_chars(38);
    status.set_visible(false);
    body.append(&status);
    popover.set_child(Some(&body));

    let panel = Panel {
        popover: popover.clone(),
        form,
        plus: plus.clone(),
        name: name.clone(),
        default_switch,
        list,
        status,
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
    plus.connect_clicked({
        let panel = panel.clone();
        move |_| {
            let showing = !panel.form.is_visible();
            if showing {
                panel.form.set_visible(true);
                panel.plus.add_css_class("vep-open");
                panel.name.grab_focus();
            } else {
                panel.close_form();
            }
        }
    });
    name.connect_changed({
        let create = create.clone();
        move |name| create.set_sensitive(!name.text().trim().is_empty())
    });
    cancel.connect_clicked({
        let panel = panel.clone();
        move |_| panel.close_form()
    });
    create.connect_clicked({
        let panel = panel.clone();
        move |_| panel.create()
    });
    name.connect_activate(move |_| panel.create());
}

impl Panel {
    fn open(&self) {
        *self.pending.borrow_mut() = Pending::Idle;
        self.close_form();
        self.say("");
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

    fn close_form(&self) {
        self.form.set_visible(false);
        self.plus.remove_css_class("vep-open");
        self.name.set_text("");
        self.default_switch.set_active(false);
    }

    fn say(&self, text: &str) {
        self.status.set_text(text);
        self.status.set_visible(!text.is_empty());
    }

    fn refresh_later(&self) {
        let panel = self.clone();
        glib::idle_add_local_once(move || panel.fill());
    }

    fn begin(&self, pending: Pending) {
        *self.pending.borrow_mut() = pending;
        self.refresh_later();
    }

    /// Rebuild the rows. Called from an idle callback when a row triggered it,
    /// because the row's own button is among the widgets being replaced.
    fn fill(&self) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let presets = presets::list();
        if presets.is_empty() {
            self.list.append(&self.empty_state());
            return;
        }
        let configured = load_config().rec_video_default_preset;
        let pending = self.pending.borrow().clone();
        for preset in &presets {
            let is_default = configured.as_deref() == Some(preset.id.as_str());
            let row = match &pending {
                Pending::Delete(id) if *id == preset.id => self.confirm_row(preset),
                Pending::Rename(id) if *id == preset.id => self.rename_row(preset),
                _ => self.preset_row(preset, is_default),
            };
            self.list.append(&row);
        }
    }

    fn empty_state(&self) -> GtkBox {
        let px = |value: f64| (value * self.scale).round() as i32;
        let empty = GtkBox::new(Orientation::Vertical, px(6.0));
        empty.set_margin_top(px(14.0));
        empty.set_margin_bottom(px(14.0));
        let glyph_size = px(28.0);
        let glyph = DrawingArea::new();
        glyph.set_content_width(glyph_size);
        glyph.set_content_height(glyph_size);
        glyph.set_halign(Align::Center);
        glyph.add_css_class("vep-muted");
        glyph.set_draw_func(|glyph, context, width, height| {
            let color = glyph.style_context().color();
            draw_bookmark(
                context,
                f64::from(width),
                f64::from(height),
                (
                    f64::from(color.red()),
                    f64::from(color.green()),
                    f64::from(color.blue()),
                    f64::from(color.alpha()),
                ),
                false,
            );
        });
        let title = Label::new(Some(&t("No presets yet")));
        title.add_css_class("vep-empty-title");
        let hint = Label::new(Some(&t(
            "Press + to save this project's background, cursor, camera and captions as a preset.",
        )));
        hint.add_css_class("vep-muted");
        hint.set_justify(gtk4::Justification::Center);
        hint.set_wrap(true);
        hint.set_max_width_chars(34);
        empty.append(&glyph);
        empty.append(&title);
        empty.append(&hint);
        empty
    }

    fn preset_row(&self, preset: &Preset, is_default: bool) -> GtkBox {
        let px = |value: f64| (value * self.scale).round() as i32;
        let row = GtkBox::new(Orientation::Horizontal, px(4.0));

        let pick = Button::new();
        pick.add_css_class("vep-row");
        if self.applied.borrow().as_deref() == Some(preset.id.as_str()) {
            pick.add_css_class("vep-applied");
        }
        pick.set_has_frame(false);
        pick.set_focus_on_click(false);
        pick.set_hexpand(true);
        pick.set_tooltip_text(Some(&t("Apply to this project")));
        let content = GtkBox::new(Orientation::Horizontal, px(12.0));
        content.append(&thumbnail(preset, self.scale));
        let name = Label::new(Some(&preset.name));
        name.add_css_class("vep-name");
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        content.append(&name);
        if is_default {
            let pill = Label::new(Some(&t("Default")));
            pill.add_css_class("vep-pill");
            pill.set_valign(Align::Center);
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
        row.append(&self.gear_button(preset, is_default));
        row
    }

    fn gear_button(&self, preset: &Preset, is_default: bool) -> Button {
        let gear = Button::new();
        gear.add_css_class("vep-gear");
        gear.set_has_frame(false);
        gear.set_focus_on_click(false);
        gear.set_valign(Align::Center);
        gear.set_tooltip_text(Some(&t("Preset options")));
        let icon = Image::from_icon_name(icon_names::custom::SETTINGS_SYMBOLIC);
        icon.set_pixel_size((17.0 * self.scale).round() as i32);
        gear.set_child(Some(&icon));

        let menu = Popover::new();
        menu.add_css_class("history-action-popover");
        menu.add_css_class("settings-select-popover");
        menu.set_has_arrow(false);
        menu.set_autohide(true);
        menu.set_position(PositionType::Bottom);
        menu.set_halign(Align::End);
        menu.set_offset(0, (4.0 * self.scale).round() as i32);
        menu.set_parent(&gear);

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
                let id = preset.id.clone();
                move || panel.begin(Pending::Rename(id.clone()))
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
                let id = preset.id.clone();
                move || panel.begin(Pending::Delete(id.clone()))
            },
        ));
        menu.set_child(Some(&items));

        gear.connect_clicked(move |_| menu.popup());
        gear
    }

    fn rename_row(&self, preset: &Preset) -> GtkBox {
        let row = GtkBox::new(Orientation::Horizontal, (12.0 * self.scale).round() as i32);
        row.set_margin_start((6.0 * self.scale).round() as i32);
        row.set_margin_end((6.0 * self.scale).round() as i32);
        row.set_margin_top((4.0 * self.scale).round() as i32);
        row.set_margin_bottom((4.0 * self.scale).round() as i32);
        row.append(&thumbnail(preset, self.scale));
        let entry = Entry::new();
        entry.add_css_class("vep-entry");
        entry.set_valign(Align::Center);
        entry.set_max_length(120);
        entry.set_text(&preset.name);
        entry.set_hexpand(true);
        entry.connect_activate({
            let panel = self.clone();
            let id = preset.id.clone();
            move |entry| panel.commit_rename(&id, &entry.text())
        });

        let keys = EventControllerKey::new();
        keys.connect_key_pressed({
            let panel = self.clone();
            let id = preset.id.clone();
            move |_, key, _, _| {
                if key != gdk::Key::Escape {
                    return glib::Propagation::Proceed;
                }
                if panel.end_rename(&id) {
                    panel.refresh_later();
                }
                glib::Propagation::Stop
            }
        });
        entry.add_controller(keys);

        let focus = EventControllerFocus::new();
        focus.connect_leave({
            let panel = self.clone();
            let id = preset.id.clone();
            let entry = entry.clone();
            move |_| panel.commit_rename(&id, &entry.text())
        });
        entry.add_controller(focus);

        row.append(&entry);
        glib::idle_add_local_once({
            let entry = entry.clone();
            move || {
                entry.grab_focus();
            }
        });
        row
    }

    fn confirm_row(&self, preset: &Preset) -> GtkBox {
        let row = GtkBox::new(Orientation::Vertical, (10.0 * self.scale).round() as i32);
        row.add_css_class("vep-confirm");
        let question = Label::new(Some(&tfmt("Delete {name}?", &[("name", &preset.name)])));
        question.set_xalign(0.0);
        question.set_hexpand(true);
        question.set_ellipsize(gtk4::pango::EllipsizeMode::End);

        question.add_css_class("vep-form-label");
        let buttons = GtkBox::new(Orientation::Horizontal, 0);
        let cancel = Button::with_label(&t("Cancel"));
        cancel.add_css_class("vep-secondary");
        cancel.set_has_frame(false);
        cancel.set_halign(Align::Start);
        cancel.set_hexpand(true);
        cancel.connect_clicked({
            let panel = self.clone();
            move |_| panel.begin(Pending::Idle)
        });
        let delete = Button::with_label(&t("Delete"));
        delete.add_css_class("vep-danger");
        delete.set_has_frame(false);
        delete.set_halign(Align::End);
        delete.connect_clicked({
            let panel = self.clone();
            let id = preset.id.clone();
            let name = preset.name.clone();
            move |_| panel.delete(&id, &name)
        });

        buttons.append(&cancel);
        buttons.append(&delete);
        row.append(&question);
        row.append(&buttons);
        row
    }

    fn create(&self) {
        let name = self.name.text().trim().to_owned();
        if name.is_empty() {
            self.say(&t("Name the preset before saving it."));
            return;
        }
        match self.timeline.save_preset(&name) {
            Ok(preset) => {
                let defaulted = if self.default_switch.is_active() {
                    set_default_video_preset(&preset.id)
                } else {
                    Ok(())
                };
                match defaulted {
                    Ok(()) => {
                        self.close_form();
                        self.say(&tfmt("{name} saved.", &[("name", &preset.name)]));
                    }
                    Err(err) => self.say(&err),
                }
            }
            Err(err) => self.say(&err),
        }
        self.refresh_later();
    }

    fn toggle_default(&self, id: &str, is_default: bool) {
        let next = if is_default { "" } else { id };
        if let Err(err) = set_default_video_preset(next) {
            self.say(&err);
        }
        self.refresh_later();
    }

    fn update(&self, preset: &Preset) {
        match self.timeline.update_preset(preset) {
            Ok(_) => self.say(&tfmt(
                "{name} now holds the current look.",
                &[("name", &preset.name)],
            )),
            Err(err) => self.say(&err),
        }
        self.refresh_later();
    }

    fn show_in_folder(&self, id: &str) {
        let Some(path) = presets::path_of(id) else {
            self.say(&t("That preset is no longer on disk."));
            return;
        };
        let opened = std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn();
        if opened.is_err() {
            self.say(&t("Could not open the preset's folder."));
        }
    }

    /// Take the rename pending on `id`, if that is the one pending. Returns
    /// whether it was, so a second end of the same rename does nothing.
    fn end_rename(&self, id: &str) -> bool {
        let mut pending = self.pending.borrow_mut();
        if *pending == Pending::Rename(id.to_owned()) {
            *pending = Pending::Idle;
            return true;
        }
        false
    }

    fn commit_rename(&self, id: &str, text: &str) {
        if !self.end_rename(id) {
            return;
        }
        let name = text.trim();
        if name.is_empty() {
            self.say(&t("Name the preset before saving it."));
        } else if let Some(mut preset) = presets::load(id) {
            preset.name = name.to_owned();
            match presets::save(&preset) {
                Ok(()) => self.say(&tfmt("Renamed to {name}.", &[("name", name)])),
                Err(err) => self.say(&err),
            }
        } else {
            self.say(&t("That preset is no longer on disk."));
        }
        self.refresh_later();
    }

    fn delete(&self, id: &str, name: &str) {
        *self.pending.borrow_mut() = Pending::Idle;
        if self.applied.borrow().as_deref() == Some(id) {
            *self.applied.borrow_mut() = None;
        }
        match presets::delete(id).and_then(|()| forget_default(id)) {
            Ok(()) => self.say(&tfmt("{name} deleted.", &[("name", name)])),
            Err(err) => self.say(&err),
        }
        self.refresh_later();
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

/// The preset's wallpaper as the Background grid draws it, or a neutral swatch
/// when the look has no wallpaper picked.
fn thumbnail(preset: &Preset, scale: f64) -> DrawingArea {
    let width = (54.0 * scale).round() as i32;
    let height = (38.0 * scale).round() as i32;
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
    area.set_draw_func(move |_, context, width, height| match &pixbuf {
        Some(pixbuf) => draw_wallpaper_thumbnail(context, width, height, pixbuf),
        None => draw_wallpaper_fallback(context, width, height),
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
