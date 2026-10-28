//! The ApexShot Windows entry point: first-run onboarding, then Settings.

use apexshot_settings as app;

fn main() {
    app::platform::prepare_process();
    app::i18n::init_from_config();

    if needs_onboarding() {
        match app::onboarding::run() {
            Ok(outcome) => {
                if !outcome.finished || !outcome.open_settings {
                    return;
                }
            }
            Err(error) => {
                eprintln!("Failed to open onboarding: {error}");
                std::process::exit(1);
            }
        }
    }

    if let Err(error) = app::settings::show_settings_window() {
        eprintln!("Failed to open settings window: {error}");
        std::process::exit(1);
    }
}

/// A Windows install that has not finished onboarding opens the first-run flow
/// instead of Settings. `--onboarding` and the environment override re-open it
/// for a walk-through without touching the stored flag.
fn needs_onboarding() -> bool {
    let forced = std::env::args().any(|arg| arg == "--onboarding")
        || std::env::var("APEXSHOT_FORCE_ONBOARDING")
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);
    forced || !app::onboarding::is_complete()
}
