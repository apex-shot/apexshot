//! Recording constants shared with the settings pages.
//!
//! The Windows recorder itself lives in the C++ shell; the settings window only
//! needs the option counts and the availability answer.

pub const VIDEO_MAX_RES_OPTION_COUNT: usize = 7;
pub const VIDEO_FPS_OPTION_COUNT: usize = 4;

/// x264 CRF for a Settings quality tier, on the same 23 / 20 / 16 scale the
/// recorder uses, so an export is never sharper than its recording tier.
pub fn crf_for_quality(tier: u8) -> u32 {
    match tier {
        0 => 23, // Balanced
        2 => 16, // Ultra
        _ => 20, // High (default)
    }
}

pub const FEDORA_RECORDING_UNSUPPORTED_MSG: &str =
    "Video recording is not supported here. Screenshots still work.";

pub mod editor {
    /// Click-effect geometry, shared with the Linux editor.
    #[path = "../../../../src/recording/editor/click_effect.rs"]
    #[allow(dead_code)]
    #[rustfmt::skip]
    pub mod click_effect;

    /// The cursor model the editor's cursor panel speaks.
    #[path = "../../../../src/recording/editor/model_parts/cursor_types.rs"]
    #[rustfmt::skip]
    pub mod model;

    /// The cursor sprites and previews the Linux editor draws.
    #[path = "../../../../src/recording/editor/cursor_sprite.rs"]
    #[rustfmt::skip]
    pub mod cursor_sprite;

    pub mod ffmpeg {
        use std::path::{Path, PathBuf};
        use std::process::Command;

        /// How many frames the timeline filmstrip samples. Fixed, as in the
        /// source editor: the strip stretches its tiles to the clip width.
        pub const FILMSTRIP_FRAMES: usize = 12;

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

        /// Read the container duration without decoding the stream.
        pub fn probe_duration(source: &Path) -> Result<f64, String> {
            let output = Command::new(ffmpeg_executable())
                .args(["-hide_banner", "-i"])
                .arg(source)
                .output()
                .map_err(|error| format!("could not start ffmpeg: {error}"))?;
            let stderr = String::from_utf8_lossy(&output.stderr);
            parse_duration(&stderr).ok_or_else(|| "ffmpeg did not report a duration".to_string())
        }

        fn parse_duration(stderr: &str) -> Option<f64> {
            let marker = stderr.find("Duration: ")? + "Duration: ".len();
            let stamp = stderr.get(marker..marker + 11)?;
            let mut parts = stamp.split([':', '.']);
            let hours: f64 = parts.next()?.parse().ok()?;
            let minutes: f64 = parts.next()?.parse().ok()?;
            let seconds: f64 = parts.next()?.parse().ok()?;
            let fraction: f64 = parts.next()?.parse().ok()?;
            let duration = hours * 3600.0 + minutes * 60.0 + seconds + fraction / 100.0;
            duration.is_finite().then_some(duration)
        }

        /// Extract the filmstrip frames used by the timeline clip.
        pub fn generate_filmstrip(
            source: &Path,
            output_dir: &Path,
            duration: f64,
        ) -> Result<Vec<PathBuf>, String> {
            std::fs::create_dir_all(output_dir)
                .map_err(|error| format!("could not create the filmstrip cache: {error}"))?;
            for entry in std::fs::read_dir(output_dir)
                .map_err(|error| format!("could not read the filmstrip cache: {error}"))?
                .flatten()
            {
                let _ = std::fs::remove_file(entry.path());
            }

            let rate = FILMSTRIP_FRAMES as f64 / duration.max(0.1);
            let pattern = output_dir.join("frame-%02d.png");
            let filter = format!("fps={rate:.6},scale=260:-2");
            let output = Command::new(ffmpeg_executable())
                .args(["-hide_banner", "-loglevel", "error", "-y"])
                .arg("-i")
                .arg(source)
                .args(["-vf", &filter])
                .args(["-frames:v", &FILMSTRIP_FRAMES.to_string()])
                .arg(&pattern)
                .output()
                .map_err(|error| format!("could not start ffmpeg: {error}"))?;
            if !output.status.success() {
                let detail = String::from_utf8_lossy(&output.stderr);
                return Err(format!(
                    "ffmpeg could not build the filmstrip: {}",
                    detail.trim()
                ));
            }

            let mut frames: Vec<PathBuf> = std::fs::read_dir(output_dir)
                .map_err(|error| format!("could not read the filmstrip cache: {error}"))?
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
                .collect();
            frames.sort();
            if frames.is_empty() {
                return Err("ffmpeg produced no filmstrip frames".to_string());
            }
            Ok(frames)
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

        /// The preview decodes raw RGBA frames of a known size over a pipe, so
        /// the caller can read fixed-length frames as they arrive. `realtime`
        /// paces the decode to the wall clock for playback; without it a
        /// single frame is produced for a paused playhead.
        pub fn frame_command(
            source: &Path,
            seconds: f64,
            width: u32,
            height: u32,
            realtime: bool,
        ) -> Command {
            let mut command = Command::new(ffmpeg_executable());
            command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
            if realtime {
                command.arg("-re");
            }
            command
                .args(["-ss", &format!("{:.3}", seconds.max(0.0))])
                .arg("-i")
                .arg(source)
                .arg("-an");
            let filter = if realtime {
                format!("fps=30,scale={width}:{height}")
            } else {
                format!("scale={width}:{height}")
            };
            command.args(["-vf", &filter]);
            if !realtime {
                command.args(["-frames:v", "1"]);
            }
            command.args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]);
            command
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

        #[cfg(test)]
        mod tests {
            use super::parse_duration;

            #[test]
            fn ffmpeg_duration_lines_become_seconds() {
                let stderr = "  Duration: 00:00:07.64, start: 0.000000, bitrate: 8061 kb/s\n";
                assert!((parse_duration(stderr).unwrap() - 7.64).abs() < 1e-6);
            }

            #[test]
            fn hours_and_minutes_are_folded_in() {
                let stderr = "  Duration: 01:02:03.50, start: 0.0, bitrate: 1 kb/s";
                assert!((parse_duration(stderr).unwrap() - 3723.5).abs() < 1e-6);
            }

            #[test]
            fn missing_duration_is_not_guessed() {
                assert!(parse_duration("Duration: N/A, start: 0.0").is_none());
                assert!(parse_duration("Input #0, mov, from 'clip.mp4':").is_none());
            }
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
