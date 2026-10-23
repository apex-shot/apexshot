//! Recording constants shared with the settings pages.
//!
//! The Windows recorder itself lives in the C++ shell; the settings window only
//! needs the option counts and the availability answer.

pub const VIDEO_MAX_RES_OPTION_COUNT: usize = 7;
pub const VIDEO_FPS_OPTION_COUNT: usize = 4;

pub const FEDORA_RECORDING_UNSUPPORTED_MSG: &str =
    "Video recording is not supported here. Screenshots still work.";

pub mod editor {
    pub mod ffmpeg {
        use std::path::{Path, PathBuf};
        use std::process::Command;

        pub fn thumbnail_cache_dir(source: &Path) -> PathBuf {
            let mut hash = 1469598103934665603_u64;
            for byte in source.to_string_lossy().as_bytes() {
                hash ^= *byte as u64;
                hash = hash.wrapping_mul(1099511628211);
            }
            let mut directory = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
            directory.push("apexshot");
            directory.push("video-thumbnails");
            directory.push(format!("{hash:x}"));
            directory
        }

        pub fn extract_poster_frame(
            source: &Path,
            output: &Path,
            seconds: f64,
        ) -> Result<(), String> {
            let Some(parent) = output.parent() else {
                return Err("thumbnail output has no parent directory".to_string());
            };
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not create thumbnail cache: {error}"))?;

            let output = Command::new(ffmpeg_executable())
                .args(["-hide_banner", "-loglevel", "error", "-y"])
                .args(["-ss", &seconds.to_string(), "-i"])
                .arg(source)
                .args(["-frames:v", "1"])
                .arg(output)
                .output()
                .map_err(|error| format!("could not start ffmpeg: {error}"))?;
            if !output.status.success() {
                let detail = String::from_utf8_lossy(&output.stderr);
                return Err(format!(
                    "ffmpeg could not extract a poster: {}",
                    detail.trim()
                ));
            }
            Ok(())
        }

        fn ffmpeg_executable() -> PathBuf {
            if let Some(path) = std::env::var_os("APEXSHOT_FFMPEG") {
                let path = PathBuf::from(path);
                if path.is_file() {
                    return path;
                }
            }
            if let Ok(executable) = std::env::current_exe() {
                if let Some(directory) = executable.parent() {
                    for candidate in [
                        directory.join("ffmpeg.exe"),
                        directory.join("bin/ffmpeg.exe"),
                    ] {
                        if candidate.is_file() {
                            return candidate;
                        }
                    }
                }
            }
            PathBuf::from("ffmpeg.exe")
        }
    }
}

pub fn recording_output_dir(config: &crate::config::AppConfig) -> std::path::PathBuf {
    let configured = config.video_export_location.trim();
    if !configured.is_empty() {
        return std::path::PathBuf::from(configured);
    }
    dirs::video_dir()
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(std::path::PathBuf::from)
                .map(|home| home.join("Videos"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

pub fn recording_supported() -> bool {
    true
}
