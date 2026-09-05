use super::model::VideoMetadata;
use super::sidecar::{
    CaptureRegion, CursorKind, PointerSample, PointerSidecar, MAX_POINTER_SAMPLES,
};
use anyhow::{anyhow, bail, Context};
use std::io::{ErrorKind, Read};
use std::process::{Command, Stdio};

const TARGET_SAMPLE_RATE: f64 = 12.0;
const MIN_SAMPLE_RATE: f64 = 2.0;
const MAX_ANALYSIS_DIMENSION: u32 = 640;
const CHANGE_THRESHOLD: u8 = 32;
const CELL_SIZE: usize = 16;
const CELL_RADIUS: usize = 2;
const MIN_TRACK_OBSERVATIONS: usize = 3;
const MIN_INFERRED_HOLD_SECONDS: f64 = 0.5;
const MAX_STDERR_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy)]
struct AnalysisGeometry {
    width: usize,
    height: usize,
    source_width: u32,
    source_height: u32,
}

#[derive(Debug, Clone, Copy)]
struct MotionEvidence {
    frame_index: usize,
    segment: usize,
    t: f64,
    x: f64,
    y: f64,
    dominance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DifferenceKind {
    NoMotion,
    Broad,
}

#[derive(Debug)]
struct AnalysisAccumulator {
    geometry: AnalysisGeometry,
    sample_rate: f64,
    observations: Vec<MotionEvidence>,
    segment: usize,
    transitions: usize,
}

impl AnalysisAccumulator {
    fn new(geometry: AnalysisGeometry, sample_rate: f64) -> Self {
        Self {
            geometry,
            sample_rate,
            observations: Vec::new(),
            segment: 0,
            transitions: 0,
        }
    }

    fn push(&mut self, previous: &[u8], current: &[u8], frame_index: usize) {
        self.transitions += 1;
        match localized_difference(previous, current, self.geometry) {
            Ok((x, y, dominance)) => self.observations.push(MotionEvidence {
                frame_index,
                segment: self.segment,
                t: frame_index as f64 / self.sample_rate,
                x,
                y,
                dominance,
            }),
            Err(DifferenceKind::Broad) => {
                self.segment = self.segment.saturating_add(1);
            }
            Err(DifferenceKind::NoMotion) => {}
        }
    }

    fn finish(self, final_time: f64) -> anyhow::Result<Vec<PointerSample>> {
        if self.transitions < MIN_TRACK_OBSERVATIONS + 2 {
            bail!("not enough decoded frames to analyze pointer motion");
        }
        if self.observations.len() < MIN_TRACK_OBSERVATIONS {
            bail!("insufficient high-confidence localized pointer motion");
        }

        let max_step = (self.geometry.width.max(self.geometry.height) as f64 * 0.20).max(8.0);
        let mut runs: Vec<Vec<MotionEvidence>> = Vec::new();
        for observation in self.observations.iter().copied() {
            let continues = runs
                .last()
                .and_then(|run| run.last())
                .map(|previous| {
                    previous.segment == observation.segment
                        && distance(previous.x, previous.y, observation.x, observation.y)
                            <= max_step
                })
                .unwrap_or(false);
            if continues {
                runs.last_mut().expect("run exists").push(observation);
            } else {
                runs.push(vec![observation]);
            }
        }

        let minimum_extent =
            (self.geometry.width.min(self.geometry.height) as f64 * 0.015).max(3.0);
        let mut qualified_runs = Vec::new();
        for run in runs {
            if run.len() < MIN_TRACK_OBSERVATIONS {
                continue;
            }
            let path_length: f64 = run
                .windows(2)
                .map(|pair| distance(pair[0].x, pair[0].y, pair[1].x, pair[1].y))
                .sum();
            let (min_x, max_x, min_y, max_y) = run.iter().fold(
                (
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                ),
                |(min_x, max_x, min_y, max_y), item| {
                    (
                        min_x.min(item.x),
                        max_x.max(item.x),
                        min_y.min(item.y),
                        max_y.max(item.y),
                    )
                },
            );
            let extent = (max_x - min_x).hypot(max_y - min_y);
            let duration = run.last().expect("non-empty run").t - run[0].t;
            let average_dominance =
                run.iter().map(|item| item.dominance).sum::<f64>() / run.len() as f64;
            if path_length < minimum_extent
                || extent < minimum_extent
                || duration + f64::EPSILON < 2.0 / self.sample_rate
                || average_dominance < 0.82
            {
                continue;
            }
            qualified_runs.push(run);
        }

        if qualified_runs.is_empty() {
            bail!("pointer-motion confidence is insufficient");
        }

        let capacity = qualified_runs
            .iter()
            .map(|run| run.len().saturating_add(2))
            .sum();
        let mut samples = Vec::with_capacity(capacity);
        let mut stable_landings = 0;
        for run in qualified_runs {
            let first_velocity = (run[1].x - run[0].x, run[1].y - run[0].y);
            let initial_t = (run[0].t - 1.0 / self.sample_rate).max(0.0);
            push_mapped_sample(
                &mut samples,
                initial_t,
                run[0].x - first_velocity.0 * 0.5,
                run[0].y - first_velocity.1 * 0.5,
                self.geometry,
            );
            for observation in &run {
                push_mapped_sample(
                    &mut samples,
                    observation.t,
                    observation.x,
                    observation.y,
                    self.geometry,
                );
            }

            let last = run.last().expect("validated run");
            let next_evidence_time = self
                .observations
                .iter()
                .find(|item| item.frame_index > last.frame_index)
                .map(|item| item.t)
                .unwrap_or(final_time);
            let hold_end = (last.t + MIN_INFERRED_HOLD_SECONDS).min(final_time);
            if hold_end - last.t + f64::EPSILON >= MIN_INFERRED_HOLD_SECONDS
                && hold_end + 1.0 / self.sample_rate <= next_evidence_time + f64::EPSILON
            {
                push_mapped_sample(&mut samples, hold_end, last.x, last.y, self.geometry);
                stable_landings += 1;
            }
        }
        if stable_landings == 0 {
            bail!("no stable pointer landing was visible after localized motion");
        }

        normalize_samples(&mut samples);
        Ok(samples)
    }
}

/// Infers a conservative pointer-motion sidecar from an imported video.
///
/// This deliberately returns an error instead of guessing when localized motion cannot be
/// distinguished from scene content. Clicks cannot be inferred reliably from video pixels and
/// are therefore never generated.
pub fn analyze(metadata: &VideoMetadata) -> anyhow::Result<PointerSidecar> {
    validate_metadata(metadata)?;
    let geometry = analysis_geometry(metadata.width, metadata.height)?;
    let sample_rate = sample_rate(metadata.duration_seconds)?;
    let filter = format!(
        "setpts=PTS-STARTPTS,fps=fps={sample_rate:.6}:start_time=0:round=near,scale={}:{}:flags=area,format=gray",
        geometry.width, geometry.height
    );

    let mut child = Command::new("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(&metadata.path)
        .args([
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-vf",
            &filter,
            "-frames:v",
            &MAX_POINTER_SAMPLES.to_string(),
            "-pix_fmt",
            "gray",
            "-f",
            "rawvideo",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| {
            format!(
                "failed to start ffmpeg while analyzing {}",
                metadata.path.display()
            )
        })?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("failed to capture ffmpeg video output"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("failed to capture ffmpeg error output"))?;
    let stderr_reader = std::thread::Builder::new()
        .name("imported-pointer-ffmpeg-stderr".to_owned())
        .spawn(move || read_limited(stderr, MAX_STDERR_BYTES));
    let stderr_reader = match stderr_reader {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error).context("failed to start ffmpeg error reader");
        }
    };

    let decoded = analyze_stream(&mut stdout, geometry, sample_rate, MAX_POINTER_SAMPLES);
    drop(stdout);
    if decoded.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().context("failed to wait for ffmpeg")?;
    let stderr = stderr_reader.join().unwrap_or_default();
    let (accumulator, decoded_frames) = match decoded {
        Ok(decoded) => decoded,
        Err(error) => {
            let detail = String::from_utf8_lossy(&stderr);
            if detail.trim().is_empty() {
                return Err(error);
            }
            return Err(error).context(format!("ffmpeg reported: {}", detail.trim()));
        }
    };
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr);
        bail!("ffmpeg pointer analysis failed: {}", detail.trim());
    }
    let covered_seconds = decoded_frames as f64 / sample_rate;
    if covered_seconds + 2.0 / sample_rate < metadata.duration_seconds {
        bail!("ffmpeg output did not cover enough of the source video");
    }
    let final_time = metadata
        .duration_seconds
        .min(decoded_frames.saturating_sub(1) as f64 / sample_rate);
    let pointer = accumulator.finish(final_time)?;

    let mut sidecar = PointerSidecar::new(
        0,
        CaptureRegion {
            x: 0,
            y: 0,
            w: metadata.width,
            h: metadata.height,
        },
    );
    sidecar.pointer = pointer;
    sidecar.mark_inferred_from_video();
    Ok(sidecar)
}

fn validate_metadata(metadata: &VideoMetadata) -> anyhow::Result<()> {
    if metadata.width == 0 || metadata.height == 0 {
        bail!("cannot analyze a video with empty dimensions");
    }
    if !metadata.duration_seconds.is_finite() || metadata.duration_seconds <= 0.0 {
        bail!("cannot analyze a video with an invalid duration");
    }
    if !metadata.path.is_file() {
        bail!("video does not exist: {}", metadata.path.display());
    }
    Ok(())
}

fn analysis_geometry(source_width: u32, source_height: u32) -> anyhow::Result<AnalysisGeometry> {
    if source_width == 0 || source_height == 0 {
        bail!("video dimensions must be non-zero");
    }
    let scale = (MAX_ANALYSIS_DIMENSION as f64 / source_width.max(source_height) as f64).min(1.0);
    let width = (source_width as f64 * scale).round().max(1.0) as usize;
    let height = (source_height as f64 * scale).round().max(1.0) as usize;
    if width < 16 || height < 16 {
        bail!("video dimensions are too narrow for reliable pointer analysis");
    }
    Ok(AnalysisGeometry {
        width,
        height,
        source_width,
        source_height,
    })
}

fn sample_rate(duration_seconds: f64) -> anyhow::Result<f64> {
    let capped_rate =
        (MAX_POINTER_SAMPLES.saturating_sub(1) as f64 / duration_seconds).min(TARGET_SAMPLE_RATE);
    if capped_rate < MIN_SAMPLE_RATE {
        bail!("video is too long for reliable bounded pointer analysis");
    }
    Ok(capped_rate)
}

fn analyze_stream(
    reader: &mut impl Read,
    geometry: AnalysisGeometry,
    sample_rate: f64,
    frame_limit: usize,
) -> anyhow::Result<(AnalysisAccumulator, usize)> {
    let frame_bytes = geometry
        .width
        .checked_mul(geometry.height)
        .ok_or_else(|| anyhow!("analysis frame dimensions overflow"))?;
    let mut previous = vec![0_u8; frame_bytes];
    let mut current = vec![0_u8; frame_bytes];
    if !read_frame(reader, &mut previous)? {
        bail!("ffmpeg decoded no video frames");
    }
    let mut decoded_frames = 1;
    let mut accumulator = AnalysisAccumulator::new(geometry, sample_rate);
    while decoded_frames < frame_limit && read_frame(reader, &mut current)? {
        accumulator.push(&previous, &current, decoded_frames);
        decoded_frames += 1;
        std::mem::swap(&mut previous, &mut current);
    }
    Ok((accumulator, decoded_frames))
}

fn read_frame(reader: &mut impl Read, frame: &mut [u8]) -> anyhow::Result<bool> {
    let mut offset = 0;
    while offset < frame.len() {
        match reader.read(&mut frame[offset..]) {
            Ok(0) if offset == 0 => return Ok(false),
            Ok(0) => bail!("ffmpeg produced a truncated grayscale frame"),
            Ok(count) => offset += count,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error).context("failed to read ffmpeg video output"),
        }
    }
    Ok(true)
}

fn read_limited(mut reader: impl Read, limit: usize) -> Vec<u8> {
    let mut retained = Vec::with_capacity(limit.min(4096));
    let mut buffer = [0_u8; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                let keep = count.min(limit.saturating_sub(retained.len()));
                retained.extend_from_slice(&buffer[..keep]);
            }
        }
    }
    retained
}

fn localized_difference(
    previous: &[u8],
    current: &[u8],
    geometry: AnalysisGeometry,
) -> Result<(f64, f64, f64), DifferenceKind> {
    debug_assert_eq!(previous.len(), geometry.width * geometry.height);
    debug_assert_eq!(current.len(), previous.len());
    let area = previous.len();
    let minimum_changed = (area / 100_000).max(8);
    let maximum_changed = (area / 20).max(64);
    let bins_w = geometry.width.div_ceil(CELL_SIZE);
    let bins_h = geometry.height.div_ceil(CELL_SIZE);
    let mut bins = vec![0_usize; bins_w * bins_h];
    let mut changed = Vec::new();

    for (index, (&before, &after)) in previous.iter().zip(current).enumerate() {
        let difference = before.abs_diff(after);
        if difference < CHANGE_THRESHOLD {
            continue;
        }
        let x = index % geometry.width;
        let y = index / geometry.width;
        bins[(y / CELL_SIZE) * bins_w + x / CELL_SIZE] += 1;
        changed.push((x, y, difference as usize));
        if changed.len() > maximum_changed {
            return Err(DifferenceKind::Broad);
        }
    }
    if changed.len() < minimum_changed {
        return Err(DifferenceKind::NoMotion);
    }

    let mut best_cell = (0, 0);
    let mut best_count = 0;
    for cell_y in 0..bins_h {
        for cell_x in 0..bins_w {
            let min_x = cell_x.saturating_sub(CELL_RADIUS);
            let max_x = (cell_x + CELL_RADIUS).min(bins_w - 1);
            let min_y = cell_y.saturating_sub(CELL_RADIUS);
            let max_y = (cell_y + CELL_RADIUS).min(bins_h - 1);
            let mut count = 0;
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    count += bins[y * bins_w + x];
                }
            }
            if count > best_count {
                best_count = count;
                best_cell = (cell_x, cell_y);
            }
        }
    }
    let dominance = best_count as f64 / changed.len() as f64;
    if dominance < 0.82 || best_count < minimum_changed {
        return Err(DifferenceKind::Broad);
    }

    let min_cell_x = best_cell.0.saturating_sub(CELL_RADIUS);
    let max_cell_x = (best_cell.0 + CELL_RADIUS).min(bins_w - 1);
    let min_cell_y = best_cell.1.saturating_sub(CELL_RADIUS);
    let max_cell_y = (best_cell.1 + CELL_RADIUS).min(bins_h - 1);
    let mut min_x = usize::MAX;
    let mut max_x = 0;
    let mut min_y = usize::MAX;
    let mut max_y = 0;
    let mut weighted_x = 0_f64;
    let mut weighted_y = 0_f64;
    let mut total_weight = 0_f64;
    for (x, y, weight) in changed {
        if x / CELL_SIZE < min_cell_x
            || x / CELL_SIZE > max_cell_x
            || y / CELL_SIZE < min_cell_y
            || y / CELL_SIZE > max_cell_y
        {
            continue;
        }
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
        weighted_x += (x as f64 + 0.5) * weight as f64;
        weighted_y += (y as f64 + 0.5) * weight as f64;
        total_weight += weight as f64;
    }
    let box_width = max_x.saturating_sub(min_x) + 1;
    let box_height = max_y.saturating_sub(min_y) + 1;
    let box_area = box_width.saturating_mul(box_height);
    let maximum_span = geometry.width.max(geometry.height) / 5;
    if min_x == usize::MAX
        || box_width > maximum_span.max(16)
        || box_height > maximum_span.max(16)
        || best_count * 40 < box_area
        || total_weight == 0.0
    {
        return Err(DifferenceKind::Broad);
    }

    Ok((
        weighted_x / total_weight,
        weighted_y / total_weight,
        dominance,
    ))
}

fn push_mapped_sample(
    samples: &mut Vec<PointerSample>,
    t: f64,
    x: f64,
    y: f64,
    geometry: AnalysisGeometry,
) {
    let mapped_x = (x * geometry.source_width as f64 / geometry.width as f64)
        .clamp(0.0, geometry.source_width.saturating_sub(1) as f64);
    let mapped_y = (y * geometry.source_height as f64 / geometry.height as f64)
        .clamp(0.0, geometry.source_height.saturating_sub(1) as f64);
    samples.push(PointerSample {
        t: t.max(0.0),
        x: mapped_x,
        y: mapped_y,
        kind: CursorKind::Default,
    });
}

fn normalize_samples(samples: &mut Vec<PointerSample>) {
    samples.sort_by(|left, right| left.t.total_cmp(&right.t));
    samples.dedup_by(|right, left| {
        (right.t - left.t).abs() < 1.0e-9
            && (right.x - left.x).abs() < 1.0e-9
            && (right.y - left.y).abs() < 1.0e-9
    });
    if samples.len() <= MAX_POINTER_SAMPLES {
        return;
    }
    let last_index = samples.len() - 1;
    let mut reduced = Vec::with_capacity(MAX_POINTER_SAMPLES);
    for output_index in 0..MAX_POINTER_SAMPLES {
        let source_index = output_index * last_index / (MAX_POINTER_SAMPLES - 1);
        reduced.push(samples[source_index].clone());
    }
    *samples = reduced;
}

fn distance(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    (x2 - x1).hypot(y2 - y1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn geometry(width: usize, height: usize) -> AnalysisGeometry {
        AnalysisGeometry {
            width,
            height,
            source_width: width as u32,
            source_height: height as u32,
        }
    }

    fn frame(width: usize, height: usize, cursor: Option<(usize, usize)>) -> Vec<u8> {
        let mut pixels = vec![220_u8; width * height];
        if let Some((cursor_x, cursor_y)) = cursor {
            for y in cursor_y..(cursor_y + 8).min(height) {
                for x in cursor_x..(cursor_x + 6).min(width) {
                    if x == cursor_x || y == cursor_y || x <= cursor_x + (y - cursor_y) / 2 {
                        pixels[y * width + x] = 20;
                    }
                }
            }
        }
        pixels
    }

    fn analyze_frames(
        frames: &[Vec<u8>],
        geometry: AnalysisGeometry,
        sample_rate: f64,
    ) -> anyhow::Result<Vec<PointerSample>> {
        let mut accumulator = AnalysisAccumulator::new(geometry, sample_rate);
        for (index, pair) in frames.windows(2).enumerate() {
            accumulator.push(&pair[0], &pair[1], index + 1);
        }
        accumulator.finish((frames.len() - 1) as f64 / sample_rate)
    }

    #[test]
    fn tracks_a_moving_then_pausing_cursor() {
        let positions = [8, 12, 16, 20, 24, 24, 24, 24, 24, 24, 24];
        let frames: Vec<_> = positions
            .into_iter()
            .map(|x| frame(64, 48, Some((x, 17))))
            .collect();

        let samples = analyze_frames(&frames, geometry(64, 48), 10.0).unwrap();

        assert!(samples.len() >= 6);
        assert!(samples.windows(2).all(|pair| pair[0].t < pair[1].t));
        assert!(samples[3].x > samples[1].x);
        let last = &samples[samples.len() - 1];
        let before_last = &samples[samples.len() - 2];
        assert!((last.x - before_last.x).abs() < 1.0e-9);
        assert!(last.t > before_last.t);
        assert!(samples
            .iter()
            .all(|sample| sample.kind == CursorKind::Default));
    }

    #[test]
    fn rejects_broad_scene_changes_and_no_motion() {
        let still = frame(64, 48, None);
        let broad = vec![15_u8; 64 * 48];
        let broad_frames = vec![
            still.clone(),
            broad.clone(),
            still.clone(),
            broad,
            still.clone(),
        ];
        assert!(analyze_frames(&broad_frames, geometry(64, 48), 10.0).is_err());

        let still_frames = vec![
            still.clone(),
            still.clone(),
            still.clone(),
            still.clone(),
            still,
        ];
        assert!(analyze_frames(&still_frames, geometry(64, 48), 10.0).is_err());
    }

    #[test]
    fn accepts_coherent_pointer_motion_despite_unrelated_scene_changes() {
        let still = frame(64, 48, None);
        let broad = vec![80_u8; 64 * 48];
        let mut frames = vec![still.clone(), broad.clone(), still.clone(), broad, still];
        frames.extend(
            [8, 18, 28, 38, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48]
                .into_iter()
                .map(|x| frame(64, 48, Some((x, 17)))),
        );

        let samples = analyze_frames(&frames, geometry(64, 48), 10.0).unwrap();

        assert!(samples.len() >= 6);
        assert!(samples.last().unwrap().x > samples.first().unwrap().x);
        let mut sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 64,
                h: 48,
            },
        );
        sidecar.pointer = samples;
        sidecar.mark_inferred_from_video();
        assert!(
            !super::super::zoom_suggest::suggest_zooms(&sidecar, 64.0, 48.0, 1.8).is_empty(),
            "samples: {:?}",
            sidecar.pointer
        );
    }

    #[test]
    fn maps_analysis_coordinates_to_source_pixels() {
        let mut samples = Vec::new();
        push_mapped_sample(
            &mut samples,
            0.25,
            32.0,
            18.0,
            AnalysisGeometry {
                width: 64,
                height: 36,
                source_width: 1920,
                source_height: 1080,
            },
        );

        assert_eq!(samples[0].x, 960.0);
        assert_eq!(samples[0].y, 540.0);
        assert_eq!(samples[0].t, 0.25);
    }

    #[test]
    fn rejects_insufficient_motion_confidence() {
        let frames = vec![
            frame(64, 48, Some((8, 17))),
            frame(64, 48, Some((12, 17))),
            frame(64, 48, Some((16, 17))),
            frame(64, 48, Some((16, 17))),
            frame(64, 48, Some((16, 17))),
        ];

        assert!(analyze_frames(&frames, geometry(64, 48), 10.0).is_err());
    }

    #[test]
    fn analyzes_a_real_ffmpeg_video_stream() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        // The ffmpeg child must see the fixture too. Cargo's sandbox can
        // isolate `/tmp`, while the workspace target directory is shared.
        let fixture_dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test-fixtures");
        std::fs::create_dir_all(&fixture_dir).unwrap();
        let path = fixture_dir.join(format!(
            "apexshot-imported-pointer-{}-{:?}.mp4",
            std::process::id(),
            std::thread::current().id()
        ));
        let raw_path = path.with_extension("gray");
        let mut frames = Vec::with_capacity(160 * 120 * 24);
        for index in 0..24 {
            let x = 10 + index.min(11) * 7;
            frames.extend_from_slice(&frame(160, 120, Some((x, 50))));
        }
        std::fs::write(&raw_path, frames).unwrap();
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "rawvideo",
                "-pixel_format",
                "gray",
                "-video_size",
                "160x120",
                "-framerate",
                "12",
                "-i",
            ])
            .arg(&raw_path)
            .args(["-c:v", "mpeg4"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());

        let sidecar = analyze(&VideoMetadata {
            path: PathBuf::from(&path),
            duration_seconds: 2.0,
            width: 160,
            height: 120,
            file_size_bytes: std::fs::metadata(&path).unwrap().len(),
            has_audio: false,
        })
        .unwrap();
        assert_eq!(
            sidecar.source,
            super::super::sidecar::PointerDataSource::InferredFromVideo
        );
        assert!(sidecar.pointer.len() >= 5);
        assert!(sidecar.clicks.is_empty());
        assert!(sidecar.pointer.windows(2).all(|pair| pair[0].t < pair[1].t));

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(raw_path);
    }
}
