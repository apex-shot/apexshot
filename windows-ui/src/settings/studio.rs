//! The Windows Studio page, in the same group-and-table layout as the other
//! Settings pages: where this device stands, the AI naming controls, what
//! Studio includes, then the license.
//!
//! The page rebuilds itself from the stored entitlement after an activation,
//! so every row always describes the same state.

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
    glib, Align, Box as GtkBox, Button, Entry, Label, Orientation, Overlay, ProgressBar, Revealer,
    Switch,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The example name rows need two tones the shared Settings sheet lacks.
const AI_NAMES_CSS: &str = r#"
.settings-table-row label.studio-example-free {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.62);
}

.settings-table-row label.studio-example-ai {
    font-size: 12px;
    font-weight: 700;
    color: #8fdcab;
}

.editor-root.editor-theme-light .settings-table-row label.studio-example-free {
    color: rgba(29, 33, 41, 0.60);
}

.editor-root.editor-theme-light .settings-table-row label.studio-example-ai {
    color: #17643e;
}
"#;

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
    install_css();
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_halign(Align::Fill);
    root.set_valign(Align::Start);
    root.set_hexpand(true);
    root.set_margin_top(super::PAGE_TOP_MARGIN);
    root.set_margin_bottom(8);
    render(&root, None);
    root
}

/// Replace the page's contents with the current license state. `flash` is a
/// one-off result shown under the status row, such as a fresh activation.
fn render(root: &GtkBox, flash: Option<String>) {
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }
    let status = current_status();

    root.append(&group_title(&t("Status")));
    let frame = table_frame();
    frame.append(&status_row(status, flash));
    root.append(&frame);

    if matches!(status, StudioStatus::ConfigurationUnavailable) {
        return;
    }

    root.append(&group_title(&t("AI names")));
    let frame = table_frame();
    ai_names_rows(&frame, root, status);
    root.append(&frame);

    root.append(&group_title(&t("What's included")));
    let frame = table_frame();
    included_rows(&frame, status);
    root.append(&frame);

    root.append(&group_title(&t("License")));
    let frame = table_frame();
    match status {
        StudioStatus::Active { .. } | StudioStatus::UpdateAccessEnded { .. } => {
            if crate::studio_entitlement::stored_token().is_some() {
                let row = table_row();
                row.append(&row_text(&t("License key"), None));
                row.append(&copy_button(
                    &t("Copy license"),
                    crate::studio_entitlement::stored_token,
                ));
                frame.append(&row);
            }
            let row = table_row();
            row.append(&row_text(&t("Replace license"), None));
            let revealer = Revealer::new();
            let replace = Button::with_label(&t("Replace…"));
            replace.set_valign(Align::Center);
            replace.connect_clicked({
                let revealer = revealer.clone();
                move |_| revealer.set_reveal_child(!revealer.reveals_child())
            });
            row.append(&replace);
            frame.append(&row);
            let form = key_form(root);
            form.add_css_class("settings-table-row");
            revealer.set_child(Some(&form));
            frame.append(&revealer);
        }
        _ => {
            let form = key_form(root);
            form.add_css_class("settings-table-row");
            frame.append(&form);
        }
    }
    frame.append(&device_row());
    root.append(&frame);
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

fn row_text(title: &str, hint: Option<&str>) -> GtkBox {
    let text = GtkBox::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.set_valign(Align::Center);
    let name = Label::new(Some(title));
    name.set_xalign(0.0);
    text.append(&name);
    if let Some(hint) = hint {
        let hint = Label::new(Some(hint));
        hint.add_css_class("settings-sub-option-hint");
        hint.set_wrap(true);
        hint.set_xalign(0.0);
        text.append(&hint);
    }
    text
}

/// "ApexShot Studio" with its state pill, one short sentence, and the one
/// action that fits the state.
fn status_row(status: StudioStatus, flash: Option<String>) -> Overlay {
    let row = table_row();
    let text = GtkBox::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.set_valign(Align::Center);

    let title_line = GtkBox::new(Orientation::Horizontal, 8);
    let name = Label::new(Some(&t("ApexShot Studio")));
    name.add_css_class("studio-hero-title");
    name.set_xalign(0.0);
    title_line.append(&name);
    let (pill_text, pill_class) = status_pill(status);
    let pill = Label::new(Some(&pill_text));
    pill.add_css_class("studio-pill");
    pill.add_css_class(pill_class);
    pill.set_valign(Align::Center);
    title_line.append(&pill);
    text.append(&title_line);

    let message = Label::new(Some(&short_message(status)));
    message.add_css_class("settings-sub-option-hint");
    message.add_css_class("studio-hero-message");
    message.set_wrap(true);
    message.set_xalign(0.0);
    text.append(&message);

    if let Some(flash) = flash {
        let note = Label::new(Some(&flash));
        note.add_css_class("studio-flash");
        note.set_wrap(true);
        note.set_xalign(0.0);
        text.append(&note);
    }
    row.append(&text);

    let action = Button::with_label(&action_label(status));
    action.set_valign(Align::Center);
    if matches!(status, StudioStatus::NotActivated) {
        action.add_css_class("settings-primary-btn");
        action.set_tooltip_text(Some(&format!(
            "{} · {} · {}",
            t("$39 once"),
            t("12 months of updates"),
            t("14-day refund")
        )));
    }
    action.connect_clicked(|_| {
        std::thread::spawn(|| {
            if let Err(error) =
                crate::utils::open::open_url(&crate::studio_entitlement::studio_page_url())
            {
                eprintln!("[studio] Could not open the Studio page: {error}");
            }
        });
    });
    row.append(&action);
    with_dither_field(row)
}

/// Put the dither field behind the status card without letting the field take
/// part in the row's size or intercept its clicks.
fn with_dither_field(row: GtkBox) -> Overlay {
    let field =
        crate::dither_field::dither_field(super::windowing::prefers_dark_glass_theme(), 0.0);
    // The overlay sizes the field to the row, so drop the standalone height
    // request that would otherwise enlarge the card.
    field.set_size_request(-1, -1);
    field.set_halign(Align::Fill);
    field.set_valign(Align::Fill);
    field.set_hexpand(true);
    field.set_vexpand(true);

    let overlay = Overlay::new();
    overlay.set_halign(Align::Fill);
    overlay.set_valign(Align::Fill);
    overlay.set_hexpand(true);
    overlay.set_child(Some(&field));
    overlay.add_overlay(&row);
    overlay.set_measure_overlay(&row, true);
    overlay
}

/// The AI naming controls: the switch, how names change, the model choice,
/// its install state, and the free-trial countdown.
fn ai_names_rows(frame: &GtkBox, root: &GtkBox, status: StudioStatus) {
    let config = load_config();
    let tier = tier_from_setting(&config.organizer_ai_model);
    let spec = model_spec(tier);

    // The switch and where names come from.
    let row = table_row();
    row.append(&row_text(
        &t("Name screenshots with AI"),
        Some(&t(
            "A small model on this PC reads the window title and on-screen text and writes a clearer name. Nothing is uploaded.",
        )),
    ));
    let switch = Switch::new();
    switch.set_active(config.organizer_ai_enabled);
    switch.set_sensitive(is_installed(tier));
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
        None => t("This PC is below the recommended specs, so names may be slow."),
    };
    row.append(&row_text(&t("Model"), Some(&hint)));
    frame.append(&row);

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
                        render(&root, None);
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
    );
    feature_row(
        frame,
        active,
        &t("Motion export"),
        &t("Animated titles, timing, and Motion video export in the editors."),
    );
    feature_row(
        frame,
        active,
        &t("Upscale"),
        &t("Sharpen small captures with offline neural upscaling."),
    );
    feature_row(
        frame,
        active,
        &t("Studio styles"),
        &t("Frame styles, scene shadows, and presentation transforms."),
    );
    feature_row(
        frame,
        active,
        &t("Advanced zoom camera"),
        &t("Instant zooms and fine camera control in the video editor."),
    );
}

fn feature_row(frame: &GtkBox, active: bool, title: &str, hint: &str) {
    let row = table_row();
    row.append(&row_text(title, Some(hint)));
    let pill_text = if active { t("Active") } else { t("Studio") };
    let pill = Label::new(Some(&pill_text));
    pill.add_css_class("studio-pill");
    pill.add_css_class(if active {
        "studio-pill-active"
    } else {
        "studio-pill-neutral"
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
                        render(&root, None);
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

/// The key field with its Activate button on one line, and the result of
/// the last attempt underneath.
fn key_form(root: &GtkBox) -> GtkBox {
    let form = GtkBox::new(Orientation::Vertical, 8);
    let line = GtkBox::new(Orientation::Horizontal, 12);
    let label = Label::new(Some(&t("License key")));
    label.set_valign(Align::Center);
    line.append(&label);
    let entry = Entry::new();
    entry.set_hexpand(true);
    entry.set_placeholder_text(Some(&t("Paste your license key")));
    line.append(&entry);
    let activate = Button::with_label(&t("Activate"));
    activate.add_css_class("settings-primary-btn");
    activate.set_valign(Align::Center);
    activate.set_sensitive(false);
    line.append(&activate);
    form.append(&line);

    let feedback = Label::new(None);
    feedback.add_css_class("studio-error");
    feedback.set_wrap(true);
    feedback.set_xalign(0.0);
    feedback.set_visible(false);
    form.append(&feedback);

    entry.connect_changed({
        let activate = activate.clone();
        let feedback = feedback.clone();
        move |entry| {
            activate.set_sensitive(!entry.text().trim().is_empty());
            feedback.set_visible(false);
        }
    });
    activate.connect_clicked({
        let entry = entry.clone();
        let root = root.downgrade();
        move |_| match activate_token(&entry.text()) {
            Ok(()) => {
                // The status card's Active badge already says it worked.
                let message = match current_status() {
                    StudioStatus::Active { .. } => None,
                    StudioStatus::UpdateAccessEnded { .. } => Some(t(
                        "Entitlement saved. This build isn't covered by the included updates; install a covered build to use Studio output.",
                    )),
                    other => Some(other.message()),
                };
                if let Some(root) = root.upgrade() {
                    // Rebuilding drops this form; let the click finish first.
                    glib::idle_add_local_once(move || render(&root, message));
                }
            }
            Err(error) => {
                feedback.set_text(&match error {
                    StudioEntitlementError::DeviceMismatch => {
                        StudioStatus::DeviceMismatch.message()
                    }
                    error => error.to_string(),
                });
                feedback.set_visible(true);
            }
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
    form
}

/// This machine's Device ID, which a buyer pastes when claiming a license.
fn device_row() -> GtkBox {
    let row = table_row();
    let device_id = crate::studio_entitlement::device_id();
    let value = device_id
        .clone()
        .unwrap_or_else(|| t("Unavailable on this system"));
    let text = row_text(&t("Device ID"), Some(&value));
    if let Some(hint) = text.last_child() {
        hint.add_css_class("studio-mono");
        if let Ok(hint) = hint.downcast::<Label>() {
            hint.set_selectable(true);
        }
    }
    row.append(&text);
    let copy = copy_button(&t("Copy"), crate::studio_entitlement::device_id);
    copy.set_sensitive(device_id.is_some());
    copy.set_tooltip_text(Some(&t(
        "Paste this at apexshot.org/studio to claim a Studio licence for this device.",
    )));
    row.append(&copy);
    row
}

/// A copy action that confirms on the button itself for two seconds.
fn copy_button(resting: &str, value: impl Fn() -> Option<String> + 'static) -> Button {
    let button = Button::with_label(resting);
    button.set_valign(Align::Center);
    let resting = resting.to_string();
    let pending = Rc::new(Cell::new(0_u32));
    button.connect_clicked(move |button| {
        let Some(text) = value() else {
            return;
        };
        if crate::utils::clipboard::copy_text_via_windows_clipboard(&text)
            .or_else(|_| crate::utils::clipboard::copy_text_to_clipboard(&text))
            .is_err()
        {
            return;
        }
        button.set_label(&t("Copied"));
        let generation = pending.get().wrapping_add(1);
        pending.set(generation);
        let button = button.downgrade();
        let resting = resting.clone();
        let pending = pending.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
            if pending.get() == generation {
                if let Some(button) = button.upgrade() {
                    button.set_label(&resting);
                }
            }
        });
    });
    button
}

fn status_pill(status: StudioStatus) -> (String, &'static str) {
    match status {
        StudioStatus::Active { .. } => (t("Active"), "studio-pill-active"),
        StudioStatus::NotActivated => (t("Free"), "studio-pill-neutral"),
        StudioStatus::UpdateAccessEnded { .. } => (t("Updates ended"), "studio-pill-warning"),
        StudioStatus::InvalidEntitlement => (t("Needs attention"), "studio-pill-attention"),
        StudioStatus::DeviceMismatch => (t("Other device"), "studio-pill-attention"),
        StudioStatus::ConfigurationUnavailable => (t("Unavailable"), "studio-pill-neutral"),
    }
}

fn action_label(status: StudioStatus) -> String {
    match status {
        StudioStatus::NotActivated => t("Upgrade to Studio"),
        StudioStatus::InvalidEntitlement => t("Recover license"),
        StudioStatus::DeviceMismatch => t("Claim for this device"),
        StudioStatus::UpdateAccessEnded { .. } | StudioStatus::Active { .. } => t("Manage license"),
        StudioStatus::ConfigurationUnavailable => t("Check Studio availability"),
    }
}

/// One short sentence per state; the long explanations stay in
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

fn install_css() {
    thread_local! {
        static INSTALLED: Cell<bool> = const { Cell::new(false) };
    }
    if INSTALLED.with(|installed| installed.replace(true)) {
        return;
    }
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = gtk4::CssProvider::new();
    let sheet = format!("{}\n{}", include_str!("studio.css"), AI_NAMES_CSS);
    provider.load_from_data(&crate::css_scale::scale_px_lengths(
        &sheet,
        crate::platform::ui_scale(),
    ));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 5,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_a_message_and_an_action() {
        for status in [
            StudioStatus::NotActivated,
            StudioStatus::Active { updates_until: 1 },
            StudioStatus::UpdateAccessEnded { updates_until: 1 },
            StudioStatus::InvalidEntitlement,
            StudioStatus::DeviceMismatch,
            StudioStatus::ConfigurationUnavailable,
        ] {
            assert!(!short_message(status).is_empty(), "{status:?}");
            assert!(!action_label(status).is_empty(), "{status:?}");
        }
        assert_eq!(
            action_label(StudioStatus::NotActivated),
            t("Upgrade to Studio")
        );
    }

    #[test]
    fn studio_styles_parse() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let provider = gtk4::CssProvider::new();
            let errors = Rc::new(Cell::new(0));
            provider.connect_parsing_error({
                let errors = errors.clone();
                move |_, _, error| {
                    eprintln!("{error}");
                    errors.set(errors.get() + 1);
                }
            });
            provider.load_from_data(include_str!("studio.css"));
            assert_eq!(errors.get(), 0);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
