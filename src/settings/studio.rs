use crate::i18n::t;
use crate::studio_entitlement::{
    activate_token, current_status, StudioEntitlementError, StudioStatus,
};
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Entry, Expander, Label, Orientation};

pub fn build_studio_section() -> GtkBox {
    let status_snapshot = current_status();
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.set_halign(Align::Fill);
    section.set_valign(Align::Start);
    section.set_hexpand(true);
    section.set_margin_top(super::PAGE_TOP_MARGIN);
    section.set_margin_bottom(8);

    let intro = Label::new(Some(&t(
        "Studio is optional. Your free capture, editing, recording, projects, and clean exports remain available without an account or network connection.",
    )));
    intro.add_css_class("dim-label");
    intro.set_wrap(true);
    intro.set_xalign(0.0);
    intro.set_halign(Align::Fill);
    intro.set_margin_bottom(4);
    section.append(&intro);

    // --- Status ---
    section.append(&group_title(&t("Status")));
    let status_frame = table_frame();
    let status_row = GtkBox::new(Orientation::Horizontal, 12);
    status_row.add_css_class("settings-table-row");
    status_row.set_hexpand(true);

    let status_text = GtkBox::new(Orientation::Vertical, 4);
    status_text.set_hexpand(true);
    status_text.set_valign(Align::Center);
    if matches!(status_snapshot, StudioStatus::Active { .. }) {
        let pill = Label::new(Some(&t("Active")));
        pill.add_css_class("settings-status-pill");
        pill.set_halign(Align::Start);
        status_text.append(&pill);
    }
    let status = Label::new(Some(&status_snapshot.message()));
    status.add_css_class("settings-sub-option");
    status.set_wrap(true);
    status.set_xalign(0.0);
    status_text.append(&status);
    status_row.append(&status_text);

    // The action that belongs with the status sits at the row's right edge,
    // the same place the Cloud page keeps its Connect button.
    let status_actions = GtkBox::new(Orientation::Horizontal, 8);
    status_actions.set_valign(Align::Center);
    status_row.append(&status_actions);
    status_frame.append(&status_row);

    match status_snapshot {
        StudioStatus::ConfigurationUnavailable => {
            let guidance = Label::new(Some(&t(
                "This build can't verify Studio licenses. Check the official Studio page for build availability or license recovery; buying a license alone can't enable this build.",
            )));
            guidance.add_css_class("dim-label");
            guidance.set_wrap(true);
            guidance.set_xalign(0.0);
            status_text.append(&guidance);

            let studio_page = Button::with_label(&studio_page_action_label(status_snapshot));
            studio_page.connect_clicked(|_| {
                let _ = crate::utils::open::open_url("https://apexshot.org/studio");
            });
            status_actions.append(&studio_page);
        }
        StudioStatus::Active { .. } | StudioStatus::UpdateAccessEnded { .. } => {
            // A saved license that already works: keep the status and the way to
            // open the purchase page visible, and tuck the paste box behind a
            // disclosure so an activated installation doesn't read like a form
            // that still needs filling in.
            let studio_page = Button::with_label(&studio_page_action_label(status_snapshot));
            status_actions.append(&studio_page);
            if crate::studio_entitlement::stored_token().is_some() {
                let copy = Button::with_label(&t("Copy license"));
                copy.connect_clicked(|button| {
                    let Some(token) = crate::studio_entitlement::stored_token() else {
                        return;
                    };
                    if crate::utils::clipboard::copy_text_to_clipboard(&token).is_ok() {
                        button.set_label(&t("Copied"));
                        let button = button.clone();
                        gtk4::glib::timeout_add_local_once(
                            std::time::Duration::from_secs(2),
                            move || button.set_label(&t("Copy license")),
                        );
                    }
                });
                status_actions.append(&copy);
            }

            let replacement = Expander::new(Some(&t("Replace license")));
            replacement.set_expanded(false);
            let controls = build_activation_controls(&status, &studio_page, {
                let replacement = replacement.clone();
                move || replacement.set_expanded(false)
            });

            let replacement_body = GtkBox::new(Orientation::Vertical, 8);
            replacement_body.set_margin_top(8);
            replacement_body.append(&controls.hint);
            replacement_body.append(&controls.token);
            // The submit closes the form, so it sits a gap further down than
            // the form's own line spacing.
            controls.activate.set_margin_top(8);
            replacement_body.append(&controls.activate);
            replacement_body.append(&controls.feedback);
            replacement.set_child(Some(&replacement_body));

            let replace_row = GtkBox::new(Orientation::Vertical, 0);
            replace_row.add_css_class("settings-table-row");
            replace_row.append(&replacement);
            status_frame.append(&replace_row);
        }
        StudioStatus::NotActivated
        | StudioStatus::InvalidEntitlement
        | StudioStatus::DeviceMismatch => {}
    }
    section.append(&status_frame);

    match status_snapshot {
        StudioStatus::NotActivated
        | StudioStatus::InvalidEntitlement
        | StudioStatus::DeviceMismatch => {
            let studio_page = Button::with_label(&studio_page_action_label(status_snapshot));
            let controls = build_activation_controls(&status, &studio_page, || {});

            section.append(&group_title(&t("Activation")));
            let activation_frame = table_frame();
            let activation_row = GtkBox::new(Orientation::Vertical, 8);
            activation_row.add_css_class("settings-table-row");
            let actions = GtkBox::new(Orientation::Horizontal, 8);
            actions.append(&studio_page);
            // The purchase link keeps the left; the form's submit rides the
            // row's far edge instead of bunching against it.
            let gap = GtkBox::new(Orientation::Horizontal, 0);
            gap.set_hexpand(true);
            actions.append(&gap);
            actions.append(&controls.activate);
            // The submit row closes the form, so it sits a gap further down than
            // the form's own line spacing.
            actions.set_margin_top(8);
            activation_row.append(&controls.hint);
            activation_row.append(&controls.token);
            activation_row.append(&actions);
            activation_row.append(&controls.feedback);
            activation_frame.append(&activation_row);
            section.append(&activation_frame);
        }
        _ => {}
    }

    if !matches!(status_snapshot, StudioStatus::ConfigurationUnavailable) {
        section.append(&group_title(&t("Device ID")));
        let device_frame = table_frame();
        device_frame.append(&build_device_id_row());
        section.append(&device_frame);
    }

    section
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

struct ActivationControls {
    hint: Label,
    token: Entry,
    activate: Button,
    feedback: Label,
}

fn build_activation_controls(
    status: &Label,
    studio_page: &Button,
    on_activated: impl Fn() + 'static,
) -> ActivationControls {
    let hint = Label::new(Some(&t(
        "Paste the signed entitlement you received after purchase. Activation is stored on this device and works offline.",
    )));
    hint.add_css_class("dim-label");
    hint.set_wrap(true);
    hint.set_xalign(0.0);

    let token = Entry::new();
    token.set_hexpand(true);
    token.set_placeholder_text(Some(&t("Paste Studio entitlement JSON")));

    let feedback = Label::new(Some(""));
    feedback.add_css_class("dim-label");
    feedback.set_wrap(true);
    feedback.set_xalign(0.0);
    // The result line only appears once a submit reports something, so the form
    // does not reserve a blank line under the buttons.
    feedback.set_visible(false);

    let activate = Button::with_label(&t("Activate"));
    activate.add_css_class("settings-primary-btn");

    activate.connect_clicked({
        let token = token.clone();
        let status = status.clone();
        let feedback = feedback.clone();
        let studio_page = studio_page.clone();
        move |_| match activate_token(&token.text()) {
            Ok(()) => {
                token.set_text("");
                let status_snapshot = current_status();
                status.set_text(&status_snapshot.message());
                studio_page.set_label(&studio_page_action_label(status_snapshot));
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
                status.set_text(&current_status().message());
                feedback.set_text(&match error {
                    StudioEntitlementError::DeviceMismatch => StudioStatus::DeviceMismatch.message(),
                    error => error.to_string(),
                });
                feedback.set_visible(true);
            }
        }
    });

    studio_page.connect_clicked(|_| {
        let _ = crate::utils::open::open_url("https://apexshot.org/studio");
    });

    token.connect_activate({
        let activate = activate.clone();
        move |_| activate.emit_clicked()
    });

    ActivationControls {
        hint,
        token,
        activate,
        feedback,
    }
}

/// The value a buyer pastes on apexshot.org/studio when claiming a licence for
/// this machine. Shown even before activation, because it is needed first.
fn build_device_id_row() -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.add_css_class("settings-table-row");
    row.set_hexpand(true);

    let text = GtkBox::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.set_valign(Align::Center);

    let device_id = crate::studio_entitlement::device_id();
    let value = Label::new(Some(
        &device_id
            .clone()
            .unwrap_or_else(|| t("Unavailable on this system")),
    ));
    value.add_css_class("settings-mono-value");
    value.set_selectable(true);
    value.set_xalign(0.0);
    text.append(&value);

    let hint = Label::new(Some(&t(
        "Paste this at apexshot.org/studio to claim a Studio licence for this device.",
    )));
    hint.add_css_class("dim-label");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    text.append(&hint);

    let copy = Button::with_label(&t("Copy"));
    copy.set_valign(Align::Center);
    copy.set_sensitive(device_id.is_some());
    copy.connect_clicked(move |button| {
        let Some(device_id) = device_id.as_deref() else {
            return;
        };
        if crate::utils::clipboard::copy_text_to_clipboard(device_id).is_ok() {
            button.set_label(&t("Copied"));
            let button = button.clone();
            gtk4::glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                button.set_label(&t("Copy"))
            });
        }
    });

    row.append(&text);
    row.append(&copy);
    row
}

fn studio_page_action_label(status: StudioStatus) -> String {
    match status {
        StudioStatus::NotActivated => t("Upgrade to Studio — $39 with 12 months of updates"),
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
            t("Upgrade to Studio — $39 with 12 months of updates")
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
}
