//! The recording's loudness over source time, for the video timeline's Sound
//! lane.
//!
//! The mixed audio track is decoded once, on a worker thread, into one peak
//! value per 20 ms bucket. The lane then reads the buckets the footage under
//! each of its columns plays, so the waveform follows the cuts and the speed
//! changes instead of the raw file.

use std::io::Read;
use std::path::Path;

/// The rate the waveform is decoded at.
const PEAK_RATE: usize = 8000;

/// The samples one waveform value covers: 20 ms at the peak rate.
const PEAK_BUCKET: usize = PEAK_RATE / 50;

/// The recording's loudness over source time.
#[derive(Debug, Clone, PartialEq)]
pub struct Peaks {
    /// Seconds each value covers.
    bucket_seconds: f64,
    /// Peak level per bucket, 0..=1, from source time 0.
    values: Vec<f32>,
}

impl Peaks {
    /// The peak over source seconds `start..end`: the max of the buckets that
    /// overlap that span, with `end` exclusive so a span stopping on a bucket
    /// boundary does not read the bucket after it. 0 outside the measured
    /// data.
    pub fn level(&self, start: f64, end: f64) -> f32 {
        if self.values.is_empty() || !self.bucket_seconds.is_finite() || self.bucket_seconds <= 0.0
        {
            return 0.0;
        }
        if !start.is_finite() || !end.is_finite() || end < 0.0 || start > end {
            return 0.0;
        }
        let first = (start.max(0.0) / self.bucket_seconds).floor() as usize;
        // `ceil - 1` is the last bucket the exclusive end reaches; a
        // zero-length span is floored onto its own bucket instead.
        let last =
            (((end.max(0.0) / self.bucket_seconds).ceil() as usize).saturating_sub(1)).max(first);
        if first >= self.values.len() {
            return 0.0;
        }
        let last = last.min(self.values.len() - 1);
        self.values[first..=last]
            .iter()
            .copied()
            .fold(0.0, f32::max)
    }
}

/// Decode the recording's audio to mono and measure peaks in 20 ms buckets.
/// Blocking, so the caller runs it on a worker thread; `None` for a recording
/// with no sound or a reader that could not be started.
pub fn measure_peaks(video: &Path) -> Option<Peaks> {
    let mut child = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(video)
        .args([
            "-map", "0:a:0", "-ac", "1", "-ar", "8000", "-f", "f32le", "-",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let values = peaks_from_chunks(std::iter::from_fn(|| {
        let mut chunk = vec![0u8; 65536];
        match stdout.read(&mut chunk) {
            Ok(0) | Err(_) => None,
            Ok(read) => {
                chunk.truncate(read);
                Some(chunk)
            }
        }
    }));
    drop(stdout);
    let finished = child.wait();
    if !finished.is_ok_and(|status| status.success()) || values.is_empty() {
        return None;
    }
    Some(Peaks {
        bucket_seconds: PEAK_BUCKET as f64 / PEAK_RATE as f64,
        values,
    })
}

/// Peak level per bucket over chunks of little-endian `f32` samples. A chunk
/// may end mid-sample, so its trailing bytes carry into the next one, and the
/// last bucket keeps a partial fill instead of being dropped.
fn peaks_from_chunks<I, B>(chunks: I) -> Vec<f32>
where
    I: IntoIterator<Item = B>,
    B: AsRef<[u8]>,
{
    let mut values: Vec<f32> = Vec::new();
    let mut peak = 0.0f32;
    let mut filled = 0usize;
    let mut carry = [0u8; 4];
    let mut carried = 0usize;
    for chunk in chunks {
        let mut rest = chunk.as_ref();
        if carried > 0 {
            let take = (4 - carried).min(rest.len());
            carry[carried..carried + take].copy_from_slice(&rest[..take]);
            carried += take;
            rest = &rest[take..];
            if carried == 4 {
                peak = peak.max(f32::from_le_bytes(carry).abs());
                filled += 1;
                if filled == PEAK_BUCKET {
                    values.push(peak.min(1.0));
                    peak = 0.0;
                    filled = 0;
                }
                carried = 0;
            }
        }
        if rest.is_empty() {
            continue;
        }
        let (samples, tail) = rest.as_chunks::<4>();
        for bytes in samples {
            peak = peak.max(f32::from_le_bytes(*bytes).abs());
            filled += 1;
            if filled == PEAK_BUCKET {
                values.push(peak.min(1.0));
                peak = 0.0;
                filled = 0;
            }
        }
        carry[..tail.len()].copy_from_slice(tail);
        carried = tail.len();
    }
    if filled > 0 {
        values.push(peak.min(1.0));
    }
    values
}

#[cfg(test)]
mod tests {
    use super::{peaks_from_chunks, Peaks, PEAK_BUCKET};

    fn samples_bytes(samples: &[f32]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    #[test]
    fn a_chunk_split_mid_sample_carries_its_bytes_across() {
        let mut samples = vec![0.5f32; PEAK_BUCKET];
        samples.extend(std::iter::repeat_n(0.25f32, PEAK_BUCKET));
        let bytes = samples_bytes(&samples);
        // Split inside a sample, so the decoder has to carry the tail bytes.
        let split = 10 * 4 + 2;
        let values = peaks_from_chunks([bytes[..split].to_vec(), bytes[split..].to_vec()]);
        assert_eq!(values.len(), 2);
        assert!((values[0] - 0.5).abs() < 1e-6);
        assert!((values[1] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_partial_final_bucket_is_kept() {
        let mut samples = vec![0.4f32; PEAK_BUCKET];
        samples.extend(std::iter::repeat_n(0.9f32, 7));
        let values = peaks_from_chunks([samples_bytes(&samples)]);
        assert_eq!(values.len(), 2);
        assert!((values[0] - 0.4).abs() < 1e-6);
        assert!((values[1] - 0.9).abs() < 1e-6);
    }

    #[test]
    fn levels_are_clamped_and_an_empty_stream_measures_nothing() {
        let loud = peaks_from_chunks([samples_bytes(&[2.0, -3.0])]);
        assert_eq!(
            loud,
            vec![1.0],
            "a clipped sample must not exceed full scale"
        );
        assert!(peaks_from_chunks(std::iter::empty::<Vec<u8>>()).is_empty());
        assert!(peaks_from_chunks([Vec::new()]).is_empty());
        assert!(
            peaks_from_chunks([vec![0u8; 3]]).is_empty(),
            "a trailing partial sample is not a level"
        );
    }

    #[test]
    fn level_takes_the_max_of_the_buckets_touched_and_reads_zero_outside() {
        let peaks = Peaks {
            bucket_seconds: 0.02,
            values: vec![0.1, 0.6, 0.3],
        };
        assert!((peaks.level(0.0, 0.02) - 0.1).abs() < 1e-6);
        assert!((peaks.level(0.0, 0.06) - 0.6).abs() < 1e-6);
        assert!((peaks.level(0.01, 0.03) - 0.6).abs() < 1e-6);
        // Past the measured data, inverted, or not a number at all.
        assert_eq!(peaks.level(1.0, 2.0), 0.0);
        assert_eq!(peaks.level(0.04, 0.02), 0.0);
        assert_eq!(peaks.level(f64::NAN, 0.02), 0.0);
        assert_eq!(peaks.level(-1.0, -0.5), 0.0);
        let empty = Peaks {
            bucket_seconds: 0.02,
            values: Vec::new(),
        };
        assert_eq!(empty.level(0.0, 1.0), 0.0);
    }
}
