use crate::config::{load_config, DEFAULT_AFTER_CAPTURE_SAVE};
use gtk4::{
    glib, prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button, CenterBox,
    CheckButton, FileChooserAction, FileChooserNative, Image, Label, Orientation, ResponseType,
    ScrolledWindow, Separator,
};

mod actions;
mod after_capture;
mod general;
mod storage;
mod ui_support;
mod windowing;

use self::{
    actions::{close_window, install_checkbox_behaviors, save_settings, SaveInputs},
    after_capture::build_after_capture_section,
    general::build_general_section,
    storage::build_storage_section,
    ui_support::{install_settings_css, traffic_light_button},
    windowing::{
        install_edge_resize, install_window_drag, prefers_dark_glass_theme,
        prefers_reduced_transparency, SETTINGS_WINDOW_MIN_HEIGHT, SETTINGS_WINDOW_MIN_WIDTH,
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
        .decorated(false)
        .build();
    window.set_size_request(SETTINGS_WINDOW_MIN_WIDTH, SETTINGS_WINDOW_MIN_HEIGHT);
    window.add_css_class("editor-window");

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    root.set_overflow(gtk4::Overflow::Hidden);
    if prefers_dark {
        root.add_css_class("editor-theme-dark");
    } else {
        root.add_css_class("editor-theme-light");
    }
    if reduced_transparency {
        root.add_css_class("editor-reduced-transparency");
    }

    let toolbar = CenterBox::new();
    toolbar.add_css_class("editor-toolbar");

    let traffic_close = traffic_light_button("traffic-light-red", "Close");
    let traffic_minimize = traffic_light_button("traffic-light-yellow", "Minimize");
    let traffic_zoom = traffic_light_button("traffic-light-green", "Maximize");

    let traffic_lights = GtkBox::new(Orientation::Horizontal, 6);
    traffic_lights.add_css_class("editor-traffic-lights");
    traffic_lights.append(&traffic_close);
    traffic_lights.append(&traffic_minimize);
    traffic_lights.append(&traffic_zoom);

    let left_group = GtkBox::new(Orientation::Horizontal, 16);
    left_group.add_css_class("editor-toolbar-left");
    left_group.append(&traffic_lights);
    toolbar.set_start_widget(Some(&left_group));

    let title = Label::new(Some("General"));
    title.add_css_class("title-4");
    title.set_halign(Align::Center);
    let title_drag_area = GtkBox::new(Orientation::Horizontal, 0);
    title_drag_area.set_halign(Align::Center);
    title_drag_area.append(&title);
    toolbar.set_center_widget(Some(&title_drag_area));

    let right_group = GtkBox::new(Orientation::Horizontal, 12);
    right_group.add_css_class("editor-toolbar-right");
    let save_status = Label::new(None);
    save_status.add_css_class("settings-save-status");
    save_status.set_visible(false);
    save_status.set_halign(Align::End);
    let save_btn = Button::with_label("Save");
    save_btn.add_css_class("suggested-action");
    right_group.append(&save_status);
    right_group.append(&save_btn);
    toolbar.set_end_widget(Some(&right_group));

    install_window_drag(&title_drag_area, &window);

    let content = GtkBox::new(Orientation::Vertical, 0);
    content.set_margin_top(12);
    content.set_margin_bottom(16);

    let content_scroller = ScrolledWindow::new();
    content_scroller.set_hexpand(true);
    content_scroller.set_vexpand(true);
    content_scroller.set_hscrollbar_policy(gtk4::PolicyType::Never);
    content_scroller.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
    content_scroller.set_propagate_natural_height(true);
    content_scroller.set_child(Some(&content));

    let body_frame = GtkBox::new(Orientation::Vertical, 0);
    body_frame.set_hexpand(true);
    body_frame.set_vexpand(true);
    body_frame.add_css_class("editor-canvas-frame");
    body_frame.append(&content_scroller);

    let nav_strip = GtkBox::new(Orientation::Horizontal, 18);
    nav_strip.add_css_class("settings-nav-strip");
    nav_strip.set_halign(Align::Start);
    nav_strip.set_hexpand(true);

    let nav_items = [
        ("preferences-system-symbolic", "General"),
        ("image-x-generic-symbolic", "Wallpaper"),
        ("input-keyboard-symbolic", "Shortcuts"),
        ("starred-symbolic", "Quick Access"),
        ("camera-video-symbolic", "Recording"),
        ("camera-photo-symbolic", "Screenshots"),
        ("draw-freehand-symbolic", "Annotate"),
        ("folder-cloud-symbolic", "Cloud"),
        ("applications-system-symbolic", "Advanced"),
        ("help-about-symbolic", "About"),
    ];

    for (index, (icon_name, label_text)) in nav_items.into_iter().enumerate() {
        let item = GtkBox::new(Orientation::Vertical, 0);
        item.add_css_class("settings-nav-item");
        item.set_halign(Align::Center);
        item.set_valign(Align::Start);

        let icon = Image::from_icon_name(icon_name);
        icon.add_css_class("settings-nav-icon");
        icon.set_pixel_size(22);
        icon.set_halign(Align::Center);

        let label = Label::new(Some(label_text));
        label.add_css_class("settings-nav-label");
        label.set_halign(Align::Center);

        if index == 0 {
            item.add_css_class("settings-nav-item-selected");
            icon.add_css_class("settings-nav-icon-selected");
            label.add_css_class("settings-nav-label-selected");
        }

        let motion = gtk4::EventControllerMotion::new();
        {
            let item = item.clone();
            let icon = icon.clone();
            let label = label.clone();
            motion.connect_enter(move |_, _, _| {
                item.add_css_class("settings-nav-item-hover");
                icon.add_css_class("settings-nav-icon-hover");
                label.add_css_class("settings-nav-label-hover");
            });
        }
        {
            let item = item.clone();
            let icon = icon.clone();
            let label = label.clone();
            motion.connect_leave(move |_| {
                item.remove_css_class("settings-nav-item-hover");
                icon.remove_css_class("settings-nav-icon-hover");
                label.remove_css_class("settings-nav-label-hover");
            });
        }
        item.add_controller(motion);

        item.append(&icon);
        item.append(&label);
        nav_strip.append(&item);
    }

    let general = build_general_section(&config);
    let storage = build_storage_section(&config);
    let after_capture = build_after_capture_section(&config);

    let general_separator = Separator::new(Orientation::Horizontal);
    general_separator.set_margin_top(8);
    general_separator.set_margin_bottom(8);
    general_separator.set_hexpand(true);

    let after_capture_separator = Separator::new(Orientation::Horizontal);
    after_capture_separator.set_margin_top(8);
    after_capture_separator.set_margin_bottom(8);
    after_capture_separator.set_hexpand(true);

    let export_location_entry_pick = storage.export_location_entry.clone();
    let window_weak_picker = window.downgrade();
    storage.export_location_browse.connect_clicked(move |_| {
        let chooser = FileChooserNative::new(
            Some("Select export location"),
            window_weak_picker.upgrade().as_ref(),
            FileChooserAction::SelectFolder,
            Some("Select"),
            Some("Cancel"),
        );
        let export_location_entry_pick = export_location_entry_pick.clone();
        chooser.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                if let Some(file) = dialog.file() {
                    if let Some(path) = file.path() {
                        export_location_entry_pick.set_text(&path.to_string_lossy());
                    }
                }
            }
            dialog.hide();
        });
        chooser.show();
    });

    let screenshot_quick_access_check = after_capture
        .screenshot_after_capture_checks
        .first()
        .cloned()
        .unwrap_or_else(CheckButton::new);
    let screenshot_copy_to_clipboard_check = after_capture
        .screenshot_after_capture_checks
        .get(1)
        .cloned()
        .unwrap_or_else(CheckButton::new);
    let screenshot_save_check = after_capture
        .screenshot_after_capture_checks
        .get(2)
        .cloned()
        .unwrap_or_else(|| {
            let check = CheckButton::new();
            check.set_active(DEFAULT_AFTER_CAPTURE_SAVE);
            check
        });
    let screenshot_open_annotate_check = after_capture
        .screenshot_after_capture_checks
        .get(3)
        .cloned()
        .unwrap_or_else(CheckButton::new);

    install_checkbox_behaviors(
        &general.play_sounds_check,
        &general.shutter_sound_input,
        &screenshot_quick_access_check,
        &screenshot_copy_to_clipboard_check,
        &screenshot_save_check,
        &screenshot_open_annotate_check,
    );

    content.append(&general.section);
    content.append(&general_separator);
    content.append(&storage.wrapper);
    content.append(&after_capture_separator);
    content.append(&after_capture.wrapper);

    root.append(&toolbar);
    root.append(&nav_strip);
    root.append(&body_frame);

    install_edge_resize(&root, &window);
    window.set_child(Some(&root));

    let window_weak_close = window.downgrade();
    traffic_close.connect_clicked(move |_| {
        if let Some(window) = window_weak_close.upgrade() {
            close_window(&window);
        }
    });

    let window_weak_minimize = window.downgrade();
    traffic_minimize.connect_clicked(move |_| {
        if let Some(window) = window_weak_minimize.upgrade() {
            window.minimize();
        }
    });

    let window_weak_zoom = window.downgrade();
    traffic_zoom.connect_clicked(move |_| {
        if let Some(window) = window_weak_zoom.upgrade() {
            if window.is_maximized() {
                window.unmaximize();
            } else {
                window.maximize();
            }
        }
    });

    let save_inputs = SaveInputs {
        start_at_login: general.start_at_login_check.clone(),
        play_sounds: general.play_sounds_check.clone(),
        shutter_sound: general.shutter_sound_input.clone(),
        show_tray_icon: general.show_icon_check.clone(),
        export_location: storage.export_location_entry.clone(),
        hide_desktop_icons: storage.hide_desktop_icons_check.clone(),
        screenshot_quick_access: screenshot_quick_access_check,
        screenshot_copy_to_clipboard: screenshot_copy_to_clipboard_check,
        screenshot_save: screenshot_save_check,
        screenshot_open_annotate: screenshot_open_annotate_check,
    };
    let save_status_label = save_status.clone();
    save_btn.connect_clicked(move |_| {
        save_status_label.remove_css_class("settings-save-status-success");
        save_status_label.remove_css_class("settings-save-status-error");

        match save_settings(&save_inputs) {
            Ok(_) => {
                save_status_label.set_text("Saved");
                save_status_label.add_css_class("settings-save-status-success");
            }
            Err(e) => {
                eprintln!("[settings] Failed to save config: {e}");
                save_status_label.set_text("Save failed");
                save_status_label.add_css_class("settings-save-status-error");
            }
        }

        save_status_label.set_visible(true);
        let save_status_label = save_status_label.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
            save_status_label.set_visible(false);
        });
    });

    let app_weak = app.downgrade();
    window.connect_close_request(move |_| {
        if let Some(app) = app_weak.upgrade() {
            app.quit();
        }
        glib::Propagation::Proceed
    });

    window.present();
}
