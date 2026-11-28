//! The two small AI names cards: the trial ran out, or AI names is worth a try.

use std::cell::Cell;
use std::process::{Command, Stdio};
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

const CARD_WIDTH: f64 = 440.0;
const CTA_WIDTH: f64 = 320.0;

/// Run one AI names card as its own small window; returns the exit code.
///
/// The card claims the shared promo slot first, so it never opens beside
/// another card that advertises something.
pub fn run_card(kind: CardKind) -> i32 {
    let Some(_slot) = crate::promo_slot::try_claim(crate::promo_slot::KIND_AI_NAMES) else {
        eprintln!("[ai-names] Another promotion window is open; not showing the card.");
        return 0;
    };

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

/// Start the card as its own process; a failure leaves the capture untouched.
pub fn spawn_card(kind: CardKind) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut command = Command::new(exe);
    command.args(["ai-names-card", kind.arg()]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _ = command.spawn();
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

    let named = ai_named_count();
    let (title, lede) = match kind {
        CardKind::TrialEnded if named > 0 => (
            t_args(
                "AI named {} of your screenshots",
                &[named.to_string().as_str()],
            ),
            t("That was the free trial. New screenshots go back to window-title names."),
        ),
        CardKind::TrialEnded => (
            t("Your free AI names are used up"),
            t("That was the free trial. New screenshots go back to window-title names."),
        ),
        CardKind::TryAiNames => (
            t("Want clearer screenshot names?"),
            t("Your screenshots are named from their window titles. AI names also reads what is on screen, using a small model on this PC."),
        ),
    };

    let title_label = centered_label(&title, "apexshot-studio-headline");
    title_label.add_css_class("apexshot-studio-headline-compact");
    title_label.add_css_class("ai-names-headline");
    title_label.set_max_width_chars(32);
    let title_line = GtkBox::new(Orientation::Horizontal, px(8.0));
    title_line.set_halign(Align::Center);
    title_line.set_valign(Align::Center);
    title_line.append(&title_label);
    let beta = Label::new(Some(&t("Beta")));
    beta.add_css_class("ai-names-tag");
    beta.set_valign(Align::Center);
    title_line.append(&beta);
    body.append(&title_line);

    let lede_label = centered_label(&lede, "apexshot-studio-lede");
    lede_label.set_max_width_chars(40);
    lede_label.set_margin_top(px(6.0));
    body.append(&lede_label);

    let pairs = match kind {
        CardKind::TrialEnded => {
            let own = example_pairs(3);
            if own.is_empty() {
                sample_pairs()
            } else {
                own
            }
        }
        CardKind::TryAiNames => sample_pairs(),
    };
    let examples = GtkBox::new(Orientation::Vertical, 0);
    examples.add_css_class("ai-names-examples");
    examples.set_margin_top(px(16.0));
    examples.set_margin_start(px(28.0));
    examples.set_margin_end(px(28.0));
    for (free, ai) in pairs.iter().take(3) {
        examples.append(&example_tile(free, ai));
    }
    body.append(&examples);

    let studio_active = crate::studio_entitlement::studio_unlocked();
    let primary_label = match kind {
        CardKind::TrialEnded => t("Keep AI names"),
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
                &crate::studio_offer::ai_names_price_note(),
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

/// Open Settings on the Studio page, then close the card.
fn open_ai_setup_and_close(window: &Window) {
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe)
            .args(["settings", "--studio"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    window.close();
}

/// The free name, small and quiet, over the AI name that replaced it.
fn example_tile(free: &str, ai: &str) -> GtkBox {
    let tile = GtkBox::new(Orientation::Vertical, px(3.0));
    tile.add_css_class("ai-names-example");

    let before = Label::new(Some(free));
    before.add_css_class("ai-names-before");
    before.set_xalign(0.0);
    before.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    before.set_max_width_chars(40);
    tile.append(&before);

    let line = GtkBox::new(Orientation::Horizontal, px(8.0));
    let tag = Label::new(Some("AI"));
    tag.add_css_class("ai-names-tag");
    tag.set_valign(Align::Center);
    line.append(&tag);
    let after = Label::new(Some(ai));
    after.add_css_class("ai-names-after");
    after.set_xalign(0.0);
    after.set_hexpand(true);
    after.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    after.set_max_width_chars(36);
    line.append(&after);
    tile.append(&line);
    tile
}

/// How many captures the model has named.
fn ai_named_count() -> usize {
    crate::config::organizer_index_path()
        .and_then(|path| apexshot_organizer::index::Index::load(&path).ok())
        .map(|index| {
            index
                .entries()
                .iter()
                .filter(|entry| entry.ai_named)
                .count()
        })
        .unwrap_or(0)
}

/// The user's own renames that show the model best: names that really
/// changed, the biggest gain in words first, then the newest.
pub(crate) fn example_pairs(limit: usize) -> Vec<(String, String)> {
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
            let stem = entry.path.file_stem()?.to_string_lossy().to_string();
            // Show older names with today's cleanup (stray OCR letters,
            // density markers), falling back to the stem as saved.
            let ai = apexshot_organizer::ai::clean_reply(&stem).unwrap_or_else(|| {
                apexshot_organizer::ai::strip_density_marker(stem.trim()).to_string()
            });
            // Names from before the readability check could be OCR noise;
            // only a name that reads as words is worth showing off.
            let ai = apexshot_organizer::quality::readable_name(&ai)?;
            if ai.trim().eq_ignore_ascii_case(free.trim()) {
                return None;
            }
            let gain =
                ai.split_whitespace().count() as i64 - free.split_whitespace().count() as i64;
            Some((gain, entry.captured_at, free, ai))
        })
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    entries
        .into_iter()
        .take(limit)
        .map(|(_, _, free, ai)| (free, ai))
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
    .apexshot-studio-card label.ai-names-headline {
        font-size: 24px;
    }
    .apexshot-studio-card box.ai-names-examples {
        border-radius: 12px;
        background-color: rgba(255, 255, 255, 0.05);
        border: 1px solid rgba(255, 255, 255, 0.08);
    }
    .apexshot-studio-card box.ai-names-example {
        padding: 10px 14px;
    }
    .apexshot-studio-card box.ai-names-example + box.ai-names-example {
        border-top: 1px solid rgba(255, 255, 255, 0.07);
    }
    .apexshot-studio-card label.ai-names-before {
        font-size: 12px;
        color: rgba(255, 255, 255, 0.50);
    }
    .apexshot-studio-card label.ai-names-after {
        font-size: 14px;
        font-weight: 600;
        color: rgba(255, 255, 255, 0.94);
    }
    .apexshot-studio-card label.ai-names-tag {
        font-size: 10px;
        font-weight: 700;
        padding: 1px 6px;
        border-radius: 999px;
        color: #ff670d;
        background-color: rgba(255, 103, 13, 0.14);
    }
    .apexshot-studio-card.apexshot-studio-light box.ai-names-examples {
        background-color: rgba(29, 33, 41, 0.04);
        border-color: rgba(29, 33, 41, 0.10);
    }
    .apexshot-studio-card.apexshot-studio-light box.ai-names-example + box.ai-names-example {
        border-top-color: rgba(29, 33, 41, 0.08);
    }
    .apexshot-studio-card.apexshot-studio-light label.ai-names-before {
        color: rgba(29, 33, 41, 0.55);
    }
    .apexshot-studio-card.apexshot-studio-light label.ai-names-after {
        color: rgba(29, 33, 41, 0.92);
    }
    .apexshot-studio-card.apexshot-studio-light label.ai-names-tag {
        color: #c94f05;
        background-color: rgba(255, 103, 13, 0.12);
    }
"#;

fn install_example_css() {
    thread_local! {
        static INSTALLED: Cell<bool> = const { Cell::new(false) };
    }
    if INSTALLED.with(Cell::get) {
        return;
    }
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(EXAMPLE_CSS);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    INSTALLED.with(|installed| installed.set(true));
}
