//! Device-code sign-in for the Settings window.
//!
//! Windows users never see the terminal the Linux `apexshot login` flow prints
//! its one-time code into, so Connect swaps the Cloud settings page for a
//! device-authorization view in the same window.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, Button, Label, Orientation, Spinner, Stack, Widget,
};

use crate::cloud::auth::{begin_device_login, poll_device_login, DeviceLogin, LoginError};
use crate::cloud::listing::CloudAccount;
use crate::i18n::{t, tfmt};
use crate::onboarding::ui::{copy_with_feedback, login_error_message};

/// Results from the worker threads that run the device-authorization flow.
enum ConnectMsg {
    Started(Result<DeviceLogin, LoginError>),
    Polled(Result<Option<CloudAccount>, LoginError>),
}

/// What the call-to-action does on its next click.
#[derive(Clone, Copy, PartialEq)]
enum PrimaryAction {
    OpenBrowser,
    Retry,
}

/// Replace the Cloud page with sign-in without opening a separate window.
pub(crate) fn wrap_cloud_page(cloud_page: &impl IsA<Widget>) -> (Stack, ConnectFlow) {
    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
    stack.set_vhomogeneous(false);
    stack.add_named(cloud_page, Some("status"));

    let view = GtkBox::new(Orientation::Vertical, 0);
    view.set_hexpand(true);
    view.set_size_request(-1, 540);

    let cancel_btn = Button::with_label(&t("Back to Cloud"));
    cancel_btn.add_css_class("cloud-connect-back");
    cancel_btn.set_halign(Align::Start);
    view.append(&cancel_btn);

    let content = GtkBox::new(Orientation::Vertical, 0);
    content.add_css_class("cloud-connect-content");
    content.set_halign(Align::Center);
    content.set_valign(Align::Center);
    content.set_vexpand(true);
    content.set_margin_top(24);
    content.set_margin_bottom(40);
    view.append(&content);

    let eyebrow = Label::new(Some(&t("APEXSHOT CLOUD")));
    eyebrow.add_css_class("cloud-connect-eyebrow");
    eyebrow.set_halign(Align::Center);
    content.append(&eyebrow);

    let title = Label::new(Some(&t("Authorize this device")));
    title.add_css_class("cloud-connect-title");
    title.set_halign(Align::Center);
    title.set_margin_top(8);
    content.append(&title);

    let subtitle = Label::new(Some(&t(
        "Enter this code in your browser to link this device.",
    )));
    subtitle.add_css_class("onboarding-copy");
    subtitle.set_wrap(true);
    subtitle.set_xalign(0.5);
    subtitle.set_halign(Align::Center);
    subtitle.set_justify(gtk4::Justification::Center);
    subtitle.set_margin_top(8);
    content.append(&subtitle);

    // The code mirrors the browser page's segmented input: one rounded cell
    // per character, empty until the backend answers.
    let code_row = GtkBox::new(Orientation::Horizontal, 8);
    code_row.set_halign(Align::Center);
    code_row.set_margin_top(24);
    set_code_cells(&code_row, "    -    ");
    content.append(&code_row);

    let link_label = Label::new(None);
    link_label.add_css_class("settings-sub-option");
    link_label.set_selectable(true);
    link_label.set_wrap(true);
    link_label.set_xalign(0.5);
    link_label.set_halign(Align::Center);
    link_label.set_margin_top(14);
    link_label.set_visible(false);
    content.append(&link_label);

    let open_btn = Button::with_label(&t("Open browser"));
    open_btn.add_css_class("settings-primary-btn");
    open_btn.add_css_class("cloud-connect-action");
    open_btn.set_halign(Align::Fill);
    open_btn.set_margin_top(22);
    open_btn.set_sensitive(false);
    content.append(&open_btn);

    let actions = GtkBox::new(Orientation::Horizontal, 10);
    actions.set_homogeneous(true);
    actions.set_margin_top(10);
    let copy_btn = Button::with_label(&t("Copy code"));
    copy_btn.add_css_class("cloud-connect-secondary");
    copy_btn.set_sensitive(false);
    actions.append(&copy_btn);
    let copy_link_btn = Button::with_label(&t("Copy link"));
    copy_link_btn.add_css_class("cloud-connect-secondary");
    copy_link_btn.set_sensitive(false);
    actions.append(&copy_link_btn);
    content.append(&actions);

    let browser_status = Label::new(None);
    browser_status.add_css_class("settings-sub-option");
    browser_status.set_halign(Align::Center);
    browser_status.set_xalign(0.5);
    browser_status.set_wrap(true);
    browser_status.set_margin_top(12);
    browser_status.set_visible(false);
    content.append(&browser_status);

    let status_row = GtkBox::new(Orientation::Horizontal, 8);
    status_row.set_halign(Align::Center);
    status_row.set_margin_top(14);
    let spinner = Spinner::new();
    spinner.set_valign(Align::Center);
    spinner.start();
    status_row.append(&spinner);
    let status = Label::new(Some(&t("Starting…")));
    status.add_css_class("settings-sub-option");
    status.set_valign(Align::Center);
    status.set_wrap(true);
    status_row.append(&status);
    content.append(&status_row);

    let expiry = Label::new(None);
    expiry.add_css_class("settings-sub-option");
    expiry.set_halign(Align::Center);
    expiry.set_xalign(0.5);
    expiry.set_margin_top(4);
    expiry.set_visible(false);
    content.append(&expiry);

    stack.add_named(&view, Some("connect"));

    let ui = ConnectUi {
        stack: stack.clone(),
        view: view.clone(),
        code_row,
        session: Rc::new(Cell::new(0)),
        active: Rc::new(Cell::new(false)),
        primary_action: Rc::new(Cell::new(PrimaryAction::OpenBrowser)),
        copy_btn,
        link_label,
        copy_link_btn,
        browser_status,
        open_btn,
        status,
        spinner,
        expiry,
        polling: Rc::new(Cell::new(false)),
        code: Rc::new(RefCell::new(String::new())),
        uri: Rc::new(RefCell::new(String::new())),
    };
    ui.wire_actions(&cancel_btn);
    // Leaving Cloud abandons the flow and returns to settings on the next visit.
    {
        let ui_unmap = ui.clone();
        view.connect_unmap(move |_| {
            ui_unmap.abandon();
            ui_unmap.stack.set_visible_child_name("status");
        });
    }

    (stack, ConnectFlow { ui })
}

/// The Connect button's handle: switch pages and start the flow.
pub(crate) struct ConnectFlow {
    ui: ConnectUi,
}

impl ConnectFlow {
    pub(crate) fn begin(&self) {
        self.ui.stack.set_visible_child_name("connect");
        self.ui.start();
    }
}

/// One cell per code character, dashes passed through as separators.
fn set_code_cells(row: &GtkBox, code: &str) {
    let mut child = row.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        row.remove(&widget);
    }
    for ch in code.chars() {
        let text = if ch == ' ' {
            String::new()
        } else {
            ch.to_string()
        };
        let cell = Label::new(Some(&text));
        cell.add_css_class(if ch == '-' {
            "cloud-connect-dash"
        } else {
            "cloud-connect-cell"
        });
        cell.set_valign(Align::Center);
        row.append(&cell);
    }
}

#[derive(Clone)]
struct ConnectUi {
    stack: Stack,
    view: GtkBox,
    code_row: GtkBox,
    /// Bumped on every (re)start, failure, and abandon, so callbacks from an
    /// older run stop touching the widgets.
    session: Rc<Cell<u64>>,
    /// True while a login is in flight; a stale view that maps without one
    /// falls back to the account row.
    active: Rc<Cell<bool>>,
    primary_action: Rc<Cell<PrimaryAction>>,
    copy_btn: Button,
    link_label: Label,
    copy_link_btn: Button,
    browser_status: Label,
    open_btn: Button,
    status: Label,
    spinner: Spinner,
    expiry: Label,
    polling: Rc<Cell<bool>>,
    code: Rc<RefCell<String>>,
    uri: Rc<RefCell<String>>,
}

impl ConnectUi {
    fn is_live(&self, session: u64) -> bool {
        self.session.get() == session && self.active.get() && self.view.is_mapped()
    }

    fn wire_actions(&self, cancel_btn: &Button) {
        {
            let ui = self.clone();
            cancel_btn.connect_clicked(move |_| {
                ui.abandon();
                ui.stack.set_visible_child_name("status");
            });
        }

        {
            let code = Rc::clone(&self.code);
            self.copy_btn.connect_clicked(move |button| {
                let value = code.borrow().clone();
                if value.is_empty() {
                    return;
                }
                copy_with_feedback(button, &value, &t("Copy code"));
            });
        }

        {
            let uri = Rc::clone(&self.uri);
            self.copy_link_btn.connect_clicked(move |button| {
                let value = uri.borrow().clone();
                if value.is_empty() {
                    return;
                }
                copy_with_feedback(button, &value, &t("Copy link"));
            });
        }

        {
            let ui = self.clone();
            self.open_btn
                .connect_clicked(move |_| match ui.primary_action.get() {
                    PrimaryAction::OpenBrowser => {
                        let target = ui.uri.borrow().clone();
                        if target.is_empty() {
                            return;
                        }
                        open_browser(&target, &ui.browser_status);
                    }
                    PrimaryAction::Retry => ui.start(),
                });
        }
    }

    /// (Re)start the flow: reset the widgets, bump the session so callbacks
    /// from an older run go dead, and request a fresh device code.
    fn start(&self) {
        let session = self.session.get() + 1;
        self.session.set(session);
        self.active.set(true);

        set_code_cells(&self.code_row, "    -    ");
        self.code.replace(String::new());
        self.uri.replace(String::new());
        self.copy_btn.set_sensitive(false);
        self.copy_link_btn.set_sensitive(false);
        self.link_label.set_visible(false);
        self.browser_status.set_visible(false);
        self.expiry.set_visible(false);
        self.status.remove_css_class("onboarding-status-error");
        self.status.set_text(&t("Starting…"));
        self.spinner.start();
        self.polling.set(false);
        self.primary_action.set(PrimaryAction::OpenBrowser);
        self.open_btn.set_label(&t("Open browser"));
        self.open_btn.set_sensitive(false);

        let (tx, rx) = async_channel::unbounded::<ConnectMsg>();
        let login_tx = tx.clone();
        std::thread::spawn(move || {
            let _ = login_tx.send_blocking(ConnectMsg::Started(begin_device_login()));
        });

        let pending_tx = Rc::new(RefCell::new(Some(tx)));
        let ui = self.clone();
        glib::spawn_future_local(async move {
            while let Ok(message) = rx.recv().await {
                if !ui.is_live(session) {
                    break;
                }
                match message {
                    ConnectMsg::Started(Ok(login)) => {
                        if let Some(tx) = pending_tx.borrow_mut().take() {
                            ui.waiting(&login);
                            ui.schedule_polls(&login, session, tx);
                        }
                    }
                    ConnectMsg::Started(Err(error)) => {
                        pending_tx.borrow_mut().take();
                        ui.failed(&error);
                    }
                    // The session landed and finish_login already saved it —
                    // the account row's refresh picks it up on its next tick.
                    ConnectMsg::Polled(Ok(Some(_account))) => {
                        ui.abandon();
                        ui.stack.set_visible_child_name("status");
                        break;
                    }
                    ConnectMsg::Polled(Ok(None)) => ui.polling.set(false),
                    ConnectMsg::Polled(Err(error)) => ui.failed(&error),
                }
            }
        });
    }

    /// End an in-flight login without changing which page is visible.
    fn abandon(&self) {
        self.active.set(false);
        self.session.set(self.session.get() + 1);
        self.spinner.stop();
        self.polling.set(false);
    }

    fn waiting(&self, login: &DeviceLogin) {
        set_code_cells(&self.code_row, &login.user_code);
        self.code.replace(login.user_code.clone());
        self.uri.replace(login.verification_uri.clone());
        self.copy_btn.set_sensitive(true);
        self.copy_link_btn.set_sensitive(true);
        self.open_btn.set_sensitive(true);
        self.link_label.set_text(&login.verification_uri);
        self.link_label.set_visible(true);
        self.browser_status.set_visible(true);
        self.status.set_text(&t("Waiting for you to authorize…"));

        let minutes = login.expires_in_secs.div_ceil(60).max(1);
        self.expiry.set_text(&tfmt(
            "Code expires in {minutes} min",
            &[("minutes", &minutes.to_string())],
        ));
        self.expiry.set_visible(true);

        open_browser(&login.verification_uri, &self.browser_status);
    }

    /// Poll on the server's interval, and end the session on the device code's
    /// own expiry. Each timer stops itself once this session is no longer live.
    fn schedule_polls(
        &self,
        login: &DeviceLogin,
        session: u64,
        tx: async_channel::Sender<ConnectMsg>,
    ) {
        let polling_ui = self.clone();
        let device_code = login.device_code.clone();
        let interval = login.interval_secs.clamp(1, u64::from(u32::MAX)) as u32;
        glib::timeout_add_seconds_local(interval, move || {
            if !polling_ui.is_live(session) {
                return glib::ControlFlow::Break;
            }
            if polling_ui.polling.get() {
                return glib::ControlFlow::Continue;
            }
            polling_ui.polling.set(true);
            let tx = tx.clone();
            let device_code = device_code.clone();
            std::thread::spawn(move || {
                let _ = tx.send_blocking(ConnectMsg::Polled(poll_device_login(&device_code)));
            });
            glib::ControlFlow::Continue
        });

        let expires = login.expires_in_secs.clamp(1, u64::from(u32::MAX)) as u32;
        let expiry_ui = self.clone();
        glib::timeout_add_seconds_local_once(expires, move || {
            if expiry_ui.is_live(session) {
                expiry_ui.failed(&LoginError::Expired);
            }
        });
    }

    fn failed(&self, error: &LoginError) {
        eprintln!("[settings] cloud sign-in failed: {error}");
        self.session.set(self.session.get() + 1);
        self.active.set(false);
        self.spinner.stop();
        self.expiry.set_visible(false);
        self.copy_btn.set_sensitive(false);
        self.copy_link_btn.set_sensitive(false);
        self.link_label.set_visible(false);
        self.browser_status.set_visible(false);
        self.status.add_css_class("onboarding-status-error");
        self.status.set_text(&login_error_message(error));
        self.primary_action.set(PrimaryAction::Retry);
        self.open_btn.set_label(&t("Try again"));
        self.open_btn.set_sensitive(true);
    }
}

fn open_browser(url: &str, status: &Label) {
    status.set_visible(false);
    let url = url.to_string();
    let status = status.downgrade();
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(crate::utils::open::open_url(&url));
    });
    glib::spawn_future_local(async move {
        if let (Ok(Err(_)), Some(status)) = (rx.recv().await, status.upgrade()) {
            status.set_text(&t(
                "Couldn't open your browser. Copy the link and open it yourself.",
            ));
            status.set_visible(true);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_settings_and_sign_in_use_independent_page_heights() {
        let Some((homogeneous, initial_page, settings_height, sign_in_height)) =
            crate::test_support::with_gtk(|| {
                let settings = GtkBox::new(Orientation::Vertical, 0);
                settings.set_size_request(-1, 80);
                let (stack, _flow) = wrap_cloud_page(&settings);
                let initial_page = stack.visible_child_name().map(|name| name.to_string());
                let settings_height = stack.measure(Orientation::Vertical, -1).1;
                stack.set_visible_child_name("connect");
                let sign_in_height = stack.measure(Orientation::Vertical, -1).1;
                (
                    stack.is_vhomogeneous(),
                    initial_page,
                    settings_height,
                    sign_in_height,
                )
            })
        else {
            eprintln!("skipping: no display available");
            return;
        };

        assert!(!homogeneous);
        assert_eq!(initial_page.as_deref(), Some("status"));
        assert!(settings_height < sign_in_height);
    }

    #[test]
    fn code_cells_keep_the_separator_outside_the_boxes() {
        let Some(classes) = crate::test_support::with_gtk(|| {
            let row = GtkBox::new(Orientation::Horizontal, 0);
            set_code_cells(&row, "ABCD-EFGH");
            let mut child = row.first_child();
            let mut classes = Vec::new();
            while let Some(widget) = child {
                classes.push((
                    widget.has_css_class("cloud-connect-cell"),
                    widget.has_css_class("cloud-connect-dash"),
                ));
                child = widget.next_sibling();
            }
            classes
        }) else {
            eprintln!("skipping: no display available");
            return;
        };

        assert_eq!(classes.len(), 9);
        assert_eq!(classes[4], (false, true));
        assert!(classes
            .iter()
            .enumerate()
            .all(|(i, classes)| i == 4 || *classes == (true, false)));
    }

    #[test]
    fn only_browser_action_uses_the_primary_style() {
        let Some(classes) = crate::test_support::with_gtk(|| {
            let settings = GtkBox::new(Orientation::Vertical, 0);
            let (_, flow) = wrap_cloud_page(&settings);
            (
                flow.ui.open_btn.has_css_class("settings-primary-btn"),
                flow.ui.copy_btn.has_css_class("settings-primary-btn"),
                flow.ui.copy_link_btn.has_css_class("settings-primary-btn"),
                flow.ui.copy_btn.has_css_class("cloud-connect-secondary"),
                flow.ui
                    .copy_link_btn
                    .has_css_class("cloud-connect-secondary"),
            )
        }) else {
            eprintln!("skipping: no display available");
            return;
        };
        assert_eq!(classes, (true, false, false, true, true));
    }

    #[test]
    fn cloud_connect_styles_parse() {
        let Some(errors) = crate::test_support::with_gtk(|| {
            let provider = gtk4::CssProvider::new();
            let errors = Rc::new(RefCell::new(Vec::new()));
            let captured = Rc::clone(&errors);
            provider.connect_parsing_error(move |_, _, error| {
                captured.borrow_mut().push(error.message().to_string());
            });
            provider.load_from_data(include_str!(
                "../../../src/settings/css/07-pages-about-onboarding.css"
            ));
            let result = errors.borrow().clone();
            result
        }) else {
            eprintln!("skipping: no display available");
            return;
        };
        assert!(errors.is_empty(), "CSS parsing errors: {errors:?}");
    }
}
