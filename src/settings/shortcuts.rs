use crate::config::AppConfig;
use gtk4::{prelude::*, Align, Box as GtkBox, Button, Grid, Image, Label, Orientation};

pub struct ShortcutSettingsWidgets {
    pub section: GtkBox,
    pub toggle_icons_btn: Button,
    pub open_file_btn: Button,
    pub open_clipboard_btn: Button,
    pub pin_screen_btn: Button,
    pub restore_file_btn: Button,
    pub toggle_overlays_btn: Button,
    pub capture_area_btn: Button,
    pub capture_prev_btn: Button,
    pub capture_fullscreen_btn: Button,
    pub capture_window_btn: Button,
}

pub fn build_shortcuts_section(config: &AppConfig) -> ShortcutSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.set_hexpand(true);

    let grid = Grid::new();
    grid.set_column_spacing(12);
    grid.set_row_spacing(0); // Rows will have padding/backgrounds

    // Spacers for horizontal centering
    let l_spacer = GtkBox::new(Orientation::Horizontal, 0);
    l_spacer.set_hexpand(true);
    let r_spacer = GtkBox::new(Orientation::Horizontal, 0);
    r_spacer.set_hexpand(true);
    grid.attach(&l_spacer, 0, 0, 1, 1);
    grid.attach(&r_spacer, 4, 0, 1, 1);

    let mut row = 0;

    let create_header = |grid: &Grid, label: &str, icon: &str, row: &mut i32| {
        let hbox = GtkBox::new(Orientation::Horizontal, 10);
        hbox.set_margin_top(24);
        hbox.set_margin_bottom(12);

        let img = Image::from_icon_name(icon);
        img.set_pixel_size(20);

        let lbl = Label::new(Some(label));
        lbl.add_css_class("shortcuts-header-title"); // Style like image

        hbox.append(&img);
        hbox.append(&lbl);

        grid.attach(&hbox, 1, *row, 3, 1);
        *row += 1;
    };

    let create_row =
        |grid: &Grid, label: &str, current_val: &str, row: &mut i32, is_zebra: bool| -> Button {
            if is_zebra {
                let bg = GtkBox::new(Orientation::Horizontal, 0);
                bg.add_css_class("shortcuts-row-zebra");
                bg.set_vexpand(false);
                grid.attach(&bg, 0, *row, 5, 1);
            }

            let lbl = Label::new(Some(label));
            lbl.set_xalign(0.0);
            lbl.set_halign(Align::Start);
            lbl.set_margin_start(16);
            lbl.set_margin_top(10);
            lbl.set_margin_bottom(10);
            lbl.add_css_class("shortcuts-label");

            let btn = Button::new();
            btn.add_css_class("shortcuts-record-btn");
            btn.set_label(if current_val.is_empty() {
                "Record shortcut"
            } else {
                current_val
            });
            btn.set_halign(Align::End);
            btn.set_margin_end(16);
            btn.set_margin_top(8);
            btn.set_margin_bottom(8);
            btn.set_size_request(200, -1);

            grid.attach(&lbl, 1, *row, 2, 1);
            grid.attach(&btn, 3, *row, 1, 1);

            *row += 1;
            btn
        };

    // --- General Section ---
    create_header(&grid, "General", "emblem-system-symbolic", &mut row);
    let toggle_icons_btn = create_row(
        &grid,
        "Toggle Desktop Icons:",
        &config.shortcut_toggle_desktop_icons,
        &mut row,
        false,
    );
    let open_file_btn = create_row(
        &grid,
        "Open File:",
        &config.shortcut_open_file,
        &mut row,
        true,
    );
    let open_clipboard_btn = create_row(
        &grid,
        "Open From Clipboard:",
        &config.shortcut_open_from_clipboard,
        &mut row,
        false,
    );
    let pin_screen_btn = create_row(
        &grid,
        "Pin to the Screen:",
        &config.shortcut_pin_to_screen,
        &mut row,
        true,
    );
    let restore_file_btn = create_row(
        &grid,
        "Restore Recently Closed File:",
        &config.shortcut_restore_recently_closed,
        &mut row,
        false,
    );
    let toggle_overlays_btn = create_row(
        &grid,
        "Hide/Show Overlays:",
        &config.shortcut_toggle_overlays,
        &mut row,
        true,
    );

    // --- Screenshots Section ---
    create_header(&grid, "Screenshots", "camera-photo-symbolic", &mut row);
    let capture_area_btn = create_row(
        &grid,
        "Capture Area:",
        &config.shortcut_capture_area,
        &mut row,
        false,
    );
    let capture_prev_btn = create_row(
        &grid,
        "Capture Previous Area:",
        &config.shortcut_capture_previous_area,
        &mut row,
        true,
    );
    let capture_fullscreen_btn = create_row(
        &grid,
        "Capture Full Screen:",
        &config.shortcut_capture_fullscreen,
        &mut row,
        false,
    );
    let capture_window_btn = create_row(
        &grid,
        "Capture Window:",
        &config.shortcut_capture_window,
        &mut row,
        true,
    );

    section.append(&grid);

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
        capture_prev_btn,
        capture_fullscreen_btn,
        capture_window_btn,
    }
}
