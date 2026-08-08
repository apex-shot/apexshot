//! Select-tool inspector panel (PR 10.11).

use gtk4::{prelude::*, Box as GtkBox, Label};

use super::shell::{append_inspector_section, build_tool_inspector};

pub(super) struct SelectInspectorInputs<'a> {
    pub select_status_label: &'a Label,
    pub select_detail_label: &'a Label,
    pub select_geometry_label: &'a Label,
    pub select_hint_label: &'a Label,
}

pub(super) fn build_select_inspector(input: SelectInspectorInputs<'_>) -> GtkBox {
    let (select_inspector, select_inspector_content) = build_tool_inspector();
    append_inspector_section(
        &select_inspector_content,
        "Selection",
        input.select_status_label.upcast_ref(),
    );
    append_inspector_section(
        &select_inspector_content,
        "Details",
        input.select_detail_label.upcast_ref(),
    );
    append_inspector_section(
        &select_inspector_content,
        "Geometry",
        input.select_geometry_label.upcast_ref(),
    );
    append_inspector_section(
        &select_inspector_content,
        "Actions",
        input.select_hint_label.upcast_ref(),
    );
    select_inspector
}
