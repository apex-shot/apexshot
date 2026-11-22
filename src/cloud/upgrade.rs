//! The upgrade card a cloud export shows when the account is out of space.

use crate::i18n::t;
use crate::studio_upsell::{
    centered_label, close_button, install_css, link_button, primary_button, px, theme_is_dark,
    LIGHT_CLASS,
};
use gtk4::{
    gdk, glib, prelude::*, Align, Box as GtkBox, EventControllerKey, Orientation, Overlay,
    PropagationPhase, Window,
};

pub const BILLING_URL: &str = "https://apexshot.org/dashboard/settings/billing";

pub fn show_prompt(window: &impl IsA<Window>, message: &str) {
    install_css();
    let parent: &Window = window.upcast_ref();
    let dark = theme_is_dark(parent);

    let modal = Window::builder()
        .transient_for(window)
        .modal(true)
        .decorated(false)
        .resizable(false)
        .default_width(px(480.0))
        .build();
    modal.add_css_class("apexshot-studio-prompt");
    modal.set_destroy_with_parent(true);

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
    let headline = centered_label(&t("Your capture is saved"), "apexshot-studio-headline");
    headline.add_css_class("apexshot-studio-headline-compact");
    headline.set_max_width_chars(26);
    let lede = centered_label(message, "apexshot-studio-lede");
    lede.set_max_width_chars(52);
    words.append(&headline);
    words.append(&lede);
    hero.add_overlay(&words);

    let close = close_button();
    close.set_halign(Align::End);
    close.set_valign(Align::Start);
    close.set_margin_top(px(12.0));
    close.set_margin_end(px(12.0));
    close.connect_clicked({
        let modal = modal.downgrade();
        move |_| {
            if let Some(modal) = modal.upgrade() {
                modal.close();
            }
        }
    });
    hero.add_overlay(&close);
    card.append(&hero);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("apexshot-studio-body");

    let go_pro = primary_button(&t("Go Pro"));
    go_pro.add_css_class("apexshot-studio-cta");
    go_pro.set_halign(Align::Center);
    go_pro.set_size_request(px(320.0), -1);
    go_pro.connect_clicked({
        let modal = modal.downgrade();
        move |_| {
            std::thread::spawn(|| {
                if let Err(error) = crate::utils::open::open_url(BILLING_URL) {
                    crate::utils::notify::desktop_notification(
                        &t("Could not open billing"),
                        &error,
                    );
                }
            });
            if let Some(modal) = modal.upgrade() {
                modal.close();
            }
        }
    });
    body.append(&go_pro);

    let keep = link_button(&t("Keep working"));
    keep.set_halign(Align::Center);
    keep.set_margin_top(px(8.0));
    keep.connect_clicked({
        let modal = modal.downgrade();
        move |_| {
            if let Some(modal) = modal.upgrade() {
                modal.close();
            }
        }
    });
    body.append(&keep);
    card.append(&body);

    modal.set_child(Some(&card));

    let keys = EventControllerKey::new();
    keys.set_propagation_phase(PropagationPhase::Capture);
    keys.connect_key_pressed({
        let modal = modal.downgrade();
        move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(modal) = modal.upgrade() {
                    modal.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    });
    modal.add_controller(keys);

    modal.present();
}
