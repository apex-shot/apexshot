//! The two small AI names cards: the trial ran out, or AI names is worth a try.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gtk4::{
    gdk, glib, prelude::*, Align, Application, Box as GtkBox, EventControllerKey, Label,
    Orientation, Overlay, PropagationPhase, Window, WindowHandle,
};

use super::trial::{CardKind, TRIAL_NAMES};
use crate::i18n::t;
use crate::studio_upsell::{
    centered_label, close_button, install_css, link_button, primary_button, px, theme_is_dark,
    LIGHT_CLASS,
};

const CARD_WIDTH: f64 = 480.0;
const CTA_WIDTH: f64 = 320.0;

/// Run one AI names card as its own small window; returns the exit code.
pub fn run_card(kind: CardKind) -> i32 {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        if app.windows().iter().any(|window| window.is_visible()) {
            return;
        }
        build_card(app, kind);
    });

    let _ = app.run_with_args::<String>(&[]);
    0
}

fn build_card(app: &Application, kind: CardKind) {
    install_css();
    install_example_css();

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

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("apexshot-studio-body");
    body.set_margin_top(px(24.0));

    let (title, lede) = match kind {
        CardKind::TrialEnded => (
            t("Your free AI names are used up"),
            t("New screenshots are named from the window title again. Here is what AI names did for yours:"),
        ),
        CardKind::TryAiNames => (
            t("Want clearer screenshot names?"),
            t("Your screenshots are named from their window titles. AI names also reads what is on screen, using a small model on this PC."),
        ),
    };

    let title_label = centered_label(&title, "apexshot-studio-headline");
    title_label.add_css_class("apexshot-studio-headline-compact");
    title_label.set_max_width_chars(26);
    body.append(&title_label);

    let lede_label = centered_label(&lede, "apexshot-studio-lede");
    lede_label.set_max_width_chars(48);
    lede_label.set_margin_top(px(6.0));
    body.append(&lede_label);

    let pairs = match kind {
        CardKind::TrialEnded => {
            let own = example_pairs();
            if own.is_empty() {
                sample_pairs()
            } else {
                own
            }
        }
        CardKind::TryAiNames => sample_pairs(),
    };
    let examples = GtkBox::new(Orientation::Vertical, px(6.0));
    examples.set_halign(Align::Center);
    examples.set_margin_top(px(14.0));
    for (free, ai) in pairs.iter().take(3) {
        examples.append(&example_line(free, ai));
    }
    body.append(&examples);

    let studio_active = crate::studio_entitlement::studio_unlocked();
    let primary_label = match kind {
        CardKind::TrialEnded => t("Upgrade to Studio"),
        CardKind::TryAiNames if studio_active => t("Set up AI names"),
        CardKind::TryAiNames => t("Try AI names"),
    };
    let primary = primary_button(&primary_label);
    primary.add_css_class("apexshot-studio-cta");
    primary.set_halign(Align::Center);
    primary.set_size_request(px(CTA_WIDTH), -1);
    primary.set_margin_top(px(18.0));
    match kind {
        CardKind::TrialEnded => {
            primary.connect_clicked({
                let window = window.clone();
                move |_| open_studio_and_close(&window)
            });
        }
        CardKind::TryAiNames => {
            primary.connect_clicked({
                let window = window.clone();
                move |_| open_ai_setup_and_close(&window)
            });
        }
    }
    body.append(&primary);

    match kind {
        CardKind::TrialEnded => {
            let hint = centered_label(
                &t("$39 once · also includes Motion export, Upscale, and Studio styles"),
                "apexshot-studio-note",
            );
            hint.set_max_width_chars(46);
            hint.set_margin_top(px(8.0));
            body.append(&hint);
        }
        CardKind::TryAiNames if !studio_active => {
            let count = TRIAL_NAMES.to_string();
            let hint = centered_label(
                &t_args("{} free names, then part of Studio", &[count.as_str()]),
                "apexshot-studio-note",
            );
            hint.set_max_width_chars(46);
            hint.set_margin_top(px(8.0));
            body.append(&hint);
        }
        CardKind::TryAiNames => {}
    }

    let not_now = link_button(&t("Not now"));
    not_now.set_halign(Align::Center);
    not_now.set_margin_top(px(4.0));
    not_now.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    body.append(&not_now);

    let content = Overlay::new();
    content.set_child(Some(&body));
    let close = close_button();
    close.set_halign(Align::End);
    close.set_valign(Align::Start);
    close.set_margin_top(px(12.0));
    close.set_margin_end(px(12.0));
    close.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    content.add_overlay(&close);
    card.append(&content);

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

/// Open the Studio page, then close the card once the hand-off is done.
fn open_studio_and_close(window: &Window) {
    let done = Arc::new(AtomicBool::new(false));
    let finished = done.clone();
    std::thread::spawn(move || {
        if let Err(error) =
            crate::utils::open::open_url(&crate::studio_entitlement::studio_page_url())
        {
            eprintln!("[ai-names] Could not open the Studio page: {error}");
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

/// Start this executable's `--studio` setup, then close the card.
fn open_ai_setup_and_close(window: &Window) {
    if let Ok(exe) = std::env::current_exe() {
        let _ = crate::utils::hidden_command(exe)
            .arg("--studio")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    window.close();
}

/// One free name, an arrow, and the AI name that replaces it.
fn example_line(free: &str, ai: &str) -> GtkBox {
    let line = GtkBox::new(Orientation::Horizontal, px(8.0));
    let before = Label::new(Some(free));
    before.add_css_class("studio-example-free");
    before.set_xalign(0.0);
    line.append(&before);
    let arrow = Label::new(Some("→"));
    arrow.add_css_class("apexshot-studio-note");
    line.append(&arrow);
    let after = Label::new(Some(ai));
    after.add_css_class("studio-example-ai");
    after.set_xalign(0.0);
    after.set_wrap(true);
    line.append(&after);
    line
}

/// Up to three recent captures the organizer renamed, newest first.
fn example_pairs() -> Vec<(String, String)> {
    let Some(path) = crate::config::organizer_index_path() else {
        return Vec::new();
    };
    let Ok(index) = apexshot_organizer::index::Index::load(&path) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = index
        .entries()
        .iter()
        .filter(|entry| entry.ai_named)
        .filter_map(|entry| {
            let free = entry.heuristic_name.clone()?;
            let ai = entry.path.file_stem()?.to_string_lossy().to_string();
            Some((entry.captured_at, free, ai))
        })
        .collect();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    entries
        .into_iter()
        .take(3)
        .map(|(_, free, ai)| (free, ai))
        .collect()
}

/// The pairs shown until the index has captures the user renamed.
fn sample_pairs() -> Vec<(String, String)> {
    [
        ("Home X", "Sam Lee post announcing a new keyboard"),
        ("Inbox Outlook", "Invoice reminder from Northwind Traders"),
        (
            "main.rs apexshot",
            "Rust organizer module with naming tests",
        ),
    ]
    .iter()
    .map(|(free, ai)| (free.to_string(), ai.to_string()))
    .collect()
}

/// `t` with positional `{}` placeholders filled left to right.
fn t_args(msgid: &str, values: &[&str]) -> String {
    let mut text = t(msgid);
    for value in values {
        text = text.replacen("{}", value, 1);
    }
    text
}

const EXAMPLE_CSS: &str = r#"
    .apexshot-studio-card label.studio-example-free {
        font-size: 12px;
        color: rgba(255, 255, 255, 0.62);
    }
    .apexshot-studio-card label.studio-example-ai {
        font-size: 12px;
        font-weight: 700;
        color: #8fdcab;
    }
    .apexshot-studio-card.apexshot-studio-light label.studio-example-free {
        color: rgba(29, 33, 41, 0.60);
    }
    .apexshot-studio-card.apexshot-studio-light label.studio-example-ai {
        color: #17643e;
    }
"#;

fn install_example_css() {
    thread_local! {
        static INSTALLED: Cell<bool> = const { Cell::new(false) };
    }
    if INSTALLED.with(Cell::get) {
        return;
    }
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(&crate::css_scale::scale_px_lengths(
        EXAMPLE_CSS,
        crate::platform::ui_scale(),
    ));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    INSTALLED.with(|installed| installed.set(true));
}
