//! The floating annotation tool bar and the history pill.
//!
//! The tool rail is grouped the way the Linux editor groups its tools:
//! selection, drawing, shapes, content and effects, each a subgroup with a
//! hairline between them. Undo, redo and delete live in their own pill at the
//! canvas' bottom-right; the editing model that will sit behind them arrives
//! in later slices.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

use super::ui_support::{px, tool_button, toolbar_rule};
use crate::capture::editor::window::icon_names::{custom, shipped};
use crate::i18n::t;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Tool {
    Select,
    Crop,
    Pen,
    Highlighter,
    Arrow,
    Line,
    Box,
    Circle,
    Text,
    Number,
    Obfuscate,
    Focus,
}

/// The tool rail's groups, in the order and grouping the Linux editor uses.
const TOOL_GROUPS: [&[Tool]; 5] = [
    &[Tool::Select, Tool::Crop],
    &[Tool::Pen, Tool::Highlighter],
    &[Tool::Arrow, Tool::Line, Tool::Box, Tool::Circle],
    &[Tool::Text, Tool::Number],
    &[Tool::Obfuscate, Tool::Focus],
];

impl Tool {
    pub(super) fn label(self) -> String {
        match self {
            Self::Select => t("Select"),
            Self::Crop => t("Crop"),
            Self::Pen => t("Pen"),
            Self::Highlighter => t("Highlighter"),
            Self::Arrow => t("Arrow"),
            Self::Line => t("Line"),
            Self::Box => t("Box"),
            Self::Circle => t("Circle"),
            Self::Text => t("Text"),
            Self::Number => t("Number"),
            Self::Obfuscate => t("Obfuscate"),
            Self::Focus => t("Focus"),
        }
    }

    /// The same icons the Linux toolbar draws: the app's symbolic set where it
    /// ships one, the bundled fluent glyph otherwise.
    fn icon(self) -> &'static str {
        match self {
            Self::Select => custom::SELECT_MODE_SYMBOLIC,
            Self::Crop => custom::CROP_SYMBOLIC,
            Self::Pen => custom::PENCIL_SYMBOLIC,
            Self::Highlighter => custom::MARKER_SYMBOLIC,
            Self::Arrow => custom::ARROW2_TOP_RIGHT_SYMBOLIC,
            Self::Line => custom::FUNCTION_LINEAR_SYMBOLIC,
            Self::Box => custom::SQUARE_OUTLINE_THIN_SYMBOLIC,
            Self::Circle => custom::CIRCLE_OUTLINE_THIN_SYMBOLIC,
            Self::Text => custom::FONT_X_GENERIC_SYMBOLIC,
            Self::Number => shipped::NUMBER_CIRCLE_1_REGULAR,
            Self::Obfuscate => custom::BLUR_ALT_SYMBOLIC,
            Self::Focus => custom::BACKGROUND_APP_RECTANGULAR_SYMBOLIC,
        }
    }
}

pub(super) struct Toolbar {
    pub root: GtkBox,
    /// The floating undo/redo/delete pill docked at the canvas' bottom-right.
    pub history: GtkBox,
    buttons: Rc<RefCell<Vec<(Tool, Button)>>>,
}

pub(super) fn build(scale: f64, initial: Tool, on_select: Rc<dyn Fn(Tool)>) -> Toolbar {
    let root = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
    root.add_css_class("image-editor-toolbar");
    root.set_halign(Align::Center);
    root.set_valign(Align::Start);
    root.set_margin_top(px(8.0, scale));

    let buttons: Rc<RefCell<Vec<(Tool, Button)>>> = Rc::new(RefCell::new(Vec::new()));
    let icon_size = px(16.0, scale);

    for (index, group) in TOOL_GROUPS.iter().enumerate() {
        if index > 0 {
            root.append(&toolbar_rule(scale));
        }
        let subgroup = GtkBox::new(Orientation::Horizontal, px(1.0, scale));
        for tool in group.iter().copied() {
            let button = tool_button(tool.icon(), &tool.label(), icon_size);
            button.connect_clicked({
                let buttons = buttons.clone();
                let on_select = on_select.clone();
                move |_| {
                    set_active(&buttons, tool);
                    on_select(tool);
                }
            });
            subgroup.append(&button);
            buttons.borrow_mut().push((tool, button));
        }
        root.append(&subgroup);
    }

    let history = GtkBox::new(Orientation::Horizontal, px(2.0, scale));
    history.add_css_class("image-editor-history-pill");
    history.set_halign(Align::End);
    history.set_valign(Align::End);
    history.set_margin_end(px(16.0, scale));
    history.set_margin_bottom(px(16.0, scale));

    for (icon, label) in [
        (custom::EDIT_UNDO_SYMBOLIC, t("Undo")),
        (custom::EDIT_UNDO_RTL_SYMBOLIC, t("Redo")),
        (custom::USER_TRASH_SYMBOLIC, t("Delete")),
    ] {
        let button = tool_button(icon, &label, icon_size);
        button.set_sensitive(false);
        button.set_tooltip_text(Some(&format!(
            "{label}: {}",
            t("not available in the Windows image editor yet")
        )));
        history.append(&button);
    }

    let toolbar = Toolbar {
        root,
        history,
        buttons,
    };
    toolbar.set_active(initial);
    toolbar
}

impl Toolbar {
    pub(super) fn set_active(&self, tool: Tool) {
        set_active(&self.buttons, tool);
    }
}

fn set_active(buttons: &Rc<RefCell<Vec<(Tool, Button)>>>, tool: Tool) {
    for (candidate, button) in buttons.borrow().iter() {
        if *candidate == tool {
            button.add_css_class("image-editor-tool-active");
        } else {
            button.remove_css_class("image-editor-tool-active");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Tool, TOOL_GROUPS};

    #[test]
    fn every_tool_has_a_label() {
        for tool in TOOL_GROUPS.iter().flat_map(|group| group.iter()) {
            assert!(!tool.label().trim().is_empty(), "{tool:?} has no label");
        }
    }

    #[test]
    fn the_tool_row_starts_with_select_and_ends_with_focus() {
        assert_eq!(TOOL_GROUPS.first().unwrap().first(), Some(&Tool::Select));
        assert_eq!(TOOL_GROUPS.last().unwrap().last(), Some(&Tool::Focus));
    }
}
