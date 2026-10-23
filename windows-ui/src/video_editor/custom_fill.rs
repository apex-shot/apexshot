//! Custom color and gradient controls for the Windows video editor.
//!
//! The side panel summarizes the current custom fill and opens an anchored
//! editor. Gradient stops open their own color picker, mirroring the shared
//! editor's two-level interaction.

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

const COLOR_RADIUS: f64 = 8.0;
const HANDLE_RADIUS: f64 = 8.0;
const SPECTRUM_BAR_THICKNESS: f64 = 10.0;

#[derive(Clone)]
enum FillValue {
    Empty,
    Plain(Color),
    Gradient(CustomGradient),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GradientKind {
    Linear,
    Radial,
    Angular,
    Diamond,
}

#[derive(Clone)]
struct GradientStop {
    position: f64,
    color: Color,
}

#[derive(Clone)]
struct CustomGradient {
    kind: GradientKind,
    angle: f64,
    stops: Vec<GradientStop>,
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

pub(super) struct CustomFillPage {
    pub widget: GtkBox,
    pub clear: Rc<dyn Fn()>,
}

pub(super) fn build_custom_page(scale: f64, on_selected: Rc<dyn Fn()>) -> CustomFillPage {
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

    let refresh_internal: Rc<dyn Fn()> = {
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
        let refresh_internal = refresh_internal.clone();
        let on_selected = on_selected.clone();
        Rc::new(move || {
            on_selected();
            refresh_internal();
        })
    };

    let (popover, refresh_popover) =
        build_custom_popover(&edit, state.clone(), notify.clone(), scale);
    repainters.borrow_mut().push(refresh_popover);
    refresh_internal();

    edit.connect_clicked({
        let popover = popover.clone();
        let refresh_internal = refresh_internal.clone();
        move |_| {
            refresh_internal();
            popover.popup();
        }
    });

    let clear: Rc<dyn Fn()> = {
        let state = state.clone();
        let refresh_internal = refresh_internal.clone();
        Rc::new(move || {
            *state.borrow_mut() = FillValue::Empty;
            refresh_internal();
        })
    };

    let widget = GtkBox::new(Orientation::Vertical, 0);
    widget.set_hexpand(true);
    widget.append(&row);
    CustomFillPage { widget, clear }
}

fn build_custom_popover(
    edit: &Button,
    state: Rc<RefCell<FillValue>>,
    notify: Rc<dyn Fn()>,
    scale: f64,
) -> (Popover, Rc<dyn Fn()>) {
    let popover = Popover::new();
    popover.add_css_class("recording-editor-custom-popover");
    popover.set_has_arrow(false);
    popover.set_position(PositionType::Left);
    popover.set_parent(edit);

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("recording-editor-custom-body");
    root.set_size_request((210.0 * scale).round() as i32, -1);

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
            _ => (17, 17, 17),
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

    let gradient_page = build_gradient_page(state.clone(), notify.clone(), scale);

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
        Rc::new(move || {
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
struct ColorPicker {
    widget: GtkBox,
    repaint: Rc<dyn Fn()>,
}

fn build_color_picker(
    get: Rc<dyn Fn() -> Color>,
    set: Rc<dyn Fn(Color)>,
    notify: Rc<dyn Fn()>,
    scale: f64,
) -> ColorPicker {
    let widget = GtkBox::new(Orientation::Vertical, 0);
    widget.set_hexpand(true);

    let field = DrawingArea::new();
    field.add_css_class("recording-editor-custom-field");
    field.set_content_height((190.0 * scale).round() as i32);
    field.set_hexpand(true);
    field.set_cursor(gtk4::gdk::Cursor::from_name("crosshair", None).as_ref());

    let spectrum = DrawingArea::new();
    spectrum.add_css_class("recording-editor-custom-spectrum");
    spectrum.set_content_height((22.0 * scale).round() as i32);
    spectrum.set_hexpand(true);

    let value_row = GtkBox::new(Orientation::Horizontal, 8);
    value_row.add_css_class("recording-editor-custom-value-row");
    let swatch = DrawingArea::new();
    swatch.add_css_class("recording-editor-custom-swatch");
    swatch.set_content_width((22.0 * scale).round() as i32);
    swatch.set_content_height((22.0 * scale).round() as i32);
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
    for (kind, label) in kinds {
        let item = Button::with_label(&label);
        item.add_css_class("recording-editor-dropdown-item");
        item.set_has_frame(false);
        item.set_halign(Align::Fill);
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

    let rebuild_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let rebuild: Rc<dyn Fn()> = {
        let stops = stops.clone();
        let state = state.clone();
        let notify = notify.clone();
        let refresh_ramp = refresh_ramp.clone();
        let add = add.clone();
        let selected = selected.clone();
        let scale = scale;
        Rc::new(move || {
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
                stops.append(&row);

                let stop_picker =
                    build_stop_picker(&swatch, state.clone(), index, notify.clone(), scale);
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
        move |_| {
            let mut guard = state.borrow_mut();
            let mut gradient = ensure_gradient(&mut guard);
            for stop in &mut gradient.stops {
                stop.position = 1.0 - stop.position;
            }
            *guard = FillValue::Gradient(gradient.normalized());
            drop(guard);
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
        Rc::new(move || refresh_ramp()) as Rc<dyn Fn()>
    };
    GradientView { widget, repaint }
}

fn build_stop_picker(
    swatch: &Button,
    state: Rc<RefCell<FillValue>>,
    index: usize,
    notify: Rc<dyn Fn()>,
    scale: f64,
) -> Popover {
    let popover = Popover::new();
    popover.add_css_class("recording-editor-gradient-picker-popover");
    popover.set_has_arrow(false);
    popover.set_position(PositionType::Bottom);
    popover.set_parent(swatch);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("recording-editor-gradient-picker-card");
    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.add_css_class("recording-editor-gradient-picker-header");
    let title = Label::new(Some(&t("Stop color")));
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
    let set = {
        let state = state.clone();
        let notify = notify.clone();
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
    let picker = build_color_picker(get, set, notify, scale);
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
        move |entry| apply(&entry.text())
    });
    let focus = EventControllerFocus::new();
    focus.connect_leave({
        let apply = apply.clone();
        move |controller| {
            let Some(entry) = controller
                .widget()
                .and_then(|widget| widget.downcast::<Entry>().ok())
            else {
                return;
            };
            apply(&entry.text());
        }
    });
    entry.add_controller(focus);
}

fn sample_gradient_color(gradient: &CustomGradient, position: f64) -> Color {
    let gradient = gradient.normalized();
    let Some(first) = gradient.stops.first() else {
        return (255, 255, 255);
    };
    if position <= first.position {
        return first.color;
    }
    for pair in gradient.stops.windows(2) {
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
    gradient
        .stops
        .last()
        .map_or((255, 255, 255), |stop| stop.color)
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
    let mut flat = gradient.clone();
    flat.kind = GradientKind::Linear;
    flat.angle = 0.0;
    context.save().ok();
    context.translate(0.0, track_top);
    draw_gradient(context, width, track_height, &flat, track_height * 0.5);
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
    rebuild_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
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
    if width < 2.0 || height < 2.0 {
        return;
    }
    let gradient = gradient.normalized();
    context.save().ok();
    rounded_rect(context, 0.0, 0.0, width, height, radius);
    context.clip();
    let start_end = gradient_endpoints(width, height, gradient.angle);
    match gradient.kind {
        GradientKind::Radial | GradientKind::Diamond => {
            let pattern = RadialGradient::new(
                width * 0.5,
                height * 0.5,
                0.0,
                width * 0.5,
                height * 0.5,
                width.max(height) * 0.65,
            );
            for stop in &gradient.stops {
                let color = stop_color(stop.color);
                pattern.add_color_stop_rgb(stop.position, color.0, color.1, color.2);
            }
            context.set_source(&pattern).ok();
        }
        GradientKind::Linear | GradientKind::Angular => {
            let pattern = LinearGradient::new(start_end.0, start_end.1, start_end.2, start_end.3);
            for stop in &gradient.stops {
                let color = stop_color(stop.color);
                pattern.add_color_stop_rgb(stop.position, color.0, color.1, color.2);
            }
            context.set_source(&pattern).ok();
        }
    }
    context.rectangle(0.0, 0.0, width, height);
    context.fill().ok();
    context.restore().ok();
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

    let x = (1.0 - saturation) * width;
    let y = (1.0 - value) * height;
    context.set_source_rgb(1.0, 1.0, 1.0);
    context.arc(x, y, 7.0, 0.0, TAU);
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
            for (index, pair) in bytes.chunks_exact(2).enumerate() {
                out[index] = nibble(pair[0] as char)? * 16 + nibble(pair[1] as char)?;
            }
            Some((out[0], out[1], out[2]))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{hex_string, parse_hex, CustomGradient, FillValue};

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
}
