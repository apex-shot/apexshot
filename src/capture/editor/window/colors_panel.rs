use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CssProvider, EventControllerMotion, Label, Orientation, Overlay, Scale,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::i18n::t;

use super::super::color::{
    draw_color_to_rgba_u8, palette_index_for_color, picker_dynamic_css,
    save_persisted_custom_slot_colors, DRAW_COLORS,
};
use super::super::state::EditorState;
use super::super::types::{BackgroundStyle, DrawColor, Tool};
use super::super::ui_support::{color_swatch_button, set_active_color_button};
use super::background_panel::BACKGROUND_SIDEBAR_WIDTH;
use super::icon_names;

const SIDEBAR_PALETTE_SPECS: [(&str, &str); 12] = [
    ("Black", "editor-color-black"),
    ("Blue", "editor-color-blue"),
    ("Dark Green", "editor-color-dark-green"),
    ("Red", "editor-color-red"),
    ("Orange", "editor-color-orange"),
    ("Yellow", "editor-color-yellow"),
    ("Green", "editor-color-green"),
    ("Cyan", "editor-color-cyan"),
    ("Blue Bright", "editor-color-blue-bright"),
    ("Purple", "editor-color-purple"),
    ("Pink", "editor-color-pink"),
    ("White", "editor-color-white"),
];

pub struct ColorsPanelParts {
    pub root: GtkBox,
    pub sync_for_active_tool: Rc<dyn Fn()>,
    pub refresh_custom_slots: Rc<dyn Fn()>,
}

pub fn build_colors_panel(
    state: Arc<Mutex<EditorState>>,
    apply_picker_color: Rc<dyn Fn(DrawColor)>,
    custom_slot_colors: Rc<RefCell<Vec<Option<DrawColor>>>>,
    refresh_shared_custom_color_slots: Rc<dyn Fn()>,
    activate_eyedropper: Rc<dyn Fn()>,
    open_color_picker: Rc<dyn Fn()>,
) -> ColorsPanelParts {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.add_css_class("editor-colors-panel");
    root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);
    root.set_hexpand(false);
    root.set_halign(gtk4::Align::Fill);
    root.set_vexpand(true);

    let content = GtkBox::new(Orientation::Vertical, 12);
    content.set_hexpand(false);
    content.set_halign(gtk4::Align::Fill);

    let helper = Label::new(Some(&t("Choose a color for the active tool")));
    helper.add_css_class("editor-colors-panel-helper");
    helper.set_wrap(true);
    helper.set_max_width_chars(26);
    helper.set_halign(gtk4::Align::Fill);
    helper.set_xalign(0.0);

    let opacity_syncing = Rc::new(Cell::new(false));
    let opacity_css_provider = CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &opacity_css_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }

    let color_section = GtkBox::new(Orientation::Vertical, 8);
    color_section.add_css_class("editor-colors-panel-section");
    color_section.set_hexpand(true);
    color_section.set_halign(gtk4::Align::Fill);

    let color_row = GtkBox::new(Orientation::Horizontal, 8);
    color_row.set_hexpand(true);
    color_row.set_halign(gtk4::Align::Fill);
    let color_label = Label::new(Some(&t("Color")));
    color_label.add_css_class("editor-color-field-label");
    color_label.set_hexpand(true);
    color_label.set_xalign(0.0);
    let edit_color_button = Button::with_label(&t("Edit"));
    edit_color_button.set_has_frame(false);
    edit_color_button.set_valign(gtk4::Align::Center);
    edit_color_button.add_css_class("editor-colors-panel-action-button");
    edit_color_button.connect_clicked(move |_| open_color_picker());
    color_row.append(&color_label);
    color_row.append(&edit_color_button);

    let opacity_row = GtkBox::new(Orientation::Horizontal, 8);
    opacity_row.set_hexpand(true);
    opacity_row.set_halign(gtk4::Align::Fill);
    let opacity_label = Label::new(Some(&t("Opacity")));
    opacity_label.add_css_class("editor-color-field-label");
    opacity_label.set_valign(gtk4::Align::Center);
    let opacity_slider = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    opacity_slider.set_draw_value(false);
    opacity_slider.set_hexpand(true);
    opacity_slider.set_halign(gtk4::Align::Fill);
    opacity_slider.add_css_class("editor-opacity-slider");
    opacity_slider.set_widget_name("editor-sidebar-picker-opacity-slider");
    opacity_row.append(&opacity_label);
    opacity_row.append(&opacity_slider);

    color_section.append(&color_row);
    color_section.append(&opacity_row);

    let palette_section = GtkBox::new(Orientation::Vertical, 8);
    palette_section.add_css_class("editor-colors-panel-section");
    palette_section.set_hexpand(true);
    palette_section.set_halign(gtk4::Align::Fill);

    let palette_title = Label::new(Some(&t("Palette")));
    palette_title.add_css_class("editor-background-section-title");
    palette_title.set_xalign(0.0);

    let palette_grid = GtkBox::new(Orientation::Vertical, 6);
    palette_grid.add_css_class("editor-colors-panel-palette-grid");
    palette_grid.set_hexpand(true);
    palette_grid.set_halign(gtk4::Align::Fill);

    let palette_buttons: Vec<Button> = SIDEBAR_PALETTE_SPECS
        .iter()
        .map(|(tooltip, class_name)| color_swatch_button(class_name, tooltip))
        .collect();

    for row_index in 0..2 {
        let row = GtkBox::new(Orientation::Horizontal, 6);
        row.add_css_class("editor-colors-panel-palette-row");
        row.set_homogeneous(true);
        row.set_hexpand(true);
        row.set_halign(gtk4::Align::Fill);

        for column_index in 0..6 {
            let index = row_index * 6 + column_index;
            let button = palette_buttons[index].clone();
            button.set_hexpand(true);
            button.set_halign(gtk4::Align::Fill);
            let apply_picker_color = apply_picker_color.clone();
            button.connect_clicked(move |_| {
                apply_picker_color(DRAW_COLORS[index]);
            });
            row.append(&button);
        }

        palette_grid.append(&row);
    }

    palette_section.append(&palette_title);
    palette_section.append(&palette_grid);

    let custom_section = GtkBox::new(Orientation::Vertical, 8);
    custom_section.add_css_class("editor-colors-panel-section");
    custom_section.set_hexpand(true);
    custom_section.set_halign(gtk4::Align::Fill);

    let custom_title = Label::new(Some(&t("My colors")));
    custom_title.add_css_class("editor-background-section-title");
    custom_title.set_xalign(0.0);

    let custom_grid = GtkBox::new(Orientation::Vertical, 6);
    custom_grid.add_css_class("editor-colors-panel-custom-grid");
    custom_grid.set_hexpand(true);
    custom_grid.set_halign(gtk4::Align::Fill);

    let custom_css_provider = CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &custom_css_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }

    let mut custom_slot_buttons = Vec::new();
    let mut custom_slot_dots = Vec::new();
    let mut custom_slot_placeholders = Vec::new();
    let mut custom_slot_remove_buttons = Vec::new();

    // Placeholder for local refresh function, populated after refresh_custom_slots is created
    let local_refresh: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    for row_index in 0..2 {
        let row = GtkBox::new(Orientation::Horizontal, 6);
        row.add_css_class("editor-colors-panel-custom-row");
        row.set_homogeneous(true);
        row.set_hexpand(true);
        row.set_halign(gtk4::Align::Fill);

        for column_index in 0..5 {
            let index = row_index * 5 + column_index;
            let slot_button = Button::new();
            slot_button.set_has_frame(false);
            slot_button.set_focusable(false);
            slot_button.set_hexpand(true);
            slot_button.set_halign(gtk4::Align::Fill);
            slot_button.add_css_class("editor-color-button");
            slot_button.add_css_class("editor-custom-color-slot");

            let placeholder = GtkBox::new(Orientation::Horizontal, 0);
            placeholder.set_size_request(18, 18);
            placeholder.set_halign(gtk4::Align::Center);
            placeholder.set_valign(gtk4::Align::Center);
            placeholder.add_css_class("editor-color-placeholder-dot");

            let custom_dot = GtkBox::new(Orientation::Horizontal, 0);
            custom_dot.set_size_request(18, 18);
            custom_dot.set_halign(gtk4::Align::Center);
            custom_dot.set_valign(gtk4::Align::Center);
            custom_dot.add_css_class("editor-color-dot");
            custom_dot.set_widget_name(&format!("editor-sidebar-custom-color-dot-{index}"));

            let remove_btn = Button::new();
            remove_btn.set_has_frame(false);
            remove_btn.set_focusable(false);
            remove_btn.set_visible(false);
            remove_btn.set_tooltip_text(Some(&t("Remove custom color")));
            remove_btn.set_halign(gtk4::Align::End);
            remove_btn.set_valign(gtk4::Align::Start);
            remove_btn.set_margin_top(-3);
            remove_btn.set_margin_end(-3);
            remove_btn.add_css_class("editor-custom-color-remove-button");
            let remove_icon = gtk4::Image::from_icon_name(icon_names::DISMISS_REGULAR);
            remove_icon.set_pixel_size(7);
            remove_icon.add_css_class("editor-custom-color-remove-icon");
            remove_btn.set_child(Some(&remove_icon));

            slot_button.set_child(Some(&placeholder));

            let overlay = Overlay::new();
            overlay.add_css_class("editor-custom-color-slot-overlay");
            overlay.set_hexpand(true);
            overlay.set_halign(gtk4::Align::Fill);
            overlay.set_child(Some(&slot_button));
            overlay.add_overlay(&remove_btn);

            let hover_controller = EventControllerMotion::new();
            let remove_btn_enter = remove_btn.clone();
            let custom_slot_colors_enter = custom_slot_colors.clone();
            hover_controller.connect_enter(move |_, _, _| {
                if custom_slot_colors_enter.borrow()[index].is_some() {
                    remove_btn_enter.set_visible(true);
                }
            });
            let remove_btn_leave = remove_btn.clone();
            hover_controller.connect_leave(move |_| {
                remove_btn_leave.set_visible(false);
            });
            overlay.add_controller(hover_controller);

            row.append(&overlay);

            let custom_slot_colors_click = custom_slot_colors.clone();
            let apply_picker_color_click = apply_picker_color.clone();
            slot_button.connect_clicked(move |_| {
                if let Some(color) = custom_slot_colors_click.borrow()[index] {
                    apply_picker_color_click(color);
                }
            });

            let custom_slot_colors_remove = custom_slot_colors.clone();
            let refresh_shared_custom_color_slots_remove =
                refresh_shared_custom_color_slots.clone();
            let local_refresh_remove = local_refresh.clone();
            remove_btn.connect_clicked(move |_| {
                let mut custom_colors = custom_slot_colors_remove.borrow_mut();
                if custom_colors[index].is_none() {
                    return;
                }

                custom_colors[index] = None;
                save_persisted_custom_slot_colors(custom_colors.as_slice());
                drop(custom_colors);
                refresh_shared_custom_color_slots_remove();
                if let Some(refresh) = local_refresh_remove.borrow().as_ref() {
                    refresh();
                }
            });

            custom_slot_buttons.push(slot_button);
            custom_slot_dots.push(custom_dot);
            custom_slot_placeholders.push(placeholder);
            custom_slot_remove_buttons.push(remove_btn);
        }

        custom_grid.append(&row);
    }

    custom_section.append(&custom_title);
    custom_section.append(&custom_grid);

    let actions = GtkBox::new(Orientation::Vertical, 8);
    actions.add_css_class("editor-colors-panel-actions");
    actions.set_halign(gtk4::Align::Fill);
    actions.set_hexpand(true);

    let add_current_color_btn = Button::with_label(&t("+ Add current color"));
    add_current_color_btn.set_has_frame(false);
    add_current_color_btn.set_halign(gtk4::Align::Fill);
    add_current_color_btn.set_hexpand(true);
    add_current_color_btn.add_css_class("editor-add-to-colors-button");
    add_current_color_btn.add_css_class("editor-colors-panel-action-button");

    let pick_from_screen_btn = Button::with_label(&t("Pick from screen"));
    pick_from_screen_btn.set_has_frame(false);
    pick_from_screen_btn.set_halign(gtk4::Align::Fill);
    pick_from_screen_btn.set_hexpand(true);
    pick_from_screen_btn.add_css_class("editor-colors-panel-action-button");

    actions.append(&add_current_color_btn);
    actions.append(&pick_from_screen_btn);

    content.append(&helper);
    content.append(&color_section);
    content.append(&palette_section);
    content.append(&custom_section);
    content.append(&actions);
    root.append(&content);

    let refresh_custom_slots_local: Rc<dyn Fn()> = Rc::new({
        let custom_slot_colors = custom_slot_colors.clone();
        let custom_slot_buttons = custom_slot_buttons.clone();
        let custom_slot_dots = custom_slot_dots.clone();
        let custom_slot_placeholders = custom_slot_placeholders.clone();
        let custom_slot_remove_buttons = custom_slot_remove_buttons.clone();
        let custom_css_provider = custom_css_provider.clone();
        move || {
            let custom_colors = custom_slot_colors.borrow();
            let mut css = String::new();
            for (index, slot_button) in custom_slot_buttons.iter().enumerate() {
                if let Some(color) = custom_colors[index] {
                    slot_button.add_css_class("has-custom-color");
                    slot_button.set_child(Some(&custom_slot_dots[index]));
                    custom_slot_remove_buttons[index].set_visible(false);

                    let (r, g, b, _) = draw_color_to_rgba_u8(color);
                    let alpha = color.a.clamp(0.0, 1.0);
                    css.push_str(&format!(
                        "#editor-sidebar-custom-color-dot-{index} {{ background: rgba({r}, {g}, {b}, {alpha:.3}); border: 1px solid rgba(0, 0, 0, 0.22); }}"
                    ));
                } else {
                    slot_button.remove_css_class("has-custom-color");
                    slot_button.set_child(Some(&custom_slot_placeholders[index]));
                    custom_slot_remove_buttons[index].set_visible(false);
                }
            }
            custom_css_provider.load_from_data(&css);
        }
    });

    // Populate the local refresh placeholder so remove buttons can call it
    *local_refresh.borrow_mut() = Some(refresh_custom_slots_local.clone());

    let refresh_custom_slots: Rc<dyn Fn()> = Rc::new({
        let refresh_custom_slots_local = refresh_custom_slots_local.clone();
        let refresh_shared_custom_color_slots = refresh_shared_custom_color_slots.clone();
        move || {
            refresh_custom_slots_local();
            refresh_shared_custom_color_slots();
        }
    });

    let active_color_for_tool: Rc<dyn Fn() -> (Tool, DrawColor)> = Rc::new({
        let state = state.clone();
        move || {
            let state = state.lock().unwrap();
            let selected_tool = state.selected_tool;
            let color = if selected_tool == Tool::Background {
                if let BackgroundStyle::PlainColor(color) = &state.background_style {
                    *color
                } else {
                    state.selected_color
                }
            } else {
                state.selected_color
            };
            (selected_tool, color)
        }
    });

    opacity_slider.connect_value_changed({
        let opacity_syncing = opacity_syncing.clone();
        let active_color_for_tool = active_color_for_tool.clone();
        let apply_picker_color = apply_picker_color.clone();
        move |slider| {
            if opacity_syncing.get() {
                return;
            }
            let (_, color) = active_color_for_tool();
            apply_picker_color(color.with_alpha((slider.value() / 100.0).clamp(0.0, 1.0)));
        }
    });

    let sync_for_active_tool: Rc<dyn Fn()> = Rc::new({
        let active_color_for_tool = active_color_for_tool.clone();
        let helper = helper.clone();
        let palette_buttons = palette_buttons.clone();
        let refresh_custom_slots = refresh_custom_slots.clone();
        let opacity_slider = opacity_slider.clone();
        let opacity_syncing = opacity_syncing.clone();
        let opacity_css_provider = opacity_css_provider.clone();
        move || {
            let (selected_tool, active_color) = active_color_for_tool();
            let helper_text = if selected_tool == Tool::Background {
                t("Choose the solid color used when Background is set to plain color")
            } else {
                t("Choose a color for the active tool")
            };
            helper.set_label(&helper_text);
            set_active_color_button(&palette_buttons, palette_index_for_color(active_color));
            opacity_syncing.set(true);
            opacity_slider.set_value((active_color.a.clamp(0.0, 1.0) * 100.0).round());
            opacity_syncing.set(false);
            let css = picker_dynamic_css(active_color).replace(
                "#editor-picker-opacity-slider",
                "#editor-sidebar-picker-opacity-slider",
            );
            opacity_css_provider.load_from_data(&css);
            refresh_custom_slots();
        }
    });

    let state_add = state.clone();
    let refresh_shared_custom_color_slots_add = refresh_shared_custom_color_slots.clone();
    let sync_for_active_tool_add = sync_for_active_tool.clone();
    add_current_color_btn.connect_clicked(move |_| {
        let color_to_add = {
            let st = state_add.lock().unwrap();
            if st.selected_tool == Tool::Background {
                if let BackgroundStyle::PlainColor(color) = st.background_style {
                    color
                } else {
                    st.selected_color
                }
            } else {
                st.selected_color
            }
        };

        let mut custom_colors = custom_slot_colors.borrow_mut();
        let Some(slot_index) = custom_colors.iter().position(Option::is_none) else {
            return;
        };

        custom_colors[slot_index] = Some(color_to_add);
        save_persisted_custom_slot_colors(custom_colors.as_slice());
        drop(custom_colors);
        refresh_shared_custom_color_slots_add();
        sync_for_active_tool_add();
    });

    pick_from_screen_btn.connect_clicked(move |_| {
        activate_eyedropper();
    });

    sync_for_active_tool();

    ColorsPanelParts {
        root,
        sync_for_active_tool,
        refresh_custom_slots,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn colors_panel_contains_background_plain_color_apply_markers() {
        let source = include_str!("colors_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("BackgroundStyle::PlainColor")
                && production_source.contains("selected_tool == Tool::Background"),
            "Colors panel should support applying plain colors for the Background tool",
        );
    }

    #[test]
    fn colors_panel_contains_shared_color_management_markers() {
        let source = include_str!("colors_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("My colors")
                && production_source.contains("+ Add current color")
                && production_source.contains("Pick from screen")
                && production_source
                    .contains("let color_section = GtkBox::new(Orientation::Vertical, 8);")
                && production_source.contains("content.append(&color_section);")
                && production_source.contains("edit_color_button.connect_clicked")
                && production_source.contains("open_color_picker()"),
            "Colors panel should expose shared color management controls",
        );
    }

    #[test]
    fn colors_panel_uses_shared_picker_and_keeps_opacity_control() {
        let source = include_str!("colors_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("let opacity_slider = Scale::with_range(")
                && production_source.contains("t(\"Opacity\")")
                && production_source.contains("color.with_alpha(")
                && production_source.contains("opacity_syncing.get()")
                && !production_source.contains("PickerColorState")
                && !production_source.contains("let hex_entry")
                && !production_source.contains("let rgba_row"),
            "Colors panel should use the shared RGB picker and retain alpha editing",
        );
    }

    #[test]
    fn colors_panel_opacity_sync_does_not_reenter_its_change_handler() {
        let source = include_str!("colors_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("opacity_syncing.set(true);")
                && production_source.contains("opacity_slider.set_value(")
                && production_source.contains("opacity_syncing.set(false);"),
            "syncing opacity from an active color should not trigger an alpha edit",
        );
    }

    #[test]
    fn colors_panel_no_longer_renders_current_color_summary_section() {
        let source = include_str!("colors_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production_source.contains("Current color")
                && !production_source.contains("editor-colors-panel-current-row")
                && !production_source.contains("editor-sidebar-current-color-preview"),
            "Colors panel should not duplicate the current color summary once the toolbar owns that status",
        );
    }

    #[test]
    fn colors_panel_matches_background_content_width() {
        let source = include_str!("colors_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
                && production_source.contains("root.set_hexpand(false);")
                && production_source.contains("root.set_halign(gtk4::Align::Fill);")
                && production_source.contains("content.set_hexpand(false);")
                && production_source.contains("content.set_halign(gtk4::Align::Fill);")
                && production_source.contains("palette_section.set_hexpand(true);")
                && production_source.contains("palette_section.set_halign(gtk4::Align::Fill);")
                && production_source.contains("palette_grid.set_hexpand(true);")
                && production_source.contains("palette_grid.set_halign(gtk4::Align::Fill);")
                && production_source.contains("button.set_hexpand(true);")
                && production_source.contains("button.set_halign(gtk4::Align::Fill);")
                && production_source.contains("custom_section.set_hexpand(true);")
                && production_source.contains("custom_section.set_halign(gtk4::Align::Fill);")
                && production_source.contains("custom_grid.set_hexpand(true);")
                && production_source.contains("custom_grid.set_halign(gtk4::Align::Fill);")
                && production_source.contains("slot_button.set_hexpand(true);")
                && production_source.contains("slot_button.set_halign(gtk4::Align::Fill);")
                && production_source.contains("overlay.set_hexpand(true);")
                && production_source.contains("overlay.set_halign(gtk4::Align::Fill);")
                && production_source
                    .contains("let actions = GtkBox::new(Orientation::Vertical, 8);")
                && production_source.contains("actions.set_hexpand(true);")
                && production_source.contains("add_current_color_btn.set_hexpand(true);")
                && production_source.contains("pick_from_screen_btn.set_hexpand(true);")
                && production_source.contains("helper.set_halign(gtk4::Align::Fill);")
                && !production_source
                    .contains("helper.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
                && !production_source
                    .contains("content.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
                && !production_source
                    .contains("palette_grid.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
                && !production_source
                    .contains("custom_grid.set_width_request(BACKGROUND_SIDEBAR_WIDTH);"),
            "Colors panel should use the same content width as the Background panel",
        );
    }
}
