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
    /// The rail button for a model tool, for model-authoritative sync.
    /// Background mode has no rail button: the canvas ignores pointer input
    /// there, so highlighting Select would claim a tool that is not armed.
    pub(super) fn rail_from_model(tool: crate::capture::editor::types::Tool) -> Option<Self> {
        use crate::capture::editor::types::Tool as Model;
        match tool {
            Model::Background => None,
            Model::Select => Some(Self::Select),
            Model::Crop => Some(Self::Crop),
            Model::Pen => Some(Self::Pen),
            Model::Highlighter => Some(Self::Highlighter),
            Model::Arrow => Some(Self::Arrow),
            Model::Line => Some(Self::Line),
            Model::Box => Some(Self::Box),
            Model::Circle => Some(Self::Circle),
            Model::Text => Some(Self::Text),
            Model::Number => Some(Self::Number),
            Model::Obfuscate => Some(Self::Obfuscate),
            Model::Focus => Some(Self::Focus),
        }
    }

    /// The sub-bars' tool for a model tool. Background mode still needs a
    /// page, so it falls back to Select's.
    pub(super) fn from_model(tool: crate::capture::editor::types::Tool) -> Self {
        Self::rail_from_model(tool).unwrap_or(Self::Select)
    }

    /// The shared editor's tool this rail button arms.
    pub(super) fn to_model(self) -> crate::capture::editor::types::Tool {
        use crate::capture::editor::types::Tool as Model;
        match self {
            Self::Select => Model::Select,
            Self::Crop => Model::Crop,
            Self::Pen => Model::Pen,
            Self::Highlighter => Model::Highlighter,
            Self::Arrow => Model::Arrow,
            Self::Line => Model::Line,
            Self::Box => Model::Box,
            Self::Circle => Model::Circle,
            Self::Text => Model::Text,
            Self::Number => Model::Number,
            Self::Obfuscate => Model::Obfuscate,
            Self::Focus => Model::Focus,
        }
    }

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
    /// History pill buttons the window wires to the document.
    pub undo: Button,
    pub redo: Button,
    pub delete: Button,
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
                    set_active(&buttons, Some(tool));
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

    let mut history_buttons = Vec::new();
    for (icon, label) in [
        (custom::EDIT_UNDO_SYMBOLIC, t("Undo")),
        (custom::EDIT_UNDO_RTL_SYMBOLIC, t("Redo")),
        (custom::USER_TRASH_SYMBOLIC, t("Delete")),
    ] {
        let button = tool_button(icon, &label, icon_size);
        button.set_sensitive(false);
        button.set_tooltip_text(Some(&label));
        history.append(&button);
        history_buttons.push(button);
    }
    let mut history_iter = history_buttons.into_iter();
    let undo = history_iter.next().unwrap();
    let redo = history_iter.next().unwrap();
    let delete = history_iter.next().unwrap();

    let toolbar = Toolbar {
        root,
        history,
        undo,
        redo,
        delete,
        buttons,
    };
    toolbar.set_active(initial);
    toolbar
}

impl Toolbar {
    /// Move the active highlight; used when the model's tool changes from
    /// elsewhere (crop apply/cancel, shortcuts, loads). `None` clears every
    /// button, which is how Background mode shows no armed tool.
    pub(super) fn set_active_tool(&self, tool: Option<Tool>) {
        set_active(&self.buttons, tool);
    }

    pub(super) fn set_active(&self, tool: Tool) {
        set_active(&self.buttons, Some(tool));
    }
}

fn set_active(buttons: &Rc<RefCell<Vec<(Tool, Button)>>>, tool: Option<Tool>) {
    for (candidate, button) in buttons.borrow().iter() {
        if Some(*candidate) == tool {
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

    #[test]
    fn background_mode_leaves_the_rail_unhighlighted() {
        use crate::capture::editor::types::Tool as Model;
        assert_eq!(Tool::rail_from_model(Model::Background), None);
        assert_eq!(Tool::rail_from_model(Model::Select), Some(Tool::Select));
        assert_eq!(Tool::rail_from_model(Model::Crop), Some(Tool::Crop));
    }
}
