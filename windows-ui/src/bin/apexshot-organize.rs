#![cfg_attr(windows, windows_subsystem = "windows")]

//! The capture shell runs this right after saving a screenshot. It reads one
//! JSON `OrganizeRequest` on stdin, names, renames, and indexes the capture,
//! and prints the capture's final path on stdout. Any failure prints the
//! original path, so the shell always gets a usable path.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use apexshot_organizer::organize::{
    organize_capture_with_ai, parse_name_style, OrganizeOutcome, OrganizeRequest, OrganizeSettings,
};

fn main() {
    let started = std::time::Instant::now();
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        std::process::exit(2);
    }
    let Ok(request) = serde_json::from_str::<OrganizeRequest>(&raw) else {
        std::process::exit(2);
    };

    let config = apexshot_settings::config::load_config();
    if request.allow_ai && !apexshot_settings::ai_names::ai_allowed(&config) {
        println!("{}", display_path(&request.path));
        return;
    }
    let settings = OrganizeSettings {
        auto_name: config.organizer_auto_name,
        style: parse_name_style(&config.organizer_name_style),
        excluded_apps: config.organizer_excluded_apps.clone(),
    };

    let namer = if request.allow_ai {
        let tier = apexshot_settings::ai_names::tier_from_setting(&config.organizer_ai_model);
        let deadline = started + std::time::Duration::from_millis(8000);
        apexshot_settings::ai_names::ensure_server(tier, deadline)
            .map(|base_url| apexshot_settings::ai_names::Namer { base_url, deadline })
    } else {
        None
    };

    let Some(index_path) = apexshot_settings::config::organizer_index_path() else {
        println!("{}", display_path(&request.path));
        return;
    };

    let outcome = organize_capture_with_ai(
        &request,
        &settings,
        &index_path,
        chrono::Local::now(),
        namer
            .as_ref()
            .map(|n| n as &dyn apexshot_organizer::ai::AiNamer),
    );
    if outcome.ai_named && !apexshot_settings::studio_entitlement::studio_unlocked() {
        apexshot_settings::ai_names::consume_trial();
    }
    let ai_card_shown = show_ai_names_card(&outcome, &config, &index_path);
    // The AI names card is the more useful one after a capture; the Black
    // Friday card only takes the shared promo slot when no AI card showed and
    // nothing else (the Cloud invitation) is already up. The card records the
    // day itself once it opens, so losing the slot costs nothing. Both are
    // their own windows, so neither delays the path.
    if !ai_card_shown
        && apexshot_settings::black_friday_card_due()
        && apexshot_settings::promo_slot::is_free()
    {
        spawn_black_friday_card();
    }
    println!("{}", display_path(&outcome.path));
}

/// The capture's path as the shell writes it into its own logs and records.
fn display_path(path: &Path) -> String {
    let display = path.to_string_lossy();
    if cfg!(windows) {
        display.replace('/', "\\")
    } else {
        display.into_owned()
    }
}

/// Show the one AI names card this capture earned, without delaying the path.
/// Returns whether a card was started.
fn show_ai_names_card(
    outcome: &OrganizeOutcome,
    config: &apexshot_settings::config::AppConfig,
    index_path: &Path,
) -> bool {
    use apexshot_settings::ai_names::{claim_card, trial_remaining, CardKind};

    // A window that advertises something already owns the screen. Leaving the
    // card's one-off claim unspent keeps it available for the next capture.
    let slot_free = apexshot_settings::promo_slot::is_free();
    let kind = if slot_free
        && outcome.ai_named
        && !apexshot_settings::studio_entitlement::studio_unlocked()
        && trial_remaining() == 0
        && claim_card(CardKind::TrialEnded)
    {
        Some(CardKind::TrialEnded)
    } else if slot_free
        && !config.organizer_ai_enabled
        && apexshot_settings::ai_names::recommend().tier.is_some()
        && outcome.renamed
        && index_has_auto_named(index_path, 20)
        && claim_card(CardKind::TryAiNames)
    {
        Some(CardKind::TryAiNames)
    } else {
        None
    };
    if let Some(kind) = kind {
        spawn_card(kind);
        return true;
    }
    false
}

/// True when the index holds at least `at_least` auto-named captures.
fn index_has_auto_named(path: &Path, at_least: usize) -> bool {
    let Ok(index) = apexshot_organizer::index::Index::load(path) else {
        return false;
    };
    index
        .entries()
        .iter()
        .filter(|entry| entry.auto_named)
        .count()
        >= at_least
}

/// Start the Settings card process; a failure leaves the path untouched.
fn spawn_card(kind: apexshot_settings::ai_names::CardKind) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let settings = exe.with_file_name("apexshot-settings.exe");
    let mut command = Command::new(settings);
    command.args(["--ai-names-card", kind.arg()]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let _ = command.spawn();
}

/// Start the Black Friday card in the Settings process; a failure leaves the
/// path untouched.
fn spawn_black_friday_card() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let settings = exe.with_file_name("apexshot-settings.exe");
    let mut command = Command::new(settings);
    command.arg("--black-friday-card");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let _ = command.spawn();
}
