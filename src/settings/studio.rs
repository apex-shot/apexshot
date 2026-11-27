use crate::ai_names::{
    benchmark, install, is_installed, model_spec, recommend, remove, tier_from_setting,
    trial_remaining, InstallProgress, ModelTier, TRIAL_NAMES,
};
use crate::config::{load_config, save_config};
use crate::i18n::{t, tfmt};
use crate::studio_entitlement::{
    activate_token, current_status, StudioEntitlementError, StudioStatus,
};
use gtk4::prelude::*;
use gtk4::{
    glib, Align, Box as GtkBox, Button, Entry, Image, Label, Orientation, Overlay, ProgressBar,
    Revealer, RevealerTransitionType, Switch,
};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Extra classes the status pill can carry. Kept together so a status change
/// after activation swaps the tint instead of stacking one variant on another.
const STATUS_PILL_CLASSES: [&str; 5] = [
    "studio-status-pill-active",
    "studio-status-pill-neutral",
    "studio-status-pill-warning",
    "studio-status-pill-attention",
    "studio-status-pill-muted",
];

/// Messages from the install thread back to the row.
enum InstallMessage {
    Progress(InstallProgress),
    Finished(Result<(), String>),
}

thread_local! {
    /// Set by a finished install so the rebuilt row runs one speed test.
    static AUTO_TEST_AFTER_INSTALL: Cell<bool> = const { Cell::new(false) };
}

pub fn build_studio_section() -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_halign(Align::Fill);
    root.set_valign(Align::Start);
    root.set_hexpand(true);
    root.set_margin_top(super::PAGE_TOP_MARGIN);
    root.set_margin_bottom(8);
    render(&root);
    root
}

/// Replace the page's contents with the current license state and the AI
/// names controls that depend on it.
fn render(root: &GtkBox) {
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }
    let status_snapshot = current_status();

    let (hero, hero_parts) = build_hero(status_snapshot);
    root.append(&hero);

    if matches!(status_snapshot, StudioStatus::ConfigurationUnavailable) {
        return;
    }

    root.append(&group_title(&t("AI names")));
    let frame = table_frame();
    ai_names_rows(&frame, root, status_snapshot);
    root.append(&frame);

    root.append(&group_title(&t("What's included")));
    let frame = table_frame();
    included_rows(&frame, status_snapshot);
    root.append(&frame);

    root.append(&group_title(&t("License")));
    let frame = table_frame();
    frame.append(&build_device_id_row());

    match status_snapshot {
        StudioStatus::Active { .. } | StudioStatus::UpdateAccessEnded { .. } => {
            if crate::studio_entitlement::stored_token().is_some() {
                frame.append(&build_license_key_row());
            }

            let (replace_row, revealer) =
                build_replace_row(&hero_parts.message, &hero_parts.pill, &hero_parts.action);
            frame.append(&replace_row);
            frame.append(&revealer);
        }
        StudioStatus::NotActivated
        | StudioStatus::InvalidEntitlement
        | StudioStatus::DeviceMismatch => {
            frame.append(&build_activate_row(
                &hero_parts.message,
                &hero_parts.pill,
                &hero_parts.action,
            ));
        }
        StudioStatus::ConfigurationUnavailable => {}
    }

    root.append(&frame);
}

/// Widgets the activation form updates in place when the licence state changes.
struct HeroParts {
    message: Label,
    pill: Label,
    action: Button,
}

/// The card at the top of the page: the product name with its current
/// state, one sentence about that state, and the action that belongs with it.
fn build_hero(status: StudioStatus) -> (Overlay, HeroParts) {
    let hero = GtkBox::new(Orientation::Horizontal, 14);
    hero.add_css_class("studio-hero");
    hero.set_halign(Align::Fill);
    hero.set_valign(Align::Start);

    let text = GtkBox::new(Orientation::Vertical, 4);
    text.set_hexpand(true);
    text.set_valign(Align::Center);

    let title_row = GtkBox::new(Orientation::Horizontal, 8);
    title_row.set_valign(Align::Center);
    let title = Label::new(Some(&t("ApexShot Studio")));
    title.add_css_class("studio-hero-title");
    title.set_xalign(0.0);
    title_row.append(&title);

    let (pill_text, pill_class) = status_pill(status);
    let pill = Label::new(Some(&pill_text));
    pill.add_css_class("studio-status-pill");
    pill.add_css_class(pill_class);
    pill.set_valign(Align::Center);
    title_row.append(&pill);
    text.append(&title_row);

    let message = Label::new(Some(&short_message(status)));
    message.add_css_class("settings-sub-option");
    message.add_css_class("studio-hero-message");
    message.set_wrap(true);
    message.set_xalign(0.0);
    message.set_halign(Align::Fill);
    text.append(&message);

    if matches!(status, StudioStatus::NotActivated) {
        text.append(&build_offer_meta());
    }

    hero.append(&text);

    let action = Button::with_label(&studio_page_action_label(status));
    action.set_valign(Align::Center);
    action.set_halign(Align::End);
    if matches!(status, StudioStatus::NotActivated) {
        action.add_css_class("settings-primary-btn");
    }
    action.connect_clicked(|_| {
        let _ = crate::utils::open::open_url(&crate::studio_entitlement::studio_page_url());
    });
    hero.append(&action);

    (
        with_dither_field(hero),
        HeroParts {
            message,
            pill,
            action,
        },
    )
}

/// Put the dither field behind the hero without letting the field take part
/// in the hero's size or intercept its clicks.
fn with_dither_field(hero: GtkBox) -> Overlay {
    let field =
        crate::dither_field::dither_field(super::windowing::prefers_dark_glass_theme(), 0.0);
    // The overlay sizes the field to the hero, so drop the standalone height
    // request that would otherwise enlarge the card.
    field.set_size_request(-1, -1);
    field.set_halign(Align::Fill);
    field.set_valign(Align::Fill);
    field.set_hexpand(true);
    field.set_vexpand(true);

    let overlay = Overlay::new();
    overlay.set_halign(Align::Fill);
    overlay.set_valign(Align::Start);
    overlay.set_hexpand(true);
    overlay.set_margin_start(4);
    overlay.set_margin_end(4);
    overlay.set_child(Some(&field));
    overlay.add_overlay(&hero);
    overlay.set_measure_overlay(&hero, true);
    overlay
}

/// The purchase terms, quiet and inline: the long version that used to be the
/// button label now reads as three facts under the state sentence.
fn build_offer_meta() -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("studio-hero-meta");
    row.set_halign(Align::Start);
    row.set_valign(Align::Center);
    row.set_margin_top(4);

    let items = [
        crate::studio_offer::price_label(),
        t("12 months of updates"),
        t("14-day refund"),
    ];
    for (index, item_text) in items.iter().enumerate() {
        if index > 0 {
            let dot = GtkBox::new(Orientation::Horizontal, 0);
            dot.add_css_class("studio-hero-meta-dot");
            dot.set_valign(Align::Center);
            row.append(&dot);
        }
        let item = GtkBox::new(Orientation::Horizontal, 5);
        item.set_valign(Align::Center);
        let check = Image::from_icon_name(
            crate::capture::editor::window::icon_names::custom::CHECK_SYMBOLIC,
        );
        check.set_pixel_size(13);
        check.set_valign(Align::Center);
        item.append(&check);
        item.append(&Label::new(Some(item_text.as_str())));
        row.append(&item);
    }
    row
}

fn group_title(text: &str) -> Label {
    let title = Label::new(Some(text));
    title.add_css_class("settings-group-title");
    title.set_xalign(0.0);
    title.set_halign(Align::Start);
    title
}

fn table_frame() -> GtkBox {
    let frame = GtkBox::new(Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.set_margin_start(4);
    frame.set_margin_end(4);
    frame
}

fn table_row() -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.add_css_class("settings-table-row");
    row.set_hexpand(true);
    row
}

fn row_title(text: &str) -> Label {
    let title = Label::new(Some(text));
    title.add_css_class("studio-row-title");
    title.set_xalign(0.0);
    title.set_halign(Align::Start);
    title
}

/// A row's title with an optional sentence underneath, in the same shape as
/// the licence rows.
fn row_text(title: &str, hint: Option<&str>) -> GtkBox {
    row_text_with_badge(title, hint, false)
}

/// Like [`row_text`], with a small Beta tag after the title.
fn row_text_with_badge(title: &str, hint: Option<&str>, beta: bool) -> GtkBox {
    let text = GtkBox::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.set_valign(Align::Center);
    let title_line = GtkBox::new(Orientation::Horizontal, 8);
    title_line.set_valign(Align::Center);
    title_line.append(&row_title(title));
    if beta {
        title_line.append(&beta_pill());
    }
    text.append(&title_line);
    if let Some(hint) = hint {
        let hint = Label::new(Some(hint));
        hint.add_css_class("settings-sub-option-hint");
        hint.set_wrap(true);
        hint.set_xalign(0.0);
        text.append(&hint);
    }
    text
}

/// The Beta tag for features that are not finished yet, in the same pill
/// shape and amber tint the licence states use.
fn beta_pill() -> Label {
    let pill = Label::new(Some(&t("Beta")));
    pill.add_css_class("studio-status-pill");
    pill.add_css_class("studio-status-pill-warning");
    pill.set_valign(Align::Center);
    pill
}

/// The AI naming controls: the switch, how names change, the model choice,
/// its install state, and the free-trial countdown.
fn ai_names_rows(frame: &GtkBox, root: &GtkBox, status: StudioStatus) {
    let config = load_config();
    let tier = tier_from_setting(&config.organizer_ai_model);
    let spec = model_spec(tier);

    // The switch and where names come from. Once a free device has used its
    // trial, the switch shows off and locked; the saved choice stays, so an
    // upgrade picks AI names back up without another click.
    let locked = !matches!(status, StudioStatus::Active { .. }) && trial_remaining() == 0;
    // A PC that cannot run the model well gets no AI names at all: a weaker
    // model named screenshots worse than the free names do.
    let supported = recommend().tier.is_some();
    let row = table_row();
    let hint = if !supported {
        t("This PC is better off with the free names.")
    } else if locked {
        t("Your free AI names are used. Upgrade to Studio to keep them.")
    } else {
        t("A small model on this PC reads the window title and on-screen text and writes a clearer name. Nothing is uploaded.")
    };
    row.append(&row_text_with_badge(
        &t("Name screenshots with AI"),
        Some(&hint),
        true,
    ));
    let switch = Switch::new();
    switch.set_active(config.organizer_ai_enabled && !locked && supported);
    switch.set_sensitive(is_installed(tier) && !locked && supported);
    switch.set_valign(Align::Center);
    switch.connect_active_notify(|switch| {
        let mut config = load_config();
        config.organizer_ai_enabled = switch.is_active();
        if let Err(error) = save_config(&config) {
            eprintln!("[studio] Could not save the AI names setting: {error}");
        }
    });
    row.append(&switch);
    frame.append(&row);

    // How the free name and the AI name compare.
    let row = table_row();
    let examples = GtkBox::new(Orientation::Vertical, 6);
    examples.set_hexpand(true);
    examples.set_valign(Align::Center);
    let title = Label::new(Some(&t("How names change")));
    title.add_css_class("settings-sub-option-hint");
    title.set_xalign(0.0);
    examples.append(&title);
    let own = example_pairs();
    let from_own = !own.is_empty();
    let pairs = if from_own { own } else { sample_pairs() };
    let origin = if from_own {
        t("From your own screenshots")
    } else {
        t("Examples")
    };
    let source = Label::new(Some(&origin));
    source.add_css_class("settings-sub-option-hint");
    source.set_xalign(0.0);
    examples.append(&source);
    for (free, ai) in pairs {
        examples.append(&example_line(&free, &ai));
    }
    row.append(&examples);
    frame.append(&row);

    // The model, and whether this PC runs it fast enough.
    let row = table_row();
    let size = size_text(spec.bytes);
    let hint = match recommend().tier {
        Some(_) => t_args(
            "Recommended for this PC: {} ({})",
            &[spec.detail, size.as_str()],
        ),
        None => t("AI names need at least 8 GB of memory, 4 processor cores, and AVX2."),
    };
    row.append(&row_text(&t("Model"), Some(&hint)));
    frame.append(&row);
    if !supported && !is_installed(tier) {
        return;
    }

    // The chosen model: ready to use, or a one-time download.
    let row = table_row();
    if is_installed(tier) {
        let name = format!("{} · {}", t(spec.label), spec.detail);
        let text = row_text(
            &t("Ready"),
            Some(&t_args("{} model on this PC", &[name.as_str()])),
        );
        let result = Label::new(None);
        result.add_css_class("settings-sub-option-hint");
        result.set_xalign(0.0);
        result.set_wrap(true);
        result.set_visible(false);
        text.append(&result);
        row.append(&text);

        let test = Button::with_label(&t("Test speed"));
        test.set_valign(Align::Center);
        test.connect_clicked({
            let test = test.clone();
            let result = result.clone();
            move |_| {
                test.set_sensitive(false);
                test.set_label(&t("Testing…"));
                result.set_visible(false);
                let (tx, rx) = async_channel::unbounded::<Option<std::time::Duration>>();
                std::thread::spawn(move || {
                    let _ = tx.send_blocking(benchmark(tier));
                });
                let test = test.downgrade();
                let result = result.downgrade();
                glib::spawn_future_local(async move {
                    let Ok(sample) = rx.recv().await else {
                        return;
                    };
                    let (Some(test), Some(result)) = (test.upgrade(), result.upgrade()) else {
                        return;
                    };
                    test.set_label(&t("Test speed"));
                    test.set_sensitive(true);
                    let message = match sample {
                        Some(elapsed) => {
                            let seconds = elapsed.as_secs_f64();
                            let shown = format!("{seconds:.1}");
                            let mut message = t_args("Named a sample in {} s", &[shown.as_str()]);
                            if seconds > 2.5 {
                                message.push(' ');
                                message.push_str(&t(
                                    "That is slower than ApexShot waits for, so most names will stay rule-based.",
                                ));
                            }
                            message
                        }
                        None => t(
                            "The model did not answer. Try again, or remove and download it again.",
                        ),
                    };
                    result.set_text(&message);
                    result.set_visible(true);
                });
            }
        });
        if AUTO_TEST_AFTER_INSTALL.with(|pending| pending.replace(false)) {
            let test = test.clone();
            glib::idle_add_local_once(move || test.emit_clicked());
        }
        row.append(&test);

        let remove_button = Button::with_label(&t("Remove"));
        remove_button.set_valign(Align::Center);
        remove_button.connect_clicked({
            let root = root.downgrade();
            move |_| {
                let (tx, rx) = async_channel::unbounded::<Result<(), String>>();
                std::thread::spawn(move || {
                    let _ = tx.send_blocking(remove(tier));
                });
                glib::spawn_future_local({
                    let root = root.clone();
                    async move {
                        if let Ok(Err(error)) = rx.recv().await {
                            eprintln!("[studio] Could not remove the AI model: {error}");
                        }
                        let Some(root) = root.upgrade() else {
                            return;
                        };
                        let mut config = load_config();
                        config.organizer_ai_enabled = false;
                        if let Err(error) = save_config(&config) {
                            eprintln!("[studio] Could not save the AI names setting: {error}");
                        }
                        render(&root);
                    }
                });
            }
        });
        row.append(&remove_button);
    } else {
        let size = size_text(spec.bytes);
        let hint = t_args("{} · downloads once, then works offline", &[size.as_str()]);
        let text = row_text(&t("Download"), Some(&hint));
        let feedback = Label::new(None);
        feedback.add_css_class("settings-sub-option-hint");
        feedback.set_xalign(0.0);
        feedback.set_wrap(true);
        feedback.set_visible(false);
        text.append(&feedback);
        row.append(&text);
        row.append(&download_button(&row, &feedback, root, tier));
    }
    frame.append(&row);

    // The free trial count, until Studio is active on this device.
    if !matches!(status, StudioStatus::Active { .. }) {
        let row = table_row();
        let remaining = trial_remaining();
        let left = remaining.to_string();
        let total = TRIAL_NAMES.to_string();
        let hint = t_args(
            "{} of {} AI names left. After that, names stay rule-based until you upgrade.",
            &[left.as_str(), total.as_str()],
        );
        row.append(&row_text(&t("Free trial"), Some(&hint)));
        if remaining == 0 {
            let upgrade = Button::with_label(&t("Upgrade to Studio"));
            upgrade.add_css_class("settings-primary-btn");
            upgrade.set_valign(Align::Center);
            upgrade.connect_clicked(|_| {
                std::thread::spawn(|| {
                    if let Err(error) =
                        crate::utils::open::open_url(&crate::studio_entitlement::studio_page_url())
                    {
                        eprintln!("[studio] Could not open the Studio page: {error}");
                    }
                });
            });
            row.append(&upgrade);
        }
        frame.append(&row);
    }
}

/// Studio's features, each with a pill for whether it is on this device.
fn included_rows(frame: &GtkBox, status: StudioStatus) {
    let active = matches!(status, StudioStatus::Active { .. });
    feature_row(
        frame,
        active,
        &t("AI names"),
        &t("Clearer screenshot names from a small model on this PC."),
        true,
    );
    feature_row(
        frame,
        active,
        &t("Motion export"),
        &t("Animated titles, timing, and Motion video export in the editors."),
        false,
    );
    feature_row(
        frame,
        active,
        &t("Upscale"),
        &t("Sharpen small captures with offline neural upscaling."),
        false,
    );
    feature_row(
        frame,
        active,
        &t("Studio styles"),
        &t("Frame styles, scene shadows, and presentation transforms."),
        false,
    );
    feature_row(
        frame,
        active,
        &t("Advanced zoom camera"),
        &t("Instant zooms and fine camera control in the video editor."),
        false,
    );
}

fn feature_row(frame: &GtkBox, active: bool, title: &str, hint: &str, beta: bool) {
    let row = table_row();
    row.append(&row_text_with_badge(title, Some(hint), beta));
    let pill_text = if active { t("Active") } else { t("Studio") };
    let pill = Label::new(Some(&pill_text));
    pill.add_css_class("studio-status-pill");
    pill.add_css_class(if active {
        "studio-status-pill-active"
    } else {
        "studio-status-pill-neutral"
    });
    pill.set_valign(Align::Center);
    row.append(&pill);
    frame.append(&row);
}

/// One free name, an arrow, and the AI name that replaces it.
fn example_line(free: &str, ai: &str) -> GtkBox {
    let line = GtkBox::new(Orientation::Horizontal, 8);
    let before = Label::new(Some(free));
    before.add_css_class("studio-example-free");
    before.set_xalign(0.0);
    line.append(&before);
    let arrow = Label::new(Some("→"));
    arrow.add_css_class("settings-sub-option-hint");
    line.append(&arrow);
    let after = Label::new(Some(ai));
    after.add_css_class("studio-example-ai");
    after.set_xalign(0.0);
    after.set_wrap(true);
    line.append(&after);
    line
}

/// The same examples the AI names card picks: real changes, biggest first.
fn example_pairs() -> Vec<(String, String)> {
    crate::ai_names::card::example_pairs(3)
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

/// A Download button that swaps itself for the install row's progress state.
fn download_button(row: &GtkBox, feedback: &Label, root: &GtkBox, tier: ModelTier) -> Button {
    let button = Button::with_label(&t("Download"));
    button.add_css_class("settings-primary-btn");
    button.set_valign(Align::Center);
    button.connect_clicked({
        let row = row.clone();
        let feedback = feedback.clone();
        let root = root.downgrade();
        move |button| start_install(&row, &feedback, &root, tier, button)
    });
    button
}

/// Run the blocking install on a thread and drive the row's progress from it.
fn start_install(
    row: &GtkBox,
    feedback: &Label,
    root: &glib::WeakRef<GtkBox>,
    tier: ModelTier,
    button: &Button,
) {
    row.remove(button);
    feedback.set_visible(false);
    let progress = ProgressBar::new();
    progress.set_show_text(true);
    progress.set_hexpand(true);
    progress.set_valign(Align::Center);
    progress.set_fraction(0.0);
    progress.add_css_class("studio-download-progress");
    row.append(&progress);
    let cancel_button = Button::with_label(&t("Cancel"));
    cancel_button.set_valign(Align::Center);
    row.append(&cancel_button);

    let cancel = Arc::new(AtomicBool::new(false));
    cancel_button.connect_clicked({
        let cancel = cancel.clone();
        move |_| cancel.store(true, Ordering::Relaxed)
    });

    let (tx, rx) = async_channel::unbounded::<InstallMessage>();
    std::thread::spawn(move || {
        let progress_tx = tx.clone();
        let report = move |event: InstallProgress| {
            let _ = progress_tx.send_blocking(InstallMessage::Progress(event));
        };
        let outcome = install(tier, &report, cancel.as_ref());
        let _ = tx.send_blocking(InstallMessage::Finished(outcome));
    });

    let row = row.clone();
    let feedback = feedback.clone();
    let root = root.clone();
    glib::spawn_future_local(async move {
        while let Ok(message) = rx.recv().await {
            match message {
                InstallMessage::Progress(event) => match event {
                    InstallProgress::Downloading { done, total } => {
                        let fraction = if total == 0 {
                            0.0
                        } else {
                            done as f64 / total as f64
                        };
                        progress.set_fraction(fraction);
                        progress.set_text(Some(&format!("{} / {}", mb_text(done), mb_text(total))));
                    }
                    InstallProgress::Verifying => {
                        progress.set_text(Some(&t("Checking the download…")));
                    }
                    InstallProgress::Extracting => {
                        progress.set_text(Some(&t("Unpacking…")));
                    }
                    InstallProgress::Done => progress.set_fraction(1.0),
                },
                InstallMessage::Finished(Ok(())) => {
                    let mut config = load_config();
                    config.organizer_ai_enabled = true;
                    if let Err(error) = save_config(&config) {
                        eprintln!("[studio] Could not save the AI names setting: {error}");
                    }
                    AUTO_TEST_AFTER_INSTALL.with(|pending| pending.set(true));
                    if let Some(root) = root.upgrade() {
                        render(&root);
                    }
                    break;
                }
                InstallMessage::Finished(Err(error)) => {
                    let message = if error == "cancelled" {
                        t("Download cancelled")
                    } else {
                        error
                    };
                    feedback.set_text(&message);
                    feedback.set_visible(true);
                    row.remove(&progress);
                    row.remove(&cancel_button);
                    if let Some(root) = root.upgrade() {
                        row.append(&download_button(&row, &feedback, &root, tier));
                    }
                    break;
                }
            }
        }
    });
}

/// `t` with positional `{}` placeholders filled left to right.
fn t_args(msgid: &str, values: &[&str]) -> String {
    let mut text = t(msgid);
    for value in values {
        text = text.replacen("{}", value, 1);
    }
    text
}

/// A model's download size, rounded to MB, or GB once it is large enough.
fn size_text(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else {
        mb_text(bytes)
    }
}

fn mb_text(bytes: u64) -> String {
    format!("{} MB", (bytes as f64 / 1_000_000.0).round() as u64)
}

/// A copy action that reports success on the button itself for two seconds.
fn build_copy_button(resting: &str, value: impl Fn() -> Option<String> + 'static) -> Button {
    let button = Button::with_label(resting);
    button.set_valign(Align::Center);
    let resting = resting.to_string();
    button.connect_clicked(move |button| {
        let Some(text) = value() else {
            return;
        };
        if crate::utils::clipboard::copy_text_to_clipboard(&text).is_ok() {
            button.set_label(&t("Copied"));
            let button = button.clone();
            let resting = resting.clone();
            gtk4::glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                button.set_label(&resting);
            });
        }
    });
    button
}

struct ActivationControls {
    token: Entry,
    activate: Button,
    feedback: Label,
}

fn build_activation_controls(
    status: &Label,
    pill: &Label,
    studio_page: &Button,
    on_activated: impl Fn() + 'static,
) -> ActivationControls {
    let token = Entry::new();
    token.add_css_class("studio-token-entry");
    token.set_hexpand(true);
    token.set_placeholder_text(Some(&t("Paste your license key")));

    let feedback = Label::new(Some(""));
    feedback.add_css_class("studio-feedback");
    feedback.set_wrap(true);
    feedback.set_xalign(0.0);
    // The result line only appears once a submit reports something, so the form
    // does not reserve a blank line under the buttons.
    feedback.set_visible(false);

    let activate = Button::with_label(&t("Activate"));
    activate.add_css_class("settings-primary-btn");
    activate.set_valign(Align::Center);

    activate.connect_clicked({
        let token = token.clone();
        let status = status.clone();
        let pill = pill.clone();
        let feedback = feedback.clone();
        let studio_page = studio_page.clone();
        move |_| match activate_token(&token.text()) {
            Ok(()) => {
                token.set_text("");
                let status_snapshot = current_status();
                status.set_text(&short_message(status_snapshot));
                refresh_status_pill(&pill, status_snapshot);
                apply_action_label(&studio_page, status_snapshot);
                // The status pill already says Active; only explain the
                // outcomes it cannot.
                let feedback_message = match status_snapshot {
                    StudioStatus::Active { .. } => None,
                    StudioStatus::UpdateAccessEnded { .. } => Some(t(
                        "Entitlement saved. This build isn't covered by the included updates; install a covered build to use Studio output.",
                    )),
                    other => Some(other.message()),
                };
                feedback.set_text(feedback_message.as_deref().unwrap_or_default());
                feedback.set_visible(feedback_message.is_some());
                on_activated();
            }
            Err(error) => {
                status.set_text(&short_message(current_status()));
                feedback.set_text(&match error {
                    StudioEntitlementError::DeviceMismatch
                    | StudioEntitlementError::UnboundLicenceRetired => {
                        StudioStatus::DeviceMismatch.message()
                    }
                    error => error.to_string(),
                });
                feedback.set_visible(true);
            }
        }
    });

    token.connect_activate({
        let activate = activate.clone();
        move |_| activate.emit_clicked()
    });

    ActivationControls {
        token,
        activate,
        feedback,
    }
}

/// The value a buyer pastes on apexshot.org/studio when claiming a licence for
/// this machine. Shown even before activation, because it is needed first.
fn build_device_id_row() -> GtkBox {
    let row = table_row();

    let text = GtkBox::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.set_valign(Align::Center);
    text.append(&row_title(&t("Device ID")));

    let device_id = crate::studio_entitlement::device_id();
    let value = Label::new(Some(
        &device_id
            .clone()
            .unwrap_or_else(|| t("Unavailable on this system")),
    ));
    value.add_css_class("settings-mono-value");
    value.add_css_class("settings-sub-option");
    value.set_selectable(true);
    value.set_xalign(0.0);
    value.set_halign(Align::Start);
    text.append(&value);

    let copy = build_copy_button(&t("Copy"), crate::studio_entitlement::device_id);
    copy.set_sensitive(device_id.is_some());

    row.append(&text);
    row.append(&copy);
    row
}

/// The saved key itself, offered so it can be carried to another machine.
fn build_license_key_row() -> GtkBox {
    let row = table_row();

    let text = GtkBox::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.set_valign(Align::Center);
    text.append(&row_title(&t("License key")));

    let copy = build_copy_button(&t("Copy license"), crate::studio_entitlement::stored_token);

    row.append(&text);
    row.append(&copy);
    row
}

/// A row whose control opens the activation form underneath the row, so an
/// activated installation still reads as a card rather than a form.
fn build_replace_row(status: &Label, pill: &Label, studio_page: &Button) -> (GtkBox, Revealer) {
    let row = table_row();

    let text = GtkBox::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.set_valign(Align::Center);
    text.append(&row_title(&t("Replace license")));

    let revealer = Revealer::new();
    revealer.set_transition_type(RevealerTransitionType::SlideDown);
    revealer.set_transition_duration(200);
    revealer.set_reveal_child(false);

    let replace = Button::with_label(&t("Replace…"));
    replace.set_valign(Align::Center);
    replace.connect_clicked({
        let revealer = revealer.clone();
        move |_| revealer.set_reveal_child(!revealer.reveals_child())
    });

    row.append(&text);
    row.append(&replace);

    let controls = build_activation_controls(status, pill, studio_page, {
        let revealer = revealer.clone();
        move || revealer.set_reveal_child(false)
    });

    let body = GtkBox::new(Orientation::Vertical, 8);
    body.add_css_class("studio-reveal-body");
    body.append(&controls.token);
    controls.activate.set_halign(Align::End);
    body.append(&controls.activate);
    body.append(&controls.feedback);
    revealer.set_child(Some(&body));

    (row, revealer)
}

/// The form for an installation that has no working key yet. It stays open:
/// there is nothing else useful on the page to do first.
fn build_activate_row(status: &Label, pill: &Label, studio_page: &Button) -> GtkBox {
    let row = GtkBox::new(Orientation::Vertical, 8);
    row.add_css_class("settings-table-row");
    row.add_css_class("studio-activate-row");
    row.append(&row_title(&t("Activate a license")));

    let controls = build_activation_controls(status, pill, studio_page, || {});
    row.append(&controls.token);
    controls.activate.set_halign(Align::End);
    row.append(&controls.activate);
    row.append(&controls.feedback);
    row
}

/// The label and tint the state pill wears for each licence state.
fn status_pill(status: StudioStatus) -> (String, &'static str) {
    match status {
        StudioStatus::Active { .. } => (t("Active"), "studio-status-pill-active"),
        StudioStatus::NotActivated => (t("Free"), "studio-status-pill-neutral"),
        StudioStatus::UpdateAccessEnded { .. } => {
            (t("Updates ended"), "studio-status-pill-warning")
        }
        StudioStatus::InvalidEntitlement => (t("Needs attention"), "studio-status-pill-attention"),
        StudioStatus::DeviceMismatch => (t("Other device"), "studio-status-pill-attention"),
        StudioStatus::ConfigurationUnavailable => (t("Unavailable"), "studio-status-pill-muted"),
    }
}

fn refresh_status_pill(pill: &Label, status: StudioStatus) {
    for class in STATUS_PILL_CLASSES {
        pill.remove_css_class(class);
    }
    let (label, class) = status_pill(status);
    pill.set_text(&label);
    pill.add_css_class(class);
}

fn apply_action_label(button: &Button, status: StudioStatus) {
    button.set_label(&studio_page_action_label(status));
    if matches!(status, StudioStatus::NotActivated) {
        button.add_css_class("settings-primary-btn");
    } else {
        button.remove_css_class("settings-primary-btn");
    }
}

/// One line for the summary card. The full explanations stay in
/// `StudioStatus::message` for the places that have room for them.
fn short_message(status: StudioStatus) -> String {
    let date = |timestamp: i64| {
        chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0)
            .map(|date| date.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| timestamp.to_string())
    };
    match status {
        StudioStatus::Active { updates_until } => tfmt(
            "Updates included through {date}.",
            &[("date", &date(updates_until))],
        ),
        StudioStatus::UpdateAccessEnded { updates_until } => tfmt(
            "Included updates ended {date}.",
            &[("date", &date(updates_until))],
        ),
        StudioStatus::NotActivated => t("Not activated on this device."),
        StudioStatus::InvalidEntitlement => t("The saved license couldn't be verified."),
        StudioStatus::DeviceMismatch => t("This license belongs to another device."),
        StudioStatus::ConfigurationUnavailable => t("This build can't verify Studio licenses."),
    }
}

fn studio_page_action_label(status: StudioStatus) -> String {
    match status {
        StudioStatus::NotActivated => t("Upgrade to Studio"),
        StudioStatus::InvalidEntitlement => t("Recover license"),
        StudioStatus::DeviceMismatch => t("Claim for this device"),
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
            t("Upgrade to Studio")
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

    #[test]
    fn status_pills_name_each_license_state() {
        assert_eq!(
            status_pill(StudioStatus::Active { updates_until: 1 }),
            (t("Active"), "studio-status-pill-active")
        );
        assert_eq!(
            status_pill(StudioStatus::NotActivated),
            (t("Free"), "studio-status-pill-neutral")
        );
        assert_eq!(
            status_pill(StudioStatus::UpdateAccessEnded { updates_until: 1 }),
            (t("Updates ended"), "studio-status-pill-warning")
        );
        assert_eq!(
            status_pill(StudioStatus::InvalidEntitlement),
            (t("Needs attention"), "studio-status-pill-attention")
        );
        assert_eq!(
            status_pill(StudioStatus::DeviceMismatch),
            (t("Other device"), "studio-status-pill-attention")
        );
        assert_eq!(
            status_pill(StudioStatus::ConfigurationUnavailable),
            (t("Unavailable"), "studio-status-pill-muted")
        );
    }
}
