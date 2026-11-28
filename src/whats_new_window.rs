//! The "What's new" window for the Linux app.
//!
//! It reads as the Studio upgrade card rather than the update prompt: the
//! brand mark and headline over the dither hero, then the highlights as a
//! quiet list and one action row. The window owns its own GTK application id,
//! so a second launch presents the window that is already open instead of
//! building another.

use gtk4::prelude::*;
use gtk4::{
    gdk, glib, Align, Application, ApplicationWindow, Box as GtkBox, Button, EventControllerKey,
    Label, Orientation, Overflow, Overlay, PropagationPhase, WindowHandle,
};

use crate::i18n::{t, tfmt};
use crate::studio_upsell::{
    centered_label, close_button, install_css, link_button, primary_button, px, LIGHT_CLASS,
};
use crate::whats_new::{self, Highlight, ReleaseNotes};

/// The card width at 100% scale.
const CARD_WIDTH: f64 = 520.0;

/// The dither hero's height at 100% scale.
const HERO_HEIGHT: f64 = 170.0;

/// Open the window for the newest release this build covers.
///
/// A build older than every entry in [`whats_new::RELEASES`] has nothing to
/// show, which is a normal outcome for development builds.
pub fn show_latest() -> anyhow::Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let Some(notes) = whats_new::notes_for(current) else {
        println!("No release highlights for ApexShot {current}.");
        return Ok(());
    };
    show(notes)
}

/// Present `notes` and block until the window closes.
pub fn show(notes: &'static ReleaseNotes) -> anyhow::Result<()> {
    // GNOME Shell needs the desktop entry's id to draw the app's own name and
    // icon instead of a generic window.
    if let Some(desktop_path) = crate::app_identity::desktop_file_for_portal() {
        std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", desktop_path);
        std::env::set_var(
            "GIO_LAUNCHED_DESKTOP_FILE_PID",
            std::process::id().to_string(),
        );
    }

    let app = Application::builder()
        // Share the app's id so the dock groups this window under ApexShot;
        // NON_UNIQUE keeps activation from going to another ApexShot window.
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    // Studio owners skip the highlights that only matter before buying; when
    // nothing is left for them, no window opens.
    let studio_active = matches!(
        crate::studio_entitlement::current_status(),
        crate::studio_entitlement::StudioStatus::Active { .. }
    );
    let highlights = whats_new::visible_highlights(notes, studio_active);
    if highlights.is_empty() {
        return Ok(());
    }

    // One advertising window at a time: the highlights wait when another
    // promo card already owns the screen.
    let Some(slot) = crate::promo_slot::try_claim(crate::promo_slot::KIND_WHATS_NEW) else {
        eprintln!("[whats-new] Another promotion window is open; not showing the highlights.");
        return Ok(());
    };

    app.connect_activate(move |application| {
        crate::capture::editor::ui_support::apply_theme_to_gtk();
        if let Some(existing) = application.windows().first() {
            existing.present();
            return;
        }
        build_window(application, notes, &highlights, studio_active);
    });

    let _ = app.run_with_args::<String>(&[]);
    drop(slot);
    Ok(())
}

fn build_window(
    app: &Application,
    notes: &'static ReleaseNotes,
    highlights: &[&'static whats_new::Highlight],
    studio_active: bool,
) {
    install_css();

    let dark = crate::capture::editor::ui_support::prefers_dark_glass_theme();
    let window = ApplicationWindow::builder()
        .application(app)
        .title(t("What's new"))
        .icon_name(crate::app_identity::icon_name())
        .default_width(px(CARD_WIDTH))
        .decorated(false)
        .resizable(false)
        .build();
    window.add_css_class("apexshot-whats-new");

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("apexshot-studio-card");
    if !dark {
        card.add_css_class(LIGHT_CLASS);
    }
    card.set_size_request(px(CARD_WIDTH), -1);
    card.set_overflow(Overflow::Hidden);

    card.append(&hero(dark, notes, &window));

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("apexshot-whats-new-body");
    body.set_halign(Align::Fill);
    for (index, highlight) in highlights.iter().enumerate() {
        if index > 0 {
            body.append(&divider());
        }
        body.append(&highlight_row(highlight));
    }

    let footer = centered_label(&t(whats_new::FOOTER), "apexshot-studio-fineprint");
    footer.set_max_width_chars(52);
    footer.set_margin_top(px(16.0));
    body.append(&footer);

    let got_it = primary_button(&t("Got it"));
    got_it.add_css_class("apexshot-whats-new-action");
    got_it.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });

    let has_studio_highlight = highlights.iter().any(|highlight| highlight.studio);
    let actions = if has_studio_highlight && !studio_active {
        let see_studio = link_button(&t("Explore Studio"));
        see_studio.connect_clicked({
            let window = window.clone();
            move |_| {
                let url = crate::studio_entitlement::studio_page_url();
                // Open before closing: closing the last window ends this
                // process, which would take a background thread with it.
                // The launch only hands the link to the desktop, so it is quick.
                if let Err(error) = crate::utils::open::open_url(&url) {
                    eprintln!("[whats-new] Could not open the Studio page: {error}");
                }
                window.close();
            }
        });
        action_row(Some(&see_studio), &got_it)
    } else {
        action_row(None, &got_it)
    };
    body.append(&actions);

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

/// The card's top: the dither field with the brand mark and headline set over
/// its quiet lower edge, and the dismiss in the opposite corner.
fn hero(dark: bool, notes: &ReleaseNotes, window: &ApplicationWindow) -> Overlay {
    let hero = Overlay::new();
    hero.set_child(Some(&crate::dither_field::dither_field(dark, HERO_HEIGHT)));

    let words = GtkBox::new(Orientation::Vertical, px(8.0));
    words.set_valign(Align::End);
    words.set_margin_start(px(28.0));
    words.set_margin_end(px(28.0));
    words.set_margin_bottom(px(18.0));
    words.append(&crate::onboarding::showcase::brand_mark(dark));

    let headline = centered_label(
        &tfmt(
            "What's new in ApexShot {version}",
            &[("version", notes.version)],
        ),
        "apexshot-studio-headline",
    );
    headline.add_css_class("apexshot-studio-headline-compact");
    headline.set_max_width_chars(30);
    words.append(&headline);

    if let Some(subtitle) = notes.subtitle {
        let lede = centered_label(&t(subtitle), "apexshot-studio-lede");
        lede.set_max_width_chars(52);
        words.append(&lede);
    }
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

/// One highlight as a list row, with the Studio pill on Studio items.
fn highlight_row(highlight: &Highlight) -> GtkBox {
    let row = GtkBox::new(Orientation::Vertical, 0);
    row.add_css_class("apexshot-whats-new-row");
    row.set_halign(Align::Fill);
    row.set_hexpand(true);

    let title_row = GtkBox::new(Orientation::Horizontal, px(12.0));
    title_row.set_halign(Align::Fill);
    title_row.set_hexpand(true);

    let title = Label::new(Some(&t(highlight.title)));
    title.add_css_class("apexshot-whats-new-title");
    title.set_halign(Align::Start);
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_wrap(true);
    title_row.append(&title);

    if highlight.studio {
        let pill = Label::new(Some(&t("Studio")));
        pill.add_css_class("apexshot-whats-new-pill");
        pill.set_halign(Align::End);
        pill.set_valign(Align::Center);
        title_row.append(&pill);
    }
    row.append(&title_row);

    let copy = Label::new(Some(&t(highlight.body)));
    copy.add_css_class("apexshot-whats-new-copy");
    copy.set_halign(Align::Start);
    copy.set_xalign(0.0);
    copy.set_wrap(true);
    copy.set_max_width_chars(52);
    row.append(&copy);

    row
}

/// The hairline between two highlights, in the card's rim colour.
fn divider() -> GtkBox {
    let divider = GtkBox::new(Orientation::Horizontal, 0);
    divider.add_css_class("apexshot-whats-new-divider");
    divider.set_halign(Align::Fill);
    divider
}

/// The secondary choice on the left, the primary on the right. A lone action
/// sits in the middle.
fn action_row(left: Option<&Button>, primary: &Button) -> GtkBox {
    let actions = GtkBox::new(Orientation::Horizontal, px(12.0));
    actions.set_halign(Align::Fill);
    actions.set_margin_top(px(18.0));

    let spacer = || {
        let spacer = GtkBox::new(Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        spacer
    };

    primary.set_valign(Align::Center);
    match left {
        Some(left) => {
            left.set_halign(Align::Start);
            left.set_valign(Align::Center);
            primary.set_halign(Align::End);
            actions.append(left);
            actions.append(&spacer());
            actions.append(primary);
        }
        None => {
            primary.set_halign(Align::Center);
            actions.append(&spacer());
            actions.append(primary);
            actions.append(&spacer());
        }
    }
    actions
}
