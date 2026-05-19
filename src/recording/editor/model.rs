use std::path::{Path, PathBuf};

pub const MIN_TRIM_DURATION_SECONDS: f64 = 0.25;
const MIN_DIMENSION: u32 = 64;

#[derive(Debug, Clone)]
pub struct VideoMetadata {
    pub path: PathBuf,
    pub duration_seconds: f64,
    pub width: u32,
    pub height: u32,
    pub file_size_bytes: u64,
    pub has_audio: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionPreset {
    Original,
    P1080,
    P720,
    P480,
    Custom,
}

impl DimensionPreset {
    pub fn from_label(label: &str) -> Self {
        match label {
            "1920 x 1080" => Self::P1080,
            "1280 x 720" => Self::P720,
            "854 x 480" => Self::P480,
            "Custom" => Self::Custom,
            _ => Self::Original,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMode {
    Unchanged,
    Mono,
    Muted,
}

#[derive(Debug, Clone)]
pub struct VideoEditState {
    pub metadata: VideoMetadata,
    pub trim_start_seconds: f64,
    pub trim_end_seconds: f64,
    pub playhead_seconds: f64,
    pub dimension_preset: DimensionPreset,
    pub custom_width: u32,
    pub custom_height: u32,
    pub quality: u8,
    pub audio_mode: AudioMode,
    /// Sorted list of cut points (seconds) within the trim range.
    pub cuts: Vec<f64>,
    /// Whether each segment is kept (true) or removed (false).
    /// Length is always cuts.len() + 1.
    pub segments_kept: Vec<bool>,
    /// Output order of segments (indices into segment_boundaries()).
    /// Length is always cuts.len() + 1.
    pub segment_order: Vec<usize>,
}

impl VideoEditState {
    pub fn new(metadata: VideoMetadata) -> Self {
        Self {
            trim_start_seconds: 0.0,
            trim_end_seconds: metadata.duration_seconds,
            playhead_seconds: 0.0,
            custom_width: metadata.width,
            custom_height: metadata.height,
            metadata,
            dimension_preset: DimensionPreset::Original,
            quality: 70,
            audio_mode: AudioMode::Unchanged,
            cuts: Vec::new(),
            segments_kept: vec![true],
            segment_order: vec![0],
        }
    }

    pub fn set_trim_start(&mut self, value: f64) {
        let duration = self.metadata.duration_seconds.max(0.0);
        let max_start = if duration > MIN_TRIM_DURATION_SECONDS {
            self.trim_end_seconds - MIN_TRIM_DURATION_SECONDS
        } else {
            self.trim_end_seconds
        };
        self.trim_start_seconds = value.clamp(0.0, max_start.max(0.0));
    }

    pub fn set_trim_end(&mut self, value: f64) {
        let duration = self.metadata.duration_seconds.max(0.0);
        let min_end = if duration > MIN_TRIM_DURATION_SECONDS {
            self.trim_start_seconds + MIN_TRIM_DURATION_SECONDS
        } else {
            self.trim_start_seconds
        };
        self.trim_end_seconds = value.clamp(min_end.min(duration), duration);
    }

    pub fn trim_duration(&self) -> f64 {
        (self.trim_end_seconds - self.trim_start_seconds).max(0.0)
    }

    /// Duration of only the kept segments.
    pub fn kept_duration(&self) -> f64 {
        self.ordered_kept_segments()
            .iter()
            .map(|(start, end)| (end - start).max(0.0))
            .sum()
    }

    /// Returns (start, end) pairs for each segment.
    pub fn segment_boundaries(&self) -> Vec<(f64, f64)> {
        let mut boundaries = Vec::with_capacity(self.cuts.len() + 1);
        let mut prev = self.trim_start_seconds;
        for &cut in &self.cuts {
            boundaries.push((prev, cut));
            prev = cut;
        }
        boundaries.push((prev, self.trim_end_seconds));
        boundaries
    }

    /// Add a cut at the given time.
    pub fn add_cut(&mut self, seconds: f64) {
        if seconds <= self.trim_start_seconds + 0.1 || seconds >= self.trim_end_seconds - 0.1 {
            return;
        }
        // Don't add duplicate cuts (within 0.1s of existing)
        if self.cuts.iter().any(|&c| (c - seconds).abs() < 0.1) {
            return;
        }
        let insert_pos = self.cuts.partition_point(|&c| c < seconds);
        self.cuts.insert(insert_pos, seconds);
        // The segment at insert_pos gets split — new segment inherits kept state
        let was_kept = self.segments_kept.get(insert_pos).copied().unwrap_or(true);
        self.segments_kept.insert(insert_pos + 1, was_kept);
        // Update segment_order: shift indices >= insert_pos+1, insert new segment after original
        for idx in self.segment_order.iter_mut() {
            if *idx > insert_pos {
                *idx += 1;
            }
        }
        // Find where insert_pos is in segment_order and insert insert_pos+1 right after
        let order_pos = self
            .segment_order
            .iter()
            .position(|&i| i == insert_pos)
            .unwrap_or(self.segment_order.len());
        self.segment_order.insert(order_pos + 1, insert_pos + 1);
    }

    /// Remove a cut point by index.
    pub fn remove_cut(&mut self, cut_index: usize) {
        if cut_index >= self.cuts.len() {
            return;
        }
        self.cuts.remove(cut_index);
        // Merge the two segments — keep if either was kept
        let merged_seg = cut_index; // segment that remains
        let removed_seg = cut_index + 1; // segment that's absorbed
        let kept = self.segments_kept.get(merged_seg).copied().unwrap_or(true)
            || self.segments_kept.get(removed_seg).copied().unwrap_or(true);
        self.segments_kept.remove(removed_seg);
        if let Some(seg) = self.segments_kept.get_mut(merged_seg) {
            *seg = kept;
        }
        // Update segment_order: remove the absorbed segment, fix indices
        self.segment_order.retain(|&i| i != removed_seg);
        for idx in self.segment_order.iter_mut() {
            if *idx > removed_seg {
                *idx -= 1;
            }
        }
    }

    /// Move a cut point without crossing its neighboring cuts.
    pub fn move_cut(&mut self, cut_index: usize, seconds: f64) {
        if cut_index >= self.cuts.len() {
            return;
        }

        let min = if cut_index == 0 {
            self.trim_start_seconds + 0.1
        } else {
            self.cuts[cut_index - 1] + 0.1
        };
        let max = if cut_index + 1 >= self.cuts.len() {
            self.trim_end_seconds - 0.1
        } else {
            self.cuts[cut_index + 1] - 0.1
        };

        if min <= max {
            self.cuts[cut_index] = seconds.clamp(min, max);
        }
    }

    /// Toggle keep/remove for a segment.
    pub fn toggle_segment(&mut self, segment_index: usize) {
        if let Some(kept) = self.segments_kept.get_mut(segment_index) {
            *kept = !*kept;
        }
    }

    /// Clear all cuts.
    pub fn clear_cuts(&mut self) {
        self.cuts.clear();
        self.segments_kept = vec![true];
        self.segment_order = vec![0];
    }

    /// Move a segment from one position in the output order to another.
    pub fn move_segment(&mut self, from_order_pos: usize, to_order_pos: usize) {
        if from_order_pos >= self.segment_order.len()
            || to_order_pos >= self.segment_order.len()
            || from_order_pos == to_order_pos
        {
            return;
        }
        let seg = self.segment_order.remove(from_order_pos);
        self.segment_order.insert(to_order_pos, seg);
    }

    /// Returns kept segments in the user-defined output order (for export).
    pub fn ordered_kept_segments(&self) -> Vec<(f64, f64)> {
        let boundaries = self.segment_boundaries();
        self.segment_order
            .iter()
            .filter(|&&i| self.segments_kept.get(i).copied().unwrap_or(true))
            .filter_map(|&i| boundaries.get(i).copied())
            .collect()
    }

    /// Returns whether segments have been reordered from their default.
    pub fn is_reordered(&self) -> bool {
        self.segment_order
            .iter()
            .enumerate()
            .any(|(pos, &seg)| pos != seg)
    }

    pub fn target_dimensions(&self) -> (u32, u32) {
        let (width, height) = match self.dimension_preset {
            DimensionPreset::Original => (self.metadata.width, self.metadata.height),
            DimensionPreset::P1080 => (1920, 1080),
            DimensionPreset::P720 => (1280, 720),
            DimensionPreset::P480 => (854, 480),
            DimensionPreset::Custom => (self.custom_width, self.custom_height),
        };

        (
            even_dimension(width.clamp(MIN_DIMENSION, self.metadata.width.max(MIN_DIMENSION))),
            even_dimension(height.clamp(MIN_DIMENSION, self.metadata.height.max(MIN_DIMENSION))),
        )
    }

    pub fn estimated_size_bytes(&self, trim_only: bool) -> u64 {
        estimate_size_bytes(self, trim_only)
    }
}

pub fn even_dimension(value: u32) -> u32 {
    let clamped = value.max(MIN_DIMENSION);
    if clamped.is_multiple_of(2) {
        clamped
    } else {
        clamped - 1
    }
}

pub fn quality_to_crf(quality: u8) -> u8 {
    let quality = quality.min(100) as f64;
    (32.0 - ((quality / 100.0) * 14.0).round()).clamp(18.0, 32.0) as u8
}

pub fn estimate_size_bytes(state: &VideoEditState, trim_only: bool) -> u64 {
    let duration = state.metadata.duration_seconds.max(0.0);
    if duration <= f64::EPSILON {
        return 0;
    }

    let selected_duration_ratio = (state.kept_duration() / duration).clamp(0.0, 1.0);
    let base_size = state.metadata.file_size_bytes as f64 * selected_duration_ratio;

    if trim_only {
        return base_size.round().max(0.0) as u64;
    }

    let quality_factor = 0.55 + (state.quality.min(100) as f64 / 100.0) * 0.9;
    let (target_width, target_height) = state.target_dimensions();
    let original_pixels = (state.metadata.width as f64 * state.metadata.height as f64).max(1.0);
    let target_pixels = target_width as f64 * target_height as f64;
    let dimension_factor = (target_pixels / original_pixels).max(0.0);
    let audio_factor = match state.audio_mode {
        AudioMode::Unchanged => 1.0,
        AudioMode::Mono => 0.95,
        AudioMode::Muted => 0.88,
    };

    (base_size * quality_factor * dimension_factor * audio_factor)
        .round()
        .max(0.0) as u64
}

pub fn format_size(bytes: u64) -> String {
    let mb = bytes as f64 / 1024.0 / 1024.0;
    if mb < 10.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{mb:.0} MB")
    }
}

pub fn edited_output_path(input: &Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new(""));
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("recording");

    let mut candidate = parent.join(format!("{stem}-edited.mp4"));
    if !candidate.exists() {
        return candidate;
    }

    for index in 2.. {
        candidate = parent.join(format!("{stem}-edited-{index}.mp4"));
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("unbounded edited output path search should always return")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn metadata() -> VideoMetadata {
        VideoMetadata {
            path: PathBuf::from("/tmp/input.mp4"),
            duration_seconds: 10.0,
            width: 1920,
            height: 1080,
            file_size_bytes: 100 * 1024 * 1024,
            has_audio: true,
        }
    }

    #[test]
    fn output_path_adds_edited_suffix() {
        let path = PathBuf::from("/tmp/ApexShot Recording.mp4");
        assert_eq!(
            edited_output_path(&path),
            PathBuf::from("/tmp/ApexShot Recording-edited.mp4")
        );
    }

    #[test]
    fn output_path_increments_when_existing_file_present() {
        let dir =
            std::env::temp_dir().join(format!("apexshot-video-editor-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("recording.mp4");
        fs::write(dir.join("recording-edited.mp4"), b"existing").unwrap();
        fs::write(dir.join("recording-edited-2.mp4"), b"existing").unwrap();

        assert_eq!(
            edited_output_path(&input),
            dir.join("recording-edited-3.mp4")
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn trim_range_clamps_to_duration() {
        let mut state = VideoEditState::new(metadata());
        state.set_trim_start(-10.0);
        state.set_trim_end(50.0);

        assert_eq!(state.trim_start_seconds, 0.0);
        assert_eq!(state.trim_end_seconds, 10.0);
    }

    #[test]
    fn trim_range_enforces_min_duration() {
        let mut state = VideoEditState::new(metadata());
        state.set_trim_start(9.95);

        assert_eq!(state.trim_start_seconds, 9.75);
        state.set_trim_end(9.8);
        assert_eq!(state.trim_end_seconds, 10.0);
    }

    #[test]
    fn move_cut_keeps_cut_between_neighbors() {
        let mut state = VideoEditState::new(metadata());
        state.add_cut(3.0);
        state.add_cut(7.0);

        state.move_cut(0, 6.0);
        assert_eq!(state.cuts, vec![6.0, 7.0]);

        state.move_cut(0, 8.0);
        assert!((state.cuts[0] - 6.9).abs() < f64::EPSILON);
        assert_eq!(state.cuts[1], 7.0);

        state.move_cut(1, 0.0);
        assert!((state.cuts[0] - 6.9).abs() < f64::EPSILON);
        assert_eq!(state.cuts[1], 7.0);
    }

    #[test]
    fn quality_maps_to_expected_crf_values() {
        assert_eq!(quality_to_crf(100), 18);
        assert_eq!(quality_to_crf(70), 22);
        assert_eq!(quality_to_crf(0), 32);
    }

    #[test]
    fn dimension_preset_original_uses_source_dimensions() {
        let state = VideoEditState::new(metadata());
        assert_eq!(state.target_dimensions(), (1920, 1080));
    }

    #[test]
    fn dimension_preset_clamps_to_even_dimensions() {
        let mut state = VideoEditState::new(metadata());
        state.dimension_preset = DimensionPreset::Custom;
        state.custom_width = 1919;
        state.custom_height = 57;

        assert_eq!(state.target_dimensions(), (1918, 64));
    }

    #[test]
    fn estimate_size_scales_with_trim_duration() {
        let full = VideoEditState::new(metadata());
        let mut half = full.clone();
        half.set_trim_end(5.0);

        assert!(half.estimated_size_bytes(true) < full.estimated_size_bytes(true));
        assert_eq!(
            half.estimated_size_bytes(true),
            full.metadata.file_size_bytes / 2
        );
    }

    #[test]
    fn estimate_size_scales_with_dimensions() {
        let original = VideoEditState::new(metadata());
        let mut smaller = original.clone();
        smaller.dimension_preset = DimensionPreset::P720;

        assert!(smaller.estimated_size_bytes(false) < original.estimated_size_bytes(false));
    }
}
