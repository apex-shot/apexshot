//! The ApexShot Windows entry point: first-run onboarding, then the tray shell.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use apexshot_settings as app;

fn main() {
    app::platform::prepare_process();
    app::i18n::init_from_config();

    // The tray shell starts this once per run. The shared decision keeps a
    // fresh install quiet and shows an upgrade's highlights once.
    if std::env::args().nth(1).as_deref() == Some("--whats-new") {
        // `--force` comes from the tray's menu item and always opens the
        // window; without it this is the once-per-upgrade startup check.
        let result = if std::env::args().any(|argument| argument == "--force") {
            app::whats_new_window::show_latest()
        } else {
            app::whats_new_window::show_on_startup()
        };
        match result {
            Ok(()) => std::process::exit(0),
            Err(error) => {
                eprintln!("Failed to open the What's new window: {error}");
                std::process::exit(1);
            }
        }
    }

    if std::env::args().nth(1).as_deref() == Some("--ai-names-card") {
        let kind = std::env::args()
            .nth(2)
            .and_then(|value| app::ai_names::CardKind::from_arg(&value));
        std::process::exit(match kind {
            Some(kind) => app::ai_names::run_card(kind),
            None => 2,
        });
    }

    if std::env::args().nth(1).as_deref() == Some("--black-friday-card") {
        std::process::exit(app::black_friday::run_card());
    }

    // The website opens `apexshot://` links: the Studio page hands a purchased
    // or recovered licence back, and the Cloud page asks the app to connect.
    // A link this build does not know is a normal launch, never a command.
    if let Some(url) = std::env::args()
        .nth(1)
        .filter(|url| app::app_links::is_app_link(url.as_str()))
    {
        match app::app_links::classify_link(&url) {
            app::app_links::AppLink::CloudConnect => {
                // Count it before a handoff to the running instance can end
                // this process.
                app::usage_telemetry::record_connect_link_opened();
                if needs_onboarding() {
                    match app::onboarding::run_for_cloud_connect() {
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
                    finish_onboarding();
                    return;
                }
                if let Err(error) = app::settings::launch_shell(false) {
                    eprintln!("Failed to start the ApexShot shell: {error}");
                }
                if let Err(error) = app::settings::show_settings_window_for_cloud_connect() {
                    eprintln!("Failed to open settings window: {error}");
                    std::process::exit(1);
                }
                return;
            }
            app::app_links::AppLink::Studio => {
                match app::studio_entitlement::activate_from_url(&url) {
                    Ok(()) => {
                        app::utils::notify::desktop_notification_and_wait(
                            &app::i18n::t("Studio is active on this device"),
                            "",
                            false,
                        );
                        std::process::exit(0);
                    }
                    Err(error) => {
                        eprintln!("{error}");
                        app::utils::notify::desktop_notification_and_wait(
                            &app::i18n::t("Studio activation failed"),
                            &error.to_string(),
                            true,
                        );
                        std::process::exit(1);
                    }
                }
            }
            app::app_links::AppLink::Unknown => {}
        }
    }

    // A Cloud limit is another argument mode of this binary: the upload helper
    // cannot draw the card itself, so it starts this one and leaves.
    if let Some((limit, almost)) =
        app::cloud::helper::cloud_limit_from_args(std::env::args_os().skip(1))
    {
        match app::cloud::upgrade::show_standalone(limit, almost) {
            Ok(()) => std::process::exit(0),
            Err(error) => {
                eprintln!("Failed to open the Cloud limit card: {error}");
                std::process::exit(1);
            }
        }
    }

    // The capture shell has no Cloud transport of its own; it runs this
    // binary in helper mode to upload a finished capture, or to draw the Quick
    // Access Cloud nudge card for a copied one.
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

        finish_onboarding();
        return;
    }

    // Settings is a window, not the app: opening it from the Start menu also
    // makes sure the shell that owns the tray icon and the capture hotkeys is
    // running, so the shortcuts keep working after a reboot.
    if let Err(error) = app::settings::launch_shell(false) {
        eprintln!("Failed to start the ApexShot shell: {error}");
    }

    let settings = if std::env::args().any(|argument| argument == "--studio") {
        app::settings::show_settings_window_for_studio()
    } else {
        app::settings::show_settings_window()
    };
    if let Err(error) = settings {
        eprintln!("Failed to open settings window: {error}");
        std::process::exit(1);
    }
}

/// The steps that follow a finished setup, whether it came from the normal
/// first run or from the website's Cloud connect link.
fn finish_onboarding() {
    // The shell starts after setup on this platform, so the first shell
    // run would otherwise treat a fresh install as an upgrade and show the
    // release highlights. Recording the version keeps a new install quiet;
    // an upgrade never runs this path.
    app::whats_new::write_last_seen(env!("CARGO_PKG_VERSION"));

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
