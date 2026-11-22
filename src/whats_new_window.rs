//! The "What's new" window for the Linux app.
//!
//! It is built from the update window's pieces so it reads as the same
//! surface: the brand mark over a centred headline, one quiet card per
//! highlight, and a bottom action row. The window owns its own GTK
//! application id, so a second launch presents the window that is already
//! open instead of building another.

use gtk4::prelude::*;
use gtk4::{Align, Application, ApplicationWindow, Box as GtkBox, Label, Orientation, Overlay};

use crate::i18n::{t, tfmt};
use crate::update_window::ui::{
    action_row, body, card, header, install_css, page, page_body, primary_button, quiet_button,
    window_controls,
};
use crate::whats_new::{self, Highlight, ReleaseNotes};

/// The window box.
const BASE_WIDTH: i32 = 560;
const BASE_HEIGHT: i32 = 620;

/// The centred column that holds the page.
const PANE_WIDTH: i32 = 480;

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

    app.connect_activate(move |application| {
        crate::capture::editor::ui_support::apply_theme_to_gtk();
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
    crate::settings::ui_support::install_settings_css();
    install_css();

    let dark = crate::capture::editor::ui_support::prefers_dark_glass_theme();
    let window = ApplicationWindow::builder()
        .application(app)
        .title(t("What's new"))
        .icon_name(crate::app_identity::icon_name())
        .default_width(BASE_WIDTH)
        .default_height(BASE_HEIGHT)
        .decorated(false)
        .build();
    window.add_css_class("editor-window");

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    root.add_css_class("onboarding-root");
    root.add_css_class("update-root");
    root.add_css_class(if dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    });
    if crate::settings::windowing::prefers_reduced_transparency() {
        root.add_css_class("editor-reduced-transparency");
    }

    let pane = GtkBox::new(Orientation::Vertical, 0);
    pane.add_css_class("update-pane");
    pane.set_size_request(PANE_WIDTH, -1);
    pane.set_halign(Align::Center);
    pane.set_hexpand(false);
    crate::onboarding::install_pane_drag(&pane, &window);

    let page = page();
    let page_body_box = body();
    page_body_box.append(&header(
        dark,
        &tfmt(
            "What's new in ApexShot {version}",
            &[("version", notes.version)],
        ),
        &t(whats_new::HEADER_COPY),
    ));

    for highlight in notes.highlights {
        page_body_box.append(&highlight_card(highlight));
    }

    let footer = Label::new(Some(&t(whats_new::FOOTER)));
    footer.add_css_class("update-footer");
    footer.set_halign(Align::Center);
    footer.set_xalign(0.5);
    footer.set_justify(gtk4::Justification::Center);
    footer.set_wrap(true);
    footer.set_max_width_chars(52);
    page_body_box.append(&footer);

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
        let see_studio = quiet_button(&t("See Studio"));
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
        action_row(Some(&see_studio), &got_it)
    } else {
        action_row(None, &got_it)
    };
    page_body(&page, &page_body_box, &actions);

    let overlay = Overlay::new();
    overlay.set_vexpand(true);
    overlay.set_child(Some(&page));
    overlay.add_overlay(&window_controls(&window));
    root.append(&overlay);
    window.set_child(Some(&root));
    window.present();
}

/// One highlight as a quiet card, with the Studio pill on Studio items.
fn highlight_card(highlight: &Highlight) -> GtkBox {
    let card = card();

    let title_row = GtkBox::new(Orientation::Horizontal, 8);
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
