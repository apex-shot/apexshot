//! The ApexShot Windows entry point: first-run onboarding, then the tray shell.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use apexshot_settings as app;

fn main() {
    app::platform::prepare_process();
    app::i18n::init_from_config();

    if needs_onboarding() {
        match app::onboarding::run() {
            Ok(outcome) => {
                if !outcome.finished {
                    return;
                }
            }
            Err(error) => {
                eprintln!("Failed to open onboarding: {error}");
                std::process::exit(1);
            }
        }

        // Setup is done. The tray shell is the running app: it owns the tray
        // icon and the capture hotkeys, and it opens the quick access menu, so
        // the new install lands where the shortcuts work instead of in a
        // settings window the user did not ask for.
        if let Err(error) = app::settings::launch_shell(true) {
            eprintln!("Failed to start the ApexShot shell: {error}");
            std::process::exit(1);
        }
        return;
    }

    // Settings is a window, not the app: opening it from the Start menu also
    // makes sure the shell that owns the tray icon and the capture hotkeys is
    // running, so the shortcuts keep working after a reboot.
    if let Err(error) = app::settings::launch_shell(false) {
        eprintln!("Failed to start the ApexShot shell: {error}");
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
