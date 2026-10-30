//! The floating annotation tool bar.
//!
//! The tools are the Windows build's visible set for the image editor. They
//! take selection state and drive the inspector's context; the painting and
//! editing model that will sit behind them arrive in later slices.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

use super::ui_support::{px, tool_button, toolbar_rule};
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

/// The tool row, in the order the Linux editor groups its tools.
const PRIMARY_TOOLS: [Tool; 12] = [
    Tool::Select,
    Tool::Crop,
    Tool::Pen,
    Tool::Highlighter,
    Tool::Arrow,
    Tool::Line,
    Tool::Box,
    Tool::Circle,
    Tool::Text,
    Tool::Number,
    Tool::Obfuscate,
    Tool::Focus,
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

    pub(super) fn hint(self) -> String {
        match self {
            Self::Select => t("Select, move, or pan an annotation."),
            Self::Crop => t("Trim the screenshot to the chosen area."),
            Self::Pen => t("Draw a freehand stroke."),
            Self::Highlighter => t("Mark an area with translucent ink."),
            Self::Arrow => t("Point at something with an arrow."),
            Self::Line => t("Draw a straight line."),
            Self::Box => t("Outline a region with a rectangle."),
            Self::Circle => t("Outline a region with an ellipse."),
            Self::Text => t("Place a text label."),
            Self::Number => t("Place a numbered marker."),
            Self::Obfuscate => t("Hide sensitive details in a region."),
            Self::Focus => t("Dim everything outside a region."),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Crop => "crop",
            Self::Pen => "pen-regular",
            Self::Highlighter => "highlight-regular",
            Self::Arrow => "arrow-up-right-regular",
            Self::Line => "draw-line",
            Self::Box => "rectangle-landscape-regular",
            Self::Circle => "circle-regular",
            Self::Text => "text-t-regular",
            Self::Number => "number-circle-1-regular",
            Self::Obfuscate => "blur",
            Self::Focus => "small-rectangle-in-focus",
        }
    }
}

pub(super) struct Toolbar {
    pub root: GtkBox,
    buttons: Rc<RefCell<Vec<(Tool, Button)>>>,
}

pub(super) fn build(scale: f64, initial: Tool, on_select: Rc<dyn Fn(Tool)>) -> Toolbar {
    let root = GtkBox::new(Orientation::Horizontal, px(2.0, scale));
    root.add_css_class("image-editor-toolbar");
    root.set_halign(Align::Center);
    root.set_valign(Align::Start);
    root.set_margin_top(px(8.0, scale));

    let buttons: Rc<RefCell<Vec<(Tool, Button)>>> = Rc::new(RefCell::new(Vec::new()));
    let icon_size = px(16.0, scale);

    for (index, tool) in PRIMARY_TOOLS.iter().copied().enumerate() {
        let button = tool_button(tool.icon(), &tool.label(), icon_size);
        button.connect_clicked({
            let buttons = buttons.clone();
            let on_select = on_select.clone();
            move |_| {
                set_active(&buttons, tool);
                on_select(tool);
            }
        });
        root.append(&button);
        buttons.borrow_mut().push((tool, button));

        // Group the selection tools apart from the drawing tools.
        if index == 1 {
            root.append(&toolbar_rule(scale));
        }
    }

    root.append(&toolbar_rule(scale));

    for (icon, label) in [
        ("arrow-undo-regular", t("Undo")),
        ("arrow-redo-regular", t("Redo")),
        ("delete-regular", t("Delete")),
    ] {
        let button = tool_button(icon, &label, icon_size);
        button.set_sensitive(false);
        button.set_tooltip_text(Some(&format!(
            "{label}: {}",
            t("not available in the Windows image editor yet")
        )));
        root.append(&button);
    }

    let toolbar = Toolbar { root, buttons };
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
    use super::{Tool, PRIMARY_TOOLS};

    #[test]
    fn every_tool_has_a_label_and_a_hint() {
        for tool in PRIMARY_TOOLS {
            assert!(!tool.label().trim().is_empty(), "{tool:?} has no label");
            assert!(!tool.hint().trim().is_empty(), "{tool:?} has no hint");
        }
    }

    #[test]
    fn the_tool_row_starts_with_select_and_ends_with_focus() {
        assert_eq!(PRIMARY_TOOLS.first(), Some(&Tool::Select));
        assert_eq!(PRIMARY_TOOLS.last(), Some(&Tool::Focus));
    }
}
