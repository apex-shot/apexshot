//! The Black Friday card: the offer lives only here, shown at most once a day
//! after the first capture while it runs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gtk4::{
    gdk, glib, prelude::*, Align, Application, Box as GtkBox, Button, EventControllerKey,
    Orientation, Overlay, PropagationPhase, Window, WindowHandle,
};

use crate::i18n::t;
use crate::studio_offer::BLACK_FRIDAY_CODE;
use crate::studio_upsell::{
    centered_label, close_button, install_css, link_button, primary_button, px, theme_is_dark,
    LIGHT_CLASS,
};

const CARD_WIDTH: f64 = 440.0;
const CTA_WIDTH: f64 = 320.0;
/// The dither hero's height at 100% scale, as on What's New.
const HERO_HEIGHT: f64 = 170.0;

/// Run the Black Friday card as its own small window; returns the exit code.
///
/// The card claims the shared promo slot first, so it never opens beside
/// another card that advertises something. Only once the slot is its own does
/// it record the day, so a suppressed card does not spend the day's card.
pub fn run_card() -> i32 {
    let Some(_slot) = crate::promo_slot::try_claim(crate::promo_slot::KIND_BLACK_FRIDAY) else {
        eprintln!("[black-friday] Another promotion window is open; not showing the card.");
        return 0;
    };
    crate::studio_offer::mark_black_friday_card_shown();

    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(|app| {
        if app.windows().iter().any(|window| window.is_visible()) {
            return;
        }
        build_card(app);
    });

    let _ = app.run_with_args::<String>(&[]);
    0
}

fn build_card(app: &Application) {
    install_css();

    let window = Window::builder()
        .application(app)
        .decorated(false)
        .resizable(false)
        .default_width(px(CARD_WIDTH))
        .build();
    window.add_css_class("apexshot-studio-prompt");
    window.set_title(Some(crate::app_identity::app_name()));

    let dark = theme_is_dark(&window);
    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("apexshot-studio-card");
    if !dark {
        card.add_css_class(LIGHT_CLASS);
    }
    card.set_size_request(px(CARD_WIDTH), -1);
    card.set_overflow(gtk4::Overflow::Hidden);

    card.append(&hero(dark, &window));

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("apexshot-studio-body");
    body.set_margin_top(px(4.0));

    let lede = centered_label(
        &t("$19.50 instead of $39, until November 30. Motion export, Upscale, Studio styles, and AI names."),
        "apexshot-studio-lede",
    );
    lede.set_max_width_chars(40);
    lede.set_margin_top(px(6.0));
    body.append(&lede);

    let code = Button::with_label(BLACK_FRIDAY_CODE);
    code.add_css_class("apexshot-black-friday-code");
    code.set_halign(Align::Center);
    code.set_margin_top(px(18.0));
    code.set_tooltip_text(Some(&t("Copy code")));
    code.connect_clicked(|button| {
        button.display().clipboard().set_text(BLACK_FRIDAY_CODE);
        button.set_label(&t("Copied"));
        let button = button.clone();
        glib::timeout_add_local_once(Duration::from_millis(1500), move || {
            button.set_label(BLACK_FRIDAY_CODE);
        });
    });
    body.append(&code);

    let get_studio = primary_button(&t("Get Studio"));
    get_studio.add_css_class("apexshot-studio-cta");
    get_studio.set_halign(Align::Center);
    get_studio.set_size_request(px(CTA_WIDTH), -1);
    get_studio.set_margin_top(px(14.0));
    get_studio.connect_clicked({
        let window = window.clone();
        move |_| open_studio_and_close(&window)
    });
    body.append(&get_studio);

    let not_now = link_button(&t("Not now"));
    not_now.set_halign(Align::Center);
    not_now.set_margin_top(px(4.0));
    not_now.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    body.append(&not_now);

    card.append(&body);

    // The whole card is the window's drag handle.
    let handle = WindowHandle::new();
    handle.set_child(Some(&card));
    window.set_child(Some(&handle));

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
}

/// The card's top, as on What's New: the dither field with the brand mark and
/// headline set over its quiet lower edge, and the dismiss in the corner.
fn hero(dark: bool, window: &Window) -> Overlay {
    let hero = Overlay::new();
    hero.set_child(Some(&crate::dither_field::dither_field(dark, HERO_HEIGHT)));

    let words = GtkBox::new(Orientation::Vertical, px(8.0));
    words.set_valign(Align::End);
    words.set_margin_start(px(28.0));
    words.set_margin_end(px(28.0));
    words.set_margin_bottom(px(14.0));
    #[cfg(target_os = "windows")]
    words.append(&crate::onboarding::showcase::brand_mark(
        crate::platform::ui_scale(),
        dark,
    ));
    #[cfg(not(target_os = "windows"))]
    words.append(&crate::onboarding::showcase::brand_mark(dark));

    let title = centered_label(
        &t("Black Friday: Studio is 50% off"),
        "apexshot-studio-headline",
    );
    title.add_css_class("apexshot-studio-headline-compact");
    title.add_css_class("apexshot-black-friday-headline");
    title.set_max_width_chars(40);
    words.append(&title);
    hero.add_overlay(&words);

    let close = close_button();
    close.set_halign(Align::End);
    close.set_valign(Align::Start);
    close.set_margin_top(px(12.0));
    close.set_margin_end(px(12.0));
    close.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    hero.add_overlay(&close);
    hero
}

/// Open the Studio page with the offer's code, then close the card once the
/// hand-off is done.
fn open_studio_and_close(window: &Window) {
    let done = Arc::new(AtomicBool::new(false));
    let finished = done.clone();
    std::thread::spawn(move || {
        if let Err(error) =
            crate::utils::open::open_url(&crate::studio_offer::black_friday_studio_url())
        {
            eprintln!("[black-friday] Could not open the Studio page: {error}");
        }
        finished.store(true, Ordering::Release);
    });
    let card = window.downgrade();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        if !done.load(Ordering::Acquire) {
            return glib::ControlFlow::Continue;
        }
        if let Some(window) = card.upgrade() {
            window.close();
        }
        glib::ControlFlow::Break
    });
}
