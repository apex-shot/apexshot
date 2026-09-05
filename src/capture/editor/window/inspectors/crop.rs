//! Crop-tool inspector panel (PR 10.11).

use gtk4::{prelude::*, Box as GtkBox};

use super::shell::{append_inspector_section, build_tool_inspector};
use crate::i18n::t;

pub(super) struct CropInspectorInputs<'a> {
    pub crop_dimensions_group: &'a GtkBox,
    pub crop_ratio_list: &'a GtkBox,
    pub crop_actions_group: &'a GtkBox,
}

pub(super) fn build_crop_inspector(input: CropInspectorInputs<'_>) -> GtkBox {
    let (crop_inspector, crop_inspector_content) = build_tool_inspector();
    input
        .crop_ratio_list
        .add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &crop_inspector_content,
        &t("Dimensions"),
        input.crop_dimensions_group.upcast_ref(),
    );
    append_inspector_section(
        &crop_inspector_content,
        &t("Aspect Ratio"),
        input.crop_ratio_list.upcast_ref(),
    );
    append_inspector_section(
        &crop_inspector_content,
        &t("Actions"),
        input.crop_actions_group.upcast_ref(),
    );
    crop_inspector
}
