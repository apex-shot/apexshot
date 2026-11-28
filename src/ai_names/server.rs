//! The local llama.cpp server: one per user, started on demand.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::catalog::{ai_dir, model_path, model_spec, runtime_dir, runtime_exe, ModelTier};

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
    let threads = inference_threads();
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
    // The archive ships its shared libraries beside the binary, and the
    // build's own RUNPATH is relative to where it was unpacked.
    if let Some(dir) = runtime_dir() {
        let mut search = dir.into_os_string();
        if let Some(existing) = std::env::var_os("LD_LIBRARY_PATH") {
            if !existing.is_empty() {
                search.push(":");
                search.push(existing);
            }
        }
        command.env("LD_LIBRARY_PATH", search);
    }
    lower_priority(&mut command);
    let mut child = command.spawn().ok()?;
    let pid = child.id();
    // Reap the server when it exits so it never lingers as a zombie under a
    // long-lived parent such as the daemon or Settings.
    std::thread::spawn(move || {
        let _ = child.wait();
    });

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
            warm_up(&base, deadline);
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

/// Read the naming instructions once as soon as the server is up. The server
/// keeps them cached, so the first real name only reads its own text instead
/// of paying for the whole instruction block.
fn warm_up(base: &str, deadline: Instant) {
    let _ = complete(
        base,
        apexshot_organizer::ai::SYSTEM_PROMPT,
        "Window title: ApexShot",
        deadline,
    );
}

/// How many threads the model gets. On a CPU with performance and efficiency
/// cores, a thread on an efficiency core holds back every step, so only the
/// performance cores are counted; otherwise one per physical core. Capped at 8.
fn inference_threads() -> usize {
    let logical = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let cores = performance_core_count().unwrap_or(logical);
    cores.clamp(1, 8)
}

/// Physical performance cores on a hybrid CPU, or physical cores otherwise.
/// None when the topology cannot be read.
fn performance_core_count() -> Option<usize> {
    let logical: Vec<usize> = match std::fs::read_to_string("/sys/devices/cpu_core/cpus") {
        Ok(list) => parse_cpu_list(&list),
        Err(_) => {
            let online = std::fs::read_to_string("/sys/devices/system/cpu/online").ok()?;
            parse_cpu_list(&online)
        }
    };
    if logical.is_empty() {
        return None;
    }
    let mut cores = std::collections::HashSet::new();
    for cpu in &logical {
        let base = format!("/sys/devices/system/cpu/cpu{cpu}/topology");
        let package = std::fs::read_to_string(format!("{base}/physical_package_id")).ok()?;
        let core = std::fs::read_to_string(format!("{base}/core_id")).ok()?;
        cores.insert((package.trim().to_string(), core.trim().to_string()));
    }
    Some(cores.len())
}

/// Parse a kernel CPU list such as "0-11" or "0,2,4-7".
fn parse_cpu_list(list: &str) -> Vec<usize> {
    let mut cpus = Vec::new();
    for part in list.trim().split(',').filter(|part| !part.is_empty()) {
        match part.split_once('-') {
            Some((start, end)) => {
                if let (Ok(start), Ok(end)) = (start.parse::<usize>(), end.parse::<usize>()) {
                    cpus.extend(start..=end);
                }
            }
            None => {
                if let Ok(cpu) = part.parse() {
                    cpus.push(cpu);
                }
            }
        }
    }
    cpus
}

/// Naming must never make the desktop feel slow, so the model runs below the
/// rest of the session.
fn lower_priority(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        // SAFETY: setpriority is async-signal-safe and takes no locks, which
        // is what a pre_exec closure may do.
        unsafe {
            command.pre_exec(|| {
                libc::setpriority(libc::PRIO_PROCESS, 0, 10);
                Ok(())
            });
        }
    }
    #[cfg(not(unix))]
    {
        let _ = command;
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

fn kill(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: kill only reads its arguments. A pid that already exited
        // simply fails, with nothing left to clean up.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cpu_lists_parse_ranges_and_singles() {
        assert_eq!(super::parse_cpu_list("0-3\n"), vec![0, 1, 2, 3]);
        assert_eq!(super::parse_cpu_list("0,2,4-5"), vec![0, 2, 4, 5]);
        assert!(super::parse_cpu_list("").is_empty());
    }

    use super::*;

    #[test]
    fn the_server_url_is_loopback_only() {
        assert_eq!(base_url(8080), "http://127.0.0.1:8080");
    }

    #[test]
    fn a_free_port_is_offered() {
        assert!(free_port().is_some_and(|port| port != 0));
    }
}
