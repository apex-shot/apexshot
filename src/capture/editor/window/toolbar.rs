use gtk4::{
    prelude::*, ApplicationWindow, Box as GtkBox, Button, CenterBox, Entry, GestureClick, Image,
    Label, MenuButton, Orientation, Overlay, Popover, Scale, Stack,
};
use std::rc::Rc;

use super::super::{
    pen_weight::PenWeight,
    types::{ObfuscateMethod, Tool},
    ui_support::{
        icon_tool_button, recommended_window_size, recommended_window_size_with_extra_width,
        traffic_light_button,
    },
};
use super::background_panel::BACKGROUND_SIDEBAR_WIDTH;

pub(super) struct ToolbarBaseParts {
    pub root: CenterBox,
    pub traffic_close: Button,
    pub traffic_minimize: Button,
    pub traffic_zoom: Button,
    pub select_btn: Button,
    pub crop_btn: Button,
    pub background_btn: Button,
    pub draw_btn: Button,
    pub arrow_btn: Button,
    pub line_btn: Button,
    pub box_btn: Button,
    pub circle_btn: Button,
    pub text_btn: Button,
    pub number_btn: Button,
    pub highlighter_btn: Button,
    pub obfuscate_btn: Button,
    pub focus_btn: Button,
    pub sep_1: GtkBox,
    pub sep_2: GtkBox,
}

#[allow(dead_code)]
pub(super) struct ToolbarBaseIconNames<'a> {
    pub crop: &'a str,
    pub draw: &'a str,
    pub arrow: &'a str,
    pub line: &'a str,
    pub box_: &'a str,
    pub circle: &'a str,
    pub text: &'a str,
    pub number: &'a str,
    pub highlighter: &'a str,
    pub obfuscate: &'a str,
    pub focus: &'a str,
    #[allow(dead_code)]
    pub obfuscate_pixelate: &'a str,
    #[allow(dead_code)]
    pub obfuscate_blur_secure: &'a str,
    #[allow(dead_code)]
    pub obfuscate_blur_smooth: &'a str,
    #[allow(dead_code)]
    pub obfuscate_blackout: &'a str,
}

pub(super) struct ToolbarRightParts {
    pub root: GtkBox,
    pub undo_btn: Button,
    pub redo_btn: Button,
    pub delete_selected_btn: Button,
    pub save_btn: Button,
    pub apply_crop_btn: Button,
}

pub(super) struct ToolbarModeParts {
    pub root: GtkBox,
    pub toolbar_mode_stack: Stack,
    pub size_group: GtkBox,
    pub size_slider: gtk4::Scale,
    pub text_size_group: GtkBox,
    pub text_size_label: Label,
    pub text_size_list: GtkBox,
    pub font_family_group: GtkBox,
    pub font_family_label: Label,
    pub font_family_list: GtkBox,
    pub crop_type_label: Label,
    pub crop_type_popover: Popover,
    pub crop_type_list: GtkBox,
    pub crop_width_entry: Entry,
    pub crop_height_entry: Entry,
    pub obfuscate_method_group: GtkBox,
    #[allow(dead_code)]
    pub obfuscate_method_button: Button,
    #[allow(dead_code)]
    pub obfuscate_method_popover: Popover,
    pub obfuscate_method_list: GtkBox,
    /// Pen weight selector for highlighter
    pub pen_weight_button: gtk4::MenuButton,
    #[allow(dead_code)]
    pub pen_weight_popover: gtk4::Popover,
    pub pen_weight_list: gtk4::Box,
}

pub(super) fn build_toolbar_base(icon_names: ToolbarBaseIconNames<'_>) -> ToolbarBaseParts {
    let root = CenterBox::new();
    root.add_css_class("editor-toolbar");

    let traffic_close = traffic_light_button("traffic-light-red", "Close");
    let traffic_minimize = traffic_light_button("traffic-light-yellow", "Minimize");
    let traffic_zoom = traffic_light_button("traffic-light-green", "Zoom");

    let traffic_lights = GtkBox::new(Orientation::Horizontal, 6);
    traffic_lights.add_css_class("editor-traffic-lights");
    traffic_lights.append(&traffic_close);
    traffic_lights.append(&traffic_minimize);
    traffic_lights.append(&traffic_zoom);

    let select_btn = icon_tool_button("pointer-primary-click-symbolic", "Select");
    let crop_btn = icon_tool_button(icon_names.crop, "Crop");
    crop_btn.add_css_class("standalone-tool");
    let background_btn = icon_tool_button("image-x-generic-symbolic", "Background");
    background_btn.add_css_class("standalone-tool");
    let draw_btn = icon_tool_button(icon_names.draw, "Pen");

    let left_group = GtkBox::new(Orientation::Horizontal, 16);
    left_group.add_css_class("editor-toolbar-left");
    left_group.append(&traffic_lights);
    root.set_start_widget(Some(&left_group));

    let arrow_btn = icon_tool_button(icon_names.arrow, "Arrow");
    let line_btn = icon_tool_button(icon_names.line, "Line");
    let box_btn = icon_tool_button(icon_names.box_, "Box");
    let circle_btn = icon_tool_button(icon_names.circle, "Circle");
    let text_btn = icon_tool_button(icon_names.text, "Text");
    let number_btn = icon_tool_button(icon_names.number, "Number");
    let highlighter_btn = icon_tool_button(icon_names.highlighter, "Highlighter");
    let obfuscate_btn = icon_tool_button(icon_names.obfuscate, "Obfuscate");
    let focus_btn = icon_tool_button(icon_names.focus, "Focus");

    let sep_1 = GtkBox::new(Orientation::Vertical, 0);
    sep_1.add_css_class("editor-tools-divider");
    sep_1.set_vexpand(true);

    let sep_2 = GtkBox::new(Orientation::Vertical, 0);
    sep_2.add_css_class("editor-tools-divider");
    sep_2.set_vexpand(true);

    ToolbarBaseParts {
        root,
        traffic_close,
        traffic_minimize,
        traffic_zoom,
        select_btn,
        crop_btn,
        background_btn,
        draw_btn,
        arrow_btn,
        line_btn,
        box_btn,
        circle_btn,
        text_btn,
        number_btn,
        highlighter_btn,
        obfuscate_btn,
        focus_btn,
        sep_1,
        sep_2,
    }
}

pub(super) fn build_obfuscate_method_controls() -> (
    GtkBox,
    Button,
    Popover,
    GtkBox,
) {
    let obfuscate_method_group = GtkBox::new(Orientation::Horizontal, 4);
    obfuscate_method_group.add_css_class("editor-obfuscate-method-group");
    obfuscate_method_group.add_css_class("editor-tools-group");
    obfuscate_method_group.set_visible(false);

    let obfuscate_method_button = Button::new();
    obfuscate_method_button.set_has_frame(false);
    obfuscate_method_button.set_focusable(false);
    obfuscate_method_button.add_css_class("editor-tool-button");
    obfuscate_method_button.add_css_class("flat");
    obfuscate_method_button.set_tooltip_text(Some("Obfuscate method"));

    let obfuscate_method_icon = Image::from_icon_name("view-grid-symbolic");
    obfuscate_method_button.set_child(Some(&obfuscate_method_icon));

    let obfuscate_method_popover = Popover::new();
    obfuscate_method_popover.set_has_arrow(false);
    obfuscate_method_popover.set_autohide(true);
    obfuscate_method_popover.add_css_class("editor-popover");
    obfuscate_method_popover.set_parent(&obfuscate_method_button);

    let obfuscate_method_list = GtkBox::new(Orientation::Vertical, 0);
    obfuscate_method_list.add_css_class("editor-popover-list");
    obfuscate_method_popover.set_child(Some(&obfuscate_method_list));

    let p_popover = obfuscate_method_popover.clone();
    obfuscate_method_button.connect_clicked(move |_| {
        p_popover.popup();
    });

    obfuscate_method_group.append(&obfuscate_method_button);

    (
        obfuscate_method_group,
        obfuscate_method_button,
        obfuscate_method_popover,
        obfuscate_method_list,
    )
}

fn build_pen_weight_dropdown() -> (gtk4::MenuButton, gtk4::Popover, gtk4::Box) {
    let button = gtk4::MenuButton::new();
    button.add_css_class("editor-pen-weight-button");
    button.set_tooltip_text(Some("Pen Weight"));

    // Use icon_name for MenuButton
    button.set_icon_name("document-edit-symbolic");

    let popover = gtk4::Popover::new();
    popover.add_css_class("editor-pen-weight-popover");

    let list = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    list.add_css_class("editor-pen-weight-list");
    list.set_margin_top(8);
    list.set_margin_bottom(8);
    list.set_margin_start(8);
    list.set_margin_end(8);

    // Add pen weight options
    for weight in PenWeight::ALL {
        let item = gtk4::Button::new();
        item.add_css_class("editor-pen-weight-item");

        let box_ = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);

        // Pen icon with visual weight representation
        let pen_icon = gtk4::Image::from_icon_name(weight.icon_name());
        pen_icon.set_pixel_size(24);
        box_.append(&pen_icon);

        let label = gtk4::Label::new(Some(weight.label()));
        box_.append(&label);

        item.set_child(Some(&box_));
        list.append(&item);
    }

    popover.set_child(Some(&list));
    button.set_popover(Some(&popover));

    (button, popover, list)
}

pub(super) fn build_toolbar_mode_controls(
    crop_btn: &Button,
    background_btn: &Button,
    select_btn: &Button,
    draw_btn: &Button,
    box_btn: &Button,
    circle_btn: &Button,
    arrow_btn: &Button,
    line_btn: &Button,
    text_btn: &Button,
    _text_italic_icon: &str,
    obfuscate_btn: &Button,
    focus_btn: &Button,
    number_btn: &Button,
    highlighter_btn: &Button,
    sep_1: &GtkBox,
    sep_2: &GtkBox,
    color_picker_trigger_host: &Overlay,
) -> ToolbarModeParts {
    let color_group = GtkBox::new(Orientation::Horizontal, 0);
    color_group.add_css_class("editor-color-group");
    color_group.append(color_picker_trigger_host);

    let size_slider = Scale::with_range(Orientation::Horizontal, 1.0, 24.0, 1.0);
    size_slider.add_css_class("editor-toolbar-size-slider");
    size_slider.set_draw_value(false);
    size_slider.set_size_request(100, -1);
    size_slider.set_halign(gtk4::Align::Center);
    size_slider.set_valign(gtk4::Align::Center);
    size_slider.set_tooltip_text(Some("Stroke size"));

    let size_group = GtkBox::new(Orientation::Horizontal, 0);
    size_group.add_css_class("editor-tools-group");
    size_group.add_css_class("editor-size-group");
    size_group.append(&size_slider);

    // Text Size Picker
    let text_size_button = Button::new();
    text_size_button.set_has_frame(false);
    text_size_button.set_focusable(false);
    text_size_button.add_css_class("editor-tool-button");
    text_size_button.add_css_class("flat");
    text_size_button.add_css_class("editor-text-size-button");
    text_size_button.set_tooltip_text(Some("Text size"));

    let text_size_button_box = GtkBox::new(Orientation::Horizontal, 2);
    text_size_button_box.set_halign(gtk4::Align::Center);
    text_size_button_box.set_valign(gtk4::Align::Center);
    let text_size_label = Label::new(Some("24pt"));
    text_size_label.add_css_class("editor-text-size-label");
    let text_size_arrow = Image::from_icon_name("pan-down-symbolic");
    text_size_arrow.set_pixel_size(10);
    text_size_arrow.add_css_class("editor-text-size-arrow");
    text_size_button_box.append(&text_size_label);
    text_size_button_box.append(&text_size_arrow);
    text_size_button.set_child(Some(&text_size_button_box));

    let text_size_popover = Popover::new();
    text_size_popover.set_has_arrow(false);
    text_size_popover.set_autohide(true);
    text_size_popover.add_css_class("editor-popover");
    text_size_popover.set_parent(&text_size_button);
    let text_size_list = GtkBox::new(Orientation::Vertical, 0);
    text_size_list.add_css_class("editor-popover-list");
    text_size_popover.set_child(Some(&text_size_list));

    let p_size = text_size_popover.clone();
    text_size_button.connect_clicked(move |_| {
        p_size.popup();
    });

    for size in [12, 14, 16, 18, 20, 24, 28, 32, 36, 48, 64, 72] {
        let label = format!("{}pt", size);
        let btn = Button::builder()
            .label(&label)
            .has_frame(false)
            .css_classes(["editor-popover-list-item", "flat"])
            .build();
        text_size_list.append(&btn);
    }

    // Font Picker
    let font_family_button = Button::new();
    font_family_button.set_has_frame(false);
    font_family_button.set_focusable(false);
    font_family_button.add_css_class("editor-tool-button");
    font_family_button.add_css_class("flat");
    font_family_button.set_tooltip_text(Some("Font family"));

    let font_family_button_box = GtkBox::new(Orientation::Horizontal, 2);
    font_family_button_box.set_halign(gtk4::Align::Center);
    font_family_button_box.set_valign(gtk4::Align::Center);
    let font_family_label = Label::new(Some("Sans"));
    font_family_label.add_css_class("editor-font-family-label");
    let font_family_arrow = Image::from_icon_name("pan-down-symbolic");
    font_family_arrow.set_pixel_size(10);
    font_family_arrow.add_css_class("editor-font-family-arrow");
    font_family_button_box.append(&font_family_label);
    font_family_button_box.append(&font_family_arrow);
    font_family_button.set_child(Some(&font_family_button_box));

    let font_family_popover = Popover::new();
    font_family_popover.set_has_arrow(false);
    font_family_popover.set_autohide(true);
    font_family_popover.add_css_class("editor-popover");
    font_family_popover.set_parent(&font_family_button);
    let font_family_list = GtkBox::new(Orientation::Vertical, 0);
    font_family_list.add_css_class("editor-popover-list");
    font_family_popover.set_child(Some(&font_family_list));

    let p_font = font_family_popover.clone();
    font_family_button.connect_clicked(move |_| {
        p_font.popup();
    });

    for family in ["Sans", "Serif", "Monospace", "Fantasy", "Cursive"] {
        let btn = Button::builder()
            .label(family)
            .has_frame(false)
            .css_classes(["editor-popover-list-item", "flat"])
            .build();
        font_family_list.append(&btn);
    }

    let text_size_group = GtkBox::new(Orientation::Horizontal, 2);
    text_size_group.add_css_class("editor-tools-group");
    text_size_group.append(&text_size_button);
    text_size_group.set_visible(false);

    let font_family_group = GtkBox::new(Orientation::Horizontal, 2);
    font_family_group.add_css_class("editor-tools-group");
    font_family_group.append(&font_family_button);
    font_family_group.set_visible(false);

    let crop_tools_group = GtkBox::new(Orientation::Horizontal, 2);
    crop_tools_group.add_css_class("editor-tools-group");
    crop_tools_group.append(crop_btn);

    let background_tools_group = GtkBox::new(Orientation::Horizontal, 2);
    background_tools_group.add_css_class("editor-tools-group");
    background_tools_group.append(background_btn);

    let crop_type_button = MenuButton::new();
    crop_type_button.set_has_frame(false);
    crop_type_button.set_focusable(false);
    crop_type_button.set_icon_name("");
    crop_type_button.add_css_class("editor-crop-type-button");
    crop_type_button.add_css_class("editor-tool-button");
    crop_type_button.add_css_class("flat");
    crop_type_button.set_tooltip_text(Some("Crop type"));

    let crop_type_label = Label::new(Some("Freeform"));
    crop_type_label.add_css_class("editor-crop-type-label");
    crop_type_label.set_xalign(0.0);

    let crop_type_arrow_box = GtkBox::new(Orientation::Horizontal, 0);
    crop_type_arrow_box.add_css_class("editor-crop-type-arrow-box");
    crop_type_arrow_box.set_halign(gtk4::Align::Center);
    crop_type_arrow_box.set_valign(gtk4::Align::Center);
    let crop_type_arrow = Image::from_icon_name("pan-down-symbolic");
    crop_type_arrow.set_pixel_size(10);
    crop_type_arrow.add_css_class("editor-crop-type-arrow");
    crop_type_arrow_box.append(&crop_type_arrow);

    let crop_type_shell = GtkBox::new(Orientation::Horizontal, 8);
    crop_type_shell.add_css_class("editor-crop-type-shell");
    crop_type_shell.set_valign(gtk4::Align::Fill);
    crop_type_shell.append(&crop_type_label);
    crop_type_shell.append(&crop_type_arrow_box);

    let crop_type_host = Overlay::new();
    crop_type_host.set_size_request(68, 30);
    crop_type_host.set_valign(gtk4::Align::Center);
    crop_type_host.set_child(Some(&crop_type_shell));
    crop_type_host.add_overlay(&crop_type_button);
    crop_type_button.set_valign(gtk4::Align::Fill);
    crop_type_button.set_halign(gtk4::Align::Fill);

    let crop_type_popover = Popover::new();
    crop_type_popover.set_has_arrow(false);
    crop_type_popover.set_autohide(true);
    crop_type_popover.set_position(gtk4::PositionType::Bottom);
    crop_type_popover.set_offset(0, 4);
    crop_type_popover.add_css_class("editor-crop-type-popover");

    let crop_type_list = GtkBox::new(Orientation::Vertical, 4);
    crop_type_list.add_css_class("editor-crop-type-popover-body");

    crop_type_popover.set_child(Some(&crop_type_list));
    crop_type_button.set_popover(Some(&crop_type_popover));

    let crop_type_group = GtkBox::new(Orientation::Horizontal, 0);
    crop_type_group.add_css_class("editor-tools-group");
    crop_type_group.add_css_class("editor-crop-type-group");
    crop_type_group.append(&crop_type_host);

    let crop_type_shell_click = GestureClick::new();
    let crop_type_button_popup = crop_type_button.clone();
    crop_type_shell_click.connect_pressed(move |_, _, _, _| {
        crop_type_button_popup.popup();
    });
    crop_type_shell.add_controller(crop_type_shell_click);

    let crop_width_entry = Entry::new();
    crop_width_entry.set_editable(false);
    crop_width_entry.set_focusable(false);
    crop_width_entry.set_width_chars(5);
    crop_width_entry.set_max_width_chars(6);
    crop_width_entry.set_width_request(68);
    crop_width_entry.set_hexpand(false);
    gtk4::prelude::EditableExt::set_alignment(&crop_width_entry, 0.5);
    crop_width_entry.add_css_class("editor-crop-size-entry");

    let crop_size_separator = Label::new(Some("×"));
    crop_size_separator.add_css_class("editor-crop-size-separator");

    let crop_height_entry = Entry::new();
    crop_height_entry.set_editable(false);
    crop_height_entry.set_focusable(false);
    crop_height_entry.set_width_chars(5);
    crop_height_entry.set_max_width_chars(6);
    crop_height_entry.set_width_request(68);
    crop_height_entry.set_hexpand(false);
    gtk4::prelude::EditableExt::set_alignment(&crop_height_entry, 0.5);
    crop_height_entry.add_css_class("editor-crop-size-entry");

    // Build obfuscate method selector
    let (
        obfuscate_method_group,
        obfuscate_method_button,
        obfuscate_method_popover,
        obfuscate_method_list,
    ) = build_obfuscate_method_controls();

    // Populate obfuscate method list with options
    let methods = [
        (ObfuscateMethod::Pixelate, "view-grid-symbolic", "Pixelate"),
        (ObfuscateMethod::BlurSecure, "security-high-symbolic", "Blur (Secure)"),
        (ObfuscateMethod::BlurSmooth, "blur-symbolic", "Blur (Smooth)"),
        (ObfuscateMethod::Blackout, "media-playback-stop-symbolic", "Blackout"),
    ];

    for (_method, icon_name, label) in methods {
        let btn = Button::builder()
            .label(label)
            .has_frame(false)
            .css_classes(["editor-popover-list-item", "flat"])
            .build();

        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        let icon = Image::from_icon_name(icon_name);
        let label_widget = Label::new(Some(label));
        btn_box.append(&icon);
        btn_box.append(&label_widget);
        btn.set_child(Some(&btn_box));

        obfuscate_method_list.append(&btn);
    }

    // Build pen weight selector for highlighter
    let (pen_weight_button, pen_weight_popover, pen_weight_list) = build_pen_weight_dropdown();
    pen_weight_button.set_visible(false); // Initially hidden, shown when highlighter is selected

    let pen_weight_group = GtkBox::new(Orientation::Horizontal, 0);
    pen_weight_group.add_css_class("editor-tools-group");
    pen_weight_group.add_css_class("editor-pen-weight-group");
    pen_weight_group.append(&pen_weight_button);

    let crop_size_group = GtkBox::new(Orientation::Horizontal, 4);
    crop_size_group.add_css_class("editor-tools-group");
    crop_size_group.add_css_class("editor-crop-size-group");
    crop_size_group.append(&crop_width_entry);
    crop_size_group.append(&crop_size_separator);
    crop_size_group.append(&crop_height_entry);

    let crop_mode_group = GtkBox::new(Orientation::Horizontal, 8);
    crop_mode_group.add_css_class("editor-crop-mode-group");
    crop_mode_group.append(&crop_type_group);
    crop_mode_group.append(&crop_size_group);

    let primary_tools_group = GtkBox::new(Orientation::Horizontal, 2);
    primary_tools_group.add_css_class("editor-tools-group");
    primary_tools_group.add_css_class("editor-primary-tools-group");
    primary_tools_group.append(select_btn);
    primary_tools_group.append(draw_btn);
    primary_tools_group.append(sep_1);
    primary_tools_group.append(box_btn);
    primary_tools_group.append(circle_btn);
    primary_tools_group.append(arrow_btn);
    primary_tools_group.append(line_btn);
    primary_tools_group.append(text_btn);
    primary_tools_group.append(obfuscate_btn);
    primary_tools_group.append(focus_btn);
    primary_tools_group.append(number_btn);
    primary_tools_group.append(highlighter_btn);
    primary_tools_group.append(sep_2);

    let standard_mode_group = GtkBox::new(Orientation::Horizontal, 10);
    standard_mode_group.add_css_class("editor-toolbar-mode-group");
    standard_mode_group.append(&primary_tools_group);
    standard_mode_group.append(&obfuscate_method_group);
    standard_mode_group.append(&pen_weight_group);
    standard_mode_group.append(&size_group);

    let toolbar_mode_stack = Stack::new();
    toolbar_mode_stack.add_css_class("editor-toolbar-mode-stack");
    toolbar_mode_stack.set_hhomogeneous(false);
    toolbar_mode_stack.set_vhomogeneous(false);
    toolbar_mode_stack.add_named(&standard_mode_group, Some("standard"));
    toolbar_mode_stack.add_named(&crop_mode_group, Some("crop"));
    toolbar_mode_stack.set_visible_child_name("standard");

    let root = GtkBox::new(Orientation::Horizontal, 10);
    root.add_css_class("editor-toolbar-center");
    root.append(&crop_tools_group);
    root.append(&background_tools_group);
    root.append(&toolbar_mode_stack);
    root.append(&text_size_group);
    root.append(&font_family_group);
    root.append(&color_group);

    ToolbarModeParts {
        root,
        toolbar_mode_stack,
        size_group,
        size_slider,
        text_size_group,
        text_size_label,
        text_size_list,
        font_family_group,
        font_family_label,
        font_family_list,
        crop_type_label,
        crop_type_popover,
        crop_type_list,
        crop_width_entry,
        crop_height_entry,
        obfuscate_method_group,
        obfuscate_method_button,
        obfuscate_method_popover,
        obfuscate_method_list,
        pen_weight_button,
        pen_weight_popover,
        pen_weight_list,
    }
}

pub(super) fn build_toolbar_right_controls(
    undo_icon_name: &str,
    redo_icon_name: &str,
) -> ToolbarRightParts {
    let undo_btn = icon_tool_button(undo_icon_name, "Undo");
    let redo_btn = icon_tool_button(redo_icon_name, "Redo");
    let delete_selected_btn = icon_tool_button("edit-delete-symbolic", "Delete selected");
    undo_btn.set_sensitive(false);
    redo_btn.set_sensitive(false);
    delete_selected_btn.set_sensitive(false);

    let history_group = GtkBox::new(Orientation::Horizontal, 2);
    history_group.add_css_class("editor-tools-group");
    history_group.append(&undo_btn);
    history_group.append(&redo_btn);
    history_group.append(&delete_selected_btn);

    let right_tools = GtkBox::new(Orientation::Horizontal, 12);
    right_tools.add_css_class("editor-toolbar-right-tools");
    right_tools.append(&history_group);

    let save_btn = Button::with_label("Done");
    save_btn.set_has_frame(false);
    save_btn.add_css_class("editor-done-button");
    save_btn.add_css_class("body");
    save_btn.set_valign(gtk4::Align::Center);

    let apply_crop_btn = Button::with_label("Apply");
    apply_crop_btn.set_has_frame(false);
    apply_crop_btn.add_css_class("editor-done-button");
    apply_crop_btn.add_css_class("body");
    apply_crop_btn.set_valign(gtk4::Align::Center);
    apply_crop_btn.set_visible(false);
    apply_crop_btn.set_sensitive(false);

    let apply_crop_slot = GtkBox::new(Orientation::Horizontal, 0);
    apply_crop_slot.add_css_class("crop-apply-slot");
    apply_crop_slot.append(&apply_crop_btn);
    apply_crop_slot.set_visible(false);

    let root = GtkBox::new(Orientation::Horizontal, 16);
    root.add_css_class("editor-toolbar-right");
    root.append(&right_tools);
    root.append(&apply_crop_slot);
    root.append(&save_btn);

    ToolbarRightParts {
        root,
        undo_btn,
        redo_btn,
        delete_selected_btn,
        save_btn,
        apply_crop_btn,
    }
}

pub(super) fn build_toolbar_tool_updater(
    toolbar_mode_stack: &Stack,
    background_sidebar: &GtkBox,
    text_size_group: &GtkBox,
    font_family_group: &GtkBox,
    obfuscate_method_group: &GtkBox,
    canvas_scroller: &gtk4::ScrolledWindow,
    start_background_gradient_preview_loading: Rc<dyn Fn()>,
    window: &ApplicationWindow,
    image_width: i32,
    image_height: i32,
) -> Rc<dyn Fn(Tool)> {
    let toolbar_mode_stack = toolbar_mode_stack.clone();
    let background_sidebar = background_sidebar.clone();
    let text_size_group = text_size_group.clone();
    let font_family_group = font_family_group.clone();
    let obfuscate_method_group = obfuscate_method_group.clone();
    let canvas_scroller = canvas_scroller.clone();
    let window = window.downgrade();

    Rc::new(move |tool| {
        toolbar_mode_stack.set_visible_child_name(if matches!(tool, Tool::Crop) {
            "crop"
        } else {
            "standard"
        });

        let is_text_tool = matches!(tool, Tool::Text);
        text_size_group.set_visible(is_text_tool);
        font_family_group.set_visible(is_text_tool);

        let is_obfuscate_tool = matches!(tool, Tool::Obfuscate);
        obfuscate_method_group.set_visible(is_obfuscate_tool);

        // Only allow vertical scrolling in Crop mode
        if matches!(tool, Tool::Crop) {
            canvas_scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        } else {
            canvas_scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Never);
        }

        let background_mode = matches!(tool, Tool::Background);
        background_sidebar.set_visible(background_mode);

        if let Some(window) = window.upgrade() {
            if background_mode {
                start_background_gradient_preview_loading();
                let (target_width, target_height) = recommended_window_size_with_extra_width(
                    image_width,
                    image_height,
                    BACKGROUND_SIDEBAR_WIDTH,
                );
                window.set_default_size(
                    window.allocated_width().max(target_width),
                    window.allocated_height().max(target_height),
                );
            } else {
                // Return window to standard recommended size when sidebar is hidden
                let (base_width, base_height) = recommended_window_size(image_width, image_height);
                window.set_default_size(base_width, base_height);
                window.queue_resize();
            }
        }
    })
}
