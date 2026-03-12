use crate::config::AppConfig;
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, CheckButton, Entry, Grid, Label, Orientation,
};

pub struct StorageSettingsWidgets {
    pub wrapper: GtkBox,
    pub export_location_entry: Entry,
    pub export_location_browse: Button,
    pub hide_desktop_icons_check: CheckButton,
}

pub fn build_storage_section(config: &AppConfig) -> StorageSettingsWidgets {
    let secondary_wrapper = GtkBox::new(Orientation::Vertical, 0);
    secondary_wrapper.set_halign(Align::Center);
    secondary_wrapper.set_size_request(450, -1);

    let secondary_grid = Grid::new();
    secondary_grid.set_halign(Align::Center);
    secondary_grid.set_row_spacing(8);
    secondary_grid.set_column_spacing(10);

    let export_title = Label::new(Some("Export location:"));
    export_title.add_css_class("settings-group-title");
    export_title.set_size_request(165, -1);
    export_title.set_xalign(1.0);
    let export_location_spacer = GtkBox::new(Orientation::Horizontal, 0);
    export_location_spacer.set_size_request(28, -1);
    export_location_spacer.set_halign(Align::Start);
    let export_location_entry = Entry::new();
    export_location_entry.set_hexpand(true);
    export_location_entry.set_width_chars(28);
    export_location_entry.set_placeholder_text(Some("Choose a folder"));
    export_location_entry.set_text(&config.export_location);
    let export_location_browse = Button::with_label("Browse");
    let export_location_row = GtkBox::new(Orientation::Horizontal, 8);
    export_location_row.set_halign(Align::Start);
    export_location_row.append(&export_location_entry);
    export_location_row.append(&export_location_browse);
    secondary_grid.attach(&export_title, 0, 0, 1, 1);
    secondary_grid.attach(&export_location_row, 1, 0, 1, 1);

    let desktop_icons_title = Label::new(Some("Desktop icons:"));
    desktop_icons_title.add_css_class("settings-group-title");
    desktop_icons_title.set_size_request(165, -1);
    desktop_icons_title.set_xalign(1.0);
    let hide_desktop_icons_check = CheckButton::new();
    hide_desktop_icons_check.set_active(config.hide_desktop_icons_while_capturing);
    let desktop_icons_option = Label::new(Some("Hide while capturing"));
    desktop_icons_option.set_xalign(0.0);
    let desktop_icons_row = GtkBox::new(Orientation::Horizontal, 8);
    desktop_icons_row.set_halign(Align::Start);
    desktop_icons_row.append(&hide_desktop_icons_check);
    desktop_icons_row.append(&desktop_icons_option);
    secondary_grid.attach(&desktop_icons_title, 0, 1, 1, 1);
    secondary_grid.attach(&desktop_icons_row, 1, 1, 1, 1);

    secondary_wrapper.append(&secondary_grid);

    StorageSettingsWidgets {
        wrapper: secondary_wrapper,
        export_location_entry,
        export_location_browse,
        hide_desktop_icons_check,
    }
}
