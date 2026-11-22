//! The "What's new" window for the Windows build.
//!
//! It follows the update window's surface: the brand mark over a centred
//! headline, one quiet card per highlight, and a bottom action row. The
//! decision about whether this start should show it is shared with the Linux
//! app through `whats_new`.

use gtk4::prelude::*;
use gtk4::{Align, Application, ApplicationWindow, Box as GtkBox, Label, Orientation, Overlay};

use crate::i18n::{t, tfmt};
use crate::update::window::{
    action_row, body, card, header_parts, install_css, outlined_button, page, page_body,
    primary_button, px, window_controls,
};
use crate::whats_new::{self, Highlight, ReleaseNotes};

/// The window box at 100% display scaling.
const BASE_WIDTH: f64 = 560.0;
const BASE_HEIGHT: f64 = 620.0;

/// The centred column that holds the page.
const PANE_WIDTH: f64 = 480.0;

/// Decide whether this start should show the highlights, then do it.
///
/// The shell starts this binary once per start; the state file and the
/// onboarding flag make sure a fresh install never sees the window and an
/// upgrade sees it once.
pub fn show_on_startup() -> anyhow::Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let last_seen = whats_new::read_last_seen();
    match whats_new::decide_on_startup(
        current,
        last_seen.as_deref(),
        crate::onboarding::is_complete(),
    ) {
        whats_new::StartupDecision::Show(notes) => {
            // Recorded before the window opens, so a crash while showing it
            // cannot make the same release announce itself again.
            whats_new::write_last_seen(current);
            show(notes)
        }
        whats_new::StartupDecision::MarkSeen => {
            whats_new::write_last_seen(current);
            Ok(())
        }
        whats_new::StartupDecision::Nothing => Ok(()),
    }
}

/// Present `notes` and block until the window closes.
fn show(notes: &'static ReleaseNotes) -> anyhow::Result<()> {
    let app = gtk4::Application::builder()
        // Share the app's id so the dock groups this window under ApexShot;
        // NON_UNIQUE keeps activation from going to another ApexShot window.
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |application| {
        crate::platform::sharpen_text();
        if let Some(existing) = application.windows().first() {
            existing.present();
            return;
        }
        build_window(application, notes);
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn build_window(app: &Application, notes: &'static ReleaseNotes) {
    crate::icons::install();
    crate::settings::ui_support::install_settings_css();
    crate::settings::install_window_css();
    let scale = crate::platform::ui_scale();
    crate::onboarding::install_css(scale);
    install_css(scale);

    let window = ApplicationWindow::builder()
        .application(app)
        .title(t("What's new"))
        .icon_name(crate::app_identity::icon_name())
        .default_width(px(scale, BASE_WIDTH))
        .default_height(px(scale, BASE_HEIGHT))
        .build();
    window.set_decorated(false);
    window.add_css_class("editor-window");

    let dark = crate::settings::windowing::prefers_dark_glass_theme();
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    root.add_css_class("onboarding-root");
    root.add_css_class("update-root");
    if !dark {
        root.add_css_class("editor-theme-light");
    }
    if crate::settings::windowing::prefers_reduced_transparency() {
        root.add_css_class("editor-reduced-transparency");
    }

    let pane = GtkBox::new(Orientation::Vertical, 0);
    pane.add_css_class("update-pane");
    pane.set_size_request(px(scale, PANE_WIDTH), -1);
    pane.set_halign(Align::Center);
    pane.set_hexpand(false);
    crate::settings::windowing::install_window_drag(&pane, &window);

    let page = page(scale);
    let reading = body();
    let subtitle = notes.subtitle.map(|text| t(text)).unwrap_or_default();
    let (heading, subtitle_label) = header_parts(
        scale,
        dark,
        &tfmt(
            "What's new in ApexShot {version}",
            &[("version", notes.version)],
        ),
        &subtitle,
    );
    // Releases without a supporting line hide it instead of leaving a gap.
    subtitle_label.set_visible(notes.subtitle.is_some());
    reading.append(&heading);

    for highlight in notes.highlights {
        reading.append(&highlight_card(scale, highlight));
    }

    let footer = Label::new(Some(&t(whats_new::FOOTER)));
    footer.add_css_class("update-footer");
    footer.set_halign(Align::Center);
    footer.set_xalign(0.5);
    footer.set_justify(gtk4::Justification::Center);
    footer.set_wrap(true);
    footer.set_max_width_chars(52);
    reading.append(&footer);

    let got_it = primary_button(&t("Got it"));
    got_it.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });

    let studio_active = matches!(
        crate::studio_entitlement::current_status(),
        crate::studio_entitlement::StudioStatus::Active { .. }
    );
    let has_studio_highlight = notes.highlights.iter().any(|highlight| highlight.studio);
    let actions = if has_studio_highlight && !studio_active {
        let see_studio = outlined_button(&t("Explore Studio"));
        see_studio.connect_clicked({
            let window = window.clone();
            move |_| {
                let url = crate::studio_entitlement::studio_page_url();
                std::thread::spawn(move || {
                    if let Err(error) = crate::utils::open::open_url(&url) {
                        eprintln!("[whats-new] Could not open the Studio page: {error}");
                    }
                });
                window.close();
            }
        });
        action_row(scale, Some(&see_studio), &got_it)
    } else {
        action_row(scale, None, &got_it)
    };
    page_body(&page, &reading, &actions);

    let overlay = Overlay::new();
    overlay.set_vexpand(true);
    overlay.set_child(Some(&page));
    overlay.add_overlay(&window_controls(&window, scale));
    root.append(&overlay);
    window.set_child(Some(&root));
    window.present();
}

/// One highlight as a quiet card, with the Studio pill on Studio items.
fn highlight_card(scale: f64, highlight: &Highlight) -> GtkBox {
    let card = card(scale);

    let title_row = GtkBox::new(Orientation::Horizontal, px(scale, 8.0));
    title_row.set_halign(Align::Fill);
    title_row.set_hexpand(true);

    let title = Label::new(Some(&t(highlight.title)));
    title.add_css_class("update-highlight-title");
    title.set_halign(Align::Start);
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_wrap(true);
    title_row.append(&title);

    if highlight.studio {
        let pill = Label::new(Some(&t("Studio")));
        pill.add_css_class("update-studio-pill");
        pill.set_halign(Align::End);
        pill.set_valign(Align::Center);
        title_row.append(&pill);
    }
    card.append(&title_row);

    let body = Label::new(Some(&t(highlight.body)));
    body.add_css_class("update-highlight-body");
    body.set_halign(Align::Start);
    body.set_xalign(0.0);
    body.set_wrap(true);
    body.set_max_width_chars(52);
    card.append(&body);

    card
}
