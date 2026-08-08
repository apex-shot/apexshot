use apexshot::{
    app_identity,
    backend::CaptureData,
    capture::{save_capture, SaveConfig},
    daemon::import_web_scroll_capture,
    preview_launch::launch_preview,
};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub(crate) enum BrowserTarget {
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

pub(crate) fn user_config_dir() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
    Ok(PathBuf::from(home).join(".config"))
}

pub(crate) fn native_manifest_paths(target: BrowserTarget) -> Result<Vec<PathBuf>, String> {
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

pub(crate) fn validate_extension_id(extension_id: &str) -> Result<(), String> {
    if extension_id.len() != 32 {
        return Err("extension id must be 32 characters".into());
    }
    if !extension_id.chars().all(|c| matches!(c, 'a'..='p')) {
        return Err("extension id must contain only letters a-p".into());
    }
    Ok(())
}

pub(crate) fn install_native_host_manifest(
    extension_id: &str,
    browser: BrowserTarget,
) -> Result<(), String> {
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

pub(crate) fn uninstall_native_host_manifest(browser: BrowserTarget) -> Result<(), String> {
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

pub(crate) fn run_native_host_command(args: &[String]) {
    if app_identity::portal_only() {
        eprintln!(
            "Error: browser native-host install is not available in Flatpak/portal-only builds."
        );
        std::process::exit(1);
    }
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

#[derive(Debug, Deserialize)]
pub(crate) struct NativeHostRequest {
    cmd: String,
    png_data_url: Option<String>,
    page_url: Option<String>,
    page_title: Option<String>,
    extension_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct NativeHostResponse {
    ok: bool,
    message: String,
}

pub(crate) fn write_native_host_response(resp: &NativeHostResponse) -> Result<(), String> {
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

pub(crate) fn extract_png_base64(data_url: &str) -> Result<String, String> {
    let prefix = "data:image/png;base64,";
    if let Some(rest) = data_url.strip_prefix(prefix) {
        if rest.is_empty() {
            return Err("Empty PNG payload".into());
        }
        return Ok(rest.to_string());
    }
    Err("png_data_url must be a data:image/png;base64 URL".into())
}

pub(crate) fn import_web_scroll_capture_direct(
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

pub(crate) async fn run_native_host() -> Result<(), String> {
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
