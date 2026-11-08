//! Optional cloud sign-in.
//!
//! The same browser device-authorization handoff the Linux flow runs, with a
//! Skip on every view: linking an account is never required to use ApexShot.
//! Signing in selects ApexShot Cloud as the upload destination, so the
//! connected view names what the account is for.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::{glib, prelude::*, Align, Box as GtkBox, Button, Label, Orientation, Spinner};

use super::ui::{copy_with_feedback, heading, login_error_message, open_link, status_pill};
use super::{px, Nav, OnboardingStep, SECTION_GAP};
use crate::cloud::auth::{begin_device_login, poll_device_login, DeviceLogin, LoginError};
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
        super::clear(&self.body);
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
    let scale = view.nav.scale;
    view.body.append(&heading(
        scale,
        &t("ApexShot Cloud"),
        &t("Optional. Link your account to upload screenshots and recordings to ApexShot Cloud."),
    ));

    let skip_btn = Button::with_label(&t("Skip for now"));
    skip_btn.add_css_class("onboarding-text-button");
    let advance = view.nav.advance.clone();
    skip_btn.connect_clicked(move |_| advance());
    view.actions.append(&skip_btn);

    let connect_btn = Button::with_label(&t("Connect account"));
    connect_btn.add_css_class("settings-primary-btn");
    let view_c = view.clone();
    connect_btn.connect_clicked(move |_| connect(&view_c));
    view.actions.append(&connect_btn);
}

fn connected(view: &View, config: &AppConfig) {
    view.begin();
    let scale = view.nav.scale;
    view.body.append(&heading(
        scale,
        &t("You're connected"),
        &t("Captures and recordings can now upload from this PC."),
    ));

    let tier = config.cloud_plan_tier.trim();
    let plan = if tier.is_empty() {
        t("Free")
    } else {
        capitalize(tier)
    };
    let pill = status_pill(&tfmt("{plan} plan", &[("plan", &plan)]));
    pill.set_halign(Align::Center);
    pill.set_margin_top(px(scale, SECTION_GAP));
    view.body.append(&pill);

    let email = if config.cloud_user_email.trim().is_empty() {
        t("Signed in")
    } else {
        config.cloud_user_email.clone()
    };
    let who = Label::new(Some(&email));
    who.set_halign(Align::Center);
    who.set_xalign(0.5);
    who.set_wrap(true);
    who.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    who.set_max_width_chars(36);
    who.set_justify(gtk4::Justification::Center);
    who.set_margin_top(px(scale, 10.0));
    who.add_css_class("cloud-user-name");
    view.body.append(&who);

    // A setup that picked up an account from an earlier install still needs a
    // way back to the code view, to link a different account or to try again.
    // It sits in the footer with Continue so both actions share one row.
    let logout_btn = Button::with_label(&t("Logout"));
    logout_btn.add_css_class("onboarding-text-button");
    logout_btn.set_valign(Align::Center);
    let view_logout = view.clone();
    logout_btn.connect_clicked(move |button| {
        button.set_sensitive(false);
        sign_out(&view_logout);
    });
    view.actions.append(&logout_btn);

    let continue_btn = Button::with_label(&t("Continue"));
    continue_btn.add_css_class("settings-primary-btn");
    let advance = view.nav.advance.clone();
    continue_btn.connect_clicked(move |_| advance());
    view.actions.append(&continue_btn);
    view.nav.layout_footer();
}

/// Revoke the session when the server answers, and forget it locally either
/// way, then re-render the step so it shows the sign-in views again.
fn sign_out(view: &View) {
    let (tx, rx) = async_channel::bounded::<()>(1);
    std::thread::spawn(move || {
        if crate::cloud::auth::logout().is_err() {
            let mut config = load_config();
            crate::cloud::auth::clear_session(&mut config);
            if let Err(error) = save_config(&config.sanitized()) {
                eprintln!("[onboarding] failed to clear the cloud session: {error}");
            }
        }
        let _ = tx.send_blocking(());
    });

    let view = view.clone();
    glib::spawn_future_local(async move {
        let _ = rx.recv().await;
        let go = view.nav.go.clone();
        go(OnboardingStep::Cloud);
    });
}

/// Device-authorization sub-view: show the code, open the browser, and poll
/// until the account lands. Expiry and failures end the session and offer a retry.
fn connect_ui(view: &View) -> ConnectUi {
    let scale = view.nav.scale;
    let session = view.begin();
    view.body.append(&heading(
        scale,
        &t("Connect your account"),
        &t("Enter this code in your browser to link this device."),
    ));

    // The code and its Copy action share one row. The link is not printed:
    // the browser opens on its own and Copy link covers the fallback.
    let code_frame = GtkBox::new(Orientation::Horizontal, px(scale, 8.0));
    code_frame.add_css_class("onboarding-connect-code-frame");
    code_frame.set_hexpand(true);
    code_frame.set_margin_top(px(scale, SECTION_GAP));

    let code_label = Label::new(Some("— — — —"));
    code_label.add_css_class("onboarding-connect-code");
    code_label.set_valign(Align::Center);
    code_label.set_xalign(0.0);
    code_label.set_hexpand(true);
    code_frame.append(&code_label);

    let copy_btn = Button::with_label(&t("Copy"));
    copy_btn.add_css_class("onboarding-text-button");
    copy_btn.set_valign(Align::Center);
    copy_btn.set_sensitive(false);
    code_frame.append(&copy_btn);
    view.body.append(&code_frame);

    let status_row = GtkBox::new(Orientation::Horizontal, px(scale, 8.0));
    status_row.set_halign(Align::Center);
    status_row.set_margin_top(px(scale, 18.0));
    let spinner = Spinner::new();
    spinner.set_valign(Align::Center);
    spinner.start();
    status_row.append(&spinner);
    let status = Label::new(Some(&t("Starting…")));
    status.add_css_class("settings-sub-option");
    status.set_valign(Align::Center);
    status.set_wrap(true);
    status.set_max_width_chars(34);
    status_row.append(&status);
    view.body.append(&status_row);

    let expiry = Label::new(None);
    expiry.add_css_class("settings-sub-option");
    expiry.set_halign(Align::Center);
    expiry.set_xalign(0.5);
    expiry.set_margin_top(px(scale, 6.0));
    expiry.set_visible(false);
    view.body.append(&expiry);

    // Only shown when the browser could not be opened.
    let browser_status = Label::new(None);
    browser_status.add_css_class("settings-sub-option");
    browser_status.set_halign(Align::Center);
    browser_status.set_xalign(0.5);
    browser_status.set_justify(gtk4::Justification::Center);
    browser_status.set_wrap(true);
    browser_status.set_max_width_chars(36);
    browser_status.set_margin_top(px(scale, 8.0));
    browser_status.set_visible(false);
    view.body.append(&browser_status);

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

    let secondary = GtkBox::new(Orientation::Horizontal, px(scale, 4.0));
    let copy_link_btn = Button::with_label(&t("Copy link"));
    copy_link_btn.add_css_class("onboarding-text-button");
    copy_link_btn.set_sensitive(false);
    secondary.append(&copy_link_btn);
    let skip_btn = Button::with_label(&t("Skip for now"));
    skip_btn.add_css_class("onboarding-text-button");
    let advance = view.nav.advance.clone();
    skip_btn.connect_clicked(move |_| advance());
    secondary.append(&skip_btn);
    view.actions.append(&secondary);
    view.nav.layout_footer();

    let ui = ConnectUi {
        view: view.clone(),
        session,
        code_label,
        copy_btn,
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
    ui
}

fn connect(view: &View) {
    let ui = connect_ui(view);
    let session = ui.session;
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
            copy_with_feedback(button, &value, &t("Copy"));
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
        eprintln!("[onboarding] cloud sign-in failed: {error}");
        self.view.session.set(self.session + 1);
        self.spinner.stop();
        self.spinner.set_visible(false);
        self.expiry.set_visible(false);
        self.open_btn.set_visible(false);
        self.copy_btn.set_sensitive(false);
        self.copy_link_btn.set_sensitive(false);
        self.browser_status.set_visible(false);
        self.status.set_text(&login_error_message(error));
        self.retry_btn.set_visible(true);
    }
}

/// A completed hosted sign-in makes ApexShot Cloud the upload destination, since
/// that is the service the user just chose.
fn select_hosted_destination() {
    let mut config = load_config();
    if config.cloud_destination == "apexshot" {
        return;
    }
    config.cloud_destination = "apexshot".to_string();
    if let Err(error) = save_config(&config.sanitized()) {
        eprintln!("[onboarding] failed to select ApexShot Cloud: {error}");
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
    use super::*;

    #[test]
    fn sign_in_waiting_and_failure_states_fit_without_scrolling() {
        let _ = crate::test_support::with_gtk(|| {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let (root, body, actions, nav) =
                    super::super::tests::test_form(scale, OnboardingStep::Cloud);
                let view = View {
                    body,
                    actions,
                    nav,
                    session: Rc::new(Cell::new(0)),
                };
                let config = AppConfig {
                    cloud_user_email: "a.long.account.address@example.com".into(),
                    ..Default::default()
                };
                connected(&view, &config);
                super::super::tests::assert_form_fits(&root, scale);
                // Build only the presentation; no login request or browser launch.
                let ui = connect_ui(&view);
                ui.code_label.set_text("KM5Y-6MNK");
                ui.status.set_text("Waiting for you to authorize…");
                ui.expiry.set_text("Code expires in 15 min");
                ui.expiry.set_visible(true);
                super::super::tests::assert_form_fits(&root, scale);
                ui.browser_status
                    .set_text("Couldn’t open a browser. Use Copy link and open it yourself.");
                ui.browser_status.set_visible(true);
                super::super::tests::assert_form_fits(&root, scale);
                ui.failed(&LoginError::Expired);
                super::super::tests::assert_form_fits(&root, scale);
            }
        });
    }

    #[test]
    fn capitalize_uppercases_the_first_letter_only() {
        assert_eq!(capitalize("pro"), "Pro");
        assert_eq!(capitalize("team"), "Team");
        assert_eq!(capitalize(""), "");
    }
}
