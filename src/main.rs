//! OpenShotX CLI - Screenshot tool for Linux
//!
//! Usage:
//!   cargo run -- capture screen
//!   cargo run -- capture area
//!   cargo run -- capture window
//!   cargo run -- record screen
//!   cargo run -- record area
//!   cargo run -- ocr <image>

use gtk4;
use gtk4_layer_shell;

use cleanshitx::{
    backend::{CaptureData, DisplayBackend, WaylandBackend, X11Backend},
    capture::{
        open_image_editor, save_capture, show_capture_preview_overlay, ImageFormat, SaveConfig,
    },
    capture_overlay::{capture_area_via_cpp, capture_screen_via_cpp, run_capture_overlay, AreaCaptureResult},
    daemon::{import_web_scroll_capture, trigger_daemon_action},
    hotkeys::{
        ensure_desktop_entry_pub, install_hotkeys_for_current_desktop, reset_hotkey_config,
        setup_hotkeys_for_current_desktop, uninstall_hotkeys_for_current_desktop,
    },
    ocr::{extract_text_from_path, OcrConfig},
    recording::{
        copy_to_clipboard as copy_recording_to_clipboard, run_recording_stop_overlay,
        start_recording, start_recording_with_stop, RecordingConfig,
    },
    show_settings_window,
};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "daemon" => {
            // Parse legacy flags that still apply to the new tray daemon.
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--debug-hotkeys" => {
                        std::env::set_var("CLEANSHITX_HOTKEY_DEBUG", "1");
                        i += 1;
                    }
                    "--log" => {
                        if i + 1 >= args.len() {
                            eprintln!("Error: --log requires a path");
                            std::process::exit(1);
                        }
                        std::env::set_var("CLEANSHITX_HOTKEY_LOG", &args[i + 1]);
                        i += 2;
                    }
                    "--reset-config" => {
                        let config_path =
                            std::env::var_os("CLEANSHITX_HOTKEY_CONFIG").map(PathBuf::from);
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
            return;
        }
        "hotkeys" => {
            if let Err(e) = run_hotkeys_command(&args) {
                eprintln!("Hotkeys command failed: {e}");
                std::process::exit(1);
            }
        }
        "preview" => {
            // Show the capture preview overlay for a given file path.
            // Spawned as a subprocess by the daemon to avoid GTK thread conflicts.
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
        "capture" => {
            if args.len() < 3 {
                eprintln!("Error: missing capture type");
                print_usage();
                std::process::exit(1);
            }
            // Try to delegate to the running daemon first (instant, no GTK cold-start).
            let daemon_action = match args[2].as_str() {
                "area" => Some("capture_area"),
                "screen" => Some("capture_screen"),
                "window" => Some("capture_window"),
                _ => None,
            };
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
                "screen" => Some("record_screen"),
                "area" => Some("record_area"),
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
            run_edit(&args);
        }
        "settings" => {
            if let Err(e) = show_settings_window() {
                eprintln!("Failed to open settings window: {e}");
                std::process::exit(1);
            }
            return;
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
            return;
        }
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
        install_binary();
    }

    if !no_autostart {
        install_autostart();
    }

    if let Some(id) = extension_id {
        if let Err(e) = install_native_host_manifest(&id, BrowserTarget::Both) {
            eprintln!("Error: failed to install native host: {e}");
            std::process::exit(1);
        }
    }
}

fn run_uninstall(args: &[String]) {
    let mut autostart_only = false;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--autostart-only" => {
                autostart_only = true;
                i += 1;
            }
            other => {
                eprintln!("Error: unknown uninstall option '{other}'");
                std::process::exit(1);
            }
        }
    }

    uninstall_autostart();

    if !autostart_only {
        if let Err(e) = uninstall_native_host_manifest(BrowserTarget::Both) {
            eprintln!("Error: failed to uninstall native host: {e}");
            std::process::exit(1);
        }
    }
}

fn install_binary() {
    let dest = std::path::Path::new("/usr/local/bin/cleanshitx");

    // Find the source binary: prefer the running executable itself.
    let src = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("target/release/cleanshitx"));

    println!("Installing binary: {} → {}", src.display(), dest.display());

    match std::fs::copy(&src, dest) {
        Ok(_) => {
            // Make it executable (rwxr-xr-x = 0o755).
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755)) {
                eprintln!("Warning: could not set executable permissions: {e}");
            } else {
                println!("✓ Binary installed to {}", dest.display());
            }
        }
        Err(e) => {
            eprintln!("Error: failed to install binary: {e}");
            eprintln!("Hint: try running with sudo, e.g.  sudo cleanshitx install");
            std::process::exit(1);
        }
    }
}

fn install_autostart() {
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

    // The binary path to launch — prefer the installed system path.
    let binary_path = if std::path::Path::new("/usr/local/bin/cleanshitx").exists() {
        "/usr/local/bin/cleanshitx".to_string()
    } else {
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "cleanshitx".to_string())
    };

    let desktop_content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=CleanShotX Daemon\n\
         Comment=CleanShotX screenshot daemon — tray icon and hotkey listener\n\
         Exec={binary_path} daemon\n\
         Icon=camera-photo\n\
         Categories=Utility;\n\
         Keywords=screenshot;capture;record;\n\
         StartupNotify=false\n\
         X-GNOME-Autostart-enabled=true\n\
         X-GNOME-Autostart-Delay=2\n\
         Hidden=false\n\
         NoDisplay=true\n"
    );

    let desktop_path = autostart_dir.join("cleanshitx-daemon.desktop");
    match std::fs::write(&desktop_path, &desktop_content) {
        Ok(()) => println!("✓ Autostart entry installed: {}", desktop_path.display()),
        Err(e) => {
            eprintln!("Error: failed to write autostart file: {e}");
            std::process::exit(1);
        }
    }
}

fn uninstall_autostart() {
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
    let desktop_path = autostart_dir.join("cleanshitx-daemon.desktop");
    match std::fs::remove_file(&desktop_path) {
        Ok(()) => println!("✓ Autostart entry removed: {}", desktop_path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("Autostart entry not found (nothing to remove).");
        }
        Err(e) => {
            eprintln!("Error: failed to remove autostart file: {e}");
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
    let filename = "io.github.codegoddy.cleanshitx.json";
    let mut paths = Vec::new();
    match target {
        BrowserTarget::Chrome => {
            paths.push(config_dir.join("google-chrome/NativeMessagingHosts").join(filename));
        }
        BrowserTarget::Chromium => {
            paths.push(config_dir.join("chromium/NativeMessagingHosts").join(filename));
        }
        BrowserTarget::Both => {
            paths.push(config_dir.join("google-chrome/NativeMessagingHosts").join(filename));
            paths.push(config_dir.join("chromium/NativeMessagingHosts").join(filename));
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

    let binary_path = if std::path::Path::new("/usr/local/bin/cleanshitx").exists() {
        PathBuf::from("/usr/local/bin/cleanshitx")
    } else {
        std::env::current_exe().map_err(|e| format!("current_exe failed: {e}"))?
    };

    let local_bin = if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".local/bin")
    } else {
        return Err("HOME is not set".into());
    };
    std::fs::create_dir_all(&local_bin).map_err(|e| format!("create ~/.local/bin failed: {e}"))?;

    let host_script = local_bin.join("cleanshitx-native-host");
    let script_content = format!(
        "#!/usr/bin/env bash\nexec \"{}\" native-host\n",
        binary_path.display()
    );
    std::fs::write(&host_script, script_content)
        .map_err(|e| format!("writing native host launcher failed: {e}"))?;
    std::fs::set_permissions(&host_script, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("chmod native host launcher failed: {e}"))?;

    let manifest = serde_json::json!({
        "name": "io.github.codegoddy.cleanshitx",
        "description": "CleanShotX native host",
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
                return Err(format!("failed to remove native manifest ({}): {e}", path.display()));
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
    use cleanshitx::daemon::GtkWork;

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
             area selector will use screenshot-backed mode."
        );
        eprintln!("[daemon] ⚠ On GNOME Wayland: background capture via Screenshot portal will trigger a screen flash + sound before the selector UI opens. This is the known bug being investigated.");
    }

    // Spawn the Tokio runtime on a background thread so GTK keeps the main thread.
    let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
    std::thread::spawn(move || {
        rt.block_on(async move {
            if let Err(e) =
                cleanshitx::daemon::run_daemon_with_gtk_channel(gtk_tx, layer_shell_supported)
                    .await
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
                    .map(|opt| opt.map(|a| cleanshitx::SelectionArea {
                        x: a.x, y: a.y, width: a.width, height: a.height,
                    }))
                    .map_err(|e| cleanshitx::SelectionError::InitError(e.to_string()));
                eprintln!("[gtk] SelectAreaLive completed after {:.0}ms", live_start.elapsed().as_millis());
                let _ = reply.send(result);
            }
            GtkWork::SelectArea { capture, reply } => {
                eprintln!("[gtk] SelectArea received, launching C++ overlay ({}x{})...", capture.width, capture.height);
                let ui_start = std::time::Instant::now();
                let tmp_bg = save_temp_png(&capture);
                let area = run_capture_overlay(tmp_bg.as_deref())
                    .ok()
                    .flatten()
                    .map(|a| cleanshitx::SelectionArea {
                        x: a.x, y: a.y, width: a.width, height: a.height,
                    });
                if let Some(ref p) = tmp_bg { let _ = std::fs::remove_file(p); }
                eprintln!("[gtk] SelectArea result after {:.0}ms: {:?}", ui_start.elapsed().as_millis(), area);
                let _ = reply.send(area);
            }
        }
    }
}

fn print_usage() {
    println!("OpenShotX - Screenshot tool for Linux");
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
    println!("  settings          Open settings window");
    println!("  native-host <sub> Install/uninstall native messaging host");
    println!("  install           Install binary to /usr/local/bin/ and set up autostart");
    println!("  uninstall         Remove autostart entry (and native host manifests by default)");
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
    println!("  uninstall                   Remove desktop keybindings installed by CleanShotX");
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
        _ => {
            anyhow::bail!(
                "Unknown hotkeys subcommand '{subcommand}' (expected setup|install|uninstall)"
            );
        }
    }

    Ok(())
}

fn ensure_gio_desktop_env_for_capture() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return;
    }

    let app_id = std::env::var("CLEANSHITX_APP_ID")
        .unwrap_or_else(|_| "io.github.codegoddy.cleanshitx".to_string());

    if let Ok(desktop_path) = ensure_desktop_entry_pub(&app_id) {
        if std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE").is_none() {
            std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", &desktop_path);
        }
        if std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE_PID").is_none() {
            std::env::set_var(
                "GIO_LAUNCHED_DESKTOP_FILE_PID",
                std::process::id().to_string(),
            );
        }
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

    let cpp_capture = match capture_type {
        "screen" => match capture_screen_via_cpp() {
            Ok(capture) => Some(capture),
            Err(err) => {
                eprintln!(
                    "[capture] C++ fullscreen capture failed ({err}); falling back to Rust backend."
                );
                None
            }
        },
        "area" => match capture_area_via_cpp() {
            Ok(AreaCaptureResult::Captured(capture)) => Some(capture),
            Ok(AreaCaptureResult::ScrollCaptured(capture)) => Some(capture),
            Ok(AreaCaptureResult::OcrRequested(capture)) => {
                run_ocr = true;
                Some(capture)
            }
            Ok(AreaCaptureResult::Cancelled) => {
                eprintln!("Selection cancelled");
                std::process::exit(0);
            }
            Err(err) => {
                eprintln!(
                    "[capture] C++ area-init capture failed ({err}); falling back to Rust backend."
                );
                None
            }
        },
        _ => None,
    };

    let capture: CaptureData = if let Some(capture) = cpp_capture {
        println!("Using C++ capture backend...");
        capture
    } else if WaylandBackend::is_supported() {
        println!("Using Wayland backend...");
        let backend = WaylandBackend::new().expect("Failed to initialize Wayland backend");

        match capture_type {
            "screen" => backend.capture_screen().expect("Screen capture failed"),
            "area" => {
                println!("Select an area by dragging the mouse. Press ESC to cancel.");

                // 1. Take full screenshot via Rust Wayland backend
                let full_capture = backend.capture_screen_for_selection_impl()
                    .expect("Failed to capture screen for area selection");

                // 2. Save to temp PNG and pass to C++ overlay as background
                let tmp_bg = save_temp_png(&full_capture);
                let selection = run_capture_overlay(tmp_bg.as_deref())
                    .expect("Failed to show area selection UI");
                if let Some(ref p) = tmp_bg { let _ = std::fs::remove_file(p); }

                let Some(area) = selection else {
                    eprintln!("Selection cancelled");
                    std::process::exit(0);
                };

                let is_fullscreen = area.x <= 0
                    && area.y <= 0
                    && area.width >= full_capture.width as i32
                    && area.height >= full_capture.height as i32;

                if is_fullscreen {
                    full_capture
                } else {
                    crop_capture_data(&full_capture, area.x, area.y, area.width, area.height)
                        .expect("Area crop failed")
                }
            }
            "window" => {
                println!(
                    "Note: On Wayland, window capture requires selecting the window in the portal prompt"
                );
                backend.capture_window(0).expect("Window capture failed")
            }
            _ => {
                eprintln!("Error: unknown capture type '{}'", capture_type);
                print_usage();
                std::process::exit(1);
            }
        }
    } else if X11Backend::is_supported() {
        println!("Using X11 backend...");
        let backend = X11Backend::new().expect("Failed to initialize X11 backend");

        match capture_type {
            "screen" => backend.capture_screen().expect("Screen capture failed"),
            "area" => {
                println!("Select an area by dragging the mouse. Press ESC to cancel.");

                // 1. Take full screenshot via Rust X11 backend
                let full_capture = backend.capture_screen()
                    .expect("Failed to capture screen for area selection");

                // 2. Save to temp PNG and pass to C++ overlay as background
                let tmp_bg = save_temp_png(&full_capture);
                let selection = run_capture_overlay(tmp_bg.as_deref())
                    .expect("Failed to show area selection UI");
                if let Some(ref p) = tmp_bg { let _ = std::fs::remove_file(p); }

                let Some(area) = selection else {
                    eprintln!("Selection cancelled");
                    std::process::exit(0);
                };

                let is_fullscreen = area.x <= 0
                    && area.y <= 0
                    && area.width >= full_capture.width as i32
                    && area.height >= full_capture.height as i32;

                if is_fullscreen {
                    full_capture
                } else {
                    crop_capture_data(&full_capture, area.x, area.y, area.width, area.height)
                        .expect("Area crop failed")
                }
            }
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
    } else {
        eprintln!("Error: No supported display backend found");
        eprintln!("This application requires X11 or Wayland");
        std::process::exit(1);
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

    // Save the capture
    let saved_path = match save_capture(&capture, &config) {
        Ok(path) => {
            println!("Saved to: {}", path.display());
            path
        }
        Err(e) => {
            eprintln!("Error saving capture: {}", e);
            std::process::exit(1);
        }
    };

    // Run OCR if requested
    if run_ocr {
        println!("Running OCR...");
        let mut ocr_config = OcrConfig::default().with_clipboard(ocr_clipboard);

        if let Some(lang) = ocr_lang {
            ocr_config = ocr_config.with_language(lang);
        }

        if let Some(conf) = ocr_min_conf {
            ocr_config = ocr_config.with_min_confidence(conf);
        }

        match extract_text_from_path(&saved_path, &ocr_config) {
            Ok(result) => {
                println!("OCR successful!");
                println!("Confidence: {}%", result.confidence);
                println!("Extracted text:");
                println!("{}", "-".repeat(40));
                println!("{}", result.text);
                println!("{}", "-".repeat(40));
                if result.copied_to_clipboard {
                    println!("Text copied to clipboard");
                }
            }
            Err(e) => {
                eprintln!("OCR failed: {}", e);
                std::process::exit(1);
            }
        }
    }

    // Spawn the preview as a subprocess to avoid GTK conflicts
    // (the area selector already used GTK in this process).
    let binary = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("cleanshitx"));

    if let Err(e) = std::process::Command::new(&binary)
        .arg("preview")
        .arg(&saved_path)
        .spawn()
    {
        eprintln!("Warning: Failed to spawn preview overlay: {}", e);
        // Fall back to direct call
        if let Err(e) = show_capture_preview_overlay(saved_path.clone()) {
            eprintln!("Warning: Failed to show capture preview overlay: {}", e);
        }
    }
}


/// Save a CaptureData as a temp PNG for passing to the C++ overlay as background.
/// Returns the path if successful, None on failure (overlay will run without background).
fn save_temp_png(capture: &CaptureData) -> Option<std::path::PathBuf> {
    use image::{ImageBuffer, Rgba};

    let tmp = std::env::temp_dir().join(format!(
        "cleanshitx_bg_{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));

    let bytes_per_pixel = capture.format.bytes_per_pixel as usize;
    let stride = capture.stride as usize;
    let w = capture.width;
    let h = capture.height;

    use cleanshitx::backend::PixelFormat;
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

fn crop_capture_data(
    capture: &CaptureData,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<CaptureData, String> {
    if width <= 0 || height <= 0 || x < 0 || y < 0 {
        return Err(format!("Invalid dimensions: {}x{}", width, height));
    }

    let x_end = x
        .checked_add(width)
        .ok_or_else(|| "Area width overflow".to_string())?;
    let y_end = y
        .checked_add(height)
        .ok_or_else(|| "Area height overflow".to_string())?;

    if x_end as u32 > capture.width || y_end as u32 > capture.height {
        return Err(format!(
            "Requested area ({x}, {y}, {width}, {height}) is out of bounds for {}x{} capture",
            capture.width, capture.height
        ));
    }

    let width_u32 = width as u32;
    let height_u32 = height as u32;
    let bytes_per_pixel = capture.format.bytes_per_pixel as usize;
    let source_stride = capture.stride as usize;
    let row_len = width_u32 as usize * bytes_per_pixel;

    let mut cropped = Vec::with_capacity(row_len * height_u32 as usize);
    for row in 0..height_u32 as usize {
        let src_y = y as usize + row;
        let src_offset = src_y * source_stride + x as usize * bytes_per_pixel;
        let src_end = src_offset + row_len;
        cropped.extend_from_slice(&capture.pixels[src_offset..src_end]);
    }

    let cursor = capture.cursor.clone().and_then(|mut cursor| {
        let in_x = cursor.x >= x && cursor.x < x + width;
        let in_y = cursor.y >= y && cursor.y < y + height;
        if in_x && in_y {
            cursor.x -= x;
            cursor.y -= y;
            Some(cursor)
        } else {
            None
        }
    });

    Ok(CaptureData::with_cursor(
        cropped,
        width_u32,
        height_u32,
        capture.format,
        cursor,
    ))
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
            println!("OCR successful!");
            println!("Confidence: {}%", result.confidence);
            println!("Extracted text:");
            println!("{}", "-".repeat(40));
            println!("{}", result.text);
            println!("{}", "-".repeat(40));
            if result.copied_to_clipboard {
                println!("Text copied to clipboard");
            }
        }
        Err(e) => {
            eprintln!("OCR failed: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_edit(args: &[String]) {
    let image_path = PathBuf::from(&args[2]);
    if let Err(e) = open_image_editor(image_path) {
        eprintln!("Editor failed: {e}");
        std::process::exit(1);
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

            let selection = run_capture_overlay(None).map_err(|e| format!("Selection failed: {}", e))?;
            if let Some(area) = selection {
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
    } else if record_type != "screen" {
        eprintln!(
            "Error: recording type '{}' not supported (use 'screen' or 'area')",
            record_type
        );
        std::process::exit(1);
    }

    let final_path = if overlay_stop {
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let join = tokio::spawn(async move { start_recording_with_stop(config, stop_rx).await });

        // Blocks until user hits Esc or clicks Stop.
        run_recording_stop_overlay(stop_tx)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        join.await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)??
    } else {
        start_recording(config)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?
    };

    // Post-processing
    if let Some(ext) = final_path.extension() {
        if ext == "gif" {
            // For GIFs, we default to copying to clipboard (feature requested)
            if let Err(e) = copy_recording_to_clipboard(&final_path) {
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
        let imported = import_web_scroll_capture(png_base64, page_url, page_title).await;

        if imported {
            let _ = write_native_host_response(&NativeHostResponse {
                ok: true,
                message: "Imported web scroll capture".into(),
            });
        } else {
            let _ = write_native_host_response(&NativeHostResponse {
                ok: false,
                message: "Daemon not available or import failed".into(),
            });
        }
    }
}
