//! The per-tool option bars that dock under the floating tool bar.
//!
//! Every control reads and writes the shared `EditorState`; `sync` re-reads
//! the model after each mutation so labels, checks and sliders track edits,
//! undo and image loads. A bar shows for the armed tool — or, while Select is
//! armed, for the kind of the selected annotation so it can be restyled.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Image, Label, Orientation, Popover, Stack};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use super::annotate::EditorHost;
use super::arrow_control::ArrowControl;
use super::color_control::ColorControl;
use super::effect_mode::EffectMode;
use super::number_control::{NumberControl, NumberControlKind};
use super::stroke_control::StrokeControl;
use super::toolbar::Tool;
use super::ui_support::px;
use crate::capture::editor::pen_weight::HighlighterMode;
use crate::capture::editor::state::EditorState;
use crate::capture::editor::types::{AnnotationAction, CropAspectRatio};
use crate::capture::editor::window::icon_names::custom;
use crate::i18n::t;

const TEXT_SIZE_OPTIONS: [i32; 12] = [12, 14, 16, 18, 20, 24, 28, 32, 36, 48, 64, 72];
const TEXT_FONT_FAMILIES: [&str; 5] = ["Sans", "Serif", "Monospace", "Fantasy", "Cursive"];
const CROP_RATIOS: [(CropAspectRatio, &str); 5] = [
    (CropAspectRatio::Freeform, "Freeform"),
    (CropAspectRatio::Original, "Original"),
    (CropAspectRatio::Square, "1:1"),
    (CropAspectRatio::FourThree, "4:3"),
    (CropAspectRatio::SixteenNine, "16:9"),
];
const HIGHLIGHTER_MODES: [(HighlighterMode, &str); 2] = [
    (HighlighterMode::TextAware, "Detect text"),
    (HighlighterMode::Freehand, "Freehand"),
];
/// Matches the render path's numbering ceiling.
const MAX_NUMBER: u32 = 9999;

type BarContext = (Tool, Option<&'static str>, Option<usize>);
type Refresher = Rc<dyn Fn(&EditorState)>;

pub(super) struct SubBars {
    pub root: Stack,
    syncing: Rc<Cell<bool>>,
    /// name -> refresh closure reading the model into the controls.
    refreshers: HashMap<&'static str, Refresher>,
    on_dock_change: Rc<dyn Fn(bool)>,
    color_panels: Vec<gtk4::Revealer>,
    active_context: Cell<Option<BarContext>>,
}

/// A pill button opening a popover of option rows; `set_selected` moves the
/// check and label without firing the pick callback (used by model sync).
struct Pill {
    button: Button,
    set_selected: Rc<dyn Fn(usize)>,
}

fn pill(scale: f64, options: &[&str], on_pick: Rc<dyn Fn(usize)>) -> Pill {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.add_css_class("image-editor-subbar-pill");

    let text = Label::new(Some(options.first().copied().unwrap_or("")));
    text.add_css_class("image-editor-subbar-pill-label");
    let chevron = gtk4::Image::from_icon_name("chevron-down-regular");
    chevron.set_pixel_size(px(10.0, scale));
    chevron.add_css_class("image-editor-subbar-pill-arrow");
    let content = GtkBox::new(Orientation::Horizontal, px(4.0, scale));
    content.append(&text);
    content.append(&chevron);
    button.set_child(Some(&content));

    let popover = Popover::new();
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    popover.add_css_class("image-editor-subbar-popover");
    let list = GtkBox::new(Orientation::Vertical, 0);
    list.add_css_class("image-editor-subbar-menu");

    let selected = Rc::new(Cell::new(0usize));
    let checks = Rc::new(RefCell::new(Vec::<Image>::new()));
    for (index, option) in options.iter().enumerate() {
        // A bundled check keeps the menu marker on the panel's icon grid
        // instead of a text glyph.
        let check = Image::from_icon_name(custom::CHECK_SYMBOLIC);
        check.set_pixel_size(px(12.0, scale));
        check.add_css_class("image-editor-subbar-check");
        check.set_visible(index == 0);
        let option_label = Label::new(Some(option));
        option_label.set_xalign(0.0);
        option_label.set_hexpand(true);
        let row_box = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
        row_box.append(&option_label);
        row_box.append(&check);
        let row = Button::new();
        row.set_has_frame(false);
        row.add_css_class("image-editor-subbar-option");
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
    button.connect_clicked(move |_| popover.popup());

    let labels: Vec<String> = options.iter().map(|s| s.to_string()).collect();
    let set_selected = {
        let text = text;
        let checks = checks;
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
    Pill {
        button,
        set_selected,
    }
}

/// Wrapper kept for the shell's motion sections, which only need the
/// self-contained pill (selection stays local to the pill).
pub(super) fn option_pill(scale: f64, initial: &str, options: &[&str]) -> Button {
    let pill = pill(scale, options, Rc::new(|_| {}));
    if let Some(index) = options.iter().position(|o| *o == initial) {
        (pill.set_selected)(index);
    }
    pill.button
}

fn bar() -> GtkBox {
    let root = GtkBox::new(Orientation::Horizontal, 6);
    root.add_css_class("image-editor-sub-bar");
    root
}

pub(super) fn build(
    scale: f64,
    host: Rc<EditorHost>,
    on_dock_change: Rc<dyn Fn(bool)>,
    canvas: Option<std::rc::Weak<super::canvas::Canvas>>,
) -> SubBars {
    let syncing = Rc::new(Cell::new(false));
    let root = Stack::new();
    root.add_css_class("image-editor-sub-bars");
    root.set_hhomogeneous(false);
    root.set_vhomogeneous(false);
    root.set_halign(Align::Center);
    root.set_valign(Align::Start);
    root.set_margin_top(px(56.0, scale));
    root.set_visible(false);

    let mut refreshers: HashMap<&'static str, Refresher> = HashMap::new();
    let mut color_panels = Vec::new();

    // ----- crop: ratio pill, dimensions, Cancel/Apply -----
    let (crop, crop_refresh) = {
        let root = bar();
        let ratio_options: Vec<String> = CROP_RATIOS.iter().map(|(_, label)| t(label)).collect();
        let ratio_labels: Vec<&str> = ratio_options.iter().map(String::as_str).collect();
        let ratio = pill(scale, &ratio_labels, {
            let host = host.clone();
            Rc::new(move |index| {
                if let Some((ratio, _)) = CROP_RATIOS.get(index) {
                    host.mutate(|st| st.set_crop_ratio(*ratio));
                }
            })
        });
        root.append(&ratio.button);
        let dimensions = Label::new(Some("—"));
        dimensions.add_css_class("image-editor-subbar-dimensions");
        root.append(&dimensions);
        let cancel = Button::with_label(&t("Cancel"));
        cancel.add_css_class("image-editor-subbar-button");
        cancel.connect_clicked({
            let host = host.clone();
            move |_| host.cancel_crop()
        });
        let apply = Button::with_label(&t("Apply"));
        apply.add_css_class("image-editor-subbar-button");
        apply.add_css_class("image-editor-subbar-apply");
        apply.connect_clicked({
            let host = host.clone();
            move |_| host.apply_crop()
        });
        root.append(&cancel);
        root.append(&apply);
        let refresh = {
            let set_ratio = ratio.set_selected.clone();
            let dimensions = dimensions;
            let apply = apply;
            Rc::new(move |st: &EditorState| {
                if let Some(index) = CROP_RATIOS.iter().position(|(r, _)| *r == st.crop_ratio) {
                    set_ratio(index);
                }
                match st.crop_rect {
                    Some(rect) => {
                        dimensions.set_text(&format!("{} × {}", rect.width, rect.height));
                        apply.set_sensitive(super::annotate::crop_apply_enabled(st));
                    }
                    None => {
                        dimensions.set_text("—");
                        apply.set_sensitive(false);
                    }
                }
            })
        };
        (root, refresh)
    };
    root.add_named(&crop, Some("crop"));
    refreshers.insert("crop", crop_refresh);

    // ----- stroke tools (pen, line, box, circle): presets + colour -----
    let (stroke, stroke_refresh) = {
        let root = bar();
        let weight = StrokeControl::new(scale, &host);
        root.append(&weight.button);
        weight.anchor_to(&root);
        let colour = ColorControl::new(scale, &host, canvas.clone());
        root.prepend(&colour.button);
        weight.button.connect_clicked({
            let panel = colour.panel.downgrade();
            move |_| {
                if let Some(panel) = panel.upgrade() {
                    panel.set_reveal_child(false);
                }
            }
        });
        let wrapped = colour.wrap(&root);
        color_panels.push(colour.panel.clone());
        let refresh = {
            let weight = weight;
            let colour = colour;
            Rc::new(move |st: &EditorState| {
                weight.sync(st);
                colour.sync(st);
            })
        };
        (wrapped, refresh)
    };
    root.add_named(&stroke, Some("stroke"));
    refreshers.insert("stroke", stroke_refresh);

    // ----- arrow: style, reverse, stroke, colour -----
    let (arrow, arrow_refresh) = {
        let root = bar();
        let style = ArrowControl::new(scale, &host);
        root.append(&style.button);
        style.anchor_to(&root);
        let reverse = Button::with_label(&t("Reverse"));
        reverse.add_css_class("image-editor-subbar-button");
        reverse.add_css_class("image-editor-arrow-reverse");
        reverse.connect_clicked({
            let host = host.clone();
            move |_| {
                host.mutate(|st| {
                    if !st.reverse_selected_arrow_action() {
                        st.inverse_arrow_direction = !st.inverse_arrow_direction;
                    }
                });
            }
        });
        root.append(&reverse);
        let weight = StrokeControl::new(scale, &host);
        root.append(&weight.button);
        weight.anchor_to(&root);
        let colour = ColorControl::new(scale, &host, canvas.clone());
        root.prepend(&colour.button);
        style.button.connect_clicked({
            let panel = colour.panel.downgrade();
            move |_| {
                if let Some(panel) = panel.upgrade() {
                    panel.set_reveal_child(false);
                }
            }
        });
        weight.button.connect_clicked({
            let panel = colour.panel.downgrade();
            move |_| {
                if let Some(panel) = panel.upgrade() {
                    panel.set_reveal_child(false);
                }
            }
        });
        let wrapped = colour.wrap(&root);
        color_panels.push(colour.panel.clone());
        let refresh = {
            let style = style;
            let weight = weight;
            let reverse = reverse;
            let colour = colour;
            Rc::new(move |st: &EditorState| {
                colour.sync(st);
                style.sync(st);
                let reversed = st
                    .selected_arrow_reversed()
                    .unwrap_or(st.inverse_arrow_direction);
                if reversed {
                    reverse.add_css_class("selected");
                } else {
                    reverse.remove_css_class("selected");
                }
                reverse.update_state(&[gtk4::accessible::State::Pressed(if reversed {
                    gtk4::AccessibleTristate::True
                } else {
                    gtk4::AccessibleTristate::False
                })]);
                weight.sync(st);
                reverse.set_sensitive(true);
            })
        };
        (wrapped, refresh)
    };
    root.add_named(&arrow, Some("arrow"));
    refreshers.insert("arrow", arrow_refresh);

    // ----- highlighter: mode + weight + colour -----
    let (highlighter, highlighter_refresh) = {
        let root = bar();
        let mode_options: Vec<String> = HIGHLIGHTER_MODES
            .iter()
            .map(|(_, label)| t(label))
            .collect();
        let mode_labels: Vec<&str> = mode_options.iter().map(String::as_str).collect();
        let mode = pill(scale, &mode_labels, {
            let host = host.clone();
            Rc::new(move |index| {
                if let Some((mode, _)) = HIGHLIGHTER_MODES.get(index) {
                    host.mutate(|st| {
                        st.set_highlighter_mode_and_check(*mode);
                    });
                }
            })
        });
        root.append(&mode.button);
        let weight = StrokeControl::new(scale, &host);
        root.append(&weight.button);
        weight.anchor_to(&root);
        let colour = ColorControl::new(scale, &host, canvas.clone());
        root.prepend(&colour.button);
        weight.button.connect_clicked({
            let panel = colour.panel.downgrade();
            move |_| {
                if let Some(panel) = panel.upgrade() {
                    panel.set_reveal_child(false);
                }
            }
        });
        let wrapped = colour.wrap(&root);
        color_panels.push(colour.panel.clone());
        let refresh = {
            let set_mode = mode.set_selected.clone();
            let weight = weight;
            let colour = colour;
            Rc::new(move |st: &EditorState| {
                colour.sync(st);
                if let Some(index) = HIGHLIGHTER_MODES
                    .iter()
                    .position(|(m, _)| *m == st.highlighter_mode)
                {
                    set_mode(index);
                }
                weight.sync(st);
            })
        };
        (wrapped, refresh)
    };
    root.add_named(&highlighter, Some("highlighter"));
    refreshers.insert("highlighter", highlighter_refresh);

    // ----- text: family + size + colour -----
    let (text, text_refresh) = {
        let root = bar();
        let family = pill(scale, &TEXT_FONT_FAMILIES, {
            let host = host.clone();
            Rc::new(move |index| {
                if let Some(family) = TEXT_FONT_FAMILIES.get(index) {
                    host.mutate(|st| {
                        st.text_font_family = family.to_string();
                        st.set_selected_text_font_family(family.to_string());
                    });
                }
            })
        });
        root.append(&family.button);
        let size_options: Vec<String> = TEXT_SIZE_OPTIONS
            .iter()
            .map(|size| format!("{size}pt"))
            .collect();
        let size_refs: Vec<&str> = size_options.iter().map(|s| s.as_str()).collect();
        let size = pill(scale, &size_refs, {
            let host = host.clone();
            Rc::new(move |index| {
                if let Some(size) = TEXT_SIZE_OPTIONS.get(index) {
                    host.mutate(|st| {
                        super::annotate::set_text_size_and_fit(st, *size as f64);
                    });
                }
            })
        });
        root.append(&size.button);
        let colour = ColorControl::new(scale, &host, canvas.clone());
        root.prepend(&colour.button);
        let wrapped = colour.wrap(&root);
        color_panels.push(colour.panel.clone());
        let refresh = {
            let set_family = family.set_selected.clone();
            let set_size = size.set_selected.clone();
            let colour = colour;
            Rc::new(move |st: &EditorState| {
                colour.sync(st);
                if let Some(index) = TEXT_FONT_FAMILIES
                    .iter()
                    .position(|f| *f == st.text_font_family)
                {
                    set_family(index);
                }
                let size = st.selected_text_action_size().unwrap_or(st.text_size) as i32;
                if let Some(index) = TEXT_SIZE_OPTIONS.iter().position(|s| *s == size) {
                    set_size(index);
                }
            })
        };
        (wrapped, refresh)
    };
    root.add_named(&text, Some("text"));
    refreshers.insert("text", text_refresh);

    // ----- number: style + start stepper + size + colour -----
    let (number, number_refresh) = {
        let root = bar();
        let style = NumberControl::new(scale, &host, NumberControlKind::Style);
        root.append(&style.button);
        style.anchor_to(&root);

        // The `- [n] +` start stepper; edits update the armed start or the
        // selected marker's own number.
        let stepper_row = GtkBox::new(Orientation::Horizontal, 0);
        stepper_row.add_css_class("image-editor-subbar-stepper");
        let entry = gtk4::Entry::new();
        entry.set_text("1");
        entry.set_width_chars(3);
        entry.set_max_width_chars(4);
        gtk4::prelude::EditableExt::set_alignment(&entry, 0.5);
        entry.add_css_class("image-editor-subbar-entry");
        for (label, delta) in [("−", -1i64), ("+", 1)] {
            let step = Button::with_label(label);
            step.add_css_class("image-editor-subbar-step");
            step.set_focusable(false);
            step.connect_clicked({
                let host = host.clone();
                move |_| {
                    host.mutate(|st| {
                        let next = (i64::from(st.active_number_start()) + delta)
                            .clamp(1, i64::from(MAX_NUMBER));
                        st.set_active_number_start(next as u32);
                    });
                }
            });
            if delta < 0 {
                stepper_row.append(&step);
                stepper_row.append(&entry);
            } else {
                stepper_row.append(&step);
            }
        }
        entry.connect_changed({
            let host = host.clone();
            let syncing = syncing.clone();
            move |entry| {
                if syncing.get() {
                    return;
                }
                if let Ok(value) = entry.text().parse::<u32>() {
                    host.mutate(|st| {
                        st.set_active_number_start(value.clamp(1, MAX_NUMBER));
                    });
                }
            }
        });
        root.append(&stepper_row);

        let size = NumberControl::new(scale, &host, NumberControlKind::Size);
        root.append(&size.button);
        size.anchor_to(&root);
        let colour = ColorControl::new(scale, &host, canvas.clone());
        root.prepend(&colour.button);
        for button in [&style.button, &size.button] {
            let panel = colour.panel.downgrade();
            button.connect_clicked(move |_| {
                if let Some(panel) = panel.upgrade() {
                    panel.set_reveal_child(false);
                }
            });
        }
        let wrapped = colour.wrap(&root);
        color_panels.push(colour.panel.clone());
        let refresh = {
            let style = style;
            let size = size;
            let entry = entry;
            let colour = colour;
            Rc::new(move |st: &EditorState| {
                colour.sync(st);
                style.sync(st);
                size.sync(st);
                let display = st.active_number_start_display();
                if entry.text().as_str() != display {
                    entry.set_text(&display);
                }
            })
        };
        (wrapped, refresh)
    };
    root.add_named(&number, Some("number"));
    refreshers.insert("number", number_refresh);

    // ----- obfuscate: method (intensity is anchored to the selected region) -----
    let (obfuscate, obfuscate_refresh) = {
        let root = bar();
        let method = EffectMode::new(scale, &host);
        root.append(&method.button);
        method.anchor_to(&root);
        let refresh = Rc::new(move |st: &EditorState| method.sync(st));
        (root, refresh)
    };
    root.add_named(&obfuscate, Some("obfuscate"));
    refreshers.insert("obfuscate", obfuscate_refresh);

    // ----- focus: intensity is anchored to the selected region -----
    let (focus, focus_refresh) = {
        let root = bar();
        let label = Label::new(Some(&t("Focus")));
        label.add_css_class("image-editor-subbar-pill-label");
        root.append(&label);
        let refresh = Rc::new(move |_: &EditorState| {});
        (root, refresh)
    };
    root.add_named(&focus, Some("focus"));
    refreshers.insert("focus", focus_refresh);

    SubBars {
        root,
        syncing,
        refreshers,
        on_dock_change,
        color_panels,
        active_context: Cell::new(None),
    }
}

/// Which bar an action kind restyles through.
fn bar_name_for_action(action: &AnnotationAction) -> Option<&'static str> {
    match action {
        AnnotationAction::Pen { .. } | AnnotationAction::Line { .. } => Some("stroke"),
        AnnotationAction::Highlighter { .. } => Some("highlighter"),
        AnnotationAction::Arrow { .. } => Some("arrow"),
        AnnotationAction::Box { .. } | AnnotationAction::Circle { .. } => Some("stroke"),
        AnnotationAction::Text { .. } => Some("text"),
        AnnotationAction::Number { .. } => Some("number"),
        AnnotationAction::Obfuscate { .. } => Some("obfuscate"),
        AnnotationAction::Focus { .. } => Some("focus"),
    }
}

fn bar_name_for_tool(tool: Tool) -> Option<&'static str> {
    match tool {
        Tool::Crop => Some("crop"),
        Tool::Pen | Tool::Line | Tool::Box | Tool::Circle => Some("stroke"),
        Tool::Arrow => Some("arrow"),
        Tool::Highlighter => Some("highlighter"),
        Tool::Text => Some("text"),
        Tool::Number => Some("number"),
        Tool::Obfuscate => Some("obfuscate"),
        Tool::Focus => Some("focus"),
        Tool::Select => None,
    }
}

impl SubBars {
    pub(super) fn close_color_panels(&self) -> bool {
        let mut closed = false;
        for panel in &self.color_panels {
            if panel.reveals_child() {
                panel.set_reveal_child(false);
                closed = true;
            }
        }
        closed
    }

    /// Re-read the model into the visible bar after every document change;
    /// also decides which bar shows (armed tool or compatible selection).
    pub(super) fn sync(&self, st: Option<&EditorState>) {
        self.refresh_visible(st);
    }

    fn refresh_visible(&self, st: Option<&EditorState>) {
        // The model's tool is authoritative: crop Apply/Cancel, keyboard
        // shortcuts and loads all move the rail and bars together. No
        // document means no bar.
        let Some(st) = st else {
            self.active_context.set(None);
            for panel in &self.color_panels {
                panel.set_reveal_child(false);
            }
            self.root.set_visible(false);
            return;
        };
        self.root.set_visible(true);
        let tool = Tool::from_model(st.selected_tool);
        // Compatible selected annotations show their own bar so an
        // existing arrow/text etc. stays restylable under Select.
        let name = match tool {
            Tool::Select => st.selected_action().and_then(bar_name_for_action),
            Tool::Arrow => st
                .selected_action()
                .filter(|a| matches!(a, AnnotationAction::Arrow { .. }))
                .and_then(bar_name_for_action)
                .or(Some("arrow")),
            Tool::Text => st
                .selected_action()
                .filter(|a| matches!(a, AnnotationAction::Text { .. }))
                .and_then(bar_name_for_action)
                .or(Some("text")),
            _ => bar_name_for_tool(tool),
        };
        let context = Some((tool, name, st.selected_action_index));
        if self.active_context.replace(context) != context {
            for panel in &self.color_panels {
                panel.set_reveal_child(false);
            }
        }
        match name {
            Some(name) => {
                self.root.set_visible_child_name(name);
                self.root.set_visible(true);
                (self.on_dock_change)(true);
                if let Some(refresh) = self.refreshers.get(name) {
                    self.syncing.set(true);
                    refresh(st);
                    self.syncing.set(false);
                }
            }
            None => {
                self.root.set_visible(false);
                (self.on_dock_change)(false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn contains(widget: &gtk4::Widget, predicate: &dyn Fn(&gtk4::Widget) -> bool) -> bool {
        if predicate(widget) {
            return true;
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if contains(&current, predicate) {
                return true;
            }
            child = current.next_sibling();
        }
        false
    }

    fn find_class(widget: &gtk4::Widget, class: &str) -> Option<gtk4::Widget> {
        if widget.has_css_class(class) {
            return Some(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(found) = find_class(&current, class) {
                return Some(found);
            }
            child = current.next_sibling();
        }
        None
    }

    #[test]
    fn reverse_button_tracks_default_and_selected_arrow() {
        use crate::capture::editor::types::Point;

        let Some(()) = crate::test_support::with_gtk(|| {
            let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
            let mut state = EditorState::new(RgbaImage::new(100, 100));
            state.selected_tool = Tool::Arrow.to_model();
            *host.editor.borrow_mut() = Some(state);
            let bars = build(1.0, host.clone(), Rc::new(|_| {}), None);
            let wrapper = bars.root.child_by_name("arrow").unwrap();
            let reverse = find_class(&wrapper, "image-editor-arrow-reverse")
                .unwrap()
                .downcast::<Button>()
                .unwrap();
            host.read(|st| bars.sync(Some(st)));
            assert!(!reverse.has_css_class("selected"));
            reverse.emit_clicked();
            host.read(|st| bars.sync(Some(st)));
            assert!(reverse.has_css_class("selected"));
            host.mutate(|st| {
                st.begin_drag(Point { x: 10.0, y: 10.0 });
                st.update_drag(Point { x: 80.0, y: 80.0 });
                let arrow = st.finalize_drag_action().unwrap();
                st.push_action(arrow);
                st.selected_action_index = Some(0);
            });
            host.read(|st| bars.sync(Some(st)));
            assert!(reverse.has_css_class("selected"));
            reverse.emit_clicked();
            host.read(|st| bars.sync(Some(st)));
            assert!(!reverse.has_css_class("selected"));
            host.mutate(|st| {
                assert!(st.undo());
                st.selected_action_index = Some(0);
            });
            host.read(|st| bars.sync(Some(st)));
            assert!(reverse.has_css_class("selected"));
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn number_bar_uses_visual_menus_and_preserves_stepper_sync() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
            let mut state = EditorState::new(RgbaImage::new(32, 32));
            state.selected_tool = Tool::Number.to_model();
            state.next_number = 19;
            *host.editor.borrow_mut() = Some(state);
            let bars = build(1.0, host.clone(), Rc::new(|_| {}), None);
            host.read(|st| bars.sync(Some(st)));
            let wrapper = bars.root.child_by_name("number").unwrap();
            assert_eq!(bars.root.visible_child_name().as_deref(), Some("number"));
            assert!(find_class(&wrapper, "image-editor-number-style-trigger").is_some());
            assert!(find_class(&wrapper, "image-editor-number-size-trigger").is_some());
            let entry = find_class(&wrapper, "image-editor-subbar-entry")
                .unwrap()
                .downcast::<gtk4::Entry>()
                .unwrap();
            assert_eq!(entry.text(), "19");
            let style_option = find_class(&wrapper, "image-editor-number-style-option")
                .unwrap()
                .downcast::<Button>()
                .unwrap();
            assert!(style_option.has_css_class("selected"));
            host.read(|st| {
                assert_eq!(st.next_number, 19);
                assert!(!st.can_undo());
            });
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn stroke_tool_bars_use_presets_without_a_size_slider() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
            *host.editor.borrow_mut() = Some(EditorState::new(RgbaImage::new(32, 32)));
            let bars = build(1.0, host.clone(), Rc::new(|_| {}), None);
            for (tool, name) in [
                (Tool::Pen, "stroke"),
                (Tool::Line, "stroke"),
                (Tool::Box, "stroke"),
                (Tool::Circle, "stroke"),
                (Tool::Arrow, "arrow"),
                (Tool::Highlighter, "highlighter"),
            ] {
                host.mutate(|st| {
                    st.selected_tool = tool.to_model();
                    st.stroke_size = 6.0;
                });
                host.read(|st| bars.sync(Some(st)));
                assert_eq!(bars.root.visible_child_name().as_deref(), Some(name));
                let wrapper = bars.root.child_by_name(name).unwrap();
                let bar = wrapper.first_child().unwrap();
                assert!(contains(&bar, &|widget| widget
                    .has_css_class("image-editor-stroke-trigger")));
                assert!(!contains(&bar, &|widget| widget.is::<gtk4::Scale>()));
                assert_eq!(host.read(|st| st.stroke_size), Some(6.0));
            }
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
