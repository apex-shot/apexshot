use crate::config::AppConfig;
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, CenterBox, CheckButton, ColorButton, Image,
    Label, Orientation, Scale,
};

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

    // 0. Top Description
    let desc = Label::new(Some("Here, you can choose a wallpaper which will be set if you hide icons or take a screenshot/record a video."));
    desc.add_css_class("settings-description");
    desc.set_xalign(0.0);
    desc.set_halign(Align::Start);
    desc.set_margin_bottom(16);
    desc.set_wrap(true);
    section.append(&desc);

    // --- Wallpaper Group ---
    let wallpaper_title = Label::new(Some("Wallpaper"));
    wallpaper_title.add_css_class("settings-group-title");
    wallpaper_title.set_xalign(0.0);
    wallpaper_title.set_halign(Align::Start);
    wallpaper_title.set_margin_bottom(8);
    section.append(&wallpaper_title);

    let wallpaper_frame = build_frame();

    // Desktop
    let wallpaper_mode_desktop = CheckButton::new();
    wallpaper_mode_desktop.set_active(config.wallpaper_mode == "Desktop");
    let desktop_hbox = GtkBox::new(Orientation::Horizontal, 12);
    desktop_hbox.set_hexpand(true);
    let desktop_label = Label::new(Some("Desktop wallpaper"));
    desktop_label.set_xalign(0.0);
    desktop_label.set_hexpand(true);
    desktop_hbox.append(&desktop_label);
    desktop_hbox.append(&wallpaper_mode_desktop);
    wallpaper_frame.append(&build_row!(&desktop_hbox, false));

    // Don't change check
    let wallpaper_dont_change_check = CheckButton::new();
    wallpaper_dont_change_check.set_active(config.wallpaper_dont_change_on_space);
    let dont_change_hbox = GtkBox::new(Orientation::Horizontal, 12);
    dont_change_hbox.set_hexpand(true);
    let dont_change_label = Label::new(Some("Don't change the wallpaper when switching spaces"));
    dont_change_label.set_xalign(0.0);
    dont_change_label.set_hexpand(true);
    dont_change_hbox.append(&dont_change_label);
    dont_change_hbox.append(&wallpaper_dont_change_check);
    wallpaper_frame.append(&build_row!(&dont_change_hbox, true));

    // Custom
    let wallpaper_mode_custom = CheckButton::new();
    wallpaper_mode_custom.set_group(Some(&wallpaper_mode_desktop));
    wallpaper_mode_custom.set_active(config.wallpaper_mode == "Custom");
    
    let wallpaper_custom_path_btn = Button::new();
    wallpaper_custom_path_btn.add_css_class("secondary-settings-button");
    wallpaper_custom_path_btn.set_label(if config.wallpaper_custom_path.is_empty() {
        "Choose image..."
    } else {
        &config.wallpaper_custom_path
    });

    let custom_hbox = GtkBox::new(Orientation::Horizontal, 12);
    custom_hbox.set_hexpand(true);
    let custom_label = Label::new(Some("Custom wallpaper"));
    custom_label.set_xalign(0.0);
    custom_label.set_hexpand(true);
    let custom_row = GtkBox::new(Orientation::Horizontal, 12);
    custom_row.append(&wallpaper_custom_path_btn);
    custom_row.append(&wallpaper_mode_custom);
    custom_hbox.append(&custom_label);
    custom_hbox.append(&custom_row);
    wallpaper_frame.append(&build_row!(&custom_hbox, false));

    // Plain Color
    let wallpaper_mode_color = CheckButton::new();
    wallpaper_mode_color.set_group(Some(&wallpaper_mode_desktop));
    wallpaper_mode_color.set_active(config.wallpaper_mode == "Color");
    
    let wallpaper_color_btn = ColorButton::new();
    if let Ok(rgba) = gtk4::gdk::RGBA::parse(&config.wallpaper_plain_color) {
        wallpaper_color_btn.set_rgba(&rgba);
    }
    
    let color_hbox = GtkBox::new(Orientation::Horizontal, 12);
    color_hbox.set_hexpand(true);
    let color_label = Label::new(Some("Plain color"));
    color_label.set_xalign(0.0);
    color_label.set_hexpand(true);
    let color_row = GtkBox::new(Orientation::Horizontal, 12);
    color_row.append(&wallpaper_color_btn);
    color_row.append(&wallpaper_mode_color);
    color_hbox.append(&color_label);
    color_hbox.append(&color_row);
    wallpaper_frame.append(&build_row!(&color_hbox, true));

    section.append(&wallpaper_frame);

    // --- Window Screenshot Group ---
    let win_title = Label::new(Some("Window Screenshot"));
    win_title.add_css_class("settings-group-title");
    win_title.set_xalign(0.0);
    win_title.set_halign(Align::Start);
    win_title.set_margin_bottom(8);
    section.append(&win_title);

    let win_frame = build_frame();

    let mode_hbox = GtkBox::new(Orientation::Horizontal, 20);

    let create_mode_btn = |label: &str, icon: &str, active: bool| -> (GtkBox, CheckButton) {
        let container = GtkBox::new(Orientation::Vertical, 6);
        let check = CheckButton::new();
        check.set_active(active);
        check.add_css_class("mode-icon-check");
        let img_box = GtkBox::new(Orientation::Vertical, 0);
        img_box.set_size_request(80, 60);
        img_box.add_css_class("mode-preview-box");
        if active {
            img_box.add_css_class("active");
        }
        let icon_img = Image::from_icon_name(icon);
        icon_img.set_pixel_size(40);
        img_box.append(&icon_img);
        let lbl = Label::new(Some(label));
        lbl.add_css_class("settings-sub-option");
        container.append(&img_box);
        container.append(&lbl);
        (container, check)
    };

    let (full_btn, window_screenshot_mode_full) = create_mode_btn(
        "With wallpaper",
        "video-display-symbolic",
        config.window_screenshot_mode == "Wallpaper",
    );
    let (trans_btn, window_screenshot_mode_trans) = create_mode_btn(
        "Transparent",
        "view-app-grid-symbolic",
        config.window_screenshot_mode == "Transparent",
    );
    window_screenshot_mode_trans.set_group(Some(&window_screenshot_mode_full));

    let f_check = window_screenshot_mode_full.clone();
    let t_check = window_screenshot_mode_trans.clone();
    let f_box = full_btn
        .first_child()
        .unwrap()
        .downcast::<GtkBox>()
        .unwrap();
    let t_box = trans_btn
        .first_child()
        .unwrap()
        .downcast::<GtkBox>()
        .unwrap();
    let click_f = gtk4::GestureClick::new();
    let fc = f_check.clone();
    let fb = f_box.clone();
    let tb = t_box.clone();
    click_f.connect_pressed(move |_, _, _, _| {
        fc.set_active(true);
        fb.add_css_class("active");
        tb.remove_css_class("active");
    });
    full_btn.add_controller(click_f);
    let click_t = gtk4::GestureClick::new();
    let tc = t_check.clone();
    let fb2 = f_box.clone();
    let tb2 = t_box.clone();
    click_t.connect_pressed(move |_, _, _, _| {
        tc.set_active(true);
        tb2.add_css_class("active");
        fb2.remove_css_class("active");
    });
    trans_btn.add_controller(click_t);

    mode_hbox.append(&full_btn);
    mode_hbox.append(&trans_btn);

    let shift_hint = Label::new(Some("Hold ⇧ Shift while taking a screenshot to get a transparent background."));
    shift_hint.add_css_class("settings-sub-option-hint");
    shift_hint.set_xalign(0.0);
    
    let mode_vbox1 = GtkBox::new(Orientation::Vertical, 4);
    mode_vbox1.append(&Label::new(Some("Mode")));
    mode_vbox1.append(&shift_hint);
    
    let w_mode_hbox = GtkBox::new(Orientation::Horizontal, 12);
    w_mode_hbox.set_hexpand(true);
    let lbl_m = Label::new(Some("Mode"));
    lbl_m.set_xalign(0.0);
    let hint_m = Label::new(Some("Hold ⇧ Shift while taking a screenshot for transparent background."));
    hint_m.add_css_class("settings-sub-option-hint");
    hint_m.set_xalign(0.0);
    let v_m = GtkBox::new(Orientation::Vertical, 4);
    v_m.set_hexpand(true);
    v_m.append(&lbl_m);
    v_m.append(&hint_m);
    w_mode_hbox.append(&v_m);
    w_mode_hbox.append(&mode_hbox);
    win_frame.append(&build_row!(&w_mode_hbox, false));

    // Padding
    let window_screenshot_padding_input = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.05);
    window_screenshot_padding_input.set_value(config.window_screenshot_padding);
    window_screenshot_padding_input.set_hexpand(false);
    window_screenshot_padding_input.set_size_request(200, -1);
    let padding_vbox = GtkBox::new(Orientation::Vertical, 4);
    padding_vbox.append(&window_screenshot_padding_input);
    let padding_labels = CenterBox::new();
    padding_labels.set_start_widget(Some(&Label::new(Some("Min"))));
    padding_labels.set_center_widget(Some(&Label::new(Some("Default"))));
    padding_labels.set_end_widget(Some(&Label::new(Some("Max"))));
    padding_vbox.append(&padding_labels);
    
    let padding_hbox = GtkBox::new(Orientation::Horizontal, 12);
    padding_hbox.set_hexpand(true);
    let padding_label = Label::new(Some("Padding"));
    padding_label.set_xalign(0.0);
    padding_label.set_hexpand(true);
    padding_hbox.append(&padding_label);
    padding_hbox.append(&padding_vbox);
    win_frame.append(&build_row!(&padding_hbox, true));

    // Shadow
    let window_screenshot_shadow_check = CheckButton::new();
    window_screenshot_shadow_check.set_active(config.window_screenshot_shadow);
    let shadow_hbox = GtkBox::new(Orientation::Horizontal, 12);
    shadow_hbox.set_hexpand(true);
    let shadow_vbox = GtkBox::new(Orientation::Vertical, 4);
    shadow_vbox.set_hexpand(true);
    let shadow_main = Label::new(Some("Capture window shadow"));
    shadow_main.set_xalign(0.0);
    let opt_hint = Label::new(Some("Hold ⌥ (alt/option) while taking a screenshot to disable shadow."));
    opt_hint.add_css_class("settings-sub-option-hint");
    opt_hint.set_xalign(0.0);
    shadow_vbox.append(&shadow_main);
    shadow_vbox.append(&opt_hint);
    shadow_hbox.append(&shadow_vbox);
    shadow_hbox.append(&window_screenshot_shadow_check);
    win_frame.append(&build_row!(&shadow_hbox, false));

    section.append(&win_frame);

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
