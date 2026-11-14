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

    /// The cursor and background model types the editors share: cursor
    /// settings plus the `VideoGradient` spec the annotation types reference.
    pub mod model {
        include!("../../src/recording/editor/model_parts/cursor_types.rs");
        include!("../../src/recording/editor/model_parts/background_types.rs");
        include!("../../src/recording/editor/model_parts/motion_transform.rs");
        include!("../../src/recording/editor/model_parts/motion_text.rs");
        include!("../../src/recording/editor/model_parts/motion_blur.rs");
        include!("../../src/recording/editor/model_parts/motion_scene.rs");
        include!("../../src/recording/editor/model_parts/motion_state_impl.rs");
    }

    /// The cursor sprites and previews the Linux editor draws.
    #[path = "../../../../src/recording/editor/cursor_sprite.rs"]
    #[rustfmt::skip]
    pub mod cursor_sprite;

    /// The spring the cursor travels on, shared with the Linux editor.
    #[allow(dead_code)]
    #[path = "../../../../src/recording/editor/cursor_path.rs"]
    #[rustfmt::skip]
    pub mod cursor_path;

    /// The studied planner that feeds the cursor spring.
    #[allow(dead_code)]
    #[path = "../../../../src/recording/editor/cursor_planner.rs"]
    #[rustfmt::skip]
    pub mod cursor_planner;

    /// The studied shake filter that thins the planner's input.
    #[path = "../../../../src/recording/editor/cursor_shake.rs"]
    #[rustfmt::skip]
    pub mod cursor_shake;

    /// The recording ID a finished recording carries in its MP4.
    #[allow(dead_code)]
    #[path = "../../../../src/recording/editor/recording_id.rs"]
    #[rustfmt::skip]
    pub mod recording_id;

    #[allow(dead_code)]
    #[path = "../../../../src/recording/editor/sidecar.rs"]
    #[rustfmt::skip]
    pub mod sidecar;

    #[allow(dead_code)]
    #[path = "../../../../src/recording/editor/auto_zoom.rs"]
    #[rustfmt::skip]
    pub mod auto_zoom;

    /// The live click ripple's frame warp, shared with the Linux editor.
    #[allow(dead_code)]
    #[path = "../../../../src/recording/editor/ripple_preview.rs"]
    #[rustfmt::skip]
    pub mod ripple_preview;

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

        /// The stream's colour matrix and range, as ffprobe names them.
        ///
        /// The export converts its composed RGBA back to YUV, so it has to use
        /// the same matrix the decoder used to produce that RGBA. Unknown or
        /// missing metadata comes back as `None`, which the caller treats the
        /// way ffmpeg does for an untagged stream.
        pub fn probe_color_space(source: &Path) -> (Option<String>, Option<String>) {
            let Ok(output) = crate::utils::hidden_command(ffprobe_executable())
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    "v:0",
                    "-show_entries",
                    "stream=color_space,color_range",
                    "-of",
                    "default=noprint_wrappers=1",
                ])
                .arg(source)
                .output()
            else {
                return (None, None);
            };
            let text = String::from_utf8_lossy(&output.stdout);
            let mut color_space = None;
            let mut color_range = None;
            for line in text.lines() {
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                let value = value.trim();
                let value = (!value.is_empty() && value != "unknown").then(|| value.to_string());
                match key.trim() {
                    "color_space" => color_space = value,
                    "color_range" => color_range = value,
                    _ => {}
                }
            }
            (color_space, color_range)
        }

        /// Read the container duration without decoding the stream.
        pub fn probe_duration(source: &Path) -> Result<f64, String> {
            let output = crate::utils::hidden_command(ffmpeg_executable())
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
            let output = crate::utils::hidden_command(ffmpeg_executable())
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

            let output = crate::utils::hidden_command(ffmpeg_executable())
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
            rate: f64,
        ) -> Command {
            let mut command = crate::utils::hidden_command(ffmpeg_executable());
            command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
            if realtime {
                // A sped-up piece reads its source faster than the wall clock,
                // so the stream keeps pace instead of being restarted.
                if (rate - 1.0).abs() < 1e-3 {
                    command.arg("-re");
                } else {
                    command.args(["-readrate", &format!("{:.3}", rate.clamp(0.1, 16.0))]);
                }
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

        /// The recording's frame rate, from the container's video stream.
        /// Falls back to the "30 fps" banner in ffmpeg's stream info.
        pub fn probe_frame_rate(source: &Path) -> Option<f64> {
            if let Ok(output) = crate::utils::hidden_command(ffprobe_executable())
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    "v:0",
                    "-show_entries",
                    "stream=r_frame_rate",
                    "-of",
                    "default=noprint_wrappers=1:nokey=1",
                ])
                .arg(source)
                .output()
            {
                if output.status.success() {
                    let rate = String::from_utf8_lossy(&output.stdout);
                    if let Some(fps) = parse_rate(rate.trim()) {
                        return Some(fps);
                    }
                }
            }
            let output = crate::utils::hidden_command(ffmpeg_executable())
                .args(["-hide_banner", "-i"])
                .arg(source)
                .output()
                .ok()?;
            parse_fps_line(&String::from_utf8_lossy(&output.stderr))
        }

        fn parse_rate(text: &str) -> Option<f64> {
            let (numerator, denominator) = text.trim().split_once('/')?;
            let numerator: f64 = numerator.trim().parse().ok()?;
            let denominator: f64 = denominator.trim().parse().ok()?;
            (numerator.is_finite()
                && denominator.is_finite()
                && numerator > 0.0
                && denominator > 0.0)
                .then(|| numerator / denominator)
        }

        fn parse_fps_line(stderr: &str) -> Option<f64> {
            for (index, _) in stderr.match_indices(" fps") {
                let token = stderr[..index]
                    .rsplit([',', ' '])
                    .find(|token| !token.is_empty())?;
                if let Ok(fps) = token.parse::<f64>() {
                    if fps.is_finite() && fps > 0.0 {
                        return Some(fps);
                    }
                }
            }
            None
        }

        /// The encoded video's frame size, from the container's video stream.
        /// The pointer sidecar's clicks are mapped into these pixels before an
        /// automatic zoom is placed, so a fresh import needs them from ffprobe
        /// rather than from the editor's preview.
        pub fn probe_frame_size(source: &Path) -> Option<(u32, u32)> {
            let output = crate::utils::hidden_command(ffprobe_executable())
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    "v:0",
                    "-show_entries",
                    "stream=width,height",
                    "-of",
                    "json",
                ])
                .arg(source)
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            parse_frame_size(&String::from_utf8_lossy(&output.stdout))
        }

        fn parse_frame_size(text: &str) -> Option<(u32, u32)> {
            let value: serde_json::Value = serde_json::from_str(text).ok()?;
            let stream = value.get("streams")?.as_array()?.first()?;
            let width = u32::try_from(stream.get("width")?.as_u64()?).ok()?;
            let height = u32::try_from(stream.get("height")?.as_u64()?).ok()?;
            (width >= 2 && height >= 2).then_some((width, height))
        }

        pub fn ffmpeg_executable() -> PathBuf {
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

        pub fn ffprobe_executable() -> PathBuf {
            if let Some(path) = std::env::var_os("APEXSHOT_FFPROBE") {
                let path = PathBuf::from(path);
                if path.is_file() {
                    return path;
                }
            }
            let ffmpeg = ffmpeg_executable();
            if ffmpeg.components().count() > 1 {
                let name = if cfg!(windows) {
                    "ffprobe.exe"
                } else {
                    "ffprobe"
                };
                return ffmpeg.with_file_name(name);
            }
            PathBuf::from(if cfg!(windows) {
                "ffprobe.exe"
            } else {
                "ffprobe"
            })
        }

        /// Check the bundled tools before an export starts, so the failure
        /// names the missing binary instead of a generic encoder error.
        pub fn ensure_tools_available() -> Result<(), String> {
            for (name, path) in [
                ("ffmpeg", ffmpeg_executable()),
                ("ffprobe", ffprobe_executable()),
            ] {
                if !tool_available(&path) {
                    return Err(format!(
                        "{name} is not available. Reinstall ApexShot or add {name} to PATH."
                    ));
                }
            }
            Ok(())
        }

        fn tool_available(path: &Path) -> bool {
            path.is_file()
                || crate::utils::hidden_command(path)
                    .arg("-version")
                    .output()
                    .map(|output| output.status.success())
                    .unwrap_or(false)
        }

        /// Build the encoder command through the Windows executable lookup, so
        /// the packaged `bin/ffmpeg.exe` wins over whatever `PATH` holds.
        pub fn export_command() -> Command {
            crate::utils::hidden_command(ffmpeg_executable())
        }

        #[cfg(test)]
        mod tests {
            use super::{parse_duration, parse_fps_line, parse_frame_size, parse_rate};

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

            #[test]
            fn frame_rates_come_from_the_stream_or_the_banner() {
                assert!((parse_rate("30/1").unwrap() - 30.0).abs() < 1e-9);
                assert!((parse_rate("30000/1001").unwrap() - 30000.0 / 1001.0).abs() < 1e-9);
                assert!(parse_rate("0/0").is_none());
                assert!(parse_rate("30").is_none());
                let banner = "  Stream #0:0: Video: h264, yuv420p, 1920x1080, 30 fps, 30 tbr\n";
                assert!((parse_fps_line(banner).unwrap() - 30.0).abs() < 1e-9);
                let other = "  Stream #0:0: Video: h264, yuv420p, 1280x720, 59.94 fps, 59.94 tbr";
                assert!((parse_fps_line(other).unwrap() - 59.94).abs() < 1e-6);
                assert!(parse_fps_line("no rate here").is_none());
            }

            #[test]
            fn the_frame_size_comes_from_the_video_stream() {
                let json = r#"{"programs":[],"streams":[{"width":1920,"height":1080,"codec_name":"h264"}]}"#;
                assert_eq!(parse_frame_size(json), Some((1920, 1080)));
            }

            #[test]
            fn a_missing_or_unreadable_frame_size_is_not_guessed() {
                assert_eq!(parse_frame_size(r#"{"streams":[]}"#), None);
                assert_eq!(parse_frame_size(r#"{"streams":[{"height":1080}]}"#), None);
                assert_eq!(
                    parse_frame_size(r#"{"streams":[{"width":"1920","height":1080}]}"#),
                    None
                );
                assert_eq!(parse_frame_size("not json"), None);
            }

            #[test]
            fn a_frame_size_too_small_to_compose_is_refused() {
                assert_eq!(
                    parse_frame_size(r#"{"streams":[{"width":0,"height":1080}]}"#),
                    None
                );
                assert_eq!(
                    parse_frame_size(r#"{"streams":[{"width":1,"height":1}]}"#),
                    None
                );
            }

            #[test]
            fn a_frame_size_beyond_u32_is_refused() {
                let json = r#"{"streams":[{"width":4294967296,"height":1080}]}"#;
                assert_eq!(parse_frame_size(json), None);
            }
        }
    }
}

/// Folder recordings land in: Settings' video export location, else the
/// capture shell's own default, `ApexShotRec` inside the Videos folder
/// (`defaultRecordingDirectory` in the shell's settings store). History lists
/// this folder, so the two defaults have to stay the same.
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
        .join("ApexShotRec")
}

#[cfg(test)]
mod output_dir_tests {
    #[test]
    fn recordings_default_to_the_capture_shells_folder() {
        let config = crate::config::AppConfig::default();
        assert!(config.video_export_location.trim().is_empty());
        let dir = super::recording_output_dir(&config);
        assert_eq!(
            dir.file_name().and_then(|name| name.to_str()),
            Some("ApexShotRec")
        );
    }

    #[test]
    fn a_configured_folder_is_used_as_is() {
        let config = crate::config::AppConfig {
            video_export_location: r"D:\Clips".to_string(),
            ..crate::config::AppConfig::default()
        };
        assert_eq!(
            super::recording_output_dir(&config),
            std::path::PathBuf::from(r"D:\Clips")
        );
    }
}

pub fn recording_supported() -> bool {
    true
}
