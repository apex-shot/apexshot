use super::super::{RecordError, RecordResult, RecordingConfig};
use gstreamer as gst;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug)]
pub(super) struct EncoderProfile {
    pub(super) name: &'static str,
    pub(super) encoder: &'static str, // GStreamer element name (used by X11 path)
    pub(super) ffmpeg_encoder: &'static str, // ffmpeg -c:v name (used by Wayland path)
    pub(super) muxer: &'static str,
    pub(super) extension: &'static str,
}

pub(super) const PROFILES: &[EncoderProfile] = &[
    // VP9 (WebM)
    EncoderProfile {
        name: "VP9",
        encoder: "vp9enc",
        ffmpeg_encoder: "libvpx-vp9",
        muxer: "webmmux",
        extension: "webm",
    },
    // VP8 (WebM) - fallback when VP9 is unavailable
    EncoderProfile {
        name: "VP8",
        encoder: "vp8enc",
        ffmpeg_encoder: "libvpx",
        muxer: "webmmux",
        extension: "webm",
    },
    // Standard H.264
    EncoderProfile {
        name: "H.264 (x264)",
        encoder: "x264enc",
        ffmpeg_encoder: "libx264",
        muxer: "mp4mux",
        extension: "mp4",
    },
    // Cisco OpenH264
    EncoderProfile {
        name: "H.264 (OpenH264)",
        encoder: "openh264enc",
        ffmpeg_encoder: "libopenh264",
        muxer: "mp4mux",
        extension: "mp4",
    },
    // Theora (Ogg) - Last resort
    EncoderProfile {
        name: "Theora",
        encoder: "theoraenc",
        ffmpeg_encoder: "libtheora",
        muxer: "oggmux",
        extension: "ogv",
    },
];

/// Cached set of encoder names reported by `ffmpeg -encoders`.
/// Fedora ships `ffmpeg-free` without `libx264`; `libopenh264` is the usual
/// H.264 path. Ubuntu/etc. typically have `libx264`. Probe once per process.
pub(super) fn ffmpeg_available_encoders() -> &'static HashSet<String> {
    static CACHE: OnceLock<HashSet<String>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut set = HashSet::new();
        let Ok(output) = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
        else {
            return set;
        };
        // ffmpeg prints the encoder table to stdout (sometimes mixed with
        // banner noise on stderr). Parse both.
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for line in text.lines() {
            // Lines look like: " V....D libopenh264          OpenH264 ..."
            let trimmed = line.trim();
            if trimmed.len() < 10 {
                continue;
            }
            // Flag field is typically 6 chars then space then name.
            let rest = if trimmed.starts_with([' ', 'V', 'A', 'S']) && trimmed.len() > 8 {
                trimmed.get(7..).unwrap_or(trimmed).trim_start()
            } else {
                continue;
            };
            let name = rest.split_whitespace().next().unwrap_or("");
            if name.starts_with("lib")
                || name.contains("264")
                || name.contains("265")
                || name.contains("vp8")
                || name.contains("vp9")
                || name.contains("theora")
                || name.contains("av1")
            {
                set.insert(name.to_string());
            }
        }
        set
    })
}

pub(super) fn ffmpeg_encoder_available(name: &str) -> bool {
    ffmpeg_available_encoders().contains(name)
}

/// Whether this profile can be used on the current session.
/// Wayland recording encodes with ffmpeg; X11 uses GStreamer elements.
fn encoder_profile_available(profile: &EncoderProfile) -> bool {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return ffmpeg_encoder_available(profile.ffmpeg_encoder);
    }
    // X11 / GStreamer path — best-effort; init is cheap if already done.
    if gst::init().is_err() {
        // Fall back to ffmpeg availability if GST is unavailable.
        return ffmpeg_encoder_available(profile.ffmpeg_encoder);
    }
    gst::ElementFactory::find(profile.encoder).is_some()
        || ffmpeg_encoder_available(profile.ffmpeg_encoder)
}

pub(super) fn select_encoder(
    requested_path: &Path,
) -> RecordResult<(&'static EncoderProfile, PathBuf)> {
    // Prefer the requested container when an actually-installed encoder supports it.
    // Important where ffmpeg-free lacks libx264: pick OpenH264 or another format
    // instead of hard-failing mid-encode.
    if let Some(ext) = requested_path.extension().and_then(|s| s.to_str()) {
        let mut matched_ext = false;
        for profile in PROFILES {
            if profile.extension != ext {
                continue;
            }
            matched_ext = true;
            if encoder_profile_available(profile) {
                return Ok((profile, requested_path.to_path_buf()));
            }
        }
        if matched_ext {
            println!("Warning: no installed encoder for '.{ext}'; trying another format.");
        } else {
            println!("Warning: Requested format '{ext}' not in profile list; using default.");
        }
    }

    // Fall back: first available profile in priority order (VP9 → VP8 → x264 → OpenH264 → Theora).
    for profile in PROFILES {
        if encoder_profile_available(profile) {
            let mut new_path = requested_path.to_path_buf();
            new_path.set_extension(profile.extension);
            if new_path != requested_path {
                println!(
                    "Note: using {} ({}) → {}",
                    profile.name,
                    profile.ffmpeg_encoder,
                    new_path.display()
                );
            }
            return Ok((profile, new_path));
        }
    }

    Err(RecordError::NoEncoderFound)
}

#[cfg(test)]
pub(super) fn video_encoder_props(profile: &EncoderProfile, config: &RecordingConfig) -> String {
    let key_int_max = config.fps.saturating_mul(2).max(1);

    // File-recording presets (quality over streaming latency).

    if profile.encoder == "x264enc" {
        // veryfast, CRF 22, main profile.
        return format!("preset=veryfast crf=22 profile=main key-int-max={key_int_max}",);
    }

    if profile.encoder == "vp9enc" {
        // Local recording: slightly higher quality than typical streaming CQ.
        return format!(
            "deadline=good end-usage=cq cq-level=20 target-bitrate=0 cpu-used=2 row-mt=true threads=8 keyframe-max-dist={key_int_max} lag-in-frames=0",
        );
    }

    if profile.encoder == "vp8enc" {
        return format!(
            "deadline=good end-usage=cq cq-level=10 target-bitrate=0 cpu-used=2 threads=8 keyframe-max-dist={key_int_max} lag-in-frames=0",
        );
    }

    if profile.encoder == "openh264enc" {
        return "bitrate=8000000 complexity=medium".to_string();
    }

    String::new()
}

pub(super) fn normalize_recording_config_for_profile(
    profile: &EncoderProfile,
    config: &RecordingConfig,
) -> RecordingConfig {
    let mut normalized = config.clone();

    // H.264 encoders (x264 / OpenH264) need even dimensions for yuv420p.
    if !matches!(profile.encoder, "x264enc" | "openh264enc") {
        return normalized;
    }

    if let Some(width) = normalized.width {
        if width > 1 && width % 2 != 0 {
            normalized.width = Some(width - 1);
        }
    }

    if let Some(height) = normalized.height {
        if height > 1 && height % 2 != 0 {
            normalized.height = Some(height - 1);
        }
    }

    normalized
}
