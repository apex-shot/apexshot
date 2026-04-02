use crate::config::AppConfig;
use gtk4::{prelude::*, Align, Box as GtkBox, Button, Image, Label, Orientation};

pub struct ShortcutSettingsWidgets {
    pub section: GtkBox,
    pub toggle_icons_btn: Button,
    pub open_file_btn: Button,
    pub open_clipboard_btn: Button,
    pub pin_screen_btn: Button,
    pub restore_file_btn: Button,
    pub toggle_overlays_btn: Button,
    pub capture_area_btn: Button,
    pub capture_crosshair_btn: Button,
    pub capture_prev_btn: Button,
    pub capture_fullscreen_btn: Button,
    pub capture_window_btn: Button,
}

pub fn build_shortcuts_section(config: &AppConfig) -> ShortcutSettingsWidgets {
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

    let create_header = |section: &GtkBox, label_text: &str, icon: &str| {
        let hbox = GtkBox::new(Orientation::Horizontal, 10);
        hbox.set_margin_bottom(8);
        hbox.set_margin_top(8);

        let img = Image::from_icon_name(icon);
        img.set_pixel_size(20);

        let lbl = Label::new(Some(label_text));
        lbl.add_css_class("settings-group-title");

        hbox.append(&img);
        hbox.append(&lbl);
        section.append(&hbox);
    };

    let create_row =
        |frame: &GtkBox, label_text: &str, current_val: &str, is_muted: bool| -> Button {
            let hbox = GtkBox::new(Orientation::Horizontal, 12);
            hbox.set_hexpand(true);

            let lbl = Label::new(Some(label_text));
            lbl.set_xalign(0.0);
            lbl.set_hexpand(true);

            let btn = Button::new();
            btn.add_css_class("shortcuts-record-btn");
            btn.set_label(if current_val.is_empty() {
                "Record shortcut"
            } else {
                current_val
            });
            btn.set_size_request(200, -1);

            hbox.append(&lbl);
            hbox.append(&btn);

            frame.append(&build_row!(&hbox, is_muted));
            btn
        };

    // --- General Section ---
    create_header(&section, "General", "emblem-system-symbolic");
    let general_frame = build_frame();
    let toggle_icons_btn = create_row(
        &general_frame,
        "Toggle Desktop Icons:",
        &config.shortcut_toggle_desktop_icons,
        false,
    );
    let open_file_btn = create_row(
        &general_frame,
        "Open File:",
        &config.shortcut_open_file,
        true,
    );
    let open_clipboard_btn = create_row(
        &general_frame,
        "Open From Clipboard:",
        &config.shortcut_open_from_clipboard,
        false,
    );
    let pin_screen_btn = create_row(
        &general_frame,
        "Pin to the Screen:",
        &config.shortcut_pin_to_screen,
        true,
    );
    let restore_file_btn = create_row(
        &general_frame,
        "Restore Recently Closed File:",
        &config.shortcut_restore_recently_closed,
        false,
    );
    let toggle_overlays_btn = create_row(
        &general_frame,
        "Hide/Show Overlays:",
        &config.shortcut_toggle_overlays,
        true,
    );
    section.append(&general_frame);

    // --- Screenshots Section ---
    create_header(&section, "Screenshots", "camera-photo-symbolic");
    let screenshots_frame = build_frame();
    let capture_area_btn = create_row(
        &screenshots_frame,
        "Capture Area:",
        &config.shortcut_capture_area,
        false,
    );
    let capture_crosshair_btn = create_row(
        &screenshots_frame,
        "Crosshair Capture:",
        &config.shortcut_capture_crosshair,
        false,
    );
    let capture_prev_btn = create_row(
        &screenshots_frame,
        "Capture Previous Area:",
        &config.shortcut_capture_previous_area,
        true,
    );
    let capture_fullscreen_btn = create_row(
        &screenshots_frame,
        "Capture Full Screen:",
        &config.shortcut_capture_fullscreen,
        false,
    );
    let capture_window_btn = create_row(
        &screenshots_frame,
        "Capture Window:",
        &config.shortcut_capture_window,
        true,
    );
    section.append(&screenshots_frame);

    // Bottom buttons
    let bottom_box = GtkBox::new(Orientation::Horizontal, 0);
    bottom_box.set_margin_start(32);
    bottom_box.set_margin_end(32);
    bottom_box.set_margin_top(40);
    bottom_box.set_margin_bottom(20);

    let system_defaults_btn = Button::with_label("Use System Default Shortcuts...");
    system_defaults_btn.add_css_class("secondary-settings-button");
    system_defaults_btn.set_halign(Align::Start);

    let restore_defaults_btn = Button::with_label("Restore Defaults");
    restore_defaults_btn.add_css_class("secondary-settings-button");
    restore_defaults_btn.set_halign(Align::End);
    restore_defaults_btn.set_hexpand(true);

    bottom_box.append(&system_defaults_btn);
    bottom_box.append(&restore_defaults_btn);
    section.append(&bottom_box);

    ShortcutSettingsWidgets {
        section,
        toggle_icons_btn,
        open_file_btn,
        open_clipboard_btn,
        pin_screen_btn,
        restore_file_btn,
        toggle_overlays_btn,
        capture_area_btn,
        capture_crosshair_btn,
        capture_prev_btn,
        capture_fullscreen_btn,
        capture_window_btn,
    }
}
