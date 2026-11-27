#![cfg_attr(windows, windows_subsystem = "windows")]

//! The capture shell runs this right after saving a screenshot. It reads one
//! JSON `OrganizeRequest` on stdin, names, renames, and indexes the capture,
//! and prints the capture's final path on stdout. Any failure prints the
//! original path, so the shell always gets a usable path. A `clean_text`
//! request instead rebuilds and cleans copied OCR text and prints JSON.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use apexshot_organizer::cleanup::{accept_correction, ai_eligible, CLEANUP_PROMPT};
use apexshot_organizer::organize::{
    organize_capture_with_ai, parse_name_style, OrganizeOutcome, OrganizeRequest, OrganizeSettings,
};
use apexshot_organizer::text_layout::{rebuild, LayoutText, TextKind};
use apexshot_organizer::OcrPage;

fn main() {
    let started = std::time::Instant::now();
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        std::process::exit(2);
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
        if value.get("task").and_then(|task| task.as_str()) == Some("clean_text") {
            clean_text(value);
            return;
        }
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
    show_ai_names_card(&outcome, &config, &index_path);
    println!("{}", display_path(&outcome.path));
}

/// Rebuild copied OCR text, optionally fixing reading mistakes with the model.
fn clean_text(value: serde_json::Value) {
    let started = std::time::Instant::now();
    let Some(page) = value
        .get("page")
        .cloned()
        .and_then(|page| serde_json::from_value::<OcrPage>(page).ok())
    else {
        println!("{{}}");
        return;
    };
    let allow_ai = value
        .get("allow_ai")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let config = apexshot_settings::config::load_config();
    let mut layout = rebuild(&page);
    if !config.ocr_keep_layout {
        layout = LayoutText {
            kind: TextKind::Prose,
            text: page
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            html: None,
        };
    }
    let mut ai_used = false;
    if allow_ai
        && config.ocr_ai_cleanup
        && apexshot_settings::ai_names::ai_allowed(&config)
        && ai_eligible(&layout)
    {
        let deadline = started + std::time::Duration::from_secs(12);
        let tier = apexshot_settings::ai_names::tier_from_setting(&config.organizer_ai_model);
        if let Some(base) = apexshot_settings::ai_names::ensure_server(tier, deadline) {
            let words = layout.text.split_whitespace().count() as u32;
            let max_tokens = (words * 2 + 32).min(512);
            let reply = apexshot_settings::ai_names::complete_long(
                &base,
                CLEANUP_PROMPT,
                &layout.text,
                max_tokens,
                deadline,
            );
            if let Some(reply) = reply {
                if let Some(text) = accept_correction(&layout.text, &reply) {
                    if text != layout.text {
                        layout.text = text;
                        ai_used = true;
                        if !apexshot_settings::studio_entitlement::studio_unlocked() {
                            apexshot_settings::ai_names::consume_trial();
                        }
                    }
                }
            }
        }
    }
    let kind = match layout.kind {
        TextKind::Prose => "prose",
        TextKind::List => "list",
        TextKind::Code => "code",
        TextKind::Table => "table",
    };
    println!(
        "{}",
        serde_json::json!({
            "text": layout.text,
            "html": layout.html,
            "kind": kind,
            "ai_used": ai_used,
        })
    );
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
fn show_ai_names_card(
    outcome: &OrganizeOutcome,
    config: &apexshot_settings::config::AppConfig,
    index_path: &Path,
) {
    use apexshot_settings::ai_names::{claim_card, trial_remaining, CardKind};

    let kind = if outcome.ai_named
        && !apexshot_settings::studio_entitlement::studio_unlocked()
        && trial_remaining() == 0
        && claim_card(CardKind::TrialEnded)
    {
        Some(CardKind::TrialEnded)
    } else if !config.organizer_ai_enabled
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
    }
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
