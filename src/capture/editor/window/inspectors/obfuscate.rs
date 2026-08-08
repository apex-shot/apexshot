//! Obfuscate-tool inspector panel (PR 10.11).

use gtk4::{prelude::*, Box as GtkBox};

use super::shell::{append_inspector_section, build_tool_inspector};

pub(super) struct ObfuscateInspectorInputs<'a> {
    pub obfuscate_method_list: &'a GtkBox,
}

pub(super) fn build_obfuscate_inspector(input: ObfuscateInspectorInputs<'_>) -> GtkBox {
    let (obfuscate_inspector, obfuscate_inspector_content) = build_tool_inspector();
    input
        .obfuscate_method_list
        .add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &obfuscate_inspector_content,
        "Method",
        input.obfuscate_method_list.upcast_ref(),
    );
    obfuscate_inspector
}
