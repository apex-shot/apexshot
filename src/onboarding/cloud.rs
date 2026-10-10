use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::{glib, prelude::*, Align, Box as GtkBox, Button, Label, Orientation, Spinner};

use super::ui::{copy_with_feedback, heading_centered, open_link, option_card, status_pill};
use super::{Nav, OnboardingStep};
use crate::capture::editor::window::icon_names::custom;
use crate::cloud::auth::{begin_device_login, poll_device_login, DeviceLogin, LoginError};
use crate::cloud::destination::Destination;
use crate::cloud::listing::CloudAccount;
use crate::config::{is_cloud_logged_in, load_config, save_config, AppConfig};
use crate::i18n::{t, tfmt};

/// Results from the worker threads that run the device-authorization flow.
enum ConnectMsg {
    Started(Result<DeviceLogin, LoginError>),
    Polled(Result<Option<CloudAccount>, LoginError>),
}

/// The cloud step's view of the window. `session` changes whenever the view is
/// re-rendered, so a login started by an older render stops on its next check.
#[derive(Clone)]
struct View {
    body: GtkBox,
    actions: GtkBox,
    nav: Nav,
    session: Rc<Cell<u64>>,
}

impl View {
    fn begin(&self) -> u64 {
        let mut child = self.body.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if !widget.has_css_class("onboarding-mascot") {
                self.body.remove(&widget);
            }
        }
        super::clear(&self.actions);
        let session = self.session.get() + 1;
        self.session.set(session);
        session
    }

    fn is_live(&self, session: u64) -> bool {
        self.session.get() == session && self.nav.scope.is_current() && self.body.is_mapped()
    }
}

pub fn build(body: &GtkBox, actions: &GtkBox, nav: &Nav) {
    let view = View {
        body: body.clone(),
        actions: actions.clone(),
        nav: nav.clone(),
        session: Rc::new(Cell::new(0)),
    };
    let config = load_config().sanitized();
    if is_cloud_logged_in(&config) {
        connected(&view, &config);
    } else {
        intro(&view);
    }
}

fn intro(view: &View) {
    view.begin();
    view.body.append(&heading_centered(
        &t("Share captures as links"),
        &t("Optional. Connect ApexShot Cloud to turn a capture into a link you can paste anywhere. ApexShot works offline without an account, so you can skip this."),
    ));

    let options = GtkBox::new(Orientation::Vertical, 12);
    options.set_margin_top(26);
    options.append(&option_card(
        custom::APEXSHOT_CLOUD,
        &t("ApexShot Cloud"),
        &t("Hosted by us. Connect with a one-time code from your browser."),
    ));
    options.append(&option_card(
        custom::XBACKBONE,
        &t("XBackBone"),
        &t("Self-host for full control of storage. Set it up later in Settings → Cloud."),
    ));
    view.body.append(&options);

    let skip_btn = Button::with_label(&t("Skip for now"));
    skip_btn.add_css_class("onboarding-text-button");
    let advance = view.nav.advance.clone();
    skip_btn.connect_clicked(move |_| advance());
    view.actions.append(&skip_btn);

    let connect_btn = Button::with_label(&t("Connect ApexShot Cloud"));
    connect_btn.add_css_class("settings-primary-btn");
    let view_c = view.clone();
    connect_btn.connect_clicked(move |_| connect(&view_c));
    view.actions.append(&connect_btn);
}

fn connected(view: &View, config: &AppConfig) {
    view.begin();
    view.body.append(&heading_centered(
        &t("You're connected"),
        &connected_note(config),
    ));

    let tier = config.cloud_plan_tier.trim();
    let plan = if tier.is_empty() {
        t("Free")
    } else {
        capitalize(tier)
    };
    let pill = status_pill(&tfmt("{plan} plan", &[("plan", &plan)]));
    pill.set_halign(Align::Center);
    pill.set_margin_top(26);
    view.body.append(&pill);

    let email = if config.cloud_user_email.trim().is_empty() {
        t("Signed in")
    } else {
        config.cloud_user_email.clone()
    };
    let who = Label::new(Some(&email));
    who.set_halign(Align::Center);
    who.set_xalign(0.5);
    who.set_margin_top(10);
    who.add_css_class("cloud-user-name");
    view.body.append(&who);

    let continue_btn = Button::with_label(&t("Continue"));
    continue_btn.add_css_class("settings-primary-btn");
    let advance = view.nav.advance.clone();
    continue_btn.connect_clicked(move |_| advance());
    view.actions.append(&continue_btn);
    view.nav.layout_footer();
}

/// Describe where captures actually go, so the connected view never promises a
/// share link that the current destination or auto-upload setting will not make.
fn connected_note(config: &AppConfig) -> String {
    if Destination::from_config(config) == Destination::XBackbone {
        return t(
            "Captures upload to your XBackBone server. Change the destination in Settings → Cloud.",
        );
    }
    if !config.cloud_auto_upload_after_capture {
        return t("Automatic upload is off. Turn it on in Settings → Cloud to get a share link after each capture.");
    }
    t("Uploads are on: your next screenshot gets a share link copied for you.")
}

/// Device-authorization sub-view: show the code, open the browser, and poll
/// until the account lands. Expiry and failures end the session and offer a retry.
fn connect(view: &View) {
    let session = view.begin();
    view.body.append(&heading_centered(
        &t("Connect your account"),
        &t("Enter this code in your browser to link this device."),
    ));

    let code_frame = GtkBox::new(Orientation::Horizontal, 12);
    code_frame.add_css_class("onboarding-connect-code-frame");
    code_frame.set_halign(Align::Center);
    code_frame.set_margin_top(26);

    let code_label = Label::new(Some("— — — —"));
    code_label.add_css_class("onboarding-connect-code");
    code_label.set_valign(Align::Center);
    code_frame.append(&code_label);

    let copy_btn = Button::with_label(&t("Copy code"));
    copy_btn.add_css_class("secondary-settings-button");
    copy_btn.set_valign(Align::Center);
    copy_btn.set_sensitive(false);
    code_frame.append(&copy_btn);
    view.body.append(&code_frame);

    let link_row = GtkBox::new(Orientation::Horizontal, 10);
    link_row.add_css_class("onboarding-connect-link-frame");
    link_row.set_halign(Align::Center);
    link_row.set_margin_top(14);
    link_row.set_visible(false);
    let link_label = Label::new(None);
    link_label.add_css_class("onboarding-connect-link");
    link_label.set_selectable(true);
    link_label.set_wrap(true);
    link_label.set_xalign(0.0);
    link_label.set_valign(Align::Center);
    link_row.append(&link_label);
    let copy_link_btn = Button::with_label(&t("Copy link"));
    copy_link_btn.add_css_class("secondary-settings-button");
    copy_link_btn.set_valign(Align::Center);
    link_row.append(&copy_link_btn);
    view.body.append(&link_row);

    let browser_status = Label::new(None);
    browser_status.add_css_class("settings-sub-option");
    browser_status.set_halign(Align::Center);
    browser_status.set_xalign(0.5);
    browser_status.set_wrap(true);
    browser_status.set_margin_top(10);
    browser_status.set_visible(false);
    view.body.append(&browser_status);

    let status_row = GtkBox::new(Orientation::Horizontal, 8);
    status_row.set_halign(Align::Center);
    status_row.set_margin_top(18);
    let spinner = Spinner::new();
    spinner.set_valign(Align::Center);
    spinner.start();
    status_row.append(&spinner);
    let status = Label::new(Some(&t("Starting…")));
    status.add_css_class("settings-sub-option");
    status.set_valign(Align::Center);
    status_row.append(&status);
    view.body.append(&status_row);

    let expiry = Label::new(None);
    expiry.add_css_class("settings-sub-option");
    expiry.set_halign(Align::Center);
    expiry.set_xalign(0.5);
    expiry.set_margin_top(6);
    expiry.set_visible(false);
    view.body.append(&expiry);

    let cancel_btn = Button::with_label(&t("Cancel"));
    cancel_btn.add_css_class("onboarding-danger-button");
    cancel_btn.set_halign(Align::Center);
    cancel_btn.set_margin_top(12);
    let view_cancel = view.clone();
    cancel_btn.connect_clicked(move |_| intro(&view_cancel));
    view.body.append(&cancel_btn);

    let skip_btn = Button::with_label(&t("Skip for now"));
    skip_btn.add_css_class("onboarding-text-button");
    let advance = view.nav.advance.clone();
    skip_btn.connect_clicked(move |_| advance());
    view.actions.append(&skip_btn);

    let open_btn = Button::with_label(&t("Open browser"));
    open_btn.add_css_class("settings-primary-btn");
    open_btn.set_sensitive(false);
    view.actions.append(&open_btn);

    let retry_btn = Button::with_label(&t("Try again"));
    retry_btn.add_css_class("settings-primary-btn");
    retry_btn.set_visible(false);
    let view_retry = view.clone();
    retry_btn.connect_clicked(move |_| connect(&view_retry));
    view.actions.append(&retry_btn);
    view.nav.layout_footer();

    let ui = ConnectUi {
        view: view.clone(),
        session,
        code_label,
        copy_btn,
        link_row,
        link_label,
        copy_link_btn,
        browser_status,
        open_btn,
        retry_btn,
        status,
        spinner,
        expiry,
        polling: Rc::new(Cell::new(false)),
        code: Rc::new(RefCell::new(String::new())),
        uri: Rc::new(RefCell::new(String::new())),
    };
    ui.wire_actions();

    let (tx, rx) = async_channel::unbounded::<ConnectMsg>();
    let login_tx = tx.clone();
    std::thread::spawn(move || {
        let _ = login_tx.send_blocking(ConnectMsg::Started(begin_device_login()));
    });

    let pending_tx = Rc::new(RefCell::new(Some(tx)));
    glib::spawn_future_local(async move {
        while let Ok(message) = rx.recv().await {
            if !ui.view.is_live(session) {
                break;
            }
            match message {
                ConnectMsg::Started(Ok(login)) => {
                    if let Some(tx) = pending_tx.borrow_mut().take() {
                        ui.waiting(&login);
                        ui.schedule_polls(&login, tx);
                    }
                }
                ConnectMsg::Started(Err(error)) => {
                    pending_tx.borrow_mut().take();
                    ui.failed(&error);
                }
                ConnectMsg::Polled(Ok(Some(_account))) => {
                    select_hosted_destination();
                    let go = ui.view.nav.go.clone();
                    go(OnboardingStep::Cloud);
                    break;
                }
                ConnectMsg::Polled(Ok(None)) => ui.polling.set(false),
                ConnectMsg::Polled(Err(error)) => ui.failed(&error),
            }
        }
    });
}

#[derive(Clone)]
struct ConnectUi {
    view: View,
    session: u64,
    code_label: Label,
    copy_btn: Button,
    link_row: GtkBox,
    link_label: Label,
    copy_link_btn: Button,
    browser_status: Label,
    open_btn: Button,
    retry_btn: Button,
    status: Label,
    spinner: Spinner,
    expiry: Label,
    polling: Rc<Cell<bool>>,
    code: Rc<RefCell<String>>,
    uri: Rc<RefCell<String>>,
}

impl ConnectUi {
    fn wire_actions(&self) {
        let code = Rc::clone(&self.code);
        self.copy_btn.connect_clicked(move |button| {
            let value = code.borrow().clone();
            if value.is_empty() {
                return;
            }
            copy_with_feedback(button, &value, &t("Copy code"));
        });

        let uri = Rc::clone(&self.uri);
        self.copy_link_btn.connect_clicked(move |button| {
            let value = uri.borrow().clone();
            if value.is_empty() {
                return;
            }
            copy_with_feedback(button, &value, &t("Copy link"));
        });

        let uri = Rc::clone(&self.uri);
        let browser_status = self.browser_status.clone();
        self.open_btn.connect_clicked(move |_| {
            let target = uri.borrow().clone();
            if target.is_empty() {
                return;
            }
            open_link(&target, &browser_status);
        });
    }

    fn waiting(&self, login: &DeviceLogin) {
        self.code_label.set_text(&login.user_code);
        self.code.replace(login.user_code.clone());
        self.uri.replace(login.verification_uri.clone());
        self.copy_btn.set_sensitive(true);
        self.copy_link_btn.set_sensitive(true);
        self.open_btn.set_sensitive(true);
        self.link_label.set_text(&login.verification_uri);
        self.link_row.set_visible(true);
        self.browser_status.set_visible(true);
        self.status.set_text(&t("Waiting for you to authorize…"));

        let minutes = login.expires_in_secs.div_ceil(60).max(1);
        self.expiry.set_text(&tfmt(
            "Code expires in {minutes} min",
            &[("minutes", &minutes.to_string())],
        ));
        self.expiry.set_visible(true);

        open_link(&login.verification_uri, &self.browser_status);
    }

    /// Poll on the server's interval, and end the session on the device code's
    /// own expiry. Each timer stops itself once this session is no longer live.
    fn schedule_polls(&self, login: &DeviceLogin, tx: async_channel::Sender<ConnectMsg>) {
        let session = self.session;
        let view = self.view.clone();
        let polling = Rc::clone(&self.polling);
        let device_code = login.device_code.clone();
        let interval = login.interval_secs.clamp(1, u64::from(u32::MAX)) as u32;
        glib::timeout_add_seconds_local(interval, move || {
            if !view.is_live(session) {
                return glib::ControlFlow::Break;
            }
            if polling.get() {
                return glib::ControlFlow::Continue;
            }
            polling.set(true);
            let tx = tx.clone();
            let device_code = device_code.clone();
            std::thread::spawn(move || {
                let _ = tx.send_blocking(ConnectMsg::Polled(poll_device_login(&device_code)));
            });
            glib::ControlFlow::Continue
        });

        let expiry_ui = self.clone();
        let expires = login.expires_in_secs.clamp(1, u64::from(u32::MAX)) as u32;
        glib::timeout_add_seconds_local_once(expires, move || {
            if expiry_ui.view.is_live(expiry_ui.session) {
                expiry_ui.failed(&LoginError::Expired);
            }
        });
    }

    fn failed(&self, error: &LoginError) {
        eprintln!("[onboarding] Cloud sign-in failed: {error}");
        self.view.session.set(self.session + 1);
        self.spinner.stop();
        self.expiry.set_visible(false);
        self.open_btn.set_sensitive(false);
        self.copy_btn.set_sensitive(false);
        self.copy_link_btn.set_sensitive(false);
        self.link_row.set_visible(false);
        self.browser_status.set_visible(false);
        self.status.set_text(&error_message(error));
        self.retry_btn.set_visible(true);
    }
}

fn error_message(error: &LoginError) -> String {
    match error {
        LoginError::Expired => t("This code expired. Try again to get a new one."),
        LoginError::Denied => t("Authorization was denied in the browser."),
        LoginError::NotConfigured | LoginError::HttpRequest(_) | LoginError::Server(_) => {
            t("Couldn't reach ApexShot Cloud. Check your connection and try again.")
        }
    }
}

/// A completed hosted sign-in makes ApexShot Cloud the upload destination, since
/// that is the service the user just chose. The generic CLI login leaves it alone.
fn select_hosted_destination() {
    let mut config = load_config();
    if config.cloud_destination == "apexshot" {
        return;
    }
    config.cloud_destination = "apexshot".to_string();
    if let Err(error) = save_config(&config.sanitized()) {
        eprintln!("[onboarding] Failed to select ApexShot Cloud: {error}");
    }
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::capitalize;

    #[test]
    fn capitalize_uppercases_the_first_letter_only() {
        assert_eq!(capitalize("pro"), "Pro");
        assert_eq!(capitalize("team"), "Team");
        assert_eq!(capitalize(""), "");
    }
}
