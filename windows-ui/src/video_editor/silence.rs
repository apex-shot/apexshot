//! Silent spans of a recording's audio, for the studied "hide the camera
//! during silence" option.

use std::path::Path;
use std::process::Stdio;

/// The studied silence threshold, 0.1 of full scale, in decibels.
const SILENCE_NOISE_DB: f64 = -20.0;
/// The shortest silence the analysis reports, in seconds. The studied floor.
const SILENCE_MIN_SECONDS: f64 = 0.1;
/// How far inside a silence the camera has to be before it hides.
pub(super) const SILENCE_MARGIN_SECONDS: f64 = 0.5;

/// The silent spans of `source`'s first audio stream, in source seconds, or
/// `None` when it has no audio stream (or the analysis could not run).
pub(super) fn silent_ranges(source: &Path) -> Option<Vec<(f64, f64)>> {
    let output =
        crate::utils::hidden_command(crate::recording::editor::ffmpeg::ffmpeg_executable())
            .args(["-hide_banner", "-nostats", "-nostdin", "-i"])
            .arg(source)
            .args([
                "-map",
                "0:a:0",
                "-af",
                &format!("silencedetect=noise={SILENCE_NOISE_DB}dB:d={SILENCE_MIN_SECONDS}"),
                "-f",
                "null",
                "-",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .ok()?;
    let log = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return None;
    }
    Some(parse_silence_log(&log))
}

/// The spans an ffmpeg `silencedetect` log reports. A silence still open at
/// the end of the file runs to infinity.
pub(super) fn parse_silence_log(log: &str) -> Vec<(f64, f64)> {
    let value = |line: &str, key: &str| -> Option<f64> {
        let rest = &line[line.find(key)? + key.len()..];
        rest.split_whitespace()
            .next()?
            .trim_end_matches(|c: char| !c.is_ascii_digit())
            .parse()
            .ok()
    };
    let mut ranges = Vec::new();
    let mut open: Option<f64> = None;
    for line in log.lines() {
        if let Some(start) = value(line, "silence_start: ") {
            open = Some(start.max(0.0));
        } else if let Some(end) = value(line, "silence_end: ") {
            if let Some(start) = open.take() {
                if end > start {
                    ranges.push((start, end));
                }
            }
        }
    }
    if let Some(start) = open {
        ranges.push((start, f64::INFINITY));
    }
    ranges
}

/// Whether the studied rule hides the camera at `t`: inside a silence at
/// least `min_seconds` long and at least the margin from both its ends.
pub(super) fn silent_at(ranges: &[(f64, f64)], t: f64, min_seconds: f64) -> bool {
    ranges.iter().any(|&(start, end)| {
        end - start >= min_seconds
            && t - start >= SILENCE_MARGIN_SECONDS
            && end - t >= SILENCE_MARGIN_SECONDS
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_silence_log, silent_at};

    #[test]
    fn the_log_reads_its_spans_and_leaves_an_open_one_running() {
        let log = "\
[silencedetect @ 0x1] silence_start: 2.5
[silencedetect @ 0x1] silence_end: 8.75 | silence_duration: 6.25
[silencedetect @ 0x1] silence_start: 20
";
        assert_eq!(
            parse_silence_log(log),
            vec![(2.5, 8.75), (20.0, f64::INFINITY)]
        );
    }

    #[test]
    fn the_margin_and_the_minimum_length_both_decide_the_hide() {
        let ranges = [(2.0, 8.0)];
        assert!(silent_at(&ranges, 4.0, 5.0));
        assert!(!silent_at(&ranges, 2.4, 5.0));
        assert!(!silent_at(&ranges, 7.7, 5.0));
        assert!(!silent_at(&[(2.0, 4.0)], 3.0, 5.0));
    }
}
