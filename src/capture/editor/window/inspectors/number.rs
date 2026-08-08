//! Number-tool inspector panel (PR 10.11).

use gtk4::{prelude::*, Box as GtkBox};

use super::shell::{append_inspector_section, build_tool_inspector};

pub(super) struct NumberInspectorInputs<'a> {
    pub number_options_list: &'a GtkBox,
    pub number_start_row: &'a GtkBox,
    pub number_size_list: &'a GtkBox,
}

pub(super) fn build_number_inspector(input: NumberInspectorInputs<'_>) -> GtkBox {
    let (number_inspector, number_inspector_content) = build_tool_inspector();
    input
        .number_options_list
        .add_css_class("editor-inspector-option-list");
    input
        .number_size_list
        .add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &number_inspector_content,
        "Style",
        input.number_options_list.upcast_ref(),
    );
    append_inspector_section(
        &number_inspector_content,
        "Start",
        input.number_start_row.upcast_ref(),
    );
    append_inspector_section(
        &number_inspector_content,
        "Size",
        input.number_size_list.upcast_ref(),
    );
    number_inspector
}
