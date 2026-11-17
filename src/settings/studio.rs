use crate::i18n::{t, tfmt};
use crate::studio_entitlement::{
    activate_token, current_status, StudioEntitlementError, StudioStatus,
};
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, Entry, Image, Label, Orientation, Revealer,
    RevealerTransitionType,
};

/// Extra classes the status pill can carry. Kept together so a status change
/// after activation swaps the tint instead of stacking one variant on another.
const STATUS_PILL_CLASSES: [&str; 5] = [
    "studio-status-pill-active",
    "studio-status-pill-neutral",
    "studio-status-pill-warning",
    "studio-status-pill-attention",
    "studio-status-pill-muted",
];

pub fn build_studio_section() -> GtkBox {
    let status_snapshot = current_status();
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.set_halign(Align::Fill);
    section.set_valign(Align::Start);
    section.set_hexpand(true);
    section.set_margin_top(super::PAGE_TOP_MARGIN);
    section.set_margin_bottom(8);

    let (hero, hero_parts) = build_hero(status_snapshot);
    section.append(&hero);

    if !matches!(status_snapshot, StudioStatus::ConfigurationUnavailable) {
        section.append(&group_title(&t("License")));
        let frame = table_frame();
        frame.append(&build_device_id_row());

        match status_snapshot {
            StudioStatus::Active { .. } | StudioStatus::UpdateAccessEnded { .. } => {
                if crate::studio_entitlement::stored_token().is_some() {
                    frame.append(&build_license_key_row());
                }

                let (replace_row, revealer) =
                    build_replace_row(&hero_parts.message, &hero_parts.pill, &hero_parts.action);
                frame.append(&replace_row);
                frame.append(&revealer);
            }
            StudioStatus::NotActivated
            | StudioStatus::InvalidEntitlement
            | StudioStatus::DeviceMismatch => {
                frame.append(&build_activate_row(
                    &hero_parts.message,
                    &hero_parts.pill,
                    &hero_parts.action,
                ));
            }
            StudioStatus::ConfigurationUnavailable => {}
        }

        section.append(&frame);
    }

    section
}

/// Widgets the activation form updates in place when the licence state changes.
struct HeroParts {
    message: Label,
    pill: Label,
    action: Button,
}

/// The card at the top of the page: the product name with its current
/// state, one sentence about that state, and the action that belongs with it.
fn build_hero(status: StudioStatus) -> (GtkBox, HeroParts) {
    let hero = GtkBox::new(Orientation::Horizontal, 14);
    hero.add_css_class("studio-hero");
    hero.set_halign(Align::Fill);
    hero.set_valign(Align::Start);
    hero.set_margin_start(4);
    hero.set_margin_end(4);

    let text = GtkBox::new(Orientation::Vertical, 4);
    text.set_hexpand(true);
    text.set_valign(Align::Center);

    let title_row = GtkBox::new(Orientation::Horizontal, 8);
    title_row.set_valign(Align::Center);
    let title = Label::new(Some(&t("ApexShot Studio")));
    title.add_css_class("studio-hero-title");
    title.set_xalign(0.0);
    title_row.append(&title);

    let (pill_text, pill_class) = status_pill(status);
    let pill = Label::new(Some(&pill_text));
    pill.add_css_class("studio-status-pill");
    pill.add_css_class(pill_class);
    pill.set_valign(Align::Center);
    title_row.append(&pill);
    text.append(&title_row);

    let message = Label::new(Some(&short_message(status)));
    message.add_css_class("settings-sub-option");
    message.set_wrap(true);
    message.set_xalign(0.0);
    message.set_halign(Align::Fill);
    text.append(&message);

    if matches!(status, StudioStatus::NotActivated) {
        text.append(&build_offer_meta());
    }

    hero.append(&text);

    let action = Button::with_label(&studio_page_action_label(status));
    action.set_valign(Align::Center);
    action.set_halign(Align::End);
    if matches!(status, StudioStatus::NotActivated) {
        action.add_css_class("settings-primary-btn");
    }
    action.connect_clicked(|_| {
        let _ = crate::utils::open::open_url("https://apexshot.org/studio");
    });
    hero.append(&action);

    (
        hero,
        HeroParts {
            message,
            pill,
            action,
        },
    )
}

/// The purchase terms, quiet and inline: the long version that used to be the
/// button label now reads as three facts under the state sentence.
fn build_offer_meta() -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("studio-hero-meta");
    row.set_halign(Align::Start);
    row.set_valign(Align::Center);
    row.set_margin_top(4);

    let items = [t("$39 once"), t("12 months of updates"), t("14-day refund")];
    for (index, item_text) in items.iter().enumerate() {
        if index > 0 {
            let dot = GtkBox::new(Orientation::Horizontal, 0);
            dot.add_css_class("studio-hero-meta-dot");
            dot.set_valign(Align::Center);
            row.append(&dot);
        }
        let item = GtkBox::new(Orientation::Horizontal, 5);
        item.set_valign(Align::Center);
        let check = Image::from_icon_name(
            crate::capture::editor::window::icon_names::custom::CHECK_SYMBOLIC,
        );
        check.set_pixel_size(13);
        check.set_valign(Align::Center);
        item.append(&check);
        item.append(&Label::new(Some(item_text.as_str())));
        row.append(&item);
    }
    row
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

fn row_title(text: &str) -> Label {
    let title = Label::new(Some(text));
    title.add_css_class("studio-row-title");
    title.set_xalign(0.0);
    title.set_halign(Align::Start);
    title
}

/// A copy action that reports success on the button itself for two seconds.
fn build_copy_button(resting: &str, value: impl Fn() -> Option<String> + 'static) -> Button {
    let button = Button::with_label(resting);
    button.set_valign(Align::Center);
    let resting = resting.to_string();
    button.connect_clicked(move |button| {
        let Some(text) = value() else {
            return;
        };
        if crate::utils::clipboard::copy_text_to_clipboard(&text).is_ok() {
            button.set_label(&t("Copied"));
            let button = button.clone();
            let resting = resting.clone();
            gtk4::glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                button.set_label(&resting);
            });
        }
    });
    button
}

struct ActivationControls {
    token: Entry,
    activate: Button,
    feedback: Label,
}

fn build_activation_controls(
    status: &Label,
    pill: &Label,
    studio_page: &Button,
    on_activated: impl Fn() + 'static,
) -> ActivationControls {
    let token = Entry::new();
    token.add_css_class("studio-token-entry");
    token.set_hexpand(true);
    token.set_placeholder_text(Some(&t("Paste your license key")));

    let feedback = Label::new(Some(""));
    feedback.add_css_class("studio-feedback");
    feedback.set_wrap(true);
    feedback.set_xalign(0.0);
    // The result line only appears once a submit reports something, so the form
    // does not reserve a blank line under the buttons.
    feedback.set_visible(false);

    let activate = Button::with_label(&t("Activate"));
    activate.add_css_class("settings-primary-btn");
    activate.set_valign(Align::Center);

    activate.connect_clicked({
        let token = token.clone();
        let status = status.clone();
        let pill = pill.clone();
        let feedback = feedback.clone();
        let studio_page = studio_page.clone();
        move |_| match activate_token(&token.text()) {
            Ok(()) => {
                token.set_text("");
                let status_snapshot = current_status();
                status.set_text(&short_message(status_snapshot));
                refresh_status_pill(&pill, status_snapshot);
                apply_action_label(&studio_page, status_snapshot);
                let feedback_message = match status_snapshot {
                    StudioStatus::Active { .. } => t("Studio activated for this build."),
                    StudioStatus::UpdateAccessEnded { .. } => t(
                        "Entitlement saved. This build isn't covered by the included updates; install a covered build to use Studio output.",
                    ),
                    other => other.message(),
                };
                feedback.set_text(&feedback_message);
                feedback.set_visible(true);
                on_activated();
            }
            Err(error) => {
                status.set_text(&short_message(current_status()));
                feedback.set_text(&match error {
                    StudioEntitlementError::DeviceMismatch => StudioStatus::DeviceMismatch.message(),
                    error => error.to_string(),
                });
                feedback.set_visible(true);
            }
        }
    });

    token.connect_activate({
        let activate = activate.clone();
        move |_| activate.emit_clicked()
    });

    ActivationControls {
        token,
        activate,
        feedback,
    }
}

/// The value a buyer pastes on apexshot.org/studio when claiming a licence for
/// this machine. Shown even before activation, because it is needed first.
fn build_device_id_row() -> GtkBox {
    let row = table_row();

    let text = GtkBox::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.set_valign(Align::Center);
    text.append(&row_title(&t("Device ID")));

    let device_id = crate::studio_entitlement::device_id();
    let value = Label::new(Some(
        &device_id
            .clone()
            .unwrap_or_else(|| t("Unavailable on this system")),
    ));
    value.add_css_class("settings-mono-value");
    value.add_css_class("settings-sub-option");
    value.set_selectable(true);
    value.set_xalign(0.0);
    value.set_halign(Align::Start);
    text.append(&value);

    let copy = build_copy_button(&t("Copy"), crate::studio_entitlement::device_id);
    copy.set_sensitive(device_id.is_some());

    row.append(&text);
    row.append(&copy);
    row
}

/// The saved key itself, offered so it can be carried to another machine.
fn build_license_key_row() -> GtkBox {
    let row = table_row();

    let text = GtkBox::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.set_valign(Align::Center);
    text.append(&row_title(&t("License key")));

    let copy = build_copy_button(&t("Copy license"), crate::studio_entitlement::stored_token);

    row.append(&text);
    row.append(&copy);
    row
}

/// A row whose control opens the activation form underneath the row, so an
/// activated installation still reads as a card rather than a form.
fn build_replace_row(status: &Label, pill: &Label, studio_page: &Button) -> (GtkBox, Revealer) {
    let row = table_row();

    let text = GtkBox::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.set_valign(Align::Center);
    text.append(&row_title(&t("Replace license")));

    let revealer = Revealer::new();
    revealer.set_transition_type(RevealerTransitionType::SlideDown);
    revealer.set_transition_duration(200);
    revealer.set_reveal_child(false);

    let replace = Button::with_label(&t("Replace…"));
    replace.set_valign(Align::Center);
    replace.connect_clicked({
        let revealer = revealer.clone();
        move |_| revealer.set_reveal_child(!revealer.reveals_child())
    });

    row.append(&text);
    row.append(&replace);

    let controls = build_activation_controls(status, pill, studio_page, {
        let revealer = revealer.clone();
        move || revealer.set_reveal_child(false)
    });

    let body = GtkBox::new(Orientation::Vertical, 8);
    body.add_css_class("studio-reveal-body");
    body.append(&controls.token);
    controls.activate.set_halign(Align::End);
    body.append(&controls.activate);
    body.append(&controls.feedback);
    revealer.set_child(Some(&body));

    (row, revealer)
}

/// The form for an installation that has no working key yet. It stays open:
/// there is nothing else useful on the page to do first.
fn build_activate_row(status: &Label, pill: &Label, studio_page: &Button) -> GtkBox {
    let row = GtkBox::new(Orientation::Vertical, 8);
    row.add_css_class("settings-table-row");
    row.add_css_class("studio-activate-row");
    row.append(&row_title(&t("Activate a license")));

    let controls = build_activation_controls(status, pill, studio_page, || {});
    row.append(&controls.token);
    controls.activate.set_halign(Align::End);
    row.append(&controls.activate);
    row.append(&controls.feedback);
    row
}

/// The label and tint the state pill wears for each licence state.
fn status_pill(status: StudioStatus) -> (String, &'static str) {
    match status {
        StudioStatus::Active { .. } => (t("Active"), "studio-status-pill-active"),
        StudioStatus::NotActivated => (t("Free"), "studio-status-pill-neutral"),
        StudioStatus::UpdateAccessEnded { .. } => {
            (t("Updates ended"), "studio-status-pill-warning")
        }
        StudioStatus::InvalidEntitlement => (t("Needs attention"), "studio-status-pill-attention"),
        StudioStatus::DeviceMismatch => (t("Other device"), "studio-status-pill-attention"),
        StudioStatus::ConfigurationUnavailable => (t("Unavailable"), "studio-status-pill-muted"),
    }
}

fn refresh_status_pill(pill: &Label, status: StudioStatus) {
    for class in STATUS_PILL_CLASSES {
        pill.remove_css_class(class);
    }
    let (label, class) = status_pill(status);
    pill.set_text(&label);
    pill.add_css_class(class);
}

fn apply_action_label(button: &Button, status: StudioStatus) {
    button.set_label(&studio_page_action_label(status));
    if matches!(status, StudioStatus::NotActivated) {
        button.add_css_class("settings-primary-btn");
    } else {
        button.remove_css_class("settings-primary-btn");
    }
}

/// One line for the summary card. The full explanations stay in
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

fn studio_page_action_label(status: StudioStatus) -> String {
    match status {
        StudioStatus::NotActivated => t("Upgrade to Studio"),
        StudioStatus::InvalidEntitlement => t("Recover license"),
        StudioStatus::DeviceMismatch => t("Claim for this device"),
        StudioStatus::UpdateAccessEnded { .. } | StudioStatus::Active { .. } => t("Manage license"),
        StudioStatus::ConfigurationUnavailable => t("Check Studio availability"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_page_routes_each_license_state_without_a_repurchase_pitch() {
        assert_eq!(
            studio_page_action_label(StudioStatus::NotActivated),
            t("Upgrade to Studio")
        );
        assert_eq!(
            studio_page_action_label(StudioStatus::InvalidEntitlement),
            t("Recover license")
        );
        assert_eq!(
            studio_page_action_label(StudioStatus::UpdateAccessEnded { updates_until: 1 }),
            t("Manage license")
        );
        assert_eq!(
            studio_page_action_label(StudioStatus::Active { updates_until: 2 }),
            t("Manage license")
        );
        assert_eq!(
            studio_page_action_label(StudioStatus::ConfigurationUnavailable),
            t("Check Studio availability")
        );
    }

    #[test]
    fn status_pills_name_each_license_state() {
        assert_eq!(
            status_pill(StudioStatus::Active { updates_until: 1 }),
            (t("Active"), "studio-status-pill-active")
        );
        assert_eq!(
            status_pill(StudioStatus::NotActivated),
            (t("Free"), "studio-status-pill-neutral")
        );
        assert_eq!(
            status_pill(StudioStatus::UpdateAccessEnded { updates_until: 1 }),
            (t("Updates ended"), "studio-status-pill-warning")
        );
        assert_eq!(
            status_pill(StudioStatus::InvalidEntitlement),
            (t("Needs attention"), "studio-status-pill-attention")
        );
        assert_eq!(
            status_pill(StudioStatus::DeviceMismatch),
            (t("Other device"), "studio-status-pill-attention")
        );
        assert_eq!(
            status_pill(StudioStatus::ConfigurationUnavailable),
            (t("Unavailable"), "studio-status-pill-muted")
        );
    }
}
