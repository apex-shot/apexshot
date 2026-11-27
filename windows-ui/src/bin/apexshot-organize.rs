#![cfg_attr(windows, windows_subsystem = "windows")]

//! The capture shell runs this right after saving a screenshot. It reads one
//! JSON `OrganizeRequest` on stdin, names, renames, and indexes the capture,
//! and prints the capture's final path on stdout. Any failure prints the
//! original path, so the shell always gets a usable path.

use std::io::Read;
use std::path::Path;

use apexshot_organizer::organize::{
    organize_capture_with_ai, parse_name_style, OrganizeRequest, OrganizeSettings,
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
    let settings = OrganizeSettings {
        auto_name: config.organizer_auto_name,
        style: parse_name_style(&config.organizer_name_style),
        excluded_apps: config.organizer_excluded_apps.clone(),
    };

    let namer = if apexshot_settings::ai_names::ai_allowed(&config) {
        let tier = apexshot_settings::ai_names::tier_from_setting(&config.organizer_ai_model);
        let deadline = started + std::time::Duration::from_millis(2600);
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
