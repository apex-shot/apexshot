//! ApexShot CLI - Screenshot tool for Linux
//!
//! Usage:
//!   cargo run -- capture screen
//!   cargo run -- capture area
//!   cargo run -- capture window
//!   cargo run -- record screen
//!   cargo run -- record area
//!   cargo run -- ocr <image>
#![allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::items_after_test_module,
    clippy::arc_with_non_send_sync
)]

mod cli;

use apexshot::{
    app_identity,
    capture::{open_image_editor, open_image_editor_empty, show_capture_preview_overlay},
    capture_overlay::run_capture_overlay,
    daemon::trigger_daemon_action,
    hotkeys::{
        install_hotkeys_for_current_desktop, reset_hotkey_config,
        setup_hotkeys_for_current_desktop, uninstall_hotkeys_for_current_desktop,
    },
    onboarding::{is_onboarding_complete, show_onboarding_window},
    recording::{
        editor::{open_empty_recording_editor, open_recording_editor},
        run_overlay_recording_request, run_recording_countdown_bar,
    },
    settings::show_settings_window,
};
use std::path::PathBuf;

fn main() {
    let _ = dotenvy::dotenv();
    if let Some(mut config_dir) = dirs::config_dir() {
        config_dir.push("apexshot");
        config_dir.push(".env");
        let _ = dotenvy::from_path(&config_dir);
    }

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        // No arguments - open settings or onboarding, and ensure the tray/hotkey
        // daemon is up so first-run is not a dead UI with no capture path.
        std::thread::spawn(|| {
            let _ = apexshot::daemon::ensure_daemon_running();
        });
        if !is_onboarding_complete() {
            let _ = show_onboarding_window();
            return;
        }
        let _ = show_settings_window();
        return;
    }

    // Handle GTK-only commands BEFORE entering tokio runtime
    // These commands run their own GTK main loop and don't need tokio
    match args[1].as_str() {
        "edit-internal" => {
            if args.len() < 3 {
                eprintln!("Error: missing image path");
                std::process::exit(1);
            }
            let image_path = PathBuf::from(&args[2]);
            if let Err(e) = open_image_editor(image_path) {
                eprintln!("Editor failed: {e}");
                std::process::exit(1);
            }
            return;
        }
        "settings-internal" => {
            if let Err(e) = show_settings_window() {
                eprintln!("Failed to open settings window: {e}");
                std::process::exit(1);
            }
            return;
        }
        "history-internal" => {
            if let Err(e) = apexshot::history::show_history_window() {
                eprintln!("Failed to open history window: {e}");
                std::process::exit(1);
            }
            return;
        }
        "video-editor" => {
            let result = if args.len() < 3 {
                open_empty_recording_editor()
            } else {
                open_recording_editor(PathBuf::from(&args[2]))
            };
            if let Err(e) = result {
                eprintln!("Recording editor failed: {e}");
                std::process::exit(1);
            }
            return;
        }
        "image-editor" => {
            if let Err(e) = open_image_editor_empty() {
                eprintln!("Image editor failed: {e}");
                std::process::exit(1);
            }
            return;
        }
        "preview" => {
            if args.len() < 3 {
                eprintln!("Error: preview requires a file path");
                std::process::exit(1);
            }
            let path = std::path::PathBuf::from(&args[2]);
            if let Err(e) = show_capture_preview_overlay(path) {
                eprintln!("Preview failed: {e}");
                std::process::exit(1);
            }
            return;
        }
        "login" => {
            if let Err(e) = apexshot::cloud::auth::login() {
                eprintln!("Login failed: {e}");
                std::process::exit(1);
            }
            return;
        }
        "logout" => {
            if let Err(e) = apexshot::cloud::auth::logout() {
                eprintln!("Logout failed: {e}");
                std::process::exit(1);
            }
            return;
        }
        _ => {}
    }

    // For all other commands, run inside tokio runtime
    tokio::runtime::Runtime::new()
        .expect("Failed to create tokio runtime")
        .block_on(async_main(args));
}

async fn async_main(args: Vec<String>) {
    match args[1].as_str() {
        "daemon" => {
            apexshot::gnome_shell::hide_recording_mask_best_effort();

            // Parse legacy flags that still apply to the new tray daemon.
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--debug-hotkeys" => {
                        std::env::set_var("APEXSHOT_HOTKEY_DEBUG", "1");
                        i += 1;
                    }
                    "--log" => {
                        if i + 1 >= args.len() {
                            eprintln!("Error: --log requires a path");
                            std::process::exit(1);
                        }
                        std::env::set_var("APEXSHOT_HOTKEY_LOG", &args[i + 1]);
                        i += 2;
                    }
                    "--reset-config" => {
                        let config_path =
                            std::env::var_os("APEXSHOT_HOTKEY_CONFIG").map(PathBuf::from);
                        match reset_hotkey_config(config_path) {
                            Ok(p) => println!("Hotkey config reset: {}", p.display()),
                            Err(e) => {
                                eprintln!("Failed to reset hotkey config: {e}");
                                std::process::exit(1);
                            }
                        }
                        i += 1;
                    }
                    _ => {
                        eprintln!("Error: unknown daemon option '{}'", args[i]);
                        std::process::exit(1);
                    }
                }
            }

            // GTK MUST run on the main OS thread. We therefore:
            //   1. Spin up a dedicated Tokio runtime on a background thread.
            //   2. The daemon async loop sends GTK work requests to the main
            //      thread via a channel.
            //   3. The main thread runs a tiny dispatch loop that executes
            //      GTK work and sends results back.
            run_daemon_with_gtk_on_main_thread();
        }
        "hotkeys" => {
            if let Err(e) = run_hotkeys_command(&args) {
                eprintln!("Hotkeys command failed: {e}");
                std::process::exit(1);
            }
        }
        "show-last-preview" => {
            if !trigger_daemon_action("show_last_preview").await {
                eprintln!("Show last preview requires a running ApexShot daemon.");
                std::process::exit(1);
            }
        }
        "open-file" => {
            if !trigger_daemon_action("open_file").await {
                eprintln!("Open File requires a running ApexShot daemon.");
                std::process::exit(1);
            }
        }
        "open-from-clipboard" => {
            if !trigger_daemon_action("open_from_clipboard").await {
                eprintln!("Open From Clipboard requires a running ApexShot daemon.");
                std::process::exit(1);
            }
        }
        "restore-recently-closed" => {
            if !trigger_daemon_action("restore_recently_closed").await {
                eprintln!("Restore Recently Closed requires a running ApexShot daemon.");
                std::process::exit(1);
            }
        }
        "toggle-overlays" => {
            if !trigger_daemon_action("toggle_overlays").await {
                eprintln!("Hide/Show Overlays requires a running ApexShot daemon.");
                std::process::exit(1);
            }
        }
        "recording-control" => {
            if args.len() < 3 {
                eprintln!("Error: missing recording control action");
                std::process::exit(1);
            }

            match args[2].as_str() {
                "stop-save" => {
                    trigger_daemon_action("recording_stop_save").await;
                }
                _ => {
                    eprintln!("Error: unknown recording control action '{}'", args[2]);
                    std::process::exit(1);
                }
            }
        }

        "capture" => {
            if args.len() < 3 {
                eprintln!("Error: missing capture type");
                print_usage();
                std::process::exit(1);
            }
            // Try to delegate to the running daemon first (instant, no GTK cold-start).
            let daemon_action = cli::handlers::capture_daemon_action(args[2].as_str());
            if let Some(action) = daemon_action {
                if trigger_daemon_action(action).await {
                    // Daemon handled it — exit this short-lived subprocess immediately.
                    return;
                }
            }
            // Daemon not running — do the capture in-process as before.
            cli::handlers::run_capture(&args);
        }
        "record" => {
            if args.len() < 3 {
                eprintln!("Error: missing recording type");
                print_usage();
                std::process::exit(1);
            }
            let record_type = args[2].as_str();
            // Try to delegate to the running daemon first.
            if let Some(action) = cli::handlers::record_daemon_action(record_type) {
                if trigger_daemon_action(action).await {
                    return;
                }
                // Control actions only work through the daemon.
                if cli::handlers::is_record_control_action(record_type) {
                    eprintln!(
                        "Error: 'record {record_type}' requires a running ApexShot daemon (apexshot daemon)."
                    );
                    std::process::exit(1);
                }
            }
            if let Err(e) = cli::handlers::run_record(&args).await {
                eprintln!("Recording failed: {}", e);
                std::process::exit(1);
            }
        }
        "ocr" => {
            if args.len() < 3 {
                eprintln!("Error: missing image path");
                print_usage();
                std::process::exit(1);
            }
            cli::handlers::run_ocr(&args);
        }
        "edit" => {
            if args.len() < 3 {
                eprintln!("Error: missing image path");
                print_usage();
                std::process::exit(1);
            }
            // Run editor as a subprocess to avoid tokio runtime conflicts
            // The editor runs its own GTK main loop which doesn't play well with tokio
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
            let status = std::process::Command::new(&exe)
                .arg("edit-internal")
                .arg(&args[2])
                .status()
                .expect("Failed to spawn editor");
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
        "settings" => {
            // Run settings as a subprocess to avoid tokio runtime conflicts
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
            let status = std::process::Command::new(&exe)
                .arg("settings-internal")
                .status()
                .expect("Failed to spawn settings");
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
        "history" => {
            // Run history as a subprocess to avoid tokio runtime conflicts
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
            let status = std::process::Command::new(&exe)
                .arg("history-internal")
                .status()
                .expect("Failed to spawn history");
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
        "record-from-overlay" => {
            // Run recording as a subprocess (like the C++ binary approach)
            // to isolate GTK/layer-shell state from the just-closed overlay.
            if args.len() < 3 {
                eprintln!("Error: missing recording request JSON");
                std::process::exit(1);
            }
            let request: apexshot::capture_overlay::RecordingRequest =
                serde_json::from_str(&args[2]).unwrap_or_else(|e| {
                    eprintln!("Error: invalid recording request JSON: {e}");
                    std::process::exit(1);
                });
            if let Err(e) = run_overlay_recording_request(request) {
                eprintln!("Recording failed: {e}");
                std::process::exit(1);
            }
        }
        "recording-countdown-internal" => {
            if args.len() < 4 {
                std::process::exit(1);
            }
            let params: apexshot::recording::RecordingControlsParams =
                serde_json::from_str(&args[2]).unwrap();
            let seconds: u32 = args[3].parse().unwrap_or(3);
            let completed = apexshot::recording::run_recording_countdown_bar(params, seconds)
                .expect("Failed to run recording countdown");
            std::process::exit(if completed { 0 } else { 2 });
        }
        "native-host" => {
            if args.len() >= 3 {
                cli::native_host::run_native_host_command(&args);
                return;
            }
            if let Err(e) = cli::native_host::run_native_host().await {
                eprintln!("Native host failed: {e}");
                std::process::exit(1);
            }
        }
        "--version" | "-V" => println!("apexshot {}", env!("CARGO_PKG_VERSION")),
        "--help" | "-h" | "help" => print_usage(),
        "install" => {
            if app_identity::portal_only() {
                eprintln!("Error: host install is not available in Flatpak/portal-only builds.");
                std::process::exit(1);
            }
            cli::install::run_install(&args);
        }
        "uninstall" => {
            if app_identity::portal_only() {
                eprintln!("Error: host uninstall is not available in Flatpak/portal-only builds.");
                std::process::exit(1);
            }
            cli::install::run_uninstall(&args);
        }
        _ => {
            eprintln!("Error: unknown command '{}'", args[1]);
            print_usage();
            std::process::exit(1);
        }
    }
}

/// Run the daemon with GTK on the main OS thread.
///
/// Architecture:
///   - Main thread: GTK dispatch loop (required by GTK)
///   - Background thread: Tokio async runtime running the daemon
///   - Communication: `std::sync::mpsc` channels between the two
fn run_daemon_with_gtk_on_main_thread() {
    use apexshot::daemon::GtkWork;

    // Channel for the daemon to send GTK work to the main thread.
    let (gtk_tx, gtk_rx) = std::sync::mpsc::channel::<GtkWork>();

    // Initialize GTK before any GTK/layer-shell calls.
    // gtk4::init() is idempotent — safe to call even if GTK is initialized later
    // again via Application::new(). This is needed so gtk4_layer_shell::is_supported()
    // can query the Wayland compositor without panicking.
    if let Err(e) = gtk4::init() {
        eprintln!("[daemon] GTK initialization failed: {e}");
    }

    // Detect layer-shell support here on the GTK main thread (GTK is initialized).
    // gtk4_layer_shell::is_supported() must NOT be called from worker threads.
    let layer_shell_supported = gtk4_layer_shell::is_supported();
    eprintln!("[daemon] Layer Shell (gtk4-layer-shell) supported: {layer_shell_supported}");
    if !layer_shell_supported && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        eprintln!(
            "[daemon] Wayland compositor does not support Layer Shell (e.g. GNOME); \
             area selector will use the C++ capture overlay."
        );
        eprintln!(
            "[daemon] On GNOME Wayland, still screenshots use the C++ overlay with the XDG Screenshot portal for the final image."
        );
    }

    // Spawn the Tokio runtime on a background thread so GTK keeps the main thread.
    let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
    std::thread::spawn(move || {
        rt.block_on(async move {
            if let Err(e) =
                apexshot::daemon::run_daemon_with_gtk_channel(gtk_tx, layer_shell_supported).await
            {
                eprintln!("Daemon failed: {e}");
                std::process::exit(1);
            }
        });
        // Daemon exited cleanly — quit the process so the GTK loop also exits.
        std::process::exit(0);
    });

    // Main thread: process GTK work requests from the daemon.
    while let Ok(work) = gtk_rx.recv() {
        match work {
            GtkWork::SelectAreaLive { reply } => {
                eprintln!("[gtk] SelectAreaLive received — launching C++ overlay");
                let live_start = std::time::Instant::now();
                let result = run_capture_overlay(None)
                    .map_err(|e| apexshot::SelectionError::InitError(e.to_string()));
                eprintln!(
                    "[gtk] SelectAreaLive completed after {:.0}ms",
                    live_start.elapsed().as_millis()
                );
                let _ = reply.send(result);
            }
            GtkWork::CaptureAreaInit { reply } => {
                // portal_only: interactive Screenshot portal (no apexshot-capture).
                // Native: C++ / layer-shell area selector.
                eprintln!(
                    "[gtk] CaptureAreaInit received — {}",
                    if app_identity::portal_only() {
                        "portal-only area capture"
                    } else {
                        "launching area selector"
                    }
                );
                let result = apexshot::capture_overlay::capture_area_file_via_cpp()
                    .map_err(|e| e.to_string());
                let _ = reply.send(result);
            }
            GtkWork::CaptureCrosshair { reply } => {
                eprintln!(
                    "[gtk] CaptureCrosshair received — {}",
                    if app_identity::portal_only() {
                        "portal-only interactive capture"
                    } else {
                        "launching Rust crosshair selector"
                    }
                );
                let result = apexshot::capture_overlay::capture_crosshair_file_via_cpp().map_err(
                    |e| match e {
                        apexshot::SelectionError::Cancelled => "cancelled".to_string(),
                        other => other.to_string(),
                    },
                );
                let _ = reply.send(result);
            }
            GtkWork::CaptureScreen { reply } => {
                eprintln!(
                    "[gtk] CaptureScreen received — {}",
                    if app_identity::portal_only() {
                        "portal-only fullscreen capture"
                    } else {
                        "launching fullscreen capture"
                    }
                );
                let result =
                    apexshot::capture_overlay::capture_screen_file_via_cpp().map_err(|e| match e {
                        apexshot::SelectionError::Cancelled => "cancelled".to_string(),
                        other => other.to_string(),
                    });
                let _ = reply.send(result);
            }
            GtkWork::CaptureWindow { reply } => {
                eprintln!("[gtk] CaptureWindow ignored — window capture discontinued");
                let _ = reply.send(Err(
                    "Window capture is temporarily discontinued. Use area or fullscreen capture instead."
                        .to_string(),
                ));
            }
            GtkWork::RunCountdown {
                seconds,
                params,
                reply,
            } => {
                eprintln!("[gtk] RunCountdown received — launching countdown UI");
                if let Some(params) = params {
                    match run_recording_countdown_bar(params, seconds) {
                        Ok(true) => {}
                        Ok(false) => eprintln!("[gtk] Countdown cancelled"),
                        Err(err) => {
                            eprintln!("[gtk] Countdown bar failed: {err}");
                            apexshot::recording::countdown_overlay::run_countdown_overlay(seconds);
                        }
                    }
                } else {
                    apexshot::recording::countdown_overlay::run_countdown_overlay(seconds);
                }
                let _ = reply.send(());
            }
            GtkWork::SelectArea { capture, reply } => {
                eprintln!(
                    "[gtk] SelectArea received, launching C++ overlay ({}x{})...",
                    capture.width, capture.height
                );
                let ui_start = std::time::Instant::now();
                let tmp_bg = cli::handlers::save_temp_png(&capture);
                let area = run_capture_overlay(tmp_bg.as_deref())
                    .ok()
                    .and_then(|selection| match selection {
                        apexshot::OverlaySelection::Area(area) => area,
                        apexshot::OverlaySelection::Recording(_) => None,
                    })
                    .map(|a| apexshot::SelectionArea {
                        x: a.x,
                        y: a.y,
                        width: a.width,
                        height: a.height,
                    });
                if let Some(ref p) = tmp_bg {
                    let _ = std::fs::remove_file(p);
                }
                eprintln!(
                    "[gtk] SelectArea result after {:.0}ms: {:?}",
                    ui_start.elapsed().as_millis(),
                    area
                );
                let _ = reply.send(area);
            }
        }
    }
}

pub(crate) fn print_usage() {
    println!("ApexShot - Screenshot tool for Linux");
    println!();
    println!("Usage: cargo run -- <command> [options]");
    println!();
    println!("Commands:");
    println!("  daemon           Run hotkey daemon (Wayland-friendly via portal)");
    println!("  hotkeys <sub>    Setup no-daemon desktop keybindings");
    println!("  capture <type>    Capture a screenshot");
    println!("  record <type>     Record video (MP4/GIF)");
    println!("  ocr <image>       Extract text from an image");
    println!("  edit <image>      Open image editor window");
    println!("  show-last-preview Reopen the last capture preview via daemon");
    println!("  settings          Open settings window");
    println!("  history           Open capture history window");
    println!("  login             Sign in to ApexShot Cloud (device authorization)");
    println!("  logout            Sign out of ApexShot Cloud");
    println!("  native-host <sub> Install/uninstall native messaging host");
    println!("  video-editor [mp4] Open the recording editor");
    println!("  install           Install local binary and set up autostart");
    println!("  --version / -V    Print version");
    println!("  uninstall         Remove local install, autostart, and native host manifests");
    println!();

    println!("Daemon options:");
    println!("  --config <path>       Use a specific hotkey config file");
    println!("  --reset-config        Overwrite config with defaults");
    println!("  --configure           Open the portal shortcut configuration UI");
    println!("  --no-desktop-relaunch Disable GNOME desktop-entry relaunch workaround");
    println!("  --debug-hotkeys       Log raw GlobalShortcuts signals (for debugging)");
    println!(
        "  --log <path>          Append daemon logs to a file (useful when launched from desktop)"
    );
    println!();
    println!("Hotkeys subcommands:");
    println!("  setup [--config <path>]     Interactive wizard + install desktop keybindings");
    println!("  install [--config <path>]   Install desktop keybindings from config");
    println!("  uninstall                   Remove desktop keybindings installed by ApexShot");
    println!();
    println!("Capture types:");
    println!("  screen            Capture the entire screen");
    println!("  area              Capture a selected area (Wayland: interactive)");
    println!("  window            Capture a specific window (Wayland: interactive)");
    println!("  crosshair         Capture around a precise point (crosshair mode)");
    println!();
    println!("Capture options:");
    println!("  --output <path>   Save to specific path (default: ~/Pictures)");
    println!("  --no-cursor       Don't include cursor in screenshot");
    println!("  --jpeg [quality]  Save as JPEG with quality 1-100 (default: PNG)");
    println!("  --prefix <text>   Prefix for filename (default: 'screenshot')");
    println!("  --ocr             Run OCR on captured image and copy to clipboard");
    println!();
    println!("Recording types:");
    println!("  screen            Record the full screen");
    println!("  area              Record a selected area");
    println!("  ui                Open the recording configuration UI");
    println!("  stop              Stop and save the active recording (requires daemon)");
    println!();
    println!("Recording options:");
    println!("  --output <path>   Save to specific path (default: ~/Videos/output.mp4)");
    println!("  --gif             Record as GIF and copy to clipboard");
    println!("  --overlay-stop    Show a small window to stop recording (Esc/Stop button)");
    println!();
    println!("Recording control (requires daemon):");
    println!("  recording-control stop-save");
    println!();
    println!("Install options:");
    println!("  --no-autostart            Skip autostart desktop file");
    println!("  --no-binary               Skip binary copy to /usr/local/bin");
    println!("  --dev                     Install as separate apexshot-dev test app");
    println!("  --force                   Reinstall even if the same version is already installed");
    println!("  --extension-id <id>       Also install native host manifest for extension");
    println!();
    println!("Native host subcommands:");
    println!("  native-host install --extension-id <id> [--browser chrome|chromium|both]");
    println!("  native-host uninstall [--browser chrome|chromium|both]");
    println!();
    println!("Examples:");
    println!("  cargo run -- hotkeys setup");
    println!("  cargo run -- hotkeys install");
    println!("  cargo run -- capture screen");
    println!("  cargo run -- record screen");
    println!("  cargo run -- record area --gif");
    println!("  cargo run -- native-host install --extension-id <extension_id>");
    println!("  cargo run -- install --extension-id <extension_id>");
}

fn run_hotkeys_command(args: &[String]) -> anyhow::Result<()> {
    if args.len() < 3 {
        anyhow::bail!("Missing hotkeys subcommand (setup|install|uninstall)");
    }

    let subcommand = args[2].as_str();
    let mut config_path: Option<PathBuf> = None;

    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--config" => {
                if i + 1 >= args.len() {
                    anyhow::bail!("--config requires a path");
                }
                config_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            other => anyhow::bail!("Unknown hotkeys option '{other}'"),
        }
    }

    match subcommand {
        "setup" => {
            setup_hotkeys_for_current_desktop(config_path)?;
        }
        "install" => {
            install_hotkeys_for_current_desktop(config_path)?;
        }
        "uninstall" => {
            uninstall_hotkeys_for_current_desktop()?;
        }
        "export" => {
            let format = if i < args.len() {
                args[i].as_str()
            } else {
                "hyprland"
            };
            match format {
                "hyprland" => {
                    let output =
                        apexshot::hotkeys::export_configured_hotkeys_for_hyprland(config_path)?;
                    println!("{}", output);
                }
                "sway" | "i3" => {
                    let output = apexshot::hotkeys::export_hotkeys_for_sway()?;
                    println!("{}", output);
                }
                "niri" => {
                    let output = apexshot::hotkeys::export_hotkeys_for_niri()?;
                    println!("{}", output);
                }
                "river" => {
                    let output = apexshot::hotkeys::export_hotkeys_for_river()?;
                    println!("{}", output);
                }
                _ => anyhow::bail!(
                    "Unknown export format '{format}' (expected hyprland|sway|niri|river)"
                ),
            }
        }
        _ => {
            anyhow::bail!(
                "Unknown hotkeys subcommand '{subcommand}' (expected setup|install|uninstall|export)"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::cli::handlers::{
        capture_daemon_action, is_record_control_action, record_daemon_action,
    };
    use crate::cli::install::{package_uninstall_command_for, uninstall_binary_paths};

    #[test]
    fn crosshair_capture_type_delegates_to_daemon() {
        assert_eq!(
            capture_daemon_action("crosshair"),
            Some("capture_crosshair")
        );
    }

    #[test]
    fn supported_capture_types_map_to_expected_daemon_actions() {
        assert_eq!(capture_daemon_action("area"), Some("capture_area"));
        assert_eq!(capture_daemon_action("screen"), Some("capture_screen"));
        assert_eq!(capture_daemon_action("window"), Some("capture_window"));
        assert_eq!(capture_daemon_action("unknown"), None);
    }

    #[test]
    fn record_types_map_to_expected_daemon_actions() {
        assert_eq!(record_daemon_action("ui"), Some("open_recording_ui"));
        assert_eq!(record_daemon_action("screen"), Some("record_screen"));
        assert_eq!(record_daemon_action("area"), Some("record_area"));
        // Control actions must use the daemon Trigger names (not legacy aliases).
        assert_eq!(record_daemon_action("stop"), Some("recording_stop_save"));
        assert_eq!(record_daemon_action("unknown"), None);
    }

    #[test]
    fn record_control_actions_require_daemon() {
        assert!(is_record_control_action("stop"));
        assert!(!is_record_control_action("screen"));
        assert!(!is_record_control_action("area"));
        assert!(!is_record_control_action("ui"));
    }

    #[test]
    fn package_uninstall_command_uses_pacman_for_arch_package_installs() {
        assert_eq!(
            package_uninstall_command_for("pacman", false),
            Some(("pacman".into(), vec!["-R".into(), "apexshot".into()]))
        );
        assert_eq!(
            package_uninstall_command_for("pacman", true),
            Some((
                "sudo".into(),
                vec!["pacman".into(), "-R".into(), "apexshot".into()]
            ))
        );
    }

    #[test]
    fn package_uninstall_command_uses_dnf_for_fedora_package_installs() {
        assert_eq!(
            package_uninstall_command_for("dnf", false),
            Some((
                "dnf".into(),
                vec!["remove".into(), "-y".into(), "apexshot".into()]
            ))
        );
        assert_eq!(
            package_uninstall_command_for("dnf", true),
            Some((
                "sudo".into(),
                vec![
                    "dnf".into(),
                    "remove".into(),
                    "-y".into(),
                    "apexshot".into()
                ]
            ))
        );
    }

    #[test]
    fn package_uninstall_command_uses_zypper_for_opensuse_package_installs() {
        assert_eq!(
            package_uninstall_command_for("zypper", false),
            Some((
                "zypper".into(),
                vec![
                    "--non-interactive".into(),
                    "remove".into(),
                    "apexshot".into()
                ]
            ))
        );
        assert_eq!(
            package_uninstall_command_for("zypper", true),
            Some((
                "sudo".into(),
                vec![
                    "zypper".into(),
                    "--non-interactive".into(),
                    "remove".into(),
                    "apexshot".into()
                ]
            ))
        );
    }

    #[test]
    fn local_uninstall_removes_source_install_helpers() {
        assert!(uninstall_binary_paths(false).contains(&"/usr/local/bin/apexshot"));
        assert!(uninstall_binary_paths(false).contains(&"/usr/local/bin/apexshot-capture"));
        assert!(uninstall_binary_paths(false).contains(&"/usr/local/bin/apexshot-native-host"));
    }
}
