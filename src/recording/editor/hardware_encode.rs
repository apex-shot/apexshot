//! Runtime detection of working hardware H.264 encoders, and the ffmpeg
//! arguments to drive them.
//!
//! The recording path chooses NVENC/VA-API from `ffmpeg -encoders` plus the
//! presence of a render node. That only proves the encoder is *compiled in*,
//! not that the driver can encode a frame. This module answers the stronger
//! question by actually encoding a few synthetic frames at the settings an
//! export would use and checking the exit status. A machine whose driver is
//! missing, too old, or busy fails the probe and gets libx264 instead of a
//! failed export.
//!
//! The editor export does **not** switch to hardware on its own. VA-API and
//! NVENC do not share libx264's rate control: the same quality tier produces a
//! different file — sometimes larger, sometimes softer — and the user picked a
//! tier, not an encoder. Linux has no single consistent hardware encoder across
//! GPUs and drivers, so the same tier can produce a different file. Hardware is
//! therefore used only when the user explicitly
//! asks for it with `APEXSHOT_EXPORT_HW_ENCODER` (`nvenc`, `vaapi`, or `auto`),
//! and only after the probe succeeds. Every other case keeps libx264, so the
//! default export is byte-for-byte what it was before this module existed.

use std::process::Command;
use std::sync::OnceLock;

/// The environment variable that opts an export into hardware encoding. Unset
/// or unrecognised values keep libx264.
pub const EXPORT_ENCODER_ENV: &str = "APEXSHOT_EXPORT_HW_ENCODER";

/// The filter-graph tail VA-API needs so its encoder can read the frames.
/// NVENC reads system memory and needs nothing.
pub const VAAPI_UPLOAD_FILTER: &str = "format=nv12,hwupload";

/// An H.264 hardware encoder the editor export can drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareEncoder {
    Nvenc,
    Vaapi,
}

impl HardwareEncoder {
    /// The `ffmpeg -c:v` name.
    pub fn ffmpeg_name(self) -> &'static str {
        match self {
            Self::Nvenc => "h264_nvenc",
            Self::Vaapi => "h264_vaapi",
        }
    }

    /// Whether frames have to be uploaded to the GPU before this encoder can
    /// read them. VA-API needs `hwupload` in the filter graph; NVENC takes
    /// system-memory frames directly.
    pub fn needs_hwupload(self) -> bool {
        matches!(self, Self::Vaapi)
    }
}

/// The order a probe tries when the user asks for `auto`. NVENC first because
/// it needs no filter-graph change, so it is the smaller integration risk.
const PROBE_ORDER: [HardwareEncoder; 2] = [HardwareEncoder::Nvenc, HardwareEncoder::Vaapi];

/// The render node VA-API encodes through. This is a concrete device rather
/// than a vendor guess: a missing node is a real reason to skip VA-API, and a
/// present one still has to pass the probe.
fn vaapi_device() -> Option<&'static str> {
    ["/dev/dri/renderD128", "/dev/dri/renderD129"]
        .into_iter()
        .find(|candidate| std::path::Path::new(candidate).exists())
}

/// Map the export's x264 CRF tier onto hardware constant-quantizer (CQP).
///
/// Both scales run 0–51 with lower meaning better quality, but they are not the
/// same measurement: x264 CRF adapts the bitrate to a perceptual target while
/// CQP holds one quantizer. Passing the tier's CRF through is a starting point,
/// not a validated equivalence — which is exactly why the hardware path is
/// opt-in rather than the default.
pub fn hardware_qp(crf: u32) -> u32 {
    crf.min(51)
}

/// The `-c:v`-onward arguments for `encoder` at quantizer `qp`.
pub fn hardware_video_args(encoder: HardwareEncoder, qp: u32) -> Vec<String> {
    match encoder {
        HardwareEncoder::Nvenc => vec![
            "-c:v".into(),
            "h264_nvenc".into(),
            "-preset".into(),
            "p5".into(),
            "-tune".into(),
            "hq".into(),
            // Same rate control the recording path uses (see
            // `src/recording/backend/wayland.rs`): constant quantizer so a
            // screen recording's flat areas do not starve detail.
            "-rc".into(),
            "constqp".into(),
            "-qp".into(),
            qp.to_string(),
            "-profile:v".into(),
            "high".into(),
        ],
        HardwareEncoder::Vaapi => vec![
            "-c:v".into(),
            "h264_vaapi".into(),
            "-rc_mode".into(),
            "CQP".into(),
            "-qp".into(),
            qp.to_string(),
            "-profile".into(),
            "high".into(),
        ],
    }
}

/// Output-side options an encoder needs before its frames are usable. NVENC
/// needs none; VA-API needs its render device selected.
pub fn hardware_input_args(encoder: HardwareEncoder) -> Vec<String> {
    match (encoder, vaapi_device()) {
        (HardwareEncoder::Vaapi, Some(device)) => vec!["-vaapi_device".into(), device.into()],
        _ => Vec::new(),
    }
}

/// The filter-graph tail `encoder` needs, if any. `None` for libx264 and NVENC,
/// which read system-memory frames.
pub fn hardware_upload_suffix(encoder: Option<HardwareEncoder>) -> Option<&'static str> {
    encoder
        .filter(|encoder| encoder.needs_hwupload())
        .map(|_| VAAPI_UPLOAD_FILTER)
}

/// The ffmpeg arguments that prove `encoder` can actually encode.
///
/// Split out as a pure function so the command shape is testable without a
/// GPU. `-f null -` runs the encoder and discards its output, so the probe
/// exercises the codec and driver without writing a file.
fn probe_args(encoder: HardwareEncoder, device: Option<&str>, qp: u32) -> Vec<String> {
    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y"]
        .into_iter()
        .map(str::to_owned)
        .collect();

    if encoder.needs_hwupload() {
        if let Some(device) = device {
            args.push("-vaapi_device".into());
            args.push(device.to_string());
        }
    }
    args.extend([
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        // The probe frame must exceed every hardware encoder's minimum
        // dimension, or a driver that refuses small frames turns a working
        // encoder into a false negative. 64x64 was too small: NVENC on a
        // 595.91 driver rejects anything under 256, so every export fell back
        // to libx264. 256x256 is accepted by NVENC and VA-API and is still
        // cheap to encode.
        "color=c=black:s=256x256:r=25:d=0.2".into(),
    ]);
    if encoder.needs_hwupload() {
        args.extend(["-vf".into(), VAAPI_UPLOAD_FILTER.to_string()]);
    }
    args.extend(["-frames:v".into(), "3".into()]);
    args.extend(hardware_video_args(encoder, qp));
    args.extend(["-f".into(), "null".into(), "-".into()]);
    args
}

/// Whether `encoder` can encode a frame with the flags an export would pass.
///
/// A missing VA-API render node is checked first so a machine without one does
/// not pay for an ffmpeg start-up that is certain to fail.
fn encoder_works(encoder: HardwareEncoder) -> bool {
    let device = if encoder.needs_hwupload() {
        let Some(device) = vaapi_device() else {
            return false;
        };
        Some(device)
    } else {
        None
    };

    Command::new("ffmpeg")
        .args(probe_args(encoder, device, 25))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(crate) fn recording_uses_nvenc() -> bool {
    detect_hardware_encoder() == Some(HardwareEncoder::Nvenc)
}

pub(crate) fn recording_uses_vaapi() -> bool {
    detect_hardware_encoder() == Some(HardwareEncoder::Vaapi)
}

pub(crate) fn recording_nvenc_works() -> bool {
    encoder_works(HardwareEncoder::Nvenc)
}

pub(crate) fn recording_vaapi_works() -> bool {
    encoder_works(HardwareEncoder::Vaapi)
}

/// The first hardware encoder that actually works, probed once per process.
pub fn detect_hardware_encoder() -> Option<HardwareEncoder> {
    static DETECTED: OnceLock<Option<HardwareEncoder>> = OnceLock::new();
    *DETECTED.get_or_init(|| {
        PROBE_ORDER
            .into_iter()
            .find(|encoder| encoder_works(*encoder))
    })
}

/// What `APEXSHOT_EXPORT_HW_ENCODER` asked for. Pure so the accepted values and
/// the software default are testable without touching the process environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Preference {
    /// libx264: the default, and what every unrecognised value means.
    Software,
    /// Use whatever working hardware encoder the probe finds.
    Auto,
    /// Use this encoder specifically, but only if it actually works.
    Specific(HardwareEncoder),
}

fn parse_preference(value: &str) -> Preference {
    match value.trim().to_ascii_lowercase().as_str() {
        "nvenc" | "h264_nvenc" => Preference::Specific(HardwareEncoder::Nvenc),
        "vaapi" | "h264_vaapi" => Preference::Specific(HardwareEncoder::Vaapi),
        "auto" => Preference::Auto,
        _ => Preference::Software,
    }
}

/// The hardware encoder an export should use, or `None` for libx264.
///
/// `None` is the default and the fallback: hardware requires an explicit
/// `APEXSHOT_EXPORT_HW_ENCODER` request *and* a passing runtime probe. An
/// explicit request that cannot encode falls back to libx264 with a note rather
/// than failing an export that would otherwise have finished.
///
/// Cached once per process: a multi-segment export builds one command per
/// segment, and the probe must not run once per segment.
pub fn selected_export_encoder() -> Option<HardwareEncoder> {
    static SELECTED: OnceLock<Option<HardwareEncoder>> = OnceLock::new();
    *SELECTED.get_or_init(|| {
        match parse_preference(&std::env::var(EXPORT_ENCODER_ENV).unwrap_or_default()) {
            Preference::Software => None,
            Preference::Auto => detect_hardware_encoder(),
            Preference::Specific(encoder) => {
                if encoder_works(encoder) {
                    Some(encoder)
                } else {
                    eprintln!(
                        "[export] {} was requested but cannot encode on this machine; using libx264",
                        encoder.ffmpeg_name()
                    );
                    None
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_or_unknown_opt_in_keeps_software() {
        // The default has to stay libx264, because a silent switch would change
        // the output file the user picked a quality tier for.
        assert_eq!(parse_preference(""), Preference::Software);
        assert_eq!(parse_preference("  "), Preference::Software);
        assert_eq!(parse_preference("cpu"), Preference::Software);
        assert_eq!(parse_preference("off"), Preference::Software);
        assert_eq!(parse_preference("software"), Preference::Software);
        assert_eq!(parse_preference("something-else"), Preference::Software);
    }

    #[test]
    fn the_opt_in_names_each_encoder_and_auto() {
        assert_eq!(
            parse_preference("NVENC"),
            Preference::Specific(HardwareEncoder::Nvenc)
        );
        assert_eq!(
            parse_preference(" h264_nvenc "),
            Preference::Specific(HardwareEncoder::Nvenc)
        );
        assert_eq!(
            parse_preference("vaapi"),
            Preference::Specific(HardwareEncoder::Vaapi)
        );
        assert_eq!(
            parse_preference("h264_vaapi"),
            Preference::Specific(HardwareEncoder::Vaapi)
        );
        assert_eq!(parse_preference("auto"), Preference::Auto);
    }

    #[test]
    fn only_vaapi_needs_an_upload() {
        assert!(!HardwareEncoder::Nvenc.needs_hwupload());
        assert!(HardwareEncoder::Vaapi.needs_hwupload());
        assert_eq!(hardware_upload_suffix(None), None);
        assert_eq!(hardware_upload_suffix(Some(HardwareEncoder::Nvenc)), None);
        assert_eq!(
            hardware_upload_suffix(Some(HardwareEncoder::Vaapi)),
            Some(VAAPI_UPLOAD_FILTER)
        );
    }

    #[test]
    fn hardware_arguments_name_the_encoder_and_its_quantizer() {
        let nvenc = hardware_video_args(HardwareEncoder::Nvenc, 20);
        assert!(nvenc.windows(2).any(|pair| pair == ["-c:v", "h264_nvenc"]));
        assert!(nvenc.windows(2).any(|pair| pair == ["-qp", "20"]));
        assert!(nvenc.windows(2).any(|pair| pair == ["-rc", "constqp"]));
        assert!(
            !nvenc.iter().any(|arg| arg == "-crf"),
            "hardware encoders do not take -crf"
        );

        let vaapi = hardware_video_args(HardwareEncoder::Vaapi, 16);
        assert!(vaapi.windows(2).any(|pair| pair == ["-c:v", "h264_vaapi"]));
        assert!(vaapi.windows(2).any(|pair| pair == ["-qp", "16"]));
        assert!(vaapi.windows(2).any(|pair| pair == ["-rc_mode", "CQP"]));
    }

    #[test]
    fn the_quantizer_mapping_clamps_to_the_legal_range() {
        assert_eq!(hardware_qp(0), 0);
        assert_eq!(hardware_qp(20), 20);
        assert_eq!(hardware_qp(51), 51);
        assert_eq!(hardware_qp(99), 51);
    }

    #[test]
    fn the_probe_encodes_a_frame_without_writing_a_file() {
        let nvenc = probe_args(HardwareEncoder::Nvenc, None, 25);
        assert!(nvenc.windows(2).any(|pair| pair == ["-c:v", "h264_nvenc"]));
        assert!(nvenc.windows(2).any(|pair| pair == ["-f", "null"]));
        assert!(nvenc.windows(2).any(|pair| pair == ["-frames:v", "3"]));
        assert!(
            !nvenc.iter().any(|arg| arg == "-vaapi_device"),
            "NVENC does not take a VA-API device"
        );
        assert!(
            !nvenc.iter().any(|arg| arg == VAAPI_UPLOAD_FILTER),
            "NVENC reads system-memory frames directly"
        );
    }

    #[test]
    fn the_probe_frame_is_large_enough_for_hardware_encoders() {
        // A probe frame below an encoder's minimum dimension makes a working
        // encoder look absent, which disables the whole opt-in path.
        let nvenc = probe_args(HardwareEncoder::Nvenc, None, 25);
        let source = nvenc
            .iter()
            .find(|arg| arg.starts_with("color=c=black:s="))
            .expect("the probe names its lavfi source");
        assert_eq!(
            source, "color=c=black:s=256x256:r=25:d=0.2",
            "the probe frame must be large enough for hardware encoders"
        );
    }

    #[test]
    fn the_vaapi_probe_selects_its_device_and_uploads() {
        let vaapi = probe_args(HardwareEncoder::Vaapi, Some("/dev/dri/renderD128"), 25);
        assert!(vaapi
            .windows(2)
            .any(|pair| pair == ["-vaapi_device", "/dev/dri/renderD128"]));
        assert!(vaapi
            .windows(2)
            .any(|pair| pair == ["-vf", VAAPI_UPLOAD_FILTER]));
        assert!(vaapi.windows(2).any(|pair| pair == ["-c:v", "h264_vaapi"]));
    }

    #[test]
    fn a_vaapi_probe_without_a_device_still_names_the_encoder() {
        // `encoder_works` refuses a device-less VA-API before this runs, but
        // the argument builder must not panic or drop the encoder.
        let vaapi = probe_args(HardwareEncoder::Vaapi, None, 25);
        assert!(vaapi.windows(2).any(|pair| pair == ["-c:v", "h264_vaapi"]));
    }
}
