use crate::config::AppConfig;
use gtk4::{prelude::*, Align, Box as GtkBox, CheckButton, Label, Orientation};

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

    macro_rules! build_row {
        ($content:expr, $is_muted:expr) => {{
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
            row.add_css_class("settings-table-row");
            if $is_muted {
                row.add_css_class("settings-table-row-muted");
            }
            row.set_hexpand(true);
            row.append($content);
            row
        }};
    }

    let build_frame = || -> gtk4::Box {
        let frame = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        frame.add_css_class("settings-table-frame");
        frame.set_margin_bottom(24);
        frame.set_margin_start(4);
        frame.set_margin_end(4);
        frame
    };

    // --- Tools Group ---
    let tools_title = Label::new(Some("Drawing Tools"));
    tools_title.add_css_class("settings-group-title");
    tools_title.set_xalign(0.0);
    tools_title.set_halign(Align::Start);
    tools_title.set_margin_bottom(8);
    section.append(&tools_title);

    let tools_frame = build_frame();

    // Arrow tool
    let inverse_arrow_check = CheckButton::new();
    inverse_arrow_check.set_active(config.annotate_inverse_arrow);
    let arrow_hbox = GtkBox::new(Orientation::Horizontal, 12);
    arrow_hbox.set_hexpand(true);
    let arrow_option = Label::new(Some("Inverse arrow direction"));
    arrow_option.set_xalign(0.0);
    arrow_option.set_hexpand(true);
    arrow_hbox.append(&arrow_option);
    arrow_hbox.append(&inverse_arrow_check);
    tools_frame.append(&build_row!(&arrow_hbox, false));

    // Pencil tool
    let smooth_drawing_check = CheckButton::new();
    smooth_drawing_check.set_active(config.annotate_smooth_drawing);
    let pencil_hbox = GtkBox::new(Orientation::Horizontal, 12);
    pencil_hbox.set_hexpand(true);
    let pencil_option = Label::new(Some("Smooth drawing"));
    pencil_option.set_xalign(0.0);
    pencil_option.set_hexpand(true);
    pencil_hbox.append(&pencil_option);
    pencil_hbox.append(&smooth_drawing_check);
    tools_frame.append(&build_row!(&pencil_hbox, true));
    
    // Shadow
    let draw_shadow_check = CheckButton::new();
    draw_shadow_check.set_active(config.annotate_draw_shadow);
    let shadow_hbox = GtkBox::new(Orientation::Horizontal, 12);
    shadow_hbox.set_hexpand(true);
    let shadow_option = Label::new(Some("Draw shadow on objects"));
    shadow_option.set_xalign(0.0);
    shadow_option.set_hexpand(true);
    shadow_hbox.append(&shadow_option);
    shadow_hbox.append(&draw_shadow_check);
    tools_frame.append(&build_row!(&shadow_hbox, false));

    section.append(&tools_frame);


    // --- Canvas Group ---
    let canvas_title = Label::new(Some("Canvas & Interface"));
    canvas_title.add_css_class("settings-group-title");
    canvas_title.set_xalign(0.0);
    canvas_title.set_halign(Align::Start);
    canvas_title.set_margin_bottom(8);
    section.append(&canvas_title);

    let canvas_frame = build_frame();

    // Canvas
    let auto_expand_check = CheckButton::new();
    auto_expand_check.set_active(config.annotate_auto_expand);
    let canvas_hbox = GtkBox::new(Orientation::Horizontal, 12);
    canvas_hbox.set_hexpand(true);
    let canvas_option = Label::new(Some("Automatically expand canvas"));
    canvas_option.set_xalign(0.0);
    canvas_option.set_hexpand(true);
    canvas_hbox.append(&canvas_option);
    canvas_hbox.append(&auto_expand_check);
    canvas_frame.append(&build_row!(&canvas_hbox, false));

    // Accessibility
    let show_color_names_check = CheckButton::new();
    show_color_names_check.set_active(config.annotate_show_color_names);
    let access_hbox = GtkBox::new(Orientation::Horizontal, 12);
    access_hbox.set_hexpand(true);
    let access_option = Label::new(Some("Show color names (Accessibility)"));
    access_option.set_xalign(0.0);
    access_option.set_hexpand(true);
    access_hbox.append(&access_option);
    access_hbox.append(&show_color_names_check);
    canvas_frame.append(&build_row!(&access_hbox, true));

    section.append(&canvas_frame);


    // --- Window Group ---
    let window_title = Label::new(Some("Window"));
    window_title.add_css_class("settings-group-title");
    window_title.set_xalign(0.0);
    window_title.set_halign(Align::Start);
    window_title.set_margin_bottom(8);
    section.append(&window_title);

    let window_frame = build_frame();

    // Always on top
    let always_on_top_check = CheckButton::new();
    always_on_top_check.set_active(config.annotate_always_on_top);
    let on_top_hbox = GtkBox::new(Orientation::Horizontal, 12);
    on_top_hbox.set_hexpand(true);
    let always_on_top_option = Label::new(Some("Always on top"));
    always_on_top_option.set_xalign(0.0);
    always_on_top_option.set_hexpand(true);
    on_top_hbox.append(&always_on_top_option);
    on_top_hbox.append(&always_on_top_check);
    window_frame.append(&build_row!(&on_top_hbox, false));

    // Show Dock icon
    let show_dock_icon_check = CheckButton::new();
    show_dock_icon_check.set_active(config.annotate_show_dock_icon);
    let dock_hbox = GtkBox::new(Orientation::Horizontal, 12);
    dock_hbox.set_hexpand(true);
    let dock_option = Label::new(Some("Show Dock icon"));
    dock_option.set_xalign(0.0);
    dock_option.set_hexpand(true);
    dock_hbox.append(&dock_option);
    dock_hbox.append(&show_dock_icon_check);
    window_frame.append(&build_row!(&dock_hbox, true));

    section.append(&window_frame);

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
