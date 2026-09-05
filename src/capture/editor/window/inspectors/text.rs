//! Text-tool inspector panel (PR 10.11).

use gtk4::{prelude::*, Box as GtkBox};

use super::shell::{append_inspector_section, build_tool_inspector};
use crate::i18n::t;

pub(super) struct TextInspectorInputs<'a> {
    pub text_size_list: &'a GtkBox,
    pub font_family_list: &'a GtkBox,
}

pub(super) fn build_text_inspector(input: TextInspectorInputs<'_>) -> GtkBox {
    let (text_inspector, text_inspector_content) = build_tool_inspector();
    input
        .text_size_list
        .add_css_class("editor-inspector-option-list");
    input
        .font_family_list
        .add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &text_inspector_content,
        &t("Size"),
        input.text_size_list.upcast_ref(),
    );
    append_inspector_section(
        &text_inspector_content,
        &t("Font"),
        input.font_family_list.upcast_ref(),
    );
    text_inspector
}
