//! Stroke-family inspector panels: pen, arrow, line, highlighter (PR 10.11).

use gtk4::{prelude::*, Box as GtkBox};

use super::shell::{append_inspector_section, build_tool_inspector};

pub(super) struct PenInspectorInputs<'a> {
    pub pen_inspector_list: &'a GtkBox,
}

pub(super) fn build_pen_inspector(input: PenInspectorInputs<'_>) -> GtkBox {
    let (pen_inspector, pen_inspector_content) = build_tool_inspector();
    input
        .pen_inspector_list
        .add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &pen_inspector_content,
        "Thickness",
        input.pen_inspector_list.upcast_ref(),
    );
    pen_inspector
}

pub(super) struct ArrowInspectorInputs<'a> {
    pub arrow_style_list: &'a GtkBox,
    pub arrow_thickness_list: &'a GtkBox,
    pub arrow_behavior_group: &'a GtkBox,
}

pub(super) fn build_arrow_inspector(input: ArrowInspectorInputs<'_>) -> GtkBox {
    let (arrow_inspector, arrow_inspector_content) = build_tool_inspector();
    input
        .arrow_style_list
        .add_css_class("editor-inspector-option-list");
    input
        .arrow_thickness_list
        .add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &arrow_inspector_content,
        "Style",
        input.arrow_style_list.upcast_ref(),
    );
    append_inspector_section(
        &arrow_inspector_content,
        "Thickness",
        input.arrow_thickness_list.upcast_ref(),
    );
    append_inspector_section(
        &arrow_inspector_content,
        "Behavior",
        input.arrow_behavior_group.upcast_ref(),
    );
    arrow_inspector
}

pub(super) struct LineInspectorInputs<'a> {
    pub line_inspector_list: &'a GtkBox,
}

pub(super) fn build_line_inspector(input: LineInspectorInputs<'_>) -> GtkBox {
    let (line_inspector, line_inspector_content) = build_tool_inspector();
    input
        .line_inspector_list
        .add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &line_inspector_content,
        "Thickness",
        input.line_inspector_list.upcast_ref(),
    );
    line_inspector
}

pub(super) struct HighlighterInspectorInputs<'a> {
    pub highlighter_inspector_list: &'a GtkBox,
}

pub(super) fn build_highlighter_inspector(input: HighlighterInspectorInputs<'_>) -> GtkBox {
    let (highlighter_inspector, highlighter_inspector_content) = build_tool_inspector();
    input
        .highlighter_inspector_list
        .add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &highlighter_inspector_content,
        "Thickness",
        input.highlighter_inspector_list.upcast_ref(),
    );
    highlighter_inspector
}
