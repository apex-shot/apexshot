//! The Windows Studio page, in the same group-and-table layout as the other
//! Settings pages: one row for where this device stands, then the license.
//!
//! The page rebuilds itself from the stored entitlement after an activation,
//! so every row always describes the same state.

use crate::i18n::{t, tfmt};
use crate::studio_entitlement::{
    activate_token, current_status, StudioEntitlementError, StudioStatus,
};
use gtk4::prelude::*;
use gtk4::{glib, Align, Box as GtkBox, Button, Entry, Label, Orientation, Revealer};
use std::cell::Cell;
use std::rc::Rc;

const STUDIO_URL: &str = "https://apexshot.org/studio";

pub fn build_studio_section() -> GtkBox {
    install_css();
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_halign(Align::Fill);
    root.set_valign(Align::Start);
    root.set_hexpand(true);
    root.set_margin_top(super::PAGE_TOP_MARGIN);
    root.set_margin_bottom(8);
    render(&root, None);
    root
}

/// Replace the page's contents with the current license state. `flash` is a
/// one-off result shown under the status row, such as a fresh activation.
fn render(root: &GtkBox, flash: Option<String>) {
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }
    let status = current_status();

    root.append(&group_title(&t("Status")));
    let frame = table_frame();
    frame.append(&status_row(status, flash));
    root.append(&frame);

    if matches!(status, StudioStatus::ConfigurationUnavailable) {
        return;
    }

    root.append(&group_title(&t("License")));
    let frame = table_frame();
    match status {
        StudioStatus::Active { .. } | StudioStatus::UpdateAccessEnded { .. } => {
            if crate::studio_entitlement::stored_token().is_some() {
                let row = table_row();
                row.append(&row_text(&t("License key"), None));
                row.append(&copy_button(
                    &t("Copy license"),
                    crate::studio_entitlement::stored_token,
                ));
                frame.append(&row);
            }
            let row = table_row();
            row.append(&row_text(&t("Replace license"), None));
            let revealer = Revealer::new();
            let replace = Button::with_label(&t("Replace…"));
            replace.set_valign(Align::Center);
            replace.connect_clicked({
                let revealer = revealer.clone();
                move |_| revealer.set_reveal_child(!revealer.reveals_child())
            });
            row.append(&replace);
            frame.append(&row);
            let form = key_form(root);
            form.add_css_class("settings-table-row");
            revealer.set_child(Some(&form));
            frame.append(&revealer);
        }
        _ => {
            let form = key_form(root);
            form.add_css_class("settings-table-row");
            frame.append(&form);
        }
    }
    frame.append(&device_row());
    root.append(&frame);
}

fn group_title(text: &str) -> Label {
    let title = Label::new(Some(text));
    title.add_css_class("settings-group-title");
    title.set_xalign(0.0);
    title.set_halign(Align::Start);
    title
}

fn table_frame() -> GtkBox {
    let frame = GtkBox::new(Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.set_margin_start(4);
    frame.set_margin_end(4);
    frame
}

fn table_row() -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.add_css_class("settings-table-row");
    row.set_hexpand(true);
    row
}

fn row_text(title: &str, hint: Option<&str>) -> GtkBox {
    let text = GtkBox::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.set_valign(Align::Center);
    let name = Label::new(Some(title));
    name.set_xalign(0.0);
    text.append(&name);
    if let Some(hint) = hint {
        let hint = Label::new(Some(hint));
        hint.add_css_class("settings-sub-option-hint");
        hint.set_wrap(true);
        hint.set_xalign(0.0);
        text.append(&hint);
    }
    text
}

/// "ApexShot Studio" with its state pill, one short sentence, and the one
/// action that fits the state.
fn status_row(status: StudioStatus, flash: Option<String>) -> GtkBox {
    let row = table_row();
    let text = GtkBox::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.set_valign(Align::Center);

    let title_line = GtkBox::new(Orientation::Horizontal, 8);
    let name = Label::new(Some(&t("ApexShot Studio")));
    name.set_xalign(0.0);
    title_line.append(&name);
    let (pill_text, pill_class) = status_pill(status);
    let pill = Label::new(Some(&pill_text));
    pill.add_css_class("studio-pill");
    pill.add_css_class(pill_class);
    pill.set_valign(Align::Center);
    title_line.append(&pill);
    text.append(&title_line);

    let message = Label::new(Some(&short_message(status)));
    message.add_css_class("settings-sub-option-hint");
    message.set_wrap(true);
    message.set_xalign(0.0);
    text.append(&message);

    if let Some(flash) = flash {
        let note = Label::new(Some(&flash));
        note.add_css_class("studio-flash");
        note.set_wrap(true);
        note.set_xalign(0.0);
        text.append(&note);
    }
    row.append(&text);

    let action = Button::with_label(&action_label(status));
    action.set_valign(Align::Center);
    if matches!(status, StudioStatus::NotActivated) {
        action.add_css_class("settings-primary-btn");
        action.set_tooltip_text(Some(&format!(
            "{} · {} · {}",
            t("$39 once"),
            t("12 months of updates"),
            t("14-day refund")
        )));
    }
    action.connect_clicked(|_| {
        std::thread::spawn(|| {
            if let Err(error) = crate::utils::open::open_url(STUDIO_URL) {
                eprintln!("[studio] Could not open the Studio page: {error}");
            }
        });
    });
    row.append(&action);
    row
}

/// The key field with its Activate button on one line, and the result of
/// the last attempt underneath.
fn key_form(root: &GtkBox) -> GtkBox {
    let form = GtkBox::new(Orientation::Vertical, 8);
    let line = GtkBox::new(Orientation::Horizontal, 12);
    let label = Label::new(Some(&t("License key")));
    label.set_valign(Align::Center);
    line.append(&label);
    let entry = Entry::new();
    entry.set_hexpand(true);
    entry.set_placeholder_text(Some(&t("Paste your license key")));
    line.append(&entry);
    let activate = Button::with_label(&t("Activate"));
    activate.add_css_class("settings-primary-btn");
    activate.set_valign(Align::Center);
    activate.set_sensitive(false);
    line.append(&activate);
    form.append(&line);

    let feedback = Label::new(None);
    feedback.add_css_class("studio-error");
    feedback.set_wrap(true);
    feedback.set_xalign(0.0);
    feedback.set_visible(false);
    form.append(&feedback);

    entry.connect_changed({
        let activate = activate.clone();
        let feedback = feedback.clone();
        move |entry| {
            activate.set_sensitive(!entry.text().trim().is_empty());
            feedback.set_visible(false);
        }
    });
    activate.connect_clicked({
        let entry = entry.clone();
        let root = root.downgrade();
        move |_| match activate_token(&entry.text()) {
            Ok(()) => {
                let message = match current_status() {
                    StudioStatus::Active { .. } => t("Studio activated for this build."),
                    StudioStatus::UpdateAccessEnded { .. } => t(
                        "Entitlement saved. This build isn't covered by the included updates; install a covered build to use Studio output.",
                    ),
                    other => other.message(),
                };
                if let Some(root) = root.upgrade() {
                    // Rebuilding drops this form; let the click finish first.
                    glib::idle_add_local_once(move || render(&root, Some(message)));
                }
            }
            Err(error) => {
                feedback.set_text(&match error {
                    StudioEntitlementError::DeviceMismatch => {
                        StudioStatus::DeviceMismatch.message()
                    }
                    error => error.to_string(),
                });
                feedback.set_visible(true);
            }
        }
    });
    entry.connect_activate({
        let activate = activate.clone();
        move |_| {
            if activate.is_sensitive() {
                activate.emit_clicked();
            }
        }
    });
    form
}

/// This machine's Device ID, which a buyer pastes when claiming a license.
fn device_row() -> GtkBox {
    let row = table_row();
    let device_id = crate::studio_entitlement::device_id();
    let value = device_id
        .clone()
        .unwrap_or_else(|| t("Unavailable on this system"));
    let text = row_text(&t("Device ID"), Some(&value));
    if let Some(hint) = text.last_child() {
        hint.add_css_class("studio-mono");
        if let Ok(hint) = hint.downcast::<Label>() {
            hint.set_selectable(true);
        }
    }
    row.append(&text);
    let copy = copy_button(&t("Copy"), crate::studio_entitlement::device_id);
    copy.set_sensitive(device_id.is_some());
    copy.set_tooltip_text(Some(&t(
        "Paste this at apexshot.org/studio to claim a Studio licence for this device.",
    )));
    row.append(&copy);
    row
}

/// A copy action that confirms on the button itself for two seconds.
fn copy_button(resting: &str, value: impl Fn() -> Option<String> + 'static) -> Button {
    let button = Button::with_label(resting);
    button.set_valign(Align::Center);
    let resting = resting.to_string();
    let pending = Rc::new(Cell::new(0_u32));
    button.connect_clicked(move |button| {
        let Some(text) = value() else {
            return;
        };
        if crate::utils::clipboard::copy_text_via_windows_clipboard(&text)
            .or_else(|_| crate::utils::clipboard::copy_text_to_clipboard(&text))
            .is_err()
        {
            return;
        }
        button.set_label(&t("Copied"));
        let generation = pending.get().wrapping_add(1);
        pending.set(generation);
        let button = button.downgrade();
        let resting = resting.clone();
        let pending = pending.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
            if pending.get() == generation {
                if let Some(button) = button.upgrade() {
                    button.set_label(&resting);
                }
            }
        });
    });
    button
}

fn status_pill(status: StudioStatus) -> (String, &'static str) {
    match status {
        StudioStatus::Active { .. } => (t("Active"), "studio-pill-active"),
        StudioStatus::NotActivated => (t("Free"), "studio-pill-neutral"),
        StudioStatus::UpdateAccessEnded { .. } => (t("Updates ended"), "studio-pill-warning"),
        StudioStatus::InvalidEntitlement => (t("Needs attention"), "studio-pill-attention"),
        StudioStatus::DeviceMismatch => (t("Other device"), "studio-pill-attention"),
        StudioStatus::ConfigurationUnavailable => (t("Unavailable"), "studio-pill-neutral"),
    }
}

fn action_label(status: StudioStatus) -> String {
    match status {
        StudioStatus::NotActivated => t("Upgrade to Studio"),
        StudioStatus::InvalidEntitlement => t("Recover license"),
        StudioStatus::DeviceMismatch => t("Claim for this device"),
        StudioStatus::UpdateAccessEnded { .. } | StudioStatus::Active { .. } => t("Manage license"),
        StudioStatus::ConfigurationUnavailable => t("Check Studio availability"),
    }
}

/// One short sentence per state; the long explanations stay in
/// `StudioStatus::message` for the places that have room for them.
fn short_message(status: StudioStatus) -> String {
    let date = |timestamp: i64| {
        chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0)
            .map(|date| date.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| timestamp.to_string())
    };
    match status {
        StudioStatus::Active { updates_until } => tfmt(
            "Updates included through {date}.",
            &[("date", &date(updates_until))],
        ),
        StudioStatus::UpdateAccessEnded { updates_until } => tfmt(
            "Included updates ended {date}.",
            &[("date", &date(updates_until))],
        ),
        StudioStatus::NotActivated => t("Not activated on this device."),
        StudioStatus::InvalidEntitlement => t("The saved license couldn't be verified."),
        StudioStatus::DeviceMismatch => t("This license belongs to another device."),
        StudioStatus::ConfigurationUnavailable => t("This build can't verify Studio licenses."),
    }
}

fn install_css() {
    thread_local! {
        static INSTALLED: Cell<bool> = const { Cell::new(false) };
    }
    if INSTALLED.with(|installed| installed.replace(true)) {
        return;
    }
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(&crate::css_scale::scale_px_lengths(
        include_str!("studio.css"),
        crate::platform::ui_scale(),
    ));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 5,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_a_message_and_an_action() {
        for status in [
            StudioStatus::NotActivated,
            StudioStatus::Active { updates_until: 1 },
            StudioStatus::UpdateAccessEnded { updates_until: 1 },
            StudioStatus::InvalidEntitlement,
            StudioStatus::DeviceMismatch,
            StudioStatus::ConfigurationUnavailable,
        ] {
            assert!(!short_message(status).is_empty(), "{status:?}");
            assert!(!action_label(status).is_empty(), "{status:?}");
        }
        assert_eq!(
            action_label(StudioStatus::NotActivated),
            t("Upgrade to Studio")
        );
    }

    #[test]
    fn studio_styles_parse() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let provider = gtk4::CssProvider::new();
            let errors = Rc::new(Cell::new(0));
            provider.connect_parsing_error({
                let errors = errors.clone();
                move |_, _, error| {
                    eprintln!("{error}");
                    errors.set(errors.get() + 1);
                }
            });
            provider.load_from_data(include_str!("studio.css"));
            assert_eq!(errors.get(), 0);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
