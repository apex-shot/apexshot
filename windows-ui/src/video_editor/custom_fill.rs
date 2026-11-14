//! Custom color and gradient controls for the Windows video editor.
//!
//! The side panel summarizes the current custom fill and opens an anchored
//! editor. Gradient stops open their own color picker, mirroring the shared
//! editor's two-level interaction.

use super::background::{Background, FillSpec, GradientKind, GradientStop};
use crate::i18n::t;
use gtk4::cairo::{LinearGradient, RadialGradient};
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, DrawingArea, Entry, EventControllerFocus, GestureDrag, Image,
    Label, Orientation, Popover, PositionType, ToggleButton,
};
use std::cell::{Cell, RefCell};
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::rc::Rc;

type Color = (u8, u8, u8);
type RebuildSlot = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

const COLOR_RADIUS: f64 = 8.0;
const HANDLE_RADIUS: f64 = 8.0;
const SPECTRUM_BAR_THICKNESS: f64 = 10.0;
/// The clear gap between the stop picker's card and the card it inspects.
const PICKER_CARD_GAP: f64 = 6.0;

#[derive(Clone, PartialEq)]
enum FillValue {
    Empty,
    Plain(Color),
    Gradient(CustomGradient),
}

#[derive(Clone, PartialEq)]
struct CustomGradient {
    kind: GradientKind,
    angle: f64,
    stops: Vec<GradientStop>,
}

impl FillValue {
    /// The serialisable shape the background stores, or `None` when the tab
    /// holds no fill.
    fn to_spec(&self) -> Option<FillSpec> {
        match self {
            FillValue::Empty => None,
            FillValue::Plain(color) => Some(FillSpec::Color {
                rgb: [color.0, color.1, color.2],
            }),
            FillValue::Gradient(gradient) => Some(FillSpec::Gradient {
                kind: gradient.kind,
                stops: gradient.stops.clone(),
                angle: gradient.angle,
            }),
        }
    }

    /// Read a fill the model holds. A picture is not one of this tab's shapes,
    /// so it reads as no custom fill here; the Image tab shows it instead.
    fn from_spec(spec: &FillSpec) -> Self {
        match spec {
            FillSpec::Color { rgb } => FillValue::Plain((rgb[0], rgb[1], rgb[2])),
            FillSpec::Gradient { kind, stops, angle } => FillValue::Gradient(CustomGradient {
                kind: *kind,
                angle: *angle,
                stops: stops.clone(),
            }),
            FillSpec::Image { .. } => FillValue::Empty,
        }
    }
}

impl Default for CustomGradient {
    fn default() -> Self {
        Self {
            kind: GradientKind::Linear,
            angle: 0.0,
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: (255, 103, 13),
                },
                GradientStop {
                    position: 1.0,
                    color: (38, 70, 122),
                },
            ],
        }
    }
}

impl CustomGradient {
    fn normalized(&self) -> Self {
        let mut normalized = self.clone();
        normalized
            .stops
            .sort_by(|left, right| left.position.total_cmp(&right.position));
        for stop in &mut normalized.stops {
            stop.position = stop.position.clamp(0.0, 1.0);
        }
        normalized
    }
}

pub(crate) struct CustomFillPage {
    pub widget: GtkBox,
    pub clear: Rc<dyn Fn()>,
    /// Copy the model's fill into the tab and its popover without publishing.
    pub sync: Rc<dyn Fn()>,
}

pub(crate) fn build_custom_page(
    scale: f64,
    column: &GtkBox,
    background: &Background,
    on_selected: Rc<dyn Fn()>,
) -> CustomFillPage {
    let state = Rc::new(RefCell::new(FillValue::Empty));

    let row = GtkBox::new(Orientation::Horizontal, 10);
    row.add_css_class("recording-editor-bg-custom-row");
    row.add_css_class("video-editor-custom-row");
    row.set_hexpand(true);

    let swatch = DrawingArea::new();
    swatch.add_css_class("recording-editor-bg-custom-swatch");
    swatch.set_content_width((28.0 * scale).round() as i32);
    swatch.set_content_height((28.0 * scale).round() as i32);
    swatch.set_valign(Align::Center);
    swatch.set_can_target(false);

    let label = Label::new(Some(&t("Color")));
    label.set_hexpand(true);
    label.set_xalign(0.0);
    label.set_valign(Align::Center);

    let edit = Button::with_label(&t("Edit"));
    edit.add_css_class("recording-editor-bg-custom-edit");
    edit.set_has_frame(false);
    edit.set_valign(Align::Center);
    edit.set_tooltip_text(Some(&t("Edit custom fill")));

    row.append(&swatch);
    row.append(&label);
    row.append(&edit);

    let repainters = Rc::new(RefCell::new(Vec::<Rc<dyn Fn()>>::new()));
    let refresh_summary = {
        let state = state.clone();
        let swatch = swatch.clone();
        let label = label.clone();
        let row = row.clone();
        Rc::new(move || {
            let value = state.borrow().clone();
            match &value {
                FillValue::Empty => {
                    label.set_text(&t("Color"));
                    row.remove_css_class("active-background-option");
                }
                FillValue::Plain(_) => {
                    label.set_text(&t("Color"));
                    row.add_css_class("active-background-option");
                }
                FillValue::Gradient(_) => {
                    label.set_text(&t("Gradient"));
                    row.add_css_class("active-background-option");
                }
            }
            let draw_value = value.clone();
            swatch.set_draw_func(move |_, context, width, height| {
                draw_fill_chip(context, width, height, &draw_value, 7.0);
            });
            swatch.queue_draw();
        }) as Rc<dyn Fn()>
    };

    // Hand the stage the fill the tab now holds. The model stores the shape
    // itself, so the panel publishes a serialisable value and the background
    // rebuilds the painter from it; this is what makes a drag in the picker
    // live on the stage as well as in the panel.
    let publish_fill: Rc<dyn Fn()> = {
        let background = background.clone();
        let state = state.clone();
        Rc::new(move || {
            // Read the value out before publishing: the background notifies
            // its listeners synchronously, and one of them reads this state
            // again, so the borrow must not still be alive.
            let spec = state.borrow().to_spec();
            background.set_custom_fill(spec);
        })
    };
    let refresh_view: Rc<dyn Fn()> = {
        let refresh_summary = refresh_summary.clone();
        let repainters = repainters.clone();
        Rc::new(move || {
            refresh_summary();
            for repaint in repainters.borrow().iter() {
                repaint();
            }
        })
    };
    let notify: Rc<dyn Fn()> = {
        let publish_fill = publish_fill.clone();
        let refresh_view = refresh_view.clone();
        let on_selected = on_selected.clone();
        Rc::new(move || {
            // Publish before telling the panel a fill is picked: the panel's
            // listener re-reads the model, so the model must already hold the
            // new value when it runs.
            publish_fill();
            on_selected();
            refresh_view();
        })
    };

    let (popover, refresh_popover) =
        build_custom_popover(&edit, column, state.clone(), notify.clone(), scale);
    repainters.borrow_mut().push(refresh_popover);

    // The popover is a view of the model, not its owner: opening it, applying a
    // preset or undoing copies the model's fill into the tab without publishing
    // it back. Publishing only happens from an actual edit.
    let sync: Rc<dyn Fn()> = {
        let state = state.clone();
        let background = background.state();
        let refresh_view = refresh_view.clone();
        Rc::new(move || {
            let next = background
                .fill_spec()
                .map_or(FillValue::Empty, |spec| FillValue::from_spec(&spec));
            let changed = {
                let mut current = state.borrow_mut();
                if *current == next {
                    false
                } else {
                    *current = next;
                    true
                }
            };
            if changed {
                refresh_view();
            }
        })
    };
    sync();
    refresh_view();

    edit.connect_clicked({
        let popover = popover.clone();
        let sync = sync.clone();
        let refresh_view = refresh_view.clone();
        move |_| {
            sync();
            refresh_view();
            popover.popup();
        }
    });

    let clear: Rc<dyn Fn()> = {
        let state = state.clone();
        let publish_fill = publish_fill.clone();
        let refresh_view = refresh_view.clone();
        Rc::new(move || {
            *state.borrow_mut() = FillValue::Empty;
            publish_fill();
            refresh_view();
        })
    };

    let widget = GtkBox::new(Orientation::Vertical, 0);
    widget.set_hexpand(true);
    widget.append(&row);
    CustomFillPage {
        widget,
        clear,
        sync,
    }
}

fn build_custom_popover(
    edit: &Button,
    column: &GtkBox,
    state: Rc<RefCell<FillValue>>,
    notify: Rc<dyn Fn()>,
    scale: f64,
) -> (Popover, Rc<dyn Fn()>) {
    let popover = Popover::new();
    popover.add_css_class("recording-editor-custom-popover");
    popover.set_has_arrow(false);
    popover.set_position(PositionType::Left);
    popover.set_parent(column);
    popover.connect_show({
        let edit = edit.clone();
        let column = column.clone();
        move |popover| {
            if let Some(bounds) = edit.compute_bounds(&column) {
                popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
                    0,
                    (bounds.y() + bounds.height() / 2.0).round() as i32,
                    1,
                    1,
                )));
            }
        }
    });

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("recording-editor-custom-body");
    root.set_size_request((184.0 * scale).round() as i32, -1);

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
    close.connect_clicked({
        let popover = popover.clone();
        move |_| popover.popdown()
    });
    header.append(&title);
    header.append(&close);
    root.append(&header);

    let tabs = GtkBox::new(Orientation::Horizontal, 0);
    tabs.add_css_class("recording-editor-bg-tabs");
    tabs.set_homogeneous(true);
    let color_tab = tab_button(&t("Color"));
    let gradient_tab = tab_button(&t("Gradient"));
    color_tab.set_group(Some(&gradient_tab));
    color_tab.set_active(true);
    tabs.append(&color_tab);
    tabs.append(&gradient_tab);
    root.append(&tabs);

    let get_color = {
        let state = state.clone();
        Rc::new(move || match &*state.borrow() {
            FillValue::Plain(color) => *color,
            // A gradient opens its own tab, but the colour picker still starts
            // from the fill rather than an unrelated placeholder.
            FillValue::Gradient(gradient) => gradient
                .stops
                .first()
                .map_or((17, 17, 17), |stop| stop.color),
            FillValue::Empty => (17, 17, 17),
        }) as Rc<dyn Fn() -> Color>
    };
    let set_color = {
        let state = state.clone();
        let notify = notify.clone();
        Rc::new(move |color| {
            *state.borrow_mut() = FillValue::Plain(color);
            notify();
        }) as Rc<dyn Fn(Color)>
    };
    let color_picker = build_color_picker(get_color, set_color, notify.clone(), scale);

    let gradient_page = build_gradient_page(root.clone(), state.clone(), notify.clone(), scale);

    let pages = gtk4::Stack::new();
    pages.set_transition_type(gtk4::StackTransitionType::Crossfade);
    pages.add_named(&color_picker.widget, Some("color"));
    pages.add_named(&gradient_page.widget, Some("gradient"));
    pages.set_visible_child_name("color");
    root.append(&pages);

    color_tab.connect_toggled({
        let pages = pages.clone();
        move |tab| {
            if tab.is_active() {
                pages.set_visible_child_name("color");
            }
        }
    });
    gradient_tab.connect_toggled({
        let pages = pages.clone();
        move |tab| {
            if tab.is_active() {
                pages.set_visible_child_name("gradient");
            }
        }
    });

    popover.set_child(Some(&root));
    let refresh = {
        let color_repaint = color_picker.repaint;
        let gradient_repaint = gradient_page.repaint;
        let state = state.clone();
        let color_tab = color_tab.clone();
        let gradient_tab = gradient_tab.clone();
        Rc::new(move || {
            // The tab follows the fill the popover is showing, so reopening on
            // a gradient does not present the flat colour picker.
            let showing_gradient = matches!(*state.borrow(), FillValue::Gradient(_));
            if showing_gradient {
                gradient_tab.set_active(true);
            } else {
                color_tab.set_active(true);
            }
            color_repaint();
            gradient_repaint();
        }) as Rc<dyn Fn()>
    };
    (popover, refresh)
}

fn tab_button(label: &str) -> ToggleButton {
    let button = ToggleButton::with_label(label);
    button.add_css_class("recording-editor-bg-tab");
    button.set_has_frame(false);
    button.set_hexpand(true);
    button
}

#[derive(Clone)]
pub(crate) struct ColorPicker {
    pub(crate) widget: GtkBox,
    pub(crate) repaint: Rc<dyn Fn()>,
}

pub(crate) fn build_color_picker(
    get: Rc<dyn Fn() -> Color>,
    set: Rc<dyn Fn(Color)>,
    notify: Rc<dyn Fn()>,
    scale: f64,
) -> ColorPicker {
    let widget = GtkBox::new(Orientation::Vertical, 0);
    widget.set_hexpand(true);

    let field = DrawingArea::new();
    field.add_css_class("recording-editor-custom-field");
    field.set_content_height((132.0 * scale).round() as i32);
    field.set_hexpand(true);
    field.set_cursor(gtk4::gdk::Cursor::from_name("crosshair", None).as_ref());

    let spectrum = DrawingArea::new();
    spectrum.add_css_class("recording-editor-custom-spectrum");
    spectrum.set_content_height((18.0 * scale).round() as i32);
    spectrum.set_hexpand(true);

    let value_row = GtkBox::new(Orientation::Horizontal, 8);
    value_row.add_css_class("recording-editor-custom-value-row");
    let swatch = DrawingArea::new();
    swatch.add_css_class("recording-editor-custom-swatch");
    swatch.set_content_width((18.0 * scale).round() as i32);
    swatch.set_content_height((18.0 * scale).round() as i32);
    swatch.set_valign(Align::Center);
    swatch.set_can_target(false);

    let hex = Entry::new();
    hex.add_css_class("recording-editor-custom-hex");
    hex.set_hexpand(true);
    hex.set_width_chars(7);
    hex.set_max_width_chars(7);
    hex.set_valign(Align::Center);
    let unit = Label::new(Some(&t("rgb")));
    unit.add_css_class("recording-editor-custom-unit");
    unit.set_valign(Align::Center);
    value_row.append(&swatch);
    value_row.append(&hex);
    value_row.append(&unit);

    widget.append(&field);
    widget.append(&spectrum);
    widget.append(&value_row);

    let hsv = Rc::new(Cell::new((0.0, 0.0, 0.0)));
    let synced: Rc<Cell<Option<Color>>> = Rc::new(Cell::new(None));
    let syncing = Rc::new(Cell::new(false));
    let set_hsv: Rc<dyn Fn(f64, f64, f64)> = {
        let hsv = hsv.clone();
        let synced = synced.clone();
        let set = set.clone();
        let notify = notify.clone();
        Rc::new(move |hue, saturation, value| {
            hsv.set((hue, saturation, value));
            let color = hsv_to_rgb(hue, saturation, value);
            synced.set(Some(color));
            set(color);
            notify();
        })
    };
    let adopt_rgb: Rc<dyn Fn(Color)> = {
        let hsv = hsv.clone();
        let synced = synced.clone();
        let set = set.clone();
        let notify = notify.clone();
        Rc::new(move |color| {
            hsv.set(rgb_to_hsv(color));
            synced.set(Some(color));
            set(color);
            notify();
        })
    };

    hex.connect_activate({
        let adopt_rgb = adopt_rgb.clone();
        let syncing = syncing.clone();
        move |entry| {
            if syncing.get() {
                return;
            }
            if let Some(color) = parse_hex(&entry.text()) {
                adopt_rgb(color);
            }
        }
    });
    let focus = EventControllerFocus::new();
    focus.connect_leave({
        let adopt_rgb = adopt_rgb.clone();
        let syncing = syncing.clone();
        move |controller| {
            if syncing.get() {
                return;
            }
            let Some(entry) = controller
                .widget()
                .and_then(|widget| widget.downcast::<Entry>().ok())
            else {
                return;
            };
            if let Some(color) = parse_hex(&entry.text()) {
                adopt_rgb(color);
            }
        }
    });
    hex.add_controller(focus);

    attach_plane_drag(&field, hsv.clone(), set_hsv.clone());
    attach_hue_drag(&spectrum, hsv.clone(), set_hsv);

    let repaint: Rc<dyn Fn()> = {
        let get = get.clone();
        let hsv = hsv.clone();
        let synced = synced.clone();
        let syncing = syncing.clone();
        let field = field.clone();
        let spectrum = spectrum.clone();
        let swatch = swatch.clone();
        let hex = hex.clone();
        Rc::new(move || {
            let color = get();
            let (hue, saturation, value) = reconciled_hsv(hsv.get(), synced.get(), color);
            hsv.set((hue, saturation, value));
            synced.set(Some(color));
            field.set_draw_func(move |_, context, width, height| {
                draw_plane(
                    context,
                    f64::from(width),
                    f64::from(height),
                    (hue, saturation, value),
                );
            });
            field.queue_draw();
            spectrum.set_draw_func(move |_, context, width, height| {
                draw_spectrum(context, f64::from(width), f64::from(height), hue, color);
            });
            spectrum.queue_draw();
            swatch.set_draw_func(move |_, context, width, height| {
                fill_rounded(
                    context,
                    0.0,
                    0.0,
                    f64::from(width),
                    f64::from(height),
                    5.0,
                    color,
                );
            });
            swatch.queue_draw();
            syncing.set(true);
            hex.set_text(&hex_string(color));
            syncing.set(false);
        }) as Rc<dyn Fn()>
    };
    repaint();

    ColorPicker { widget, repaint }
}

struct GradientView {
    widget: GtkBox,
    repaint: Rc<dyn Fn()>,
}

fn build_gradient_page(
    card_body: GtkBox,
    state: Rc<RefCell<FillValue>>,
    notify: Rc<dyn Fn()>,
    scale: f64,
) -> GradientView {
    let widget = GtkBox::new(Orientation::Vertical, 0);
    widget.add_css_class("recording-editor-gradient-editor");
    widget.set_hexpand(true);

    let header = GtkBox::new(Orientation::Horizontal, 5);
    header.add_css_class("recording-editor-gradient-header");
    let type_button = Button::new();
    type_button.add_css_class("recording-editor-dropdown");
    type_button.add_css_class("recording-editor-gradient-type");
    type_button.set_has_frame(false);
    type_button.set_valign(Align::Center);
    type_button.set_size_request((104.0 * scale).round() as i32, -1);
    let type_row = GtkBox::new(Orientation::Horizontal, 6);
    let type_label = Label::new(Some(&gradient_kind_label(GradientKind::Linear)));
    type_label.add_css_class("recording-editor-dropdown-label");
    type_label.set_xalign(0.0);
    type_label.set_hexpand(true);
    let type_arrow = Image::from_icon_name("pan-down-symbolic");
    type_arrow.add_css_class("recording-editor-dropdown-arrow");
    type_arrow.set_pixel_size(10);
    type_row.append(&type_label);
    type_row.append(&type_arrow);
    type_button.set_child(Some(&type_row));

    let type_popover = Popover::new();
    type_popover.add_css_class("recording-editor-dropdown-popover");
    type_popover.add_css_class("gradient-type-menu");
    type_popover.set_has_arrow(false);
    type_popover.set_position(PositionType::Bottom);
    type_popover.set_size_request((104.0 * scale).round() as i32, -1);
    type_popover.set_parent(&type_button);
    let type_list = GtkBox::new(Orientation::Vertical, 0);
    type_list.add_css_class("recording-editor-dropdown-list");
    let kinds = [
        (GradientKind::Linear, t("Linear")),
        (GradientKind::Radial, t("Radial")),
        (GradientKind::Angular, t("Angular")),
        (GradientKind::Diamond, t("Diamond")),
    ];
    let kind_items = Rc::new(RefCell::new(Vec::new()));
    for (kind, label) in kinds {
        let item = Button::new();
        item.add_css_class("recording-editor-dropdown-item");
        item.set_has_frame(false);
        item.set_halign(Align::Fill);
        let text = Label::new(Some(&label));
        text.set_xalign(0.0);
        text.set_hexpand(true);
        item.set_child(Some(&text));
        let state = state.clone();
        let notify = notify.clone();
        let type_label = type_label.clone();
        let type_popover = type_popover.clone();
        item.connect_clicked(move |_| {
            let mut guard = state.borrow_mut();
            let mut gradient = ensure_gradient(&mut guard);
            gradient.kind = kind;
            *guard = FillValue::Gradient(gradient);
            drop(guard);
            type_label.set_text(&gradient_kind_label(kind));
            type_popover.popdown();
            notify();
        });
        kind_items.borrow_mut().push((kind, item.clone()));
        type_list.append(&item);
    }
    type_popover.set_child(Some(&type_list));
    type_button.connect_clicked({
        let type_popover = type_popover.clone();
        move |_| type_popover.popup()
    });

    let flip = gradient_icon_button(GradientIcon::Flip, &t("Reverse gradient"), scale);
    let rotate = gradient_icon_button(GradientIcon::Rotate, &t("Rotate gradient"), scale);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    header.append(&type_button);
    header.append(&spacer);
    header.append(&flip);
    header.append(&rotate);

    let selected = Rc::new(Cell::new(0usize));
    let ramp = DrawingArea::new();
    ramp.add_css_class("recording-editor-gradient-bar");
    ramp.set_content_height((26.0 * scale).round() as i32);
    ramp.set_hexpand(true);

    let steps_header = GtkBox::new(Orientation::Horizontal, 8);
    steps_header.add_css_class("recording-editor-gradient-steps-header");
    let steps_label = Label::new(Some(&t("STEPS")));
    steps_label.set_xalign(0.0);
    steps_label.set_hexpand(true);
    let add = Button::with_label("+");
    add.add_css_class("recording-editor-gradient-add");
    add.set_tooltip_text(Some(&t("Add gradient stop")));
    steps_header.append(&steps_label);
    steps_header.append(&add);

    let stops = GtkBox::new(Orientation::Vertical, 4);
    stops.add_css_class("recording-editor-gradient-steps");
    stops.set_hexpand(true);
    let stop_rows: Rc<RefCell<Vec<(DrawingArea, Entry)>>> = Rc::new(RefCell::new(Vec::new()));
    let stop_pickers: Rc<RefCell<Vec<Popover>>> = Rc::new(RefCell::new(Vec::new()));

    let refresh_ramp = {
        let state = state.clone();
        let ramp = ramp.clone();
        let selected = selected.clone();
        Rc::new(move || {
            let gradient = current_gradient(&state);
            let selected = selected.get();
            ramp.set_draw_func(move |_, context, width, height| {
                draw_stop_bar(
                    context,
                    f64::from(width),
                    f64::from(height),
                    scale,
                    &gradient,
                    selected,
                );
            });
            ramp.queue_draw();
        }) as Rc<dyn Fn()>
    };
    refresh_ramp();

    let rebuild_slot: RebuildSlot = Rc::new(RefCell::new(None));
    let rebuild: Rc<dyn Fn()> = {
        let stops = stops.clone();
        let state = state.clone();
        let notify = notify.clone();
        let refresh_ramp = refresh_ramp.clone();
        let add = add.clone();
        let selected = selected.clone();
        let card_body = card_body.clone();
        let rebuild_slot = rebuild_slot.clone();
        let stop_rows = stop_rows.clone();
        let stop_pickers = stop_pickers.clone();
        Rc::new(move || {
            for picker in stop_pickers.borrow_mut().drain(..) {
                picker.popdown();
                picker.unparent();
            }
            stop_rows.borrow_mut().clear();
            while let Some(child) = stops.first_child() {
                stops.remove(&child);
            }
            let gradient = current_gradient(&state);
            if selected.get() >= gradient.stops.len() {
                selected.set(gradient.stops.len().saturating_sub(1));
            }
            for (index, stop) in gradient.stops.iter().enumerate() {
                let row = GtkBox::new(Orientation::Horizontal, 8);
                row.add_css_class("recording-editor-gradient-step");
                if index == selected.get() {
                    row.add_css_class("selected");
                }
                let swatch = Button::new();
                swatch.add_css_class("recording-editor-gradient-step-swatch");
                swatch.set_has_frame(false);
                swatch
                    .set_size_request((26.0 * scale).round() as i32, (26.0 * scale).round() as i32);
                let chip = DrawingArea::new();
                chip.set_content_width((26.0 * scale).round() as i32);
                chip.set_content_height((26.0 * scale).round() as i32);
                let state_for_chip = state.clone();
                let stop_color = stop.color;
                chip.set_draw_func(move |_, context, width, height| {
                    let color = current_gradient(&state_for_chip)
                        .stops
                        .get(index)
                        .map_or(stop_color, |stop| stop.color);
                    fill_rounded(
                        context,
                        0.5,
                        0.5,
                        f64::from(width) - 1.0,
                        f64::from(height) - 1.0,
                        7.0,
                        color,
                    );
                });
                swatch.set_child(Some(&chip));

                let hex = Entry::new();
                hex.add_css_class("recording-editor-gradient-step-hex");
                hex.set_width_chars(7);
                hex.set_max_width_chars(7);
                hex.set_hexpand(true);
                hex.set_text(&hex_string(stop.color));

                row.append(&swatch);
                row.append(&hex);

                let remove = Button::new();
                remove.add_css_class("recording-editor-gradient-remove");
                remove.set_has_frame(false);
                remove.set_tooltip_text(Some(&t("Remove stop")));
                let remove_icon = Image::from_icon_name("window-close-symbolic");
                remove_icon.set_pixel_size((10.0 * scale).round() as i32);
                remove.set_child(Some(&remove_icon));
                // A gradient is at least two stops, so the last pair keeps no way
                // out and the row gives its width back to the hex field.
                remove.set_visible(gradient.stops.len() > 2);
                remove.connect_clicked({
                    let state = state.clone();
                    let notify = notify.clone();
                    let rebuild_slot = rebuild_slot.clone();
                    let selected = selected.clone();
                    move |_| {
                        {
                            let mut guard = state.borrow_mut();
                            let mut gradient = ensure_gradient(&mut guard);
                            if gradient.stops.len() <= 2 || index >= gradient.stops.len() {
                                return;
                            }
                            gradient.stops.remove(index);
                            let current = selected.get();
                            if index < current {
                                selected.set(current - 1);
                            } else if index == current {
                                selected.set(current.min(gradient.stops.len() - 1));
                            }
                            *guard = FillValue::Gradient(gradient.normalized());
                        }
                        if let Some(rebuild) = rebuild_slot.borrow().clone() {
                            rebuild();
                        }
                        notify();
                    }
                });
                row.append(&remove);
                stops.append(&row);

                let stop_picker =
                    build_stop_picker(&card_body, state.clone(), index, notify.clone(), scale);
                stop_pickers.borrow_mut().push(stop_picker.clone());
                stop_rows.borrow_mut().push((chip, hex.clone()));
                swatch.connect_clicked({
                    let stop_picker = stop_picker.clone();
                    let selected = selected.clone();
                    let row = row.clone();
                    let stops = stops.clone();
                    let notify = notify.clone();
                    move |_| {
                        selected.set(index);
                        let mut child = stops.first_child();
                        while let Some(widget) = child {
                            widget.remove_css_class("selected");
                            child = widget.next_sibling();
                        }
                        row.add_css_class("selected");
                        stop_picker.popup();
                        notify();
                    }
                });

                connect_hex_entry(&hex, state.clone(), index, notify.clone());
            }
            add.set_sensitive(gradient.stops.len() < 8);
            refresh_ramp();
        }) as Rc<dyn Fn()>
    };
    *rebuild_slot.borrow_mut() = Some(rebuild.clone());
    rebuild();
    attach_stop_drag(
        &ramp,
        state.clone(),
        notify.clone(),
        selected.clone(),
        rebuild_slot.clone(),
        scale,
    );

    add.connect_clicked({
        let state = state.clone();
        let notify = notify.clone();
        let rebuild_slot = rebuild_slot.clone();
        let selected = selected.clone();
        move |_| {
            let mut guard = state.borrow_mut();
            let mut gradient = ensure_gradient(&mut guard);
            if gradient.stops.len() >= 8 {
                return;
            }
            let mut widest_gap = -1.0;
            let mut position = 0.5;
            for pair in gradient.stops.windows(2) {
                let gap = pair[1].position - pair[0].position;
                if gap > widest_gap {
                    widest_gap = gap;
                    position = (pair[0].position + pair[1].position) * 0.5;
                }
            }
            let color = sample_gradient_color(&gradient, position);
            gradient.stops.push(GradientStop { position, color });
            let gradient = gradient.normalized();
            selected.set(
                gradient
                    .stops
                    .iter()
                    .position(|stop| (stop.position - position).abs() < 1e-6)
                    .unwrap_or(0),
            );
            *guard = FillValue::Gradient(gradient);
            drop(guard);
            if let Some(rebuild) = rebuild_slot.borrow().clone() {
                rebuild();
            }
            notify();
        }
    });

    flip.connect_clicked({
        let state = state.clone();
        let notify = notify.clone();
        let selected = selected.clone();
        let rebuild_slot = rebuild_slot.clone();
        move |_| {
            let mut guard = state.borrow_mut();
            let mut gradient = ensure_gradient(&mut guard);
            for stop in &mut gradient.stops {
                stop.position = 1.0 - stop.position;
            }
            selected.set(
                gradient
                    .stops
                    .len()
                    .saturating_sub(1)
                    .saturating_sub(selected.get()),
            );
            *guard = FillValue::Gradient(gradient.normalized());
            drop(guard);
            if let Some(rebuild) = rebuild_slot.borrow().clone() {
                rebuild();
            }
            notify();
        }
    });
    rotate.connect_clicked({
        let state = state.clone();
        let notify = notify.clone();
        move |_| {
            let mut guard = state.borrow_mut();
            let mut gradient = ensure_gradient(&mut guard);
            gradient.angle = (gradient.angle + 90.0) % 360.0;
            *guard = FillValue::Gradient(gradient);
            drop(guard);
            notify();
        }
    });

    widget.append(&header);
    widget.append(&ramp);
    widget.append(&steps_header);
    widget.append(&stops);

    let repaint = {
        let refresh_ramp = refresh_ramp.clone();
        let state = state.clone();
        let rebuild = rebuild.clone();
        Rc::new(move || {
            let gradient = current_gradient(&state);
            if stop_rows.borrow().len() != gradient.stops.len() {
                rebuild();
            }
            type_label.set_text(&gradient_kind_label(gradient.kind));
            rotate.set_sensitive(matches!(
                gradient.kind,
                GradientKind::Linear | GradientKind::Angular
            ));
            for (kind, item) in kind_items.borrow().iter() {
                if *kind == gradient.kind {
                    item.add_css_class("selected");
                } else {
                    item.remove_css_class("selected");
                }
            }
            for ((chip, hex), stop) in stop_rows.borrow().iter().zip(&gradient.stops) {
                chip.queue_draw();
                if !hex.has_focus() {
                    hex.set_text(&hex_string(stop.color));
                }
            }
            refresh_ramp();
        }) as Rc<dyn Fn()>
    };
    GradientView { widget, repaint }
}

fn build_stop_picker(
    card_body: &GtkBox,
    state: Rc<RefCell<FillValue>>,
    index: usize,
    notify: Rc<dyn Fn()>,
    scale: f64,
) -> Popover {
    let popover = Popover::new();
    popover.add_css_class("recording-editor-gradient-picker-popover");
    popover.set_has_arrow(false);
    popover.set_position(PositionType::Left);
    // GTK seats a left popover's corner on the pointing rect's own corner
    // rather than on the edge's centre, so START is what keeps the two cards'
    // tops level instead of parking this one half its height higher.
    popover.set_valign(Align::Start);
    popover.set_parent(card_body);
    popover.connect_show({
        let gap = (PICKER_CARD_GAP * scale).round() as i32;
        move |popover| {
            popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(-gap, 0, 1, 1)));
        }
    });

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("recording-editor-gradient-picker-card");
    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.add_css_class("recording-editor-gradient-picker-header");
    let title = Label::new(Some(&t("Color")));
    title.add_css_class("recording-editor-custom-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    let close = Button::new();
    close.add_css_class("recording-editor-gradient-picker-close");
    close.set_has_frame(false);
    let close_icon = Image::from_icon_name("window-close-symbolic");
    close_icon.set_pixel_size(12);
    close.set_child(Some(&close_icon));
    close.connect_clicked({
        let popover = popover.clone();
        move |_| popover.popdown()
    });
    header.append(&title);
    header.append(&close);
    body.append(&header);

    let get = {
        let state = state.clone();
        Rc::new(move || {
            current_gradient(&state)
                .stops
                .get(index)
                .map_or((255, 255, 255), |stop| stop.color)
        }) as Rc<dyn Fn() -> Color>
    };
    // This card owns the only copy of its picker, so nothing else repaints it:
    // the parent's refresh list holds the Color page's picker and the ramp, not
    // this one. Without a slot holding its own repaint, a drag inside the card
    // updated the gradient while the handle and the hex field stayed where they
    // were until the card was closed and reopened.
    let repaint_slot: RebuildSlot = Rc::new(RefCell::new(None));
    let notify_repainting: Rc<dyn Fn()> = {
        let slot = repaint_slot.clone();
        let notify = notify.clone();
        Rc::new(move || {
            if let Some(repaint) = slot.borrow().clone() {
                repaint();
            }
            notify();
        })
    };
    let set = {
        let state = state.clone();
        let notify = notify_repainting.clone();
        Rc::new(move |color| {
            let mut guard = state.borrow_mut();
            let mut gradient = ensure_gradient(&mut guard);
            if let Some(stop) = gradient.stops.get_mut(index) {
                stop.color = color;
            }
            *guard = FillValue::Gradient(gradient);
            drop(guard);
            notify();
        }) as Rc<dyn Fn(Color)>
    };
    let picker = build_color_picker(get, set, notify_repainting, scale);
    *repaint_slot.borrow_mut() = Some(picker.repaint.clone());
    body.append(&picker.widget);
    popover.set_child(Some(&body));
    popover.connect_show(move |_| (picker.repaint)());
    popover
}

#[derive(Clone, Copy)]
enum GradientIcon {
    Flip,
    Rotate,
}

#[derive(Clone, Copy)]
enum IconSegment {
    Move(f64, f64),
    Line(f64, f64),
    Corner {
        cx: f64,
        cy: f64,
        radius: f64,
        from: f64,
        to: f64,
    },
}

fn gradient_icon_button(icon: GradientIcon, tooltip: &str, scale: f64) -> Button {
    let button = Button::new();
    button.add_css_class("recording-editor-gradient-icon");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(tooltip));
    button.set_size_request((26.0 * scale).round() as i32, (26.0 * scale).round() as i32);
    let glyph = DrawingArea::new();
    glyph.set_content_width((16.0 * scale).round() as i32);
    glyph.set_content_height((16.0 * scale).round() as i32);
    glyph.set_can_target(false);
    glyph.set_draw_func(move |glyph, context, width, height| {
        draw_gradient_icon(
            context,
            f64::from(width),
            f64::from(height),
            icon,
            glyph.style_context().color(),
        );
    });
    button.set_child(Some(&glyph));
    button
}

fn draw_gradient_icon(
    context: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    icon: GradientIcon,
    color: gtk4::gdk::RGBA,
) {
    let size = width.min(height).max(1.0);
    context.save().ok();
    context.translate((width - size) * 0.5, (height - size) * 0.5);
    context.scale(size / 24.0, size / 24.0);
    context.set_line_width(2.0);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    context.set_source_rgba(
        color.red().into(),
        color.green().into(),
        color.blue().into(),
        color.alpha().into(),
    );
    for segment in gradient_icon_path(icon) {
        match *segment {
            IconSegment::Move(x, y) => context.move_to(x, y),
            IconSegment::Line(x, y) => context.line_to(x, y),
            IconSegment::Corner {
                cx,
                cy,
                radius,
                from,
                to,
            } => context.arc_negative(cx, cy, radius, from, to),
        }
    }
    context.stroke().ok();
    context.restore().ok();
}

fn gradient_icon_path(icon: GradientIcon) -> &'static [IconSegment] {
    const FLIP: [IconSegment; 10] = [
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
    ];
    const ROTATE: [IconSegment; 15] = [
        IconSegment::Move(12.0, 5.0),
        IconSegment::Line(6.0, 5.0),
        IconSegment::Corner {
            cx: 6.0,
            cy: 7.0,
            radius: 2.0,
            from: -FRAC_PI_2,
            to: -PI,
        },
        IconSegment::Line(4.0, 10.0),
        IconSegment::Move(4.0, 14.0),
        IconSegment::Line(4.0, 18.0),
        IconSegment::Corner {
            cx: 6.0,
            cy: 18.0,
            radius: 2.0,
            from: PI,
            to: FRAC_PI_2,
        },
        IconSegment::Line(18.0, 20.0),
        IconSegment::Corner {
            cx: 18.0,
            cy: 18.0,
            radius: 2.0,
            from: FRAC_PI_2,
            to: 0.0,
        },
        IconSegment::Line(20.0, 7.0),
        IconSegment::Corner {
            cx: 18.0,
            cy: 7.0,
            radius: 2.0,
            from: 0.0,
            to: -FRAC_PI_2,
        },
        IconSegment::Line(16.0, 5.0),
        IconSegment::Move(9.0, 8.0),
        IconSegment::Line(12.0, 5.0),
        IconSegment::Line(9.0, 2.0),
    ];
    match icon {
        GradientIcon::Flip => &FLIP,
        GradientIcon::Rotate => &ROTATE,
    }
}

fn gradient_kind_label(kind: GradientKind) -> String {
    match kind {
        GradientKind::Linear => t("Linear"),
        GradientKind::Radial => t("Radial"),
        GradientKind::Angular => t("Angular"),
        GradientKind::Diamond => t("Diamond"),
    }
}

fn ensure_gradient(value: &mut FillValue) -> CustomGradient {
    match value {
        FillValue::Gradient(gradient) => gradient.clone(),
        FillValue::Plain(color) => CustomGradient {
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: *color,
                },
                GradientStop {
                    position: 1.0,
                    color: *color,
                },
            ],
            ..CustomGradient::default()
        },
        FillValue::Empty => CustomGradient::default(),
    }
}

fn current_gradient(state: &Rc<RefCell<FillValue>>) -> CustomGradient {
    ensure_gradient(&mut state.borrow_mut()).normalized()
}

fn connect_hex_entry(
    entry: &Entry,
    state: Rc<RefCell<FillValue>>,
    index: usize,
    notify: Rc<dyn Fn()>,
) {
    let current = {
        let state = state.clone();
        Rc::new(move || {
            current_gradient(&state)
                .stops
                .get(index)
                .map_or((255, 255, 255), |stop| stop.color)
        })
    };
    let apply = Rc::new(move |text: &str| {
        let Some(color) = parse_hex(text) else {
            return;
        };
        let mut guard = state.borrow_mut();
        let mut gradient = ensure_gradient(&mut guard);
        if let Some(stop) = gradient.stops.get_mut(index) {
            stop.color = color;
        }
        *guard = FillValue::Gradient(gradient);
        drop(guard);
        notify();
    });
    entry.connect_activate({
        let apply = apply.clone();
        let current = current.clone();
        move |entry| {
            apply(&entry.text());
            entry.set_text(&hex_string(current()));
        }
    });
    let focus = EventControllerFocus::new();
    focus.connect_leave({
        let apply = apply.clone();
        let current = current.clone();
        move |controller| {
            let Some(entry) = controller
                .widget()
                .and_then(|widget| widget.downcast::<Entry>().ok())
            else {
                return;
            };
            apply(&entry.text());
            entry.set_text(&hex_string(current()));
        }
    });
    entry.add_controller(focus);
}

fn sample_gradient_color(gradient: &CustomGradient, position: f64) -> Color {
    let gradient = gradient.normalized();
    sample_gradient_stops(&gradient.stops, position)
}

fn sample_gradient_stops(stops: &[GradientStop], position: f64) -> Color {
    let Some(first) = stops.first() else {
        return (255, 255, 255);
    };
    if position <= first.position {
        return first.color;
    }
    for pair in stops.windows(2) {
        if position <= pair[1].position {
            let span = (pair[1].position - pair[0].position).max(1e-9);
            let amount = ((position - pair[0].position) / span).clamp(0.0, 1.0);
            return (
                (f64::from(pair[0].color.0)
                    + (f64::from(pair[1].color.0) - f64::from(pair[0].color.0)) * amount)
                    .round() as u8,
                (f64::from(pair[0].color.1)
                    + (f64::from(pair[1].color.1) - f64::from(pair[0].color.1)) * amount)
                    .round() as u8,
                (f64::from(pair[0].color.2)
                    + (f64::from(pair[1].color.2) - f64::from(pair[0].color.2)) * amount)
                    .round() as u8,
            );
        }
    }
    stops.last().map_or((255, 255, 255), |stop| stop.color)
}

fn draw_stop_bar(
    context: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    scale: f64,
    gradient: &CustomGradient,
    selected: usize,
) {
    if width < 2.0 || height < 2.0 {
        return;
    }
    let track_height = (14.0 * scale).min(height);
    let track_top = (height - track_height) * 0.5;
    context.save().ok();
    context.translate(0.0, track_top);
    rounded_rect(context, 0.0, 0.0, width, track_height, track_height * 0.5);
    context.clip();
    let pattern = LinearGradient::new(
        gradient_pin_x(0.0, width, scale),
        0.0,
        gradient_pin_x(1.0, width, scale),
        0.0,
    );
    pattern.set_extend(gtk4::cairo::Extend::Pad);
    for stop in &gradient.normalized().stops {
        let (r, g, b) = stop_color(stop.color);
        pattern.add_color_stop_rgb(stop.position, r, g, b);
    }
    context.set_source(&pattern).ok();
    context.paint().ok();
    context.restore().ok();

    for (index, stop) in gradient.normalized().stops.iter().enumerate() {
        let x = gradient_pin_x(stop.position, width, scale);
        draw_stop_pin(
            context,
            x,
            height * 0.5,
            scale,
            stop.color,
            index == selected,
        );
    }
}

fn gradient_pin_x(position: f64, width: f64, scale: f64) -> f64 {
    let inset = (20.0 * 0.5 + 3.0 + 1.0) * scale;
    inset + position.clamp(0.0, 1.0) * (width - inset * 2.0).max(1.0)
}

fn gradient_position_at(x: f64, width: f64, scale: f64) -> f64 {
    let inset = (20.0 * 0.5 + 3.0 + 1.0) * scale;
    ((x - inset) / (width - inset * 2.0).max(1.0)).clamp(0.0, 1.0)
}

fn draw_stop_pin(
    context: &gtk4::cairo::Context,
    center_x: f64,
    center_y: f64,
    scale: f64,
    color: Color,
    selected: bool,
) {
    let size = 20.0 * scale;
    let left = center_x - size * 0.5;
    let top = center_y - size * 0.5;
    if selected {
        rounded_rect(
            context,
            left - 3.0 * scale,
            top - 3.0 * scale,
            size + 6.0 * scale,
            size + 6.0 * scale,
            9.0 * scale,
        );
        context.set_source_rgba(1.0, 1.0, 1.0, 0.95);
        context.set_line_width(2.0 * scale);
        context.stroke().ok();
    }
    fill_rounded(context, left, top, size, size, 6.0 * scale, color);
    rounded_rect(context, left, top, size, size, 6.0 * scale);
    let luminance =
        (0.2126 * f64::from(color.0) + 0.7152 * f64::from(color.1) + 0.0722 * f64::from(color.2))
            / 255.0;
    if luminance > 0.6 {
        context.set_source_rgb(0.09, 0.09, 0.11);
    } else {
        context.set_source_rgb(0.97, 0.97, 0.97);
    }
    context.set_line_width(1.5 * scale);
    context.stroke().ok();
}

fn attach_stop_drag(
    bar: &DrawingArea,
    state: Rc<RefCell<FillValue>>,
    notify: Rc<dyn Fn()>,
    selected: Rc<Cell<usize>>,
    rebuild_slot: RebuildSlot,
    scale: f64,
) {
    let dragging = Rc::new(Cell::new(usize::MAX));
    let grab_offset = Rc::new(Cell::new(0.0));
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.set_propagation_limit(gtk4::PropagationLimit::None);
    drag.connect_drag_begin({
        let state = state.clone();
        let dragging = dragging.clone();
        let selected = selected.clone();
        let grab_offset = grab_offset.clone();
        move |gesture, x, _| {
            let Some(widget) = gesture.widget() else {
                return;
            };
            let width = f64::from(widget.width().max(1));
            let gradient = current_gradient(&state);
            let mut best = usize::MAX;
            let mut best_distance = f64::MAX;
            let mut best_x = 0.0;
            for (index, stop) in gradient.stops.iter().enumerate() {
                let pin_x = gradient_pin_x(stop.position, width, scale);
                let distance = (pin_x - x).abs();
                if distance < best_distance {
                    best = index;
                    best_distance = distance;
                    best_x = pin_x;
                }
            }
            if best_distance <= 22.0 * scale {
                dragging.set(best);
                selected.set(best);
                grab_offset.set(x - best_x);
                widget.queue_draw();
            }
        }
    });
    drag.connect_drag_update({
        let state = state.clone();
        let dragging = dragging.clone();
        let grab_offset = grab_offset.clone();
        let notify = notify.clone();
        move |gesture, offset_x, _| {
            let index = dragging.get();
            if index == usize::MAX {
                return;
            }
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let Some(widget) = gesture.widget() else {
                return;
            };
            let width = f64::from(widget.width().max(1));
            let position =
                gradient_position_at(start_x + offset_x - grab_offset.get(), width, scale);
            let mut guard = state.borrow_mut();
            let mut gradient = ensure_gradient(&mut guard);
            if index >= gradient.stops.len() {
                return;
            }
            let lower = if index == 0 {
                0.0
            } else {
                gradient.stops[index - 1].position + 0.001
            };
            let upper = if index + 1 >= gradient.stops.len() {
                1.0
            } else {
                gradient.stops[index + 1].position - 0.001
            };
            gradient.stops[index].position = position.clamp(lower.min(upper), upper.max(lower));
            *guard = FillValue::Gradient(gradient);
            drop(guard);
            notify();
        }
    });
    drag.connect_drag_end({
        let dragging = dragging.clone();
        let notify = notify.clone();
        let rebuild_slot = rebuild_slot.clone();
        move |_, _, _| {
            dragging.set(usize::MAX);
            if let Some(rebuild) = rebuild_slot.borrow().clone() {
                rebuild();
            }
            notify();
        }
    });
    bar.add_controller(drag);
}

/// Paint the chosen fill over a whole surface: the stage behind the video.
///
/// Unlike [`draw_fill_chip`] this is full bleed — no half-pixel inset and no
/// corner radius — because the fill is the canvas there, and an inset edge would
/// leave a hairline of stage showing around it.
fn paint_fill(context: &gtk4::cairo::Context, width: i32, height: i32, value: &FillValue) {
    match value {
        FillValue::Empty => {}
        FillValue::Plain(color) => fill_rounded(
            context,
            0.0,
            0.0,
            f64::from(width),
            f64::from(height),
            0.0,
            *color,
        ),
        FillValue::Gradient(gradient) => {
            draw_gradient(context, f64::from(width), f64::from(height), gradient, 0.0);
        }
    }
}

/// Paint a fill in the serialisable shape the background stores. The stage
/// calls this through its painter; the panel's own previews keep using
/// [`paint_fill`] with the local value.
pub(crate) fn paint_spec(context: &gtk4::cairo::Context, width: i32, height: i32, spec: &FillSpec) {
    paint_fill(context, width, height, &FillValue::from_spec(spec));
}

fn draw_fill_chip(
    context: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    value: &FillValue,
    radius: f64,
) {
    match value {
        FillValue::Empty => {}
        FillValue::Plain(color) => fill_rounded(
            context,
            0.5,
            0.5,
            f64::from(width) - 1.0,
            f64::from(height) - 1.0,
            radius,
            *color,
        ),
        FillValue::Gradient(gradient) => draw_gradient(
            context,
            f64::from(width),
            f64::from(height),
            gradient,
            radius,
        ),
    }
}

fn draw_gradient(
    context: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    gradient: &CustomGradient,
    radius: f64,
) {
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let gradient = gradient.normalized();
    context.save().ok();
    rounded_rect(context, 0.0, 0.0, width, height, radius);
    context.clip();
    let start_end = gradient_endpoints(width, height, gradient.angle);
    match gradient.kind {
        GradientKind::Radial => {
            let pattern = RadialGradient::new(
                width * 0.5,
                height * 0.5,
                0.0,
                width * 0.5,
                height * 0.5,
                (width * 0.5).hypot(height * 0.5),
            );
            pattern.set_extend(gtk4::cairo::Extend::Pad);
            for stop in &gradient.stops {
                let color = stop_color(stop.color);
                pattern.add_color_stop_rgb(stop.position, color.0, color.1, color.2);
            }
            context.set_source(&pattern).ok();
        }
        GradientKind::Linear => {
            let pattern = LinearGradient::new(start_end.0, start_end.1, start_end.2, start_end.3);
            pattern.set_extend(gtk4::cairo::Extend::Pad);
            for stop in &gradient.stops {
                let color = stop_color(stop.color);
                pattern.add_color_stop_rgb(stop.position, color.0, color.1, color.2);
            }
            context.set_source(&pattern).ok();
        }
        GradientKind::Angular | GradientKind::Diamond => {
            let Some(surface) = raster_gradient(width as i32, height as i32, &gradient) else {
                context.restore().ok();
                return;
            };
            context.set_source_surface(&surface, 0.0, 0.0).ok();
        }
    }
    context.rectangle(0.0, 0.0, width, height);
    context.fill().ok();
    context.restore().ok();
}

fn raster_gradient(
    width: i32,
    height: i32,
    gradient: &CustomGradient,
) -> Option<gtk4::cairo::ImageSurface> {
    let mut surface =
        gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, width.max(1), height.max(1))
            .ok()?;
    let stride = surface.stride() as usize;
    let (cx, cy) = (f64::from(width) * 0.5, f64::from(height) * 0.5);
    {
        let mut data = surface.data().ok()?;
        for y in 0..height as usize {
            for x in 0..width as usize {
                let dx = x as f64 + 0.5 - cx;
                let dy = y as f64 + 0.5 - cy;
                let position = match gradient.kind {
                    GradientKind::Angular => {
                        (dy.atan2(dx) - gradient.angle.to_radians()).rem_euclid(TAU) / TAU
                    }
                    GradientKind::Diamond => (dx.abs() / cx).max(dy.abs() / cy).min(1.0),
                    _ => return None,
                };
                let (r, g, b) = sample_gradient_stops(&gradient.stops, position);
                let pixel =
                    (0xff00_0000 | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
                        .to_ne_bytes();
                let offset = y * stride + x * 4;
                data[offset..offset + 4].copy_from_slice(&pixel);
            }
        }
    }
    surface.mark_dirty();
    Some(surface)
}

fn gradient_endpoints(width: f64, height: f64, angle: f64) -> (f64, f64, f64, f64) {
    let radians = angle.to_radians();
    let dx = radians.cos() * width * 0.5;
    let dy = radians.sin() * height * 0.5;
    (
        width * 0.5 - dx,
        height * 0.5 - dy,
        width * 0.5 + dx,
        height * 0.5 + dy,
    )
}

fn stop_color(color: Color) -> (f64, f64, f64) {
    (
        f64::from(color.0) / 255.0,
        f64::from(color.1) / 255.0,
        f64::from(color.2) / 255.0,
    )
}

fn fill_rounded(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
    color: Color,
) {
    rounded_rect(context, x, y, width, height, radius);
    context.set_source_rgb(
        f64::from(color.0) / 255.0,
        f64::from(color.1) / 255.0,
        f64::from(color.2) / 255.0,
    );
    context.fill().ok();
}

fn rounded_rect(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    let radius = radius.min(width * 0.5).min(height * 0.5).max(0.0);
    context.new_sub_path();
    context.arc(x + width - radius, y + radius, radius, -FRAC_PI_2, 0.0);
    context.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        FRAC_PI_2,
    );
    context.arc(x + radius, y + height - radius, radius, FRAC_PI_2, PI);
    context.arc(x + radius, y + radius, radius, PI, PI + FRAC_PI_2);
    context.close_path();
}

fn draw_plane(context: &gtk4::cairo::Context, width: f64, height: f64, hsv: (f64, f64, f64)) {
    if width < 2.0 || height < 2.0 {
        return;
    }
    let (hue, saturation, value) = hsv;
    context.save().ok();
    rounded_rect(context, 0.0, 0.0, width, height, COLOR_RADIUS);
    context.clip();
    let base = hsv_to_rgb(hue, 1.0, 1.0);
    context.set_source_rgb(
        f64::from(base.0) / 255.0,
        f64::from(base.1) / 255.0,
        f64::from(base.2) / 255.0,
    );
    context.rectangle(0.0, 0.0, width, height);
    context.fill().ok();
    let white = LinearGradient::new(0.0, 0.0, width, 0.0);
    white.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.0);
    white.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 1.0);
    context.set_source(&white).ok();
    context.rectangle(0.0, 0.0, width, height);
    context.fill().ok();
    let shade = LinearGradient::new(0.0, 0.0, 0.0, height);
    shade.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
    shade.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 1.0);
    context.set_source(&shade).ok();
    context.rectangle(0.0, 0.0, width, height);
    context.fill().ok();
    context.restore().ok();

    let radius = 7.0;
    let x =
        ((1.0 - saturation.clamp(0.0, 1.0)) * width).clamp(radius, (width - radius).max(radius));
    let y = ((1.0 - value.clamp(0.0, 1.0)) * height).clamp(radius, (height - radius).max(radius));
    context.set_source_rgba(0.0, 0.0, 0.0, 0.35);
    context.set_line_width(1.0);
    context.arc(x, y, radius + 0.5, 0.0, TAU);
    context.stroke().ok();
    context.set_source_rgb(1.0, 1.0, 1.0);
    context.arc(x, y, radius, 0.0, TAU);
    context.fill().ok();
    let color = hsv_to_rgb(hue, saturation, value);
    context.set_source_rgb(
        f64::from(color.0) / 255.0,
        f64::from(color.1) / 255.0,
        f64::from(color.2) / 255.0,
    );
    context.arc(x, y, 5.0, 0.0, TAU);
    context.fill().ok();
}

fn draw_spectrum(context: &gtk4::cairo::Context, width: f64, height: f64, hue: f64, color: Color) {
    if width < 2.0 || height < 2.0 {
        return;
    }
    let bar_height = SPECTRUM_BAR_THICKNESS.min(height);
    let bar_y = (height - bar_height) * 0.5;
    context.save().ok();
    rounded_rect(context, 0.0, bar_y, width, bar_height, bar_height * 0.5);
    context.clip();
    for x in 0..width as i32 {
        let swatch = hsv_to_rgb(x as f64 / width, 1.0, 1.0);
        context.set_source_rgb(
            f64::from(swatch.0) / 255.0,
            f64::from(swatch.1) / 255.0,
            f64::from(swatch.2) / 255.0,
        );
        context.rectangle(f64::from(x), bar_y, 1.0, bar_height);
        context.fill().ok();
    }
    context.restore().ok();

    let x = (hue * width).clamp(HANDLE_RADIUS, (width - HANDLE_RADIUS).max(HANDLE_RADIUS));
    context.set_source_rgb(1.0, 1.0, 1.0);
    context.arc(x, height * 0.5, HANDLE_RADIUS, 0.0, TAU);
    context.fill().ok();
    context.set_source_rgb(
        f64::from(color.0) / 255.0,
        f64::from(color.1) / 255.0,
        f64::from(color.2) / 255.0,
    );
    context.arc(x, height * 0.5, HANDLE_RADIUS - 2.5, 0.0, TAU);
    context.fill().ok();
}

fn attach_plane_drag(
    field: &DrawingArea,
    hsv: Rc<Cell<(f64, f64, f64)>>,
    set_hsv: Rc<dyn Fn(f64, f64, f64)>,
) {
    let drag = GestureDrag::new();
    drag.set_button(1);
    let apply = {
        let hsv = hsv.clone();
        let set_hsv = set_hsv.clone();
        move |gesture: &GestureDrag, x: f64, y: f64| {
            let Some(widget) = gesture.widget() else {
                return;
            };
            let width = f64::from(widget.width().max(1));
            let height = f64::from(widget.height().max(1));
            let (hue, _, _) = hsv.get();
            set_hsv(
                hue,
                (1.0 - x / width).clamp(0.0, 1.0),
                (1.0 - y / height).clamp(0.0, 1.0),
            );
        }
    };
    let begin = apply.clone();
    drag.connect_drag_begin(move |gesture, x, y| begin(gesture, x, y));
    let update = apply;
    drag.connect_drag_update(move |gesture, dx, dy| {
        if let Some(start) = gesture.start_point() {
            update(gesture, start.0 + dx, start.1 + dy);
        }
    });
    field.add_controller(drag);
}

fn attach_hue_drag(
    spectrum: &DrawingArea,
    hsv: Rc<Cell<(f64, f64, f64)>>,
    set_hsv: Rc<dyn Fn(f64, f64, f64)>,
) {
    let drag = GestureDrag::new();
    drag.set_button(1);
    let apply = {
        let hsv = hsv.clone();
        let set_hsv = set_hsv.clone();
        move |gesture: &GestureDrag, x: f64| {
            let Some(widget) = gesture.widget() else {
                return;
            };
            let hue = (x / f64::from(widget.width().max(1))).clamp(0.0, 1.0);
            let (_, saturation, value) = hsv.get();
            let (saturation, value) = if saturation < 0.02 {
                (1.0, if value < 0.2 { 1.0 } else { value })
            } else {
                (saturation, value)
            };
            set_hsv(hue, saturation, value);
        }
    };
    let begin = apply.clone();
    drag.connect_drag_begin(move |gesture, x, _| begin(gesture, x));
    let update = apply;
    drag.connect_drag_update(move |gesture, dx, _| {
        if let Some(start) = gesture.start_point() {
            update(gesture, start.0 + dx);
        }
    });
    spectrum.add_controller(drag);
}

fn reconciled_hsv(
    held: (f64, f64, f64),
    last_written: Option<Color>,
    color: Color,
) -> (f64, f64, f64) {
    if last_written == Some(color) {
        held
    } else {
        rgb_to_hsv(color)
    }
}

fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> Color {
    let h = hue.rem_euclid(1.0) * 6.0;
    let sector = h.floor();
    let f = h - sector;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * f);
    let t = value * (1.0 - saturation * (1.0 - f));
    let (r, g, b) = match sector as i32 {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    };
    let to_u8 = |channel: f64| (channel * 255.0).round().clamp(0.0, 255.0) as u8;
    (to_u8(r), to_u8(g), to_u8(b))
}

fn rgb_to_hsv(color: Color) -> (f64, f64, f64) {
    let (r, g, b) = (
        f64::from(color.0) / 255.0,
        f64::from(color.1) / 255.0,
        f64::from(color.2) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if delta <= f64::EPSILON {
        return (0.0, 0.0, max);
    }
    let hue = if (max - r).abs() < f64::EPSILON {
        (g - b) / delta
    } else if (max - g).abs() < f64::EPSILON {
        2.0 + (b - r) / delta
    } else {
        4.0 + (r - g) / delta
    };
    ((hue / 6.0).rem_euclid(1.0), delta / max, max)
}

fn hex_string(color: Color) -> String {
    format!("#{:02X}{:02X}{:02X}", color.0, color.1, color.2)
}

fn parse_hex(text: &str) -> Option<Color> {
    let digits: String = text
        .trim()
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '#')
        .collect();
    let nibble = |character: char| character.to_digit(16).map(|digit| digit as u8);
    match digits.len() {
        3 => {
            let mut bytes = [0u8; 3];
            for (index, character) in digits.chars().enumerate() {
                bytes[index] = nibble(character)? * 17;
            }
            Some((bytes[0], bytes[1], bytes[2]))
        }
        6 => {
            let bytes = digits.as_bytes();
            let mut out = [0u8; 3];
            for (index, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
                out[index] = nibble(pair[0] as char)? * 16 + nibble(pair[1] as char)?;
            }
            Some((out[0], out[1], out[2]))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::background::{GradientKind, GradientStop};
    use super::{hex_string, parse_hex, CustomGradient, FillValue, RebuildSlot};

    #[test]
    fn hex_colors_round_trip() {
        let color = (255, 103, 13);
        assert_eq!(parse_hex(&hex_string(color)), Some(color));
        assert_eq!(parse_hex("#abc"), Some((170, 187, 204)));
    }

    #[test]
    fn plain_fill_promotes_to_a_two_stop_gradient() {
        let mut value = FillValue::Plain((20, 30, 40));
        let gradient = super::ensure_gradient(&mut value);
        assert_eq!(gradient.stops.len(), 2);
        assert_eq!(gradient.stops[0].color, (20, 30, 40));
        assert_eq!(gradient.stops[1].color, (20, 30, 40));
    }

    #[test]
    fn gradient_normalization_sorts_stops() {
        let mut gradient = CustomGradient::default();
        gradient.stops.reverse();
        let normalized = gradient.normalized();
        assert_eq!(normalized.stops[0].position, 0.0);
        assert_eq!(normalized.stops[1].position, 1.0);
    }

    fn rendered_gradient(kind: GradientKind, angle: f64) -> image::RgbaImage {
        let gradient = CustomGradient {
            kind,
            angle,
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: (0, 0, 0),
                },
                GradientStop {
                    position: 1.0,
                    color: (255, 255, 255),
                },
            ],
        };
        let mut surface =
            gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 65, 65).unwrap();
        {
            let context = gtk4::cairo::Context::new(&surface).unwrap();
            super::draw_gradient(&context, 65.0, 65.0, &gradient, 0.0);
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        crate::capture::editor::render::cairo_argb_to_rgba_image(65, 65, stride, &data)
    }

    #[test]
    fn gradient_kinds_render_distinct_geometry_and_rotate() {
        let linear = rendered_gradient(GradientKind::Linear, 0.0);
        let vertical = rendered_gradient(GradientKind::Linear, 90.0);
        assert!(linear.get_pixel(60, 32)[0] > linear.get_pixel(4, 32)[0] + 150);
        assert!(vertical.get_pixel(32, 60)[0] > vertical.get_pixel(32, 4)[0] + 150);

        let radial = rendered_gradient(GradientKind::Radial, 0.0);
        let diamond = rendered_gradient(GradientKind::Diamond, 0.0);
        assert!(radial.get_pixel(32, 32)[0] < 10);
        assert!(radial.get_pixel(0, 0)[0] > 240);
        assert!(diamond.get_pixel(0, 32)[0] > radial.get_pixel(0, 32)[0] + 40);
        assert!(
            (i16::from(diamond.get_pixel(16, 16)[0]) - i16::from(diamond.get_pixel(16, 32)[0]))
                .abs()
                <= 2
        );
        assert!(radial.get_pixel(16, 16)[0] > radial.get_pixel(16, 32)[0] + 30);

        let angular = rendered_gradient(GradientKind::Angular, 0.0);
        assert!(angular.get_pixel(60, 32)[0] < 10);
        assert!((60..=68).contains(&angular.get_pixel(32, 60)[0]));
        assert!((124..=132).contains(&angular.get_pixel(4, 32)[0]));
        let rotated = rendered_gradient(GradientKind::Angular, 90.0);
        assert!(rotated.get_pixel(32, 60)[0] < 10);
        assert!((187..=195).contains(&rotated.get_pixel(60, 32)[0]));
    }

    #[test]
    fn gradient_preview_and_export_use_the_same_fill() {
        use crate::capture::editor::state::{background::Fill, EditorState};
        use image::{Rgba, RgbaImage};
        use std::rc::Rc;

        for kind in [
            GradientKind::Linear,
            GradientKind::Radial,
            GradientKind::Angular,
            GradientKind::Diamond,
        ] {
            let gradient = CustomGradient {
                kind,
                angle: 90.0,
                ..CustomGradient::default()
            };
            let mut state =
                EditorState::new(RgbaImage::from_pixel(100, 80, Rgba([255, 0, 0, 255])));
            state.background.set_padding(40.0);
            state
                .background
                .set_fill(Fill::Custom(Rc::new(move |context, width, height| {
                    super::draw_gradient(
                        context,
                        f64::from(width),
                        f64::from(height),
                        &gradient,
                        0.0,
                    );
                })));
            let layout = state.background.layout(100, 80);
            let mut surface = state.background.surface(layout, 4096.0).unwrap();
            surface.flush();
            let stride = surface.stride() as usize;
            let data = surface.data().unwrap();
            let preview = crate::capture::editor::render::cairo_argb_to_rgba_image(
                layout.width as u32,
                layout.height as u32,
                stride,
                &data,
            );
            let exported = state.to_export_image().unwrap();
            for (x, y) in [(0, 0), (60, 2), (2, 50), (119, 99)] {
                assert_eq!(
                    preview.get_pixel(x, y),
                    exported.get_pixel(x, y),
                    "{kind:?} at {x},{y}"
                );
            }
        }
    }

    #[test]
    fn gradient_stop_picker_refreshes_rows_and_operations() {
        use gtk4::prelude::*;
        use std::cell::RefCell;
        use std::rc::Rc;

        fn collect(widget: &gtk4::Widget, class: &str, result: &mut Vec<gtk4::Widget>) {
            if widget.has_css_class(class) {
                result.push(widget.clone());
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                collect(&widget, class, result);
            }
        }

        let Some(()) = crate::test_support::with_gtk(|| {
            let parent = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
            let state = Rc::new(RefCell::new(FillValue::Gradient(CustomGradient::default())));
            let repaint: RebuildSlot = Rc::new(RefCell::new(None));
            let notify: Rc<dyn Fn()> = {
                let repaint = repaint.clone();
                Rc::new(move || {
                    if let Some(repaint) = repaint.borrow().clone() {
                        repaint()
                    }
                })
            };
            let view = super::build_gradient_page(parent.clone(), state.clone(), notify, 1.0);
            *repaint.borrow_mut() = Some(view.repaint.clone());
            let mut entries = Vec::new();
            collect(
                view.widget.upcast_ref(),
                "recording-editor-gradient-step-hex",
                &mut entries,
            );
            assert_eq!(entries.len(), 2);
            let mut picker_entries = Vec::new();
            collect(
                parent.upcast_ref(),
                "recording-editor-custom-hex",
                &mut picker_entries,
            );
            assert_eq!(picker_entries.len(), 2);
            let picker = picker_entries[1].clone().downcast::<gtk4::Entry>().unwrap();
            picker.set_text("#FFFFFF");
            picker.emit_activate();
            assert_eq!(
                super::current_gradient(&state).stops[1].color,
                (255, 255, 255)
            );
            assert_eq!(
                entries[1]
                    .clone()
                    .downcast::<gtk4::Entry>()
                    .unwrap()
                    .text()
                    .as_str(),
                "#FFFFFF"
            );

            let entry = entries[0].clone().downcast::<gtk4::Entry>().unwrap();
            entry.set_text("#123");
            entry.emit_activate();
            assert_eq!(super::current_gradient(&state).stops[0].color, (17, 34, 51));
            assert_eq!(entry.text().as_str(), "#112233");
            entry.set_text("invalid");
            entry.emit_activate();
            assert_eq!(entry.text().as_str(), "#112233");

            let mut adds = Vec::new();
            collect(
                view.widget.upcast_ref(),
                "recording-editor-gradient-add",
                &mut adds,
            );
            adds[0]
                .clone()
                .downcast::<gtk4::Button>()
                .unwrap()
                .emit_clicked();
            assert_eq!(super::current_gradient(&state).stops.len(), 3);
            let mut removes = Vec::new();
            collect(
                view.widget.upcast_ref(),
                "recording-editor-gradient-remove",
                &mut removes,
            );
            removes[1]
                .clone()
                .downcast::<gtk4::Button>()
                .unwrap()
                .emit_clicked();
            assert_eq!(super::current_gradient(&state).stops.len(), 2);

            let mut icons = Vec::new();
            collect(
                view.widget.upcast_ref(),
                "recording-editor-gradient-icon",
                &mut icons,
            );
            let flip = icons[0].clone().downcast::<gtk4::Button>().unwrap();
            let rotate = icons[1].clone().downcast::<gtk4::Button>().unwrap();
            flip.emit_clicked();
            let reversed = super::current_gradient(&state);
            assert_eq!(reversed.stops[0].color, (255, 255, 255));
            assert_eq!(reversed.stops[1].color, (17, 34, 51));
            let mut entries = Vec::new();
            collect(
                view.widget.upcast_ref(),
                "recording-editor-gradient-step-hex",
                &mut entries,
            );
            assert_eq!(
                entries[0]
                    .clone()
                    .downcast::<gtk4::Entry>()
                    .unwrap()
                    .text()
                    .as_str(),
                "#FFFFFF"
            );

            let mut items = Vec::new();
            collect(
                view.widget.upcast_ref(),
                "recording-editor-dropdown-item",
                &mut items,
            );
            assert_eq!(items.len(), 4);
            for (item, kind) in items.iter().zip([
                GradientKind::Linear,
                GradientKind::Radial,
                GradientKind::Angular,
                GradientKind::Diamond,
            ]) {
                let item = item.clone().downcast::<gtk4::Button>().unwrap();
                assert!(item.child().unwrap().is::<gtk4::Label>());
                item.emit_clicked();
                assert_eq!(super::current_gradient(&state).kind, kind);
                assert!(item.has_css_class("selected"));
                assert_eq!(
                    rotate.is_sensitive(),
                    matches!(kind, GradientKind::Linear | GradientKind::Angular)
                );
            }
            items[2]
                .clone()
                .downcast::<gtk4::Button>()
                .unwrap()
                .emit_clicked();
            rotate.emit_clicked();
            assert_eq!(super::current_gradient(&state).angle, 90.0);
        }) else {
            return;
        };
    }
}
