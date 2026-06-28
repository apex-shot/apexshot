use serde::Deserialize;
use std::time::Duration;

use crate::config::{load_config, save_config, AppConfig};

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
            LoginError::NotConfigured => write!(f, "Cloud backend URL not set. Configure it in Settings first."),
            LoginError::HttpRequest(msg) => write!(f, "Request failed: {msg}"),
            LoginError::Expired => write!(f, "Device code expired. Run `apexshot login` again."),
            LoginError::Denied => write!(f, "Authorization was denied."),
            LoginError::Server(msg) => write!(f, "Server error: {msg}"),
        }
    }
}

impl std::error::Error for LoginError {}

pub fn needs_backend_url(config: &AppConfig) -> bool {
    config.cloud_backend_url.is_empty()
}

pub fn login() -> Result<(), LoginError> {
    let mut config = load_config();
    if needs_backend_url(&config) {
        return Err(LoginError::NotConfigured);
    }

    let backend_url = config.cloud_backend_url.trim_end_matches('/');
    let was_logged_in = !config.cloud_api_token.is_empty();
    let previous_email = config.cloud_user_email.clone();

    let device_body = serde_json::json!({ "client_id": "apexshot-desktop" }).to_string();
    let device_resp: DeviceCodeResponse = ureq::post(&format!("{backend_url}/v1/auth/device"))
        .set("Content-Type", "application/json")
        .send_string(&device_body)
        .map_err(|e| LoginError::HttpRequest(e.to_string()))?
        .into_json()
        .map_err(|e| LoginError::Server(format!("Invalid response: {e}")))?;

    let user_code = format_user_code(&device_resp.user_code);
    println!("First copy your one-time code: {user_code}");
    println!("Press Enter to open {} in your browser...", device_resp.verification_uri);

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

                let account: AccountResponse = ureq::get(&format!("{backend_url}/v1/account"))
                    .set("Authorization", &format!("Bearer {}", config.cloud_api_token))
                    .call()
                    .map_err(|e| LoginError::HttpRequest(e.to_string()))?
                    .into_json()
                    .map_err(|e| LoginError::Server(format!("Invalid account response: {e}")))?;

                config.cloud_user_email = account.email.clone();
                save_config(&config).map_err(|e| {
                    LoginError::Server(format!("Failed to save config: {e}"))
                })?;

                println!("\n✓ Authentication complete.");
                println!("✓ Logged in as {}", config.cloud_user_email);
                if was_logged_in && config.cloud_user_email == previous_email {
                    println!("! You were already logged in to this account");
                }
                return Ok(());
            }
            Err(ureq::Error::Status(400, resp)) => {
                let body: serde_json::Value = resp
                    .into_json()
                    .unwrap_or(serde_json::Value::Null);
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

fn format_user_code(code: &str) -> String {
    let chars: Vec<char> = code.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() <= 4 {
        return chars.into_iter().collect();
    }
    let mid = chars.len() / 2;
    let (a, b) = chars.split_at(mid);
    format!("{}-{}", a.iter().collect::<String>(), b.iter().collect::<String>())
}

fn open_browser(url: &str) -> Result<(), String> {
    let commands = ["xdg-open", "gio", "sensible-browser"];
    for cmd in &commands {
        if std::process::Command::new(cmd)
            .arg(url)
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }
    Err("No browser launcher found".to_string())
}
