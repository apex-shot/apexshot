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

fn capture_daemon_action(capture_type: &str) -> Option<&'static str> {
    match capture_type {
        "area" => Some("capture_area"),
        "crosshair" => Some("capture_crosshair"),
        "previous-area" | "previous_area" => Some("capture_area"),
        "screen" => Some("capture_screen"),
        "window" => Some("capture_window"),
        _ => None,
    }
}

use apexshot::{
    app_identity,
    backend::{CaptureData, DisplayBackend, WaylandBackend, X11Backend},
    capture::{
        open_image_editor, save_capture, show_capture_preview_overlay, ImageFormat, SaveConfig,
    },
    capture_overlay::{
        capture_area_via_cpp, capture_crosshair_via_cpp, capture_screen_via_cpp,
        capture_window_via_cpp, is_launch_blocked_error, open_recording_ui_via_cpp,
        run_capture_overlay, AreaCapturePathResult, AreaCaptureResult,
    },
    daemon::{import_web_scroll_capture, trigger_daemon_action},
    hotkeys::{
        ensure_desktop_entry_pub, install_hotkeys_for_current_desktop, reset_hotkey_config,
        setup_hotkeys_for_current_desktop, uninstall_hotkeys_for_current_desktop,
    },
    ocr::{extract_text_from_capture, extract_text_from_path, OcrConfig},
    onboarding::{is_onboarding_complete, show_onboarding_window},
    preview_launch::{launch_preview, show_preview_direct},
    recording::{
        editor::{open_empty_recording_editor, open_recording_editor},
        run_overlay_recording_request, run_recording_countdown_bar, run_recording_ui,
        run_recording_with_controls, start_recording, RecordingConfig, RecordingControlsParams,
        StopAction,
    },
    settings::show_settings_window,
};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
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
        // No arguments - just open settings or onboarding
        // The tray daemon runs independently based on settings
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
            apexshot::gnome_shell::hide_recording_controls_best_effort();
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
                "pause-resume" => {
                    trigger_daemon_action("recording_pause_resume").await;
                }
                "stop-save" => {
                    trigger_daemon_action("recording_stop_save").await;
                }
                "restart" => {
                    trigger_daemon_action("recording_restart").await;
                }
                "discard" => {
                    trigger_daemon_action("recording_discard").await;
                }
                "move-webcam" => {
                    if args.len() < 5 {
                        eprintln!("Error: move-webcam requires x and y coordinates");
                        std::process::exit(1);
                    }
                    let x: f64 = args[3].parse().unwrap_or(0.0);
                    let y: f64 = args[4].parse().unwrap_or(0.0);
                    if let Ok(conn) = zbus::Connection::session().await {
                        if let Ok(proxy) = zbus::Proxy::new(
                            &conn,
                            apexshot::daemon::DAEMON_BUS_NAME,
                            apexshot::daemon::DAEMON_OBJECT_PATH,
                            apexshot::daemon::DAEMON_INTERFACE,
                        )
                        .await
                        {
                            let _ = proxy.call::<_, _, ()>("move_webcam", &(x, y)).await;
                        }
                    }
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
            let daemon_action = capture_daemon_action(args[2].as_str());
            if let Some(action) = daemon_action {
                if trigger_daemon_action(action).await {
                    // Daemon handled it — exit this short-lived subprocess immediately.
                    return;
                }
            }
            // Daemon not running — do the capture in-process as before.
            run_capture(&args);
        }
        "record" => {
            if args.len() < 3 {
                eprintln!("Error: missing recording type");
                print_usage();
                std::process::exit(1);
            }
            // Try to delegate to the running daemon first.
            let daemon_action = match args[2].as_str() {
                "ui" => Some("open_recording_ui"),
                "screen" => Some("record_screen"),
                "area" => Some("record_area"),
                "stop" => Some("stop_recording_save"),
                "pause" => Some("toggle_recording_pause"), // Simplified to toggle for now
                "resume" => Some("toggle_recording_pause"),
                "toggle-pause" => Some("toggle_recording_pause"),
                "restart" => Some("restart_recording"),
                "discard" => Some("discard_recording"),
                _ => None,
            };
            if let Some(action) = daemon_action {
                if trigger_daemon_action(action).await {
                    return;
                }
            }
            if let Err(e) = run_record(&args).await {
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
            run_ocr(&args);
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
        "recording-ui-internal" => {
            if args.len() < 4 {
                std::process::exit(1);
            }
            let params: RecordingControlsParams = serde_json::from_str(&args[2]).unwrap();
            let seconds: u32 = args[3].parse().unwrap_or(3);

            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = run_recording_ui(params, seconds, tx);

            let action = tokio::task::block_in_place(|| {
                let handle = tokio::runtime::Handle::current();
                handle.block_on(rx)
            })
            .unwrap_or(StopAction::Save);
            match action {
                StopAction::Save => {
                    println!("save");
                    std::process::exit(0);
                }
                StopAction::Discard => {
                    println!("discard");
                    std::process::exit(2);
                }
            }
        }
        "recording-controls-internal" => {
            if args.len() < 3 {
                std::process::exit(1);
            }
            let params: apexshot::recording::RecordingControlsParams =
                serde_json::from_str(&args[2]).unwrap();

            // Optional session ID and bus name for D-Bus communication
            let _session_id = args.get(3).cloned();
            let _bus_name = args.get(4).cloned();

            let (tx, rx) = tokio::sync::oneshot::channel();
            apexshot::recording::run_recording_controls(params, _session_id, _bus_name, tx)
                .expect("Failed to run recording controls");

            let action = tokio::task::block_in_place(|| {
                let handle = tokio::runtime::Handle::current();
                handle.block_on(rx)
            })
            .unwrap_or(apexshot::recording::StopAction::Save);
            match action {
                apexshot::recording::StopAction::Save => std::process::exit(0),
                apexshot::recording::StopAction::Discard => std::process::exit(2),
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
                run_native_host_command(&args);
                return;
            }
            if let Err(e) = run_native_host().await {
                eprintln!("Native host failed: {e}");
                std::process::exit(1);
            }
        }
        "--version" | "-V" => println!("apexshot {}", env!("CARGO_PKG_VERSION")),
        "--help" | "-h" => print_usage(),
        "install" => {
            run_install(&args);
        }
        "uninstall" => {
            run_uninstall(&args);
        }
        _ => {
            eprintln!("Error: unknown command '{}'", args[1]);
            print_usage();
            std::process::exit(1);
        }
    }
}

/// Install the binary to /usr/local/bin/ and set up autostart.
fn run_install(args: &[String]) {
    let mut no_autostart = false;
    let mut no_binary = false;
    let mut force = false;
    let mut dev_install = false;
    let mut extension_id: Option<String> = None;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--no-autostart" => {
                no_autostart = true;
                i += 1;
            }
            "--no-binary" => {
                no_binary = true;
                i += 1;
            }
            "--force" => {
                force = true;
                i += 1;
            }
            "--dev" => {
                dev_install = true;
                i += 1;
            }
            "--extension-id" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --extension-id requires a value");
                    std::process::exit(1);
                }
                extension_id = Some(args[i + 1].clone());
                i += 2;
            }
            other => {
                eprintln!("Error: unknown install option '{other}'");
                std::process::exit(1);
            }
        }
    }

    if !no_binary {
        install_binary(force, dev_install);
        install_desktop_launcher(dev_install);
    }

    if !no_autostart {
        install_autostart(dev_install);
    }

    if let Some(id) = extension_id {
        if let Err(e) = install_native_host_manifest(&id, BrowserTarget::Both) {
            eprintln!("Error: failed to install native host: {e}");
            std::process::exit(1);
        }
    }

    // Persist XDG portal permissions so the user doesn't have to re-approve
    // screenshot/screencast access after every reboot.
    apexshot::backend::portal_permissions::ensure_portal_permissions();

    // Auto-configure shortcuts so they work out of the box on all desktops.
    // Best-effort: don't abort the install if hotkey setup fails.
    let app_config = apexshot::config::load_config();
    if let Err(e) = apexshot::hotkeys::sync_hotkeys_from_app_config(&app_config) {
        eprintln!("Warning: failed to write compositor hotkey snippets: {e}");
    }
    if let Err(e) = apexshot::hotkeys::sync_gnome_hotkeys_for_current_desktop(None) {
        eprintln!("Warning: GNOME hotkey setup skipped: {e}");
    }
}

fn run_uninstall(args: &[String]) {
    let mut autostart_only = false;
    let mut dev_install = false;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--autostart-only" => {
                autostart_only = true;
                i += 1;
            }
            "--dev" => {
                dev_install = true;
                i += 1;
            }
            other => {
                eprintln!("Error: unknown uninstall option '{other}'");
                std::process::exit(1);
            }
        }
    }

    uninstall_autostart(dev_install);

    if !autostart_only {
        if !dev_install && uninstall_package_managed_app_if_present() {
            return;
        }
        uninstall_binary(dev_install);
        uninstall_desktop_launcher(dev_install);
        if let Err(e) = uninstall_native_host_manifest(BrowserTarget::Both) {
            eprintln!("Error: failed to uninstall native host: {e}");
            std::process::exit(1);
        }
    }
}

fn command_exists(command: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {command} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn is_running_as_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| {
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim() == "0")
        })
        .unwrap_or(false)
}

fn pacman_has_apexshot_package() -> bool {
    std::process::Command::new("pacman")
        .args(["-Qq", "apexshot"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn dpkg_has_apexshot_package() -> bool {
    std::process::Command::new("dpkg-query")
        .args(["-W", "-f=${Status}", "apexshot"])
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("install ok installed")
        })
        .unwrap_or(false)
}

fn rpm_has_apexshot_package() -> bool {
    std::process::Command::new("rpm")
        .args(["-q", "apexshot"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn os_release_field(field: &str) -> Option<String> {
    let content = std::fs::read_to_string("/etc/os-release").ok()?;
    content.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key != field {
            return None;
        }
        Some(value.trim_matches('"').to_string())
    })
}

fn rpm_package_manager() -> &'static str {
    let id = os_release_field("ID")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let id_like = os_release_field("ID_LIKE")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let distro = format!(" {id} {id_like} ");

    if distro.contains(" opensuse ") || distro.contains(" suse ") || distro.contains(" sles ") {
        return "zypper";
    }
    if distro.contains(" fedora ")
        || distro.contains(" rhel ")
        || distro.contains(" centos ")
        || distro.contains(" rocky ")
        || distro.contains(" alma ")
    {
        return "dnf";
    }

    if command_exists("dnf") {
        "dnf"
    } else {
        "zypper"
    }
}

fn package_uninstall_command_for(manager: &str, needs_sudo: bool) -> Option<(String, Vec<String>)> {
    match (manager, needs_sudo) {
        ("pacman", true) => Some((
            "sudo".into(),
            vec!["pacman".into(), "-R".into(), "apexshot".into()],
        )),
        ("pacman", false) => Some(("pacman".into(), vec!["-R".into(), "apexshot".into()])),
        ("apt", true) => Some((
            "sudo".into(),
            vec!["apt".into(), "remove".into(), "apexshot".into()],
        )),
        ("apt", false) => Some(("apt".into(), vec!["remove".into(), "apexshot".into()])),
        ("dnf", true) => Some((
            "sudo".into(),
            vec![
                "dnf".into(),
                "remove".into(),
                "-y".into(),
                "apexshot".into(),
            ],
        )),
        ("dnf", false) => Some((
            "dnf".into(),
            vec!["remove".into(), "-y".into(), "apexshot".into()],
        )),
        ("zypper", true) => Some((
            "sudo".into(),
            vec![
                "zypper".into(),
                "--non-interactive".into(),
                "remove".into(),
                "apexshot".into(),
            ],
        )),
        ("zypper", false) => Some((
            "zypper".into(),
            vec![
                "--non-interactive".into(),
                "remove".into(),
                "apexshot".into(),
            ],
        )),
        _ => None,
    }
}

fn package_uninstall_command(manager: &str) -> Option<(String, Vec<String>)> {
    package_uninstall_command_for(manager, !is_running_as_root())
}

fn uninstall_package_managed_app_if_present() -> bool {
    let packaged_binary = std::path::Path::new("/usr/bin/apexshot");
    let packaged_capture = std::path::Path::new("/usr/bin/apexshot-capture");
    if !packaged_binary.exists() && !packaged_capture.exists() {
        return false;
    }

    let manager = if command_exists("pacman") && pacman_has_apexshot_package() {
        Some("pacman")
    } else if command_exists("dpkg-query") && dpkg_has_apexshot_package() {
        Some("apt")
    } else if command_exists("rpm") && rpm_has_apexshot_package() {
        Some(rpm_package_manager())
    } else {
        None
    };

    let Some(manager) = manager else {
        eprintln!("Error: package-managed ApexShot files exist under /usr/bin, but no supported package manager owns an installed 'apexshot' package.");
        eprintln!("Remove ApexShot with your distribution package manager, or remove the package files manually.");
        std::process::exit(1);
    };

    let Some((program, args)) = package_uninstall_command(manager) else {
        eprintln!("Error: unsupported package manager for ApexShot uninstall: {manager}");
        std::process::exit(1);
    };

    println!("Package-managed ApexShot install detected; uninstalling with {manager}.");
    let status = std::process::Command::new(&program).args(&args).status();
    match status {
        Ok(status) if status.success() => {
            println!("✓ Package-managed ApexShot removed");
            true
        }
        Ok(status) => {
            eprintln!("Error: package uninstall failed with status {status}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: failed to run package uninstall command: {e}");
            std::process::exit(1);
        }
    }
}

/// Query an installed apexshot binary for its version string.
/// Returns `None` if the binary cannot be executed or the version cannot be parsed.
fn get_installed_version(binary: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Expected format: "apexshot 0.2.14"
    stdout
        .trim()
        .strip_prefix("apexshot ")
        .map(|v| v.to_string())
}

fn install_binary(force: bool, dev_install: bool) {
    use std::os::unix::fs::PermissionsExt;

    let dest = if dev_install {
        std::path::Path::new("/usr/local/lib/apexshot-dev/apexshot")
    } else {
        std::path::Path::new("/usr/local/bin/apexshot")
    };
    let capture_dest = if dev_install {
        std::path::Path::new("/usr/local/lib/apexshot-dev/apexshot-capture")
    } else {
        std::path::Path::new("/usr/local/bin/apexshot-capture")
    };
    let packaged_dest = std::path::Path::new("/usr/bin/apexshot");
    let packaged_capture_dest = std::path::Path::new("/usr/bin/apexshot-capture");
    let dev_wrapper = std::path::Path::new(app_identity::DEV_WRAPPER);

    let src = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("target/release/apexshot"));

    let current_version = env!("CARGO_PKG_VERSION");

    if !dev_install && (packaged_dest.exists() || packaged_capture_dest.exists()) {
        eprintln!("Error: package-managed ApexShot binaries exist under /usr/bin.");
        eprintln!(
            "`apexshot install` would write to /usr/local/bin/apexshot, which shadows the distro-managed installation."
        );
        eprintln!("Use `sudo apexshot install --dev --no-autostart` for a separate test install,");
        if command_exists("rpm") && rpm_has_apexshot_package() {
            let manager = rpm_package_manager();
            if manager == "dnf" {
                eprintln!("or update the package-managed app with `sudo dnf upgrade apexshot` or an updated RPM.");
            } else {
                eprintln!("or update the package-managed app with `sudo zypper update apexshot` or an updated RPM.");
            }
        } else {
            eprintln!("or update the package-managed app with your distro package manager.");
        }
        std::process::exit(1);
    }

    // Check if an existing installation is present and compare versions.
    if dest.exists() && !force {
        let installed_version = get_installed_version(dest);
        match installed_version {
            Some(ref v) if v == current_version => {
                if dev_install {
                    println!(
                        "Refreshing ApexShot Dev {} at {}.",
                        current_version,
                        dest.display()
                    );
                } else {
                    println!(
                        "ApexShot {} is already installed at {}. Use --force to reinstall.",
                        current_version,
                        dest.display()
                    );
                    return;
                }
            }
            Some(ref v) => {
                println!("Updating ApexShot {} → {}", v, current_version);
            }
            None => {
                // Could not determine version — proceed with install (likely a dev build or corrupted).
                println!(
                    "Existing installation found at {}. Updating to {}.",
                    dest.display(),
                    current_version
                );
            }
        }
    }

    println!("Installing binary: {} → {}", src.display(), dest.display());

    if let Some(parent) = dest.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!(
                "Error: failed to create install directory {}: {e}",
                parent.display()
            );
            std::process::exit(1);
        }
    }

    // Remove first to avoid ETXTBUSY when overwriting a running binary
    let _ = std::fs::remove_file(dest);

    match std::fs::copy(&src, dest) {
        Ok(_) => {
            if let Err(e) = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755)) {
                eprintln!("Warning: could not set executable permissions: {e}");
            } else {
                println!("✓ Binary installed to {}", dest.display());
            }
        }
        Err(e) => {
            eprintln!("Error: failed to install binary: {e}");
            eprintln!("Hint: try running with sudo, e.g.  sudo apexshot install");
            std::process::exit(1);
        }
    }

    let capture_src = src
        .with_file_name("apexshot-capture")
        .exists()
        .then(|| src.with_file_name("apexshot-capture"))
        .or_else(|| {
            option_env!("APEXSHOT_CAPTURE_BIN_DIR").and_then(|dir| {
                let candidate = std::path::PathBuf::from(dir).join("apexshot-capture");
                candidate.exists().then_some(candidate)
            })
        });

    let Some(capture_src) = capture_src else {
        eprintln!("Error: apexshot-capture binary not found next to the built binary");
        eprintln!(
            "Hint: build from the backend directory so the C++ helper is compiled before install"
        );
        std::process::exit(1);
    };

    println!(
        "Installing capture helper: {} → {}",
        capture_src.display(),
        capture_dest.display()
    );

    if let Some(parent) = capture_dest.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!(
                "Error: failed to create install directory {}: {e}",
                parent.display()
            );
            std::process::exit(1);
        }
    }

    let _ = std::fs::remove_file(capture_dest);

    match std::fs::copy(&capture_src, capture_dest) {
        Ok(_) => {
            if let Err(e) =
                std::fs::set_permissions(capture_dest, std::fs::Permissions::from_mode(0o755))
            {
                eprintln!("Warning: could not set capture helper executable permissions: {e}");
            } else {
                println!("✓ Capture helper installed to {}", capture_dest.display());
            }
        }
        Err(e) => {
            eprintln!("Error: failed to install capture helper: {e}");
            eprintln!("Hint: try running with sudo, e.g.  sudo apexshot install");
            std::process::exit(1);
        }
    }

    if dev_install {
        let wrapper_content = format!(
            "#!/usr/bin/env bash\nexport APEXSHOT_APP_FLAVOR=dev\nexport APEXSHOT_CAPTURE_BIN=\"{}\"\nexec \"{}\" \"$@\"\n",
            capture_dest.display(),
            dest.display()
        );
        if let Err(e) = std::fs::write(dev_wrapper, wrapper_content) {
            eprintln!(
                "Error: failed to write dev wrapper {}: {e}",
                dev_wrapper.display()
            );
            std::process::exit(1);
        }
        if let Err(e) =
            std::fs::set_permissions(dev_wrapper, std::fs::Permissions::from_mode(0o755))
        {
            eprintln!("Warning: could not set dev wrapper executable permissions: {e}");
        } else {
            println!("✓ Dev wrapper installed to {}", dev_wrapper.display());
        }
    }
}

fn install_autostart(dev_install: bool) {
    // Clean up stale desktop files from previous `apexshot install` runs.
    // The .deb package installs the proper desktop entry to /usr/share/applications/,
    // but older versions of `apexshot install` wrote one to ~/.local/share/applications/
    // which takes priority and can point to a non-existent binary path.
    {
        let local_apps_dir = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME")
                    .map(std::path::PathBuf::from)
                    .expect("HOME is not set");
                home.join(".local/share")
            })
            .join("applications");

        let stale_desktop = local_apps_dir.join("io.github.codegoddy.apexshot.desktop");
        if stale_desktop.exists() {
            let _ = std::fs::remove_file(&stale_desktop);
            eprintln!(
                "[install] Removed stale desktop entry: {}",
                stale_desktop.display()
            );
        }
    }

    let autostart_dir = {
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME")
                    .map(std::path::PathBuf::from)
                    .expect("HOME is not set");
                home.join(".config")
            });
        config_home.join("autostart")
    };

    if let Err(e) = std::fs::create_dir_all(&autostart_dir) {
        eprintln!("Error: could not create autostart directory: {e}");
        std::process::exit(1);
    }

    let binary_path = if dev_install {
        app_identity::DEV_WRAPPER.to_string()
    } else if std::path::Path::new("/usr/bin/apexshot").exists() {
        "/usr/bin/apexshot".to_string()
    } else if std::path::Path::new("/usr/local/bin/apexshot").exists() {
        "/usr/local/bin/apexshot".to_string()
    } else {
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "apexshot".to_string())
    };

    let desktop_content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={}\n\
         Comment=ApexShot screenshot daemon - tray icon and hotkey listener\n\
         Exec={binary_path} daemon\n\
         Icon={}\n\
         Categories=Utility;\n\
         Keywords=screenshot;capture;record;\n\
         StartupNotify=false\n\
         X-GNOME-Autostart-enabled=true\n\
         X-GNOME-Autostart-Delay=2\n\
         Hidden=false\n\
         NoDisplay=true\n",
        if dev_install {
            "ApexShot Dev Daemon"
        } else {
            "ApexShot Daemon"
        },
        if dev_install {
            app_identity::DEV_APP_ID
        } else {
            app_identity::OFFICIAL_APP_ID
        },
    );

    let desktop_path = autostart_dir.join(if dev_install {
        "apexshot-dev-daemon.desktop"
    } else {
        "apexshot-daemon.desktop"
    });
    match std::fs::write(&desktop_path, &desktop_content) {
        Ok(()) => println!("✓ Autostart entry installed: {}", desktop_path.display()),
        Err(e) => {
            eprintln!("Error: failed to write autostart file: {e}");
            std::process::exit(1);
        }
    }
}

fn user_home_from_passwd(username: &std::ffi::OsStr) -> Option<std::path::PathBuf> {
    let output = std::process::Command::new("getent")
        .arg("passwd")
        .arg(username)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next()?;
    let home = line.split(':').nth(5)?;
    if home.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(home))
    }
}

fn install_desktop_launcher(dev_install: bool) {
    if !dev_install {
        return;
    }

    let local_apps_dir = if let Some(sudo_user) = std::env::var_os("SUDO_USER") {
        // `apexshot install --dev` is normally run via sudo to copy files into
        // /usr/local.  Do not install the launcher into /root; put it in the
        // invoking user's desktop database so Hyprland/app launchers can see it.
        user_home_from_passwd(&sudo_user)
            .unwrap_or_else(|| std::path::PathBuf::from("/home").join(&sudo_user))
            .join(".local/share/applications")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME")
                    .map(std::path::PathBuf::from)
                    .expect("HOME is not set");
                home.join(".local/share")
            })
            .join("applications")
    };

    if let Err(e) = std::fs::create_dir_all(&local_apps_dir) {
        eprintln!("Error: could not create applications directory: {e}");
        std::process::exit(1);
    }

    let desktop_path = local_apps_dir.join("io.github.codegoddy.apexshot.dev.desktop");
    let desktop_content = format!(
        "[Desktop Entry]\n\
         Name=ApexShot Dev\n\
         Comment=Development build of ApexShot\n\
         Exec={}\n\
         Icon=apexshot\n\
         Type=Application\n\
         Categories=Graphics;\n\
         Keywords=screenshot;capture;recording;screen;video;ocr;annotation;\n\
         StartupNotify=true\n\
         StartupWMClass={}\n\
         Terminal=false\n\
         X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n",
        app_identity::DEV_WRAPPER,
        app_identity::DEV_APP_ID,
    );

    match std::fs::write(&desktop_path, desktop_content) {
        Ok(()) => {
            if let Some(sudo_user) = std::env::var_os("SUDO_USER") {
                let _ = std::process::Command::new("chown")
                    .arg(format!(
                        "{}:{}",
                        sudo_user.to_string_lossy(),
                        sudo_user.to_string_lossy()
                    ))
                    .arg(&desktop_path)
                    .status();
            }
            println!("✓ Dev app launcher installed: {}", desktop_path.display())
        }
        Err(e) => {
            eprintln!("Error: failed to write dev launcher: {e}");
            std::process::exit(1);
        }
    }
}

fn uninstall_autostart(dev_install: bool) {
    let autostart_dir = {
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME")
                    .map(std::path::PathBuf::from)
                    .expect("HOME is not set");
                home.join(".config")
            });
        config_home.join("autostart")
    };
    let names: &[&str] = if dev_install {
        &["apexshot-dev-daemon.desktop"]
    } else {
        &["apexshot-daemon.desktop"]
    };
    for name in names {
        let desktop_path = autostart_dir.join(name);
        match std::fs::remove_file(&desktop_path) {
            Ok(()) => println!("✓ Autostart entry removed: {}", desktop_path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("Autostart entry not found: {}", desktop_path.display());
            }
            Err(e) => {
                eprintln!("Error: failed to remove autostart file: {e}");
                std::process::exit(1);
            }
        }
    }
}

fn uninstall_binary_paths(dev_install: bool) -> &'static [&'static str] {
    if dev_install {
        &[
            app_identity::DEV_WRAPPER,
            "/usr/local/lib/apexshot-dev/apexshot",
            "/usr/local/lib/apexshot-dev/apexshot-capture",
        ]
    } else {
        &[
            "/usr/local/bin/apexshot",
            "/usr/local/bin/apexshot-capture",
            "/usr/local/bin/apexshot-native-host",
        ]
    }
}

fn uninstall_binary(dev_install: bool) {
    for path in uninstall_binary_paths(dev_install) {
        match std::fs::remove_file(path) {
            Ok(()) => println!("✓ Removed {}", path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                eprintln!("Error: failed to remove {path}: {e}");
                std::process::exit(1);
            }
        }
    }
}

fn uninstall_desktop_launcher(dev_install: bool) {
    if !dev_install {
        return;
    }

    let Some(mut desktop_path) = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/share"))
        })
    else {
        return;
    };
    desktop_path.push("applications/io.github.codegoddy.apexshot.dev.desktop");

    match std::fs::remove_file(&desktop_path) {
        Ok(()) => println!("✓ Removed {}", desktop_path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            eprintln!("Error: failed to remove {}: {e}", desktop_path.display());
            std::process::exit(1);
        }
    }
}

#[derive(Clone, Copy)]
enum BrowserTarget {
    Chrome,
    Chromium,
    Both,
}

impl BrowserTarget {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "chrome" => Some(Self::Chrome),
            "chromium" => Some(Self::Chromium),
            "both" => Some(Self::Both),
            _ => None,
        }
    }
}

fn user_config_dir() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
    Ok(PathBuf::from(home).join(".config"))
}

fn native_manifest_paths(target: BrowserTarget) -> Result<Vec<PathBuf>, String> {
    let config_dir = user_config_dir()?;
    let filename = "io.github.codegoddy.apexshot.json";
    let mut paths = Vec::new();
    match target {
        BrowserTarget::Chrome => {
            paths.push(
                config_dir
                    .join("google-chrome/NativeMessagingHosts")
                    .join(filename),
            );
        }
        BrowserTarget::Chromium => {
            paths.push(
                config_dir
                    .join("chromium/NativeMessagingHosts")
                    .join(filename),
            );
        }
        BrowserTarget::Both => {
            paths.push(
                config_dir
                    .join("google-chrome/NativeMessagingHosts")
                    .join(filename),
            );
            paths.push(
                config_dir
                    .join("chromium/NativeMessagingHosts")
                    .join(filename),
            );
        }
    }
    Ok(paths)
}

fn validate_extension_id(extension_id: &str) -> Result<(), String> {
    if extension_id.len() != 32 {
        return Err("extension id must be 32 characters".into());
    }
    if !extension_id.chars().all(|c| matches!(c, 'a'..='p')) {
        return Err("extension id must contain only letters a-p".into());
    }
    Ok(())
}

fn install_native_host_manifest(extension_id: &str, browser: BrowserTarget) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    validate_extension_id(extension_id)?;

    let binary_path = app_identity::preferred_command_path();

    let local_bin = if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".local/bin")
    } else {
        return Err("HOME is not set".into());
    };
    std::fs::create_dir_all(&local_bin).map_err(|e| format!("create ~/.local/bin failed: {e}"))?;

    let host_script = local_bin.join("apexshot-native-host");
    let script_content = format!(
        "#!/usr/bin/env bash\nexec \"{}\" native-host\n",
        binary_path.display()
    );
    std::fs::write(&host_script, script_content)
        .map_err(|e| format!("writing native host launcher failed: {e}"))?;
    std::fs::set_permissions(&host_script, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("chmod native host launcher failed: {e}"))?;

    let manifest = serde_json::json!({
        "name": "io.github.codegoddy.apexshot",
        "description": "ApexShot native host",
        "path": host_script,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{}/", extension_id)],
    });

    for path in native_manifest_paths(browser)? {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating manifest dir failed ({}): {e}", parent.display()))?;
        }
        let payload = serde_json::to_string_pretty(&manifest)
            .map_err(|e| format!("serializing manifest failed: {e}"))?;
        std::fs::write(&path, payload)
            .map_err(|e| format!("writing native manifest failed ({}): {e}", path.display()))?;
        println!("✓ Native host manifest installed: {}", path.display());
    }

    Ok(())
}

fn uninstall_native_host_manifest(browser: BrowserTarget) -> Result<(), String> {
    for path in native_manifest_paths(browser)? {
        match std::fs::remove_file(&path) {
            Ok(()) => println!("✓ Native host manifest removed: {}", path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(format!(
                    "failed to remove native manifest ({}): {e}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn run_native_host_command(args: &[String]) {
    if args.len() < 3 {
        eprintln!("Error: native-host requires subcommand (install|uninstall)");
        std::process::exit(1);
    }

    match args[2].as_str() {
        "install" => {
            let mut extension_id: Option<String> = None;
            let mut browser = BrowserTarget::Both;
            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--extension-id" => {
                        if i + 1 >= args.len() {
                            eprintln!("Error: --extension-id requires a value");
                            std::process::exit(1);
                        }
                        extension_id = Some(args[i + 1].clone());
                        i += 2;
                    }
                    "--browser" => {
                        if i + 1 >= args.len() {
                            eprintln!("Error: --browser requires one of chrome|chromium|both");
                            std::process::exit(1);
                        }
                        browser = BrowserTarget::from_str(&args[i + 1]).unwrap_or_else(|| {
                            eprintln!("Error: invalid --browser value '{}', expected chrome|chromium|both", args[i + 1]);
                            std::process::exit(1);
                        });
                        i += 2;
                    }
                    other => {
                        eprintln!("Error: unknown native-host install option '{other}'");
                        std::process::exit(1);
                    }
                }
            }

            let Some(extension_id) = extension_id else {
                eprintln!("Error: native-host install requires --extension-id");
                std::process::exit(1);
            };

            if let Err(e) = install_native_host_manifest(&extension_id, browser) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        "uninstall" => {
            let mut browser = BrowserTarget::Both;
            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--browser" => {
                        if i + 1 >= args.len() {
                            eprintln!("Error: --browser requires one of chrome|chromium|both");
                            std::process::exit(1);
                        }
                        browser = BrowserTarget::from_str(&args[i + 1]).unwrap_or_else(|| {
                            eprintln!("Error: invalid --browser value '{}', expected chrome|chromium|both", args[i + 1]);
                            std::process::exit(1);
                        });
                        i += 2;
                    }
                    other => {
                        eprintln!("Error: unknown native-host uninstall option '{other}'");
                        std::process::exit(1);
                    }
                }
            }

            if let Err(e) = uninstall_native_host_manifest(browser) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        other => {
            eprintln!("Error: unknown native-host subcommand '{other}'");
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
                eprintln!("[gtk] CaptureAreaInit received — launching area selector");
                let result = apexshot::capture_overlay::capture_area_file_via_cpp()
                    .map_err(|e| e.to_string());
                let _ = reply.send(result);
            }
            GtkWork::CaptureCrosshair { reply } => {
                eprintln!("[gtk] CaptureCrosshair received — launching Rust crosshair selector");
                let result = apexshot::capture_overlay::capture_crosshair_file_via_cpp().map_err(
                    |e| match e {
                        apexshot::SelectionError::Cancelled => "cancelled".to_string(),
                        other => other.to_string(),
                    },
                );
                let _ = reply.send(result);
            }
            GtkWork::CaptureScreen { reply } => {
                eprintln!("[gtk] CaptureScreen received — launching fullscreen capture");
                let result =
                    apexshot::capture_overlay::capture_screen_file_via_cpp().map_err(|e| match e {
                        apexshot::SelectionError::Cancelled => "cancelled".to_string(),
                        other => other.to_string(),
                    });
                let _ = reply.send(result);
            }
            GtkWork::CaptureWindow { reply } => {
                eprintln!("[gtk] CaptureWindow received — launching window selector");
                let result =
                    apexshot::capture_overlay::capture_window_file_via_cpp().map_err(|e| match e {
                        apexshot::SelectionError::Cancelled => "cancelled".to_string(),
                        other => other.to_string(),
                    });
                let _ = reply.send(result);
            }
            GtkWork::RunRecordingControls { params, stop_tx } => {
                eprintln!("[gtk] RunRecordingControls received — launching recording controls");
                if let Err(err) =
                    apexshot::recording::run_recording_controls(params, None, None, stop_tx)
                {
                    eprintln!("[gtk] Recording controls failed: {err}");
                }
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
                let tmp_bg = save_temp_png(&capture);
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

fn print_usage() {
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
    println!("  recent-captures   Open the recent captures gallery");
    println!("  settings          Open settings window");
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
    println!();
    println!("Capture options:");
    println!("  --output <path>   Save to specific path (default: ~/Pictures)");
    println!("  --no-cursor       Don't include cursor in screenshot");
    println!("  --jpeg [quality]  Save as JPEG with quality 1-100 (default: PNG)");
    println!("  --prefix <text>   Prefix for filename (default: 'screenshot')");
    println!("  --ocr             Run OCR on captured image and copy to clipboard");
    println!();
    println!("Recording options:");
    println!("  --output <path>   Save to specific path (default: ~/Videos/output.mp4)");
    println!("  --gif             Record as GIF and copy to clipboard");
    println!("  --overlay-stop    Show a small window to stop recording (Esc/Stop button)");
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

fn ensure_gio_desktop_env_for_capture() {
    if let Some(desktop_path) = app_identity::desktop_file_for_portal() {
        std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", desktop_path);
        std::env::set_var(
            "GIO_LAUNCHED_DESKTOP_FILE_PID",
            std::process::id().to_string(),
        );
        return;
    }

    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return;
    }

    let app_id =
        std::env::var("APEXSHOT_APP_ID").unwrap_or_else(|_| app_identity::app_id().to_string());

    if let Ok(desktop_path) = ensure_desktop_entry_pub(&app_id) {
        std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", &desktop_path);
        std::env::set_var(
            "GIO_LAUNCHED_DESKTOP_FILE_PID",
            std::process::id().to_string(),
        );
    }
}

fn run_capture(args: &[String]) {
    ensure_gio_desktop_env_for_capture();

    // Parse capture type
    let capture_type = args[2].as_str();

    // Parse options
    let mut output_path: Option<PathBuf> = None;
    let mut include_cursor = true;
    let mut use_jpeg = false;
    let mut jpeg_quality = 85;
    let mut prefix: Option<String> = None;
    let mut run_ocr = false;
    let mut ocr_lang: Option<String> = None;
    let mut ocr_min_conf: Option<i32> = None;
    let mut ocr_clipboard = true;

    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--output" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --output requires a path");
                    std::process::exit(1);
                }
                output_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--no-cursor" => {
                include_cursor = false;
                i += 1;
            }
            "--jpeg" => {
                use_jpeg = true;
                // Check if next arg is a number
                if i + 1 < args.len() {
                    if let Ok(q) = args[i + 1].parse::<u8>() {
                        jpeg_quality = q;
                        i += 2;
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            "--prefix" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --prefix requires text");
                    std::process::exit(1);
                }
                prefix = Some(args[i + 1].clone());
                i += 2;
            }
            "--ocr" => {
                run_ocr = true;
                i += 1;
            }
            "--lang" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --lang requires a language code");
                    std::process::exit(1);
                }
                ocr_lang = Some(args[i + 1].clone());
                i += 2;
            }
            "--min-conf" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --min-conf requires a number");
                    std::process::exit(1);
                }
                let value: i32 = match args[i + 1].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        eprintln!("Error: --min-conf requires a valid number");
                        std::process::exit(1);
                    }
                };
                ocr_min_conf = Some(value);
                i += 2;
            }
            "--no-clipboard" => {
                ocr_clipboard = false;
                i += 1;
            }
            _ => {
                eprintln!("Error: unknown option '{}'", args[i]);
                std::process::exit(1);
            }
        }
    }

    let capture: CaptureData = match capture_type {
        "screen" => match capture_screen_via_cpp() {
            Ok(capture) => {
                println!("Capturing full screen...");
                capture
            }
            Err(err) if is_launch_blocked_error(&err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
            Err(err) => {
                eprintln!("[capture] C++ fullscreen capture failed: {err}");
                std::process::exit(1);
            }
        },
        "area" => match capture_area_via_cpp() {
            Ok(AreaCaptureResult::Captured(capture)) => {
                println!("Captured area...");
                capture
            }
            Ok(AreaCaptureResult::ScrollCaptured(capture)) => {
                println!("Captured area (scroll)...");
                capture
            }
            Ok(AreaCaptureResult::OcrRequested(capture)) => {
                println!("Captured area (OCR requested)...");
                run_ocr = true;
                capture
            }
            Ok(AreaCaptureResult::RecordingRequested(request)) => {
                // Run recording as a subprocess to fully isolate GTK/layer-shell
                // state from the just-closed capture overlay.
                let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
                let json = serde_json::to_string(&request).unwrap();
                let status = std::process::Command::new(&exe)
                    .arg("record-from-overlay")
                    .arg(&json)
                    .status()
                    .expect("Failed to spawn recording subprocess");
                std::process::exit(status.code().unwrap_or(1));
            }
            Ok(AreaCaptureResult::Cancelled) => {
                eprintln!("Selection cancelled");
                std::process::exit(0);
            }
            Err(err) if is_launch_blocked_error(&err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
            Err(err) => {
                eprintln!("[capture] C++ area-init capture failed: {err}");
                std::process::exit(1);
            }
        },
        "crosshair" => match capture_crosshair_via_cpp() {
            Ok(AreaCaptureResult::Captured(capture)) => {
                println!("Captured crosshair area...");
                capture
            }
            Ok(AreaCaptureResult::Cancelled) => {
                eprintln!("Selection cancelled");
                std::process::exit(0);
            }
            Ok(_) => {
                eprintln!("Error: crosshair capture returned unsupported result");
                std::process::exit(1);
            }
            Err(err) if is_launch_blocked_error(&err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
            Err(err) => {
                eprintln!("[capture] C++ crosshair capture failed: {err}");
                std::process::exit(1);
            }
        },
        "window" if WaylandBackend::is_supported() => match capture_window_via_cpp() {
            Ok(capture) => {
                println!("Captured window...");
                capture
            }
            Err(err) if is_launch_blocked_error(&err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
            Err(err) => {
                eprintln!("[capture] C++ window capture failed: {err}");
                std::process::exit(1);
            }
        },
        _ if WaylandBackend::is_supported() => {
            eprintln!("Error: unknown capture type '{}'", capture_type);
            print_usage();
            std::process::exit(1);
        }
        _ if X11Backend::is_supported() => {
            println!("Using X11 backend...");

            match capture_type {
                "window" => {
                    eprintln!("Error: window capture by ID not yet supported via CLI");
                    eprintln!("Use 'capture screen' and crop manually");
                    std::process::exit(1);
                }
                _ => {
                    eprintln!("Error: unknown capture type '{}'", capture_type);
                    print_usage();
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("Error: No supported display backend found");
            eprintln!("This application requires X11 or Wayland");
            std::process::exit(1);
        }
    };

    println!("Captured: {}x{}", capture.width, capture.height);
    println!(
        "Format: {:?} ({} bpp)",
        capture.format, capture.format.bits_per_pixel
    );
    if capture.cursor.is_some() {
        println!(
            "Cursor: captured ({})",
            if include_cursor {
                "will include"
            } else {
                "will exclude"
            }
        );
    }

    // Build save config
    let format = if use_jpeg {
        ImageFormat::Jpeg {
            quality: jpeg_quality,
        }
    } else {
        ImageFormat::Png
    };

    let mut config = SaveConfig::default()
        .with_format(format)
        .with_cursor(include_cursor);

    if let Some(path) = output_path {
        config = config.with_output_dir(path);
    }

    if let Some(p) = prefix {
        config = config.with_prefix(p);
    }

    // Save the capture (skip for OCR-only mode)
    let saved_path = if run_ocr {
        println!("Running OCR...");
        let mut ocr_config = OcrConfig::default().with_clipboard(ocr_clipboard);

        if let Some(lang) = ocr_lang {
            ocr_config = ocr_config.with_language(lang);
        }

        if let Some(conf) = ocr_min_conf {
            ocr_config = ocr_config.with_min_confidence(conf);
        }

        match extract_text_from_capture(&capture, &ocr_config) {
            Ok(result) => {
                match &result.source {
                    apexshot::ocr::ContentSource::QrCode => {
                        println!("QR code detected and decoded!");
                        println!("Content:");
                        println!("{}", "-".repeat(40));
                        println!("{}", result.text);
                        println!("{}", "-".repeat(40));
                    }
                    apexshot::ocr::ContentSource::Ocr { confidence } => {
                        println!("OCR successful!");
                        println!("Confidence: {}%", confidence);
                        println!("Extracted text:");
                        println!("{}", "-".repeat(40));
                        println!("{}", result.text);
                        println!("{}", "-".repeat(40));
                    }
                }
                if result.copied_to_clipboard {
                    println!("Copied to clipboard");
                }
            }
            Err(e) => {
                eprintln!("OCR failed: {}", e);
                std::process::exit(1);
            }
        }

        // OCR-only mode — exit after copying text to clipboard
        return;
    } else {
        match save_capture(&capture, &config) {
            Ok(path) => {
                println!("Saved to: {}", path.display());
                path
            }
            Err(e) => {
                eprintln!("Error saving capture: {}", e);
                std::process::exit(1);
            }
        }
    };

    // Keep preview in a subprocess on desktops where that preserves the
    // existing GTK isolation / shell tracking behavior. KDE Wayland uses a
    // direct launch path to avoid extra taskbar/loading artifacts.
    if let Err(e) = launch_preview(&saved_path) {
        eprintln!("Warning: Failed to launch preview overlay: {}", e);
        show_preview_direct(saved_path.clone());
    }
}

#[cfg(test)]
mod tests {
    use crate::{capture_daemon_action, package_uninstall_command_for, uninstall_binary_paths};

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

/// Save a CaptureData as a temp PNG for passing to the C++ overlay as background.
/// Returns the path if successful, None on failure (overlay will run without background).
fn save_temp_png(capture: &CaptureData) -> Option<std::path::PathBuf> {
    use image::{ImageBuffer, Rgba};

    let tmp = std::env::temp_dir().join(format!(
        "apexshot_bg_{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));

    let bytes_per_pixel = capture.format.bytes_per_pixel as usize;
    let stride = capture.stride as usize;
    let w = capture.width;
    let h = capture.height;

    use apexshot::backend::PixelFormat;
    let is_bgr = capture.format == PixelFormat::BGR24
        || capture.format == PixelFormat::BGR32
        || capture.format == PixelFormat::BGRA32;

    // Build RGBA pixel buffer from capture data
    let mut rgba: Vec<u8> = Vec::with_capacity((w * h * 4) as usize);
    for row in 0..h as usize {
        let row_start = row * stride;
        let row_end = row_start + w as usize * bytes_per_pixel;
        let row_data = &capture.pixels[row_start..row_end.min(capture.pixels.len())];
        for px in row_data.chunks(bytes_per_pixel) {
            if px.len() >= 4 {
                if is_bgr {
                    rgba.push(px[2]); // R (from BGR byte[2])
                    rgba.push(px[1]); // G
                    rgba.push(px[0]); // B (from BGR byte[0])
                    rgba.push(px[3]); // A
                } else {
                    rgba.push(px[0]); // R
                    rgba.push(px[1]); // G
                    rgba.push(px[2]); // B
                    rgba.push(px[3]); // A
                }
            } else if px.len() == 3 {
                if is_bgr {
                    rgba.push(px[2]);
                    rgba.push(px[1]);
                    rgba.push(px[0]);
                } else {
                    rgba.push(px[0]);
                    rgba.push(px[1]);
                    rgba.push(px[2]);
                }
                rgba.push(255);
            }
        }
    }

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(w, h, rgba)?;
    img.save(&tmp).ok()?;
    Some(tmp)
}

fn run_ocr(args: &[String]) {
    let image_path = &args[2];

    // Parse OCR options
    let mut ocr_lang: Option<String> = None;
    let mut ocr_min_conf: Option<i32> = None;
    let mut ocr_clipboard = true;

    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--lang" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --lang requires a language code");
                    std::process::exit(1);
                }
                ocr_lang = Some(args[i + 1].clone());
                i += 2;
            }
            "--min-conf" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --min-conf requires a number");
                    std::process::exit(1);
                }
                let value: i32 = match args[i + 1].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        eprintln!("Error: --min-conf requires a valid number");
                        std::process::exit(1);
                    }
                };
                ocr_min_conf = Some(value);
                i += 2;
            }
            "--no-clipboard" => {
                ocr_clipboard = false;
                i += 1;
            }
            _ => {
                eprintln!("Error: unknown option '{}'", args[i]);
                print_usage();
                std::process::exit(1);
            }
        }
    }

    // Build OCR config
    let mut ocr_config = OcrConfig::default().with_clipboard(ocr_clipboard);

    if let Some(lang) = ocr_lang {
        ocr_config = ocr_config.with_language(lang);
    }

    if let Some(conf) = ocr_min_conf {
        ocr_config = ocr_config.with_min_confidence(conf);
    }

    // Run OCR
    println!("Running OCR on: {}", image_path);
    match extract_text_from_path(image_path, &ocr_config) {
        Ok(result) => {
            match &result.source {
                apexshot::ocr::ContentSource::QrCode => {
                    println!("QR code detected and decoded!");
                    println!("Content:");
                    println!("{}", "-".repeat(40));
                    println!("{}", result.text);
                    println!("{}", "-".repeat(40));
                }
                apexshot::ocr::ContentSource::Ocr { confidence } => {
                    println!("OCR successful!");
                    println!("Confidence: {}%", confidence);
                    println!("Extracted text:");
                    println!("{}", "-".repeat(40));
                    println!("{}", result.text);
                    println!("{}", "-".repeat(40));
                }
            }
            if result.copied_to_clipboard {
                println!("Copied to clipboard");
            }
        }
        Err(e) => {
            eprintln!("OCR failed: {}", e);
            std::process::exit(1);
        }
    }
}

async fn run_record(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let record_type = args[2].as_str();
    let mut output_path: Option<PathBuf> = None;
    let mut is_gif = false;
    let mut overlay_stop = false;

    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--output" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --output requires a path");
                    std::process::exit(1);
                }
                output_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--gif" => {
                is_gif = true;
                i += 1;
            }
            "--overlay-stop" => {
                overlay_stop = true;
                i += 1;
            }
            _ => {
                eprintln!("Error: unknown option '{}'", args[i]);
                std::process::exit(1);
            }
        }
    }

    let mut config = RecordingConfig::default();

    // Configure output path
    if let Some(p) = output_path {
        config.output_path = p;
        if is_gif
            && config
                .output_path
                .extension()
                .map(|e| e != "gif")
                .unwrap_or(true)
        {
            config.output_path.set_extension("gif");
        }
    } else if is_gif {
        config.output_path.set_extension("gif");
    }

    // Handle area selection if needed
    if record_type == "area" {
        // If on X11, launch overlay
        if std::env::var("WAYLAND_DISPLAY").is_err() && X11Backend::is_supported() {
            println!("Select an area to record by dragging the mouse. Press ESC to cancel.");

            let selection =
                run_capture_overlay(None).map_err(|e| format!("Selection failed: {}", e))?;
            if let apexshot::OverlaySelection::Area(Some(area)) = selection {
                config.x = Some(area.x);
                config.y = Some(area.y);
                config.width = Some(area.width as u32);
                config.height = Some(area.height as u32);
            } else {
                println!("Selection cancelled.");
                return Ok(());
            }
        } else {
            // Wayland area = portal selection (handled in start_recording)
            println!("Wayland detected: 'area' recording triggers system screen/window selection.");
        }
    } else if record_type == "ui" {
        match open_recording_ui_via_cpp()
            .map_err(|e| format!("Failed to open recording UI: {e}"))?
        {
            AreaCapturePathResult::RecordingRequested(request) => {
                let _ = run_overlay_recording_request(request)?;
                return Ok(());
            }
            AreaCapturePathResult::RecordingConfigUpdated | AreaCapturePathResult::Cancelled => {
                return Ok(());
            }
            other => {
                return Err(format!("Unexpected recording UI result: {other:?}").into());
            }
        }
    } else if record_type != "screen" {
        eprintln!(
            "Error: recording type '{}' not supported (use 'screen', 'area', or 'ui')",
            record_type
        );
        std::process::exit(1);
    }

    let final_path = if overlay_stop {
        let params = RecordingControlsParams {
            capture_x: 0,
            capture_y: 0,
            capture_w: 0,
            capture_h: 0,
            is_fullscreen: true,
            show_timer: true,
            use_shell_mask: false,
            dim_screen: false,
            show_webcam: false,
            webcam_device: -1,
            webcam_size: 1,
            webcam_shape: 0,
            webcam_rel_x: 0.0,
            webcam_rel_y: 0.0,
            webcam_flip: false,
            countdown_enabled: false,
            countdown_seconds: 3,
            session_id: None,
        };

        let controls_outcome = run_recording_with_controls(config, params)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error>
            })?;

        match controls_outcome {
            (path, StopAction::Discard) => {
                let _ = std::fs::remove_file(&path);
                return Ok(());
            }
            (path, StopAction::Save) => {
                eprintln!("Recording saved: {:?}", path);
                path
            }
        }
    } else {
        start_recording(config)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?
    };

    // Post-processing
    if let Some(ext) = final_path.extension() {
        if ext == "gif" {
            // For GIFs, we default to copying to clipboard (feature requested)
            if let Err(e) = apexshot::recording::copy_to_clipboard(&final_path) {
                eprintln!("Warning: Failed to copy GIF to clipboard: {}", e);
            }
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct NativeHostRequest {
    cmd: String,
    png_data_url: Option<String>,
    page_url: Option<String>,
    page_title: Option<String>,
    extension_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct NativeHostResponse {
    ok: bool,
    message: String,
}

fn write_native_host_response(resp: &NativeHostResponse) -> Result<(), String> {
    let payload = serde_json::to_vec(resp).map_err(|e| e.to_string())?;
    let len = payload.len() as u32;
    let mut stdout = std::io::stdout();
    stdout
        .write_all(&len.to_le_bytes())
        .map_err(|e| e.to_string())?;
    stdout.write_all(&payload).map_err(|e| e.to_string())?;
    stdout.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn extract_png_base64(data_url: &str) -> Result<String, String> {
    let prefix = "data:image/png;base64,";
    if let Some(rest) = data_url.strip_prefix(prefix) {
        if rest.is_empty() {
            return Err("Empty PNG payload".into());
        }
        return Ok(rest.to_string());
    }
    Err("png_data_url must be a data:image/png;base64 URL".into())
}

fn import_web_scroll_capture_direct(
    png_base64: String,
    _page_url: String,
    _page_title: String,
) -> Result<PathBuf, String> {
    use apexshot::backend::PixelFormat;
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let decoded = STANDARD
        .decode(png_base64.as_bytes())
        .map_err(|e| format!("Invalid base64 payload: {e}"))?;
    let image = image::load_from_memory(&decoded)
        .map_err(|e| format!("Invalid image payload: {e}"))?
        .to_rgba8();

    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return Err("Imported image is empty".into());
    }

    let capture = CaptureData::new(image.into_raw(), width, height, PixelFormat::RGBA32);
    let saved_path = save_capture(&capture, &SaveConfig::default())
        .map_err(|e| format!("Failed to save imported capture: {e}"))?;

    launch_preview(&saved_path).map_err(|e| format!("Failed to launch preview overlay: {e}"))?;

    Ok(saved_path)
}

async fn run_native_host() -> Result<(), String> {
    let mut stdin = std::io::stdin();

    loop {
        let mut len_buf = [0u8; 4];
        match stdin.read_exact(&mut len_buf) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(format!("Failed to read native message length: {e}")),
        }

        let len = u32::from_le_bytes(len_buf) as usize;
        if len == 0 {
            let _ = write_native_host_response(&NativeHostResponse {
                ok: false,
                message: "Empty request".into(),
            });
            continue;
        }

        let mut payload = vec![0u8; len];
        stdin
            .read_exact(&mut payload)
            .map_err(|e| format!("Failed to read native message payload: {e}"))?;

        let req: NativeHostRequest = match serde_json::from_slice(&payload) {
            Ok(req) => req,
            Err(e) => {
                let _ = write_native_host_response(&NativeHostResponse {
                    ok: false,
                    message: format!("Invalid JSON: {e}"),
                });
                continue;
            }
        };

        // Handle ping command for connection testing
        if req.cmd == "ping" {
            let _ = write_native_host_response(&NativeHostResponse {
                ok: true,
                message: "Pong".into(),
            });
            continue;
        }

        // Handle auto-registration request
        if req.cmd == "auto_register" {
            if let Some(extension_id) = req.extension_id {
                match install_native_host_manifest(&extension_id, BrowserTarget::Both) {
                    Ok(_) => {
                        let _ = write_native_host_response(&NativeHostResponse {
                            ok: true,
                            message: format!(
                                "Native host registered for extension {}",
                                extension_id
                            ),
                        });
                    }
                    Err(e) => {
                        let _ = write_native_host_response(&NativeHostResponse {
                            ok: false,
                            message: format!("Failed to register native host: {e}"),
                        });
                    }
                }
            } else {
                let _ = write_native_host_response(&NativeHostResponse {
                    ok: false,
                    message: "Missing extension_id in auto_register request".into(),
                });
            }
            continue;
        }

        if req.cmd != "capture_web_scroll" {
            let _ = write_native_host_response(&NativeHostResponse {
                ok: false,
                message: format!("Unsupported cmd: {}", req.cmd),
            });
            continue;
        }

        let Some(data_url) = req.png_data_url.as_deref() else {
            let _ = write_native_host_response(&NativeHostResponse {
                ok: false,
                message: "Missing png_data_url".into(),
            });
            continue;
        };

        let png_base64 = match extract_png_base64(data_url) {
            Ok(value) => value,
            Err(err) => {
                let _ = write_native_host_response(&NativeHostResponse {
                    ok: false,
                    message: err,
                });
                continue;
            }
        };

        let page_url = req.page_url.unwrap_or_default();
        let page_title = req.page_title.unwrap_or_default();
        let imported =
            import_web_scroll_capture(png_base64.clone(), page_url.clone(), page_title.clone())
                .await;

        if imported {
            let _ = write_native_host_response(&NativeHostResponse {
                ok: true,
                message: "Imported web scroll capture".into(),
            });
            continue;
        }

        match import_web_scroll_capture_direct(png_base64, page_url, page_title) {
            Ok(_) => {
                let _ = write_native_host_response(&NativeHostResponse {
                    ok: true,
                    message: "Imported web scroll capture without daemon".into(),
                });
            }
            Err(err) => {
                let _ = write_native_host_response(&NativeHostResponse {
                    ok: false,
                    message: format!("Daemon not available and direct import failed: {err}"),
                });
            }
        }
    }
}
