//! The in-editor card that offers Studio at the moment an export needs it.

use crate::capture::editor::window::icon_names::custom;
use crate::i18n::t;
use crate::studio_entitlement::{
    activate_token, ensure_export_allowed, StudioCapabilities, StudioEntitlementError,
};
use gtk4::{
    glib, prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, CssProvider, Entry,
    EventControllerKey, Image, Label, Orientation, Overlay, PropagationPhase, Revealer,
    RevealerTransitionType,
};
use std::cell::Cell;
use std::rc::Rc;

const STUDIO_URL: &str = "https://apexshot.org/studio";
const SCRIM_CLASS: &str = "apexshot-studio-scrim";
const LIGHT_CLASS: &str = "apexshot-studio-light";
const CARD_WIDTH: i32 = 440;
const NATURAL_WIDTH_CHARS: i32 = 20;

const CSS: &str = r#"
    .apexshot-studio-scrim { background: alpha(#000000, 0.45); }
    .apexshot-studio-card {
        font-family: 'Inter', 'Noto Sans', system-ui, sans-serif;
        background: #0f0f0f;
        color: #f1f1f3;
        border: 1px solid alpha(white, 0.16);
        border-radius: 12px;
        padding: 20px 22px 16px;
        box-shadow: none;
    }
    .apexshot-studio-title {
        color: alpha(white, 0.92);
        font-size: 13px;
        font-weight: 600;
        letter-spacing: -0.1px;
    }
    .apexshot-studio-subtitle,
    .apexshot-studio-feature-detail,
    .apexshot-studio-term { font-size: 11px; color: alpha(white, 0.52); line-height: 1.4; }
    .apexshot-studio-feature { padding: 10px 0; }
    .apexshot-studio-feature-divided { border-top: 1px solid alpha(white, 0.06); }
    .apexshot-studio-feature-title { font-size: 12px; font-weight: 500; }
    .apexshot-studio-feature-icon { color: alpha(white, 0.68); }
    .apexshot-studio-used {
        font-size: 10px;
        color: alpha(white, 0.42);
    }
    .apexshot-studio-license-actions { padding: 8px 0; }
    .apexshot-studio-note { font-size: 10px; color: alpha(white, 0.42); }
    .apexshot-studio-feedback { font-size: 11px; color: #ff8a80; }
    .apexshot-studio-card button {
        background-image: none;
        text-shadow: none;
        -gtk-icon-shadow: none;
        box-shadow: none;
    }
    button.apexshot-studio-primary,
    button.apexshot-studio-activate {
        min-width: 72px;
        min-height: 28px;
        padding: 0 14px;
        border: none;
        border-radius: 6px;
        background: #ff670d;
        color: white;
        font-size: 12px;
        font-weight: 600;
    }
    button.apexshot-studio-primary:hover,
    button.apexshot-studio-activate:hover { background: #c06a44; }
    button.apexshot-studio-primary:active,
    button.apexshot-studio-activate:active { background: #8a4a2d; }
    button.apexshot-studio-ghost {
        min-height: 28px;
        padding: 0 10px;
        border: none;
        border-radius: 5px;
        background: transparent;
        color: alpha(white, 0.68);
        font-size: 12px;
    }
    button.apexshot-studio-ghost:hover { background: alpha(white, 0.1); color: white; }
    button.apexshot-studio-license { padding: 0; min-height: 20px; font-size: 11px; }
    button.apexshot-studio-license:hover { background: transparent; }
    button.apexshot-studio-activate:disabled { background: alpha(white, 0.06); color: alpha(white, 0.34); }
    entry.apexshot-studio-entry {
        min-height: 28px;
        padding: 0 8px;
        border: 1px solid alpha(white, 0.12);
        border-radius: 6px;
        background: alpha(white, 0.06);
        color: #f1f1f3;
        font-size: 12px;
        box-shadow: none;
    }
    entry.apexshot-studio-entry:focus-within { border-color: alpha(white, 0.28); }

    .apexshot-studio-light.apexshot-studio-scrim { background: alpha(#111827, 0.28); }
    .apexshot-studio-light .apexshot-studio-card {
        background: #ffffff;
        color: #1d2129;
        border-color: alpha(#111827, 0.16);
    }
    .apexshot-studio-light .apexshot-studio-title { color: alpha(#1d2129, 0.92); }
    .apexshot-studio-light .apexshot-studio-subtitle,
    .apexshot-studio-light .apexshot-studio-feature-detail,
    .apexshot-studio-light .apexshot-studio-term { color: alpha(#1d2129, 0.58); }
    .apexshot-studio-light .apexshot-studio-feature-divided { border-top-color: alpha(#111827, 0.09); }
    .apexshot-studio-light .apexshot-studio-feature-icon { color: alpha(#1d2129, 0.68); }
    .apexshot-studio-light .apexshot-studio-used { color: alpha(#1d2129, 0.5); }
    .apexshot-studio-light .apexshot-studio-note { color: alpha(#1d2129, 0.5); }
    .apexshot-studio-light .apexshot-studio-feedback { color: #b42318; }
    .apexshot-studio-light button.apexshot-studio-ghost { color: alpha(#1d2129, 0.68); }
    .apexshot-studio-light button.apexshot-studio-ghost:hover {
        background: alpha(#111827, 0.07);
        color: #1d2129;
    }
    .apexshot-studio-light button.apexshot-studio-activate:disabled {
        background: alpha(#111827, 0.07);
        color: alpha(#1d2129, 0.34);
    }
    .apexshot-studio-light entry.apexshot-studio-entry {
        background: alpha(#111827, 0.04);
        border-color: alpha(#111827, 0.16);
        color: #1d2129;
    }
    .apexshot-studio-light entry.apexshot-studio-entry:focus-within { border-color: alpha(#111827, 0.32); }
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Purchase,
    InvalidLicense,
    UpdatesEnded,
    ConfigurationUnavailable,
    StorageUnavailable,
}

fn kind_for(error: &StudioEntitlementError) -> Kind {
    match error {
        StudioEntitlementError::NotActivated => Kind::Purchase,
        StudioEntitlementError::InvalidEntitlement => Kind::InvalidLicense,
        StudioEntitlementError::UpdateAccessEnded => Kind::UpdatesEnded,
        StudioEntitlementError::ConfigurationUnavailable => Kind::ConfigurationUnavailable,
        StudioEntitlementError::Storage(_) => Kind::StorageUnavailable,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimaryAction {
    Upgrade,
    GetStudioReadyBuild,
    RecoverLicense,
    ManageLicense,
    KeepEditing,
}

impl PrimaryAction {
    fn label(self) -> String {
        match self {
            Self::Upgrade => t("Upgrade to Studio — $39 once"),
            Self::GetStudioReadyBuild => t("Check Studio availability"),
            Self::RecoverLicense => t("Recover license"),
            Self::ManageLicense => t("Manage license"),
            Self::KeepEditing => t("Keep editing"),
        }
    }

    fn url(self) -> Option<&'static str> {
        match self {
            Self::Upgrade
            | Self::GetStudioReadyBuild
            | Self::RecoverLicense
            | Self::ManageLicense => Some(STUDIO_URL),
            Self::KeepEditing => None,
        }
    }
}

fn primary_action(error: &StudioEntitlementError) -> PrimaryAction {
    match kind_for(error) {
        Kind::Purchase => PrimaryAction::Upgrade,
        Kind::InvalidLicense => PrimaryAction::RecoverLicense,
        Kind::UpdatesEnded => PrimaryAction::ManageLicense,
        Kind::ConfigurationUnavailable => PrimaryAction::GetStudioReadyBuild,
        Kind::StorageUnavailable => PrimaryAction::KeepEditing,
    }
}

fn card_copy(error: &StudioEntitlementError) -> (String, String) {
    match (kind_for(error), error) {
        (Kind::Purchase, _) => (
            t("Unlock ApexShot Studio"),
            t("Professional tools for presenting your captures."),
        ),
        (Kind::InvalidLicense, _) => (
            t("Studio license couldn't be verified"),
            t("The license saved on this device couldn't be checked. Paste a valid license to unlock Studio."),
        ),
        (Kind::UpdatesEnded, _) => (
            t("This version is newer than your license"),
            t("Your license includes updates through an earlier release. Versions it covers keep working offline."),
        ),
        (Kind::ConfigurationUnavailable, _) => (
            t("Studio isn't available"),
            t("This build can't verify Studio licenses. Check the official Studio page for build availability or license recovery; buying a license alone can't enable this build."),
        ),
        (Kind::StorageUnavailable, _) => (t("Studio isn't available"), error.to_string()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Feature {
    Motion,
    Placement,
    Camera,
}

impl Feature {
    fn is_used_by(self, capabilities: StudioCapabilities) -> bool {
        match self {
            Self::Motion => capabilities.motion_export,
            Self::Placement => capabilities.static_presentation,
            Self::Camera => capabilities.advanced_zoom,
        }
    }
}

fn features_for(capabilities: StudioCapabilities) -> Vec<(Feature, bool)> {
    let mut features = [Feature::Motion, Feature::Placement, Feature::Camera]
        .map(|feature| (feature, feature.is_used_by(capabilities)))
        .to_vec();
    features.sort_by_key(|(_, used)| !used);
    features
}

/// Returns whether the export may proceed. When Studio blocks it, the card is
/// raised over the editor instead, and `retry` re-runs the export once a
/// license is activated from inside the card.
pub(crate) fn gate(
    window: &ApplicationWindow,
    capabilities: StudioCapabilities,
    retry: impl Fn() + 'static,
) -> bool {
    let Err(error) = ensure_export_allowed(capabilities) else {
        return true;
    };
    let Some(overlay) = window.child().and_downcast::<Overlay>() else {
        crate::utils::notify::desktop_notification_important(
            &t("Studio export unavailable"),
            &error.to_string(),
        );
        return false;
    };
    present(&overlay, &error, capabilities, Rc::new(retry));
    false
}

fn present(
    overlay: &Overlay,
    error: &StudioEntitlementError,
    capabilities: StudioCapabilities,
    retry: Rc<dyn Fn()>,
) {
    if has_child_with_class(overlay, SCRIM_CLASS) {
        return;
    }
    install_css();

    let scrim = GtkBox::new(Orientation::Vertical, 0);
    scrim.add_css_class(SCRIM_CLASS);
    scrim.set_halign(Align::Fill);
    scrim.set_valign(Align::Fill);
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    scrim.set_can_target(true);
    if has_descendant_with_class(overlay.upcast_ref(), "editor-theme-light") {
        scrim.add_css_class(LIGHT_CLASS);
    }

    let dismiss: Rc<dyn Fn()> = Rc::new({
        let overlay = overlay.downgrade();
        let scrim = scrim.downgrade();
        move || {
            if let (Some(overlay), Some(scrim)) = (overlay.upgrade(), scrim.upgrade()) {
                overlay.remove_overlay(&scrim);
            }
        }
    });

    let (card, focus_target) = build_card(error, capabilities, &dismiss, &retry);
    scrim.append(&card);

    let keys = EventControllerKey::new();
    keys.set_propagation_phase(PropagationPhase::Capture);
    keys.connect_key_pressed({
        let dismiss = dismiss.clone();
        move |_, key, _, _| {
            if key != gtk4::gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            dismiss();
            glib::Propagation::Stop
        }
    });
    scrim.add_controller(keys);

    overlay.add_overlay(&scrim);
    overlay.set_clip_overlay(&scrim, true);
    glib::idle_add_local_once(move || {
        focus_target.grab_focus();
    });
}

fn build_card(
    error: &StudioEntitlementError,
    capabilities: StudioCapabilities,
    dismiss: &Rc<dyn Fn()>,
    retry: &Rc<dyn Fn()>,
) -> (GtkBox, gtk4::Widget) {
    let kind = kind_for(error);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("apexshot-studio-card");
    card.set_size_request(CARD_WIDTH, -1);
    card.set_halign(Align::Center);
    card.set_valign(Align::Center);
    card.set_hexpand(true);
    card.set_vexpand(true);

    let (title, subtitle) = card_copy(error);

    let heading = GtkBox::new(Orientation::Vertical, 6);
    let title_label = text_label(&title, "apexshot-studio-title");
    let subtitle_label = text_label(&subtitle, "apexshot-studio-subtitle");
    heading.append(&title_label);
    heading.append(&subtitle_label);
    card.append(&heading);

    if kind == Kind::Purchase {
        card.append(&feature_panel(capabilities));
        let terms = text_label(
            &t("Includes 12 months of updates. Versions you get keep working forever."),
            "apexshot-studio-term",
        );
        terms.set_margin_top(14);
        card.append(&terms);
    }

    let license_row = license_entry(capabilities, dismiss, retry);
    let reveal = Revealer::new();
    reveal.set_transition_type(RevealerTransitionType::SlideDown);
    reveal.set_transition_duration(160);
    reveal.set_child(Some(&license_row.container));
    reveal.set_reveal_child(kind == Kind::InvalidLicense);
    if kind == Kind::InvalidLicense {
        card.append(&reveal);
    }

    let keep = ghost_button(&t("Keep editing"));
    keep.connect_clicked({
        let dismiss = dismiss.clone();
        move |_| dismiss()
    });

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_margin_top(12);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let action = primary_action(error);
    let primary = primary_button(&action.label());
    if let Some(url) = action.url() {
        let url = url.to_string();
        primary.connect_clicked(move |_| {
            let _ = crate::utils::open::open_url(&url);
        });
    } else {
        primary.connect_clicked({
            let dismiss = dismiss.clone();
            move |_| dismiss()
        });
    }

    let focus_target: gtk4::Widget = match kind {
        Kind::Purchase => {
            let have_license = ghost_button(&t("I have a license"));
            have_license.add_css_class("apexshot-studio-license");
            have_license.connect_clicked({
                let reveal = reveal.clone();
                let entry = license_row.entry.clone();
                move |_| {
                    let reveal_now = !reveal.reveals_child();
                    reveal.set_reveal_child(reveal_now);
                    if reveal_now {
                        entry.grab_focus();
                    }
                }
            });
            let license_actions = GtkBox::new(Orientation::Horizontal, 12);
            license_actions.add_css_class("apexshot-studio-license-actions");
            let refund = text_label(
                &t("14-day refund on your first purchase."),
                "apexshot-studio-term",
            );
            refund.set_hexpand(true);
            license_actions.append(&refund);
            license_actions.append(&have_license);
            card.append(&license_actions);
            card.append(&reveal);
            actions.append(&keep);
            actions.append(&spacer);
            actions.append(&primary);
            primary.clone().upcast()
        }
        Kind::InvalidLicense => {
            actions.append(&spacer);
            actions.append(&keep);
            actions.append(&primary);
            license_row.entry.clone().upcast()
        }
        Kind::UpdatesEnded => {
            actions.append(&keep);
            actions.append(&spacer);
            actions.append(&primary);
            primary.clone().upcast()
        }
        Kind::ConfigurationUnavailable => {
            actions.append(&keep);
            actions.append(&spacer);
            actions.append(&primary);
            primary.clone().upcast()
        }
        Kind::StorageUnavailable => {
            actions.append(&spacer);
            actions.append(&primary);
            primary.clone().upcast()
        }
    };
    card.append(&actions);

    let note = text_label(
        &t("Capture, annotation, recording and clean exports stay free."),
        "apexshot-studio-note",
    );
    note.set_margin_top(14);
    card.append(&note);

    (card, focus_target)
}

fn feature_panel(capabilities: StudioCapabilities) -> GtkBox {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.set_margin_top(10);

    for (index, (feature, used)) in features_for(capabilities).into_iter().enumerate() {
        let (title, detail, icon_name) = match feature {
            Feature::Motion => (
                t("Motion timeline"),
                t("Animate your captures with titles and easing, then export as MP4."),
                custom::SPLINE_SYMBOLIC,
            ),
            Feature::Placement => (
                t("Perspective and placement"),
                t("Tilt, rotate and position a still inside its background."),
                custom::OVERLAPPING_WINDOWS_SYMBOLIC,
            ),
            Feature::Camera => (
                t("Camera controls"),
                t("Instant zooms, speed, smoothness and motion blur for recordings."),
                custom::SETTINGS_SYMBOLIC,
            ),
        };

        let row = GtkBox::new(Orientation::Horizontal, 12);
        row.add_css_class("apexshot-studio-feature");
        if index > 0 {
            row.add_css_class("apexshot-studio-feature-divided");
        }
        let icon = Image::from_icon_name(icon_name);
        icon.add_css_class("apexshot-studio-feature-icon");
        icon.set_pixel_size(16);
        icon.set_valign(Align::Start);
        icon.set_margin_top(1);

        let text = GtkBox::new(Orientation::Vertical, 4);
        text.set_hexpand(true);
        let heading = GtkBox::new(Orientation::Horizontal, 12);
        let title = text_label(&title, "apexshot-studio-feature-title");
        title.set_hexpand(true);
        heading.append(&title);
        if used {
            let used = Label::new(Some(&t("In this export")));
            used.add_css_class("apexshot-studio-used");
            heading.append(&used);
        }
        text.append(&heading);
        text.append(&text_label(&detail, "apexshot-studio-feature-detail"));
        row.append(&icon);
        row.append(&text);
        panel.append(&row);
    }
    panel
}

struct LicenseEntry {
    container: GtkBox,
    entry: Entry,
}

fn license_entry(
    capabilities: StudioCapabilities,
    dismiss: &Rc<dyn Fn()>,
    retry: &Rc<dyn Fn()>,
) -> LicenseEntry {
    let container = GtkBox::new(Orientation::Vertical, 6);
    container.set_margin_top(4);
    container.set_margin_bottom(4);

    let row = GtkBox::new(Orientation::Horizontal, 8);
    let entry = Entry::new();
    entry.add_css_class("apexshot-studio-entry");
    entry.set_hexpand(true);
    entry.set_placeholder_text(Some(&t("Paste your license")));
    let activate = Button::with_label(&t("Activate"));
    activate.add_css_class("apexshot-studio-activate");
    activate.set_sensitive(false);
    row.append(&entry);
    row.append(&activate);

    let feedback = text_label("", "apexshot-studio-feedback");
    feedback.set_visible(false);
    container.append(&row);
    container.append(&feedback);

    entry.connect_changed({
        let activate = activate.clone();
        let feedback = feedback.clone();
        move |entry| {
            activate.set_sensitive(!entry.text().trim().is_empty());
            feedback.set_visible(false);
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

    let settled = Rc::new(Cell::new(false));
    activate.connect_clicked({
        let entry = entry.clone();
        let dismiss = dismiss.clone();
        let retry = retry.clone();
        move |_| {
            if settled.get() {
                return;
            }
            let message = match activate_token(&entry.text()) {
                Err(StudioEntitlementError::InvalidEntitlement) => {
                    t("That license couldn't be verified. Make sure you pasted all of it.")
                }
                Err(error) => error.to_string(),
                Ok(()) => match ensure_export_allowed(capabilities) {
                    Ok(()) => {
                        settled.set(true);
                        dismiss();
                        let retry = retry.clone();
                        glib::idle_add_local_once(move || retry());
                        return;
                    }
                    Err(StudioEntitlementError::UpdateAccessEnded) => t(
                        "License saved, but this version is newer than the updates it includes.",
                    ),
                    Err(error) => error.to_string(),
                },
            };
            feedback.set_text(&message);
            feedback.set_visible(true);
        }
    });

    LicenseEntry { container, entry }
}

fn text_label(text: &str, class: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class(class);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_max_width_chars(NATURAL_WIDTH_CHARS);
    label
}

fn primary_button(text: &str) -> Button {
    let button = Button::with_label(text);
    button.add_css_class("apexshot-studio-primary");
    button.set_has_frame(false);
    button
}

fn ghost_button(text: &str) -> Button {
    let button = Button::with_label(text);
    button.add_css_class("apexshot-studio-ghost");
    button.set_has_frame(false);
    button
}

fn install_css() {
    thread_local! {
        static INSTALLED: Cell<bool> = const { Cell::new(false) };
    }
    if INSTALLED.with(Cell::get) {
        return;
    }
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(CSS);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    INSTALLED.with(|installed| installed.set(true));
}

fn has_child_with_class(parent: &impl IsA<gtk4::Widget>, class: &str) -> bool {
    let mut child = parent.first_child();
    while let Some(widget) = child {
        if widget.has_css_class(class) {
            return true;
        }
        child = widget.next_sibling();
    }
    false
}

fn has_descendant_with_class(root: &gtk4::Widget, class: &str) -> bool {
    let mut stack = vec![root.clone()];
    while let Some(widget) = stack.pop() {
        if widget.has_css_class(class) {
            return true;
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            stack.push(current.clone());
            child = current.next_sibling();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entitlement_error_maps_to_one_card() {
        assert_eq!(
            kind_for(&StudioEntitlementError::NotActivated),
            Kind::Purchase
        );
        assert_eq!(
            kind_for(&StudioEntitlementError::InvalidEntitlement),
            Kind::InvalidLicense
        );
        assert_eq!(
            kind_for(&StudioEntitlementError::UpdateAccessEnded),
            Kind::UpdatesEnded
        );
        assert_eq!(
            kind_for(&StudioEntitlementError::ConfigurationUnavailable),
            Kind::ConfigurationUnavailable
        );
        assert_eq!(
            kind_for(&StudioEntitlementError::Storage("disk full".into())),
            Kind::StorageUnavailable
        );
    }

    #[test]
    fn primary_actions_match_license_state_and_use_the_official_studio_route() {
        for (error, expected, label, url) in [
            (
                StudioEntitlementError::NotActivated,
                PrimaryAction::Upgrade,
                t("Upgrade to Studio — $39 once"),
                Some(STUDIO_URL),
            ),
            (
                StudioEntitlementError::InvalidEntitlement,
                PrimaryAction::RecoverLicense,
                t("Recover license"),
                Some(STUDIO_URL),
            ),
            (
                StudioEntitlementError::UpdateAccessEnded,
                PrimaryAction::ManageLicense,
                t("Manage license"),
                Some(STUDIO_URL),
            ),
            (
                StudioEntitlementError::ConfigurationUnavailable,
                PrimaryAction::GetStudioReadyBuild,
                t("Check Studio availability"),
                Some(STUDIO_URL),
            ),
            (
                StudioEntitlementError::Storage("disk full".into()),
                PrimaryAction::KeepEditing,
                t("Keep editing"),
                None,
            ),
        ] {
            let action = primary_action(&error);
            assert_eq!(action, expected);
            assert_eq!(action.label(), label);
            assert_eq!(action.url(), url);
        }
    }

    #[test]
    fn unavailable_build_explains_that_purchase_alone_cannot_enable_verification() {
        let (title, subtitle) = card_copy(&StudioEntitlementError::ConfigurationUnavailable);
        assert_eq!(title, "Studio isn't available");
        assert!(subtitle.contains("buying a license alone can't enable this build"));
    }

    #[test]
    fn invalid_license_guides_recovery_without_a_purchase_pitch() {
        let (title, subtitle) = card_copy(&StudioEntitlementError::InvalidEntitlement);
        let action = primary_action(&StudioEntitlementError::InvalidEntitlement);

        assert_eq!(title, "Studio license couldn't be verified");
        assert_eq!(action.label(), t("Recover license"));
        assert!(subtitle.contains("Paste a valid license"));
        assert!(!subtitle.contains("purchase"));
        assert!(!action.label().contains("Upgrade"));
    }

    #[test]
    fn features_used_by_the_export_lead_the_list() {
        let features = features_for(StudioCapabilities {
            advanced_zoom: true,
            ..StudioCapabilities::default()
        });
        assert_eq!(
            features,
            vec![
                (Feature::Camera, true),
                (Feature::Motion, false),
                (Feature::Placement, false),
            ]
        );
    }

    #[test]
    fn features_keep_their_order_within_each_group() {
        let features = features_for(StudioCapabilities {
            motion_export: true,
            static_presentation: true,
            advanced_zoom: false,
        });
        assert_eq!(
            features,
            vec![
                (Feature::Motion, true),
                (Feature::Placement, true),
                (Feature::Camera, false),
            ]
        );
    }

    #[test]
    fn no_feature_is_marked_when_the_export_uses_none() {
        let features = features_for(StudioCapabilities::default());
        assert!(features.iter().all(|(_, used)| !used));
        assert_eq!(features.len(), 3);
    }

    #[test]
    fn the_stylesheet_parses_without_gtk_errors() {
        let Some((errors, probe_errors)) = crate::test_support::with_gtk(|| {
            let collect = |css: &str| {
                let provider = CssProvider::new();
                let errors = Rc::new(std::cell::RefCell::new(Vec::new()));
                provider.connect_parsing_error({
                    let errors = errors.clone();
                    move |_, _, error| errors.borrow_mut().push(error.message().to_string())
                });
                provider.load_from_data(css);
                let collected: Vec<String> = errors.borrow().clone();
                collected
            };
            (
                collect(CSS),
                collect(".probe { definitely-not-a-property: 1; }"),
            )
        }) else {
            eprintln!("skipping: no display available");
            return;
        };
        assert!(
            !probe_errors.is_empty(),
            "the parsing-error signal must fire for a bogus declaration"
        );
        assert!(errors.is_empty(), "studio card css errors: {errors:?}");
    }
}
