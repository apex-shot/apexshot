use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CssProvider, DragSource, DrawingArea, DropTarget, Entry,
    EventControllerMotion, GestureClick, Image, Label, MenuButton, Orientation, Overlay, Popover,
    Scale,
};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::super::color::{
    custom_color_slots_css, load_persisted_custom_slot_colors, move_custom_color_between_slots,
    parse_alpha_percent, parse_channel_u8, parse_hex_rgb, picker_dynamic_css,
    save_persisted_custom_slot_colors, DEFAULT_COLOR_INDEX, DRAW_COLORS,
};
use super::super::state::EditorState;
use super::super::types::{BackgroundStyle, DrawColor, PickerColorState, Point, Tool};
use super::super::ui_support::color_swatch_button;
use crate::capture::editor::window::icon_names;

fn transparent_drag_icon_texture() -> Option<gdk::Texture> {
    let pixbuf = gtk4::gdk_pixbuf::Pixbuf::new(gtk4::gdk_pixbuf::Colorspace::Rgb, true, 8, 1, 1)?;
    pixbuf.fill(0x0000_0000);
    Some(gdk::Texture::for_pixbuf(&pixbuf))
}

const PICKER_PANEL_WIDTH: i32 = 252;
const PICKER_SLIDER_WIDTH: i32 = 220;
const PICKER_HEX_ENTRY_WIDTH: i32 = 214;

pub struct ColorPickerParts {
    pub trigger_host: Overlay,
    pub popover: Popover,
    pub color_buttons: Vec<Button>,
    pub color_picker_dot: GtkBox,
    pub color_class_names: Vec<&'static str>,
    pub eyedropper_btn: Button,
    pub sync_for_active_tool: Rc<dyn Fn()>,
    pub sync_picker_from_color: Rc<dyn Fn(DrawColor)>,
    pub apply_picker_color: Rc<dyn Fn(DrawColor)>,
    pub set_picker_panel_visibility: Rc<dyn Fn(bool)>,
}

pub fn build_color_picker(
    state: Arc<Mutex<EditorState>>,
    canvas_queue_draw_signal: Rc<dyn Fn()>,
    drawing_area: Rc<RefCell<Option<glib::object::WeakRef<DrawingArea>>>>,
    show_color_names: bool,
) -> ColorPickerParts {
    // Color specs
    let color_specs = [
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
    let visible_color_specs = &color_specs[..10];
    let color_class_names: Vec<&'static str> = color_specs
        .iter()
        .map(|(_, class_name)| *class_name)
        .collect();
    let color_buttons: Vec<Button> = visible_color_specs
        .iter()
        .map(|(tooltip, class_name)| color_swatch_button(class_name, tooltip))
        .collect();

    // Color picker trigger
    let color_picker_trigger = MenuButton::new();
    color_picker_trigger.set_has_frame(false);
    color_picker_trigger.set_focusable(false);
    color_picker_trigger.set_can_target(false);
    color_picker_trigger.set_tooltip_text(Some("Colors"));
    color_picker_trigger.set_icon_name("");
    color_picker_trigger.set_hexpand(true);
    color_picker_trigger.set_vexpand(true);
    color_picker_trigger.set_halign(gtk4::Align::Fill);
    color_picker_trigger.set_valign(gtk4::Align::Fill);
    color_picker_trigger.add_css_class("editor-color-trigger-menu-button");
    color_picker_trigger.add_css_class("flat");

    let color_picker_dot = GtkBox::new(Orientation::Horizontal, 0);
    color_picker_dot.set_size_request(20, 20);
    color_picker_dot.set_halign(gtk4::Align::Center);
    color_picker_dot.set_valign(gtk4::Align::Center);
    color_picker_dot.add_css_class("editor-color-trigger-dot");
    color_picker_dot.add_css_class(color_specs[DEFAULT_COLOR_INDEX].1);

    let trigger_divider = GtkBox::new(Orientation::Vertical, 0);
    trigger_divider.add_css_class("editor-color-trigger-divider");

    let color_picker_arrow_box = GtkBox::new(Orientation::Horizontal, 0);
    color_picker_arrow_box.add_css_class("editor-color-trigger-arrow-box");
    color_picker_arrow_box.set_halign(gtk4::Align::Center);
    color_picker_arrow_box.set_valign(gtk4::Align::Center);
    let color_picker_arrow = Image::from_icon_name("pan-down-symbolic");
    color_picker_arrow.set_pixel_size(10);
    color_picker_arrow.add_css_class("editor-color-trigger-arrow");
    color_picker_arrow_box.append(&color_picker_arrow);

    let color_picker_trigger_shell = GtkBox::new(Orientation::Horizontal, 0);
    color_picker_trigger_shell.add_css_class("editor-color-trigger-shell");
    color_picker_trigger_shell.set_valign(gtk4::Align::Center);
    color_picker_trigger_shell.append(&color_picker_dot);
    color_picker_trigger_shell.append(&trigger_divider);
    color_picker_trigger_shell.append(&color_picker_arrow_box);

    let color_picker_trigger_host = Overlay::new();
    color_picker_trigger_host.set_child(Some(&color_picker_trigger_shell));
    color_picker_trigger_host.add_overlay(&color_picker_trigger);

    let color_picker_shell_click = GestureClick::new();
    let color_picker_trigger_popup = color_picker_trigger.clone();
    color_picker_shell_click.connect_pressed(move |_, _, _, _| {
        color_picker_trigger_popup.popup();
    });
    color_picker_trigger_shell.add_controller(color_picker_shell_click);

    // Popover
    let color_popover = Popover::new();
    color_popover.set_has_arrow(false);
    color_popover.set_autohide(true);
    color_popover.set_position(gtk4::PositionType::Bottom);
    color_popover.set_offset(0, 4);
    color_popover.add_css_class("editor-color-popover");

    // Popover content
    let popover_root = GtkBox::new(Orientation::Horizontal, 0);
    popover_root.add_css_class("editor-color-popover-body");
    popover_root.set_halign(gtk4::Align::Start);
    popover_root.set_hexpand(false);

    // Swatches side
    let swatches_side = GtkBox::new(Orientation::Vertical, 0);
    swatches_side.add_css_class("editor-color-swatches-side");
    swatches_side.set_hexpand(false);

    let color_columns = GtkBox::new(Orientation::Horizontal, 6);
    color_columns.add_css_class("editor-color-dropdown-columns");
    color_columns.set_halign(gtk4::Align::Center);
    color_columns.set_homogeneous(true);

    // Column 1: default colors
    let color_column_primary = GtkBox::new(Orientation::Vertical, 1);
    color_column_primary.add_css_class("editor-color-dropdown-column");
    color_column_primary.set_halign(gtk4::Align::Center);
    for ((label, _), button) in visible_color_specs.iter().zip(color_buttons.iter()) {
        if show_color_names {
            let row = GtkBox::new(Orientation::Horizontal, 8);
            row.add_css_class("editor-color-row");
            row.set_halign(gtk4::Align::Start);

            let text = Label::new(Some(label));
            text.add_css_class("editor-color-row-label");
            text.set_xalign(0.0);

            row.append(button);
            row.append(&text);
            color_column_primary.append(&row);
        } else {
            color_column_primary.append(button);
        }
    }

    // Column 2: custom slots
    let color_column_secondary = GtkBox::new(Orientation::Vertical, 1);
    color_column_secondary.add_css_class("editor-color-dropdown-column");
    color_column_secondary.set_halign(gtk4::Align::Center);

    let custom_slot_colors = Rc::new(RefCell::new(load_persisted_custom_slot_colors(
        color_buttons.len(),
    )));
    let custom_slot_css_provider = CssProvider::new();
    let mut custom_slot_buttons: Vec<Button> = Vec::with_capacity(color_buttons.len());
    let mut custom_slot_overlays: Vec<Overlay> = Vec::with_capacity(color_buttons.len());
    let mut custom_slot_placeholders: Vec<GtkBox> = Vec::with_capacity(color_buttons.len());
    let mut custom_slot_dots: Vec<GtkBox> = Vec::with_capacity(color_buttons.len());
    let mut custom_slot_remove_buttons: Vec<Button> = Vec::with_capacity(color_buttons.len());

    for index in 0..color_buttons.len() {
        let placeholder_btn = Button::new();
        placeholder_btn.set_has_frame(false);
        placeholder_btn.set_focusable(false);
        placeholder_btn.add_css_class("editor-color-button");
        placeholder_btn.add_css_class("editor-custom-color-slot");

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
        custom_dot.set_widget_name(&format!("editor-custom-color-dot-{index}"));

        let remove_btn = Button::new();
        remove_btn.set_has_frame(false);
        remove_btn.set_focusable(false);
        remove_btn.set_visible(false);
        remove_btn.set_tooltip_text(Some("Remove custom color"));
        remove_btn.set_halign(gtk4::Align::End);
        remove_btn.set_valign(gtk4::Align::Start);
        remove_btn.set_margin_top(-3);
        remove_btn.set_margin_end(-3);
        remove_btn.add_css_class("editor-custom-color-remove-button");
        let remove_icon = Image::from_icon_name("window-close-symbolic");
        remove_icon.set_pixel_size(7);
        remove_icon.add_css_class("editor-custom-color-remove-icon");
        remove_btn.set_child(Some(&remove_icon));

        placeholder_btn.set_child(Some(&placeholder));

        let slot_overlay = Overlay::new();
        slot_overlay.add_css_class("editor-custom-color-slot-overlay");
        slot_overlay.set_child(Some(&placeholder_btn));
        slot_overlay.add_overlay(&remove_btn);

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
        slot_overlay.add_controller(hover_controller);

        color_column_secondary.append(&slot_overlay);
        custom_slot_overlays.push(slot_overlay.clone());
        custom_slot_buttons.push(placeholder_btn);
        custom_slot_placeholders.push(placeholder);
        custom_slot_dots.push(custom_dot);
        custom_slot_remove_buttons.push(remove_btn);
    }

    color_columns.append(&color_column_primary);
    color_columns.append(&color_column_secondary);

    // Universal color row
    let color_universal_row = GtkBox::new(Orientation::Horizontal, 4);
    color_universal_row.add_css_class("editor-color-dropdown-footer");
    color_universal_row.set_halign(gtk4::Align::Center);

    let universal_color_btn = Button::new();
    universal_color_btn.set_has_frame(false);
    universal_color_btn.set_focusable(false);
    universal_color_btn.set_tooltip_text(Some("Color picker"));
    universal_color_btn.add_css_class("editor-universal-color-button");
    let universal_color_wheel = GtkBox::new(Orientation::Horizontal, 0);
    universal_color_wheel.set_size_request(22, 22);
    universal_color_wheel.add_css_class("editor-universal-color-wheel");
    universal_color_btn.set_child(Some(&universal_color_wheel));

    let universal_arrow_btn = Button::new();
    universal_arrow_btn.set_has_frame(false);
    universal_arrow_btn.set_focusable(false);
    universal_arrow_btn.set_tooltip_text(Some("Open color picker"));
    universal_arrow_btn.add_css_class("editor-universal-arrow-button");
    let universal_arrow_icon = Image::from_icon_name(icon_names::GO_NEXT);
    universal_arrow_icon.set_pixel_size(12);
    universal_arrow_btn.set_child(Some(&universal_arrow_icon));

    color_universal_row.append(&universal_color_btn);
    color_universal_row.append(&universal_arrow_btn);

    swatches_side.append(&color_columns);
    swatches_side.append(&color_universal_row);

    // Picker panel
    let picker_panel = GtkBox::new(Orientation::Vertical, 10);
    picker_panel.add_css_class("editor-color-picker-panel");
    picker_panel.set_halign(gtk4::Align::Start);
    picker_panel.set_hexpand(false);
    picker_panel.set_width_request(PICKER_PANEL_WIDTH);
    picker_panel.set_visible(false);

    let picker_state = Rc::new(RefCell::new(PickerColorState::from_color(
        DRAW_COLORS[DEFAULT_COLOR_INDEX],
    )));
    let picker_update_in_progress = Rc::new(Cell::new(false));

    // Gradient area
    let gradient_area = DrawingArea::new();
    gradient_area.set_content_width(PICKER_PANEL_WIDTH);
    gradient_area.set_content_height(150);
    gradient_area.set_size_request(PICKER_PANEL_WIDTH, 150);
    gradient_area.set_halign(gtk4::Align::Start);
    gradient_area.set_hexpand(false);
    gradient_area.add_css_class("editor-gradient-area");
    let picker_state_draw = picker_state.clone();
    gradient_area.set_draw_func(move |_area, cr: &gtk4::cairo::Context, width, height| {
        let picker = *picker_state_draw.borrow();
        let w = width as f64;
        let h = height as f64;
        let (hue_r, hue_g, hue_b) = super::super::types::hsv_to_rgb(picker.hue, 1.0, 1.0);
        cr.set_source_rgb(hue_r, hue_g, hue_b);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
        let white_grad = gtk4::cairo::LinearGradient::new(0.0, 0.0, w, 0.0);
        white_grad.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 1.0);
        white_grad.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
        let _ = cr.set_source(&white_grad);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
        let black_grad = gtk4::cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
        black_grad.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
        black_grad.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 1.0);
        let _ = cr.set_source(&black_grad);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
        let cx = picker.saturation * w;
        let cy = (1.0 - picker.value) * h;
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
        cr.set_line_width(2.0);
        cr.arc(cx, cy, 6.0, 0.0, std::f64::consts::TAU);
        let _ = cr.stroke();
    });

    // Hue slider
    let hue_slider = Scale::with_range(Orientation::Horizontal, 0.0, 360.0, 1.0);
    hue_slider.set_draw_value(false);
    hue_slider.set_hexpand(false);
    hue_slider.set_halign(gtk4::Align::Start);
    hue_slider.set_width_request(PICKER_SLIDER_WIDTH);
    hue_slider.add_css_class("editor-hue-slider");

    let hue_row = GtkBox::new(Orientation::Horizontal, 8);
    hue_row.set_halign(gtk4::Align::Start);
    hue_row.set_hexpand(false);
    hue_row.set_width_request(PICKER_PANEL_WIDTH);

    let hue_preview = GtkBox::new(Orientation::Horizontal, 0);
    hue_preview.set_size_request(24, 24);
    hue_preview.set_halign(gtk4::Align::Start);
    hue_preview.add_css_class("editor-color-preview");
    hue_preview.set_widget_name("editor-picker-preview-hue");

    hue_row.append(&hue_slider);
    hue_row.append(&hue_preview);

    // Opacity slider
    let opacity_slider = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    opacity_slider.set_draw_value(false);
    opacity_slider.set_hexpand(false);
    opacity_slider.set_halign(gtk4::Align::Start);
    opacity_slider.set_width_request(PICKER_SLIDER_WIDTH);
    opacity_slider.add_css_class("editor-opacity-slider");
    opacity_slider.set_widget_name("editor-picker-opacity-slider");

    let opacity_row = GtkBox::new(Orientation::Horizontal, 8);
    opacity_row.set_halign(gtk4::Align::Start);
    opacity_row.set_hexpand(false);
    opacity_row.set_width_request(PICKER_PANEL_WIDTH);
    let opacity_row_spacer = GtkBox::new(Orientation::Horizontal, 0);
    opacity_row_spacer.set_size_request(24, 24);
    opacity_row.append(&opacity_slider);
    opacity_row.append(&opacity_row_spacer);

    // Hex entry
    let hex_entry = Entry::new();
    hex_entry.set_max_length(6);
    hex_entry.set_width_chars(6);
    hex_entry.set_max_width_chars(6);
    hex_entry.set_width_request(PICKER_HEX_ENTRY_WIDTH);
    hex_entry.set_halign(gtk4::Align::Start);
    hex_entry.set_hexpand(false);
    gtk4::prelude::EditableExt::set_alignment(&hex_entry, 0.5);
    hex_entry.add_css_class("editor-hex-entry");

    let hex_label = Label::new(Some("HEX"));
    hex_label.add_css_class("editor-color-field-label");
    hex_label.set_halign(gtk4::Align::Center);
    hex_label.set_xalign(0.5);

    let eyedropper_btn = Button::new();
    eyedropper_btn.set_has_frame(false);
    eyedropper_btn.set_valign(gtk4::Align::Center);
    eyedropper_btn.add_css_class("editor-eyedropper-button");
    let eyedropper_icon = Image::from_icon_name("color-select-symbolic");
    eyedropper_icon.set_pixel_size(16);
    eyedropper_btn.set_child(Some(&eyedropper_icon));

    let hex_input_row = GtkBox::new(Orientation::Horizontal, 8);
    hex_input_row.set_halign(gtk4::Align::Start);
    hex_input_row.set_hexpand(false);
    hex_input_row.set_width_request(PICKER_PANEL_WIDTH);
    hex_input_row.append(&hex_entry);
    hex_input_row.append(&eyedropper_btn);

    let hex_row = GtkBox::new(Orientation::Vertical, 2);
    hex_row.set_halign(gtk4::Align::Start);
    hex_row.set_hexpand(false);
    hex_row.set_width_request(PICKER_PANEL_WIDTH);
    hex_row.append(&hex_input_row);
    hex_row.append(&hex_label);

    // RGBA inputs
    let rgba_row = GtkBox::new(Orientation::Horizontal, 6);
    rgba_row.set_halign(gtk4::Align::Start);
    rgba_row.set_hexpand(false);
    rgba_row.set_width_request(PICKER_PANEL_WIDTH);
    rgba_row.set_homogeneous(true);

    let r_entry = Entry::new();
    r_entry.set_max_length(3);
    r_entry.set_width_chars(3);
    r_entry.set_max_width_chars(3);
    r_entry.set_width_request(50);
    r_entry.set_halign(gtk4::Align::Start);
    r_entry.set_hexpand(false);
    gtk4::prelude::EditableExt::set_alignment(&r_entry, 0.5);
    r_entry.add_css_class("editor-rgba-entry");
    let r_label = Label::new(Some("R"));
    r_label.add_css_class("editor-color-field-label");
    r_label.set_halign(gtk4::Align::Center);
    r_label.set_xalign(0.5);
    let r_col = GtkBox::new(Orientation::Vertical, 2);
    r_col.set_halign(gtk4::Align::Fill);
    r_col.set_hexpand(true);
    r_col.append(&r_entry);
    r_col.append(&r_label);
    rgba_row.append(&r_col);

    let g_entry = Entry::new();
    g_entry.set_max_length(3);
    g_entry.set_width_chars(3);
    g_entry.set_max_width_chars(3);
    g_entry.set_width_request(50);
    g_entry.set_halign(gtk4::Align::Start);
    g_entry.set_hexpand(false);
    gtk4::prelude::EditableExt::set_alignment(&g_entry, 0.5);
    g_entry.add_css_class("editor-rgba-entry");
    let g_label = Label::new(Some("G"));
    g_label.add_css_class("editor-color-field-label");
    g_label.set_halign(gtk4::Align::Center);
    g_label.set_xalign(0.5);
    let g_col = GtkBox::new(Orientation::Vertical, 2);
    g_col.set_halign(gtk4::Align::Fill);
    g_col.set_hexpand(true);
    g_col.append(&g_entry);
    g_col.append(&g_label);
    rgba_row.append(&g_col);

    let b_entry = Entry::new();
    b_entry.set_max_length(3);
    b_entry.set_width_chars(3);
    b_entry.set_max_width_chars(3);
    b_entry.set_width_request(50);
    b_entry.set_halign(gtk4::Align::Start);
    b_entry.set_hexpand(false);
    gtk4::prelude::EditableExt::set_alignment(&b_entry, 0.5);
    b_entry.add_css_class("editor-rgba-entry");
    let b_label = Label::new(Some("B"));
    b_label.add_css_class("editor-color-field-label");
    b_label.set_halign(gtk4::Align::Center);
    b_label.set_xalign(0.5);
    let b_col = GtkBox::new(Orientation::Vertical, 2);
    b_col.set_halign(gtk4::Align::Fill);
    b_col.set_hexpand(true);
    b_col.append(&b_entry);
    b_col.append(&b_label);
    rgba_row.append(&b_col);

    let a_entry = Entry::new();
    a_entry.set_max_length(3);
    a_entry.set_width_chars(3);
    a_entry.set_max_width_chars(3);
    a_entry.set_width_request(50);
    a_entry.set_halign(gtk4::Align::Start);
    a_entry.set_hexpand(false);
    gtk4::prelude::EditableExt::set_alignment(&a_entry, 0.5);
    a_entry.add_css_class("editor-rgba-entry");
    let a_label = Label::new(Some("A"));
    a_label.add_css_class("editor-color-field-label");
    a_label.set_halign(gtk4::Align::Center);
    a_label.set_xalign(0.5);
    let a_col = GtkBox::new(Orientation::Vertical, 2);
    a_col.set_halign(gtk4::Align::Fill);
    a_col.set_hexpand(true);
    a_col.append(&a_entry);
    a_col.append(&a_label);
    rgba_row.append(&a_col);

    // Add to colors button
    let add_to_colors_btn = Button::with_label("\u{FF0B} Add to My Colors");
    add_to_colors_btn.set_has_frame(false);
    add_to_colors_btn.set_hexpand(false);
    add_to_colors_btn.set_halign(gtk4::Align::Start);
    add_to_colors_btn.set_width_request(PICKER_PANEL_WIDTH - 32);
    add_to_colors_btn.add_css_class("editor-add-to-colors-button");

    universal_color_wheel.set_widget_name("editor-picker-universal-wheel");

    let picker_css_provider = CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &picker_css_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
        gtk4::style_context_add_provider_for_display(
            &display,
            &custom_slot_css_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }

    let refresh_custom_color_slots: Rc<dyn Fn()> = Rc::new({
        let custom_slot_colors = custom_slot_colors.clone();
        let custom_slot_buttons = custom_slot_buttons.clone();
        let custom_slot_placeholders = custom_slot_placeholders.clone();
        let custom_slot_dots = custom_slot_dots.clone();
        let custom_slot_remove_buttons = custom_slot_remove_buttons.clone();
        let custom_slot_css_provider = custom_slot_css_provider.clone();
        move || {
            let custom_colors = custom_slot_colors.borrow();
            for (index, slot_button) in custom_slot_buttons.iter().enumerate() {
                if custom_colors[index].is_some() {
                    slot_button.add_css_class("has-custom-color");
                    slot_button.set_child(Some(&custom_slot_dots[index]));
                } else {
                    slot_button.remove_css_class("has-custom-color");
                    slot_button.set_child(Some(&custom_slot_placeholders[index]));
                }

                custom_slot_remove_buttons[index].set_visible(false);
            }

            let css = custom_color_slots_css(custom_colors.as_slice());
            custom_slot_css_provider.load_from_data(&css);
        }
    });
    refresh_custom_color_slots();

    let picker_content = GtkBox::new(Orientation::Vertical, 12);
    picker_content.set_halign(gtk4::Align::Start);
    picker_content.set_hexpand(false);
    picker_content.set_width_request(PICKER_PANEL_WIDTH);
    picker_content.set_vexpand(true);
    picker_content.append(&gradient_area);
    picker_content.append(&hue_row);
    picker_content.append(&opacity_row);
    picker_content.append(&hex_row);
    picker_content.append(&rgba_row);

    picker_panel.append(&picker_content);
    picker_panel.append(&add_to_colors_btn);

    popover_root.append(&swatches_side);
    popover_root.append(&picker_panel);

    // Wire universal buttons
    let set_picker_panel_visibility: Rc<dyn Fn(bool)> = Rc::new({
        let picker_panel = picker_panel.clone();
        let universal_arrow_icon = universal_arrow_icon.clone();
        move |visible| {
            picker_panel.set_visible(visible);
            if visible {
                universal_arrow_icon.add_css_class("editor-picker-back-arrow");
            } else {
                universal_arrow_icon.remove_css_class("editor-picker-back-arrow");
            }
        }
    });

    let picker_panel_toggle_arrow = picker_panel.clone();
    let set_picker_panel_visibility_arrow = set_picker_panel_visibility.clone();
    universal_arrow_btn.connect_clicked(move |_| {
        set_picker_panel_visibility_arrow(!picker_panel_toggle_arrow.is_visible());
    });

    let picker_panel_toggle_wheel = picker_panel.clone();
    let set_picker_panel_visibility_wheel = set_picker_panel_visibility.clone();
    universal_color_btn.connect_clicked(move |_| {
        set_picker_panel_visibility_wheel(!picker_panel_toggle_wheel.is_visible());
    });

    // Reset picker panel when popover closes
    let set_picker_panel_visibility_closed = set_picker_panel_visibility.clone();
    color_popover.connect_closed(move |_| {
        set_picker_panel_visibility_closed(false);
    });

    set_picker_panel_visibility(false);

    color_popover.set_child(Some(&popover_root));
    color_picker_trigger.set_popover(Some(&color_popover));

    let update_picker_ui: Rc<dyn Fn(PickerColorState)> = Rc::new({
        let hue_slider = hue_slider.clone();
        let opacity_slider = opacity_slider.clone();
        let hex_entry = hex_entry.clone();
        let r_entry = r_entry.clone();
        let g_entry = g_entry.clone();
        let b_entry = b_entry.clone();
        let a_entry = a_entry.clone();
        let gradient_area = gradient_area.clone();
        let picker_css_provider = picker_css_provider.clone();
        let picker_update_in_progress = picker_update_in_progress.clone();
        move |picker| {
            picker_update_in_progress.set(true);

            hue_slider.set_value(picker.hue);
            opacity_slider.set_value(picker.alpha * 100.0);

            let color = picker.to_color();
            let (r, g, b, _): (u8, u8, u8, u8) = super::super::color::draw_color_to_rgba_u8(color);
            hex_entry.set_text(&super::super::color::draw_color_to_hex(color));
            r_entry.set_text(&r.to_string());
            g_entry.set_text(&g.to_string());
            b_entry.set_text(&b.to_string());
            a_entry.set_text(&(picker.alpha * 100.0).round().to_string());

            picker_css_provider.load_from_data(&picker_dynamic_css(color));
            gradient_area.queue_draw();

            picker_update_in_progress.set(false);
        }
    });

    let apply_picker_color_to_editor: Rc<dyn Fn(super::super::types::DrawColor)> = Rc::new({
        let state_picker_apply = state.clone();
        let color_buttons_picker = color_buttons.clone();
        let color_picker_dot_picker = color_picker_dot.clone();
        let color_class_names_picker = color_class_names.clone();
        let drawing_area_picker = drawing_area.clone();
        move |color| {
            let has_active_text = {
                let mut st = state_picker_apply.lock().unwrap();
                let has_active_text = st.active_text_input.is_some();
                if st.selected_tool == Tool::Crop {
                    st.set_crop_background_color(color);
                } else if st.selected_tool == Tool::Background {
                    st.background_style = BackgroundStyle::PlainColor(color);
                    st.mark_working_image_dirty();
                } else if has_active_text {
                    st.selected_color = color;
                    let _ = st.set_selected_action_color(color);
                } else {
                    st.selected_color = color;
                }
                has_active_text
            };

            let nearest_index = super::super::color::palette_index_for_color(color);
            clear_active_color_picker_palette_state(&color_buttons_picker);
            set_color_picker_trigger_dot_state(
                &color_picker_dot_picker,
                &color_class_names_picker,
                nearest_index,
            );

            if has_active_text {
                if let Some(area) = drawing_area_picker
                    .borrow()
                    .as_ref()
                    .and_then(|weak| weak.upgrade())
                {
                    area.grab_focus();
                }
            }
            canvas_queue_draw_signal();
        }
    });

    let sync_picker_from_color: Rc<dyn Fn(super::super::types::DrawColor)> = Rc::new({
        let picker_state = picker_state.clone();
        let update_picker_ui = update_picker_ui.clone();
        move |color| {
            let picker = PickerColorState::from_color(color);
            *picker_state.borrow_mut() = picker;
            update_picker_ui(picker);
        }
    });

    let commit_picker_state: Rc<dyn Fn()> = Rc::new({
        let picker_state = picker_state.clone();
        let update_picker_ui = update_picker_ui.clone();
        let apply_picker_color_to_editor = apply_picker_color_to_editor.clone();
        move || {
            let picker = *picker_state.borrow();
            update_picker_ui(picker);
            apply_picker_color_to_editor(picker.to_color());
        }
    });

    let sync_picker_for_active_tool: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let color_buttons = color_buttons.clone();
        let color_picker_dot = color_picker_dot.clone();
        let color_class_names = color_class_names.clone();
        let sync_picker_from_color = sync_picker_from_color.clone();
        move || {
            let (active_color, show_palette_state) = {
                let st = state.lock().unwrap();
                if st.selected_tool == Tool::Crop {
                    (st.crop_background_color, st.crop_background_color_explicit)
                } else if st.selected_tool == Tool::Background {
                    if let BackgroundStyle::PlainColor(color) = st.background_style {
                        (color, true)
                    } else {
                        (st.selected_color, false)
                    }
                } else {
                    (st.selected_color, true)
                }
            };
            sync_picker_from_color(active_color);
            clear_active_color_picker_palette_state(&color_buttons);
            if show_palette_state {
                set_color_picker_trigger_dot_state(
                    &color_picker_dot,
                    &color_class_names,
                    super::super::color::palette_index_for_color(active_color),
                );
            } else {
                clear_color_picker_trigger_dot_state(&color_picker_dot, &color_class_names);
            }
        }
    });

    sync_picker_for_active_tool();

    // Hue slider
    let picker_state_hue = picker_state.clone();
    let picker_update_in_progress_hue = picker_update_in_progress.clone();
    let commit_picker_state_hue = commit_picker_state.clone();
    hue_slider.connect_value_changed(move |slider| {
        if picker_update_in_progress_hue.get() {
            return;
        }

        picker_state_hue.borrow_mut().hue = super::super::types::normalize_hue(slider.value());
        commit_picker_state_hue();
    });

    // Opacity slider
    let picker_state_opacity = picker_state.clone();
    let picker_update_in_progress_opacity = picker_update_in_progress.clone();
    let commit_picker_state_opacity = commit_picker_state.clone();
    opacity_slider.connect_value_changed(move |slider| {
        if picker_update_in_progress_opacity.get() {
            return;
        }

        picker_state_opacity.borrow_mut().alpha = (slider.value() / 100.0).clamp(0.0, 1.0);
        commit_picker_state_opacity();
    });

    // Gradient area interactions
    let update_sv_from_position: Rc<dyn Fn(f64, f64)> = Rc::new({
        let gradient_area = gradient_area.clone();
        let picker_state = picker_state.clone();
        let commit_picker_state = commit_picker_state.clone();
        move |x, y| {
            let width = gradient_area.allocated_width().max(1) as f64;
            let height = gradient_area.allocated_height().max(1) as f64;
            let saturation = (x / width).clamp(0.0, 1.0);
            let value = (1.0 - (y / height)).clamp(0.0, 1.0);

            {
                let mut picker = picker_state.borrow_mut();
                picker.saturation = saturation;
                picker.value = value;
            }

            commit_picker_state();
        }
    });

    let gradient_dragging = Rc::new(Cell::new(false));

    let gradient_click = GestureClick::new();
    let gradient_dragging_press = gradient_dragging.clone();
    let update_sv_click = update_sv_from_position.clone();
    gradient_click.connect_pressed(move |_, _, x, y| {
        gradient_dragging_press.set(true);
        update_sv_click(x, y);
    });

    let gradient_dragging_release = gradient_dragging.clone();
    gradient_click.connect_released(move |_, _, _, _| {
        gradient_dragging_release.set(false);
    });
    gradient_area.add_controller(gradient_click);

    let gradient_motion = EventControllerMotion::new();
    let gradient_dragging_motion = gradient_dragging.clone();
    let update_sv_motion = update_sv_from_position.clone();
    gradient_motion.connect_motion(move |_, x, y| {
        if gradient_dragging_motion.get() {
            update_sv_motion(x, y);
        }
    });
    gradient_area.add_controller(gradient_motion);

    // Hex entry
    let picker_state_hex = picker_state.clone();
    let picker_update_in_progress_hex = picker_update_in_progress.clone();
    let commit_picker_state_hex = commit_picker_state.clone();
    hex_entry.connect_changed(move |entry| {
        if picker_update_in_progress_hex.get() {
            return;
        }

        let text = entry.text();
        let Some((r, g, b)) = parse_hex_rgb(text.as_str()) else {
            return;
        };

        let (hue, saturation, value) =
            super::super::types::rgb_to_hsv(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
        {
            let mut picker = picker_state_hex.borrow_mut();
            picker.hue = hue;
            picker.saturation = saturation;
            picker.value = value;
        }

        commit_picker_state_hex();
    });

    // RGBA entries
    let update_picker_from_rgba_entries: Rc<dyn Fn()> = Rc::new({
        let picker_state = picker_state.clone();
        let r_entry = r_entry.clone();
        let g_entry = g_entry.clone();
        let b_entry = b_entry.clone();
        let a_entry = a_entry.clone();
        let commit_picker_state = commit_picker_state.clone();
        move || {
            let Some(r) = parse_channel_u8(r_entry.text().as_str()) else {
                return;
            };
            let Some(g) = parse_channel_u8(g_entry.text().as_str()) else {
                return;
            };
            let Some(b) = parse_channel_u8(b_entry.text().as_str()) else {
                return;
            };
            let Some(alpha) = parse_alpha_percent(a_entry.text().as_str()) else {
                return;
            };

            let (hue, saturation, value) = super::super::types::rgb_to_hsv(
                r as f64 / 255.0,
                g as f64 / 255.0,
                b as f64 / 255.0,
            );
            {
                let mut picker = picker_state.borrow_mut();
                picker.hue = hue;
                picker.saturation = saturation;
                picker.value = value;
                picker.alpha = alpha;
            }

            commit_picker_state();
        }
    });

    let picker_update_in_progress_r = picker_update_in_progress.clone();
    let update_picker_from_rgba_entries_r = update_picker_from_rgba_entries.clone();
    r_entry.connect_changed(move |_| {
        if picker_update_in_progress_r.get() {
            return;
        }
        update_picker_from_rgba_entries_r();
    });

    let picker_update_in_progress_g = picker_update_in_progress.clone();
    let update_picker_from_rgba_entries_g = update_picker_from_rgba_entries.clone();
    g_entry.connect_changed(move |_| {
        if picker_update_in_progress_g.get() {
            return;
        }
        update_picker_from_rgba_entries_g();
    });

    let picker_update_in_progress_b = picker_update_in_progress.clone();
    let update_picker_from_rgba_entries_b = update_picker_from_rgba_entries.clone();
    b_entry.connect_changed(move |_| {
        if picker_update_in_progress_b.get() {
            return;
        }
        update_picker_from_rgba_entries_b();
    });

    let picker_update_in_progress_a = picker_update_in_progress.clone();
    let update_picker_from_rgba_entries_a = update_picker_from_rgba_entries.clone();
    a_entry.connect_changed(move |_| {
        if picker_update_in_progress_a.get() {
            return;
        }
        update_picker_from_rgba_entries_a();
    });

    // Universal button sync
    let sync_picker_for_active_tool_arrow = sync_picker_for_active_tool.clone();
    let picker_panel_arrow = picker_panel.clone();
    universal_arrow_btn.connect_clicked(move |_| {
        if picker_panel_arrow.is_visible() {
            sync_picker_for_active_tool_arrow();
        }
    });

    let sync_picker_for_active_tool_wheel = sync_picker_for_active_tool.clone();
    let picker_panel_wheel = picker_panel.clone();
    universal_color_btn.connect_clicked(move |_| {
        if picker_panel_wheel.is_visible() {
            sync_picker_for_active_tool_wheel();
        }
    });

    // Add color to custom slots
    let add_color_to_custom_slots: Rc<dyn Fn(super::super::types::DrawColor)> = Rc::new({
        let custom_slot_colors = custom_slot_colors.clone();
        let refresh_custom_color_slots = refresh_custom_color_slots.clone();
        move |color_to_add| {
            let mut custom_colors = custom_slot_colors.borrow_mut();
            let Some(slot_index) = custom_colors.iter().position(Option::is_none) else {
                return;
            };

            custom_colors[slot_index] = Some(color_to_add);
            save_persisted_custom_slot_colors(custom_colors.as_slice());
            drop(custom_colors);
            refresh_custom_color_slots();
        }
    });

    let picker_state_add_to_colors = picker_state.clone();
    let add_color_to_custom_slots_add = add_color_to_custom_slots.clone();
    add_to_colors_btn.connect_clicked(move |_| {
        let color_to_add = picker_state_add_to_colors.borrow().to_color();
        add_color_to_custom_slots_add(color_to_add);
    });

    let dragged_custom_slot_index = Rc::new(Cell::new(None::<usize>));
    let suppress_custom_slot_click_once = Rc::new(Cell::new(false));

    for (index, slot_button) in custom_slot_buttons.iter().enumerate() {
        let slot_overlay = custom_slot_overlays[index].clone();

        let drag_source = DragSource::new();
        drag_source.set_actions(gdk::DragAction::MOVE);
        let transparent_drag_icon = transparent_drag_icon_texture();
        let custom_slot_colors_drag = custom_slot_colors.clone();
        let dragged_custom_slot_index_prepare = dragged_custom_slot_index.clone();
        let suppress_custom_slot_click_once_prepare = suppress_custom_slot_click_once.clone();
        drag_source.connect_prepare(move |source, _, _| {
            if custom_slot_colors_drag.borrow()[index].is_none() {
                return None;
            }

            if let Some(icon) = transparent_drag_icon.as_ref() {
                source.set_icon(Some(icon), 0, 0);
            } else {
                source.set_icon(None::<&gdk::Paintable>, 0, 0);
            }

            dragged_custom_slot_index_prepare.set(Some(index));
            suppress_custom_slot_click_once_prepare.set(true);
            let value = glib::Value::from(index as u32);
            Some(gdk::ContentProvider::for_value(&value))
        });
        let dragged_custom_slot_index_end = dragged_custom_slot_index.clone();
        let suppress_custom_slot_click_once_end = suppress_custom_slot_click_once.clone();
        drag_source.connect_drag_end(move |_, _, _| {
            dragged_custom_slot_index_end.set(None);
            suppress_custom_slot_click_once_end.set(true);
        });
        slot_overlay.add_controller(drag_source);

        let drop_target = DropTarget::new(glib::Type::U32, gdk::DragAction::MOVE);
        let custom_slot_colors_drop = custom_slot_colors.clone();
        let refresh_custom_color_slots_drop = refresh_custom_color_slots.clone();
        let suppress_custom_slot_click_once_drop = suppress_custom_slot_click_once.clone();
        drop_target.connect_drop(move |_, value, _, _| {
            let Ok(from_index_raw) = value.get::<u32>() else {
                return false;
            };

            let moved = {
                let mut colors = custom_slot_colors_drop.borrow_mut();
                move_custom_color_between_slots(
                    colors.as_mut_slice(),
                    from_index_raw as usize,
                    index,
                )
            };

            if moved {
                refresh_custom_color_slots_drop();
                save_persisted_custom_slot_colors(custom_slot_colors_drop.borrow().as_slice());
                suppress_custom_slot_click_once_drop.set(true);
            }

            moved
        });
        slot_overlay.add_controller(drop_target);

        let custom_slot_colors_click = custom_slot_colors.clone();
        let apply_picker_color_to_editor_click = apply_picker_color_to_editor.clone();
        let sync_picker_from_color_click = sync_picker_from_color.clone();
        let color_popover_click = color_popover.clone();
        let dragged_custom_slot_index_click = dragged_custom_slot_index.clone();
        let suppress_custom_slot_click_once_click = suppress_custom_slot_click_once.clone();
        slot_button.connect_clicked(move |_| {
            if dragged_custom_slot_index_click.get().is_some() {
                return;
            }

            if suppress_custom_slot_click_once_click.replace(false) {
                return;
            }

            let Some(color) = custom_slot_colors_click.borrow()[index] else {
                return;
            };

            apply_picker_color_to_editor_click(color);
            sync_picker_from_color_click(color);
            color_popover_click.popdown();
        });
    }

    for (index, remove_button) in custom_slot_remove_buttons.iter().enumerate() {
        let custom_slot_colors_remove = custom_slot_colors.clone();
        let refresh_custom_color_slots_remove = refresh_custom_color_slots.clone();
        remove_button.connect_clicked(move |_| {
            let mut custom_colors = custom_slot_colors_remove.borrow_mut();
            if custom_colors[index].is_none() {
                return;
            }

            custom_colors[index] = None;
            save_persisted_custom_slot_colors(custom_colors.as_slice());
            drop(custom_colors);
            refresh_custom_color_slots_remove();
        });
    }

    ColorPickerParts {
        trigger_host: color_picker_trigger_host,
        popover: color_popover,
        color_buttons,
        color_picker_dot,
        color_class_names,
        eyedropper_btn,
        sync_for_active_tool: sync_picker_for_active_tool,
        sync_picker_from_color,
        apply_picker_color: apply_picker_color_to_editor,
        set_picker_panel_visibility,
    }
}

pub fn set_active_color_picker_state(
    color_buttons: &[Button],
    trigger_dot: &GtkBox,
    color_classes: &[&str],
    active_index: usize,
) {
    super::super::ui_support::set_active_color_button(color_buttons, active_index);
    set_color_picker_trigger_dot_state(trigger_dot, color_classes, active_index);
}

pub fn clear_active_color_picker_palette_state(color_buttons: &[Button]) {
    for button in color_buttons {
        button.remove_css_class("active-color");
    }
}

pub fn clear_color_picker_trigger_dot_state(trigger_dot: &GtkBox, color_classes: &[&str]) {
    for class_name in color_classes {
        trigger_dot.remove_css_class(class_name);
    }
}

pub fn set_color_picker_trigger_dot_state(
    trigger_dot: &GtkBox,
    color_classes: &[&str],
    active_index: usize,
) {
    clear_color_picker_trigger_dot_state(trigger_dot, color_classes);

    if let Some(class_name) = color_classes.get(active_index) {
        trigger_dot.add_css_class(*class_name);
    }
}

pub fn connect_eyedropper_activation(
    eyedropper_btn: &Button,
    color_popover: &Popover,
    state: Arc<Mutex<EditorState>>,
    eyedropper_mode: Rc<Cell<bool>>,
    eyedropper_point: Rc<RefCell<Option<Point>>>,
    eyedropper_rendered: Rc<RefCell<Option<RgbaImage>>>,
    canvas_eyedropper_ring: &DrawingArea,
    drawing_area: &DrawingArea,
    set_cursor_crosshair: Rc<dyn Fn()>,
) {
    let color_popover = color_popover.clone();
    let canvas_eyedropper_ring = canvas_eyedropper_ring.clone();
    let drawing_area = drawing_area.downgrade();
    eyedropper_btn.connect_clicked(move |_| {
        color_popover.popdown();
        eyedropper_mode.set(true);
        *eyedropper_point.borrow_mut() = None;
        *eyedropper_rendered.borrow_mut() = state.lock().unwrap().to_rendered_image().ok();
        canvas_eyedropper_ring.set_visible(false);
        canvas_eyedropper_ring.queue_draw();
        set_cursor_crosshair();

        if let Some(area) = drawing_area.upgrade() {
            area.queue_draw();
        }
    });
}
