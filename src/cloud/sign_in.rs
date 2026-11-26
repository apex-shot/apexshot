//! In-app ApexShot Cloud sign-in, shown when an upload needs an account.
//!
//! A signed-out upload used to end in a notification that said nothing could be
//! uploaded. This card connects the account with a one-time code instead, then
//! lets the originating action retry itself.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::{
    gdk, glib, prelude::*, Align, Application, Box as GtkBox, Button, CssProvider,
    EventControllerKey, Label, Orientation, Overlay, PropagationPhase, Spinner, Window,
};

use crate::cloud::auth::{begin_device_login, poll_device_login, DeviceLogin, LoginError};
use crate::cloud::destination::Destination;
use crate::cloud::listing::CloudAccount;
use crate::cloud::upload::is_configured;
use crate::config::AppConfig;
use crate::i18n::{t, tfmt};
use crate::onboarding::ui::copy_with_feedback;
use crate::studio_upsell::{
    centered_label, close_button, install_css, link_button, primary_button, px, theme_is_dark,
    LIGHT_CLASS,
};

/// Messages from the worker threads that run the device-authorization flow.
enum ConnectMsg {
    Started(Result<DeviceLogin, LoginError>),
    Polled(Result<Option<CloudAccount>, LoginError>),
}

/// What the card's primary button does on its next click.
#[derive(Clone, Copy, PartialEq)]
enum PrimaryAction {
    OpenBrowser,
    Retry,
}

/// True when the selected destination is ApexShot Cloud and this device has no
/// account yet. XBackBone keeps its existing setup notification.
pub fn needs_apexshot_sign_in(config: &AppConfig) -> bool {
    Destination::from_config(config) == Destination::ApexShot && !is_configured(config)
}

/// Show the sign-in card for an upload, then run `retry` once the account
/// connects. The card starts with the short introduction.
pub fn prompt_for_upload(parent: Option<&Window>, retry: impl Fn() + 'static) {
    crate::usage_telemetry::record_upload_click_signed_out();
    build_card(
        parent,
        None,
        SignInOptions {
            start_at_device: false,
            success_lede: t("Uploading your capture…"),
            on_success: Rc::new(retry),
        },
    );
}

/// Show the card at the device-code step, for Settings and History.
pub fn show_connect(parent: Option<&Window>, on_connected: impl Fn() + 'static) {
    build_card(
        parent,
        None,
        SignInOptions {
            start_at_device: true,
            success_lede: t("Your next capture gets a share link."),
            on_success: Rc::new(on_connected),
        },
    );
}

/// Run the upload card as this process's own GTK application.
///
/// The Windows capture shell starts `--upload <path>` detached while signed
/// out, so this process owns the whole flow. Returns true when the account
/// connected before the window closed.
pub fn run_standalone_for_upload() -> bool {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let connected = Rc::new(Cell::new(false));
    let flag = Rc::clone(&connected);
    app.connect_activate(move |app| {
        if app.windows().iter().any(|window| window.is_visible()) {
            return;
        }
        let flag = Rc::clone(&flag);
        build_card(
            None,
            Some(app),
            SignInOptions {
                start_at_device: false,
                success_lede: t("Uploading your capture…"),
                on_success: Rc::new(move || flag.set(true)),
            },
        );
    });

    let _ = app.run_with_args::<String>(&[]);
    connected.get()
}

/// The GTK application currently running, when the card is opened from a
/// window rather than a standalone application of its own.
fn current_gtk_application() -> Option<Application> {
    gtk4::gio::Application::default()?
        .downcast::<Application>()
        .ok()
}

struct SignInOptions {
    start_at_device: bool,
    success_lede: String,
    on_success: Rc<dyn Fn()>,
}

fn build_card(parent: Option<&Window>, app: Option<&Application>, options: SignInOptions) {
    install_css();
    install_sign_in_css();

    let SignInOptions {
        start_at_device,
        success_lede,
        on_success,
    } = options;

    // Keep the card attached to the running application. A card opened over a
    // window that can close on its own (the preview after its timeout) must
    // hold the app open until the user dismisses it or the retry runs.
    let attached_app = app
        .cloned()
        .or_else(|| parent.and_then(|parent| parent.application()))
        .or_else(current_gtk_application);

    let mut builder = Window::builder()
        .decorated(false)
        .resizable(false)
        .default_width(px(480.0));
    if let Some(parent) = parent {
        builder = builder.transient_for(parent).modal(true);
    }
    if let Some(app) = &attached_app {
        builder = builder.application(app);
    }
    let window = builder.build();
    window.add_css_class("apexshot-studio-prompt");
    if parent.is_none() {
        window.set_title(Some(crate::app_identity::app_name()));
    } else {
        // The card survives a closing parent so a retry can still run.
        window.set_destroy_with_parent(false);
    }

    let dark = theme_is_dark(&window);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("apexshot-studio-card");
    if !dark {
        card.add_css_class(LIGHT_CLASS);
    }
    card.set_size_request(px(480.0), -1);
    card.set_overflow(gtk4::Overflow::Hidden);

    let hero = Overlay::new();
    hero.set_child(Some(&crate::dither_field::dither_field(dark, 150.0)));

    let words = GtkBox::new(Orientation::Vertical, px(8.0));
    words.set_valign(Align::End);
    words.set_margin_start(px(28.0));
    words.set_margin_end(px(28.0));
    words.set_margin_bottom(px(6.0));
    let headline = centered_label("", "apexshot-studio-headline");
    headline.add_css_class("apexshot-studio-headline-compact");
    headline.set_max_width_chars(26);
    let lede = centered_label("", "apexshot-studio-lede");
    lede.set_max_width_chars(52);
    words.append(&headline);
    words.append(&lede);
    hero.add_overlay(&words);

    let close = close_button();
    close.set_halign(Align::End);
    close.set_valign(Align::Start);
    close.set_margin_top(px(12.0));
    close.set_margin_end(px(12.0));
    hero.add_overlay(&close);
    card.append(&hero);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("apexshot-studio-body");
    card.append(&body);

    let intro = GtkBox::new(Orientation::Vertical, 0);
    let connect_btn = primary_button(&t("Connect account"));
    connect_btn.add_css_class("apexshot-studio-cta");
    connect_btn.set_halign(Align::Center);
    connect_btn.set_size_request(px(320.0), -1);
    let intro_links = GtkBox::new(Orientation::Horizontal, px(6.0));
    intro_links.set_halign(Align::Center);
    intro_links.set_margin_top(px(8.0));
    let not_now = link_button(&t("Not now"));
    intro_links.append(&not_now);
    intro.append(&connect_btn);
    intro.append(&intro_links);
    body.append(&intro);

    let device = GtkBox::new(Orientation::Vertical, 0);
    device.set_visible(false);
    let code_row = GtkBox::new(Orientation::Horizontal, px(6.0));
    code_row.add_css_class("apexshot-signin-code");
    code_row.set_halign(Align::Center);
    code_row.set_margin_top(px(18.0));
    code_row.set_tooltip_text(Some(&t("Copy code")));
    code_row.set_cursor_from_name(Some("pointer"));
    set_code_cells(&code_row, "    -    ");
    device.append(&code_row);

    let actions = GtkBox::new(Orientation::Horizontal, px(8.0));
    actions.set_halign(Align::Center);
    actions.set_margin_top(px(14.0));
    let open_btn = primary_button(&t("Open browser"));
    open_btn.set_sensitive(false);
    let copy_btn = link_button(&t("Copy code"));
    copy_btn.set_sensitive(false);
    actions.append(&open_btn);
    actions.append(&copy_btn);
    device.append(&actions);

    let browser_status = Label::new(None);
    browser_status.add_css_class("apexshot-signin-hint");
    browser_status.set_halign(Align::Center);
    browser_status.set_justify(gtk4::Justification::Center);
    browser_status.set_wrap(true);
    browser_status.set_max_width_chars(44);
    browser_status.set_margin_top(px(10.0));
    browser_status.set_visible(false);
    device.append(&browser_status);

    let footer = GtkBox::new(Orientation::Horizontal, px(8.0));
    footer.set_halign(Align::Center);
    footer.set_margin_top(px(14.0));
    let spinner = Spinner::new();
    spinner.set_valign(Align::Center);
    spinner.start();
    footer.append(&spinner);
    let status = Label::new(Some(&t("Starting…")));
    status.add_css_class("apexshot-signin-status");
    status.set_valign(Align::Center);
    status.set_wrap(true);
    status.set_max_width_chars(44);
    footer.append(&status);
    let expiry_box = GtkBox::new(Orientation::Horizontal, px(8.0));
    expiry_box.set_valign(Align::Center);
    expiry_box.set_visible(false);
    let dot = Label::new(Some("·"));
    dot.add_css_class("apexshot-signin-status");
    expiry_box.append(&dot);
    let expiry = Label::new(None);
    expiry.add_css_class("apexshot-signin-expiry");
    expiry_box.append(&expiry);
    footer.append(&expiry_box);
    device.append(&footer);
    body.append(&device);

    // The card is undecorated, so the whole card is the drag handle; its
    // buttons and links still take their own clicks.
    let handle = gtk4::WindowHandle::new();
    handle.set_child(Some(&card));
    window.set_child(Some(&handle));

    let ui = SignInUi {
        window: window.downgrade(),
        headline: headline.clone(),
        lede: lede.clone(),
        body: body.clone(),
        intro: intro.clone(),
        device: device.clone(),
        code_row: code_row.clone(),
        copy_btn: copy_btn.clone(),
        open_btn: open_btn.clone(),
        browser_status: browser_status.clone(),
        status: status.clone(),
        spinner: spinner.clone(),
        expiry: expiry.clone(),
        expiry_box: expiry_box.clone(),
        session: Rc::new(Cell::new(0)),
        polling: Rc::new(Cell::new(false)),
        primary_action: Rc::new(Cell::new(PrimaryAction::OpenBrowser)),
        code: Rc::new(RefCell::new(String::new())),
        uri: Rc::new(RefCell::new(String::new())),
        success_lede,
        on_success,
    };
    ui.wire_actions(&connect_btn, &not_now, &close);

    {
        let ui = ui.clone();
        window.connect_close_request(move |_| {
            ui.abandon();
            glib::Propagation::Proceed
        });
    }

    let keys = EventControllerKey::new();
    keys.set_propagation_phase(PropagationPhase::Capture);
    keys.connect_key_pressed({
        let window = window.downgrade();
        move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(window) = window.upgrade() {
                    window.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(keys);

    window.present();
    if start_at_device {
        ui.start();
    } else {
        ui.show_intro();
    }
}

#[derive(Clone)]
struct SignInUi {
    /// Weak so the window's close handler does not keep the card alive.
    window: glib::WeakRef<Window>,
    headline: Label,
    lede: Label,
    body: GtkBox,
    intro: GtkBox,
    device: GtkBox,
    code_row: GtkBox,
    copy_btn: Button,
    open_btn: Button,
    browser_status: Label,
    status: Label,
    spinner: Spinner,
    expiry: Label,
    expiry_box: GtkBox,
    /// Bumped on every (re)start, failure, and abandon, so callbacks from an
    /// older run stop touching the widgets.
    session: Rc<Cell<u64>>,
    polling: Rc<Cell<bool>>,
    primary_action: Rc<Cell<PrimaryAction>>,
    code: Rc<RefCell<String>>,
    uri: Rc<RefCell<String>>,
    success_lede: String,
    on_success: Rc<dyn Fn()>,
}

impl SignInUi {
    fn is_live(&self, session: u64) -> bool {
        self.session.get() == session
            && self
                .window
                .upgrade()
                .is_some_and(|window| window.is_visible())
    }

    fn wire_actions(&self, connect_btn: &Button, not_now: &Button, close: &Button) {
        {
            let ui = self.clone();
            connect_btn.connect_clicked(move |_| ui.start());
        }

        {
            let window = self.window.clone();
            not_now.connect_clicked(move |_| {
                if let Some(window) = window.upgrade() {
                    window.close();
                }
            });
        }

        {
            let window = self.window.clone();
            close.connect_clicked(move |_| {
                if let Some(window) = window.upgrade() {
                    window.close();
                }
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
            let ui = self.clone();
            let click = gtk4::GestureClick::new();
            click.connect_released(move |_, _, _, _| {
                if ui.copy_btn.is_sensitive() {
                    ui.copy_btn.emit_clicked();
                }
            });
            self.code_row.add_controller(click);
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
                        ui.open_browser(&target);
                    }
                    PrimaryAction::Retry => ui.start(),
                });
        }
    }

    fn show_intro(&self) {
        self.session.set(self.session.get() + 1);
        self.spinner.stop();
        self.body.set_visible(true);
        self.intro.set_visible(true);
        self.device.set_visible(false);
        self.headline.set_text(&t("Sign in to get a share link"));
        self.lede.set_text(&t(
            "Your capture is saved. Connect a free ApexShot account and every upload becomes a link in your clipboard.",
        ));
        self.primary_action.set(PrimaryAction::OpenBrowser);
    }

    fn show_device(&self) {
        self.body.set_visible(true);
        self.intro.set_visible(false);
        self.device.set_visible(true);
        self.headline.set_text(&t("Authorize this device"));
        self.lede
            .set_text(&t("Enter this code in your browser to link this device."));
    }

    /// (Re)start the flow: reset the widgets, bump the session so callbacks
    /// from an older run go dead, and request a fresh device code.
    fn start(&self) {
        let session = self.session.get() + 1;
        self.session.set(session);
        self.show_device();
        set_code_cells(&self.code_row, "    -    ");
        self.code.replace(String::new());
        self.uri.replace(String::new());
        self.copy_btn.set_sensitive(false);
        self.open_btn.set_sensitive(false);
        self.open_btn.set_label(&t("Open browser"));
        self.browser_status.set_visible(false);
        self.expiry_box.set_visible(false);
        self.expiry
            .remove_css_class("apexshot-signin-expiry-urgent");
        self.status.remove_css_class("apexshot-signin-error");
        self.status.set_text(&t("Starting…"));
        self.spinner.start();
        self.polling.set(false);
        self.primary_action.set(PrimaryAction::OpenBrowser);

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
                    ConnectMsg::Polled(Ok(Some(account))) => {
                        // The shell gates auto-upload on the mirror, so publish
                        // the new session as soon as the account lands.
                        #[cfg(target_os = "windows")]
                        if let Err(error) = crate::settings::actions::sync_shell_settings_on_start()
                        {
                            eprintln!("[cloud] could not refresh the shell settings: {error}");
                        }
                        ui.connected(&account);
                        break;
                    }
                    ConnectMsg::Polled(Ok(None)) => ui.polling.set(false),
                    ConnectMsg::Polled(Err(error)) => ui.failed(&error),
                }
            }
        });
    }

    /// End an in-flight login without changing which step is visible.
    fn abandon(&self) {
        self.session.set(self.session.get() + 1);
        self.spinner.stop();
        self.polling.set(false);
    }

    fn waiting(&self, login: &DeviceLogin) {
        set_code_cells(&self.code_row, &login.user_code);
        self.code.replace(login.user_code.clone());
        self.uri.replace(login.verification_uri.clone());
        self.copy_btn.set_sensitive(true);
        self.open_btn.set_sensitive(true);
        self.status.set_text(&t("Waiting for you to authorize…"));

        set_expiry(&self.expiry, login.expires_in_secs);
        self.expiry_box.set_visible(true);

        self.open_browser(&login.verification_uri);
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

        // Count down from the deadline itself rather than ticking a counter,
        // so a busy main loop never lets the shown time drift.
        let deadline = Instant::now() + Duration::from_secs(login.expires_in_secs);
        let countdown_ui = self.clone();
        glib::timeout_add_local(Duration::from_millis(250), move || {
            if !countdown_ui.is_live(session) {
                return glib::ControlFlow::Break;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            set_expiry(&countdown_ui.expiry, left.as_secs_f64().ceil() as u64);
            if left.is_zero() {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    fn failed(&self, error: &LoginError) {
        eprintln!("[cloud] sign-in failed: {error}");
        self.session.set(self.session.get() + 1);
        self.spinner.stop();
        self.polling.set(false);
        self.expiry_box.set_visible(false);
        self.copy_btn.set_sensitive(false);
        self.browser_status.set_visible(false);
        self.open_btn.set_sensitive(true);
        self.open_btn.set_label(&t("Try again"));
        self.status.add_css_class("apexshot-signin-error");
        self.status.set_text(&login_error_message(error));
        self.primary_action.set(PrimaryAction::Retry);
    }

    fn connected(&self, account: &CloudAccount) {
        self.session.set(self.session.get() + 1);
        self.spinner.stop();
        self.body.set_visible(false);
        let email = account.email.trim();
        if email.is_empty() {
            self.headline.set_text(&t("Connected"));
        } else {
            self.headline
                .set_text(&tfmt("Connected as {email}", &[("email", email)]));
        }
        self.lede.set_text(&self.success_lede);

        // Retry right away so closing the card early never drops the click;
        // the card only lingers to confirm the account.
        (self.on_success)();
        let session = self.session.get();
        let ui = self.clone();
        glib::timeout_add_local_once(Duration::from_millis(1500), move || {
            if ui.session.get() != session {
                return;
            }
            if let Some(window) = ui.window.upgrade() {
                window.close();
            }
        });
    }

    fn open_browser(&self, url: &str) {
        self.browser_status.set_visible(false);
        let url = url.to_string();
        let status = self.browser_status.downgrade();
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
            "apexshot-signin-dash"
        } else {
            "apexshot-signin-cell"
        });
        cell.set_valign(Align::Center);
        row.append(&cell);
    }
}

/// Time left on the code as `m:ss`.
fn format_countdown(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Show the time left on the code, and warn once a minute or less remains.
fn set_expiry(label: &Label, secs: u64) {
    label.set_text(&tfmt(
        "Expires in {time}",
        &[("time", &format_countdown(secs))],
    ));
    if secs <= 60 {
        label.add_css_class("apexshot-signin-expiry-urgent");
    } else {
        label.remove_css_class("apexshot-signin-expiry-urgent");
    }
}

/// The one-line reason a device-authorization sign-in failed.
fn login_error_message(error: &LoginError) -> String {
    match error {
        LoginError::Expired => t("This code expired. Try again to get a new one."),
        LoginError::Denied => t("Authorization was denied in the browser."),
        LoginError::NotConfigured | LoginError::HttpRequest(_) | LoginError::Server(_) => {
            t("Couldn't reach ApexShot Cloud. Check your connection and try again.")
        }
        LoginError::Local(_) => t("Signed in, but this device could not save the account."),
    }
}

const CSS: &str = r#"
    .apexshot-signin-code { margin-top: 2px; }
    .apexshot-signin-cell,
    .apexshot-signin-dash {
        font-family: 'Inter', 'JetBrains Mono', monospace;
        font-size: 24px;
        font-weight: 600;
        letter-spacing: 2px;
        padding: 8px 12px;
        min-width: 14px;
    }
    .apexshot-signin-cell {
        color: #f1f1f3;
        background: rgba(255, 255, 255, 0.06);
        border: 1px solid rgba(255, 255, 255, 0.10);
        border-radius: 8px;
    }
    .apexshot-signin-dash {
        padding: 8px 2px;
        color: rgba(255, 255, 255, 0.35);
    }
    .apexshot-signin-status {
        font-size: 13px;
        color: rgba(255, 255, 255, 0.62);
    }
    .apexshot-signin-error { color: #ff8b7a; }
    .apexshot-signin-expiry {
        font-size: 13px;
        color: rgba(255, 255, 255, 0.45);
    }
    .apexshot-signin-expiry-urgent { color: #ffb020; }
    .apexshot-signin-hint {
        font-size: 12px;
        color: rgba(255, 255, 255, 0.55);
    }
    .apexshot-studio-light .apexshot-signin-cell {
        color: #1d2129;
        background: rgba(17, 24, 39, 0.05);
        border-color: rgba(17, 24, 39, 0.12);
    }
    .apexshot-studio-light .apexshot-signin-dash { color: rgba(29, 33, 41, 0.30); }
    .apexshot-studio-light .apexshot-signin-status { color: rgba(29, 33, 41, 0.62); }
    .apexshot-studio-light .apexshot-signin-error { color: #b42318; }
    .apexshot-studio-light .apexshot-signin-expiry { color: rgba(29, 33, 41, 0.50); }
    .apexshot-studio-light .apexshot-signin-expiry-urgent { color: #b25c00; }
    .apexshot-studio-light .apexshot-signin-hint { color: rgba(29, 33, 41, 0.55); }
"#;

fn scaled_css() -> String {
    #[cfg(target_os = "windows")]
    {
        crate::css_scale::scale_px_lengths(CSS, crate::platform::ui_scale())
    }
    #[cfg(not(target_os = "windows"))]
    {
        CSS.to_owned()
    }
}

fn install_sign_in_css() {
    thread_local! {
        static INSTALLED: Cell<bool> = const { Cell::new(false) };
    }
    if INSTALLED.with(Cell::get) {
        return;
    }
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(&scaled_css());
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    INSTALLED.with(|installed| installed.set(true));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_out_apexshot_destination_needs_sign_in() {
        let config = AppConfig::default();
        assert!(needs_apexshot_sign_in(&config));
    }

    #[test]
    fn a_connected_apexshot_account_does_not_need_sign_in() {
        let config = AppConfig {
            cloud_api_token: "tok".to_string(),
            cloud_backend_url: "https://api.example".to_string(),
            ..AppConfig::default()
        };
        assert!(!needs_apexshot_sign_in(&config));
    }

    #[test]
    fn xbackbone_never_needs_an_apexshot_sign_in() {
        let config = AppConfig {
            cloud_destination: "xbackbone".to_string(),
            ..AppConfig::default()
        };
        assert!(!needs_apexshot_sign_in(&config));
    }

    #[test]
    fn the_countdown_reads_minutes_and_padded_seconds() {
        assert_eq!(format_countdown(900), "15:00");
        assert_eq!(format_countdown(61), "1:01");
        assert_eq!(format_countdown(5), "0:05");
        assert_eq!(format_countdown(0), "0:00");
    }
}
