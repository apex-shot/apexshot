//! The in-editor card that offers Studio at the moment an export needs it.

use crate::capture::editor::window::icon_names::custom;
use crate::i18n::t;
use crate::studio_entitlement::{
    activate_token, ensure_export_allowed, StudioCapabilities, StudioEntitlementError, StudioStatus,
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
const CARD_WIDTH: f64 = 440.0;
const NATURAL_WIDTH_CHARS: i32 = 20;
const HERO_HEIGHT: f64 = 164.0;
const MARK_SIZE: f64 = 68.0;
const CHIP_WIDTH_CHARS: i32 = 22;
const LOGO_PNG: &[u8] = include_bytes!("../media/apexshot-logo-400.png");

/// Logical size to device pixels, the way the editor's own widgets do it. GDK
/// already reports a scaled display on Linux, so only Windows needs the
/// conversion.
fn px(value: f64) -> i32 {
    #[cfg(target_os = "windows")]
    {
        (value * crate::platform::ui_scale()).round() as i32
    }
    #[cfg(not(target_os = "windows"))]
    {
        value.round() as i32
    }
}

const CSS: &str = r#"
    .apexshot-studio-scrim {
        background: rgba(5, 7, 12, 0.62);
        border-radius: 12px;
    }
    .apexshot-studio-card {
        font-family: 'DM Sans', 'Noto Sans', system-ui, sans-serif;
        background: #141416;
        color: #f3f0ec;
        border: 1px solid rgba(255, 255, 255, 0.09);
        border-radius: 18px;
        box-shadow: 0 32px 80px rgba(0, 0, 0, 0.55);
    }
    .apexshot-studio-body { padding: 18px; }
    .apexshot-studio-body.apexshot-studio-body-offer { padding: 6px 28px 20px 28px; }
    .apexshot-studio-hero {
        border-radius: 17px 17px 0 0;
        background-color: #120d0b;
        background-image:
            radial-gradient(ellipse closest-side at 50% 55%, rgba(255, 110, 20, 0.50), rgba(255, 103, 13, 0)),
            radial-gradient(circle at 15% 0%, rgba(255, 168, 92, 0.22), rgba(255, 168, 92, 0) 45%),
            radial-gradient(circle at 88% 10%, rgba(255, 84, 40, 0.18), rgba(255, 84, 40, 0) 40%),
            linear-gradient(180deg, #1c1310, #141416);
    }
    .apexshot-studio-mark {
        border-radius: 18px;
        box-shadow: 0 14px 36px rgba(255, 103, 13, 0.45), 0 2px 6px rgba(0, 0, 0, 0.35);
    }
    .apexshot-studio-title {
        color: #f3f0ec;
        font-size: 15px;
        font-weight: 600;
        letter-spacing: -0.2px;
    }
    .apexshot-studio-subtitle {
        font-size: 13px;
        color: #a3a09c;
        line-height: 1.4;
    }
    .apexshot-studio-headline {
        font-family: 'Inter', 'DM Sans', 'Noto Sans', system-ui, sans-serif;
        font-size: 28px;
        font-weight: 300;
        letter-spacing: -0.6px;
        color: #f6f3ef;
    }
    .apexshot-studio-lede {
        font-size: 13px;
        color: #9c9893;
        line-height: 1.4;
    }
    .apexshot-studio-chip {
        min-height: 30px;
        padding: 0 10px;
        border-radius: 999px;
        border: 1px solid rgba(255, 255, 255, 0.08);
        background: rgba(255, 255, 255, 0.035);
        color: #d6d1cb;
        font-size: 12px;
        font-weight: 500;
    }
    .apexshot-studio-chip image { color: #a8a39c; }
    .apexshot-studio-chip.apexshot-studio-chip-used {
        border-color: rgba(255, 122, 38, 0.45);
        background: rgba(255, 103, 13, 0.14);
        color: #ffb27e;
    }
    .apexshot-studio-chip.apexshot-studio-chip-used image { color: #ff9250; }
    .apexshot-studio-dot {
        background: #ff9250;
        border-radius: 999px;
    }
    .apexshot-studio-used-caption {
        font-size: 11px;
        font-weight: 600;
        color: #ff9250;
    }
    .apexshot-studio-price {
        font-family: 'Inter', 'DM Sans', 'Noto Sans', system-ui, sans-serif;
        font-size: 30px;
        font-weight: 600;
        letter-spacing: -0.8px;
        color: #f6f3ef;
    }
    .apexshot-studio-term {
        font-size: 12px;
        color: #83807b;
        line-height: 1.4;
    }
    .apexshot-studio-note { font-size: 12px; color: #83807b; }
    .apexshot-studio-fineprint { font-size: 11px; color: #6f6b66; }
    .apexshot-studio-separator { font-size: 12px; color: #5d5a56; }
    .apexshot-studio-feedback { font-size: 12px; color: #ff8a80; }
    .apexshot-studio-card button {
        background-image: none;
        text-shadow: none;
        -gtk-icon-shadow: none;
        box-shadow: none;
    }
    button.apexshot-studio-primary,
    button.apexshot-studio-activate {
        min-height: 30px;
        padding: 0 14px;
        border: none;
        border-radius: 6px;
        background: #ff670d;
        color: #ffffff;
        font-size: 13px;
        font-weight: 600;
    }
    button.apexshot-studio-primary:hover,
    button.apexshot-studio-activate:hover { background: #ff792b; }
    button.apexshot-studio-primary:active,
    button.apexshot-studio-activate:active { background: #e65c0a; }
    .apexshot-studio-card button.apexshot-studio-cta {
        min-height: 42px;
        border-radius: 12px;
        background-color: #ff670d;
        background-image: linear-gradient(180deg, #ff8a3d, #ff630a);
        box-shadow: 0 10px 28px rgba(255, 103, 13, 0.32), inset 0 1px rgba(255, 255, 255, 0.28);
        font-size: 14px;
        font-weight: 600;
        transition: box-shadow 160ms ease-out, background-image 160ms ease-out;
    }
    .apexshot-studio-card button.apexshot-studio-cta:hover {
        background-image: linear-gradient(180deg, #ff9a52, #ff7019);
        box-shadow: 0 12px 34px rgba(255, 103, 13, 0.45), inset 0 1px rgba(255, 255, 255, 0.32);
    }
    .apexshot-studio-card button.apexshot-studio-cta:active {
        background-image: linear-gradient(180deg, #f36a1c, #e35a08);
        box-shadow: 0 4px 14px rgba(255, 103, 13, 0.30);
    }
    button.apexshot-studio-ghost {
        min-height: 30px;
        padding: 0 12px;
        border: none;
        border-radius: 6px;
        background: transparent;
        color: #a3a09c;
        font-size: 13px;
        font-weight: 500;
    }
    button.apexshot-studio-ghost:hover { background: rgba(255, 255, 255, 0.06); color: #f3f0ec; }
    button.apexshot-studio-link {
        min-height: 28px;
        padding: 0 8px;
        border: none;
        border-radius: 6px;
        background: transparent;
        color: #a3a09c;
        font-size: 13px;
        font-weight: 500;
    }
    button.apexshot-studio-link:hover { background: rgba(255, 255, 255, 0.06); color: #f3f0ec; }
    button.apexshot-studio-close {
        min-width: 24px;
        min-height: 24px;
        padding: 0;
        border: none;
        border-radius: 5px;
        background: transparent;
        color: #83807b;
    }
    button.apexshot-studio-close:hover { background: rgba(255, 255, 255, 0.08); color: #f3f0ec; }
    button.apexshot-studio-close:active { background: rgba(255, 255, 255, 0.12); }
    button.apexshot-studio-close image { -gtk-icon-size: 14px; }
    button.apexshot-studio-close.apexshot-studio-hero-close {
        min-width: 28px;
        min-height: 28px;
        border-radius: 999px;
        background: rgba(0, 0, 0, 0.28);
        color: rgba(255, 255, 255, 0.72);
    }
    button.apexshot-studio-close.apexshot-studio-hero-close:hover {
        background: rgba(0, 0, 0, 0.45);
        color: #ffffff;
    }
    button.apexshot-studio-activate:disabled {
        background: rgba(255, 255, 255, 0.06);
        color: rgba(255, 255, 255, 0.34);
    }
    entry.apexshot-studio-entry {
        min-height: 30px;
        padding: 0 10px;
        border: 1px solid rgba(255, 255, 255, 0.10);
        border-radius: 6px;
        background: rgba(255, 255, 255, 0.05);
        color: #f3f0ec;
        font-size: 13px;
        box-shadow: none;
    }
    entry.apexshot-studio-entry:focus-within { border-color: rgba(255, 255, 255, 0.28); }

    .apexshot-studio-light.apexshot-studio-scrim { background: rgba(17, 24, 39, 0.30); }
    .apexshot-studio-light .apexshot-studio-card {
        background: #ffffff;
        color: #1d2129;
        border-color: rgba(17, 24, 39, 0.10);
        box-shadow: 0 32px 80px rgba(17, 24, 39, 0.20);
    }
    .apexshot-studio-light .apexshot-studio-hero {
        background-color: #fff4ec;
        background-image:
            radial-gradient(ellipse closest-side at 50% 55%, rgba(255, 122, 38, 0.38), rgba(255, 122, 38, 0)),
            radial-gradient(circle at 15% 0%, rgba(255, 190, 140, 0.35), rgba(255, 190, 140, 0) 45%),
            linear-gradient(180deg, #fff1e6, #ffffff);
    }
    .apexshot-studio-light .apexshot-studio-mark {
        box-shadow: 0 14px 32px rgba(214, 84, 6, 0.28), 0 2px 6px rgba(17, 24, 39, 0.12);
    }
    .apexshot-studio-light .apexshot-studio-title { color: #1d2129; }
    .apexshot-studio-light .apexshot-studio-subtitle { color: #5c6470; }
    .apexshot-studio-light .apexshot-studio-headline,
    .apexshot-studio-light .apexshot-studio-price { color: #14171c; }
    .apexshot-studio-light .apexshot-studio-lede { color: #5c6470; }
    .apexshot-studio-light .apexshot-studio-chip {
        border-color: rgba(17, 24, 39, 0.10);
        background: rgba(17, 24, 39, 0.03);
        color: #374151;
    }
    .apexshot-studio-light .apexshot-studio-chip image { color: #6b7280; }
    .apexshot-studio-light .apexshot-studio-chip.apexshot-studio-chip-used {
        border-color: rgba(214, 84, 6, 0.35);
        background: rgba(255, 103, 13, 0.09);
        color: #c2500a;
    }
    .apexshot-studio-light .apexshot-studio-chip.apexshot-studio-chip-used image { color: #d65406; }
    .apexshot-studio-light .apexshot-studio-dot { background: #d65406; }
    .apexshot-studio-light .apexshot-studio-used-caption { color: #c2500a; }
    .apexshot-studio-light .apexshot-studio-term,
    .apexshot-studio-light .apexshot-studio-note { color: #6b7280; }
    .apexshot-studio-light .apexshot-studio-fineprint { color: #8a8f98; }
    .apexshot-studio-light .apexshot-studio-separator { color: #b0b5bd; }
    .apexshot-studio-light .apexshot-studio-feedback { color: #b42318; }
    .apexshot-studio-light .apexshot-studio-card button.apexshot-studio-cta {
        box-shadow: 0 10px 24px rgba(214, 84, 6, 0.25), inset 0 1px rgba(255, 255, 255, 0.30);
    }
    .apexshot-studio-light .apexshot-studio-card button.apexshot-studio-cta:hover {
        box-shadow: 0 12px 30px rgba(214, 84, 6, 0.34), inset 0 1px rgba(255, 255, 255, 0.34);
    }
    .apexshot-studio-light .apexshot-studio-card button.apexshot-studio-cta:active {
        box-shadow: 0 4px 12px rgba(214, 84, 6, 0.24);
    }
    .apexshot-studio-light button.apexshot-studio-ghost,
    .apexshot-studio-light button.apexshot-studio-link { color: #5c6470; }
    .apexshot-studio-light button.apexshot-studio-ghost:hover,
    .apexshot-studio-light button.apexshot-studio-link:hover {
        background: rgba(17, 24, 39, 0.06);
        color: #1d2129;
    }
    .apexshot-studio-light button.apexshot-studio-close { color: #6b7280; }
    .apexshot-studio-light button.apexshot-studio-close:hover {
        background: rgba(17, 24, 39, 0.07);
        color: #1d2129;
    }
    .apexshot-studio-light button.apexshot-studio-close:active {
        background: rgba(17, 24, 39, 0.10);
    }
    .apexshot-studio-light button.apexshot-studio-close.apexshot-studio-hero-close {
        background: rgba(255, 255, 255, 0.65);
        color: #4b5563;
    }
    .apexshot-studio-light button.apexshot-studio-close.apexshot-studio-hero-close:hover {
        background: #ffffff;
        color: #1d2129;
    }
    .apexshot-studio-light button.apexshot-studio-activate:disabled {
        background: rgba(17, 24, 39, 0.07);
        color: rgba(29, 33, 41, 0.34);
    }
    .apexshot-studio-light entry.apexshot-studio-entry {
        background: rgba(17, 24, 39, 0.04);
        border-color: rgba(17, 24, 39, 0.16);
        color: #1d2129;
    }
    .apexshot-studio-light entry.apexshot-studio-entry:focus-within {
        border-color: rgba(17, 24, 39, 0.32);
    }
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Purchase,
    InvalidLicense,
    UpdatesEnded,
    DeviceMismatch,
    ConfigurationUnavailable,
    StorageUnavailable,
}

fn kind_for(error: &StudioEntitlementError) -> Kind {
    match error {
        StudioEntitlementError::NotActivated => Kind::Purchase,
        StudioEntitlementError::InvalidEntitlement => Kind::InvalidLicense,
        StudioEntitlementError::UpdateAccessEnded => Kind::UpdatesEnded,
        StudioEntitlementError::DeviceMismatch => Kind::DeviceMismatch,
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
            Self::Upgrade => t("Upgrade to Studio"),
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
        Kind::DeviceMismatch => PrimaryAction::RecoverLicense,
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
        (Kind::DeviceMismatch, _) => (
            t("This license belongs to another device"),
            t("Copy this machine's Device ID from Settings → Studio and claim the license for it at apexshot.org/studio. Your other device keeps working."),
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
    Style,
    Motion,
    Placement,
    Camera,
    Upscale,
}

impl Feature {
    fn is_used_by(self, capabilities: StudioCapabilities) -> bool {
        match self {
            Self::Style => capabilities.studio_style,
            Self::Motion => capabilities.motion_export,
            Self::Placement => capabilities.static_presentation,
            Self::Camera => capabilities.advanced_zoom,
            Self::Upscale => capabilities.image_upscale,
        }
    }
}

fn features_for(capabilities: StudioCapabilities) -> Vec<(Feature, bool)> {
    let mut features = [
        Feature::Style,
        Feature::Motion,
        Feature::Placement,
        Feature::Camera,
        Feature::Upscale,
    ]
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
    let dark = !has_descendant_with_class(window.upcast_ref(), "editor-theme-light");
    let Some(overlay) = overlay_for(window) else {
        crate::utils::notify::desktop_notification_important(
            &t("Studio export unavailable"),
            &error.to_string(),
        );
        return false;
    };
    present(&overlay, &error, capabilities, dark, Rc::new(retry));
    false
}

/// The overlay a card can be raised into. The image editor's window child is
/// one, while the video editor keeps its overlay under the window's root box,
/// so a direct-child cast would miss it there.
fn overlay_for(window: &ApplicationWindow) -> Option<Overlay> {
    let child = window.child()?;
    if let Ok(overlay) = child.clone().downcast::<Overlay>() {
        return Some(overlay);
    }
    first_overlay(&child)
}

fn first_overlay(root: &gtk4::Widget) -> Option<Overlay> {
    if let Ok(overlay) = root.clone().downcast::<Overlay>() {
        return Some(overlay);
    }
    let mut child = root.first_child();
    while let Some(current) = child {
        if let Some(overlay) = first_overlay(&current) {
            return Some(overlay);
        }
        child = current.next_sibling();
    }
    None
}

fn present(
    overlay: &Overlay,
    error: &StudioEntitlementError,
    capabilities: StudioCapabilities,
    dark: bool,
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
    if !dark {
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

    let (card, focus_target) = build_card(error, capabilities, dark, &dismiss, &retry);
    let entrance = Revealer::new();
    entrance.set_transition_type(RevealerTransitionType::Crossfade);
    entrance.set_transition_duration(220);
    entrance.set_halign(Align::Center);
    entrance.set_valign(Align::Center);
    entrance.set_hexpand(true);
    entrance.set_vexpand(true);
    entrance.set_child(Some(&card));
    scrim.append(&entrance);

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
        entrance.set_reveal_child(true);
        focus_target.grab_focus();
    });
}

fn build_card(
    error: &StudioEntitlementError,
    capabilities: StudioCapabilities,
    dark: bool,
    dismiss: &Rc<dyn Fn()>,
    retry: &Rc<dyn Fn()>,
) -> (GtkBox, gtk4::Widget) {
    let kind = kind_for(error);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("apexshot-studio-card");
    card.set_size_request(px(CARD_WIDTH), -1);
    card.set_overflow(gtk4::Overflow::Hidden);
    card.set_halign(Align::Center);
    card.set_valign(Align::Center);
    card.set_hexpand(true);
    card.set_vexpand(true);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("apexshot-studio-body");

    let license_row = license_entry(capabilities, dismiss, retry);
    let reveal = Revealer::new();
    reveal.set_transition_type(RevealerTransitionType::SlideDown);
    reveal.set_transition_duration(160);
    reveal.set_child(Some(&license_row.container));
    reveal.set_reveal_child(kind == Kind::InvalidLicense);

    if kind == Kind::Purchase {
        body.add_css_class("apexshot-studio-body-offer");
        card.append(&hero(dismiss));
        card.append(&body);
        let focus_target = build_offer(
            &body,
            error,
            capabilities,
            dark,
            dismiss,
            &reveal,
            &license_row.entry,
        );
        return (card, focus_target);
    }
    card.append(&body);

    let (title, subtitle) = card_copy(error);

    let heading = GtkBox::new(Orientation::Vertical, px(6.0));
    let title_row = GtkBox::new(Orientation::Horizontal, px(10.0));
    let title_label = text_label(&title, "apexshot-studio-title");
    title_label.set_hexpand(true);
    let close = close_button();
    close.connect_clicked({
        let dismiss = dismiss.clone();
        move |_| dismiss()
    });
    title_row.append(&title_label);
    title_row.append(&close);
    let subtitle_label = text_label(&subtitle, "apexshot-studio-subtitle");
    heading.append(&title_row);
    heading.append(&subtitle_label);
    body.append(&heading);

    if kind == Kind::InvalidLicense {
        body.append(&reveal);
    }

    let keep = ghost_button(&t("Keep editing"));
    keep.connect_clicked({
        let dismiss = dismiss.clone();
        move |_| dismiss()
    });

    let actions = GtkBox::new(Orientation::Horizontal, px(8.0));
    actions.set_margin_top(px(16.0));
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let primary = primary_button(&primary_action(error).label());
    connect_primary(&primary, primary_action(error), dismiss);

    let focus_target: gtk4::Widget = match kind {
        Kind::InvalidLicense | Kind::DeviceMismatch => {
            actions.append(&spacer);
            actions.append(&keep);
            actions.append(&primary);
            license_row.entry.clone().upcast()
        }
        Kind::Purchase | Kind::UpdatesEnded | Kind::ConfigurationUnavailable => {
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
    body.append(&actions);

    let note = text_label(
        &t("Capture, annotation, recording and clean exports stay free."),
        "apexshot-studio-note",
    );
    note.set_margin_top(px(12.0));
    body.append(&note);

    (card, focus_target)
}

/// Opens the action's page, or closes the card when the action has none.
fn connect_primary(primary: &Button, action: PrimaryAction, dismiss: &Rc<dyn Fn()>) {
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
}

/// The purchase card: one centred column under the hero, from the headline
/// down to the call to action and the terms beneath it.
fn build_offer(
    body: &GtkBox,
    error: &StudioEntitlementError,
    capabilities: StudioCapabilities,
    dark: bool,
    dismiss: &Rc<dyn Fn()>,
    reveal: &Revealer,
    entry: &Entry,
) -> gtk4::Widget {
    let (title, subtitle) = card_copy(error);
    body.append(&centered_label(&title, "apexshot-studio-headline"));
    let lede = centered_label(&subtitle, "apexshot-studio-lede");
    lede.set_margin_top(px(6.0));
    body.append(&lede);
    body.append(&feature_chips(capabilities, dark));
    body.append(&offer_block());

    let action = primary_action(error);
    let primary = primary_button(&action.label());
    primary.add_css_class("apexshot-studio-cta");
    primary.set_margin_top(px(16.0));
    connect_primary(&primary, action, dismiss);
    body.append(&primary);

    let links = GtkBox::new(Orientation::Horizontal, px(2.0));
    links.set_halign(Align::Center);
    links.set_margin_top(px(8.0));
    let keep = link_button(&t("Keep editing"));
    keep.connect_clicked({
        let dismiss = dismiss.clone();
        move |_| dismiss()
    });
    let separator = Label::new(Some("·"));
    separator.add_css_class("apexshot-studio-separator");
    let have_license = link_button(&t("I have a license"));
    have_license.connect_clicked({
        let reveal = reveal.clone();
        let entry = entry.clone();
        move |_| {
            let reveal_now = !reveal.reveals_child();
            reveal.set_reveal_child(reveal_now);
            if reveal_now {
                entry.grab_focus();
            }
        }
    });
    links.append(&keep);
    links.append(&separator);
    links.append(&have_license);
    body.append(&links);
    body.append(reveal);

    let fine_print = GtkBox::new(Orientation::Vertical, px(2.0));
    fine_print.set_margin_top(px(14.0));
    fine_print.append(&centered_label(
        &t("14-day refund on your first purchase."),
        "apexshot-studio-fineprint",
    ));
    fine_print.append(&centered_label(
        &t("Capture, annotation, recording and clean exports stay free."),
        "apexshot-studio-fineprint",
    ));
    body.append(&fine_print);

    primary.upcast()
}

/// The lit band at the top of the purchase card, with the app mark resting
/// in its glow and the dismiss button over its corner.
fn hero(dismiss: &Rc<dyn Fn()>) -> Overlay {
    let band = GtkBox::new(Orientation::Vertical, 0);
    band.add_css_class("apexshot-studio-hero");
    band.set_size_request(-1, px(HERO_HEIGHT));

    if let Ok(texture) = gtk4::gdk::Texture::from_bytes(&glib::Bytes::from_static(LOGO_PNG)) {
        let mark = GtkBox::new(Orientation::Vertical, 0);
        mark.add_css_class("apexshot-studio-mark");
        mark.set_overflow(gtk4::Overflow::Hidden);
        mark.set_halign(Align::Center);
        mark.set_valign(Align::Center);
        mark.set_vexpand(true);
        let picture = gtk4::Picture::for_paintable(&texture);
        picture.set_can_shrink(true);
        picture.set_content_fit(gtk4::ContentFit::Cover);
        picture.set_size_request(px(MARK_SIZE), px(MARK_SIZE));
        mark.append(&picture);
        band.append(&mark);
    }

    let overlay = Overlay::new();
    overlay.set_child(Some(&band));

    let close = close_button();
    close.add_css_class("apexshot-studio-hero-close");
    close.set_halign(Align::End);
    close.set_valign(Align::Start);
    close.set_margin_top(px(12.0));
    close.set_margin_end(px(12.0));
    close.connect_clicked({
        let dismiss = dismiss.clone();
        move |_| dismiss()
    });
    overlay.add_overlay(&close);
    overlay
}

/// Settings pairs every icon with a light-theme drawing; the card reads the
/// same package so its rows look like the pages they describe.
fn themed_icon(dark: bool, dark_icon: &'static str, light_icon: &'static str) -> &'static str {
    if dark {
        dark_icon
    } else {
        light_icon
    }
}

fn feature_copy(feature: Feature, dark: bool) -> (String, String, &'static str) {
    match feature {
        Feature::Style => (
            t("Backgrounds and style"),
            t("Wallpapers, padding, rounded corners, shadows and frames for stills."),
            custom::IMAGE_ALT_SYMBOLIC,
        ),
        Feature::Motion => (
            t("Motion timeline"),
            t("Animate your captures with titles and easing, then export as MP4."),
            themed_icon(dark, custom::SPLINE, custom::SPLINE_LIGHT),
        ),
        Feature::Placement => (
            t("Perspective and placement"),
            t("Tilt, rotate and position a still inside its background."),
            themed_icon(dark, custom::PANELS_TOP_LEFT, custom::PANELS_TOP_LEFT_LIGHT),
        ),
        Feature::Camera => (
            t("Camera controls"),
            t("Instant zooms, speed, smoothness and motion blur for recordings."),
            themed_icon(dark, custom::CAMERA, custom::CAMERA_LIGHT),
        ),
        Feature::Upscale => (
            t("Neural upscaling"),
            t("Enhance small screenshots locally before image and Motion exports."),
            custom::WAND_SPARKLES_SYMBOLIC,
        ),
    }
}

/// The features as a two-column grid of chips, each explained by its
/// tooltip. Chips this export relies on lead and carry the accent.
fn feature_chips(capabilities: StudioCapabilities, dark: bool) -> GtkBox {
    let block = GtkBox::new(Orientation::Vertical, 0);
    block.set_margin_top(px(20.0));

    let grid = gtk4::Grid::new();
    grid.set_column_homogeneous(true);
    grid.set_row_spacing(px(6.0) as u32);
    grid.set_column_spacing(px(6.0) as u32);

    let features = features_for(capabilities);
    let count = features.len() as i32;
    let mut any_used = false;
    for (index, (feature, used)) in features.into_iter().enumerate() {
        let index = index as i32;
        let (title, detail, icon_name) = feature_copy(feature, dark);

        let chip = GtkBox::new(Orientation::Horizontal, 0);
        chip.add_css_class("apexshot-studio-chip");
        if used {
            chip.add_css_class("apexshot-studio-chip-used");
            any_used = true;
        }
        chip.set_tooltip_text(Some(&detail));

        let inner = GtkBox::new(Orientation::Horizontal, px(6.0));
        inner.set_halign(Align::Center);
        inner.set_valign(Align::Center);
        inner.set_hexpand(true);
        let icon = Image::from_icon_name(icon_name);
        icon.set_pixel_size(px(14.0));
        let label = Label::new(Some(&title));
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        label.set_max_width_chars(CHIP_WIDTH_CHARS);
        inner.append(&icon);
        inner.append(&label);
        chip.append(&inner);

        if index == count - 1 && count % 2 == 1 {
            chip.set_halign(Align::Center);
            grid.attach(&chip, 0, index / 2, 2, 1);
        } else {
            grid.attach(&chip, index % 2, index / 2, 1, 1);
        }
    }
    block.append(&grid);

    if any_used {
        let caption = GtkBox::new(Orientation::Horizontal, px(6.0));
        caption.set_halign(Align::Center);
        caption.set_margin_top(px(10.0));
        let dot = GtkBox::new(Orientation::Horizontal, 0);
        dot.add_css_class("apexshot-studio-dot");
        dot.set_valign(Align::Center);
        dot.set_size_request(px(6.0), px(6.0));
        let label = Label::new(Some(&t("In this export")));
        label.add_css_class("apexshot-studio-used-caption");
        caption.append(&dot);
        caption.append(&label);
        block.append(&caption);
    }
    block
}

/// The price and what it includes, kept out of the button so the primary
/// action stays a short call to action.
fn offer_block() -> GtkBox {
    let block = GtkBox::new(Orientation::Vertical, px(4.0));
    block.set_margin_top(px(22.0));

    let price = Label::new(Some("$39"));
    price.add_css_class("apexshot-studio-price");
    price.set_halign(Align::Center);
    block.append(&price);
    block.append(&centered_label(
        &t("Includes 12 months of updates. Versions you get keep working forever."),
        "apexshot-studio-term",
    ));
    block
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
    let container = GtkBox::new(Orientation::Vertical, px(6.0));
    container.set_margin_top(px(4.0));
    container.set_margin_bottom(px(4.0));

    let row = GtkBox::new(Orientation::Horizontal, px(8.0));
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
                    Err(StudioEntitlementError::DeviceMismatch) => {
                        StudioStatus::DeviceMismatch.message()
                    }
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

fn centered_label(text: &str, class: &str) -> Label {
    let label = text_label(text, class);
    label.set_xalign(0.5);
    label.set_justify(gtk4::Justification::Center);
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

fn link_button(text: &str) -> Button {
    let button = Button::with_label(text);
    button.add_css_class("apexshot-studio-link");
    button.set_has_frame(false);
    button
}

/// The top-right dismiss. It closes the card the same way Escape does.
fn close_button() -> Button {
    let tooltip = t("Close");
    let button = Button::builder()
        .has_frame(false)
        .tooltip_text(&tooltip)
        .build();
    button.add_css_class("apexshot-studio-close");
    button.set_valign(Align::Start);
    button.set_child(Some(&Image::from_icon_name("window-close-symbolic")));
    button
}

/// The sheet is authored at 100%, like the editor's own, so the Windows build
/// runs it through the same display scaler. GDK already scales on Linux.
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
    provider.load_from_data(&scaled_css());
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
                t("Upgrade to Studio"),
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
                (Feature::Style, false),
                (Feature::Motion, false),
                (Feature::Placement, false),
                (Feature::Upscale, false),
            ]
        );
    }

    #[test]
    fn features_keep_their_order_within_each_group() {
        let features = features_for(StudioCapabilities {
            motion_export: true,
            static_presentation: true,
            advanced_zoom: false,
            image_upscale: false,
            studio_style: false,
        });
        assert_eq!(
            features,
            vec![
                (Feature::Motion, true),
                (Feature::Placement, true),
                (Feature::Style, false),
                (Feature::Camera, false),
                (Feature::Upscale, false),
            ]
        );
    }

    #[test]
    fn no_feature_is_marked_when_the_export_uses_none() {
        let features = features_for(StudioCapabilities::default());
        assert!(features.iter().all(|(_, used)| !used));
        assert_eq!(features.len(), 5);
    }

    #[test]
    fn a_styled_still_tags_backgrounds_and_style_first() {
        let features = features_for(StudioCapabilities {
            studio_style: true,
            ..StudioCapabilities::default()
        });
        assert_eq!(features[0], (Feature::Style, true));
        assert!(features[1..].iter().all(|(_, used)| !used));
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
