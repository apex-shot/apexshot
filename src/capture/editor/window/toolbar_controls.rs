use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, Button, DrawingArea, Label, Orientation, Popover,
    Revealer,
};

use crate::capture::editor::{
    numbering_style::{NumberSize, NumberingStyle},
    pen_weight::{HighlighterMode, PenWeight},
    render::{draw_arrow, draw_number},
    state::EditorState,
    types::{AnnotationAction, ArrowStyle, DrawColor, ObfuscateMethod, Point, Tool},
};
use crate::i18n::t;

use super::floating_bar::queue_draw;
use super::icon_names;

#[derive(Clone, Default)]
pub(super) struct MenuTracker {
    popovers: Rc<RefCell<Vec<glib::WeakRef<Popover>>>>,
    palettes: Rc<RefCell<Vec<glib::WeakRef<Revealer>>>>,
}

impl MenuTracker {
    pub fn register(&self, popover: &Popover) {
        self.popovers.borrow_mut().push(popover.downgrade());
    }

    pub fn register_palette(&self, panel: &Revealer) {
        self.palettes.borrow_mut().push(panel.downgrade());
    }

    pub fn any_open(&self) -> bool {
        self.popovers
            .borrow()
            .iter()
            .any(|weak| weak.upgrade().is_some_and(|p| p.is_visible()))
            || self
                .palettes
                .borrow()
                .iter()
                .any(|weak| weak.upgrade().is_some_and(|p| p.reveals_child()))
    }

    pub fn close_all(&self) {
        for weak in self.popovers.borrow().iter() {
            if let Some(popover) = weak.upgrade() {
                if popover.is_visible() {
                    popover.popdown();
                }
            }
        }
        for weak in self.palettes.borrow().iter() {
            if let Some(panel) = weak.upgrade() {
                panel.set_reveal_child(false);
            }
        }
    }
}

fn mutate(
    state: &Arc<Mutex<EditorState>>,
    area: &glib::WeakRef<DrawingArea>,
    apply: impl FnOnce(&mut EditorState),
) {
    {
        let mut st = state.lock().unwrap();
        apply(&mut st);
    }
    queue_draw(area);
}

pub(super) fn chevron() -> gtk4::Image {
    let chevron = gtk4::Image::from_icon_name(icon_names::CHEVRON_DOWN_REGULAR);
    chevron.set_pixel_size(10);
    chevron.add_css_class("editor-subbar-pill-arrow");
    chevron
}

fn menu_popover(menu_class: &str) -> Popover {
    let popover = Popover::new();
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    popover.set_position(gtk4::PositionType::Bottom);
    popover.set_offset(0, 8);
    popover.add_css_class("editor-subbar-popover");
    popover.add_css_class(menu_class);
    popover
}

fn trigger_button(trigger_class: &str) -> (Button, GtkBox) {
    let button = Button::new();
    button.set_focusable(false);
    button.add_css_class("editor-stroke-trigger");
    button.add_css_class(trigger_class);
    let content = GtkBox::new(Orientation::Horizontal, 5);
    button.set_child(Some(&content));
    (button, content)
}

fn sub_bar_menu() -> GtkBox {
    let list = GtkBox::new(Orientation::Horizontal, 3);
    list.add_css_class("editor-stroke-menu");
    list
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StrokeKind {
    Pen,
    Shape,
    Highlighter,
}

impl StrokeKind {
    fn width(self, weight: PenWeight) -> f64 {
        match self {
            Self::Pen => weight.pen_stroke_width(),
            Self::Highlighter => weight.highlighter_stroke_width(),
            Self::Shape => [2.0, 4.0, 7.0, 12.0][weight.index()],
        }
    }
}

fn active_stroke_kind(st: &EditorState) -> StrokeKind {
    match st.selected_action() {
        Some(AnnotationAction::Pen { .. }) => StrokeKind::Pen,
        Some(AnnotationAction::Highlighter { .. }) => StrokeKind::Highlighter,
        Some(
            AnnotationAction::Line { .. }
            | AnnotationAction::Arrow { .. }
            | AnnotationAction::Box { .. }
            | AnnotationAction::Circle { .. },
        ) => StrokeKind::Shape,
        _ => match st.selected_tool {
            Tool::Pen => StrokeKind::Pen,
            Tool::Highlighter => StrokeKind::Highlighter,
            _ => StrokeKind::Shape,
        },
    }
}

fn active_stroke_width(st: &EditorState, kind: StrokeKind) -> Option<f64> {
    if let Some(width) = st.selected_action_stroke_size() {
        return Some(width);
    }
    if kind == StrokeKind::Highlighter && st.highlighter_mode == HighlighterMode::TextAware {
        None
    } else {
        Some(st.stroke_size)
    }
}

fn apply_stroke_weight(st: &mut EditorState, kind: StrokeKind, weight: PenWeight) {
    if kind == StrokeKind::Highlighter {
        st.set_highlighter_weight(weight);
        st.set_stroke_size(kind.width(weight));
    } else {
        if kind == StrokeKind::Pen {
            st.set_pen_weight_and_apply(weight);
        }
        st.set_active_size_without_rebuild(kind.width(weight));
    }
}

fn stroke_preview(kind: Rc<Cell<StrokeKind>>, weight: Rc<Cell<Option<usize>>>) -> DrawingArea {
    let preview = DrawingArea::new();
    preview.add_css_class("editor-stroke-preview");
    preview.set_content_width(28);
    preview.set_content_height(22);
    preview.set_can_target(false);
    preview.set_draw_func(move |area, cr, width, height| {
        let color = area.color();
        cr.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            color.alpha().into(),
        );
        let widths = [1.5, 2.7, 4.1, 6.0];
        cr.set_line_width(widths[weight.get().unwrap_or(1).min(3)]);
        cr.set_line_cap(gtk4::cairo::LineCap::Round);
        cr.set_line_join(gtk4::cairo::LineJoin::Round);
        let w = f64::from(width);
        let h = f64::from(height);
        if kind.get() == StrokeKind::Shape {
            cr.move_to(w * 0.19, h * 0.5);
            cr.line_to(w * 0.81, h * 0.5);
        } else {
            cr.move_to(w * 0.15, h * 0.64);
            cr.curve_to(w * 0.26, h * 0.22, w * 0.44, h * 0.20, w * 0.42, h * 0.56);
            cr.curve_to(w * 0.40, h * 0.80, w * 0.62, h * 0.20, w * 0.64, h * 0.55);
            cr.curve_to(w * 0.65, h * 0.75, w * 0.78, h * 0.65, w * 0.86, h * 0.48);
        }
        let _ = cr.stroke();
    });
    preview
}

pub(super) struct StrokeControl {
    pub button: Button,
    popover: Popover,
    sync: Rc<dyn Fn(&EditorState)>,
    #[cfg(test)]
    options: Vec<Button>,
}

impl StrokeControl {
    pub fn new(
        state: &Arc<Mutex<EditorState>>,
        drawing_area: &DrawingArea,
        menus: &MenuTracker,
        after_change: Option<Rc<dyn Fn()>>,
    ) -> Self {
        let kind = Rc::new(Cell::new(StrokeKind::Pen));
        let current = Rc::new(Cell::new(None));
        let (button, content) = trigger_button("editor-stroke-trigger-button");
        let preview = stroke_preview(kind.clone(), current.clone());
        content.append(&preview);
        let label = Label::new(Some(&t("Stroke")));
        label.add_css_class("editor-subbar-pill-label");
        content.append(&label);
        content.append(&chevron());

        let popover = menu_popover("editor-stroke-popover");
        let list = sub_bar_menu();
        let mut options = Vec::new();
        let mut previews = Vec::new();
        for weight in PenWeight::ALL {
            let option = Button::new();
            option.set_focusable(false);
            option.add_css_class("editor-stroke-option");
            option.set_valign(Align::Center);
            let icon = stroke_preview(kind.clone(), Rc::new(Cell::new(Some(weight.index()))));
            option.set_child(Some(&icon));
            option.connect_clicked({
                let state = state.clone();
                let area = drawing_area.downgrade();
                let popover = popover.downgrade();
                let after_change = after_change.clone();
                move |_| {
                    if let Some(popover) = popover.upgrade() {
                        popover.popdown();
                    }
                    mutate(&state, &area, |st| {
                        let kind = active_stroke_kind(st);
                        apply_stroke_weight(st, kind, weight);
                    });
                    if let Some(after_change) = &after_change {
                        after_change();
                    }
                }
            });
            list.append(&option);
            options.push(option);
            previews.push(icon);
        }
        popover.set_child(Some(&list));
        menus.register(&popover);
        button.connect_clicked({
            let popover = popover.clone();
            let menus = menus.clone();
            move |_| {
                menus.close_all();
                popover.popup();
            }
        });

        let sync = {
            let options = options.clone();
            let button = button.downgrade();
            Rc::new(move |st: &EditorState| {
                let active_kind = active_stroke_kind(st);
                let width = active_stroke_width(st, active_kind);
                let selected = width.and_then(|width| {
                    PenWeight::ALL
                        .iter()
                        .position(|weight| (active_kind.width(*weight) - width).abs() < 0.001)
                });
                let preview_index = width.and_then(|width| {
                    PenWeight::ALL
                        .iter()
                        .min_by(|a, b| {
                            (active_kind.width(**a) - width)
                                .abs()
                                .total_cmp(&(active_kind.width(**b) - width).abs())
                        })
                        .map(|weight| weight.index())
                });
                let selected = selected.or(preview_index);
                kind.set(active_kind);
                current.set(preview_index);
                preview.queue_draw();
                if let Some(button) = button.upgrade() {
                    let description = match width {
                        Some(width) => format!("{}: {} px", t("Stroke"), width),
                        None => format!("{}: {}", t("Stroke"), t("Automatic")),
                    };
                    button.set_tooltip_text(Some(&description));
                    button.update_property(&[gtk4::accessible::Property::Label(&description)]);
                }
                for (index, option) in options.iter().enumerate() {
                    if selected == Some(index) {
                        option.add_css_class("selected");
                    } else {
                        option.remove_css_class("selected");
                    }
                    option.update_state(&[gtk4::accessible::State::Pressed(
                        if selected == Some(index) {
                            gtk4::AccessibleTristate::True
                        } else {
                            gtk4::AccessibleTristate::False
                        },
                    )]);
                    let weight = PenWeight::ALL[index];
                    let description =
                        format!("{} ({} px)", t(weight.label()), active_kind.width(weight));
                    option.set_tooltip_text(Some(&description));
                    option.update_property(&[gtk4::accessible::Property::Label(&description)]);
                    previews[index].queue_draw();
                }
            })
        };
        Self {
            button,
            popover,
            sync,
            #[cfg(test)]
            options,
        }
    }

    pub fn sync(&self, st: &EditorState) {
        (self.sync)(st);
    }

    pub fn anchor_to(&self, bar: &GtkBox) {
        self.popover.set_parent(bar);
    }
}

impl Drop for StrokeControl {
    fn drop(&mut self) {
        if self.popover.parent().is_some() {
            self.popover.unparent();
        }
    }
}

const ARROW_STYLES: [(ArrowStyle, &str); 4] = [
    (ArrowStyle::Standard, "Standard"),
    (ArrowStyle::Fancy, "Fancy"),
    (ArrowStyle::Curved, "Curved"),
    (ArrowStyle::Double, "Double"),
];

fn arrow_preview(style: Rc<Cell<ArrowStyle>>) -> DrawingArea {
    let preview = DrawingArea::new();
    preview.set_content_width(28);
    preview.set_content_height(22);
    preview.set_can_target(false);
    preview.set_draw_func(move |widget, cr, width, height| {
        let color = widget.color();
        let (w, h) = (f64::from(width), f64::from(height));
        let start = Point {
            x: w * 0.16,
            y: h * 0.76,
        };
        let end = Point {
            x: w * 0.84,
            y: h * 0.22,
        };
        let style = style.get();
        let controls = if matches!(style, ArrowStyle::Curved | ArrowStyle::Double) {
            Some(vec![
                start,
                Point {
                    x: w * 0.22,
                    y: h * 0.18,
                },
                end,
            ])
        } else {
            None
        };
        draw_arrow(
            cr,
            start,
            end,
            DrawColor::new(
                color.red().into(),
                color.green().into(),
                color.blue().into(),
                color.alpha().into(),
            ),
            1.6,
            style,
            controls,
            false,
        );
    });
    preview
}

pub(super) struct ArrowControl {
    pub button: Button,
    popover: Popover,
    style: Rc<Cell<ArrowStyle>>,
    preview: DrawingArea,
    options: Vec<Button>,
}

impl ArrowControl {
    pub fn new(
        state: &Arc<Mutex<EditorState>>,
        drawing_area: &DrawingArea,
        menus: &MenuTracker,
    ) -> Self {
        let style = Rc::new(Cell::new(ArrowStyle::Standard));
        let (button, content) = trigger_button("editor-arrow-trigger");
        let icon = arrow_preview(style.clone());
        content.append(&icon);
        let label = Label::new(Some(&t("Arrow")));
        label.add_css_class("editor-subbar-pill-label");
        content.append(&label);
        content.append(&chevron());

        let popover = menu_popover("editor-stroke-popover");
        let list = sub_bar_menu();
        let mut options = Vec::new();
        for (value, name) in ARROW_STYLES {
            let option = Button::new();
            option.set_focusable(false);
            option.add_css_class("editor-stroke-option");
            option.add_css_class("editor-arrow-option");
            option.set_valign(Align::Center);
            let description = t(name);
            option.set_tooltip_text(Some(&description));
            option.update_property(&[gtk4::accessible::Property::Label(&description)]);
            option.set_child(Some(&arrow_preview(Rc::new(Cell::new(value)))));
            option.connect_clicked({
                let state = state.clone();
                let area = drawing_area.downgrade();
                let popover = popover.downgrade();
                move |_| {
                    if let Some(popover) = popover.upgrade() {
                        popover.popdown();
                    }
                    mutate(&state, &area, |st| {
                        st.set_arrow_style(value);
                        st.set_selected_arrow_style(value);
                    });
                }
            });
            list.append(&option);
            options.push(option);
        }
        popover.set_child(Some(&list));
        menus.register(&popover);
        button.connect_clicked({
            let popover = popover.clone();
            let menus = menus.clone();
            move |_| {
                menus.close_all();
                popover.popup();
            }
        });
        Self {
            button,
            popover,
            style,
            preview: icon,
            options,
        }
    }

    pub fn anchor_to(&self, bar: &GtkBox) {
        self.popover.set_parent(bar);
    }

    pub fn sync(&self, st: &EditorState) {
        let value = st.selected_arrow_style().unwrap_or(st.arrow_style);
        self.style.set(value);
        self.preview.queue_draw();
        let label = ARROW_STYLES
            .iter()
            .find(|(style, _)| *style == value)
            .map(|(_, label)| *label)
            .unwrap_or("Standard");
        let description = format!("{}: {}", t("Arrow style"), t(label));
        self.button.set_tooltip_text(Some(&description));
        self.button
            .update_property(&[gtk4::accessible::Property::Label(&description)]);
        for (option, (style, _)) in self.options.iter().zip(ARROW_STYLES) {
            if style == value {
                option.add_css_class("selected");
            } else {
                option.remove_css_class("selected");
            }
            option.update_state(&[gtk4::accessible::State::Pressed(if style == value {
                gtk4::AccessibleTristate::True
            } else {
                gtk4::AccessibleTristate::False
            })]);
        }
    }
}

impl Drop for ArrowControl {
    fn drop(&mut self) {
        if self.popover.parent().is_some() {
            self.popover.unparent();
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum NumberControlKind {
    Style,
    Size,
}

fn number_style_sample(style: NumberingStyle) -> String {
    [1, 2, 3]
        .into_iter()
        .map(|number| style.format(number))
        .collect::<Vec<_>>()
        .join(" ")
}

fn number_size_preview(size: Rc<Cell<NumberSize>>) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_content_width(28);
    area.set_content_height(22);
    area.set_can_target(false);
    area.set_draw_func(move |widget, cr, width, height| {
        let size = size.get();
        let color = widget.color();
        let radius = [5.4, 6.9, 8.4, 9.9][size.index()];
        let factor = radius / size.radius();
        let _ = cr.save();
        cr.translate(f64::from(width) / 2.0, f64::from(height) / 2.0);
        cr.scale(factor, factor);
        draw_number(
            cr,
            Point { x: 0.0, y: 0.0 },
            1,
            DrawColor::new(
                color.red().into(),
                color.green().into(),
                color.blue().into(),
                color.alpha().into(),
            ),
            NumberingStyle::Numeric,
            size,
        );
        let _ = cr.restore();
    });
    area
}

pub(super) struct NumberControl {
    pub button: Button,
    popover: Popover,
    kind: NumberControlKind,
    style_sample: Option<Label>,
    current_size: Rc<Cell<NumberSize>>,
    size_preview: Option<DrawingArea>,
    options: Vec<Button>,
}

impl NumberControl {
    pub fn new(
        state: &Arc<Mutex<EditorState>>,
        drawing_area: &DrawingArea,
        menus: &MenuTracker,
        kind: NumberControlKind,
    ) -> Self {
        let (button, content) = trigger_button("editor-number-trigger");
        let current_size = Rc::new(Cell::new(NumberSize::Medium));
        let (trigger_style_label, trigger_size_preview) = match kind {
            NumberControlKind::Style => {
                button.add_css_class("editor-number-style-trigger");
                let label = Label::new(Some(&number_style_sample(NumberingStyle::Numeric)));
                label.add_css_class("editor-number-style-sample");
                content.append(&label);
                (Some(label), None)
            }
            NumberControlKind::Size => {
                button.add_css_class("editor-number-size-trigger");
                let icon = number_size_preview(current_size.clone());
                content.append(&icon);
                let label = Label::new(Some(&t("Size")));
                label.add_css_class("editor-subbar-pill-label");
                content.append(&label);
                (None, Some(icon))
            }
        };
        content.append(&chevron());

        let popover = menu_popover("editor-stroke-popover");
        let list = sub_bar_menu();
        let mut options = Vec::new();
        let count = match kind {
            NumberControlKind::Style => NumberingStyle::ALL.len(),
            NumberControlKind::Size => NumberSize::ALL.len(),
        };
        for index in 0..count {
            let option = Button::new();
            option.set_focusable(false);
            option.add_css_class("editor-stroke-option");
            option.set_valign(Align::Center);
            let description = match kind {
                NumberControlKind::Style => {
                    option.add_css_class("editor-number-style-option");
                    let style = NumberingStyle::ALL[index];
                    let sample = Label::new(Some(&number_style_sample(style)));
                    sample.add_css_class("editor-number-style-sample");
                    option.set_child(Some(&sample));
                    t(style.label())
                }
                NumberControlKind::Size => {
                    option.add_css_class("editor-number-size-option");
                    let size = NumberSize::ALL[index];
                    option.set_child(Some(&number_size_preview(Rc::new(Cell::new(size)))));
                    t(size.label())
                }
            };
            option.set_tooltip_text(Some(&description));
            option.update_property(&[gtk4::accessible::Property::Label(&description)]);
            option.connect_clicked({
                let state = state.clone();
                let area = drawing_area.downgrade();
                let popover = popover.downgrade();
                move |_| {
                    if let Some(popover) = popover.upgrade() {
                        popover.popdown();
                    }
                    mutate(&state, &area, |st| match kind {
                        NumberControlKind::Style => {
                            st.set_numbering_style(NumberingStyle::ALL[index]);
                        }
                        NumberControlKind::Size => {
                            st.set_number_size(NumberSize::ALL[index]);
                        }
                    });
                }
            });
            list.append(&option);
            options.push(option);
        }
        popover.set_child(Some(&list));
        menus.register(&popover);
        button.connect_clicked({
            let popover = popover.clone();
            let menus = menus.clone();
            move |_| {
                menus.close_all();
                popover.popup();
            }
        });
        Self {
            button,
            popover,
            kind,
            style_sample: trigger_style_label,
            current_size,
            size_preview: trigger_size_preview,
            options,
        }
    }

    pub fn anchor_to(&self, bar: &GtkBox) {
        self.popover.set_parent(bar);
    }

    pub fn sync(&self, st: &EditorState) {
        let (index, description) = match self.kind {
            NumberControlKind::Style => {
                let style = st.active_numbering_style();
                if let Some(label) = &self.style_sample {
                    label.set_text(&number_style_sample(style));
                }
                (
                    style.index(),
                    format!("{}: {}", t("Numbering style"), t(style.label())),
                )
            }
            NumberControlKind::Size => {
                let size = st.active_number_size();
                self.current_size.set(size);
                if let Some(preview) = &self.size_preview {
                    preview.queue_draw();
                }
                (size.index(), format!("{}: {}", t("Size"), t(size.label())))
            }
        };
        self.button.set_tooltip_text(Some(&description));
        self.button
            .update_property(&[gtk4::accessible::Property::Label(&description)]);
        for (option_index, option) in self.options.iter().enumerate() {
            if option_index == index {
                option.add_css_class("selected");
            } else {
                option.remove_css_class("selected");
            }
            option.update_state(
                &[gtk4::accessible::State::Pressed(if option_index == index {
                    gtk4::AccessibleTristate::True
                } else {
                    gtk4::AccessibleTristate::False
                })],
            );
        }
    }
}

impl Drop for NumberControl {
    fn drop(&mut self) {
        if self.popover.parent().is_some() {
            self.popover.unparent();
        }
    }
}

pub(super) const EFFECT_METHODS: [(ObfuscateMethod, &str); 3] = [
    (ObfuscateMethod::Pixelate, "Pixelate"),
    (ObfuscateMethod::Blur, "Blur"),
    (ObfuscateMethod::Blackout, "Blackout"),
];

pub(super) fn active_effect_method(st: &EditorState) -> ObfuscateMethod {
    match st.selected_action() {
        Some(AnnotationAction::Obfuscate { method, .. }) => *method,
        _ => st.obfuscate_method(),
    }
}

fn effect_icon(method: Rc<Cell<ObfuscateMethod>>) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_content_width(28);
    area.set_content_height(22);
    area.set_can_target(false);
    area.set_draw_func(move |widget, cr, width, height| {
        let color = widget.color();
        let (cx, cy) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
        let set_alpha = |alpha: f64| {
            cr.set_source_rgba(
                color.red().into(),
                color.green().into(),
                color.blue().into(),
                f64::from(color.alpha()) * alpha,
            );
        };
        match method.get() {
            ObfuscateMethod::Pixelate => {
                let side = 4.0;
                for row in 0..3 {
                    for col in 0..3 {
                        set_alpha([0.95, 0.55, 0.75][(row + col) % 3]);
                        cr.rectangle(
                            cx + (col as f64 - 1.5) * side,
                            cy + (row as f64 - 1.5) * side,
                            side - 1.0,
                            side - 1.0,
                        );
                        let _ = cr.fill();
                    }
                }
            }
            ObfuscateMethod::Blur => {
                for (radius, alpha) in [(9.0, 0.08), (7.0, 0.13), (5.0, 0.22), (3.0, 0.55)] {
                    set_alpha(alpha);
                    cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
                    let _ = cr.fill();
                }
            }
            ObfuscateMethod::Blackout => {
                set_alpha(1.0);
                cr.rectangle(cx - 7.0, cy - 7.0, 14.0, 14.0);
                let _ = cr.fill();
            }
        }
    });
    area
}

pub(super) struct EffectMode {
    pub button: Button,
    popover: Popover,
    method: Rc<Cell<ObfuscateMethod>>,
    icon: DrawingArea,
    label: Label,
    options: Vec<Button>,
}

impl EffectMode {
    pub fn new(
        state: &Arc<Mutex<EditorState>>,
        drawing_area: &DrawingArea,
        rebuild: &Rc<dyn Fn()>,
        menus: &MenuTracker,
        after_pick: Option<Rc<dyn Fn(usize)>>,
    ) -> Self {
        let method = Rc::new(Cell::new(ObfuscateMethod::Pixelate));
        let (button, content) = trigger_button("editor-effect-mode-trigger");
        let preview = effect_icon(method.clone());
        content.append(&preview);
        let label = Label::new(Some(&t("Pixelate")));
        label.add_css_class("editor-subbar-pill-label");
        content.append(&label);
        content.append(&chevron());

        let popover = menu_popover("editor-stroke-popover");
        let list = sub_bar_menu();
        let mut options = Vec::new();
        for (index, (value, name)) in EFFECT_METHODS.iter().enumerate() {
            let option = Button::new();
            option.set_focusable(false);
            option.add_css_class("editor-stroke-option");
            option.add_css_class("editor-effect-mode-option");
            option.set_valign(Align::Center);
            let description = t(name);
            option.set_tooltip_text(Some(&description));
            option.update_property(&[gtk4::accessible::Property::Label(&description)]);
            option.set_child(Some(&effect_icon(Rc::new(Cell::new(*value)))));
            option.connect_clicked({
                let state = state.clone();
                let area = drawing_area.downgrade();
                let popover = popover.downgrade();
                let rebuild = rebuild.clone();
                let after_pick = after_pick.clone();
                move |_| {
                    if let Some(popover) = popover.upgrade() {
                        popover.popdown();
                    }
                    mutate(&state, &area, |st| {
                        st.set_obfuscate_method(*value);
                    });
                    rebuild();
                    if let Some(after_pick) = &after_pick {
                        after_pick(index);
                    }
                }
            });
            list.append(&option);
            options.push(option);
        }
        popover.set_child(Some(&list));
        menus.register(&popover);
        button.connect_clicked({
            let popover = popover.clone();
            let menus = menus.clone();
            move |_| {
                menus.close_all();
                popover.popup();
            }
        });
        Self {
            button,
            popover,
            method,
            icon: preview,
            label,
            options,
        }
    }

    pub fn anchor_to(&self, bar: &GtkBox) {
        self.popover.set_parent(bar);
    }

    pub fn sync(&self, st: &EditorState) {
        let method = active_effect_method(st);
        self.method.set(method);
        self.icon.queue_draw();
        let label = EFFECT_METHODS
            .iter()
            .find(|(value, _)| *value == method)
            .map(|(_, label)| *label)
            .unwrap_or("Pixelate");
        self.label.set_text(&t(label));
        self.button.set_tooltip_text(Some(&t(label)));
        for (option, (value, _)) in self.options.iter().zip(EFFECT_METHODS) {
            if value == method {
                option.add_css_class("selected");
            } else {
                option.remove_css_class("selected");
            }
            option.update_state(&[gtk4::accessible::State::Pressed(if value == method {
                gtk4::AccessibleTristate::True
            } else {
                gtk4::AccessibleTristate::False
            })]);
        }
    }
}

impl Drop for EffectMode {
    fn drop(&mut self) {
        if self.popover.parent().is_some() {
            self.popover.unparent();
        }
    }
}

pub(super) struct TextPill {
    pub button: Button,
    pub set_selected: Rc<dyn Fn(usize)>,
    #[allow(dead_code)]
    checks: Rc<RefCell<Vec<gtk4::Image>>>,
}

pub(super) fn text_pill(
    menus: &MenuTracker,
    options: &[&str],
    on_pick: Rc<dyn Fn(usize)>,
) -> TextPill {
    let button = Button::new();
    button.set_focusable(false);
    button.add_css_class("editor-subbar-pill");
    let text = Label::new(Some(options.first().copied().unwrap_or("")));
    text.add_css_class("editor-subbar-pill-label");
    let content = GtkBox::new(Orientation::Horizontal, 4);
    content.append(&text);
    content.append(&chevron());
    button.set_child(Some(&content));

    let popover = menu_popover("editor-subbar-menu-popover");
    let list = GtkBox::new(Orientation::Vertical, 0);
    list.add_css_class("editor-subbar-menu");
    let selected = Rc::new(Cell::new(0usize));
    let checks = Rc::new(RefCell::new(Vec::<gtk4::Image>::new()));
    for (index, option) in options.iter().enumerate() {
        let check = gtk4::Image::from_icon_name(icon_names::custom::CHECK_SYMBOLIC);
        check.set_pixel_size(12);
        check.add_css_class("editor-subbar-check");
        check.set_visible(index == 0);
        let option_label = Label::new(Some(option));
        option_label.set_xalign(0.0);
        option_label.set_hexpand(true);
        let row_box = GtkBox::new(Orientation::Horizontal, 6);
        row_box.append(&option_label);
        row_box.append(&check);
        let row = Button::new();
        row.set_has_frame(false);
        row.set_focusable(false);
        row.add_css_class("editor-subbar-option");
        row.set_child(Some(&row_box));
        row.connect_clicked({
            let popover = popover.clone();
            let text = text.clone();
            let selected = selected.clone();
            let checks = checks.clone();
            let on_pick = on_pick.clone();
            let labels: Vec<String> = options.iter().map(|s| s.to_string()).collect();
            move |_| {
                selected.set(index);
                for (i, check) in checks.borrow().iter().enumerate() {
                    check.set_visible(i == index);
                }
                text.set_text(&labels[index]);
                popover.popdown();
                on_pick(index);
            }
        });
        checks.borrow_mut().push(check);
        list.append(&row);
    }
    popover.set_child(Some(&list));
    popover.set_parent(&button);
    menus.register(&popover);
    button.connect_clicked({
        let menus = menus.clone();
        move |_| {
            menus.close_all();
            popover.popup();
        }
    });

    let labels: Vec<String> = options.iter().map(|s| s.to_string()).collect();
    let set_selected = {
        let text = text;
        let checks = checks.clone();
        Rc::new(move |index: usize| {
            selected.set(index);
            for (i, check) in checks.borrow().iter().enumerate() {
                check.set_visible(i == index);
            }
            if let Some(label) = labels.get(index) {
                text.set_text(label);
            }
        })
    };
    TextPill {
        button,
        set_selected,
        checks,
    }
}

pub(super) fn sub_bar(extra_class: &str) -> GtkBox {
    let root = GtkBox::new(Orientation::Horizontal, 6);
    root.add_css_class("editor-text-floating-bar");
    root.add_css_class("editor-sub-bar");
    root.add_css_class(extra_class);
    root.set_halign(Align::Start);
    root.set_valign(Align::Start);
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn test_state() -> Arc<Mutex<EditorState>> {
        Arc::new(Mutex::new(EditorState::new(RgbaImage::new(32, 32))))
    }

    fn push_selected_arrow(st: &mut EditorState) {
        st.actions.push(AnnotationAction::Arrow {
            start: Point { x: 2.0, y: 3.0 },
            end: Point { x: 24.0, y: 26.0 },
            color: DrawColor::new(1.0, 0.5, 0.0, 1.0),
            stroke_size: 4.0,
            style: ArrowStyle::Standard,
            control_points: None,
            shadow: false,
            reversed: false,
        });
        st.selected_action_index = Some(0);
    }

    #[test]
    fn arrow_option_click_applies_default_and_selected_style_and_undo_restores() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let state = test_state();
            state.lock().unwrap().selected_tool = Tool::Arrow;
            push_selected_arrow(&mut state.lock().unwrap());
            let area = DrawingArea::new();
            let menus = MenuTracker::default();
            let control = ArrowControl::new(&state, &area, &menus);
            let curved = ARROW_STYLES
                .iter()
                .position(|(style, _)| *style == ArrowStyle::Curved)
                .unwrap();
            control.options[curved].emit_clicked();
            {
                let st = state.lock().unwrap();
                assert_eq!(st.arrow_style, ArrowStyle::Curved);
                assert_eq!(st.selected_arrow_style(), Some(ArrowStyle::Curved));
                assert_eq!(st.history_availability(), (true, false));
            }
            assert!(state.lock().unwrap().undo());
            let st = state.lock().unwrap();
            let Some(AnnotationAction::Arrow { style, .. }) = st.actions.first() else {
                panic!("undo should restore the arrow's style");
            };
            assert_eq!(*style, ArrowStyle::Standard);
        }) else {
            return;
        };
    }

    #[test]
    fn arrow_option_click_updates_defaults_without_a_selection() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let state = test_state();
            state.lock().unwrap().selected_tool = Tool::Arrow;
            let area = DrawingArea::new();
            let menus = MenuTracker::default();
            let control = ArrowControl::new(&state, &area, &menus);
            control.options[0].emit_clicked(); // Standard
            control.options[3].emit_clicked(); // Double
            let st = state.lock().unwrap();
            assert_eq!(st.arrow_style, ArrowStyle::Double);
        }) else {
            return;
        };
    }

    #[test]
    fn stroke_preset_applies_pen_weight_and_shape_widths() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let state = test_state();
            let area = DrawingArea::new();
            let menus = MenuTracker::default();
            let control = StrokeControl::new(&state, &area, &menus, None);

            state.lock().unwrap().selected_tool = Tool::Pen;
            control.options[2].emit_clicked();
            {
                let st = state.lock().unwrap();
                let weight = PenWeight::ALL[2];
                assert_eq!(st.pen_weight, weight);
                assert_eq!(st.stroke_size, weight.pen_stroke_width());
            }

            state.lock().unwrap().selected_tool = Tool::Box;
            control.options[3].emit_clicked();
            {
                let st = state.lock().unwrap();
                assert_eq!(st.stroke_size, 12.0);
            }
        }) else {
            return;
        };
    }

    #[test]
    fn stroke_preset_on_selected_compatible_action_updates_it() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let state = test_state();
            {
                let mut st = state.lock().unwrap();
                st.actions.push(AnnotationAction::Line {
                    start: Point { x: 1.0, y: 1.0 },
                    end: Point { x: 20.0, y: 20.0 },
                    color: DrawColor::new(1.0, 1.0, 1.0, 1.0),
                    stroke_size: 2.0,
                    shadow: false,
                });
                st.selected_action_index = Some(0);
                st.selected_tool = Tool::Select;
            }
            let area = DrawingArea::new();
            let menus = MenuTracker::default();
            let control = StrokeControl::new(&state, &area, &menus, None);
            control.options[2].emit_clicked();
            let st = state.lock().unwrap();
            assert_eq!(st.selected_action_stroke_size(), Some(7.0));
            assert_eq!(st.history_availability(), (true, false));
        }) else {
            return;
        };
    }

    #[test]
    fn stroke_sync_preserves_an_off_preset_width_without_history() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let state = test_state();
            {
                let mut st = state.lock().unwrap();
                st.selected_tool = Tool::Line;
                st.stroke_size = 5.5;
            }
            let area = DrawingArea::new();
            let menus = MenuTracker::default();
            let control = StrokeControl::new(&state, &area, &menus, None);
            control.sync(&state.lock().unwrap());
            let st = state.lock().unwrap();
            assert_eq!(st.stroke_size, 5.5);
            assert_eq!(st.history_availability(), (false, false));
        }) else {
            return;
        };
    }

    #[test]
    fn highlighter_sync_keeps_automatic_and_preset_pick_switches_to_freehand() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let state = test_state();
            {
                let mut st = state.lock().unwrap();
                st.selected_tool = Tool::Highlighter;
                st.highlighter_mode = HighlighterMode::TextAware;
            }
            let area = DrawingArea::new();
            let menus = MenuTracker::default();
            let control = StrokeControl::new(&state, &area, &menus, None);
            control.sync(&state.lock().unwrap());
            assert_eq!(
                state.lock().unwrap().highlighter_mode,
                HighlighterMode::TextAware
            );
            control.options[1].emit_clicked();
            let st = state.lock().unwrap();
            assert_eq!(st.highlighter_mode, HighlighterMode::Freehand);
            assert_eq!(st.stroke_size, PenWeight::ALL[1].highlighter_stroke_width());
        }) else {
            return;
        };
    }

    #[test]
    fn number_style_and_size_options_update_state_with_history() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let state = test_state();
            state.lock().unwrap().selected_tool = Tool::Number;
            let area = DrawingArea::new();
            let menus = MenuTracker::default();
            let style = NumberControl::new(&state, &area, &menus, NumberControlKind::Style);
            let size = NumberControl::new(&state, &area, &menus, NumberControlKind::Size);
            style.options[1].emit_clicked();
            size.options[2].emit_clicked();
            {
                let st = state.lock().unwrap();
                assert_eq!(st.active_numbering_style(), NumberingStyle::ALL[1]);
                assert_eq!(st.active_number_size(), NumberSize::ALL[2]);
            }
        }) else {
            return;
        };
    }

    #[test]
    fn colour_trigger_reports_selected_then_default_and_never_background() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let state = test_state();
            let applied = Rc::new(Cell::new(false));
            let menus = MenuTracker::default();
            let control = crate::capture::editor::window::color_control::ColorControl::new(
                &state,
                Rc::new({
                    let applied = applied.clone();
                    move |_| applied.set(true)
                }),
                &menus,
                None,
            );
            {
                let mut st = state.lock().unwrap();
                st.selected_color = DrawColor::new(0.1, 0.2, 0.3, 1.0);
                st.background_style = crate::capture::editor::types::BackgroundStyle::PlainColor(
                    DrawColor::new(0.9, 0.1, 0.1, 1.0),
                );
            }
            control.sync(&state.lock().unwrap());
            assert_eq!(control.color.get().r, 0.1);
            push_selected_arrow(&mut state.lock().unwrap());
            control.sync(&state.lock().unwrap());
            let c = control.color.get();
            assert!((c.r - 1.0).abs() < f64::EPSILON && (c.g - 0.5).abs() < f64::EPSILON);
            let st = state.lock().unwrap();
            let crate::capture::editor::types::BackgroundStyle::PlainColor(fill) =
                st.background_style
            else {
                panic!("background style should be untouched");
            };
            assert_eq!(fill.r, 0.9);
            control.button.emit_clicked();
            assert!(control.panel.reveals_child());
            control.swatches[0].1.emit_clicked();
            assert!(applied.get());
        }) else {
            return;
        };
    }

    #[test]
    fn text_pill_uses_bundled_tick_and_sync_moves_the_marker_without_picking() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let menus = MenuTracker::default();
            let picked = Rc::new(Cell::new(usize::MAX));
            let pill = text_pill(&menus, &["Detect text", "Freehand"], {
                let picked = picked.clone();
                Rc::new(move |index| picked.set(index))
            });
            let checks = pill.checks.borrow();
            assert_eq!(checks.len(), 2);
            for check in checks.iter() {
                assert_eq!(
                    check.icon_name().as_deref(),
                    Some(icon_names::custom::CHECK_SYMBOLIC)
                );
            }
            assert!(checks[0].property::<bool>("visible"));
            assert!(!checks[1].property::<bool>("visible"));
            drop(checks);
            (pill.set_selected)(1);
            let checks = pill.checks.borrow();
            assert!(!checks[0].property::<bool>("visible"));
            assert!(checks[1].property::<bool>("visible"));
            assert_eq!(picked.get(), usize::MAX);
        }) else {
            return;
        };
    }

    #[test]
    fn effect_mode_option_updates_the_obfuscation_method() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let state = test_state();
            state.lock().unwrap().selected_tool = Tool::Obfuscate;
            let area = DrawingArea::new();
            let menus = MenuTracker::default();
            let rebuild: Rc<dyn Fn()> = Rc::new(|| {});
            let control = EffectMode::new(&state, &area, &rebuild, &menus, None);
            control.options[1].emit_clicked(); // Blur
            let st = state.lock().unwrap();
            assert_eq!(st.obfuscate_method(), ObfuscateMethod::Blur);
        }) else {
            return;
        };
    }
}
