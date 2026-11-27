//! The local llama.cpp server: one per user, started on demand.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::catalog::{ai_dir, model_path, model_spec, runtime_exe, ModelTier};

const HEALTH_POLL: Duration = Duration::from_millis(100);
const HEALTH_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Serialize, Deserialize)]
struct ServerInfo {
    pid: u32,
    port: u16,
    model: String,
}

pub struct Namer {
    pub base_url: String,
    pub deadline: Instant,
}

impl apexshot_organizer::ai::AiNamer for Namer {
    fn complete(&self, system: &str, user: &str) -> Option<String> {
        complete(&self.base_url, system, user, self.deadline)
    }
}

/// The base URL of a running server for `tier`, starting one when needed and
/// waiting until `deadline` for it to answer. None if it cannot be ready in time.
pub fn ensure_server(tier: ModelTier, deadline: Instant) -> Option<String> {
    let info = read_server();
    if let Some(info) = &info {
        let base = base_url(info.port);
        if info.model == model_spec(tier).file_name && healthy(&base, deadline) {
            return Some(base);
        }
    }
    if info.is_some() {
        stop_server();
    }

    let exe = runtime_exe()?;
    let model = model_path(tier)?;
    let port = free_port()?;
    let threads = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(8);
    let port_arg = port.to_string();
    let threads_arg = threads.to_string();
    let mut command = Command::new(&exe);
    command
        .arg("-m")
        .arg(&model)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(&port_arg)
        .arg("-c")
        .arg("2048")
        .arg("-t")
        .arg(&threads_arg)
        .arg("--reasoning")
        .arg("off")
        .arg("--sleep-idle-seconds")
        .arg("300")
        .arg("--no-webui")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let child = command.spawn().ok()?;
    let pid = child.id();
    drop(child);

    write_server(&ServerInfo {
        pid,
        port,
        model: model_spec(tier).file_name.to_string(),
    });

    let base = base_url(port);
    loop {
        if Instant::now() >= deadline {
            return None;
        }
        if healthy(&base, deadline) {
            return Some(base);
        }
        let pause = deadline
            .saturating_duration_since(Instant::now())
            .min(HEALTH_POLL);
        if pause.is_zero() {
            return None;
        }
        std::thread::sleep(pause);
    }
}

pub fn stop_server() {
    let Some(path) = server_path() else {
        return;
    };
    if let Ok(raw) = fs::read_to_string(&path) {
        if let Ok(info) = serde_json::from_str::<ServerInfo>(&raw) {
            kill(info.pid);
        }
    }
    let _ = fs::remove_file(path);
}

pub fn complete(base_url: &str, system: &str, user: &str, deadline: Instant) -> Option<String> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return None;
    }
    let body = serde_json::json!({
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "max_tokens": 32,
        "temperature": 0.0,
        "chat_template_kwargs": {"enable_thinking": false},
    });
    let response = ureq::post(&format!("{base_url}/v1/chat/completions"))
        .timeout(left)
        .send_json(body)
        .ok()?;
    let reply: serde_json::Value = response.into_json().ok()?;
    reply
        .get("choices")?
        .get(0)?
        .get("message")?
        .get("content")?
        .as_str()
        .map(|content| content.to_string())
}

/// Time one naming request end to end (starting the server if needed), for the
/// speed check.
pub fn benchmark(tier: ModelTier) -> Option<Duration> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let base = ensure_server(tier, deadline)?;
    let started = Instant::now();
    complete(
        &base,
        apexshot_organizer::ai::SYSTEM_PROMPT,
        BENCHMARK_PROMPT,
        deadline,
    )?;
    Some(started.elapsed())
}

const BENCHMARK_PROMPT: &str = "App: Chrome\nSite: x.com\nWindow title: Home / X\n\
Current name: Home X\nText on screen, top to bottom:\nSam Lee @samlee\n\
Our new split keyboard ships next month\nPreorders open Friday for early testers\n\
Show more\nWhat's happening";

fn healthy(base: &str, deadline: Instant) -> bool {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return false;
    }
    let timeout = left.min(HEALTH_TIMEOUT);
    match ureq::get(&format!("{base}/health")).timeout(timeout).call() {
        Ok(response) => response.status() == 200,
        Err(_) => false,
    }
}

fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn free_port() -> Option<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    listener.local_addr().ok().map(|address| address.port())
}

fn server_path() -> Option<PathBuf> {
    ai_dir().map(|dir| dir.join("server.json"))
}

fn read_server() -> Option<ServerInfo> {
    let path = server_path()?;
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_server(info: &ServerInfo) {
    let Some(path) = server_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string(info) {
        let _ = fs::write(path, raw);
    }
}

#[cfg(target_os = "windows")]
fn kill(pid: u32) {
    use std::os::windows::process::CommandExt;

    let pid_arg = pid.to_string();
    let _ = Command::new("taskkill")
        .args(["/PID", pid_arg.as_str(), "/F"])
        .creation_flags(0x0800_0000)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(target_os = "windows"))]
fn kill(_pid: u32) {}
