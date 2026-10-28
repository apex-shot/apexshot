//! The account pages: sign-in and the browser device-code confirmation.
//!
//! The Windows build has no password form of its own. The account is linked
//! through the same device authorization the Linux build uses, so this page
//! opens the real sign-in in the browser, shows the code the site asks for, and
//! continues the moment the account is approved. The code boxes are the same
//! visual device as the rest of the flow, not a simulated email form.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, Orientation, Spinner};

use super::hero::{self, HeroScene};
use super::widgets::{
    brand, email_pill, feature_panel, ghost_button, label, primary_button, px, set_button_text,
    wrapped,
};
use super::{report_error, status_line, Shell, Step};
use crate::cloud::auth::{begin_device_login, poll_device_login, DeviceLogin, LoginError};
use crate::config::is_cloud_logged_in;
use crate::i18n::{t, tfmt};

enum AuthMsg {
    Started(Result<DeviceLogin, LoginError>),
    Authorized(String),
    Failed(String),
}

/// The split composition shared by both account pages: narrative on the left,
/// the drawn product preview on the right.
fn split_page(shell: &Shell) -> (GtkBox, GtkBox) {
    let page = GtkBox::new(Orientation::Horizontal, 0);
    page.set_hexpand(true);
    page.set_vexpand(true);

    let left = GtkBox::new(Orientation::Vertical, 0);
    left.add_css_class("ob-split-left");
    left.set_size_request(px(shell.scale(), 500.0), -1);
    left.set_hexpand(false);
    left.set_valign(Align::Fill);
    page.append(&left);

    let scene = Rc::new(RefCell::new(HeroScene::CameraAndVoice));
    let preview = hero::build(scene);
    preview.set_hexpand(true);
    preview.set_vexpand(true);
    preview.set_margin_top(px(shell.scale(), 10.0));
    preview.set_margin_bottom(px(shell.scale(), 10.0));
    preview.set_margin_end(px(shell.scale(), 10.0));
    preview.set_margin_start(px(shell.scale(), 6.0));
    page.append(&preview);

    (page, left)
}

pub(crate) fn build_account(shell: &Shell) -> GtkBox {
    let (page, left) = split_page(shell);
    let config = shell.config();
    let signed_in = is_cloud_logged_in(&config);

    left.append(&brand(shell));

    let gap = GtkBox::new(Orientation::Vertical, 0);
    gap.set_size_request(-1, px(shell.scale(), 62.0));
    left.append(&gap);

    let headline = label("Captures that look\nlike you meant it.", "ob-headline");
    headline.set_wrap(true);
    left.append(&headline);

    let copy = wrapped(
        "Screenshots, recordings and edits in one quiet place. Sign in to set up ApexShot on this PC.",
        "ob-lead",
        44,
    );
    copy.set_margin_top(px(shell.scale(), 18.0));
    left.append(&copy);

    let fill = GtkBox::new(Orientation::Vertical, 0);
    fill.set_vexpand(true);
    left.append(&fill);

    if signed_in {
        let pill = email_pill(shell, &config.cloud_user_email);
        pill.set_margin_bottom(px(shell.scale(), 14.0));
        left.append(&pill);
    } else {
        let panel = feature_panel(
            shell,
            &[
                "Free to capture, record and edit",
                "No subscription required",
                "Captures stay on this PC",
            ],
        );
        panel.set_margin_bottom(px(shell.scale(), 18.0));
        left.append(&panel);
    }

    let primary = primary_button(shell, "Continue", true);
    primary.set_halign(Align::Fill);
    {
        let shell_c = shell.clone();
        primary.connect_clicked(move |_| {
            if signed_in {
                shell_c.go(Step::Appearance);
            } else {
                shell_c.go(Step::Authorize);
            }
        });
    }
    left.append(&primary);
    shell.set_primary({
        let primary = primary.clone();
        Rc::new(move || primary.emit_clicked())
    });

    let fineprint = wrapped(
        if signed_in {
            "Your browser stays closed. Continue to finish setting up this PC."
        } else {
            "Opens your browser to sign in or create an account. Required on Windows."
        },
        "ob-fineprint",
        52,
    );
    fineprint.set_margin_top(px(shell.scale(), 12.0));
    left.append(&fineprint);

    if signed_in {
        let switch = ghost_button(shell, "Use a different account");
        switch.set_halign(Align::Start);
        switch.set_margin_top(px(shell.scale(), 6.0));
        let shell_c = shell.clone();
        switch.connect_clicked(move |_| shell_c.go(Step::Authorize));
        left.append(&switch);
    }

    page
}

pub(crate) fn build_authorize(shell: &Shell) -> GtkBox {
    let (page, left) = split_page(shell);
    let scale = shell.scale();
    let generation = shell.generation();
    let generation_handle = shell.generation_handle();

    let back = super::widgets::round_back_button(shell);
    {
        let shell_c = shell.clone();
        back.connect_clicked(move |_| shell_c.go(Step::Account));
    }
    left.append(&back);

    let gap = GtkBox::new(Orientation::Vertical, 0);
    gap.set_size_request(-1, px(scale, 32.0));
    left.append(&gap);

    left.append(&label("Confirm this PC.", "ob-headline"));

    let copy = wrapped(
        "Your browser is open. Sign in or create your account there, then approve this code to link ApexShot to this PC.",
        "ob-lead",
        46,
    );
    copy.set_margin_top(px(scale, 16.0));
    left.append(&copy);

    // The code the website asks for, in eight read-only cells.
    let code_row = GtkBox::new(Orientation::Horizontal, px(scale, 5.0));
    code_row.add_css_class("ob-code-row");
    code_row.set_halign(Align::Start);
    let mut cells = Vec::with_capacity(8);
    for _ in 0..8 {
        // One square per character: the label is the box, so the glyph is
        // centred on both axes instead of sitting at a wrapper's top edge.
        let cell = Label::new(Some("·"));
        cell.add_css_class("ob-code-box");
        cell.add_css_class("ob-code");
        cell.set_size_request(px(scale, 44.0), px(scale, 44.0));
        cell.set_xalign(0.5);
        cell.set_yalign(0.5);
        cell.set_halign(Align::Center);
        cell.set_valign(Align::Center);
        cell.set_opacity(0.28);
        code_row.append(&cell);
        cells.push(cell);
    }
    left.append(&code_row);

    let code_value = Rc::new(RefCell::new(String::new()));
    let copy_row = GtkBox::new(Orientation::Horizontal, px(scale, 12.0));
    copy_row.add_css_class("ob-copy-row");
    copy_row.set_halign(Align::Start);
    let copy_btn = ghost_button(shell, "Copy code");
    copy_btn.set_sensitive(false);
    {
        let code_value = Rc::clone(&code_value);
        let copy_btn_c = copy_btn.clone();
        copy_btn.connect_clicked(move |_| {
            let code = code_value.borrow().clone();
            if code.is_empty() {
                return;
            }
            if crate::utils::clipboard::copy_text_to_clipboard(&code).is_ok() {
                set_button_text(&copy_btn_c, &t("Copied"));
                let button = copy_btn_c.clone();
                glib::timeout_add_local_once(Duration::from_millis(1500), move || {
                    set_button_text(&button, &t("Copy code"));
                });
            }
        });
    }
    copy_row.append(&copy_btn);
    left.append(&copy_row);

    let status_row = GtkBox::new(Orientation::Horizontal, px(scale, 9.0));
    status_row.set_margin_top(px(scale, 18.0));
    status_row.set_halign(Align::Start);
    let spinner = Spinner::new();
    spinner.add_css_class("ob-spinner");
    spinner.set_visible(false);
    spinner.set_valign(Align::Center);
    status_row.append(&spinner);
    let status = status_line();
    status.set_text(&t("Starting sign-in…"));
    status.set_valign(Align::Center);
    status_row.append(&status);
    left.append(&status_row);

    let timer = Label::new(None);
    timer.add_css_class("ob-fineprint");
    timer.set_xalign(0.0);
    timer.set_halign(Align::Start);
    timer.set_margin_top(px(scale, 6.0));
    left.append(&timer);

    let link = Label::new(None);
    link.add_css_class("ob-fineprint");
    link.set_selectable(true);
    link.set_wrap(true);
    link.set_xalign(0.0);
    link.set_halign(Align::Start);
    link.set_visible(false);
    left.append(&link);

    let fill = GtkBox::new(Orientation::Vertical, 0);
    fill.set_vexpand(true);
    left.append(&fill);

    let open_btn = primary_button(shell, "Open browser", false);
    open_btn.set_halign(Align::Fill);
    open_btn.set_sensitive(false);
    left.append(&open_btn);
    shell.set_primary({
        let open_btn = open_btn.clone();
        Rc::new(move || open_btn.emit_clicked())
    });

    let restart = ghost_button(shell, "Start over");
    restart.set_halign(Align::Center);
    restart.set_margin_top(px(scale, 6.0));
    left.append(&restart);

    let justify = wrapped(
        "Keep this window open. ApexShot continues automatically once the browser confirms.",
        "ob-fineprint",
        52,
    );
    justify.set_margin_top(px(scale, 12.0));
    left.append(&justify);

    // --- device authorization ---
    let (sender, receiver) = std::sync::mpsc::channel::<AuthMsg>();
    let verification_url = Rc::new(RefCell::new(String::new()));

    {
        let sender = sender.clone();
        std::thread::spawn(move || {
            let _ = sender.send(AuthMsg::Started(begin_device_login()));
        });
    }

    {
        let shell = shell.clone();
        let cells = cells.clone();
        let status = status.clone();
        let spinner = spinner.clone();
        let open_btn = open_btn.clone();
        let copy_btn = copy_btn.clone();
        let restart = restart.clone();
        let timer = timer.clone();
        let link = link.clone();
        let code_value = Rc::clone(&code_value);
        let verification_url = Rc::clone(&verification_url);
        let generation_handle = generation_handle.clone();
        let sender = sender.clone();

        glib::timeout_add_local(Duration::from_millis(120), move || {
            let message = match receiver.try_recv() {
                Ok(message) => message,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    return if generation_handle.load(Ordering::SeqCst) == generation {
                        glib::ControlFlow::Continue
                    } else {
                        glib::ControlFlow::Break
                    };
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return glib::ControlFlow::Break
                }
            };

            if generation_handle.load(Ordering::SeqCst) != generation {
                return glib::ControlFlow::Break;
            }

            match message {
                AuthMsg::Started(Ok(login)) => {
                    let code: String = login
                        .user_code
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric())
                        .collect();
                    *code_value.borrow_mut() = code.clone();
                    for (index, cell) in cells.iter().enumerate() {
                        cell.set_text(&code.chars().nth(index).unwrap_or(' ').to_string());
                        cell.set_opacity(1.0);
                    }
                    copy_btn.set_sensitive(true);

                    let url = format!(
                        "{}{}code={}",
                        login.verification_uri,
                        if login.verification_uri.contains('?') {
                            "&"
                        } else {
                            "?"
                        },
                        code
                    );
                    *verification_url.borrow_mut() = url.clone();
                    match crate::utils::open::open_url(&url) {
                        Ok(()) => {
                            status.remove_css_class("ob-status-error");
                            status.set_text(&t("Waiting for your browser…"));
                        }
                        Err(error) => {
                            eprintln!("[onboarding] could not open the browser: {error}");
                            status.set_text(&t(
                                "Couldn't open your browser. Copy the address below into one.",
                            ));
                            link.set_text(&url);
                            link.set_visible(true);
                        }
                    }
                    spinner.set_visible(true);
                    spinner.start();
                    open_btn.set_sensitive(true);

                    // Countdown to expiry.
                    let deadline = Instant::now() + Duration::from_secs(login.expires_in_secs);
                    let weak_timer = timer.downgrade();
                    let countdown_generation = Arc::clone(&generation_handle);
                    glib::timeout_add_local(Duration::from_secs(1), move || {
                        let Some(timer) = weak_timer.upgrade() else {
                            return glib::ControlFlow::Break;
                        };
                        if countdown_generation.load(Ordering::SeqCst) != generation {
                            return glib::ControlFlow::Break;
                        }
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            timer.set_text(&t("That code expired. Start over to get a new one."));
                            return glib::ControlFlow::Break;
                        }
                        let seconds = remaining.as_secs();
                        timer.set_text(&tfmt(
                            "Code expires in {time}",
                            &[("time", &format!("{:02}:{:02}", seconds / 60, seconds % 60))],
                        ));
                        glib::ControlFlow::Continue
                    });

                    // Poll in one worker so a slow network never touches the UI.
                    let sender_poll = sender.clone();
                    let device_code = login.device_code.clone();
                    let interval = login.interval_secs.max(3);
                    let poll_generation = Arc::clone(&generation_handle);
                    std::thread::spawn(move || loop {
                        std::thread::sleep(Duration::from_secs(interval));
                        if poll_generation.load(Ordering::SeqCst) != generation {
                            return;
                        }
                        match poll_device_login(&device_code) {
                            Ok(None) => continue,
                            Ok(Some(account)) => {
                                let _ = sender_poll.send(AuthMsg::Authorized(account.email));
                                return;
                            }
                            Err(error) => {
                                let _ = sender_poll.send(AuthMsg::Failed(friendly(&error)));
                                return;
                            }
                        }
                    });
                    glib::ControlFlow::Continue
                }
                AuthMsg::Started(Err(error)) => {
                    eprintln!("[onboarding] could not start device login: {error:?}");
                    report_error(&status, &friendly(&error));
                    spinner.stop();
                    spinner.set_visible(false);
                    glib::ControlFlow::Break
                }
                AuthMsg::Authorized(email) => {
                    spinner.stop();
                    spinner.set_visible(false);
                    status.remove_css_class("ob-status-error");
                    status.add_css_class("ob-status-success");
                    if email.trim().is_empty() {
                        status.set_text(&t("Account confirmed. Setting up…"));
                    } else {
                        status.set_text(&tfmt(
                            "Signed in as {email}. Setting up…",
                            &[("email", email.trim())],
                        ));
                    }
                    open_btn.set_sensitive(false);
                    copy_btn.set_sensitive(false);
                    restart.set_sensitive(false);
                    let shell_c = shell.clone();
                    glib::timeout_add_local_once(Duration::from_millis(700), move || {
                        shell_c.reload_config();
                        shell_c.go(Step::Appearance);
                    });
                    glib::ControlFlow::Break
                }
                AuthMsg::Failed(message) => {
                    report_error(&status, &message);
                    spinner.stop();
                    spinner.set_visible(false);
                    glib::ControlFlow::Break
                }
            }
        });
    }

    {
        let url = Rc::clone(&verification_url);
        let status = status.clone();
        open_btn.connect_clicked(move |_| {
            let url = url.borrow().clone();
            if url.is_empty() {
                return;
            }
            if let Err(error) = crate::utils::open::open_url(&url) {
                eprintln!("[onboarding] could not open the browser: {error}");
                report_error(
                    &status,
                    &t("Couldn't open your browser. Copy the address below."),
                );
            }
        });
    }

    {
        let shell_c = shell.clone();
        restart.connect_clicked(move |_| shell_c.go(Step::Authorize));
    }

    page
}

/// Map the auth client's errors to copy a person can act on.
fn friendly(error: &LoginError) -> String {
    match error {
        LoginError::NotConfigured => t(
            "This build has no account server configured. Set a Cloud backend URL and try again.",
        ),
        LoginError::HttpRequest(_) => {
            t("Couldn't reach the account server. Check your connection and try again.")
        }
        LoginError::Expired => t("That code expired. Start over to get a new one."),
        LoginError::Denied => t("The browser cancelled the request. Start over when you're ready."),
        LoginError::Server(message) => tfmt(
            "The account server said: {message}",
            &[("message", message)],
        ),
        LoginError::Local(message) => tfmt(
            "Signed in, but this device could not save the account: {message}",
            &[("message", message)],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_errors_keep_the_server_message() {
        let message = friendly(&LoginError::Server("maintenance".to_string()));
        assert!(message.contains("maintenance"));
    }

    #[test]
    fn network_errors_ask_for_retry_not_for_a_config_change() {
        let message = friendly(&LoginError::HttpRequest("dns".to_string()));
        assert!(message.contains("connection"));
        assert!(!message.contains("backend"));
    }
}
