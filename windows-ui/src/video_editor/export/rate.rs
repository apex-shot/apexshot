//! One rate model for encoding and for the export dialog's estimates. H.264
//! quality is a CRF; the hardware encoders cannot honour one, so they get the
//! bitrate the same CRF produces on this curve.

const CRF_POINTS: [f64; 4] = [14.0, 24.0, 30.0, 42.0];
const BPP_POINTS: [f64; 4] = [0.0816, 0.0221, 0.011, 0.0038];
const REFERENCE_PIXELS: f64 = 2_073_600.0;
const REFERENCE_FPS: f64 = 60.0;
/// The low and high ends of an H.264 size estimate, as multiples of the typical size.
pub(crate) const H264_SPREAD: (f64, f64) = (0.65, 1.5);
/// The sound track's allowance in an MP4 estimate, in bits per second.
pub(crate) const MP4_AUDIO_BPS: f64 = 165_000.0;
/// How much slower than the reference model this exporter runs, measured on
/// the development machine in a release build: a 10 s 1080p60 cursor export
/// took 12.1 s against the model's 5.6 s, a plain 1080p30 export 3.6 s against
/// 2.8 s. Zoom transitions with motion blur run slower still.
const CALIBRATION: f64 = 2.0;

/// Piecewise linear through the points, extending the first and last segments'
/// lines past the ends instead of clamping.
fn interpolate(x: f64, xs: &[f64], ys: &[f64]) -> f64 {
    let last = xs.len().saturating_sub(2);
    let segment = xs
        .iter()
        .take(last + 1)
        .rposition(|value| *value <= x)
        .unwrap_or(0);
    let (x0, x1) = (xs[segment], xs[segment + 1]);
    let (y0, y1) = (ys[segment], ys[segment + 1]);
    y0 + (x - x0) * (y1 - y0) / (x1 - x0)
}

pub(crate) fn h264_bits_per_second(crf: u32, size: (u32, u32), fps: f64) -> f64 {
    let logs = BPP_POINTS.map(f64::ln);
    let bpp = interpolate(f64::from(crf), &CRF_POINTS, &logs).exp();
    let pixels = f64::from(size.0) * f64::from(size.1);
    bpp * REFERENCE_PIXELS
        * REFERENCE_FPS
        * (pixels / REFERENCE_PIXELS).powf(0.78)
        * (fps / REFERENCE_FPS).powf(0.57)
}

/// The low, typical, and high sizes in bytes of a clip at this bitrate.
pub(crate) fn bytes(bps: f64, audio_bps: f64, seconds: f64, spread: (f64, f64)) -> (f64, f64, f64) {
    let size = |video: f64| (video + audio_bps) / 8.0 * seconds.max(0.0);
    (size(bps * spread.0), size(bps), size(bps * spread.1))
}

pub(crate) fn export_seconds(size: (u32, u32), fps: f64, duration: f64) -> f64 {
    let pixels = f64::from(size.0) * f64::from(size.1);
    let frames = duration.max(0.0) * fps;
    let work = interpolate(
        pixels,
        &[921_600.0, 8_294_400.0],
        &[8_294_400.0 / 3.0, 8_294_400.0],
    );
    work * frames * (1_200_000.0 / 113_400.0 / 8_294_400.0) * 2.0 / 1000.0 * CALIBRATION
}

pub(crate) fn format_bytes(bytes: f64) -> String {
    if bytes < 100_000.0 {
        format!("{} KB", (bytes / 1000.0).round().max(1.0) as u64)
    } else if bytes < 995_000.0 {
        format!("{} KB", ((bytes / 10_000.0).round() * 10.0) as u64)
    } else if bytes < 25_000_000.0 {
        format!("{:.1} MB", bytes / 1e6)
    } else if bytes < 100_000_000.0 {
        format!("{} MB", ((bytes / 5e6).round() * 5.0) as u64)
    } else if bytes < 1_000_000_000.0 {
        format!("{} MB", ((bytes / 1e7).round() * 10.0) as u64)
    } else {
        format!("{:.1} GB", bytes / 1e9)
    }
}

pub(crate) fn format_duration(seconds: f64) -> String {
    if seconds < 59.5 {
        format!("{} s", seconds.round().max(1.0) as u64)
    } else {
        format!("{} min", (seconds / 60.0).round().max(1.0) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h264_curve_hits_the_reference_points() {
        let typical = h264_bits_per_second(24, (1920, 1080), 60.0);
        assert!((typical - 2_749_593.6).abs() / 2_749_593.6 < 0.01);
        let studio = h264_bits_per_second(14, (1920, 1080), 60.0);
        assert!(
            (studio - 0.0816 * 2_073_600.0 * 60.0).abs() / (0.0816 * 2_073_600.0 * 60.0) < 0.01
        );
        let small = h264_bits_per_second(42, (1280, 720), 30.0);
        let medium = h264_bits_per_second(30, (1280, 720), 30.0);
        let web = h264_bits_per_second(24, (1280, 720), 30.0);
        let high = h264_bits_per_second(14, (1280, 720), 30.0);
        assert!(small < medium);
        assert!(medium < web);
        assert!(web < high);
    }

    #[test]
    fn the_curve_extends_past_its_points() {
        let studio = h264_bits_per_second(14, (1920, 1080), 60.0);
        let higher = h264_bits_per_second(10, (1920, 1080), 60.0);
        let smallest = h264_bits_per_second(42, (1920, 1080), 60.0);
        let lower = h264_bits_per_second(50, (1920, 1080), 60.0);
        assert!(higher.is_finite() && higher > 0.0);
        assert!(lower.is_finite() && lower > 0.0);
        assert!(higher > studio);
        assert!(lower < smallest);
    }

    #[test]
    fn export_time_matches_the_reference_estimate() {
        let seconds = export_seconds((1920, 1080), 60.0, 10.0);
        assert!((seconds - 5.555 * CALIBRATION).abs() < 0.2, "{seconds}");
    }

    #[test]
    fn byte_ranges_follow_the_spread() {
        let (low, typical, high) = bytes(1_000_000.0, 0.0, 8.0, (0.5, 2.0));
        assert_eq!((low, typical, high), (500_000.0, 1_000_000.0, 2_000_000.0));
    }

    #[test]
    fn sizes_and_durations_format_readably() {
        assert_eq!(format_bytes(40_000.0), "40 KB");
        assert_eq!(format_bytes(512_345.0), "510 KB");
        assert_eq!(format_bytes(12_345_678.0), "12.3 MB");
        assert_eq!(format_bytes(43_000_000.0), "45 MB");
        assert_eq!(format_bytes(432_000_000.0), "430 MB");
        assert_eq!(format_bytes(2_340_000_000.0), "2.3 GB");
        assert_eq!(format_duration(5.6), "6 s");
        assert_eq!(format_duration(200.0), "3 min");
    }
}
