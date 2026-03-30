use crate::config::AppConfig;
use gtk4::{prelude::*, Align, Box as GtkBox, CheckButton, Grid, Label, Orientation, Separator};

#[allow(dead_code)]
pub struct AnnotateSettingsWidgets {
    pub section: GtkBox,
    pub inverse_arrow_check: CheckButton,
    pub smooth_drawing_check: CheckButton,
    pub draw_shadow_check: CheckButton,
    pub auto_expand_check: CheckButton,
    pub show_color_names_check: CheckButton,
    pub always_on_top_check: CheckButton,
    pub show_dock_icon_check: CheckButton,
}

pub fn build_annotate_section(config: &AppConfig) -> AnnotateSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.set_hexpand(true);
    section.set_vexpand(true);

    let grid = Grid::new();
    grid.set_hexpand(true);
    grid.set_row_spacing(30);
    grid.set_column_spacing(12);

    // Spacers for centering
    let left_spacer = GtkBox::new(Orientation::Horizontal, 0);
    left_spacer.set_hexpand(true);
    let right_spacer = GtkBox::new(Orientation::Horizontal, 0);
    right_spacer.set_hexpand(true);

    grid.attach(&left_spacer, 0, 0, 1, 1);
    grid.attach(&right_spacer, 4, 0, 1, 1);

    let mut row = 0;

    // --- GROUP 1 ---

    // Arrow tool
    let arrow_tool_label = Label::new(Some("Arrow tool:"));
    arrow_tool_label.add_css_class("settings-group-title");
    arrow_tool_label.set_xalign(1.0);
    arrow_tool_label.set_size_request(165, -1);
    let inverse_arrow_check = CheckButton::new();
    inverse_arrow_check.set_active(config.annotate_inverse_arrow);
    let arrow_cell = GtkBox::new(Orientation::Horizontal, 0);
    arrow_cell.set_size_request(28, -1);
    arrow_cell.set_halign(Align::Start);
    arrow_cell.append(&inverse_arrow_check);
    let arrow_option = Label::new(Some("Inverse arrow direction"));
    arrow_option.set_xalign(0.0);
    grid.attach(&arrow_tool_label, 1, row, 1, 1);
    grid.attach(&arrow_cell, 2, row, 1, 1);
    grid.attach(&arrow_option, 3, row, 1, 1);

    row += 1;
    // Pencil tool
    let pencil_tool_label = Label::new(Some("Pencil tool:"));
    pencil_tool_label.add_css_class("settings-group-title");
    pencil_tool_label.set_xalign(1.0);
    let smooth_drawing_check = CheckButton::new();
    smooth_drawing_check.set_active(config.annotate_smooth_drawing);
    let pencil_cell = GtkBox::new(Orientation::Horizontal, 0);
    pencil_cell.set_size_request(28, -1);
    pencil_cell.set_halign(Align::Start);
    pencil_cell.append(&smooth_drawing_check);
    let pencil_option = Label::new(Some("Smooth drawing"));
    pencil_option.set_xalign(0.0);
    grid.attach(&pencil_tool_label, 1, row, 1, 1);
    grid.attach(&pencil_cell, 2, row, 1, 1);
    grid.attach(&pencil_option, 3, row, 1, 1);

    row += 1;
    // Shadow
    let shadow_label = Label::new(Some("Shadow:"));
    shadow_label.add_css_class("settings-group-title");
    shadow_label.set_xalign(1.0);
    let draw_shadow_check = CheckButton::new();
    draw_shadow_check.set_active(config.annotate_draw_shadow);
    let shadow_cell = GtkBox::new(Orientation::Horizontal, 0);
    shadow_cell.set_size_request(28, -1);
    shadow_cell.set_halign(Align::Start);
    shadow_cell.append(&draw_shadow_check);
    let shadow_option = Label::new(Some("Draw shadow on objects"));
    shadow_option.set_xalign(0.0);
    grid.attach(&shadow_label, 1, row, 1, 1);
    grid.attach(&shadow_cell, 2, row, 1, 1);
    grid.attach(&shadow_option, 3, row, 1, 1);

    row += 1;
    // Canvas
    let canvas_label = Label::new(Some("Canvas:"));
    canvas_label.add_css_class("settings-group-title");
    canvas_label.set_xalign(1.0);
    let auto_expand_check = CheckButton::new();
    auto_expand_check.set_active(config.annotate_auto_expand);
    let canvas_cell = GtkBox::new(Orientation::Horizontal, 0);
    canvas_cell.set_size_request(28, -1);
    canvas_cell.set_halign(Align::Start);
    canvas_cell.append(&auto_expand_check);
    let canvas_option = Label::new(Some("Automatically expand"));
    canvas_option.set_xalign(0.0);
    grid.attach(&canvas_label, 1, row, 1, 1);
    grid.attach(&canvas_cell, 2, row, 1, 1);
    grid.attach(&canvas_option, 3, row, 1, 1);

    row += 1;
    // FULL WIDTH SEPARATOR
    let separator = Separator::new(Orientation::Horizontal);
    separator.set_margin_top(14);
    separator.set_margin_bottom(14);
    separator.set_hexpand(true);
    // Spans all columns (0 to 4)
    grid.attach(&separator, 0, row, 5, 1);

    row += 1;
    // --- GROUP 2 ---

    // Accessibility
    let access_label = Label::new(Some("Accessibility:"));
    access_label.add_css_class("settings-group-title");
    access_label.set_xalign(1.0);
    let show_color_names_check = CheckButton::new();
    show_color_names_check.set_active(config.annotate_show_color_names);
    let access_cell = GtkBox::new(Orientation::Horizontal, 0);
    access_cell.set_size_request(28, -1);
    access_cell.set_halign(Align::Start);
    access_cell.append(&show_color_names_check);
    let access_option = Label::new(Some("Show color names"));
    access_option.set_xalign(0.0);
    grid.attach(&access_label, 1, row, 1, 1);
    grid.attach(&access_cell, 2, row, 1, 1);
    grid.attach(&access_option, 3, row, 1, 1);

    row += 1;
    // Window: Always on top
    let window_label = Label::new(Some("Window:"));
    window_label.add_css_class("settings-group-title");
    window_label.set_xalign(1.0);
    let always_on_top_check = CheckButton::new();
    always_on_top_check.set_active(config.annotate_always_on_top);
    let always_on_top_cell = GtkBox::new(Orientation::Horizontal, 0);
    always_on_top_cell.set_size_request(28, -1);
    always_on_top_cell.set_halign(Align::Start);
    always_on_top_cell.append(&always_on_top_check);
    let always_on_top_option = Label::new(Some("Always on top"));
    always_on_top_option.set_xalign(0.0);
    grid.attach(&window_label, 1, row, 1, 1);
    grid.attach(&always_on_top_cell, 2, row, 1, 1);
    grid.attach(&always_on_top_option, 3, row, 1, 1);

    row += 1;
    // Window: Show Dock icon
    let show_dock_icon_check = CheckButton::new();
    show_dock_icon_check.set_active(config.annotate_show_dock_icon);
    let dock_cell = GtkBox::new(Orientation::Horizontal, 0);
    dock_cell.set_size_request(28, -1);
    dock_cell.set_halign(Align::Start);
    dock_cell.append(&show_dock_icon_check);
    let dock_option = Label::new(Some("Show Dock icon"));
    dock_option.set_xalign(0.0);
    grid.attach(&dock_cell, 2, row, 1, 1);
    grid.attach(&dock_option, 3, row, 1, 1);

    section.append(&grid);

    AnnotateSettingsWidgets {
        section,
        inverse_arrow_check,
        smooth_drawing_check,
        draw_shadow_check,
        auto_expand_check,
        show_color_names_check,
        always_on_top_check,
        show_dock_icon_check,
    }
}
