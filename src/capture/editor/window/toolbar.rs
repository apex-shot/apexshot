use gtk4::{
    prelude::*, Box as GtkBox, Button, CenterBox, Entry, Image, Label, Orientation, Popover, Scale,
    Stack,
};
use std::rc::Rc;

use super::super::{
    pen_weight::PenWeight,
    types::{ArrowStyle, ObfuscateMethod, Tool},
    ui_support::{
        arrow_style_toolbar_icon, icon_tool_button, tool_icon_widget, toolbar_icon_size,
        traffic_light_button,
    },
};
use super::icon_names;

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
}

pub(super) struct ToolbarModeParts {
    pub root: GtkBox,
    pub toolbar_mode_stack: Stack,
    pub color_status: GtkBox,
    pub color_status_swatch: GtkBox,
    pub color_status_label: Label,
    pub size_group: GtkBox,
    pub size_slider: gtk4::Scale,
    pub text_size_group: GtkBox,
    pub text_size_label: Label,
    pub text_size_list: GtkBox,
    pub font_family_group: GtkBox,
    pub font_family_label: Label,
    pub font_family_list: GtkBox,
    pub obfuscate_method_group: GtkBox,
    #[allow(dead_code)]
    pub obfuscate_method_button: Button,
    #[allow(dead_code)]
    pub obfuscate_method_popover: Popover,
    pub obfuscate_method_list: GtkBox,
    /// Pen weight selector for highlighter
    pub pen_weight_button: gtk4::Button,
    #[allow(dead_code)]
    pub pen_weight_popover: gtk4::Popover,
    pub pen_weight_list: gtk4::Box,
    /// Group containing pen weight button (for visibility toggle)
    pub pen_weight_group: GtkBox,
    #[allow(dead_code)]
    pub number_options_popover: gtk4::Popover,
    pub number_options_list: gtk4::Box,
    /// Entry for starting number
    pub number_start_entry: gtk4::Entry,
    /// Increment button for starting number
    pub number_inc_btn: gtk4::Button,
    /// Decrement button for starting number
    pub number_dec_btn: gtk4::Button,
    /// Size submenu button for number tool
    pub number_size_button: gtk4::Button,
    #[allow(dead_code)]
    pub number_size_popover: gtk4::Popover,
    pub number_size_list: gtk4::Box,
    /// Group containing number options button (for visibility toggle)
    pub number_options_group: GtkBox,
    /// Arrow style selector group
    pub arrow_style_group: GtkBox,
    #[allow(dead_code)]
    pub arrow_style_button: Button,
    #[allow(dead_code)]
    pub arrow_style_popover: Popover,
    pub arrow_style_list: GtkBox,
    /// Stroke size selector for arrow/line tools
    pub stroke_size_group: GtkBox,
    pub stroke_size_button: gtk4::Button,
    #[allow(dead_code)]
    pub stroke_size_popover: gtk4::Popover,
    pub stroke_size_list: gtk4::Box,
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

    let select_btn = icon_tool_button(icon_names::POINTER_PRIMARY_CLICK, "Select");
    let crop_btn = icon_tool_button(icon_names.crop, "Crop");
    let background_btn = icon_tool_button(icon_names::IMAGE_REGULAR, "Background");
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

pub(super) fn build_obfuscate_method_controls() -> (GtkBox, Button, Popover, GtkBox) {
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

    let obfuscate_method_icon = Image::from_icon_name(icon_names::VIEW_GRID);
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

fn build_pen_weight_dropdown() -> (GtkBox, Button, Popover, GtkBox) {
    let pen_weight_group = GtkBox::new(Orientation::Horizontal, 4);
    pen_weight_group.add_css_class("editor-pen-weight-group");
    pen_weight_group.add_css_class("editor-tools-group");
    pen_weight_group.set_visible(false);

    let pen_weight_button = Button::new();
    pen_weight_button.set_has_frame(false);
    pen_weight_button.set_focusable(false);
    pen_weight_button.add_css_class("editor-tool-button");
    pen_weight_button.add_css_class("flat");
    pen_weight_button.set_tooltip_text(Some("Stroke Thickness"));

    let pen_weight_icon = Image::from_icon_name(PenWeight::Medium.icon_name());
    pen_weight_icon.set_pixel_size(PenWeight::Medium.icon_pixel_size());
    pen_weight_button.set_child(Some(&pen_weight_icon));

    let pen_weight_popover = Popover::new();
    pen_weight_popover.set_has_arrow(false);
    pen_weight_popover.set_autohide(true);
    pen_weight_popover.add_css_class("editor-popover");
    pen_weight_popover.set_parent(&pen_weight_button);

    let pen_weight_list = GtkBox::new(Orientation::Vertical, 0);
    pen_weight_list.add_css_class("editor-popover-list");
    pen_weight_popover.set_child(Some(&pen_weight_list));

    let p_popover = pen_weight_popover.clone();
    pen_weight_button.connect_clicked(move |_| {
        p_popover.popup();
    });

    pen_weight_group.append(&pen_weight_button);

    (
        pen_weight_group,
        pen_weight_button,
        pen_weight_popover,
        pen_weight_list,
    )
}

fn build_stroke_size_dropdown() -> (GtkBox, Button, Popover, GtkBox) {
    let stroke_size_group = GtkBox::new(Orientation::Horizontal, 4);
    stroke_size_group.add_css_class("editor-stroke-size-group");
    stroke_size_group.add_css_class("editor-tools-group");
    stroke_size_group.set_visible(false);

    let stroke_size_button = Button::new();
    stroke_size_button.set_has_frame(false);
    stroke_size_button.set_focusable(false);
    stroke_size_button.add_css_class("editor-tool-button");
    stroke_size_button.add_css_class("flat");
    stroke_size_button.set_tooltip_text(Some("Stroke Thickness"));

    let stroke_size_icon = Image::from_icon_name(PenWeight::Medium.icon_name());
    stroke_size_icon.set_pixel_size(PenWeight::Medium.icon_pixel_size());
    stroke_size_button.set_child(Some(&stroke_size_icon));

    let stroke_size_popover = Popover::new();
    stroke_size_popover.set_has_arrow(false);
    stroke_size_popover.set_autohide(true);
    stroke_size_popover.add_css_class("editor-popover");
    stroke_size_popover.set_parent(&stroke_size_button);

    let stroke_size_list = GtkBox::new(Orientation::Vertical, 0);
    stroke_size_list.add_css_class("editor-popover-list");
    stroke_size_popover.set_child(Some(&stroke_size_list));

    let p_popover = stroke_size_popover.clone();
    stroke_size_button.connect_clicked(move |_| {
        p_popover.popup();
    });

    stroke_size_group.append(&stroke_size_button);

    (
        stroke_size_group,
        stroke_size_button,
        stroke_size_popover,
        stroke_size_list,
    )
}

fn build_arrow_style_controls() -> (GtkBox, Button, Popover, GtkBox) {
    let arrow_style_group = GtkBox::new(Orientation::Horizontal, 4);
    arrow_style_group.add_css_class("editor-arrow-style-group");
    arrow_style_group.add_css_class("editor-tools-group");
    arrow_style_group.set_visible(false);

    let arrow_style_button = Button::new();
    arrow_style_button.set_has_frame(false);
    arrow_style_button.set_focusable(false);
    arrow_style_button.add_css_class("editor-tool-button");
    arrow_style_button.add_css_class("flat");
    arrow_style_button.set_tooltip_text(Some("Arrow style"));

    let standard_arrow_icon = arrow_style_toolbar_icon(ArrowStyle::Standard);
    let arrow_style_icon = tool_icon_widget(
        standard_arrow_icon.clone(),
        toolbar_icon_size(&standard_arrow_icon),
    );
    arrow_style_button.set_child(Some(&arrow_style_icon));

    let arrow_style_popover = Popover::new();
    arrow_style_popover.set_has_arrow(false);
    arrow_style_popover.set_autohide(true);
    arrow_style_popover.add_css_class("editor-popover");
    arrow_style_popover.set_parent(&arrow_style_button);

    let arrow_style_list = GtkBox::new(Orientation::Vertical, 0);
    arrow_style_list.add_css_class("editor-popover-list");
    arrow_style_popover.set_child(Some(&arrow_style_list));

    let p_popover = arrow_style_popover.clone();
    arrow_style_button.connect_clicked(move |_| {
        p_popover.popup();
    });

    arrow_style_group.append(&arrow_style_button);

    (
        arrow_style_group,
        arrow_style_button,
        arrow_style_popover,
        arrow_style_list,
    )
}

fn build_number_options_dropdown() -> (
    GtkBox,
    Button,
    Popover,
    GtkBox,
    Entry,
    Button,
    Button,
    Button,
    Popover,
    GtkBox,
) {
    use super::super::numbering_style::{NumberSize, NumberingStyle};

    let number_options_group = GtkBox::new(Orientation::Horizontal, 4);
    number_options_group.add_css_class("editor-number-options-group");
    number_options_group.add_css_class("editor-tools-group");
    number_options_group.set_visible(false);

    // Main dropdown button with "123" label
    let number_options_button = Button::new();
    number_options_button.set_has_frame(false);
    number_options_button.set_focusable(false);
    number_options_button.add_css_class("editor-tool-button");
    number_options_button.add_css_class("flat");
    number_options_button.set_tooltip_text(Some("Number Options"));

    let btn_box = GtkBox::new(Orientation::Horizontal, 2);
    let btn_label = Label::new(Some("123"));
    btn_label.add_css_class("editor-number-options-label");
    let btn_arrow = Image::from_icon_name(icon_names::CHEVRON_DOWN_REGULAR);
    btn_arrow.set_pixel_size(10);
    btn_box.append(&btn_label);
    btn_box.append(&btn_arrow);
    number_options_button.set_child(Some(&btn_box));

    // Main popover
    let number_options_popover = Popover::new();
    number_options_popover.set_has_arrow(false);
    number_options_popover.set_autohide(true);
    number_options_popover.add_css_class("editor-popover");
    number_options_popover.add_css_class("editor-number-options-popover");
    number_options_popover.set_parent(&number_options_button);

    // Main list container
    let number_options_list = GtkBox::new(Orientation::Vertical, 4);
    number_options_list.add_css_class("editor-popover-list");
    number_options_list.set_margin_start(4);
    number_options_list.set_margin_end(4);
    number_options_list.set_margin_top(4);
    number_options_list.set_margin_bottom(4);
    number_options_popover.set_child(Some(&number_options_list));

    // Numbering style options
    for style in NumberingStyle::ALL {
        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        let check_icon = Image::from_icon_name(icon_names::SELECT);
        check_icon.set_pixel_size(12);
        check_icon.set_visible(style == NumberingStyle::default());
        check_icon.add_css_class("editor-number-style-check");

        let label = Label::new(Some(style.label()));
        label.set_hexpand(true);
        label.set_halign(gtk4::Align::Start);

        btn_box.append(&check_icon);
        btn_box.append(&label);

        let btn = Button::builder()
            .has_frame(false)
            .css_classes([
                "editor-popover-list-item",
                "flat",
                "editor-number-style-option",
            ])
            .child(&btn_box)
            .build();
        number_options_list.append(&btn);
    }

    // Separator
    let sep = GtkBox::new(Orientation::Horizontal, 0);
    sep.add_css_class("editor-popover-separator");
    sep.set_margin_top(4);
    sep.set_margin_bottom(4);
    number_options_list.append(&sep);

    // Starting number control
    let start_box = GtkBox::new(Orientation::Horizontal, 8);
    start_box.set_margin_start(4);
    start_box.set_margin_end(4);
    start_box.add_css_class("editor-number-start-row");

    let start_label = Label::new(Some("Start with:"));
    start_label.add_css_class("editor-number-start-label");

    let number_start_entry = Entry::new();
    number_start_entry.set_width_chars(5);
    number_start_entry.set_max_width_chars(5);
    number_start_entry.set_text("1");
    number_start_entry.set_editable(false);
    number_start_entry.add_css_class("editor-number-start-entry");

    let inc_btn = Button::with_label("+");
    let dec_btn = Button::with_label("-");

    start_box.append(&start_label);
    start_box.append(&dec_btn);
    start_box.append(&number_start_entry);
    start_box.append(&inc_btn);
    number_options_list.append(&start_box);

    // Separator
    let sep2 = GtkBox::new(Orientation::Horizontal, 0);
    sep2.add_css_class("editor-popover-separator");
    sep2.set_margin_top(4);
    sep2.set_margin_bottom(4);
    number_options_list.append(&sep2);

    // Size submenu button
    let number_size_button = Button::new();
    number_size_button.set_has_frame(false);
    number_size_button.set_focusable(false);
    number_size_button.add_css_class("editor-popover-list-item");
    number_size_button.add_css_class("flat");

    let size_btn_box = GtkBox::new(Orientation::Horizontal, 8);
    size_btn_box.set_margin_start(4);
    size_btn_box.set_margin_end(4);
    let size_label = Label::new(Some("Size"));
    size_label.set_hexpand(true);
    size_label.set_halign(gtk4::Align::Start);
    let size_arrow = Image::from_icon_name(icon_names::CHEVRON_RIGHT_REGULAR);
    size_arrow.set_pixel_size(10);
    size_arrow.set_halign(gtk4::Align::End);
    size_btn_box.append(&size_label);
    size_btn_box.append(&size_arrow);
    number_size_button.set_child(Some(&size_btn_box));

    // Size popover (submenu)
    let number_size_popover = Popover::new();
    number_size_popover.set_has_arrow(false);
    number_size_popover.set_autohide(true);
    number_size_popover.add_css_class("editor-popover");
    number_size_popover.set_position(gtk4::PositionType::Right);
    number_size_popover.set_parent(&number_size_button);

    let number_size_list = GtkBox::new(Orientation::Vertical, 0);
    number_size_list.add_css_class("editor-popover-list");
    number_size_popover.set_child(Some(&number_size_list));

    for size in NumberSize::ALL {
        let btn = Button::builder()
            .label(size.label())
            .has_frame(false)
            .css_classes([
                "editor-popover-list-item",
                "flat",
                "editor-number-size-option",
            ])
            .build();
        number_size_list.append(&btn);
    }

    // Show size popover on click
    let size_popover = number_size_popover.clone();
    number_size_button.connect_clicked(move |_| {
        size_popover.popup();
    });

    number_options_list.append(&number_size_button);

    // Show main popover on click
    let main_popover = number_options_popover.clone();
    number_options_button.connect_clicked(move |_| {
        main_popover.popup();
    });

    number_options_group.append(&number_options_button);

    (
        number_options_group,
        number_options_button,
        number_options_popover,
        number_options_list,
        number_start_entry,
        inc_btn,
        dec_btn,
        number_size_button,
        number_size_popover,
        number_size_list,
    )
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
) -> ToolbarModeParts {
    let color_group = GtkBox::new(Orientation::Horizontal, 0);
    color_group.add_css_class("editor-color-group");

    let color_status = GtkBox::new(Orientation::Horizontal, 8);
    color_status.add_css_class("editor-toolbar-color-status");
    color_status.set_valign(gtk4::Align::Center);

    let color_status_swatch = GtkBox::new(Orientation::Horizontal, 0);
    color_status_swatch.add_css_class("editor-toolbar-color-status-swatch");
    color_status_swatch.set_widget_name("editor-toolbar-color-status-swatch");
    color_status_swatch.set_size_request(30, 30);
    color_status_swatch.set_halign(gtk4::Align::Center);
    color_status_swatch.set_valign(gtk4::Align::Center);
    color_status_swatch.set_hexpand(false);
    color_status_swatch.set_vexpand(false);

    let color_status_label = Label::new(Some("#121212"));
    color_status_label.add_css_class("editor-toolbar-color-status-label");
    color_status_label.set_xalign(0.0);
    color_status_label.set_valign(gtk4::Align::Center);

    color_status.append(&color_status_swatch);
    color_status.append(&color_status_label);
    color_group.append(&color_status);

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
    let text_size_arrow = Image::from_icon_name(icon_names::CHEVRON_DOWN_REGULAR);
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
    let font_family_arrow = Image::from_icon_name(icon_names::CHEVRON_DOWN_REGULAR);
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

    // Build obfuscate method selector
    let (
        obfuscate_method_group,
        obfuscate_method_button,
        obfuscate_method_popover,
        obfuscate_method_list,
    ) = build_obfuscate_method_controls();

    // Populate obfuscate method list with options
    let methods = [
        (ObfuscateMethod::Pixelate, icon_names::VIEW_GRID, "Pixelate"),
        (ObfuscateMethod::Blur, icon_names::BLUR, "Blur"),
        (
            ObfuscateMethod::Blackout,
            icon_names::MEDIA_PLAYBACK_STOP,
            "Blackout",
        ),
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
    let (pen_weight_group, pen_weight_button, pen_weight_popover, pen_weight_list) =
        build_pen_weight_dropdown();

    // Populate pen weight list with actual icon widgets instead of custom-drawn visuals.
    for weight in PenWeight::ALL {
        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        btn_box.set_margin_start(8);
        btn_box.set_margin_end(8);
        btn_box.set_margin_top(4);
        btn_box.set_margin_bottom(4);

        let icon = Image::from_icon_name(weight.icon_name());
        icon.set_pixel_size(weight.icon_pixel_size());

        let label_widget = Label::new(Some(weight.label()));

        btn_box.append(&icon);
        btn_box.append(&label_widget);

        let btn = Button::builder()
            .has_frame(false)
            .css_classes(["editor-popover-list-item", "flat"])
            .child(&btn_box)
            .build();

        pen_weight_list.append(&btn);
    }

    // Build arrow style selector
    let (arrow_style_group, arrow_style_button, arrow_style_popover, arrow_style_list) =
        build_arrow_style_controls();

    for style in ArrowStyle::ALL {
        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        btn_box.set_margin_start(8);
        btn_box.set_margin_end(8);
        btn_box.set_margin_top(4);
        btn_box.set_margin_bottom(4);

        let style_icon = arrow_style_toolbar_icon(style);
        let icon = tool_icon_widget(style_icon.clone(), toolbar_icon_size(&style_icon));
        let label_widget = Label::new(Some(style.display_name()));

        btn_box.append(&icon);
        btn_box.append(&label_widget);

        let btn = Button::builder()
            .has_frame(false)
            .css_classes(["editor-popover-list-item", "flat"])
            .child(&btn_box)
            .build();

        arrow_style_list.append(&btn);
    }

    // Build stroke size selector for arrow/line tools
    let (stroke_size_group, stroke_size_button, stroke_size_popover, stroke_size_list) =
        build_stroke_size_dropdown();

    // Sizes: (label, stroke_size_value, PenWeight for icon)
    let stroke_sizes = [
        ("Thin", 2.0_f64, PenWeight::Small),
        ("Medium", 4.0_f64, PenWeight::Medium),
        ("Thick", 7.0_f64, PenWeight::Large),
        ("Very Thick", 12.0_f64, PenWeight::ExtraLarge),
    ];
    for (label, _size, weight) in stroke_sizes {
        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        btn_box.set_margin_start(8);
        btn_box.set_margin_end(8);
        btn_box.set_margin_top(4);
        btn_box.set_margin_bottom(4);

        let icon = Image::from_icon_name(weight.icon_name());
        icon.set_pixel_size(weight.icon_pixel_size());
        let label_widget = Label::new(Some(label));

        btn_box.append(&icon);
        btn_box.append(&label_widget);

        let btn = Button::builder()
            .has_frame(false)
            .css_classes(["editor-popover-list-item", "flat"])
            .child(&btn_box)
            .build();

        stroke_size_list.append(&btn);
    }

    // Build number options selector for number tool
    let (
        number_options_group,
        _number_options_button,
        number_options_popover,
        number_options_list,
        number_start_entry,
        number_inc_btn,
        number_dec_btn,
        number_size_button,
        number_size_popover,
        number_size_list,
    ) = build_number_options_dropdown();

    let primary_tools_group = GtkBox::new(Orientation::Horizontal, 2);
    primary_tools_group.add_css_class("editor-tools-group");
    primary_tools_group.add_css_class("editor-primary-tools-group");
    primary_tools_group.append(select_btn);
    primary_tools_group.append(crop_btn);
    primary_tools_group.append(background_btn);
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
    toolbar_mode_stack.set_visible_child_name("standard");

    let root = GtkBox::new(Orientation::Horizontal, 10);
    root.add_css_class("editor-toolbar-center");
    root.append(&toolbar_mode_stack);
    ToolbarModeParts {
        root,
        toolbar_mode_stack,
        color_status,
        color_status_swatch,
        color_status_label,
        size_group,
        size_slider,
        text_size_group,
        text_size_label,
        text_size_list,
        font_family_group,
        font_family_label,
        font_family_list,
        obfuscate_method_group,
        obfuscate_method_button,
        obfuscate_method_popover,
        obfuscate_method_list,
        pen_weight_button,
        pen_weight_popover,
        pen_weight_list,
        pen_weight_group,
        number_options_popover,
        number_options_list,
        number_start_entry,
        number_inc_btn,
        number_dec_btn,
        number_size_button,
        number_size_popover,
        number_size_list,
        number_options_group,
        arrow_style_group,
        arrow_style_button,
        arrow_style_popover,
        arrow_style_list,
        stroke_size_group,
        stroke_size_button,
        stroke_size_popover,
        stroke_size_list,
    }
}

pub(super) fn build_toolbar_right_controls(
    color_status: &GtkBox,
    undo_icon_name: &str,
    redo_icon_name: &str,
    delete_icon_name: &str,
) -> ToolbarRightParts {
    let undo_btn = icon_tool_button(undo_icon_name, "Undo");
    let redo_btn = icon_tool_button(redo_icon_name, "Redo");
    let delete_selected_btn = icon_tool_button(delete_icon_name, "Delete selected");
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
    right_tools.append(color_status);

    let save_btn = Button::with_label("Done");
    save_btn.set_has_frame(false);
    save_btn.add_css_class("editor-done-button");
    save_btn.add_css_class("body");
    save_btn.set_valign(gtk4::Align::Center);

    let root = GtkBox::new(Orientation::Horizontal, 16);
    root.add_css_class("editor-toolbar-right");
    root.append(&right_tools);
    root.append(&save_btn);

    ToolbarRightParts {
        root,
        undo_btn,
        redo_btn,
        delete_selected_btn,
        save_btn,
    }
}

pub(super) fn build_toolbar_tool_updater(
    toolbar_mode_stack: &Stack,
    inspector_stack: &Stack,
    inspector_tabs: &GtkBox,
    background_tab_btn: &Button,
    colors_tab_btn: &Button,
    text_size_group: &GtkBox,
    font_family_group: &GtkBox,
    obfuscate_method_group: &GtkBox,
    pen_weight_group: &GtkBox,
    number_options_group: &GtkBox,
    arrow_style_group: &GtkBox,
    stroke_size_group: &GtkBox,
    canvas_scroller: &gtk4::ScrolledWindow,
    start_background_gradient_preview_loading: Rc<dyn Fn()>,
) -> Rc<dyn Fn(Tool)> {
    let toolbar_mode_stack = toolbar_mode_stack.clone();
    let inspector_stack = inspector_stack.clone();
    let inspector_tabs = inspector_tabs.clone();
    let background_tab_btn = background_tab_btn.clone();
    let colors_tab_btn = colors_tab_btn.clone();
    let text_size_group = text_size_group.clone();
    let font_family_group = font_family_group.clone();
    let obfuscate_method_group = obfuscate_method_group.clone();
    let pen_weight_group = pen_weight_group.clone();
    let number_options_group = number_options_group.clone();
    let arrow_style_group = arrow_style_group.clone();
    let stroke_size_group = stroke_size_group.clone();
    let canvas_scroller = canvas_scroller.clone();

    Rc::new(move |tool| {
        toolbar_mode_stack.set_visible_child_name("standard");

        let is_text_tool = matches!(tool, Tool::Text);
        text_size_group.set_visible(is_text_tool);
        font_family_group.set_visible(is_text_tool);

        obfuscate_method_group.set_visible(false);

        // Pen weight is handled in the inspector panel for highlighter
        pen_weight_group.set_visible(false);

        let is_number_tool = matches!(tool, Tool::Number);
        number_options_group.set_visible(is_number_tool);

        let is_arrow_tool = matches!(tool, Tool::Arrow);
        arrow_style_group.set_visible(is_arrow_tool);
        stroke_size_group.set_visible(false);

        canvas_scroller.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);

        let primary_surface = match tool {
            Tool::Background => Some(("Background", "background")),
            Tool::Crop => Some(("Crop", "crop")),
            Tool::Pen => Some(("Pen", "pen")),
            Tool::Arrow => Some(("Arrow", "arrow")),
            Tool::Line => Some(("Line", "line")),
            Tool::Text => Some(("Text", "text")),
            Tool::Highlighter => Some(("Highlighter", "highlighter")),
            Tool::Obfuscate => Some(("Obfuscate", "obfuscate")),
            Tool::Number => Some(("Number", "number")),
            _ => None,
        };
        let background_mode = matches!(tool, Tool::Background);
        let colors_mode = matches!(
            tool,
            Tool::Crop
                | Tool::Background
                | Tool::Pen
                | Tool::Arrow
                | Tool::Line
                | Tool::Box
                | Tool::Circle
                | Tool::Text
                | Tool::Number
                | Tool::Highlighter
                | Tool::Focus
        );
        background_tab_btn.set_label(
            primary_surface
                .map(|(label, _)| label)
                .unwrap_or("Background"),
        );
        colors_tab_btn.set_label("Colors");
        inspector_tabs.set_visible(primary_surface.is_some() || colors_mode);
        background_tab_btn.set_visible(primary_surface.is_some());
        colors_tab_btn.set_visible(colors_mode);

        if let Some((_, surface)) = primary_surface {
            inspector_stack.set_visible_child_name(surface);
            background_tab_btn.add_css_class("active-inspector-tab");
            colors_tab_btn.remove_css_class("active-inspector-tab");
        } else if colors_mode {
            inspector_stack.set_visible_child_name("colors");
            colors_tab_btn.add_css_class("active-inspector-tab");
            background_tab_btn.remove_css_class("active-inspector-tab");
        } else {
            inspector_stack.set_visible_child_name("placeholder");
            background_tab_btn.remove_css_class("active-inspector-tab");
            colors_tab_btn.remove_css_class("active-inspector-tab");
        }

        if background_mode {
            start_background_gradient_preview_loading();
        }
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn toolbar_uses_read_only_color_status_chip_instead_of_picker_trigger() {
        let source = include_str!("toolbar.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("editor-toolbar-color-status")
                && production_source.contains("editor-toolbar-color-status-swatch")
                && production_source.contains("editor-toolbar-color-status-label")
                && !production_source.contains("color_picker_trigger_host"),
            "Toolbar should use a read-only color status chip instead of the picker trigger host",
        );
    }

    #[test]
    fn toolbar_color_status_swatch_is_fixed_to_square_allocation() {
        let source = include_str!("toolbar.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("color_status_swatch.set_size_request(30, 30);")
                && production_source.contains("color_status_swatch.set_halign(gtk4::Align::Center);")
                && production_source.contains("color_status_swatch.set_valign(gtk4::Align::Center);")
                && production_source.contains("color_status_swatch.set_hexpand(false);")
                && production_source.contains("color_status_swatch.set_vexpand(false);"),
            "Toolbar color swatch should keep a fixed boxed allocation so the shape is not stretched",
        );
    }

    #[test]
    fn background_tool_defaults_to_background_inspector_surface() {
        let source = include_str!("toolbar.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("let primary_surface = match tool {")
                && production_source.contains("Tool::Background => Some((\"Background\", \"background\"))")
                && production_source.contains("if let Some((_, surface)) = primary_surface {")
                && production_source.contains("inspector_stack.set_visible_child_name(surface);")
                && production_source.contains("background_tab_btn.add_css_class(\"active-inspector-tab\");")
                && production_source.contains("colors_tab_btn.remove_css_class(\"active-inspector-tab\");"),
            "Background mode should open on the Background inspector surface instead of the Colors surface",
        );
    }

    #[test]
    fn toolbar_color_status_label_is_center_aligned_with_swatch() {
        let source = include_str!("toolbar.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("color_status.set_valign(gtk4::Align::Center);")
                && production_source
                    .contains("color_status_label.set_valign(gtk4::Align::Center);"),
            "Toolbar color status label should align vertically with the swatch",
        );
    }

    #[test]
    fn toolbar_places_color_status_before_done_button_in_right_controls() {
        let source = include_str!("toolbar.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains(
                "pub(super) fn build_toolbar_right_controls(\n    color_status: &GtkBox,"
            ) && production_source.contains("right_tools.append(color_status);")
                && production_source.contains("root.append(&save_btn);"),
            "Toolbar should place the color status in the right controls before the Done button",
        );
    }

    #[test]
    fn line_and_shape_tools_without_primary_tabs_still_default_to_colors_inspector_surface() {
        let source = include_str!("toolbar.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("inspector_stack.set_visible_child_name(\"colors\");")
                && production_source.contains("} else if colors_mode {")
                && production_source.contains("Tool::Box")
                && production_source.contains("Tool::Circle")
                && !production_source.contains("Tool::Arrow => Some((\"Arrow\", \"colors\"))")
                && !production_source.contains("Tool::Text => Some((\"Text\", \"colors\"))")
                && !production_source.contains("Tool::Number => Some((\"Number\", \"colors\"))")
                && !production_source.contains("Tool::Pen => Some((\"Pen\", \"colors\"))")
                && !production_source.contains("Tool::Line => Some((\"Line\", \"colors\"))")
                && !production_source.contains("Tool::Highlighter => Some((\"Highlighter\", \"colors\"))")
                && !production_source.contains("| Tool::Obfuscate"),
            "Color-capable tools without dedicated primary tabs should still switch the right inspector to the Colors surface",
        );
    }

    #[test]
    fn obfuscate_routes_to_a_dedicated_primary_tab_instead_of_shared_colors() {
        let source = include_str!("toolbar.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("Tool::Obfuscate => Some((\"Obfuscate\", \"obfuscate\"))")
                && !production_source.contains("| Tool::Obfuscate"),
            "Obfuscate should stop using the shared Colors inspector flow and route to its own primary tab",
        );
    }

    #[test]
    fn pen_line_and_highlighter_route_to_dedicated_primary_tabs_before_colors() {
        let source = include_str!("toolbar.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("Tool::Pen => Some((\"Pen\", \"pen\"))")
                && production_source.contains("Tool::Line => Some((\"Line\", \"line\"))")
                && production_source.contains("Tool::Highlighter => Some((\"Highlighter\", \"highlighter\"))")
                && production_source.contains("\"Colors\""),
            "Pen, Line, and Highlighter should route to dedicated primary inspector tabs alongside the shared Colors tab",
        );
    }

    #[test]
    fn toolbar_no_longer_exposes_arrow_text_and_number_detail_groups() {
        let source = include_str!("toolbar.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production_source.contains("standard_mode_group.append(&arrow_style_group);")
                && !production_source.contains("standard_mode_group.append(&stroke_size_group);")
                && !production_source.contains("standard_mode_group.append(&number_options_group);")
                && !production_source.contains("root.append(&text_size_group);")
                && !production_source.contains("root.append(&font_family_group);")
                && !production_source.contains("obfuscate_method_group.set_visible(is_obfuscate_tool);"),
            "Toolbar layout should stop mounting Arrow thickness, Text, Number, and Obfuscate detail groups after the inspector migration",
        );
    }

    #[test]
    fn toolbar_keeps_crop_tool_button_but_not_crop_mode_stack_controls() {
        let source = include_str!("toolbar.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("primary_tools_group.append(crop_btn);")
                && !production_source.contains("toolbar_mode_stack.add_named(&crop_mode_group, Some(\"crop\"));")
                && production_source.contains("Tool::Crop => Some((\"Crop\", \"crop\"))"),
            "Toolbar should keep the Crop tool button in primary_tools_group while routing Crop through the inspector instead of a toolbar mode stack",
        );
    }
}
