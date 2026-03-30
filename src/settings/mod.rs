use crate::config::load_config;
use gtk4::{
    prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button,
    Image, Label, Orientation, ScrolledWindow,
};

mod about;
mod actions;
mod advanced;
mod after_capture;
mod annotate;
mod cloud;
mod general;
mod quick_access;
mod recording;
mod screenshots;
mod shortcuts;
mod storage;
mod ui_support;
mod windowing;
mod wallpaper;

use self::{
    actions::{install_checkbox_behaviors, save_settings, SaveInputs},
    after_capture::build_after_capture_section,
    general::build_general_section,
    quick_access::build_quick_access_section,
    screenshots::build_screenshots_section,
    annotate::build_annotate_section,
    ui_support::{install_settings_css, traffic_light_button},
    windowing::{
        install_edge_resize, install_window_drag, prefers_dark_glass_theme,
        prefers_reduced_transparency,
    },
};

pub fn show_settings_window() -> anyhow::Result<()> {
    let app = Application::builder()
        .application_id("com.apexshot.settings")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(build_settings_window);
    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn build_settings_window(app: &Application) {
    install_settings_css();

    let config = load_config().sanitized();
    let prefers_dark = prefers_dark_glass_theme();
    let reduced_transparency = prefers_reduced_transparency();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("ApexShot Settings")
        .default_width(920)
        .default_height(820)
        .build();

    window.set_decorated(false);
    window.add_css_class("editor-window");

    let root_box = GtkBox::new(Orientation::Vertical, 0);
    root_box.add_css_class("editor-root");
    if !prefers_dark {
        root_box.add_css_class("editor-theme-light");
    }
    if reduced_transparency {
        root_box.add_css_class("editor-reduced-transparency");
    }

    // --- TOOLBAR ---
    let toolbar = GtkBox::new(Orientation::Horizontal, 0);
    toolbar.add_css_class("editor-toolbar");

    let left_box = GtkBox::new(Orientation::Horizontal, 8);
    left_box.set_hexpand(true);
    left_box.set_halign(Align::Start);
    left_box.add_css_class("editor-toolbar-left");

    let close_btn = traffic_light_button("close", "Close");
    let win_clone = window.clone();
    close_btn.connect_clicked(move |_| win_clone.close());

    let min_btn = traffic_light_button("minimize", "Minimize");
    let win_clone = window.clone();
    min_btn.connect_clicked(move |_| win_clone.minimize());

    let max_btn = traffic_light_button("maximize", "Maximize");

    left_box.append(&close_btn);
    left_box.append(&min_btn);
    left_box.append(&max_btn);
    toolbar.append(&left_box);

    let save_status = Label::new(None);
    save_status.add_css_class("settings-save-status");
    let save_btn = Button::with_label("Save Changes");
    save_btn.add_css_class("primary-settings-button");
    
    let right_box = GtkBox::new(Orientation::Horizontal, 12);
    right_box.set_hexpand(true);
    right_box.set_halign(Align::End);
    right_box.add_css_class("editor-toolbar-right");
    right_box.append(&save_status);
    right_box.append(&save_btn);
    toolbar.append(&right_box);

    root_box.append(&toolbar);

    // --- WINDOW GESTURES ---
    install_window_drag(&toolbar, &window);
    install_edge_resize(&root_box, &window);

    // --- NAVIGATION ---
    let nav_strip = GtkBox::new(Orientation::Horizontal, 0);
    nav_strip.add_css_class("settings-nav-strip");
    nav_strip.set_halign(Align::Center);

    let labels = [
        ("General", "preferences-system-symbolic"),
        ("Wallpaper", "folder-pictures-symbolic"),
        ("Shortcuts", "input-keyboard-symbolic"),
        ("Quick Access", "view-list-symbolic"),
        ("Recording", "media-record-symbolic"),
        ("Screenshots", "camera-photo-symbolic"),
        ("Annotate", "content-loading-symbolic"),
        ("Cloud", "folder-remote-symbolic"),
        ("Advanced", "preferences-other-symbolic"),
        ("About", "help-about-symbolic"),
    ];

    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
    stack.set_vexpand(true);

    let mut nav_buttons = Vec::new();

    for (i, (label_text, icon_name)) in labels.iter().enumerate() {
        let btn = Button::builder()
            .has_frame(false)
            .build();
        btn.add_css_class("settings-nav-item");

        let content = GtkBox::new(Orientation::Vertical, 4);
        let icon = Image::from_icon_name(icon_name);
        icon.add_css_class("settings-nav-icon");
        let label = Label::new(Some(label_text));
        label.add_css_class("settings-nav-label");

        content.append(&icon);
        content.append(&label);
        btn.set_child(Some(&content));

        let s_clone = stack.clone();
        let idx_str = i.to_string();
        btn.connect_clicked(move |_| {
            s_clone.set_visible_child_name(&idx_str);
        });

        nav_strip.append(&btn);
        nav_buttons.push(btn);
    }

    root_box.append(&nav_strip);

    // --- BODY (STACK) ---
    let body_frame = GtkBox::new(Orientation::Vertical, 0);
    body_frame.set_vexpand(true);
    body_frame.set_hexpand(true);

    // Build all sections
    let general = build_general_section(&config);
    let after_capture = build_after_capture_section(&config);
    let recordings = recording::build_recording_section(&config);
    let screenshots = build_screenshots_section(&config);
    let cloud = cloud::build_cloud_section(&config);
    let advanced = advanced::build_advanced_section(&config);
    let about = about::build_about_section();
    let annotate = build_annotate_section(&config);
    let shortcuts = shortcuts::build_shortcuts_section(&config);
    let quick_access = build_quick_access_section(&config);
    let wallpaper = wallpaper::build_wallpaper_section(&config);

    // Add them to stack
    fn add_section(stack: &gtk4::Stack, widget: &impl IsA<gtk4::Widget>, name: &str, title: &str) {
        let scroller = ScrolledWindow::new();
        scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.set_margin_top(12);
        vbox.set_margin_bottom(16);
        vbox.append(widget);
        scroller.set_child(Some(&vbox));
        stack.add_titled(&scroller, Some(name), title);
    }

    add_section(&stack, &general.section, "0", "General");
    add_section(&stack, &wallpaper.section, "1", "Wallpaper");
    add_section(&stack, &shortcuts.section, "2", "Shortcuts");
    add_section(&stack, &quick_access.section, "3", "Quick Access");
    add_section(&stack, &recordings.section, "4", "Recording");
    add_section(&stack, &screenshots.section, "5", "Screenshots");
    add_section(&stack, &annotate.section, "6", "Annotate");
    add_section(&stack, &cloud.section, "7", "Cloud");
    add_section(&stack, &advanced.section, "8", "Advanced");
    add_section(&stack, &about.section, "9", "About");

    body_frame.append(&stack);

    // Update nav selection on stack change
    let nav_btns_clone = nav_buttons.clone();
    stack.connect_visible_child_name_notify(move |s| {
        if let Some(name) = s.visible_child_name() {
            if let Ok(idx) = name.parse::<usize>() {
                for (i, btn) in nav_btns_clone.iter().enumerate() {
                    if i == idx {
                        btn.add_css_class("settings-nav-item-selected");
                    } else {
                        btn.remove_css_class("settings-nav-item-selected");
                    }
                }
            }
        }
    });
    // Set initial
    nav_buttons[0].add_css_class("settings-nav-item-selected");

    root_box.append(&body_frame);
    window.set_child(Some(&root_box));

    // --- SAVE LOGIC ---
    let save_inputs = SaveInputs {
        start_at_login: general.start_at_login_check.clone(),
        play_sounds: general.play_sounds_check.clone(),
        shutter_sound: general.shutter_sound_input.clone(),
        show_menu_bar_icon: general.show_icon_check.clone(),
        screenshot_quick_access: after_capture.screenshot_after_capture_checks[0].clone(),
        screenshot_copy_to_clipboard: after_capture.screenshot_after_capture_checks[1].clone(),
        screenshot_save: after_capture.screenshot_after_capture_checks[2].clone(),
        screenshot_open_annotate: after_capture.screenshot_after_capture_checks[3].clone(),
        quick_access_auto_close_enabled: quick_access.auto_close_enabled_check.clone(),
        quick_access_auto_close_action: quick_access.auto_close_action_input.clone(),
        quick_access_auto_close_interval: quick_access.auto_close_interval_input.clone(),
        screenshot_crosshair_mode: screenshots.crosshair_mode_input.clone(),
        screenshot_show_magnifier: screenshots.show_magnifier_check.clone(),
        screenshot_freeze_screen: screenshots.freeze_screen_check.clone(),
        screenshot_capture_cursor: screenshots.show_cursor_check.clone(),
        rec_controls: recordings.rec_controls_check.clone(),
        rec_display_time: recordings.rec_display_time_check.clone(),
        rec_hidpi: recordings.rec_hidpi_check.clone(),
        rec_notifications: recordings.rec_notifications_check.clone(),
        rec_cursor: recordings.rec_cursor_check.clone(),
        rec_clicks: recordings.rec_clicks_check.clone(),
        rec_keystrokes: recordings.rec_keystrokes_check.clone(),
        rec_key_filter: recordings.rec_key_filter_input.clone(),
        wallpaper_mode_desktop: wallpaper.wallpaper_mode_desktop.clone(),
        wallpaper_dont_change_on_space: wallpaper.wallpaper_dont_change_check.clone(),
        wallpaper_mode_custom: wallpaper.wallpaper_mode_custom.clone(),
        wallpaper_custom_path_btn: wallpaper.wallpaper_custom_path_btn.clone(),
        wallpaper_mode_color: wallpaper.wallpaper_mode_color.clone(),
        wallpaper_color_btn: wallpaper.wallpaper_color_btn.clone(),
        window_screenshot_mode_full: wallpaper.window_screenshot_mode_full.clone(),
        window_screenshot_mode_trans: wallpaper.window_screenshot_mode_trans.clone(),
        window_screenshot_padding: wallpaper.window_screenshot_padding_input.clone(),
        window_screenshot_shadow: wallpaper.window_screenshot_shadow_check.clone(),
        shortcut_toggle_desktop_icons: shortcuts.toggle_icons_btn.clone(),
        shortcut_open_file: shortcuts.open_file_btn.clone(),
        shortcut_open_from_clipboard: shortcuts.open_clipboard_btn.clone(),
        shortcut_pin_to_screen: shortcuts.pin_screen_btn.clone(),
        shortcut_restore_recently_closed: shortcuts.restore_file_btn.clone(),
        shortcut_toggle_overlays: shortcuts.toggle_overlays_btn.clone(),
        shortcut_capture_area: shortcuts.capture_area_btn.clone(),
        shortcut_capture_previous_area: shortcuts.capture_prev_btn.clone(),
        shortcut_capture_fullscreen: shortcuts.capture_fullscreen_btn.clone(),
        shortcut_capture_window: shortcuts.capture_window_btn.clone(),
        cloud_screenshot_quality: cloud.cloud_quality_input.clone(),
        cloud_copy_to_clipboard: cloud.cloud_clipboard_input.clone(),
        cloud_show_recently_uploaded: cloud.cloud_show_recent_check.clone(),
        cloud_ask_name_tags: cloud.cloud_ask_tags_check.clone(),
        adv_ask_name_after_capture: advanced.ask_name_check.clone(),
        adv_retina_suffix: advanced.retina_suffix_check.clone(),
        adv_clipboard_mode: advanced.clipboard_mode_input.clone(),
        adv_pinned_rounded_corners: advanced.pinned_rounded_check.clone(),
        adv_pinned_shadow: advanced.pinned_shadow_check.clone(),
        adv_pinned_border: advanced.pinned_border_check.clone(),
        adv_ocr_language: advanced.ocr_lang_input.clone(),
        adv_ocr_keep_line_breaks: advanced.ocr_line_breaks_check.clone(),
    };
    
    let edit_btn = advanced.filename_edit_btn.clone();
    let win_weak = window.downgrade();
    let config_clone = config.clone();
    edit_btn.connect_clicked(move |_| {
        if let Some(win) = win_weak.upgrade() {
            advanced::show_filename_format_modal(&win, &config_clone);
        }
    });

    let save_status_label = save_status.clone();
    let config_clone_for_save = config.clone();
    save_btn.connect_clicked(move |_| {
        save_status_label.remove_css_class("settings-save-status-success");
        save_status_label.remove_css_class("settings-save-status-error");
        save_status_label.set_text("Saving...");

        match save_settings(&save_inputs, config_clone_for_save.clone()) {
            Ok(_) => {
                save_status_label.set_text("Saved");
                save_status_label.add_css_class("settings-save-status-success");
            }
            Err(e) => {
                save_status_label.set_text(&format!("Error: {}", e));
                save_status_label.add_css_class("settings-save-status-error");
            }
        }
    });

    install_checkbox_behaviors(
        &general.play_sounds_check,
        &general.shutter_sound_input,
        &after_capture.screenshot_after_capture_checks[0],
        &after_capture.screenshot_after_capture_checks[1],
        &after_capture.screenshot_after_capture_checks[2],
        &after_capture.screenshot_after_capture_checks[3],
        &quick_access.auto_close_enabled_check,
        &quick_access.auto_close_action_input,
        &quick_access.auto_close_interval_input,
        &screenshots.crosshair_mode_input,
        &screenshots.show_magnifier_check,
    );

    window.present();
}
