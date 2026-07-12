use serde::Deserialize;
use std::time::Duration;

use crate::config::{
    is_cloud_logged_in, load_config, resolve_cloud_backend_url, save_config, AppConfig,
};

const POLL_INTERVAL: u64 = 5;
const MAX_POLL_SECONDS: u64 = 900;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: i32,
    interval: i32,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i32,
    device_id: String,
}

#[derive(Debug, Deserialize)]
struct AccountResponse {
    email: String,
    #[allow(dead_code)]
    tier: String,
}

#[derive(Debug)]
pub enum LoginError {
    NotConfigured,
    HttpRequest(String),
    Expired,
    Denied,
    Server(String),
}

impl std::fmt::Display for LoginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoginError::NotConfigured => write!(
                f,
                "Cloud backend URL not set. Configure it in Settings first."
            ),
            LoginError::HttpRequest(msg) => write!(f, "Request failed: {msg}"),
            LoginError::Expired => write!(f, "Device code expired. Run `apexshot login` again."),
            LoginError::Denied => write!(f, "Authorization was denied."),
            LoginError::Server(msg) => write!(f, "Server error: {msg}"),
        }
    }
}

impl std::error::Error for LoginError {}

#[derive(Debug)]
pub enum LogoutError {
    NotLoggedIn,
    HttpRequest(String),
    Server(String),
}

impl std::fmt::Display for LogoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogoutError::NotLoggedIn => write!(f, "You are not logged in."),
            LogoutError::HttpRequest(msg) => write!(f, "Logout request failed: {msg}"),
            LogoutError::Server(msg) => write!(f, "Server error: {msg}"),
        }
    }
}

impl std::error::Error for LogoutError {}

pub fn needs_backend_url(config: &AppConfig) -> bool {
    // Public installs always resolve to DEFAULT_CLOUD_BACKEND_URL when empty.
    resolve_cloud_backend_url(config).is_empty()
}

pub fn login() -> Result<(), LoginError> {
    let mut config = load_config();
    if needs_backend_url(&config) {
        return Err(LoginError::NotConfigured);
    }

    // Ensure config carries the resolved URL so later upload/login paths match.
    let backend_url = resolve_cloud_backend_url(&config);
    if config.cloud_backend_url.trim().is_empty() {
        config.cloud_backend_url = backend_url.clone();
        let _ = save_config(&config);
    }

    let was_logged_in = is_cloud_logged_in(&config);
    let previous_email = config.cloud_user_email.clone();

    if config.cloud_install_id.is_empty() {
        config.cloud_install_id = generate_install_id();
        let _ = save_config(&config);
    }

    let device_body = serde_json::json!({
        "client_id": "apexshot-desktop",
        "device_name": device_name(),
        "install_id": config.cloud_install_id,
    })
    .to_string();
    let device_resp: DeviceCodeResponse = ureq::post(&format!("{backend_url}/v1/auth/device"))
        .set("Content-Type", "application/json")
        .send_string(&device_body)
        .map_err(|e| LoginError::HttpRequest(e.to_string()))?
        .into_json()
        .map_err(|e| LoginError::Server(format!("Invalid response: {e}")))?;

    let user_code = format_user_code(&device_resp.user_code);
    println!("First copy your one-time code: {user_code}");
    println!(
        "Press Enter to open {} in your browser...",
        device_resp.verification_uri
    );

    let mut _input = String::new();
    let _ = std::io::stdin().read_line(&mut _input);

    let _ = open_browser(&device_resp.verification_uri);

    let poll_body = serde_json::json!({
        "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        "device_code": device_resp.device_code,
    })
    .to_string();

    let interval = device_resp.interval.max(1) as u64;
    let elapsed = std::time::Instant::now();

    loop {
        std::thread::sleep(Duration::from_secs(interval.max(POLL_INTERVAL)));

        if elapsed.elapsed().as_secs() > MAX_POLL_SECONDS {
            return Err(LoginError::Expired);
        }

        match ureq::post(&format!("{backend_url}/v1/auth/token"))
            .set("Content-Type", "application/json")
            .send_string(&poll_body)
        {
            Ok(resp) => {
                let token: TokenResponse = resp
                    .into_json()
                    .map_err(|e| LoginError::Server(format!("Invalid token response: {e}")))?;

                config.cloud_api_token = token.access_token;
                config.cloud_refresh_token = token.refresh_token;

                let account: AccountResponse = ureq::get(&format!("{backend_url}/v1/account"))
                    .set(
                        "Authorization",
                        &format!("Bearer {}", config.cloud_api_token),
                    )
                    .call()
                    .map_err(|e| LoginError::HttpRequest(e.to_string()))?
                    .into_json()
                    .map_err(|e| LoginError::Server(format!("Invalid account response: {e}")))?;

                config.cloud_user_email = account.email.clone();
                save_config(&config)
                    .map_err(|e| LoginError::Server(format!("Failed to save config: {e}")))?;

                println!("\n✓ Authentication complete.");
                println!("✓ Logged in as {}", config.cloud_user_email);
                if was_logged_in && config.cloud_user_email == previous_email {
                    println!("! You were already logged in to this account");
                }
                return Ok(());
            }
            Err(ureq::Error::Status(400, resp)) => {
                let body: serde_json::Value = resp.into_json().unwrap_or(serde_json::Value::Null);
                let error = body["error"].as_str().unwrap_or("");
                if error.contains("pending") {
                    continue;
                }
                if error.contains("expired") {
                    return Err(LoginError::Expired);
                }
                return Err(LoginError::Server(error.to_string()));
            }
            Err(e) => {
                return Err(LoginError::HttpRequest(e.to_string()));
            }
        }
    }
}

pub fn logout() -> Result<(), LogoutError> {
    let mut config = load_config();

    if config.cloud_api_token.is_empty() {
        return Err(LogoutError::NotLoggedIn);
    }

    let backend_url = resolve_cloud_backend_url(&config);
    let revoke_body =
        serde_json::json!({ "token": config.cloud_api_token, "token_type_hint": "access_token" })
            .to_string();

    let revoke_result = ureq::post(&format!("{backend_url}/v1/auth/revoke"))
        .set("Content-Type", "application/json")
        .send_string(&revoke_body);

    match revoke_result {
        Ok(_) => {}
        Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) => {
            // Token may already be expired/invalid — proceed with local cleanup.
        }
        Err(e) => return Err(LogoutError::HttpRequest(e.to_string())),
    }

    config.cloud_api_token.clear();
    config.cloud_refresh_token.clear();
    config.cloud_user_name.clear();
    config.cloud_user_email.clear();
    config.cloud_pro_plan = false;

    save_config(&config).map_err(|e| LogoutError::Server(format!("Failed to save config: {e}")))?;

    println!("✓ Logged out.");
    Ok(())
}

fn format_user_code(code: &str) -> String {
    let chars: Vec<char> = code.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() <= 4 {
        return chars.into_iter().collect();
    }
    let mid = chars.len() / 2;
    let (a, b) = chars.split_at(mid);
    format!(
        "{}-{}",
        a.iter().collect::<String>(),
        b.iter().collect::<String>()
    )
}

fn open_browser(url: &str) -> Result<(), String> {
    let commands = ["xdg-open", "gio", "sensible-browser"];
    for cmd in &commands {
        if std::process::Command::new(cmd).arg(url).spawn().is_ok() {
            return Ok(());
        }
    }
    Err("No browser launcher found".to_string())
}

fn hostname() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}

fn device_name() -> String {
    const MAX_LEN: usize = 100;
    const SUFFIX: &str = " (Linux)";
    match hostname() {
        Some(host) => {
            let max_host = MAX_LEN - SUFFIX.len();
            let host = if host.chars().count() > max_host {
                host.chars().take(max_host).collect::<String>()
            } else {
                host
            };
            format!("{host}{SUFFIX}")
        }
        None => "ApexShot CLI (Linux)".to_string(),
    }
}

fn generate_install_id() -> String {
    use std::io::Read;
    let mut buf = [0u8; 16];
    let ok = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .is_ok();
    if ok {
        buf[6] = (buf[6] & 0x0f) | 0x40;
        buf[8] = (buf[8] & 0x3f) | 0x80;
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
            buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]
        )
    } else {
        format!("install-{}", chrono::Utc::now().timestamp())
    }
}
