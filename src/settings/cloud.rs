use crate::config::AppConfig;
use gtk4::prelude::*;
use gtk4::{glib, Align, Box as GtkBox, Button, CheckButton, Entry, Label, Orientation};
use std::process::Command;

pub struct CloudSettingsWidgets {
    pub section: GtkBox,
    pub apexshot_check: CheckButton,
    pub xbackbone_check: CheckButton,
    pub auto_upload_check: CheckButton,
    pub xb_url_entry: Entry,
    pub xb_token_entry: Entry,
}

pub fn build_cloud_section(config: &AppConfig) -> CloudSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.set_halign(Align::Fill);
    section.set_valign(Align::Start);
    section.set_hexpand(true);
    section.set_margin_top(20);
    section.set_margin_bottom(8);

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

    // --- Destination group ---
    let dest_title = Label::new(Some("Upload destination"));
    dest_title.add_css_class("settings-group-title");
    dest_title.set_xalign(0.0);
    dest_title.set_halign(Align::Start);
    dest_title.set_margin_bottom(8);
    section.append(&dest_title);

    let dest_frame = build_frame();

    let apexshot_check = CheckButton::with_label("ApexShot Cloud");
    let xbackbone_check = CheckButton::with_label("XBackBone (self-hosted)");
    let is_xb = config.cloud_destination == "xbackbone";
    apexshot_check.set_active(!is_xb);
    xbackbone_check.set_active(is_xb);

    let dest_hbox = GtkBox::new(Orientation::Horizontal, 12);
    dest_hbox.set_hexpand(true);
    let dest_label = Label::new(Some("Upload with"));
    dest_label.set_xalign(0.0);
    dest_label.set_hexpand(true);
    let dest_actions = GtkBox::new(Orientation::Vertical, 8);
    dest_actions.append(&apexshot_check);
    dest_actions.append(&xbackbone_check);
    dest_hbox.append(&dest_label);
    dest_hbox.append(&dest_actions);
    dest_frame.append(&build_row!(&dest_hbox, false));

    let auto_upload_check = CheckButton::new();
    auto_upload_check.set_active(config.cloud_auto_upload_after_capture);
    let auto_hbox = GtkBox::new(Orientation::Horizontal, 12);
    auto_hbox.set_hexpand(true);
    let auto_label = Label::new(Some("Upload after capture"));
    auto_label.set_xalign(0.0);
    auto_label.set_hexpand(true);
    let auto_help = Label::new(Some(
        "When the selected destination is configured, upload each saved screenshot automatically.",
    ));
    auto_help.add_css_class("dim-label");
    auto_help.set_wrap(true);
    auto_help.set_xalign(0.0);
    auto_help.set_hexpand(true);
    let auto_col = GtkBox::new(Orientation::Vertical, 4);
    auto_col.set_hexpand(true);
    auto_col.append(&auto_label);
    auto_col.append(&auto_help);
    auto_hbox.append(&auto_col);
    auto_hbox.append(&auto_upload_check);
    dest_frame.append(&build_row!(&auto_hbox, true));

    section.append(&dest_frame);

    // --- ApexShot Cloud panel ---
    let apexshot_panel = build_apexshot_panel(config);
    section.append(&apexshot_panel.container);

    // --- XBackBone panel ---
    let xb_panel = build_xbackbone_panel(config);
    section.append(&xb_panel.container);

    // Keep the two checkboxes mutually exclusive without hiding either setup panel.
    {
        let xb = xbackbone_check.clone();
        apexshot_check.connect_toggled(move |check| {
            if check.is_active() {
                xb.set_active(false);
            } else if !xb.is_active() {
                check.set_active(true);
            }
        });
    }
    {
        let apex = apexshot_check.clone();
        xbackbone_check.connect_toggled(move |check| {
            if check.is_active() {
                apex.set_active(false);
            } else if !apex.is_active() {
                check.set_active(true);
            }
        });
    }

    CloudSettingsWidgets {
        section,
        apexshot_check,
        xbackbone_check,
        auto_upload_check,
        xb_url_entry: xb_panel.url_entry,
        xb_token_entry: xb_panel.token_entry,
    }
}

struct ApexShotPanel {
    container: GtkBox,
}

fn build_apexshot_panel(config: &AppConfig) -> ApexShotPanel {
    let container = GtkBox::new(Orientation::Vertical, 0);
    container.set_hexpand(true);

    let title = Label::new(Some("ApexShot Cloud account"));
    title.add_css_class("settings-group-title");
    title.set_xalign(0.0);
    title.set_halign(Align::Start);
    title.set_margin_bottom(8);
    container.append(&title);

    let frame = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.set_margin_start(4);
    frame.set_margin_end(4);

    let is_connected = !config.cloud_user_email.is_empty();

    let status_row = GtkBox::new(Orientation::Horizontal, 12);
    status_row.set_hexpand(true);
    status_row.add_css_class("settings-table-row");

    let info_box = GtkBox::new(Orientation::Vertical, 2);
    info_box.set_hexpand(true);

    let avatar = Label::new(Some(&initial(&config.cloud_user_email)));
    avatar.add_css_class("cloud-avatar");
    avatar.set_size_request(40, 40);
    avatar.set_halign(Align::Center);
    avatar.set_valign(Align::Center);
    avatar.set_visible(is_connected);

    let status_label = Label::new(Some(if is_connected {
        "Connected"
    } else {
        "Not connected"
    }));
    status_label.add_css_class("cloud-user-name");
    status_label.set_xalign(0.0);
    status_label.set_halign(Align::Start);

    let email_label = Label::new(Some(if is_connected {
        config.cloud_user_email.as_str()
    } else {
        "Run `apexshot login` in a terminal, or click Connect below."
    }));
    email_label.add_css_class("cloud-user-email");
    email_label.set_xalign(0.0);
    email_label.set_halign(Align::Start);

    info_box.append(&status_label);
    info_box.append(&email_label);

    status_row.append(&avatar);
    status_row.append(&info_box);

    let connect_btn = Button::with_label("Connect account");
    connect_btn.add_css_class("settings-primary-btn");
    connect_btn.set_visible(!is_connected);
    let logout_btn = Button::with_label("Logout");
    logout_btn.set_visible(is_connected);
    status_row.append(&connect_btn);
    status_row.append(&logout_btn);

    frame.append(&status_row);
    container.append(&frame);

    // Connect account → spawn a terminal running `apexshot login`.
    {
        connect_btn.connect_clicked(move |_| {
            spawn_apexshot_login();
        });
    }

    // Logout → run auth::logout() on a thread, then update the UI on the main
    // thread. GTK widgets are not Send, so the blocking call runs on a worker
    // thread and the result is delivered through an mpsc channel to a
    // main-thread idle callback that owns the widget clones.
    {
        let avatar_c = avatar.clone();
        let status_c = status_label.clone();
        let email_c = email_label.clone();
        let connect_c = connect_btn.clone();
        let logout_c = logout_btn.clone();
        logout_btn.connect_clicked(move |_| {
            logout_c.set_sensitive(false);
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = sender.send(crate::cloud::auth::logout());
            });
            let avatar_i = avatar_c.clone();
            let status_i = status_c.clone();
            let email_i = email_c.clone();
            let connect_i = connect_c.clone();
            let logout_i = logout_c.clone();
            glib::source::idle_add_local(move || match receiver.try_recv() {
                Ok(result) => {
                    if result.is_ok() {
                        set_apexshot_status(
                            &avatar_i, &status_i, &email_i, &connect_i, &logout_i, "",
                        );
                    }
                    logout_i.set_sensitive(true);
                    glib::ControlFlow::Break
                }
                Err(_) => glib::ControlFlow::Continue,
            });
        });
    }

    // Refresh while settings stays open, so a terminal login updates without reopening Settings.
    {
        let avatar_c = avatar.clone();
        let status_c = status_label.clone();
        let email_c = email_label.clone();
        let connect_c = connect_btn.clone();
        let logout_c = logout_btn.clone();
        glib::timeout_add_seconds_local(2, move || {
            let email = crate::config::load_config().cloud_user_email;
            set_apexshot_status(
                &avatar_c, &status_c, &email_c, &connect_c, &logout_c, &email,
            );
            glib::ControlFlow::Continue
        });
    }

    ApexShotPanel { container }
}

fn set_apexshot_status(
    avatar: &Label,
    status_label: &Label,
    email_label: &Label,
    connect_btn: &Button,
    logout_btn: &Button,
    email: &str,
) {
    let is_connected = !email.is_empty();
    avatar.set_text(&initial(email));
    avatar.set_visible(is_connected);
    status_label.set_text(if is_connected {
        "Connected"
    } else {
        "Not connected"
    });
    email_label.set_text(if is_connected {
        email
    } else {
        "Run `apexshot login` in a terminal, or click Connect below."
    });
    connect_btn.set_visible(!is_connected);
    logout_btn.set_visible(is_connected);
}

struct XBackbonePanel {
    container: GtkBox,
    url_entry: Entry,
    token_entry: Entry,
}

fn build_xbackbone_panel(config: &AppConfig) -> XBackbonePanel {
    let container = GtkBox::new(Orientation::Vertical, 0);
    container.set_hexpand(true);

    let title = Label::new(Some("XBackBone instance"));
    title.add_css_class("settings-group-title");
    title.set_xalign(0.0);
    title.set_halign(Align::Start);
    title.set_margin_bottom(8);
    container.append(&title);

    let frame = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.set_margin_start(4);
    frame.set_margin_end(4);

    // Instance URL row.
    let url_entry = Entry::new();
    url_entry.set_hexpand(true);
    url_entry.set_width_chars(32);
    url_entry.set_placeholder_text(Some("https://files.example.com"));
    url_entry.set_text(&config.xbackbone_url);

    let url_hbox = GtkBox::new(Orientation::Horizontal, 12);
    url_hbox.set_hexpand(true);
    let url_label = Label::new(Some("Instance URL"));
    url_label.set_xalign(0.0);
    url_label.set_hexpand(true);
    url_hbox.append(&url_label);
    url_hbox.append(&url_entry);
    frame.append(&build_table_row(&url_hbox));

    // API token row (masked by default) with a reveal toggle.
    let token_entry = Entry::new();
    token_entry.set_hexpand(true);
    token_entry.set_width_chars(32);
    token_entry.set_placeholder_text(Some("Paste your XBackBone API token"));
    token_entry.set_visibility(false);
    token_entry.set_text(&config.xbackbone_api_token);
    token_entry.set_input_purpose(gtk4::InputPurpose::Password);

    let show_check = CheckButton::with_label("Show");
    {
        let token_clone = token_entry.clone();
        show_check.connect_toggled(move |check| {
            token_clone.set_visibility(check.is_active());
        });
    }

    let token_controls = GtkBox::new(Orientation::Horizontal, 8);
    token_controls.append(&token_entry);
    token_controls.append(&show_check);

    let token_hbox = GtkBox::new(Orientation::Horizontal, 12);
    token_hbox.set_hexpand(true);
    let token_label = Label::new(Some("API token"));
    token_label.set_xalign(0.0);
    token_label.set_hexpand(true);
    token_hbox.append(&token_label);
    token_hbox.append(&token_controls);
    frame.append(&build_table_row(&token_hbox));

    // Helper text + docs link.
    let hint = Label::new(Some(
        "Generate a token in your XBackBone instance \u{2192} Profile \u{2192} Tokens, with the resource:upload ability.",
    ));
    hint.add_css_class("settings-sub-option-hint");
    hint.set_xalign(0.0);
    hint.set_halign(Align::Start);
    hint.set_wrap(true);
    hint.set_margin_top(8);
    frame.append(&build_table_row(&hint));

    // Test connection row.
    let test_btn = Button::with_label("Test connection");
    let status_label = Label::new(Some(""));
    status_label.set_xalign(0.0);
    status_label.set_halign(Align::Start);
    status_label.add_css_class("settings-sub-option-hint");

    let test_hbox = GtkBox::new(Orientation::Horizontal, 12);
    test_hbox.set_hexpand(true);
    let test_label = Label::new(Some("Verify setup"));
    test_label.set_xalign(0.0);
    test_label.set_hexpand(true);
    test_hbox.append(&test_label);
    let test_actions = GtkBox::new(Orientation::Horizontal, 8);
    test_actions.append(&test_btn);
    test_hbox.append(&test_actions);
    frame.append(&build_table_row(&test_hbox));

    let status_hbox = GtkBox::new(Orientation::Horizontal, 0);
    status_hbox.set_hexpand(true);
    status_hbox.append(&status_label);
    frame.append(&build_table_row(&status_hbox));

    // Open API docs link.
    let docs_btn = Button::with_label("Open XBackBone API docs");
    {
        docs_btn.connect_clicked(|_| {
            std::thread::spawn(move || {
                let _ = Command::new("xdg-open")
                    .arg("https://xbackbone.app/clients/api")
                    .spawn();
            });
        });
    }
    let docs_hbox = GtkBox::new(Orientation::Horizontal, 12);
    docs_hbox.set_hexpand(true);
    let docs_label = Label::new(Some("Documentation"));
    docs_label.set_xalign(0.0);
    docs_label.set_hexpand(true);
    docs_hbox.append(&docs_label);
    docs_hbox.append(&docs_btn);
    frame.append(&build_table_row(&docs_hbox));

    container.append(&frame);

    // Wire the Test connection button: build a config snapshot from the entry
    // values (so unsaved edits are tested), run test_connection on a worker
    // thread, then surface the result on the status label from the main thread
    // via an mpsc channel (widgets are not Send).
    {
        let url_entry_c = url_entry.clone();
        let token_entry_c = token_entry.clone();
        let status_c = status_label.clone();
        let test_btn_c = test_btn.clone();
        test_btn.connect_clicked(move |_| {
            test_btn_c.set_sensitive(false);
            status_c.set_text("Testing\u{2026}");

            let mut snapshot = crate::config::load_config();
            snapshot.xbackbone_url = url_entry_c.text().to_string();
            snapshot.xbackbone_api_token = token_entry_c.text().to_string();

            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = crate::cloud::xbackbone::test_connection(&snapshot);
                let _ = sender.send(result);
            });
            let status_i = status_c.clone();
            let btn_i = test_btn_c.clone();
            glib::source::idle_add_local(move || match receiver.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(()) => {
                            status_i.set_text("Connected \u{2014} token is valid.");
                            status_i.remove_css_class("settings-toast-error");
                        }
                        Err(msg) => {
                            status_i.set_text(&msg);
                            status_i.add_css_class("settings-toast-error");
                        }
                    }
                    btn_i.set_sensitive(true);
                    glib::ControlFlow::Break
                }
                Err(_) => glib::ControlFlow::Continue,
            });
        });
    }

    XBackbonePanel {
        container,
        url_entry,
        token_entry,
    }
}

fn build_table_row(content: &impl IsA<gtk4::Widget>) -> gtk4::Box {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    row.add_css_class("settings-table-row");
    row.set_hexpand(true);
    row.append(content);
    row
}

fn initial(email: &str) -> String {
    let ch = email
        .trim()
        .chars()
        .next()
        .filter(|c| c.is_ascii_alphabetic())
        .unwrap_or('?');
    ch.to_ascii_uppercase().to_string()
}

fn spawn_apexshot_login() {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));
    // Try a few terminal launchers; the first one that spawns wins.
    let attempts: Vec<Vec<&str>> = vec![
        vec!["x-terminal-emulator", "-e"],
        vec!["gnome-terminal", "--"],
        vec!["konsole", "-e"],
        vec!["xterm", "-e"],
        vec!["kgx", "-e"],
        vec!["alacritty", "-e"],
    ];
    for mut attempt in attempts {
        attempt.push(exe.to_str().unwrap_or("apexshot"));
        attempt.push("login");
        if Command::new(attempt[0]).args(&attempt[1..]).spawn().is_ok() {
            return;
        }
    }
    crate::utils::notify::desktop_notification(
        "Could not open a terminal",
        "Run `apexshot login` manually in a terminal to connect your ApexShot Cloud account.",
    );
}
