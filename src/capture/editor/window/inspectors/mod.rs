//! Editor right-inspector shell and tool-panel owners (PR 10.11).
//!
//! Child modules own per-tool panel assembly. This facade keeps the stack,
//! tabs, and sidebar action chrome and remains the only setup-facing entry.

mod crop;
mod number;
mod obfuscate;
mod select;
mod shell;
mod stroke;
mod text;

use gtk4::{prelude::*, Box as GtkBox, Button, Label, Orientation, Stack};

use super::background_panel::BACKGROUND_SIDEBAR_WIDTH;

use crop::{build_crop_inspector, CropInspectorInputs};
use number::{build_number_inspector, NumberInspectorInputs};
use obfuscate::{build_obfuscate_inspector, ObfuscateInspectorInputs};
use select::{build_select_inspector, SelectInspectorInputs};
use stroke::{
    build_arrow_inspector, build_highlighter_inspector, build_line_inspector, build_pen_inspector,
    ArrowInspectorInputs, HighlighterInspectorInputs, LineInspectorInputs, PenInspectorInputs,
};
use text::{build_text_inspector, TextInspectorInputs};

pub(super) struct InspectorParts {
    pub inspector_tabs: GtkBox,
    pub background_tab_btn: Button,
    pub colors_tab_btn: Button,
    pub inspector: GtkBox,
    pub inspector_stack: Stack,
}

/// Prebuilt content widgets assembled into tool-specific inspector panels.
///
/// Kept as one setup-facing input bag so `setup_editor_window_full` stays a
/// thin caller; child builders take narrower domain inputs internally.
pub(super) struct InspectorContentInputs<'a> {
    pub select_status_label: &'a Label,
    pub select_detail_label: &'a Label,
    pub select_geometry_label: &'a Label,
    pub select_hint_label: &'a Label,
    pub crop_dimensions_group: &'a GtkBox,
    pub crop_ratio_list: &'a GtkBox,
    pub crop_actions_group: &'a GtkBox,
    pub pen_inspector_list: &'a GtkBox,
    pub arrow_style_list: &'a GtkBox,
    pub arrow_thickness_list: &'a GtkBox,
    pub arrow_behavior_group: &'a GtkBox,
    pub line_inspector_list: &'a GtkBox,
    pub text_size_list: &'a GtkBox,
    pub font_family_list: &'a GtkBox,
    pub obfuscate_method_list: &'a GtkBox,
    pub number_options_list: &'a GtkBox,
    pub number_start_row: &'a GtkBox,
    pub number_size_list: &'a GtkBox,
    pub highlighter_inspector_list: &'a GtkBox,
    pub sidebar_utility_controls: &'a GtkBox,
    pub background_inspector: &'a GtkBox,
    pub colors_inspector: &'a GtkBox,
    pub placeholder_inspector: &'a GtkBox,
    pub copy_btn: &'a Button,
    pub upload_btn: &'a Button,
    pub save_btn: &'a Button,
}

pub(super) fn build_tool_inspectors(input: InspectorContentInputs<'_>) -> InspectorParts {
    let select_inspector = build_select_inspector(SelectInspectorInputs {
        select_status_label: input.select_status_label,
        select_detail_label: input.select_detail_label,
        select_geometry_label: input.select_geometry_label,
        select_hint_label: input.select_hint_label,
    });

    let crop_inspector = build_crop_inspector(CropInspectorInputs {
        crop_dimensions_group: input.crop_dimensions_group,
        crop_ratio_list: input.crop_ratio_list,
        crop_actions_group: input.crop_actions_group,
    });

    let pen_inspector = build_pen_inspector(PenInspectorInputs {
        pen_inspector_list: input.pen_inspector_list,
    });

    let arrow_inspector = build_arrow_inspector(ArrowInspectorInputs {
        arrow_style_list: input.arrow_style_list,
        arrow_thickness_list: input.arrow_thickness_list,
        arrow_behavior_group: input.arrow_behavior_group,
    });

    let line_inspector = build_line_inspector(LineInspectorInputs {
        line_inspector_list: input.line_inspector_list,
    });

    let text_inspector = build_text_inspector(TextInspectorInputs {
        text_size_list: input.text_size_list,
        font_family_list: input.font_family_list,
    });

    let obfuscate_inspector = build_obfuscate_inspector(ObfuscateInspectorInputs {
        obfuscate_method_list: input.obfuscate_method_list,
    });

    let number_inspector = build_number_inspector(NumberInspectorInputs {
        number_options_list: input.number_options_list,
        number_start_row: input.number_start_row,
        number_size_list: input.number_size_list,
    });

    let highlighter_inspector = build_highlighter_inspector(HighlighterInspectorInputs {
        highlighter_inspector_list: input.highlighter_inspector_list,
    });

    let inspector_tabs = GtkBox::new(Orientation::Horizontal, 8);
    inspector_tabs.add_css_class("editor-inspector-tabs");
    inspector_tabs.set_width_request(BACKGROUND_SIDEBAR_WIDTH);
    inspector_tabs.set_hexpand(false);
    inspector_tabs.set_halign(gtk4::Align::Fill);

    let background_tab_btn = Button::with_label("Background");
    background_tab_btn.set_has_frame(false);
    background_tab_btn.add_css_class("editor-inspector-tab-button");

    let colors_tab_btn = Button::with_label("Colors");
    colors_tab_btn.set_has_frame(false);
    colors_tab_btn.add_css_class("editor-inspector-tab-button");

    inspector_tabs.append(&background_tab_btn);
    inspector_tabs.append(&colors_tab_btn);

    let inspector = GtkBox::new(Orientation::Vertical, 0);
    inspector.add_css_class("editor-right-inspector");
    inspector.set_width_request(BACKGROUND_SIDEBAR_WIDTH);
    inspector.set_hexpand(false);
    inspector.set_vexpand(true);
    inspector.append(input.sidebar_utility_controls);
    inspector.append(&inspector_tabs);

    let inspector_stack = Stack::new();
    inspector_stack.set_hhomogeneous(true);
    inspector_stack.set_vhomogeneous(false);
    inspector_stack.set_width_request(BACKGROUND_SIDEBAR_WIDTH);
    inspector_stack.set_hexpand(false);
    inspector_stack.set_vexpand(true);
    input.background_inspector.set_visible(true);
    crop_inspector.set_visible(true);
    pen_inspector.set_visible(true);
    arrow_inspector.set_visible(true);
    line_inspector.set_visible(true);
    text_inspector.set_visible(true);
    highlighter_inspector.set_visible(true);
    obfuscate_inspector.set_visible(true);
    number_inspector.set_visible(true);
    input.colors_inspector.set_visible(true);
    input.placeholder_inspector.set_visible(true);
    select_inspector.set_visible(true);
    inspector_stack.add_named(input.background_inspector, Some("background"));
    inspector_stack.add_named(&select_inspector, Some("select"));
    inspector_stack.add_named(&crop_inspector, Some("crop"));
    inspector_stack.add_named(&pen_inspector, Some("pen"));
    inspector_stack.add_named(&arrow_inspector, Some("arrow"));
    inspector_stack.add_named(&line_inspector, Some("line"));
    inspector_stack.add_named(&text_inspector, Some("text"));
    inspector_stack.add_named(&highlighter_inspector, Some("highlighter"));
    inspector_stack.add_named(&obfuscate_inspector, Some("obfuscate"));
    inspector_stack.add_named(&number_inspector, Some("number"));
    inspector_stack.add_named(input.colors_inspector, Some("colors"));
    inspector_stack.add_named(input.placeholder_inspector, Some("placeholder"));
    inspector_stack.set_visible_child_name("placeholder");
    inspector.append(&inspector_stack);

    let sidebar_actions = GtkBox::new(Orientation::Horizontal, 8);
    sidebar_actions.add_css_class("editor-sidebar-actions");
    let sidebar_action_spacer = GtkBox::new(Orientation::Horizontal, 0);
    sidebar_action_spacer.set_hexpand(true);
    sidebar_actions.append(input.copy_btn);
    sidebar_actions.append(input.upload_btn);
    sidebar_actions.append(&sidebar_action_spacer);
    sidebar_actions.append(input.save_btn);
    inspector.append(&sidebar_actions);

    InspectorParts {
        inspector_tabs,
        background_tab_btn,
        colors_tab_btn,
        inspector,
        inspector_stack,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn inspector_children_own_tool_panel_builders() {
        let shell = include_str!("mod.rs");
        let select = include_str!("select.rs");
        let crop = include_str!("crop.rs");
        let stroke = include_str!("stroke.rs");
        let number = include_str!("number.rs");
        assert!(
            shell.contains("mod select;")
                && shell.contains("mod crop;")
                && shell.contains("mod stroke;")
                && shell.contains("mod number;")
                && select.contains("fn build_select_inspector")
                && crop.contains("fn build_crop_inspector")
                && stroke.contains("fn build_arrow_inspector")
                && stroke.contains("fn build_pen_inspector")
                && number.contains("fn build_number_inspector")
                && shell.contains("build_select_inspector(SelectInspectorInputs")
                && shell.contains("build_crop_inspector(CropInspectorInputs"),
            "inspector shell should dispatch to family-owned panel builders"
        );
    }
}
