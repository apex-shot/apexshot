//! The ApexShot Windows entry point: first-run onboarding, then the tray shell.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use apexshot_settings as app;

fn main() {
    app::platform::prepare_process();
    app::i18n::init_from_config();

    // The Studio page hands a purchased or recovered licence back through an
    // `apexshot://` link. Activate it and leave without opening Settings.
    if let Some(url) = std::env::args()
        .nth(1)
        .filter(|url| url.starts_with("apexshot://"))
    {
        match app::studio_entitlement::activate_from_url(&url) {
            Ok(()) => {
                app::utils::notify::desktop_notification_and_wait(
                    "Studio is active on this device",
                    "",
                    false,
                );
                std::process::exit(0);
            }
            Err(error) => {
                eprintln!("{error}");
                app::utils::notify::desktop_notification_and_wait(
                    "Studio activation failed",
                    &error.to_string(),
                    true,
                );
                std::process::exit(1);
            }
        }
    }

    // The capture shell has no Cloud transport of its own; it runs this
    // binary in helper mode to upload a finished capture.
    if let Some(command) = app::cloud::helper::command_from_args(std::env::args_os().skip(1)) {
        std::process::exit(app::cloud::helper::run(command));
    }

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

        // Publish the finished setup to the C++ shell before it starts, so
        // auto-upload and the rest of the mirror are current from the first
        // capture.
        if let Err(error) = app::settings::sync_shell_settings() {
            eprintln!("Failed to write the shell settings: {error}");
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
