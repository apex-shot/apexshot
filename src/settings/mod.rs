use crate::config::load_config;
use gtk4::{
    prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button, FileChooserAction,
    FileChooserNative, Image, Label, Orientation, Overlay as GtkOverlay, ResponseType,
    ScrolledWindow, Separator,
};
use std::rc::Rc;

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
pub(crate) mod ui_support;
pub(crate) mod windowing;

use self::{
    actions::{install_checkbox_behaviors, save_settings, SaveInputs},
    after_capture::build_after_capture_section,
    annotate::build_annotate_section,
    general::build_general_section,
    quick_access::build_quick_access_section,
    screenshots::build_screenshots_section,
    ui_support::{install_settings_css, traffic_light_button},
    windowing::{
        install_edge_resize, install_window_drag, prefers_dark_glass_theme,
        prefers_reduced_transparency,
    },
};

pub fn show_settings_window() -> anyhow::Result<()> {
    // Force-set GIO_LAUNCHED_DESKTOP_FILE to the main app's desktop entry
    // so GNOME Shell shows the correct icon and name.
    if let Some(desktop_path) = crate::app_identity::desktop_file_for_portal() {
        std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", desktop_path);
        std::env::set_var(
            "GIO_LAUNCHED_DESKTOP_FILE_PID",
            std::process::id().to_string(),
        );
    }

    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .build();

    app.connect_activate(|application| {
        // Check if a window already exists (single-instance behavior)
        // If yes, just present it instead of creating a new one
        let windows = application.windows();
        if let Some(existing_window) = windows.first() {
            existing_window.present();
            return;
        }

        // Start daemon if config shows tray should be visible
        let config = load_config();
        if config.show_menu_bar_icon {
            let _ = crate::daemon::start_daemon_subprocess();
        }

        build_settings_window(application);
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn build_settings_window(app: &Application) {
    use std::sync::Once;
    static INIT_ICONS: Once = Once::new();
    INIT_ICONS.call_once(|| {
        relm4_icons::initialize_icons(
            crate::capture::editor::window::icon_names::GRESOURCE_BYTES,
            crate::capture::editor::window::icon_names::RESOURCE_PREFIX,
        );
    });

    install_settings_css();

    let config = load_config().sanitized();
    let prefers_dark = prefers_dark_glass_theme();
    let reduced_transparency = prefers_reduced_transparency();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("ApexShot Settings")
        .icon_name(crate::app_identity::icon_name())
        .default_width(1020)
        .default_height(840)
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
    toolbar.add_css_class("settings-window-controls");
    toolbar.set_size_request(-1, 30);

    let drag_handle = GtkBox::new(Orientation::Horizontal, 0);
    drag_handle.set_hexpand(true);
    drag_handle.set_halign(Align::Fill);
    drag_handle.set_vexpand(false);
    toolbar.append(&drag_handle);

    let close_btn = traffic_light_button("traffic-light-red", "Close");
    close_btn.remove_css_class("recent-captures-wm-btn");
    close_btn.remove_css_class("recent-captures-wm-close");
    close_btn.add_css_class("recording-editor-traffic-btn");
    let win_clone = window.clone();
    close_btn.connect_clicked(move |_| win_clone.close());

    let min_btn = traffic_light_button("traffic-light-yellow", "Minimize");
    min_btn.remove_css_class("recent-captures-wm-btn");
    min_btn.add_css_class("recording-editor-traffic-btn");
    let win_clone = window.clone();
    min_btn.connect_clicked(move |_| win_clone.minimize());

    for button in [&close_btn, &min_btn] {
        button.set_size_request(24, 24);
        button.set_valign(Align::Center);
    }

    let right_box = GtkBox::new(Orientation::Horizontal, 6);
    right_box.set_halign(Align::End);
    right_box.append(&min_btn);
    right_box.append(&close_btn);
    toolbar.append(&right_box);

    let save_btn = Button::with_label("Save");
    save_btn.add_css_class("settings-primary-btn");

    let toast = Label::new(None);
    toast.add_css_class("settings-toast");
    toast.set_halign(Align::Center);
    toast.set_valign(Align::Start);
    toast.set_margin_top(18);
    toast.set_visible(false);

    let window_overlay = GtkOverlay::new();
    if !prefers_dark {
        window_overlay.add_css_class("editor-theme-light");
    }
    if reduced_transparency {
        window_overlay.add_css_class("editor-reduced-transparency");
    }
    window_overlay.set_child(Some(&root_box));
    window_overlay.add_overlay(&toast);

    root_box.append(&toolbar);

    // --- WINDOW GESTURES ---
    install_window_drag(&drag_handle, &window);
    install_edge_resize(&root_box, &window);

    // --- LAYOUT SPLIT ---
    let content_split = GtkBox::new(Orientation::Horizontal, 0);
    content_split.set_vexpand(true);
    content_split.set_hexpand(true);

    // --- NAVIGATION (SIDEBAR) ---
    let sidebar_scroller = ScrolledWindow::new();
    sidebar_scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

    let nav_strip = GtkBox::new(Orientation::Vertical, 4);
    nav_strip.add_css_class("settings-sidebar");
    nav_strip.set_halign(Align::Fill);
    nav_strip.set_valign(Align::Fill);
    nav_strip.set_hexpand(false);
    nav_strip.set_vexpand(true);

    sidebar_scroller.set_child(Some(&nav_strip));

    let sidebar_wrapper = GtkBox::new(Orientation::Vertical, 0);
    sidebar_wrapper.add_css_class("settings-sidebar-wrapper");
    sidebar_wrapper.set_vexpand(true);
    sidebar_wrapper.append(&sidebar_scroller);

    let save_box = GtkBox::new(Orientation::Vertical, 0);
    save_box.add_css_class("settings-save-box");
    save_box.set_halign(Align::Fill);
    save_box.set_margin_start(8);
    save_box.set_margin_end(8);
    save_box.set_margin_bottom(10);
    save_box.set_margin_top(6);
    save_btn.set_hexpand(true);
    save_box.append(&save_btn);
    sidebar_wrapper.append(&save_box);

    use crate::capture::editor::window::icon_names::custom;
    let labels = [
        ("General", custom::SETTINGS_SYMBOLIC),
        ("Shortcuts", custom::KEYBOARD_SHORTCUTS_SYMBOLIC),
        ("Quick Access", custom::OVERLAPPING_WINDOWS_SYMBOLIC),
        ("Recording", custom::RECORD_SCREEN_SYMBOLIC),
        ("Screenshots", custom::SCREENSHOOTER_SYMBOLIC),
        ("Annotate", custom::APP_ICON_DESIGN_SYMBOLIC),
        ("Cloud", custom::CLOUD_OUTLINE_THIN_SYMBOLIC),
        ("Advanced", custom::SETTINGS_SYMBOLIC),
        ("About", custom::INFO_SYMBOLIC),
    ];

    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
    stack.set_vexpand(true);

    let mut nav_items = Vec::new();

    for (i, (label_text, icon_name)) in labels.iter().enumerate() {
        let item = GtkBox::new(Orientation::Horizontal, 8);
        item.add_css_class("settings-nav-item");
        item.set_halign(Align::Fill);
        item.set_valign(Align::Center);

        let icon = Image::from_icon_name(icon_name);
        icon.add_css_class("settings-nav-icon");
        icon.set_pixel_size(16);
        icon.set_halign(Align::Start);

        let label = Label::new(Some(label_text));
        label.add_css_class("settings-nav-label");
        label.set_halign(Align::Start);

        item.append(&icon);
        item.append(&label);

        if i == 0 {
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

        let s_clone = stack.clone();
        let idx_str = i.to_string();
        let click = gtk4::GestureClick::new();
        click.connect_released(move |_, _, _, _| {
            s_clone.set_visible_child_name(&idx_str);
        });
        item.add_controller(click);

        nav_strip.append(&item);
        nav_items.push((item, icon, label));
    }

    content_split.append(&sidebar_wrapper);

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
    shortcuts::install_shortcut_editors(&shortcuts, &window);
    let quick_access = build_quick_access_section(&config);

    let after_capture_separator = Separator::new(Orientation::Horizontal);
    after_capture_separator.set_margin_top(8);
    after_capture_separator.set_margin_bottom(8);
    after_capture_separator.set_hexpand(true);

    let screenshot_export_location_entry_pick = screenshots.export_location_entry.clone();
    let window_weak_picker = window.downgrade();
    screenshots
        .export_location_browse
        .connect_clicked(move |_| {
            let chooser = FileChooserNative::new(
                Some("Select screenshot save location"),
                window_weak_picker.upgrade().as_ref(),
                FileChooserAction::SelectFolder,
                Some("Select"),
                Some("Cancel"),
            );
            let export_location_entry_pick = screenshot_export_location_entry_pick.clone();
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

    let video_export_location_entry_pick = recordings.video_export_location_entry.clone();
    let window_weak_picker = window.downgrade();
    recordings
        .video_export_location_browse
        .connect_clicked(move |_| {
            let chooser = FileChooserNative::new(
                Some("Select video save location"),
                window_weak_picker.upgrade().as_ref(),
                FileChooserAction::SelectFolder,
                Some("Select"),
                Some("Cancel"),
            );
            let export_location_entry_pick = video_export_location_entry_pick.clone();
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

    let general_tab_section = GtkBox::new(Orientation::Vertical, 0);
    general_tab_section.append(&general.section);
    general_tab_section.append(&after_capture_separator);
    general_tab_section.append(&after_capture.wrapper);

    // Add them to stack
    fn add_section(stack: &gtk4::Stack, widget: &impl IsA<gtk4::Widget>, name: &str, title: &str) {
        let scroller = ScrolledWindow::new();
        scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.set_margin_top(20);
        vbox.set_margin_bottom(32);
        vbox.set_margin_start(28);
        vbox.set_margin_end(28);

        let header = GtkBox::new(Orientation::Vertical, 4);
        header.set_margin_bottom(16);

        let title_lbl = Label::new(Some(title));
        title_lbl.add_css_class("settings-page-title");
        title_lbl.set_halign(Align::Start);

        header.append(&title_lbl);
        vbox.append(&header);

        vbox.append(widget);
        scroller.set_child(Some(&vbox));
        stack.add_titled(&scroller, Some(name), title);
    }

    add_section(&stack, &general_tab_section, "0", "General");
    add_section(&stack, &shortcuts.section, "1", "Shortcuts");
    add_section(&stack, &quick_access.section, "2", "Quick Access");
    add_section(&stack, &recordings.section, "3", "Recording");
    add_section(&stack, &screenshots.section, "4", "Screenshots");
    add_section(&stack, &annotate.section, "5", "Annotate");
    add_section(&stack, &cloud.section, "6", "Cloud");
    add_section(&stack, &advanced.section, "7", "Advanced");
    add_section(&stack, &about.section, "8", "About");

    body_frame.append(&stack);

    // Update nav selection on stack change
    let nav_items_clone = nav_items.clone();
    stack.connect_visible_child_name_notify(move |s| {
        if let Some(name) = s.visible_child_name() {
            if let Ok(idx) = name.parse::<usize>() {
                for (i, (item, icon, label)) in nav_items_clone.iter().enumerate() {
                    if i == idx {
                        item.add_css_class("settings-nav-item-selected");
                        icon.add_css_class("settings-nav-icon-selected");
                        label.add_css_class("settings-nav-label-selected");
                    } else {
                        item.remove_css_class("settings-nav-item-selected");
                        icon.remove_css_class("settings-nav-icon-selected");
                        label.remove_css_class("settings-nav-label-selected");
                    }
                }
            }
        }
    });
    stack.set_visible_child_name("0");

    content_split.append(&body_frame);
    root_box.append(&content_split);
    window.set_child(Some(&window_overlay));

    // --- SAVE LOGIC ---
    let save_inputs = Rc::new(SaveInputs {
        start_at_login: general.start_at_login_check.clone(),
        play_sounds: general.play_sounds_check.clone(),
        shutter_sound: general.shutter_sound_input.clone(),
        show_menu_bar_icon: general.show_icon_check.clone(),
        screenshot_export_location: screenshots.export_location_entry.clone(),
        screenshot_format: screenshots.format_input.clone(),
        video_export_location: recordings.video_export_location_entry.clone(),
        rec_filename_pattern: recordings.rec_filename_pattern_entry.clone(),
        screenshot_quick_access: after_capture.screenshot_after_capture_checks[0].clone(),
        screenshot_copy_to_clipboard: after_capture.screenshot_after_capture_checks[1].clone(),
        screenshot_save: after_capture.screenshot_after_capture_checks[2].clone(),
        screenshot_open_annotate: after_capture.screenshot_after_capture_checks[3].clone(),
        rec_copy_to_clipboard: after_capture.rec_copy_to_clipboard.clone(),
        rec_save: after_capture.rec_save.clone(),
        rec_open_video_editor: after_capture.rec_open_video_editor.clone(),
        quick_access_position: quick_access.position_input.clone(),
        quick_access_multi_display: quick_access.multi_display_check.clone(),
        quick_access_overlay_size: quick_access.overlay_size_input.clone(),
        quick_access_auto_close_enabled: quick_access.auto_close_enabled_check.clone(),
        quick_access_auto_close_action: quick_access.auto_close_action_input.clone(),
        quick_access_auto_close_interval: quick_access.auto_close_interval_input.clone(),
        quick_access_close_after_dragging: quick_access.close_after_dragging_check.clone(),
        quick_access_close_after_uploading: quick_access.close_after_uploading_check.clone(),
        screenshot_crosshair_mode: screenshots.crosshair_mode_input.clone(),
        screenshot_show_magnifier: screenshots.show_magnifier_check.clone(),
        screenshot_freeze_screen: screenshots.freeze_screen_check.clone(),
        screenshot_timer_interval: screenshots.timer_interval_input.clone(),
        screenshot_capture_cursor: screenshots.show_cursor_check.clone(),
        annotate_inverse_arrow: annotate.inverse_arrow_check.clone(),
        annotate_smooth_drawing: annotate.smooth_drawing_check.clone(),
        annotate_draw_shadow: annotate.draw_shadow_check.clone(),
        annotate_auto_expand: annotate.auto_expand_check.clone(),
        annotate_show_color_names: annotate.show_color_names_check.clone(),
        annotate_always_on_top: annotate.always_on_top_check.clone(),
        rec_notifications: recordings.rec_notifications_check.clone(),
        rec_countdown: recordings.rec_countdown_check.clone(),
        rec_remember_selection: recordings.rec_remember_selection_check.clone(),
        rec_display_time: recordings.rec_display_time_check.clone(),
        shortcut_open_file: shortcuts.open_file_btn.clone(),
        shortcut_open_from_clipboard: shortcuts.open_clipboard_btn.clone(),
        shortcut_restore_recently_closed: shortcuts.restore_file_btn.clone(),
        shortcut_toggle_overlays: shortcuts.toggle_overlays_btn.clone(),
        shortcut_capture_area: shortcuts.capture_area_btn.clone(),
        shortcut_capture_crosshair: shortcuts.capture_crosshair_btn.clone(),
        shortcut_capture_previous_area: shortcuts.capture_prev_btn.clone(),
        shortcut_capture_fullscreen: shortcuts.capture_fullscreen_btn.clone(),
        shortcut_capture_window: shortcuts.capture_window_btn.clone(),
        shortcut_show_last_preview: shortcuts.show_last_preview_btn.clone(),
        shortcut_open_recording_ui: shortcuts.open_recording_ui_btn.clone(),
        shortcut_record_screen: shortcuts.record_screen_btn.clone(),
        shortcut_recording_pause_resume: shortcuts.recording_pause_resume_btn.clone(),
        shortcut_recording_stop_save: shortcuts.recording_stop_save_btn.clone(),
        shortcut_recording_restart: shortcuts.recording_restart_btn.clone(),
        shortcut_recording_discard: shortcuts.recording_discard_btn.clone(),
        adv_retina_suffix: advanced.retina_suffix_check.clone(),
        adv_clipboard_mode: screenshots.clipboard_mode_input.clone(),
        adv_ocr_language: advanced.ocr_lang_input.clone(),
        adv_ocr_keep_line_breaks: advanced.ocr_line_breaks_check.clone(),
        cloud_destination: cloud.destination_combo.clone(),
        xbackbone_url: cloud.xb_url_entry.clone(),
        xbackbone_api_token: cloud.xb_token_entry.clone(),
    });

    let trigger_save: Rc<dyn Fn()> = {
        let save_inputs = Rc::clone(&save_inputs);
        let toast = toast.clone();
        Rc::new(move || {
            toast.remove_css_class("settings-toast-success");
            toast.remove_css_class("settings-toast-error");

            match save_settings(&save_inputs) {
                Ok(_) => {
                    toast.set_text("Settings saved");
                    toast.add_css_class("settings-toast-success");
                }
                Err(e) => {
                    toast.set_text(&format!("Save failed: {}", e));
                    toast.add_css_class("settings-toast-error");
                }
            }
            toast.set_visible(true);
            let toast = toast.clone();
            gtk4::glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                toast.set_visible(false);
                toast.remove_css_class("settings-toast-success");
                toast.remove_css_class("settings-toast-error");
            });
        })
    };
    let trigger_save_click = Rc::clone(&trigger_save);
    save_btn.connect_clicked(move |_| {
        trigger_save_click();
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
