//! The presets button in the title bar, and the popover it opens.
//!
//! A preset is a saved look. Clicking one puts it on the open project; the
//! gear beside it keeps it as the default for new recordings, updates it from
//! the current look, renames it, shows its file or deletes it. Saving a look
//! never touches the footage: the stage, the cursor and the click sound are
//! the whole of it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use gtk4::{
    gdk, glib, Align, Box as GtkBox, Button, DrawingArea, Entry, EventControllerKey,
    EventControllerLegacy, Image, Label, Orientation, PickFlags, PolicyType, Popover, PositionType,
    ScrolledWindow, Separator, Stack, StackTransitionType, Switch, Widget,
};

use super::super::model::VideoEditState;
use super::super::presets::{self, Preset, StageFill};
use super::custom_wallpaper_popover::bitmap_to_surface;
use crate::capture::editor::window::icon_names;
use crate::i18n::{t, tfmt};
use crate::recording::editor::model::background_render::render_gradient;

/// What the form is for. Rename reuses the create view but writes back over
/// the preset it opened from, and leaves the default switch out of it.
#[derive(Clone, PartialEq)]
enum FormMode {
    Create,
    Rename(String),
}

/// The stack pages the card switches between. The empty prompt, the list and
/// the form never show together; the delete confirmation takes the whole card
/// the way the form does.
const EMPTY_PAGE: &str = "empty";
const LIST_PAGE: &str = "list";
const FORM_PAGE: &str = "form";
const DELETE_PAGE: &str = "delete";

/// The popover and everything its handlers reach. Each handler holds a clone,
/// so they all see the same stack, list and status lines.
#[derive(Clone)]
struct Panel {
    popover: Popover,
    stack: Stack,
    title: Label,
    rows: GtkBox,
    list_status: Label,
    form_status: Label,
    default_card: GtkBox,
    name: Entry,
    default_switch: Switch,
    primary: Button,
    delete_page: GtkBox,
    mode: Rc<RefCell<FormMode>>,
    /// The page the form returns to on Cancel, so an empty list comes back to
    /// the empty prompt instead of a list it does not have.
    return_page: Rc<RefCell<String>>,
    /// Counts status messages so a stale timeout cannot clear a newer one.
    status_serial: Rc<Cell<u64>>,
    state: Arc<Mutex<VideoEditState>>,
    ping: Rc<dyn Fn()>,
    applied: Rc<RefCell<Option<String>>>,
}

/// The editor's accent, the orange the Export button is filled with.
const ACCENT: (f64, f64, f64) = (1.0, 0.404, 0.051);

/// The bookmark button before Export in the title bar. It stays insensitive
/// until a recording is open, as Export does, and fills with the accent while
/// its popover is open.
pub(super) fn title_button() -> Button {
    let button = Button::new();
    button.add_css_class("recording-editor-title-presets");
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_valign(Align::Center);
    button.set_tooltip_text(Some(&t("Presets")));
    button.set_sensitive(false);

    let glyph = DrawingArea::new();
    glyph.set_content_width(20);
    glyph.set_content_height(20);
    glyph.set_halign(Align::Center);
    glyph.set_valign(Align::Center);
    glyph.set_can_target(false);
    glyph.set_draw_func(|glyph, context, width, height| {
        let open = glyph
            .parent()
            .is_some_and(|button| button.has_css_class("recording-editor-presets-open"));
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
fn plus_glyph() -> DrawingArea {
    let glyph = DrawingArea::new();
    glyph.set_content_width(12);
    glyph.set_content_height(12);
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

/// Three dots for the row's options button, drawn here rather than taken from
/// the host icon theme so the glyph is the same on every desktop.
fn dots_glyph() -> DrawingArea {
    let glyph = DrawingArea::new();
    glyph.set_content_width(16);
    glyph.set_content_height(16);
    glyph.set_halign(Align::Center);
    glyph.set_valign(Align::Center);
    glyph.set_can_target(false);
    glyph.set_draw_func(|glyph, context, width, height| {
        let color = glyph.style_context().color();
        context.set_source_rgba(
            f64::from(color.red()),
            f64::from(color.green()),
            f64::from(color.blue()),
            f64::from(color.alpha()),
        );
        let (cx, cy) = (f64::from(width) * 0.5, f64::from(height) * 0.5);
        for offset in [-5.0, 0.0, 5.0] {
            context.arc(cx + offset, cy, 1.6, 0.0, std::f64::consts::TAU);
            let _ = context.fill();
        }
    });
    glyph
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

/// Wire the title button to its popover for this edit. The list is read when
/// the popover opens and again after each action, never on a timer.
pub(super) fn attach(button: &Button, state: Arc<Mutex<VideoEditState>>, ping: Rc<dyn Fn()>) {
    let popover = Popover::new();
    popover.add_css_class("recording-editor-presets-popover");
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    popover.set_position(PositionType::Bottom);
    popover.set_halign(Align::End);
    popover.set_offset(14, 6);
    popover.set_parent(button);

    // The card is 320 wide; the CSS 18px padding and 1px rim sit inside it.
    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("recording-editor-presets-card");
    body.set_size_request(320, -1);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.add_css_class("rep-header");
    let title = Label::new(Some(&t("Presets")));
    title.add_css_class("rep-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_valign(Align::Center);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    let close = Button::new();
    close.add_css_class("rep-close");
    close.set_has_frame(false);
    close.set_focus_on_click(false);
    close.set_valign(Align::Center);
    close.set_tooltip_text(Some(&t("Close")));
    let close_icon = Image::from_icon_name("window-close-symbolic");
    close_icon.set_pixel_size(14);
    close.set_child(Some(&close_icon));
    close.connect_clicked({
        let popover = popover.clone();
        move |_| popover.popdown()
    });
    header.append(&title);
    header.append(&close);
    body.append(&header);

    let stack = Stack::new();
    stack.set_transition_type(StackTransitionType::Crossfade);
    stack.set_transition_duration(150);
    // Each state sizes the card to itself; the empty prompt must not reserve
    // the form's height.
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);

    // ── Empty: the one thing to do with no saved looks ──
    let empty_page = GtkBox::new(Orientation::Vertical, 0);
    empty_page.add_css_class("rep-empty");
    let tile = GtkBox::new(Orientation::Vertical, 0);
    tile.add_css_class("rep-empty-tile");
    tile.set_size_request(40, 40);
    tile.set_halign(Align::Center);
    let glyph = DrawingArea::new();
    glyph.add_css_class("rep-empty-glyph");
    glyph.set_content_width(18);
    glyph.set_content_height(18);
    glyph.set_halign(Align::Center);
    // A vertical box only centres a child that may take the spare height, so
    // the glyph expands into the tile and centres itself inside it.
    glyph.set_vexpand(true);
    glyph.set_valign(Align::Center);
    glyph.set_can_target(false);
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
    tile.append(&glyph);
    let empty_title = Label::new(Some(&t("No presets yet")));
    empty_title.add_css_class("rep-empty-title");
    empty_title.set_halign(Align::Center);
    let empty_hint = Label::new(Some(&t(
        "Save this project's background, cursor and click sound, then put them on any recording.",
    )));
    empty_hint.add_css_class("rep-muted");
    empty_hint.add_css_class("rep-empty-hint");
    empty_hint.set_halign(Align::Center);
    empty_hint.set_justify(gtk4::Justification::Center);
    empty_hint.set_wrap(true);
    empty_hint.set_max_width_chars(36);
    let save_look = Button::with_label(&t("Save current look"));
    save_look.add_css_class("rep-primary");
    save_look.set_has_frame(false);
    save_look.set_focus_on_click(false);
    save_look.set_halign(Align::Fill);
    save_look.set_margin_top(16);
    empty_page.append(&tile);
    empty_page.append(&empty_title);
    empty_page.append(&empty_hint);
    empty_page.append(&save_look);

    // ── List: saved looks, then the add row, which stays put while the
    // rows scroll ──
    let list_page = GtkBox::new(Orientation::Vertical, 0);
    let rows = GtkBox::new(Orientation::Vertical, 2);
    rows.add_css_class("rep-rows");
    let scroll = ScrolledWindow::new();
    scroll.add_css_class("rep-scroll");
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(320);
    scroll.set_hexpand(true);
    scroll.set_child(Some(&rows));
    list_page.append(&scroll);

    let divider = Separator::new(Orientation::Horizontal);
    divider.add_css_class("rep-divider");
    list_page.append(&divider);

    let add_row = Button::new();
    add_row.add_css_class("rep-add-row");
    add_row.set_has_frame(false);
    add_row.set_focus_on_click(false);
    let add_content = GtkBox::new(Orientation::Horizontal, 10);
    let add_tile = GtkBox::new(Orientation::Vertical, 0);
    add_tile.add_css_class("rep-add-tile");
    add_tile.set_size_request(50, 32);
    add_tile.set_valign(Align::Center);
    let add_plus = plus_glyph();
    add_plus.add_css_class("rep-add-plus");
    add_plus.set_halign(Align::Center);
    // Same as the empty tile's bookmark: only a child that may take the spare
    // height is centred in a vertical box.
    add_plus.set_vexpand(true);
    add_plus.set_valign(Align::Center);
    add_tile.append(&add_plus);
    let add_label = Label::new(Some(&t("Save current look")));
    add_label.add_css_class("rep-add-label");
    add_label.set_xalign(0.0);
    add_content.append(&add_tile);
    add_content.append(&add_label);
    add_row.set_child(Some(&add_content));
    list_page.append(&add_row);

    let list_status = Label::new(None);
    list_status.add_css_class("rep-list-status");
    list_status.set_xalign(0.0);
    list_status.set_wrap(true);
    list_status.set_max_width_chars(38);
    list_status.set_visible(false);
    list_page.append(&list_status);

    // ── Form: create and rename share it; only the title, the primary label
    // and the default switch differ ──
    let form_page = GtkBox::new(Orientation::Vertical, 0);
    let eyebrow = Label::new(Some(&t("Name").to_uppercase()));
    eyebrow.add_css_class("rep-eyebrow");
    eyebrow.set_xalign(0.0);
    form_page.append(&eyebrow);

    let name = Entry::new();
    name.add_css_class("rep-entry");
    name.set_max_length(120);
    name.set_placeholder_text(Some(&t("Preset name")));
    name.set_margin_top(6);
    form_page.append(&name);

    let form_status = Label::new(None);
    form_status.add_css_class("rep-form-status");
    form_status.set_xalign(0.0);
    form_status.set_wrap(true);
    form_status.set_max_width_chars(38);
    form_status.set_visible(false);
    form_page.append(&form_status);

    let default_card = GtkBox::new(Orientation::Horizontal, 12);
    default_card.add_css_class("rep-toggle-card");
    let default_text = GtkBox::new(Orientation::Vertical, 2);
    default_text.set_hexpand(true);
    default_text.set_valign(Align::Center);
    let default_title = Label::new(Some(&t("Default for new recordings")));
    default_title.add_css_class("rep-toggle-title");
    default_title.set_xalign(0.0);
    let default_sub = Label::new(Some(&t("New recordings open with this look")));
    default_sub.add_css_class("rep-muted");
    default_sub.add_css_class("rep-toggle-sub");
    default_sub.set_xalign(0.0);
    default_sub.set_wrap(true);
    default_text.append(&default_title);
    default_text.append(&default_sub);
    let default_switch = Switch::new();
    default_switch.add_css_class("recording-editor-export-switch");
    default_switch.set_valign(Align::Center);
    default_switch.update_property(&[gtk4::accessible::Property::Label(&t(
        "Default for new recordings",
    ))]);
    default_card.append(&default_text);
    default_card.append(&default_switch);
    form_page.append(&default_card);

    let form_buttons = GtkBox::new(Orientation::Horizontal, 8);
    form_buttons.set_halign(Align::End);
    let cancel = Button::with_label(&t("Cancel"));
    cancel.add_css_class("rep-ghost");
    cancel.set_has_frame(false);
    cancel.set_focus_on_click(false);
    let primary = Button::with_label(&t("Create"));
    primary.add_css_class("rep-primary");
    primary.set_has_frame(false);
    primary.set_focus_on_click(false);
    form_buttons.append(&cancel);
    form_buttons.append(&primary);
    form_page.append(&form_buttons);

    // ── Delete: the confirmation replaces the card, so no row can be
    // clicked while it is up ──
    let delete_page = GtkBox::new(Orientation::Vertical, 0);

    stack.add_named(&empty_page, Some(EMPTY_PAGE));
    stack.add_named(&list_page, Some(LIST_PAGE));
    stack.add_named(&form_page, Some(FORM_PAGE));
    stack.add_named(&delete_page, Some(DELETE_PAGE));
    stack.set_visible_child_name(EMPTY_PAGE);
    body.append(&stack);
    popover.set_child(Some(&body));

    let panel = Panel {
        popover: popover.clone(),
        stack,
        title,
        rows,
        list_status,
        form_status,
        default_card,
        name: name.clone(),
        default_switch,
        primary: primary.clone(),
        delete_page,
        mode: Rc::new(RefCell::new(FormMode::Create)),
        return_page: Rc::new(RefCell::new(EMPTY_PAGE.to_owned())),
        status_serial: Rc::new(Cell::new(0)),
        state,
        ping,
        applied: Rc::new(RefCell::new(None)),
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
            button.add_css_class("recording-editor-presets-open");
            redraw_child(&button);
            panel.open();
        }
    });
    popover.connect_closed({
        let button = button.clone();
        move |_| {
            button.remove_css_class("recording-editor-presets-open");
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
    save_look.connect_clicked({
        let panel = panel.clone();
        move |_| panel.show_form(FormMode::Create, EMPTY_PAGE)
    });
    add_row.connect_clicked({
        let panel = panel.clone();
        move |_| panel.show_form(FormMode::Create, LIST_PAGE)
    });
    cancel.connect_clicked({
        let panel = panel.clone();
        move |_| panel.cancel_form()
    });
    primary.connect_clicked({
        let panel = panel.clone();
        move |_| panel.submit_form()
    });
    name.connect_activate({
        let panel = panel.clone();
        move |_| panel.submit_form()
    });
    // Escape backs out of the form from any control in it, not only the
    // entry, and stops there so the popover itself stays open.
    let form_keys = EventControllerKey::new();
    form_keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
    form_keys.connect_key_pressed({
        let panel = panel.clone();
        move |_, key, _, _| {
            if key != gdk::Key::Escape
                || panel.stack.visible_child_name().as_deref() != Some(FORM_PAGE)
            {
                return glib::Propagation::Proceed;
            }
            panel.cancel_form();
            glib::Propagation::Stop
        }
    });
    form_page.add_controller(form_keys);
}

/// The preset's look as the list draws it: its wallpaper thumbnail, its fill,
/// or a neutral swatch when the preset carries no stage.
enum Thumbnail {
    Image(Pixbuf),
    Surface(gtk4::cairo::ImageSurface),
    Color((u8, u8, u8)),
}

impl Panel {
    fn open(&self) {
        *self.mode.borrow_mut() = FormMode::Create;
        self.clear_list_status();
        self.say_form("", false);
        // An undo, a document swap or a later edit can leave the highlighted
        // preset no longer applied. The popover has no history listener, so it
        // checks the highlight against the open project whenever it opens.
        let applied = self.applied.borrow().clone();
        if let Some(id) = applied {
            let still_applied = presets::load(&id).is_some_and(|preset| {
                let guard = self.state.lock().unwrap();
                presets::look_matches(&guard, &preset)
            });
            if !still_applied {
                *self.applied.borrow_mut() = None;
            }
        }
        self.show_list_or_empty();
    }

    fn show_list_or_empty(&self) {
        if self.fill() {
            self.show_empty();
        } else {
            self.show_list();
        }
    }

    fn show_empty(&self) {
        self.title.set_text(&t("Presets"));
        self.stack.set_visible_child_name(EMPTY_PAGE);
    }

    fn show_list(&self) {
        self.title.set_text(&t("Presets"));
        self.stack.set_visible_child_name(LIST_PAGE);
    }

    /// Switch the card to the create or rename form. Rename pre-fills the
    /// entry with the preset's name and swaps the primary label and the
    /// default switch out; everything else is the same view.
    fn show_form(&self, mode: FormMode, return_page: &str) {
        *self.mode.borrow_mut() = mode.clone();
        *self.return_page.borrow_mut() = return_page.to_owned();
        self.say_form("", false);
        match &mode {
            FormMode::Create => {
                self.name.set_text("");
                self.default_card.set_visible(true);
                self.default_switch.set_active(false);
                self.title.set_text(&t("New preset"));
                self.primary.set_label(&t("Create"));
            }
            FormMode::Rename(id) => {
                let existing = presets::load(id).map(|preset| preset.name);
                self.name.set_text(existing.as_deref().unwrap_or(""));
                self.default_card.set_visible(false);
                self.title.set_text(&t("Rename"));
                self.primary.set_label(&t("Save"));
            }
        }
        self.stack.set_visible_child_name(FORM_PAGE);
        let name = self.name.clone();
        glib::idle_add_local_once(move || {
            name.grab_focus();
            name.select_region(0, -1);
        });
    }

    fn cancel_form(&self) {
        let page = self.return_page.borrow().clone();
        if page == LIST_PAGE {
            self.show_list();
        } else {
            self.show_empty();
        }
        self.name.set_text("");
        self.default_switch.set_active(false);
    }

    /// The delete confirmation takes the whole card, so the row that asked
    /// for it is hidden while it is up.
    fn show_delete(&self, preset: &Preset) {
        while let Some(child) = self.delete_page.first_child() {
            self.delete_page.remove(&child);
        }
        self.title
            .set_text(&tfmt("Delete {name}?", &[("name", &preset.name)]));
        let note = Label::new(Some(&t("This permanently removes the file from disk.")));
        note.add_css_class("rep-muted");
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.set_max_width_chars(38);
        let buttons = GtkBox::new(Orientation::Horizontal, 8);
        buttons.set_halign(Align::End);
        buttons.set_margin_top(16);
        let cancel = Button::with_label(&t("Cancel"));
        cancel.add_css_class("rep-ghost");
        cancel.set_has_frame(false);
        cancel.set_focus_on_click(false);
        cancel.connect_clicked({
            let panel = self.clone();
            move |_| panel.show_list()
        });
        let delete = Button::with_label(&t("Delete"));
        delete.add_css_class("rep-danger");
        delete.set_has_frame(false);
        delete.set_focus_on_click(false);
        delete.connect_clicked({
            let panel = self.clone();
            let id = preset.id.clone();
            let name = preset.name.clone();
            move |_| panel.delete(&id, &name)
        });
        buttons.append(&cancel);
        buttons.append(&delete);
        self.delete_page.append(&note);
        self.delete_page.append(&buttons);
        self.stack.set_visible_child_name(DELETE_PAGE);
    }

    fn say_list(&self, text: &str) {
        self.list_status.remove_css_class("rep-error");
        self.set_list_status(text);
    }

    fn say_list_error(&self, text: &str) {
        self.list_status.add_css_class("rep-error");
        self.set_list_status(text);
    }

    fn set_list_status(&self, text: &str) {
        self.list_status.set_text(text);
        self.list_status.set_visible(!text.is_empty());
        let serial = self.status_serial.get().wrapping_add(1);
        self.status_serial.set(serial);
        if text.is_empty() {
            return;
        }
        // After an action the line is there to read, not to keep: clear it so
        // the list goes back to being just saved looks.
        let panel = self.clone();
        glib::timeout_add_local_once(Duration::from_secs(3), move || {
            if panel.status_serial.get() == serial {
                panel.list_status.set_text("");
                panel.list_status.set_visible(false);
            }
        });
    }

    fn clear_list_status(&self) {
        self.list_status.set_text("");
        self.list_status.set_visible(false);
        self.status_serial
            .set(self.status_serial.get().wrapping_add(1));
    }

    fn say_form(&self, text: &str, error: bool) {
        self.form_status.set_text(text);
        if error {
            self.form_status.add_css_class("rep-error");
        } else {
            self.form_status.remove_css_class("rep-error");
        }
        self.form_status.set_visible(!text.is_empty());
    }

    fn refresh_list(&self) {
        let panel = self.clone();
        glib::idle_add_local_once(move || panel.show_list_or_empty());
    }

    /// Rebuild the rows. Called from an idle callback when a row triggered it,
    /// because the row's own button is among the widgets being replaced.
    /// Returns whether there are no presets left to show.
    fn fill(&self) -> bool {
        while let Some(child) = self.rows.first_child() {
            self.rows.remove(&child);
        }
        let presets = presets::list();
        if presets.is_empty() {
            return true;
        }
        let configured = crate::config::load_config().rec_video_default_preset;
        for preset in &presets {
            let is_default = configured.as_deref() == Some(preset.id.as_str());
            self.rows.append(&self.preset_row(preset, is_default));
        }
        false
    }

    fn preset_row(&self, preset: &Preset, is_default: bool) -> GtkBox {
        let row = GtkBox::new(Orientation::Horizontal, 0);
        row.add_css_class("rep-row-shell");

        let pick = Button::new();
        pick.add_css_class("rep-row");
        if self.applied.borrow().as_deref() == Some(preset.id.as_str()) {
            pick.add_css_class("rep-applied");
        }
        pick.set_has_frame(false);
        pick.set_focus_on_click(false);
        pick.set_hexpand(true);
        pick.set_tooltip_text(Some(&t("Apply to this project")));
        let content = GtkBox::new(Orientation::Horizontal, 10);
        content.append(&thumbnail(preset));
        let name = Label::new(Some(&preset.name));
        name.add_css_class("rep-name");
        name.set_xalign(0.0);
        name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        content.append(&name);
        if is_default {
            let pill = Label::new(Some(&t("Default")));
            pill.add_css_class("rep-pill");
            pill.set_valign(Align::Center);
            content.append(&pill);
        }
        pick.set_child(Some(&content));
        pick.connect_clicked({
            let panel = self.clone();
            let preset = preset.clone();
            move |_| {
                if panel.apply(&preset) {
                    *panel.applied.borrow_mut() = Some(preset.id.clone());
                    panel.popover.popdown();
                }
            }
        });

        row.append(&pick);
        row.append(&self.more_button(preset, is_default));
        row
    }

    fn more_button(&self, preset: &Preset, is_default: bool) -> Button {
        let more = Button::new();
        more.add_css_class("rep-more");
        more.set_has_frame(false);
        more.set_focus_on_click(false);
        more.set_valign(Align::Center);
        more.set_tooltip_text(Some(&t("Preset options")));
        more.set_child(Some(&dots_glyph()));

        let menu = Popover::new();
        menu.add_css_class("recording-editor-clip-menu-popover");
        menu.set_has_arrow(false);
        menu.set_autohide(true);
        menu.set_position(PositionType::Bottom);
        menu.set_halign(Align::End);
        menu.set_offset(0, 4);
        menu.set_parent(&more);

        let items = GtkBox::new(Orientation::Vertical, 2);
        items.add_css_class("recording-editor-clip-menu");
        items.set_size_request(160, -1);
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
            icon_names::custom::CHECK_SYMBOLIC,
            &default_label,
            default_action,
        ));
        items.append(&menu_item(
            &menu,
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
            icon_names::custom::PENCIL_SYMBOLIC,
            &t("Rename"),
            {
                let panel = self.clone();
                let id = preset.id.clone();
                move || panel.show_form(FormMode::Rename(id.clone()), LIST_PAGE)
            },
        ));
        items.append(&menu_item(
            &menu,
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
            icon_names::custom::USER_TRASH_SYMBOLIC,
            &t("Delete"),
            {
                let panel = self.clone();
                let preset = preset.clone();
                move || panel.show_delete(&preset)
            },
        ));
        menu.set_child(Some(&items));

        more.connect_clicked(move |_| menu.popup());
        more
    }

    /// Enter, Create and Save all land here. Which write happens is decided
    /// by the mode the form was opened in.
    fn submit_form(&self) {
        let text = self.name.text().trim().to_owned();
        match self.mode.borrow().clone() {
            FormMode::Create => self.create(&text),
            FormMode::Rename(id) => self.commit_rename(&id, &text),
        }
    }

    fn create(&self, name: &str) {
        if name.is_empty() {
            self.say_form(&t("Name the preset before saving it."), true);
            return;
        }
        let captured = {
            let guard = self.state.lock().unwrap();
            if !guard.has_source_video() {
                Err("Open a readable recording before saving a preset".to_owned())
            } else {
                presets::from_state(&guard, name)
            }
        };
        match captured.and_then(|preset| presets::save(&preset).map(|()| preset)) {
            Ok(preset) => {
                if self.default_switch.is_active() {
                    if let Err(err) = set_default_video_preset(&preset.id) {
                        self.say_list_error(&err);
                        self.refresh_list();
                        return;
                    }
                }
                self.say_list(&tfmt("{name} saved.", &[("name", &preset.name)]));
                self.refresh_list();
            }
            Err(err) => self.say_form(&err, true),
        }
    }

    fn toggle_default(&self, id: &str, is_default: bool) {
        let next = if is_default { "" } else { id };
        if let Err(err) = set_default_video_preset(next) {
            self.say_list_error(&err);
        }
        self.refresh_list();
    }

    fn update(&self, preset: &Preset) {
        let captured = {
            let guard = self.state.lock().unwrap();
            if !guard.has_source_video() {
                Err("Open a readable recording before saving a preset".to_owned())
            } else {
                presets::from_state(&guard, &preset.name)
            }
        };
        match captured
            .map(|captured| presets::replacing(preset, captured))
            .and_then(|preset| presets::save(&preset).map(|()| preset))
        {
            Ok(_) => self.say_list(&tfmt(
                "{name} now holds the current look.",
                &[("name", &preset.name)],
            )),
            Err(err) => self.say_list_error(&err),
        }
        self.refresh_list();
    }

    /// Put the preset on the open edit. Returns whether the edit was there to
    /// take it, so the row only highlights when the look landed.
    fn apply(&self, preset: &Preset) -> bool {
        {
            let mut guard = self.state.lock().unwrap();
            if !guard.has_source_video() {
                return false;
            }
            presets::apply_to_state(&mut guard, preset);
        }
        (self.ping)();
        true
    }

    fn show_in_folder(&self, id: &str) {
        let Some(path) = presets::path_of(id) else {
            self.say_list_error(&t("That preset is no longer on disk."));
            return;
        };
        let directory = path.parent().map(|parent| parent.to_path_buf());
        let opened = directory
            .map(|directory| {
                std::process::Command::new("xdg-open")
                    .arg(directory)
                    .spawn()
            })
            .transpose();
        if opened.is_err() {
            self.say_list_error(&t("Could not open the preset's folder."));
        }
    }

    fn commit_rename(&self, id: &str, text: &str) {
        if text.is_empty() {
            self.say_form(&t("Name the preset before saving it."), true);
            return;
        }
        let Some(mut preset) = presets::load(id) else {
            self.say_form(&t("That preset is no longer on disk."), true);
            return;
        };
        preset.name = text.to_owned();
        match presets::save(&preset) {
            Ok(()) => {
                self.say_list(&tfmt("Renamed to {name}.", &[("name", text)]));
                self.refresh_list();
            }
            Err(err) => self.say_form(&err, true),
        }
    }

    fn delete(&self, id: &str, name: &str) {
        if self.applied.borrow().as_deref() == Some(id) {
            *self.applied.borrow_mut() = None;
        }
        match presets::delete(id).and_then(|()| forget_default(id)) {
            Ok(()) => self.say_list(&tfmt("{name} deleted.", &[("name", name)])),
            Err(err) => self.say_list_error(&err),
        }
        self.refresh_list();
    }
}

fn menu_item(menu: &Popover, icon: &str, label: &str, action: impl Fn() + 'static) -> Button {
    // Every row reads alike, Delete included, like the timeline clip menu;
    // the confirm page is where a delete reads as destructive.
    let button = Button::new();
    button.add_css_class("recording-editor-clip-menu-row");
    button.set_has_frame(false);
    button.set_focus_on_click(false);

    let row = GtkBox::new(Orientation::Horizontal, 10);
    let image = Image::from_icon_name(icon);
    image.set_pixel_size(15);
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

/// The preset's stage as a thumbnail: the bundled wallpaper, the flat colour,
/// the gradient, or a neutral swatch when no stage was captured.
fn thumbnail(preset: &Preset) -> DrawingArea {
    let width = 52;
    let height = 34;
    let preview = if let Some(name) = preset.stage.selection.as_deref() {
        let path =
            crate::capture::editor::window::background_panel::motion_wallpaper_preview_asset_path(
                name,
            );
        Pixbuf::from_file_at_scale(&path, width * 2, height * 2, true)
            .ok()
            .map(Thumbnail::Image)
    } else {
        match &preset.stage.fill {
            Some(StageFill::Plain { r, g, b }) => Some(Thumbnail::Color((*r, *g, *b))),
            Some(StageFill::Gradient {
                stops,
                angle_degrees,
                reversed,
                kind,
            }) => {
                let gradient = crate::recording::editor::model::VideoGradient {
                    kind: (*kind).into(),
                    stops: stops
                        .iter()
                        .map(|stop| {
                            crate::recording::editor::model::GradientStop::rgba(
                                stop.position,
                                stop.r,
                                stop.g,
                                stop.b,
                                stop.a,
                            )
                        })
                        .collect(),
                    angle_degrees: *angle_degrees,
                    reversed: *reversed,
                }
                .normalized();
                let bitmap = render_gradient(
                    &gradient,
                    (width / 2).max(1) as u32,
                    (height / 2).max(1) as u32,
                );
                Some(Thumbnail::Surface(bitmap_to_surface(&bitmap)))
            }
            None => None,
        }
    };
    let area = DrawingArea::new();
    area.set_content_width(width);
    area.set_content_height(height);
    area.set_valign(Align::Center);
    area.set_can_target(false);
    area.set_draw_func(move |_, context, width, height| {
        let (width, height) = (f64::from(width), f64::from(height));
        match &preview {
            Some(Thumbnail::Image(pixbuf)) => draw_pixbuf_thumbnail(context, width, height, pixbuf),
            Some(Thumbnail::Surface(surface)) => {
                draw_surface_thumbnail(context, width, height, surface)
            }
            Some(Thumbnail::Color((r, g, b))) => {
                rounded_rect(context, 0.5, 0.5, width - 1.0, height - 1.0, 6.0);
                context.set_source_rgb(
                    f64::from(*r) / 255.0,
                    f64::from(*g) / 255.0,
                    f64::from(*b) / 255.0,
                );
                let _ = context.fill_preserve();
                context.set_source_rgba(0.0, 0.0, 0.0, 0.28);
                context.set_line_width(1.0);
                let _ = context.stroke();
            }
            None => draw_fallback_thumbnail(context, width, height),
        }
    });
    area
}

fn draw_pixbuf_thumbnail(
    context: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    thumbnail: &Pixbuf,
) {
    rounded_rect(context, 0.0, 0.0, width, height, 6.0);
    context.clip();
    let source_width = f64::from(thumbnail.width().max(1));
    let source_height = f64::from(thumbnail.height().max(1));
    let scale = (width / source_width).max(height / source_height);
    let draw_width = source_width * scale;
    let draw_height = source_height * scale;
    context.translate((width - draw_width) * 0.5, (height - draw_height) * 0.5);
    context.scale(scale, scale);
    context.set_source_pixbuf(thumbnail, 0.0, 0.0);
    let _ = context.paint();
}

fn draw_surface_thumbnail(
    context: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    surface: &gtk4::cairo::ImageSurface,
) {
    rounded_rect(context, 0.0, 0.0, width, height, 6.0);
    context.clip();
    let scale_x = width / f64::from(surface.width().max(1));
    let scale_y = height / f64::from(surface.height().max(1));
    context.scale(scale_x, scale_y);
    let _ = context.set_source_surface(surface, 0.0, 0.0);
    let _ = context.paint();
}

fn draw_fallback_thumbnail(context: &gtk4::cairo::Context, width: f64, height: f64) {
    rounded_rect(context, 0.5, 0.5, width - 1.0, height - 1.0, 6.0);
    context.set_source_rgb(0.18, 0.20, 0.24);
    let _ = context.fill();
}

fn rounded_rect(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    let right = x + width;
    let bottom = y + height;
    let radius = radius.min(width * 0.5).min(height * 0.5);
    context.new_sub_path();
    context.arc(
        right - radius,
        y + radius,
        radius,
        -0.5 * std::f64::consts::PI,
        0.0,
    );
    context.arc(
        right - radius,
        bottom - radius,
        radius,
        0.0,
        0.5 * std::f64::consts::PI,
    );
    context.arc(
        x + radius,
        bottom - radius,
        radius,
        0.5 * std::f64::consts::PI,
        std::f64::consts::PI,
    );
    context.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        1.5 * std::f64::consts::PI,
    );
    context.close_path();
}

/// Clear the default when it names the preset that was just deleted, so a
/// recording never opens on a look that no longer exists.
fn forget_default(id: &str) -> Result<(), String> {
    if crate::config::load_config()
        .rec_video_default_preset
        .as_deref()
        == Some(id)
    {
        set_default_video_preset("")
    } else {
        Ok(())
    }
}

/// Remember a preset as the look every new recording opens on. An empty id
/// goes back to the built-in look, which is what the open falls back to.
fn set_default_video_preset(id: &str) -> Result<(), String> {
    let mut config = crate::config::load_config();
    config.rec_video_default_preset = (!id.is_empty()).then(|| id.to_owned());
    crate::config::save_config(&config)
        .map(|_| ())
        .map_err(|err| err.to_string())
}
