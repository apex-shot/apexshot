//! Downloads, verifies, and unpacks the model and the llama.cpp runtime.

use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::catalog::{model_path, model_spec, runtime_dir, runtime_exe, runtime_spec, ModelTier};
use super::server;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallProgress {
    Downloading { done: u64, total: u64 },
    Verifying,
    Extracting,
    Done,
}

/// Download the runtime (if missing) and the model for `tier`, verifying both.
/// Blocking.
pub fn install(
    tier: ModelTier,
    progress: &dyn Fn(InstallProgress),
    cancel: &AtomicBool,
) -> Result<(), String> {
    let model = model_path(tier).ok_or_else(|| "no data folder".to_string())?;
    let exe = runtime_exe().ok_or_else(|| "no data folder".to_string())?;
    let runtime_missing = !exe.is_file();
    let model_missing = !model.is_file();
    let runtime_bytes = if runtime_missing {
        runtime_spec().bytes
    } else {
        0
    };
    let model_bytes = if model_missing {
        model_spec(tier).bytes
    } else {
        0
    };
    let grand_total = runtime_bytes + model_bytes;
    let mut base_done = 0;

    if runtime_missing {
        let dir = runtime_dir().ok_or_else(|| "no data folder".to_string())?;
        let parent = dir.parent().ok_or_else(|| "no data folder".to_string())?;
        let zip = parent.join("llama.zip");
        download(
            runtime_spec().url,
            &zip,
            runtime_spec().sha256,
            progress,
            cancel,
            base_done,
            grand_total,
        )?;
        base_done += runtime_spec().bytes;
        progress(InstallProgress::Extracting);
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        extract(&zip, &dir)?;
        let _ = fs::remove_file(&zip);
        if !exe.is_file() {
            lift_runtime(&dir)?;
        }
        if !exe.is_file() {
            return Err("the runtime did not unpack".to_string());
        }
    }

    if model_missing {
        download(
            model_spec(tier).url,
            &model,
            model_spec(tier).sha256,
            progress,
            cancel,
            base_done,
            grand_total,
        )?;
    }

    progress(InstallProgress::Done);
    Ok(())
}

/// Stop the server and delete the model file for `tier`.
pub fn remove(tier: ModelTier) -> Result<(), String> {
    server::stop_server();
    if let Some(path) = model_path(tier) {
        if path.is_file() {
            fs::remove_file(&path).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn download(
    url: &str,
    dest: &Path,
    sha256: &str,
    progress: &dyn Fn(InstallProgress),
    cancel: &AtomicBool,
    base_done: u64,
    grand_total: u64,
) -> Result<(), String> {
    let parent = dest
        .parent()
        .ok_or_else(|| "no download folder".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;

    let mut partial_name = OsString::from(dest.as_os_str());
    partial_name.push(".part");
    let partial = PathBuf::from(partial_name);

    let response = ureq::get(url)
        .timeout(Duration::from_secs(60 * 60))
        .call()
        .map_err(|error| error.to_string())?;
    let total = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok())
        .map(|length| base_done + length)
        .unwrap_or(grand_total);

    let mut reader = response.into_reader();
    let mut file = fs::File::create(&partial).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut written = 0u64;
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) => {
                let _ = fs::remove_file(&partial);
                return Err(error.to_string());
            }
        };
        hasher.update(&buffer[..read]);
        if let Err(error) = file.write_all(&buffer[..read]) {
            let _ = fs::remove_file(&partial);
            return Err(error.to_string());
        }
        written += read as u64;
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = fs::remove_file(&partial);
            return Err("cancelled".to_string());
        }
        progress(InstallProgress::Downloading {
            done: base_done + written,
            total,
        });
    }
    drop(file);

    progress(InstallProgress::Verifying);
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    if hex != sha256.trim().to_lowercase() {
        let _ = fs::remove_file(&partial);
        return Err("download was damaged; try again".to_string());
    }
    fs::rename(&partial, dest).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn extract(zip: &Path, dir: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let status = std::process::Command::new(r"C:\Windows\System32\tar.exe")
        .arg("-xf")
        .arg(zip)
        .arg("-C")
        .arg(dir)
        .creation_flags(0x0800_0000)
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("the runtime could not be unpacked ({status})"))
    }
}

#[cfg(not(target_os = "windows"))]
fn extract(_zip: &Path, _dir: &Path) -> Result<(), String> {
    Err("the AI runtime is only available on Windows".to_string())
}

/// Some archives nest the executable one folder down; lift that folder's files.
#[cfg(target_os = "windows")]
fn lift_runtime(dir: &Path) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|error| error.to_string())? {
        let folder = entry.map_err(|error| error.to_string())?.path();
        if !folder.is_dir() || !folder.join("llama-server.exe").is_file() {
            continue;
        }
        for file in fs::read_dir(&folder).map_err(|error| error.to_string())? {
            let file = file.map_err(|error| error.to_string())?;
            if file.path().is_file() {
                fs::rename(file.path(), dir.join(file.file_name()))
                    .map_err(|error| error.to_string())?;
            }
        }
        return Ok(());
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn lift_runtime(_dir: &Path) -> Result<(), String> {
    Ok(())
}
