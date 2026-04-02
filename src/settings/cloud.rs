use crate::AppConfig;
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, CheckButton, ComboBoxText, Label, Orientation,
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

    // --- Profile Group ---
    let profile_title = Label::new(Some("Account"));
    profile_title.add_css_class("settings-group-title");
    profile_title.set_xalign(0.0);
    profile_title.set_halign(Align::Start);
    profile_title.set_margin_bottom(8);
    section.append(&profile_title);

    let profile_frame = build_frame();

    let avatar_lbl = Label::new(Some("PM"));
    avatar_lbl.add_css_class("cloud-avatar");
    avatar_lbl.set_size_request(48, 48);

    let profile_info_box = GtkBox::new(Orientation::Horizontal, 16);
    profile_info_box.set_hexpand(true);
    let info_text_vbox = GtkBox::new(Orientation::Vertical, 2);
    info_text_vbox.set_valign(Align::Center);
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

    let profile_btns_hbox = GtkBox::new(Orientation::Horizontal, 6);
    let manage_acc_btn = Button::with_label("Manage Account");
    manage_acc_btn.add_css_class("secondary-settings-button");
    let sign_out_btn = Button::with_label("Sign Out");
    sign_out_btn.add_css_class("secondary-settings-button");
    profile_btns_hbox.append(&manage_acc_btn);
    profile_btns_hbox.append(&sign_out_btn);
    profile_btns_hbox.set_valign(Align::Center);

    let profile_row_hbox = GtkBox::new(Orientation::Horizontal, 12);
    profile_row_hbox.append(&profile_info_box);
    profile_row_hbox.append(&profile_btns_hbox);
    profile_frame.append(&build_row!(&profile_row_hbox, false));

    // Plan row
    let plan_info_vbox = GtkBox::new(Orientation::Vertical, 2);
    plan_info_vbox.set_hexpand(true);
    let plan_title = Label::new(Some(if config.cloud_pro_plan {
        "Pro plan"
    } else {
        "Free plan"
    }));
    plan_title.add_css_class("shortcuts-header-title");
    plan_title.set_xalign(0.0);
    let plan_sub = Label::new(Some("Unlimited storage"));
    plan_sub.add_css_class("settings-sub-option");
    plan_sub.set_xalign(0.0);
    plan_info_vbox.append(&plan_title);
    plan_info_vbox.append(&plan_sub);

    let manage_plan_btn = Button::with_label("Manage Plan");
    manage_plan_btn.add_css_class("secondary-settings-button");
    manage_plan_btn.set_valign(Align::Center);

    let plan_row_hbox = GtkBox::new(Orientation::Horizontal, 12);
    plan_row_hbox.append(&plan_info_vbox);
    plan_row_hbox.append(&manage_plan_btn);
    profile_frame.append(&build_row!(&plan_row_hbox, true));

    section.append(&profile_frame);

    // --- Settings Group ---
    let settings_title = Label::new(Some("Cloud Settings"));
    settings_title.add_css_class("settings-group-title");
    settings_title.set_xalign(0.0);
    settings_title.set_halign(Align::Start);
    settings_title.set_margin_bottom(8);
    section.append(&settings_title);

    let settings_frame = build_frame();

    // Quality
    let cloud_quality_input = ComboBoxText::new();
    cloud_quality_input.add_css_class("settings-select");
    cloud_quality_input.append(Some("Optimized for sharing"), "Optimized for sharing");
    cloud_quality_input.append(Some("Full quality"), "Full quality");
    cloud_quality_input.set_active_id(Some(&config.cloud_screenshot_quality));

    let quality_hbox = GtkBox::new(Orientation::Horizontal, 12);
    quality_hbox.set_hexpand(true);
    let quality_vbox = GtkBox::new(Orientation::Vertical, 4);
    quality_vbox.set_hexpand(true);
    let quality_label = Label::new(Some("Screenshot quality"));
    quality_label.set_xalign(0.0);
    let quality_hint = Label::new(Some("The \"Optimized for sharing\" option offers perfect balance between quality and loading time."));
    quality_hint.add_css_class("settings-sub-option-hint");
    quality_hint.set_xalign(0.0);
    quality_hint.set_wrap(true);
    quality_vbox.append(&quality_label);
    quality_vbox.append(&quality_hint);
    quality_hbox.append(&quality_vbox);
    quality_hbox.append(&cloud_quality_input);
    settings_frame.append(&build_row!(&quality_hbox, false));

    // Clipboard
    let cloud_clipboard_input = ComboBoxText::new();
    cloud_clipboard_input.add_css_class("settings-select");
    cloud_clipboard_input.append(Some("CleanShot Cloud link"), "CleanShot Cloud link");
    cloud_clipboard_input.append(Some("Direct image link"), "Direct image link");
    cloud_clipboard_input.set_active_id(Some(&config.cloud_copy_to_clipboard));
    let clipboard_hbox = GtkBox::new(Orientation::Horizontal, 12);
    clipboard_hbox.set_hexpand(true);
    let clipboard_label = Label::new(Some("Copy to clipboard after upload"));
    clipboard_label.set_xalign(0.0);
    clipboard_label.set_hexpand(true);
    clipboard_hbox.append(&clipboard_label);
    clipboard_hbox.append(&cloud_clipboard_input);
    settings_frame.append(&build_row!(&clipboard_hbox, true));

    // Menu Bar
    let cloud_show_recent_check = CheckButton::new();
    cloud_show_recent_check.set_active(config.cloud_show_recently_uploaded);
    let recent_hbox = GtkBox::new(Orientation::Horizontal, 12);
    recent_hbox.set_hexpand(true);
    let menu_bar_label = Label::new(Some("Show recently uploaded media in tray"));
    menu_bar_label.set_xalign(0.0);
    menu_bar_label.set_hexpand(true);
    recent_hbox.append(&menu_bar_label);
    recent_hbox.append(&cloud_show_recent_check);
    settings_frame.append(&build_row!(&recent_hbox, false));

    // Tags
    let cloud_ask_tags_check = CheckButton::new();
    cloud_ask_tags_check.set_active(config.cloud_ask_name_tags);
    let tags_hbox = GtkBox::new(Orientation::Horizontal, 12);
    tags_hbox.set_hexpand(true);
    let tags_label = Label::new(Some("Ask for name and tags every upload"));
    tags_label.set_xalign(0.0);
    tags_label.set_hexpand(true);
    tags_hbox.append(&tags_label);
    tags_hbox.append(&cloud_ask_tags_check);
    settings_frame.append(&build_row!(&tags_hbox, true));

    section.append(&settings_frame);

    CloudSettingsWidgets {
        section,
        cloud_quality_input,
        cloud_clipboard_input,
        cloud_show_recent_check,
        cloud_ask_tags_check,
    }
}
