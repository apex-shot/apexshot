use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CssProvider, DrawingArea, EventControllerMotion, GestureClick, Label,
    Orientation, Overlay, Scale,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::background_panel::BACKGROUND_SIDEBAR_WIDTH;
use super::super::color::{
    draw_color_to_hex, draw_color_to_rgba_u8, palette_index_for_color, parse_alpha_percent,
    parse_channel_u8, parse_hex_rgb, picker_dynamic_css, save_persisted_custom_slot_colors,
    DRAW_COLORS,
};
use super::super::state::EditorState;
use super::super::types::{BackgroundStyle, DrawColor, PickerColorState, Tool};
use super::super::ui_support::{color_swatch_button, set_active_color_button};
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
) -> ColorsPanelParts {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.add_css_class("editor-colors-panel");
    root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);
    root.set_hexpand(true);
    root.set_halign(gtk4::Align::Fill);
    root.set_vexpand(true);

    let content = GtkBox::new(Orientation::Vertical, 12);
    content.set_hexpand(true);
    content.set_halign(gtk4::Align::Fill);

    let helper = Label::new(Some("Choose a color for the active tool"));
    helper.add_css_class("editor-colors-panel-helper");
    helper.set_wrap(true);
    helper.set_max_width_chars(26);
    helper.set_xalign(0.0);

    let picker_state = Rc::new(RefCell::new(PickerColorState::from_color(DRAW_COLORS[0])));
    let picker_update_in_progress = Rc::new(Cell::new(false));
    let picker_css_provider = CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &picker_css_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }

    let spectrum_section = GtkBox::new(Orientation::Vertical, 8);
    spectrum_section.add_css_class("editor-colors-panel-section");
    spectrum_section.set_hexpand(true);
    spectrum_section.set_halign(gtk4::Align::Fill);

    let gradient_area = DrawingArea::new();
    gradient_area.set_content_width(BACKGROUND_SIDEBAR_WIDTH);
    gradient_area.set_content_height(140);
    gradient_area.set_size_request(BACKGROUND_SIDEBAR_WIDTH, 140);
    gradient_area.set_halign(gtk4::Align::Fill);
    gradient_area.set_hexpand(true);
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

    let hue_slider = Scale::with_range(Orientation::Horizontal, 0.0, 360.0, 1.0);
    hue_slider.set_draw_value(false);
    hue_slider.set_hexpand(true);
    hue_slider.set_halign(gtk4::Align::Fill);
    hue_slider.add_css_class("editor-hue-slider");

    let opacity_slider = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    opacity_slider.set_draw_value(false);
    opacity_slider.set_hexpand(true);
    opacity_slider.set_halign(gtk4::Align::Fill);
    opacity_slider.add_css_class("editor-opacity-slider");
    opacity_slider.set_widget_name("editor-sidebar-picker-opacity-slider");

    let hex_entry = gtk4::Entry::new();
    hex_entry.set_max_length(6);
    hex_entry.set_width_chars(6);
    hex_entry.set_max_width_chars(6);
    hex_entry.set_halign(gtk4::Align::Fill);
    hex_entry.set_hexpand(true);
    gtk4::prelude::EditableExt::set_alignment(&hex_entry, 0.5);
    hex_entry.add_css_class("editor-hex-entry");

    let hex_label = Label::new(Some("HEX"));
    hex_label.add_css_class("editor-color-field-label");
    hex_label.set_halign(gtk4::Align::Center);
    hex_label.set_xalign(0.5);

    let hex_row = GtkBox::new(Orientation::Vertical, 2);
    hex_row.set_halign(gtk4::Align::Fill);
    hex_row.set_hexpand(true);
    hex_row.append(&hex_entry);
    hex_row.append(&hex_label);

    let rgba_row = GtkBox::new(Orientation::Horizontal, 6);
    rgba_row.set_halign(gtk4::Align::Fill);
    rgba_row.set_hexpand(true);
    rgba_row.set_homogeneous(true);

    let r_entry = gtk4::Entry::new();
    r_entry.set_max_length(3);
    r_entry.set_width_chars(3);
    r_entry.set_max_width_chars(3);
    r_entry.set_halign(gtk4::Align::Fill);
    r_entry.set_hexpand(true);
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

    let g_entry = gtk4::Entry::new();
    g_entry.set_max_length(3);
    g_entry.set_width_chars(3);
    g_entry.set_max_width_chars(3);
    g_entry.set_halign(gtk4::Align::Fill);
    g_entry.set_hexpand(true);
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

    let b_entry = gtk4::Entry::new();
    b_entry.set_max_length(3);
    b_entry.set_width_chars(3);
    b_entry.set_max_width_chars(3);
    b_entry.set_halign(gtk4::Align::Fill);
    b_entry.set_hexpand(true);
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

    let a_entry = gtk4::Entry::new();
    a_entry.set_max_length(3);
    a_entry.set_width_chars(3);
    a_entry.set_max_width_chars(3);
    a_entry.set_halign(gtk4::Align::Fill);
    a_entry.set_hexpand(true);
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

    spectrum_section.append(&gradient_area);
    spectrum_section.append(&hue_slider);
    spectrum_section.append(&opacity_slider);
    spectrum_section.append(&hex_row);
    spectrum_section.append(&rgba_row);

    let palette_section = GtkBox::new(Orientation::Vertical, 8);
    palette_section.add_css_class("editor-colors-panel-section");
    palette_section.set_hexpand(true);
    palette_section.set_halign(gtk4::Align::Fill);

    let palette_title = Label::new(Some("Palette"));
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

    let custom_title = Label::new(Some("My colors"));
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
            remove_btn.set_tooltip_text(Some("Remove custom color"));
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
            let refresh_shared_custom_color_slots_remove = refresh_shared_custom_color_slots.clone();
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

    let add_current_color_btn = Button::with_label("+ Add current color");
    add_current_color_btn.set_has_frame(false);
    add_current_color_btn.set_halign(gtk4::Align::Fill);
    add_current_color_btn.set_hexpand(true);
    add_current_color_btn.add_css_class("editor-add-to-colors-button");
    add_current_color_btn.add_css_class("editor-colors-panel-action-button");

    let pick_from_screen_btn = Button::with_label("Pick from screen");
    pick_from_screen_btn.set_has_frame(false);
    pick_from_screen_btn.set_halign(gtk4::Align::Fill);
    pick_from_screen_btn.set_hexpand(true);
    pick_from_screen_btn.add_css_class("editor-colors-panel-action-button");

    actions.append(&add_current_color_btn);
    actions.append(&pick_from_screen_btn);

    content.append(&helper);
    content.append(&spectrum_section);
    content.append(&palette_section);
    content.append(&custom_section);
    content.append(&actions);
    root.append(&content);

    let refresh_custom_slots: Rc<dyn Fn()> = Rc::new({
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
    *local_refresh.borrow_mut() = Some(refresh_custom_slots.clone());

    let apply_picker_state_color: Rc<dyn Fn()> = Rc::new({
        let picker_state = picker_state.clone();
        let apply_picker_color = apply_picker_color.clone();
        move || {
            let color = {
                let picker = picker_state.borrow();
                picker.to_color()
            };
            apply_picker_color(color);
        }
    });

    let update_picker_ui: Rc<dyn Fn(PickerColorState)> = Rc::new({
        let picker_update_in_progress = picker_update_in_progress.clone();
        let gradient_area = gradient_area.clone();
        let hue_slider = hue_slider.clone();
        let opacity_slider = opacity_slider.clone();
        let hex_entry = hex_entry.clone();
        let r_entry = r_entry.clone();
        let g_entry = g_entry.clone();
        let b_entry = b_entry.clone();
        let a_entry = a_entry.clone();
        let picker_css_provider = picker_css_provider.clone();
        move |picker| {
            let was_in_progress = picker_update_in_progress.get();
            picker_update_in_progress.set(true);
            // Only set slider values if not already in a user interaction
            // (sliders are already at the correct position during user interaction)
            if !was_in_progress {
                hue_slider.set_value(picker.hue);
                opacity_slider.set_value((picker.alpha * 100.0).clamp(0.0, 100.0));
            }
            let color = picker.to_color();
            let (r, g, b, _): (u8, u8, u8, u8) = draw_color_to_rgba_u8(color);
            hex_entry.set_text(&draw_color_to_hex(color));
            r_entry.set_text(&r.to_string());
            g_entry.set_text(&g.to_string());
            b_entry.set_text(&b.to_string());
            a_entry.set_text(&(picker.alpha * 100.0).round().to_string());
            gradient_area.queue_draw();
            let css = picker_dynamic_css(color)
                .replace("#editor-picker-opacity-slider", "#editor-sidebar-picker-opacity-slider");
            picker_css_provider.load_from_data(&css);
            picker_update_in_progress.set(was_in_progress);
        }
    });

    let commit_picker_state: Rc<dyn Fn()> = Rc::new({
        let picker_state = picker_state.clone();
        let update_picker_ui = update_picker_ui.clone();
        let apply_picker_state_color = apply_picker_state_color.clone();
        move || {
            let picker = *picker_state.borrow();
            update_picker_ui(picker);
            apply_picker_state_color();
        }
    });

    hue_slider.connect_value_changed({
        let picker_state = picker_state.clone();
        let picker_update_in_progress = picker_update_in_progress.clone();
        let commit_picker_state = commit_picker_state.clone();
        move |slider| {
            if picker_update_in_progress.get() {
                return;
            }
            picker_update_in_progress.set(true);
            picker_state.borrow_mut().hue = super::super::types::normalize_hue(slider.value());
            commit_picker_state();
            picker_update_in_progress.set(false);
        }
    });

    opacity_slider.connect_value_changed({
        let picker_state = picker_state.clone();
        let picker_update_in_progress = picker_update_in_progress.clone();
        let commit_picker_state = commit_picker_state.clone();
        move |slider| {
            if picker_update_in_progress.get() {
                return;
            }
            picker_update_in_progress.set(true);
            picker_state.borrow_mut().alpha = (slider.value() / 100.0).clamp(0.0, 1.0);
            commit_picker_state();
            picker_update_in_progress.set(false);
        }
    });

    hex_entry.connect_changed({
        let picker_state = picker_state.clone();
        let picker_update_in_progress = picker_update_in_progress.clone();
        let commit_picker_state = commit_picker_state.clone();
        move |entry| {
            if picker_update_in_progress.get() {
                return;
            }

            let text = entry.text();
            let Some((r, g, b)) = parse_hex_rgb(text.as_str()) else {
                return;
            };
            let (hue, saturation, value) =
                super::super::types::rgb_to_hsv(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
            picker_update_in_progress.set(true);
            {
                let mut picker = picker_state.borrow_mut();
                picker.hue = hue;
                picker.saturation = saturation;
                picker.value = value;
            }
            commit_picker_state();
            picker_update_in_progress.set(false);
        }
    });

    let update_picker_from_rgba_entries: Rc<dyn Fn()> = Rc::new({
        let picker_state = picker_state.clone();
        let picker_update_in_progress = picker_update_in_progress.clone();
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
            let (hue, saturation, value) =
                super::super::types::rgb_to_hsv(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
            picker_update_in_progress.set(true);
            {
                let mut picker = picker_state.borrow_mut();
                picker.hue = hue;
                picker.saturation = saturation;
                picker.value = value;
                picker.alpha = alpha;
            }
            commit_picker_state();
            picker_update_in_progress.set(false);
        }
    });

    for entry in [&r_entry, &g_entry, &b_entry, &a_entry] {
        let picker_update_in_progress = picker_update_in_progress.clone();
        let update_picker_from_rgba_entries = update_picker_from_rgba_entries.clone();
        entry.connect_changed(move |_| {
            if picker_update_in_progress.get() {
                return;
            }
            update_picker_from_rgba_entries();
        });
    }

    let update_sv_from_position: Rc<dyn Fn(f64, f64)> = Rc::new({
        let gradient_area = gradient_area.clone();
        let picker_state = picker_state.clone();
        let picker_update_in_progress = picker_update_in_progress.clone();
        let commit_picker_state = commit_picker_state.clone();
        move |x, y| {
            let width = gradient_area.allocated_width().max(1) as f64;
            let height = gradient_area.allocated_height().max(1) as f64;
            let saturation = (x / width).clamp(0.0, 1.0);
            let value = (1.0 - (y / height)).clamp(0.0, 1.0);
            picker_update_in_progress.set(true);
            {
                let mut picker = picker_state.borrow_mut();
                picker.saturation = saturation;
                picker.value = value;
            }
            commit_picker_state();
            picker_update_in_progress.set(false);
        }
    });

    let gradient_dragging = Rc::new(Cell::new(false));
    let gradient_click = GestureClick::new();
    {
        let gradient_dragging = gradient_dragging.clone();
        let update_sv_from_position = update_sv_from_position.clone();
        gradient_click.connect_pressed(move |_, _, x, y| {
            gradient_dragging.set(true);
            update_sv_from_position(x, y);
        });
    }
    {
        let gradient_dragging = gradient_dragging.clone();
        gradient_click.connect_released(move |_, _, _, _| {
            gradient_dragging.set(false);
        });
    }
    gradient_area.add_controller(gradient_click);

    let gradient_motion = EventControllerMotion::new();
    {
        let gradient_dragging = gradient_dragging.clone();
        let update_sv_from_position = update_sv_from_position.clone();
        gradient_motion.connect_motion(move |_, x, y| {
            if gradient_dragging.get() {
                update_sv_from_position(x, y);
            }
        });
    }
    gradient_area.add_controller(gradient_motion);

    let sync_for_active_tool: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let helper = helper.clone();
        let palette_buttons = palette_buttons.clone();
        let refresh_custom_slots = refresh_custom_slots.clone();
        let picker_state = picker_state.clone();
        let picker_update_in_progress = picker_update_in_progress.clone();
        let update_picker_ui = update_picker_ui.clone();
        move || {
            // Don't reset picker state while we're in the middle of an update
            // (e.g., dragging on the gradient area)
            if picker_update_in_progress.get() {
                return;
            }

            let (selected_tool, active_color) = {
                let st = state.lock().unwrap();
                let selected_tool = st.selected_tool;
                let active_color = if selected_tool == Tool::Background {
                    if let BackgroundStyle::PlainColor(color) = st.background_style {
                        color
                    } else {
                        st.selected_color
                    }
                } else {
                    st.selected_color
                };
                (selected_tool, active_color)
            };

            helper.set_label(if selected_tool == Tool::Background {
                "Choose the solid color used when Background is set to plain color"
            } else {
                "Choose a color for the active tool"
            });

            set_active_color_button(&palette_buttons, palette_index_for_color(active_color));
            let picker = PickerColorState::from_color(active_color);
            *picker_state.borrow_mut() = picker;
            update_picker_ui(picker);
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
                && production_source.contains("let spectrum_section = GtkBox::new(Orientation::Vertical, 8);")
                && production_source.contains("content.append(&spectrum_section);"),
            "Colors panel should expose shared color management controls",
        );
    }

    #[test]
    fn colors_panel_embeds_compact_picker_fields_without_hue_preview_box() {
        let source = include_str!("colors_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production_source.contains("let hue_preview = GtkBox::new(Orientation::Horizontal, 0);")
                && production_source.contains("let hex_entry = gtk4::Entry::new();")
                && production_source.contains("let rgba_row = GtkBox::new(Orientation::Horizontal, 6);")
                && production_source.contains("let r_entry = gtk4::Entry::new();")
                && production_source.contains("let g_entry = gtk4::Entry::new();")
                && production_source.contains("let b_entry = gtk4::Entry::new();")
                && production_source.contains("let a_entry = gtk4::Entry::new();"),
            "Colors panel should include compact HEX/RGBA fields and remove the hue preview box",
        );
    }

    #[test]
    fn colors_panel_gradient_area_no_longer_reflects_alpha_slider_visually() {
        let source = include_str!("colors_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production_source.contains("cr.push_group();")
                && !production_source.contains("cr.paint_with_alpha(picker.alpha.clamp(0.0, 1.0));"),
            "Colors panel gradient area should use the reverted opaque rendering path",
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
                && production_source.contains("root.set_hexpand(true);")
                && production_source.contains("root.set_halign(gtk4::Align::Fill);")
                && production_source.contains("content.set_hexpand(true);")
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
                && production_source.contains("let actions = GtkBox::new(Orientation::Vertical, 8);")
                && production_source.contains("actions.set_hexpand(true);")
                && production_source.contains("add_current_color_btn.set_hexpand(true);")
                && production_source.contains("pick_from_screen_btn.set_hexpand(true);")
                && !production_source.contains("palette_grid.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
                && !production_source.contains("custom_grid.set_width_request(BACKGROUND_SIDEBAR_WIDTH);"),
            "Colors panel should use the same content width as the Background panel",
        );
    }
}
