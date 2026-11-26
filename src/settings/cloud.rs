use crate::config::AppConfig;
use crate::i18n::t;
use gtk4::prelude::*;
use gtk4::{glib, Align, Box as GtkBox, Button, CheckButton, Entry, Label, Orientation};
#[cfg(not(target_os = "windows"))]
use std::cell::Cell;
#[cfg(not(target_os = "windows"))]
use std::rc::Rc;

#[cfg(not(target_os = "windows"))]
pub struct AutoUploadSync {
    last_persisted: Cell<bool>,
    syncing: Cell<bool>,
}

#[cfg(not(target_os = "windows"))]
impl AutoUploadSync {
    pub fn new(persisted: bool) -> Self {
        Self {
            last_persisted: Cell::new(persisted),
            syncing: Cell::new(false),
        }
    }

    pub fn is_syncing(&self) -> bool {
        self.syncing.get()
    }

    pub fn sync(&self, check: &CheckButton, persisted: bool) {
        if persisted == self.last_persisted.get() {
            return;
        }
        self.last_persisted.set(persisted);
        self.syncing.set(true);
        check.set_active(persisted);
        self.syncing.set(false);
    }

    pub fn note_saved(&self, persisted: bool) {
        self.last_persisted.set(persisted);
    }
}

pub struct CloudSettingsWidgets {
    pub section: GtkBox,
    #[cfg(target_os = "windows")]
    pub connect_btn: Button,
    pub apexshot_check: CheckButton,
    pub xbackbone_check: CheckButton,
    pub auto_upload_check: CheckButton,
    #[cfg(not(target_os = "windows"))]
    pub auto_upload_sync: Rc<AutoUploadSync>,
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
    let dest_title = Label::new(Some(&t("Upload destination")));
    dest_title.add_css_class("settings-group-title");
    dest_title.set_xalign(0.0);
    dest_title.set_halign(Align::Start);
    dest_title.set_margin_bottom(8);
    section.append(&dest_title);

    let dest_frame = build_frame();

    let apexshot_check = CheckButton::with_label(&t("ApexShot Cloud"));
    let xbackbone_check = CheckButton::with_label(&t("XBackBone (self-hosted)"));
    let is_xb = config.cloud_destination == "xbackbone";
    apexshot_check.set_active(!is_xb);
    xbackbone_check.set_active(is_xb);

    let dest_hbox = GtkBox::new(Orientation::Horizontal, 12);
    dest_hbox.set_hexpand(true);
    let dest_label = Label::new(Some(&t("Upload with")));
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
    #[cfg(not(target_os = "windows"))]
    let auto_upload_sync = Rc::new(AutoUploadSync::new(config.cloud_auto_upload_after_capture));

    let auto_hbox = GtkBox::new(Orientation::Horizontal, 12);
    auto_hbox.set_hexpand(true);
    let auto_label = Label::new(Some(&t("Upload after capture")));
    auto_label.set_xalign(0.0);
    auto_label.set_hexpand(true);
    // The Windows shell auto-uploads recordings too.
    let auto_help_text = if cfg!(target_os = "windows") {
        t("Uploads each saved screenshot and recording and copies the share link. Does nothing until you Connect.")
    } else {
        t("Uploads each saved screenshot and copies the share link. Does nothing until you Connect.")
    };
    let auto_help = Label::new(Some(&auto_help_text));
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

    #[cfg(not(target_os = "windows"))]
    install_auto_upload_observer(&section, &auto_upload_check, &auto_upload_sync);

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
        #[cfg(target_os = "windows")]
        connect_btn: apexshot_panel.connect_btn,
        apexshot_check,
        xbackbone_check,
        auto_upload_check,
        #[cfg(not(target_os = "windows"))]
        auto_upload_sync,
        xb_url_entry: xb_panel.url_entry,
        xb_token_entry: xb_panel.token_entry,
    }
}

struct ApexShotPanel {
    container: GtkBox,
    #[cfg(target_os = "windows")]
    connect_btn: Button,
}

fn build_apexshot_panel(config: &AppConfig) -> ApexShotPanel {
    let container = GtkBox::new(Orientation::Vertical, 0);
    container.set_hexpand(true);

    let title = Label::new(Some(&t("ApexShot Cloud account")));
    title.add_css_class("settings-group-title");
    title.set_xalign(0.0);
    title.set_halign(Align::Start);
    title.set_margin_bottom(8);
    container.append(&title);

    let frame = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.set_margin_start(4);
    frame.set_margin_end(4);

    // Session = access token. Email alone was a bug that showed "Connected"
    // for stale/stray account metadata without a real login.
    let is_connected = crate::config::is_cloud_logged_in(config);

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

    let connected = t("Connected");
    let not_connected = t("Not connected");
    let status_label = Label::new(Some(if is_connected {
        connected.as_str()
    } else {
        not_connected.as_str()
    }));
    status_label.add_css_class("cloud-user-name");
    status_label.set_xalign(0.0);
    status_label.set_halign(Align::Start);

    let signed_in = t("Signed in");
    let connect_hint = connect_hint();
    let email_label = Label::new(Some(if is_connected {
        if config.cloud_user_email.is_empty() {
            signed_in.as_str()
        } else {
            config.cloud_user_email.as_str()
        }
    } else {
        connect_hint.as_str()
    }));
    email_label.add_css_class("cloud-user-email");
    email_label.set_xalign(0.0);
    email_label.set_halign(Align::Start);

    info_box.append(&status_label);
    info_box.append(&email_label);

    status_row.append(&avatar);
    status_row.append(&info_box);

    let connect_btn = Button::with_label(&t("Connect account"));
    connect_btn.add_css_class("settings-primary-btn");
    connect_btn.set_valign(Align::Center);
    connect_btn.set_visible(!is_connected);
    let logout_btn = Button::with_label(&t("Logout"));
    logout_btn.set_valign(Align::Center);
    logout_btn.set_visible(is_connected);
    status_row.append(&connect_btn);
    status_row.append(&logout_btn);

    frame.append(&status_row);
    container.append(&frame);

    #[cfg(not(target_os = "windows"))]
    {
        connect_btn.connect_clicked(move |btn| {
            let parent = btn.root().and_downcast::<gtk4::Window>();
            crate::cloud::sign_in::show_connect(parent.as_ref(), || {});
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
                            &avatar_i,
                            &status_i,
                            &email_i,
                            &connect_i,
                            &logout_i,
                            &crate::config::load_config(),
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
            let config = crate::config::load_config();
            set_apexshot_status(
                &avatar_c, &status_c, &email_c, &connect_c, &logout_c, &config,
            );
            glib::ControlFlow::Continue
        });
    }

    ApexShotPanel {
        container,
        #[cfg(target_os = "windows")]
        connect_btn,
    }
}

fn set_apexshot_status(
    avatar: &Label,
    status_label: &Label,
    email_label: &Label,
    connect_btn: &Button,
    logout_btn: &Button,
    config: &AppConfig,
) {
    let is_connected = crate::config::is_cloud_logged_in(config);
    let email = config.cloud_user_email.trim();
    avatar.set_text(&initial(email));
    avatar.set_visible(is_connected);
    status_label.set_text(&if is_connected {
        t("Connected")
    } else {
        t("Not connected")
    });
    let detail = if !is_connected {
        connect_hint()
    } else if email.is_empty() {
        t("Signed in")
    } else {
        email.to_string()
    };
    email_label.set_text(&detail);
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

    let title = Label::new(Some(&t("XBackBone instance")));
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
    let url_label = Label::new(Some(&t("Instance URL")));
    url_label.set_xalign(0.0);
    url_label.set_hexpand(true);
    url_hbox.append(&url_label);
    url_hbox.append(&url_entry);
    frame.append(&build_table_row(&url_hbox));

    // API token row (masked by default) with a reveal toggle.
    let token_entry = Entry::new();
    token_entry.set_hexpand(true);
    token_entry.set_width_chars(32);
    token_entry.set_placeholder_text(Some(&t("Paste your XBackBone API token")));
    token_entry.set_visibility(false);
    token_entry.set_text(&config.xbackbone_api_token);
    token_entry.set_input_purpose(gtk4::InputPurpose::Password);

    let show_check = CheckButton::with_label(&t("Show"));
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
    let token_label = Label::new(Some(&t("API token")));
    token_label.set_xalign(0.0);
    token_label.set_hexpand(true);
    token_hbox.append(&token_label);
    token_hbox.append(&token_controls);
    frame.append(&build_table_row(&token_hbox));

    // Helper text + docs link.
    let hint = Label::new(Some(&t(
        "Generate a token in your XBackBone instance → Profile → Tokens, with the resource:upload ability.",
    )));
    hint.add_css_class("settings-sub-option-hint");
    hint.set_xalign(0.0);
    hint.set_halign(Align::Start);
    hint.set_wrap(true);
    hint.set_margin_top(8);
    frame.append(&build_table_row(&hint));

    // Test connection row.
    let test_btn = Button::with_label(&t("Test connection"));
    let status_label = Label::new(Some(&t("")));
    status_label.set_xalign(0.0);
    status_label.set_halign(Align::Start);
    status_label.add_css_class("settings-sub-option-hint");

    let test_hbox = GtkBox::new(Orientation::Horizontal, 12);
    test_hbox.set_hexpand(true);
    let test_label = Label::new(Some(&t("Verify setup")));
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
    // The row only exists once there is something to say: an empty status
    // line used to leave a blank band above Documentation.
    let status_row = build_table_row(&status_hbox);
    status_row.set_visible(false);
    {
        let status_row = status_row.clone();
        status_label.connect_notify_local(Some("label"), move |label, _| {
            status_row.set_visible(!label.text().trim().is_empty());
        });
    }
    frame.append(&status_row);

    // Open API docs link.
    let docs_btn = Button::with_label(&t("Open XBackBone API docs"));
    {
        docs_btn.connect_clicked(|_| {
            std::thread::spawn(move || {
                let _ = crate::utils::open::open_url("https://xbackbone.app/clients/api");
            });
        });
    }
    let docs_hbox = GtkBox::new(Orientation::Horizontal, 12);
    docs_hbox.set_hexpand(true);
    let docs_label = Label::new(Some(&t("Documentation")));
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

#[cfg(not(target_os = "windows"))]
fn install_auto_upload_observer(
    section: &GtkBox,
    check: &CheckButton,
    syncer: &Rc<AutoUploadSync>,
) {
    let check_weak = check.downgrade();
    let check_for_update = check_weak.clone();
    let syncer = Rc::clone(syncer);
    let update: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(check) = check_for_update.upgrade() {
            let persisted = crate::config::load_config().cloud_auto_upload_after_capture;
            syncer.sync(&check, persisted);
        }
    });

    let monitor = crate::config::config_path().and_then(|config_file| {
        let dir = gtk4::gio::File::for_path(config_file.parent()?);
        dir.monitor_directory(
            gtk4::gio::FileMonitorFlags::WATCH_MOVES,
            gtk4::gio::Cancellable::NONE,
        )
        .ok()
        .map(|monitor| (monitor, config_file))
    });

    match monitor {
        Some((monitor, config_file)) => {
            monitor.connect_changed({
                let update = Rc::clone(&update);
                move |_, file, other_file, _| {
                    let touched = file.path().as_deref() == Some(config_file.as_path())
                        || other_file
                            .and_then(|other| other.path())
                            .is_some_and(|path| path == config_file);
                    if touched {
                        update();
                    }
                }
            });
            section.connect_map(move |_| {
                let _monitor = &monitor;
                update();
            });
        }
        None => {
            glib::timeout_add_seconds_local(2, move || {
                if check_weak.upgrade().is_none() {
                    return glib::ControlFlow::Break;
                }
                update();
                glib::ControlFlow::Continue
            });
        }
    }
}

/// How a signed-out user connects: the Connect button opens the device-code
/// sign-in card on both platforms.
fn connect_hint() -> String {
    t("Click Connect to link this device with a one-time code.")
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

/// Windows signs in through the inline connect view in the Settings window's
/// Cloud page, so the History window opens Settings and watches the config for
/// the session to land. The executable resolves like the History window's
/// editor launchers: an explicit override, then a sibling of this binary, then
/// the bare name for anything that put it on `PATH`.
#[cfg(target_os = "windows")]
pub(crate) fn spawn_apexshot_login() {
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|current| current.parent().map(std::path::Path::to_path_buf))
        .map(|directory| directory.join("apexshot-settings.exe"))
        .filter(|sibling| sibling.is_file());
    let executable = std::env::var_os("APEXSHOT_SETTINGS")
        .map(std::path::PathBuf::from)
        .or(sibling)
        .unwrap_or_else(|| std::path::PathBuf::from("apexshot-settings.exe"));
    if crate::utils::hidden_command(&executable).spawn().is_ok() {
        return;
    }
    crate::utils::notify::desktop_notification(
        "Could not open Settings",
        "Open ApexShot Settings → Cloud to connect your ApexShot Cloud account.",
    );
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn auto_upload_sync_mirrors_only_real_persisted_changes() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let check = CheckButton::new();
            let syncer = Rc::new(AutoUploadSync::new(false));
            let saw_syncing = Rc::new(Cell::new(false));
            let saw_user_toggle = Rc::new(Cell::new(false));
            check.connect_toggled({
                let syncer = Rc::clone(&syncer);
                let saw_syncing = Rc::clone(&saw_syncing);
                let saw_user_toggle = Rc::clone(&saw_user_toggle);
                move |_| {
                    if syncer.is_syncing() {
                        saw_syncing.set(true);
                    } else {
                        saw_user_toggle.set(true);
                    }
                }
            });

            syncer.sync(&check, true);
            assert!(check.is_active());
            assert!(saw_syncing.get());
            assert!(!saw_user_toggle.get());

            check.set_active(false);
            assert!(saw_user_toggle.get());
            syncer.sync(&check, true);
            assert!(!check.is_active());

            syncer.note_saved(false);
            syncer.sync(&check, false);
            assert!(!check.is_active());
        }) else {
            eprintln!("skipping: no display available");
            return;
        };
    }

    #[test]
    fn a_save_reconciles_the_checkbox_against_freshly_persisted_state() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let check = CheckButton::new();
            check.set_active(false);
            let syncer = AutoUploadSync::new(false);
            syncer.sync(&check, true);
            let desired = check.is_active();
            assert!(desired);
            syncer.note_saved(desired);
        }) else {
            eprintln!("skipping: no display available");
            return;
        };
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn account_status_uses_the_token_not_cached_email() {
        let Some((stale_email, token_without_email)) = crate::test_support::with_gtk(|| {
            let avatar = Label::new(None);
            let status = Label::new(None);
            let email = Label::new(None);
            let connect = Button::new();
            let logout = Button::new();
            let mut config = AppConfig {
                cloud_user_email: "old@example.com".to_string(),
                ..AppConfig::default()
            };
            set_apexshot_status(&avatar, &status, &email, &connect, &logout, &config);
            let stale_email = (connect.is_visible(), logout.is_visible());
            config.cloud_api_token = "token".to_string();
            config.cloud_user_email.clear();
            set_apexshot_status(&avatar, &status, &email, &connect, &logout, &config);
            let token_without_email = (connect.is_visible(), logout.is_visible());
            (stale_email, token_without_email)
        }) else {
            eprintln!("skipping: no display available");
            return;
        };
        assert_eq!(stale_email, (true, false));
        assert_eq!(token_without_email, (false, true));
    }
}
