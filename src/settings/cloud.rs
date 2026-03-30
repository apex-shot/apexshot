use crate::AppConfig;
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, CheckButton, ComboBoxText, Grid, Label, Orientation,
    Separator,
};

pub struct CloudSettingsWidgets {
    pub section: GtkBox,
    pub cloud_quality_input: ComboBoxText,
    pub cloud_clipboard_input: ComboBoxText,
    pub cloud_show_recent_check: CheckButton,
    pub cloud_ask_tags_check: CheckButton,
}

pub fn build_cloud_section(config: &AppConfig) -> CloudSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.set_hexpand(true);

    let grid = Grid::new();
    grid.set_column_spacing(24);
    grid.set_row_spacing(24);
    grid.set_margin_top(10);
    grid.set_hexpand(true);

    // Spacers for horizontal center alignment
    let l_spacer = GtkBox::new(Orientation::Horizontal, 0);
    l_spacer.set_hexpand(true);
    let r_spacer = GtkBox::new(Orientation::Horizontal, 0);
    r_spacer.set_hexpand(true);
    grid.attach(&l_spacer, 0, 0, 1, 1);
    grid.attach(&r_spacer, 3, 0, 1, 1);

    let mut row = 0;
    let label_group = gtk4::SizeGroup::new(gtk4::SizeGroupMode::Horizontal);

    // --- 1. PROFILE ROW ---
    let avatar_lbl = Label::new(Some("PM"));
    avatar_lbl.add_css_class("cloud-avatar");
    avatar_lbl.set_size_request(64, 64);
    avatar_lbl.set_halign(Align::End);
    avatar_lbl.set_valign(Align::Center);

    let profile_info_box = GtkBox::new(Orientation::Horizontal, 16);
    let info_text_vbox = GtkBox::new(Orientation::Vertical, 2);
    let name_lbl = Label::new(Some(&config.cloud_user_name));
    name_lbl.add_css_class("cloud-user-name");
    name_lbl.set_xalign(0.0);
    let email_lbl = Label::new(Some(&config.cloud_user_email));
    email_lbl.add_css_class("cloud-user-email");
    email_lbl.set_xalign(0.0);
    info_text_vbox.append(&name_lbl);
    info_text_vbox.append(&email_lbl);
    profile_info_box.append(&avatar_lbl);
    profile_info_box.append(&info_text_vbox);
    profile_info_box.set_halign(Align::Start);
    label_group.add_widget(&profile_info_box);

    let profile_btns_vbox = GtkBox::new(Orientation::Vertical, 6);
    let manage_acc_btn = Button::with_label("Manage Account");
    manage_acc_btn.add_css_class("secondary-settings-button");
    let sign_out_btn = Button::with_label("Sign Out");
    sign_out_btn.add_css_class("secondary-settings-button");
    profile_btns_vbox.append(&manage_acc_btn);
    profile_btns_vbox.append(&sign_out_btn);
    profile_btns_vbox.set_halign(Align::Start);

    grid.attach(&profile_info_box, 1, row, 1, 1);
    grid.attach(&profile_btns_vbox, 2, row, 1, 1);

    row += 1;
    let sep1 = Separator::new(Orientation::Horizontal);
    grid.attach(&sep1, 0, row, 4, 1);

    row += 1;
    // --- 2. PLAN ROW ---
    let plan_info_vbox = GtkBox::new(Orientation::Vertical, 2);
    let plan_title = Label::new(Some(if config.cloud_pro_plan {
        "Pro plan"
    } else {
        "Free plan"
    }));
    plan_title.add_css_class("shortcuts-header-title"); // Bold
    plan_title.set_xalign(1.0);
    let plan_sub = Label::new(Some("Unlimited storage"));
    plan_sub.add_css_class("settings-sub-option");
    plan_sub.set_xalign(1.0);
    plan_info_vbox.append(&plan_title);
    plan_info_vbox.append(&plan_sub);
    plan_info_vbox.set_halign(Align::End);
    label_group.add_widget(&plan_info_vbox);

    let manage_plan_btn = Button::with_label("Manage Plan");
    manage_plan_btn.add_css_class("secondary-settings-button");
    manage_plan_btn.set_halign(Align::Start);
    manage_plan_btn.set_valign(Align::Center);

    grid.attach(&plan_info_vbox, 1, row, 1, 1);
    grid.attach(&manage_plan_btn, 2, row, 1, 1);

    row += 1;
    let sep2 = Separator::new(Orientation::Horizontal);
    grid.attach(&sep2, 0, row, 4, 1);

    row += 1;
    // --- 3. SETTINGS ROWS ---

    // Quality
    let quality_label = Label::new(Some("Screenshot quality:"));
    quality_label.add_css_class("settings-group-title");
    quality_label.set_xalign(1.0);
    quality_label.set_halign(Align::End);
    label_group.add_widget(&quality_label);

    let quality_vbox = GtkBox::new(Orientation::Vertical, 6);
    quality_vbox.set_halign(Align::Start);
    let cloud_quality_input = ComboBoxText::new();
    cloud_quality_input.set_halign(Align::Start);
    cloud_quality_input.append(Some("Optimized for sharing"), "Optimized for sharing");
    cloud_quality_input.append(Some("Full quality"), "Full quality");
    cloud_quality_input.set_active_id(Some(&config.cloud_screenshot_quality));

    let quality_hint = Label::new(Some("The \"Optimized for sharing\" option offers perfect\nbalance between quality and loading time."));
    quality_hint.add_css_class("settings-sub-option-hint");
    quality_hint.set_xalign(0.0);
    quality_vbox.append(&cloud_quality_input);
    quality_vbox.append(&quality_hint);

    grid.attach(&quality_label, 1, row, 1, 1);
    grid.attach(&quality_vbox, 2, row, 1, 1);

    row += 1;
    // Clipboard
    let clipboard_label = Label::new(Some("Copy to clipboard:"));
    clipboard_label.add_css_class("settings-group-title");
    clipboard_label.set_xalign(1.0);
    clipboard_label.set_halign(Align::End);
    label_group.add_widget(&clipboard_label);

    let cloud_clipboard_input = ComboBoxText::new();
    cloud_clipboard_input.set_halign(Align::Start);
    cloud_clipboard_input.append(Some("CleanShot Cloud link"), "CleanShot Cloud link");
    cloud_clipboard_input.append(Some("Direct image link"), "Direct image link");
    cloud_clipboard_input.set_active_id(Some(&config.cloud_copy_to_clipboard));

    grid.attach(&clipboard_label, 1, row, 1, 1);
    grid.attach(&cloud_clipboard_input, 2, row, 1, 1);

    row += 1;
    // Menu Bar
    let menu_bar_label = Label::new(Some("Menu Bar:"));
    menu_bar_label.add_css_class("settings-group-title");
    menu_bar_label.set_xalign(1.0);
    menu_bar_label.set_halign(Align::End);
    label_group.add_widget(&menu_bar_label);

    let cloud_show_recent_check = CheckButton::with_label("Show recently uploaded media");
    cloud_show_recent_check.set_halign(Align::Start);
    cloud_show_recent_check.set_active(config.cloud_show_recently_uploaded);

    grid.attach(&menu_bar_label, 1, row, 1, 1);
    grid.attach(&cloud_show_recent_check, 2, row, 1, 1);

    row += 1;
    // Tags
    let tags_label = Label::new(Some("Name & Tags:"));
    tags_label.add_css_class("settings-group-title");
    tags_label.set_xalign(1.0);
    tags_label.set_halign(Align::End);
    label_group.add_widget(&tags_label);

    let cloud_ask_tags_check = CheckButton::with_label("Ask for name and tags every upload");
    cloud_ask_tags_check.set_halign(Align::Start);
    cloud_ask_tags_check.set_active(config.cloud_ask_name_tags);

    grid.attach(&tags_label, 1, row, 1, 1);
    grid.attach(&cloud_ask_tags_check, 2, row, 1, 1);

    section.append(&grid);

    CloudSettingsWidgets {
        section,
        cloud_quality_input,
        cloud_clipboard_input,
        cloud_show_recent_check,
        cloud_ask_tags_check,
    }
}
