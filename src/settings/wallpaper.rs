use crate::config::AppConfig;
use gtk4::{prelude::*, Align, Box as GtkBox, CheckButton, Grid, Label, Orientation, Separator, Scale, CenterBox, Image, ColorButton, Button};

pub struct WallpaperSettingsWidgets {
    pub section: GtkBox,
    pub wallpaper_mode_desktop: CheckButton,
    pub wallpaper_dont_change_check: CheckButton,
    pub wallpaper_mode_custom: CheckButton,
    pub wallpaper_custom_path_btn: Button,
    pub wallpaper_mode_color: CheckButton,
    pub wallpaper_color_btn: ColorButton,
    pub window_screenshot_mode_full: CheckButton,
    pub window_screenshot_mode_trans: CheckButton,
    pub window_screenshot_padding_input: Scale,
    pub window_screenshot_shadow_check: CheckButton,
}

pub fn build_wallpaper_section(config: &AppConfig) -> WallpaperSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.set_hexpand(true);

    let grid = Grid::new();
    grid.set_column_spacing(24);
    grid.set_row_spacing(24);
    grid.set_hexpand(true);
    grid.set_margin_top(10);

    let l_spacer = GtkBox::new(Orientation::Horizontal, 0); l_spacer.set_hexpand(true);
    let r_spacer = GtkBox::new(Orientation::Horizontal, 0); r_spacer.set_hexpand(true);
    grid.attach(&l_spacer, 0, 0, 1, 1);
    grid.attach(&r_spacer, 4, 0, 1, 1);

    let mut row = 0;
    let label_group = gtk4::SizeGroup::new(gtk4::SizeGroupMode::Horizontal);

    // 0. Top Description
    let desc = Label::new(Some("Here, you can choose a wallpaper which will be set if you hide icons\nor take a screenshot/record a video."));
    desc.add_css_class("settings-sub-option");
    desc.set_xalign(0.0);
    desc.set_halign(Align::Start);
    desc.set_margin_bottom(20);
    grid.attach(&desc, 1, row, 3, 1);

    row += 1;
    // 1. Desktop Wallpaper
    let wallpaper_mode_desktop = CheckButton::new();
    wallpaper_mode_desktop.set_active(config.wallpaper_mode == "Desktop");
    wallpaper_mode_desktop.set_halign(Align::End);
    
    let desktop_label = Label::new(Some("Desktop wallpaper"));
    desktop_label.add_css_class("settings-group-title");
    desktop_label.set_xalign(0.0);
    desktop_label.set_halign(Align::Start);
    label_group.add_widget(&desktop_label);

    grid.attach(&wallpaper_mode_desktop, 1, row, 1, 1);
    grid.attach(&desktop_label, 2, row, 1, 1);

    row += 1;
    let wallpaper_dont_change_check = CheckButton::with_label("Don't change the wallpaper when switching spaces");
    wallpaper_dont_change_check.set_active(config.wallpaper_dont_change_on_space);
    wallpaper_dont_change_check.set_halign(Align::Start);
    grid.attach(&wallpaper_dont_change_check, 2, row, 2, 1);

    row += 1;
    // 2. Custom Wallpaper
    let wallpaper_mode_custom = CheckButton::new();
    wallpaper_mode_custom.set_group(Some(&wallpaper_mode_desktop));
    wallpaper_mode_custom.set_active(config.wallpaper_mode == "Custom");
    wallpaper_mode_custom.set_halign(Align::End);
    
    let custom_label = Label::new(Some("Custom wallpaper:"));
    custom_label.add_css_class("settings-group-title");
    custom_label.set_xalign(0.0);
    custom_label.set_halign(Align::Start);
    label_group.add_widget(&custom_label);

    let wallpaper_custom_path_btn = Button::new();
    wallpaper_custom_path_btn.add_css_class("settings-action-button");
    wallpaper_custom_path_btn.set_label(if config.wallpaper_custom_path.is_empty() { "Choose image..." } else { &config.wallpaper_custom_path });
    wallpaper_custom_path_btn.set_halign(Align::Start);

    grid.attach(&wallpaper_mode_custom, 1, row, 1, 1);
    grid.attach(&custom_label, 2, row, 1, 1);
    grid.attach(&wallpaper_custom_path_btn, 3, row, 1, 1);

    row += 1;
    // 3. Plain Color
    let wallpaper_mode_color = CheckButton::new();
    wallpaper_mode_color.set_group(Some(&wallpaper_mode_desktop));
    wallpaper_mode_color.set_active(config.wallpaper_mode == "Color");
    wallpaper_mode_color.set_halign(Align::End);

    let color_label = Label::new(Some("Plain color:"));
    color_label.add_css_class("settings-group-title");
    color_label.set_xalign(0.0);
    color_label.set_halign(Align::Start);
    label_group.add_widget(&color_label);

    let wallpaper_color_btn = ColorButton::new();
    if let Ok(rgba) = gtk4::gdk::RGBA::parse(&config.wallpaper_plain_color) {
        wallpaper_color_btn.set_rgba(&rgba);
    }
    wallpaper_color_btn.set_halign(Align::Start);

    grid.attach(&wallpaper_mode_color, 1, row, 1, 1);
    grid.attach(&color_label, 2, row, 1, 1);
    grid.attach(&wallpaper_color_btn, 3, row, 1, 1);

    section.append(&grid);

    let sep = Separator::new(Orientation::Horizontal);
    sep.set_margin_top(20);
    sep.set_margin_bottom(20);
    section.append(&sep);

    let grid2 = Grid::new();
    grid2.set_column_spacing(24);
    grid2.set_row_spacing(24);
    grid2.set_hexpand(true);
    
    let l_spacer2 = GtkBox::new(Orientation::Horizontal, 0); l_spacer2.set_hexpand(true);
    let r_spacer2 = GtkBox::new(Orientation::Horizontal, 0); r_spacer2.set_hexpand(true);
    grid2.attach(&l_spacer2, 0, 0, 1, 1);
    grid2.attach(&r_spacer2, 3, 0, 1, 1);

    let mut row2 = 0;
    let label_group2 = gtk4::SizeGroup::new(gtk4::SizeGroupMode::Horizontal);

    // --- Window Screenshot Section ---
    let win_label = Label::new(Some("Window screenshot:"));
    win_label.add_css_class("settings-group-title");
    win_label.set_xalign(1.0);
    win_label.set_halign(Align::End);
    label_group2.add_widget(&win_label);
    
    let mode_vbox = GtkBox::new(Orientation::Vertical, 8);
    let mode_hbox = GtkBox::new(Orientation::Horizontal, 20);
    
    let create_mode_btn = |label: &str, icon: &str, active: bool| -> (GtkBox, CheckButton) {
        let container = GtkBox::new(Orientation::Vertical, 6);
        let check = CheckButton::new();
        check.set_active(active);
        check.add_css_class("mode-icon-check");
        let img_box = GtkBox::new(Orientation::Vertical, 0);
        img_box.set_size_request(80, 60);
        img_box.add_css_class("mode-preview-box");
        if active { img_box.add_css_class("active"); }
        let icon_img = Image::from_icon_name(icon);
        icon_img.set_pixel_size(40);
        img_box.append(&icon_img);
        let lbl = Label::new(Some(label));
        lbl.add_css_class("settings-sub-option");
        container.append(&img_box);
        container.append(&lbl);
        (container, check)
    };

    let (full_btn, window_screenshot_mode_full) = create_mode_btn("With wallpaper", "video-display-symbolic", config.window_screenshot_mode == "Wallpaper");
    let (trans_btn, window_screenshot_mode_trans) = create_mode_btn("Transparent", "view-app-grid-symbolic", config.window_screenshot_mode == "Transparent");
    window_screenshot_mode_trans.set_group(Some(&window_screenshot_mode_full));

    let f_check = window_screenshot_mode_full.clone();
    let t_check = window_screenshot_mode_trans.clone();
    let f_box = full_btn.first_child().unwrap().downcast::<GtkBox>().unwrap();
    let t_box = trans_btn.first_child().unwrap().downcast::<GtkBox>().unwrap();
    let click_f = gtk4::GestureClick::new();
    let fc = f_check.clone(); let fb = f_box.clone(); let tb = t_box.clone();
    click_f.connect_pressed(move |_, _, _, _| {
        fc.set_active(true); fb.add_css_class("active"); tb.remove_css_class("active");
    });
    full_btn.add_controller(click_f);
    let click_t = gtk4::GestureClick::new();
    let tc = t_check.clone(); let fb2 = f_box.clone(); let tb2 = t_box.clone();
    click_t.connect_pressed(move |_, _, _, _| {
        tc.set_active(true); tb2.add_css_class("active"); fb2.remove_css_class("active");
    });
    trans_btn.add_controller(click_t);

    mode_hbox.append(&full_btn);
    mode_hbox.append(&trans_btn);

    let shift_hint = Label::new(Some("Hold ⇧ Shift while taking a screenshot to get a transparent background."));
    shift_hint.add_css_class("settings-sub-option-hint");
    shift_hint.set_xalign(0.0);
    mode_vbox.append(&shift_hint);
    mode_vbox.append(&mode_hbox);

    grid2.attach(&win_label, 1, row2, 1, 1);
    grid2.attach(&mode_vbox, 2, row2, 1, 1);

    row2 += 1;
    let padding_label = Label::new(Some("Padding:"));
    padding_label.add_css_class("settings-group-title");
    padding_label.set_xalign(1.0);
    padding_label.set_halign(Align::End);
    label_group2.add_widget(&padding_label);
    
    let window_screenshot_padding_input = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.05);
    window_screenshot_padding_input.set_value(config.window_screenshot_padding);
    window_screenshot_padding_input.set_hexpand(false);
    window_screenshot_padding_input.set_size_request(200, -1);
    let padding_vbox = GtkBox::new(Orientation::Vertical, 4);
    padding_vbox.set_halign(Align::Start);
    padding_vbox.append(&window_screenshot_padding_input);
    let padding_labels = CenterBox::new();
    padding_labels.set_start_widget(Some(&Label::new(Some("Min"))));
    padding_labels.set_center_widget(Some(&Label::new(Some("Default"))));
    padding_labels.set_end_widget(Some(&Label::new(Some("Max"))));
    padding_vbox.append(&padding_labels);

    grid2.attach(&padding_label, 1, row2, 1, 1);
    grid2.attach(&padding_vbox, 2, row2, 1, 1);

    row2 += 1;
    let shadow_label = Label::new(Some("Shadow:"));
    shadow_label.add_css_class("settings-group-title");
    shadow_label.set_xalign(1.0);
    shadow_label.set_halign(Align::End);
    label_group2.add_widget(&shadow_label);
    
    let shadow_vbox = GtkBox::new(Orientation::Vertical, 6);
    shadow_vbox.set_halign(Align::Start);
    let window_screenshot_shadow_check = CheckButton::with_label("Capture window shadow");
    window_screenshot_shadow_check.set_active(config.window_screenshot_shadow);
    let opt_hint = Label::new(Some("Hold ⌥ (alt/option) while taking a screenshot to disable shadow."));
    opt_hint.add_css_class("settings-sub-option-hint");
    opt_hint.set_xalign(0.0);
    shadow_vbox.append(&window_screenshot_shadow_check);
    shadow_vbox.append(&opt_hint);

    grid2.attach(&shadow_label, 1, row2, 1, 1);
    grid2.attach(&shadow_vbox, 2, row2, 1, 1);

    section.append(&grid2);

    WallpaperSettingsWidgets {
        section,
        wallpaper_mode_desktop,
        wallpaper_dont_change_check,
        wallpaper_mode_custom,
        wallpaper_custom_path_btn,
        wallpaper_mode_color,
        wallpaper_color_btn,
        window_screenshot_mode_full,
        window_screenshot_mode_trans,
        window_screenshot_padding_input,
        window_screenshot_shadow_check,
    }
}
