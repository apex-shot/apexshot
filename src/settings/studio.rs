use crate::i18n::t;
use crate::studio_entitlement::{activate_token, current_status, StudioStatus};
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Entry, Expander, Label, Orientation};

pub fn build_studio_section() -> GtkBox {
    let status_snapshot = current_status();
    let section = GtkBox::new(Orientation::Vertical, 12);
    section.set_halign(Align::Fill);
    section.set_valign(Align::Start);
    section.set_hexpand(true);
    section.set_margin_top(20);
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

    let frame = GtkBox::new(Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.set_margin_start(4);
    frame.set_margin_end(4);

    let status_row = GtkBox::new(Orientation::Vertical, 4);
    status_row.add_css_class("settings-table-row");
    let status_title = Label::new(Some(&t("Studio status")));
    status_title.add_css_class("settings-group-title");
    status_title.set_xalign(0.0);
    status_title.set_hexpand(true);
    let status_heading = GtkBox::new(Orientation::Horizontal, 8);
    status_heading.append(&status_title);
    if matches!(status_snapshot, StudioStatus::Active { .. }) {
        let pill = Label::new(Some(&t("Active")));
        pill.add_css_class("settings-status-pill");
        pill.set_valign(Align::Center);
        status_heading.append(&pill);
    }
    let status = Label::new(Some(&status_snapshot.message()));
    status.add_css_class("dim-label");
    status.set_wrap(true);
    status.set_xalign(0.0);
    status_row.append(&status_heading);
    status_row.append(&status);
    frame.append(&status_row);

    match status_snapshot {
        StudioStatus::ConfigurationUnavailable => {
            let availability_row = GtkBox::new(Orientation::Vertical, 8);
            availability_row.add_css_class("settings-table-row");
            let guidance = Label::new(Some(&t(
                "This build can't verify Studio licenses. Check the official Studio page for build availability or license recovery; buying a license alone can't enable this build.",
            )));
            guidance.add_css_class("dim-label");
            guidance.set_wrap(true);
            guidance.set_xalign(0.0);
            let action = Button::with_label(&studio_page_action_label(status_snapshot));
            action.set_halign(Align::Start);
            action.connect_clicked(|_| {
                let _ = crate::utils::open::open_url("https://apexshot.org/studio");
            });
            availability_row.append(&guidance);
            availability_row.append(&action);
            frame.append(&availability_row);
        }
        StudioStatus::Active { .. } | StudioStatus::UpdateAccessEnded { .. } => {
            // A saved license that already works: keep the status and the way to
            // open the purchase page visible, and tuck the paste box behind a
            // disclosure so an activated installation doesn't read like a form
            // that still needs filling in.
            let studio_page = Button::with_label(&studio_page_action_label(status_snapshot));
            studio_page.set_halign(Align::Start);

            let replacement = Expander::new(Some(&t("Replace license")));
            replacement.set_expanded(false);
            let controls = build_activation_controls(&status, &studio_page, {
                let replacement = replacement.clone();
                move || replacement.set_expanded(false)
            });

            let replacement_body = GtkBox::new(Orientation::Vertical, 8);
            replacement_body.add_css_class("settings-table-row");
            replacement_body.append(&controls.hint);
            replacement_body.append(&controls.token);
            replacement_body.append(&controls.activate);
            replacement_body.append(&controls.feedback);
            replacement.set_child(Some(&replacement_body));

            let replace_row = GtkBox::new(Orientation::Vertical, 8);
            replace_row.add_css_class("settings-table-row");
            replace_row.append(&replacement);
            frame.append(&replace_row);

            let manage_row = GtkBox::new(Orientation::Horizontal, 8);
            manage_row.add_css_class("settings-table-row");
            manage_row.set_hexpand(true);
            manage_row.append(&studio_page);
            if crate::studio_entitlement::stored_token().is_some() {
                let copy = Button::with_label(&t("Copy license"));
                copy.set_hexpand(false);
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
                // Keep the two actions at opposite ends of the row instead of
                // stacking them together in the corner.
                let gap = GtkBox::new(Orientation::Horizontal, 0);
                gap.set_hexpand(true);
                manage_row.append(&gap);
                manage_row.append(&copy);
            }
            frame.append(&manage_row);
        }
        StudioStatus::NotActivated | StudioStatus::InvalidEntitlement => {
            let studio_page = Button::with_label(&studio_page_action_label(status_snapshot));
            let controls = build_activation_controls(&status, &studio_page, || {});

            let activation_row = GtkBox::new(Orientation::Vertical, 8);
            activation_row.add_css_class("settings-table-row");
            let actions = GtkBox::new(Orientation::Horizontal, 8);
            actions.set_halign(Align::Start);
            actions.append(&controls.activate);
            actions.append(&studio_page);
            activation_row.append(&controls.hint);
            activation_row.append(&controls.token);
            activation_row.append(&actions);
            activation_row.append(&controls.feedback);
            frame.append(&activation_row);
        }
    }

    section.append(&frame);
    section
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

    let activate = Button::with_label(&t("Activate"));
    // The themed primary rules are compound: both classes are required.
    activate.add_css_class("settings-save-btn");
    activate.add_css_class("settings-save-btn-ready");

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
                on_activated();
            }
            Err(error) => {
                status.set_text(&current_status().message());
                feedback.set_text(&error.to_string());
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

fn studio_page_action_label(status: StudioStatus) -> String {
    match status {
        StudioStatus::NotActivated => t("Upgrade to Studio — $39 once"),
        StudioStatus::InvalidEntitlement => t("Recover license"),
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
            t("Upgrade to Studio — $39 once")
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
