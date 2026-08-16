use super::sidecar::PointerSidecar;
use std::path::{Path, PathBuf};

pub const MIN_TRIM_DURATION_SECONDS: f64 = 0.25;
const MIN_DIMENSION: u32 = 64;
pub const DEFAULT_ZOOM_DURATION_SECONDS: f64 = 1.8;
pub const DEFAULT_ZOOM_SCALE: f64 = 1.8;
pub const DEFAULT_ZOOM_EASE_MS: u32 = 200;
pub const MIN_ZOOM_SCALE: f64 = 1.2;
pub const MAX_ZOOM_SCALE: f64 = 3.0;

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VideoBackground {
    None,
    Plain { r: u8, g: u8, b: u8 },
    Gradient(usize),
}

impl VideoBackground {
    pub fn is_none(self) -> bool {
        matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ZoomClip {
    pub start: f64,
    pub end: f64,
    pub scale: f64,
    pub center: (f64, f64),
    pub ease_ms: u32,
}

impl ZoomClip {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectMediaKind {
    Video,
    Audio,
    Image,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMedia {
    pub path: PathBuf,
    pub display_name: String,
    pub kind: ProjectMediaKind,
    pub duration_seconds: Option<f64>,
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
    pub zoom_clips: Vec<ZoomClip>,
    pub selected_zoom: Option<usize>,
    pub background: VideoBackground,
    pub background_padding: f64,
    pub background_corner_radius: f64,
    pub background_shadow: f64,
    pub sidecar: Option<PointerSidecar>,
    pub project_media: Vec<ProjectMedia>,
}

fn seed_project_media(metadata: &VideoMetadata) -> Vec<ProjectMedia> {
    let name = metadata
        .path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Recording")
        .to_string();
    let mut items = vec![ProjectMedia {
        path: metadata.path.clone(),
        display_name: name.clone(),
        kind: ProjectMediaKind::Video,
        duration_seconds: Some(metadata.duration_seconds),
    }];
    if metadata.has_audio {
        items.push(ProjectMedia {
            path: metadata.path.clone(),
            display_name: format!("{name} audio"),
            kind: ProjectMediaKind::Audio,
            duration_seconds: Some(metadata.duration_seconds),
        });
    }
    items
}

impl VideoEditState {
    pub fn new(metadata: VideoMetadata) -> Self {
        let sidecar = PointerSidecar::load_next_to_video(&metadata.path);
        let project_media = seed_project_media(&metadata);
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
            zoom_clips: Vec::new(),
            selected_zoom: None,
            background: VideoBackground::None,
            background_padding: 24.0,
            background_corner_radius: 18.0,
            background_shadow: 15.0,
            sidecar,
            project_media,
        }
    }

    pub fn add_project_media(&mut self, item: ProjectMedia) {
        if self
            .project_media
            .iter()
            .any(|existing| existing.path == item.path && existing.kind == item.kind)
        {
            return;
        }
        self.project_media.push(item);
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

    pub fn shift_trim(&mut self, delta: f64) {
        let duration = self.metadata.duration_seconds.max(0.0);
        let span = self.trim_duration();
        if span <= 0.0 || duration <= 0.0 {
            return;
        }
        let max_start = (duration - span).max(0.0);
        let new_start = (self.trim_start_seconds + delta).clamp(0.0, max_start);
        let shift = new_start - self.trim_start_seconds;
        if shift.abs() < f64::EPSILON {
            return;
        }
        self.trim_start_seconds = new_start;
        self.trim_end_seconds = new_start + span;
        for cut in &mut self.cuts {
            *cut += shift;
        }
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
        let src_w = self.metadata.width.max(1);
        let src_h = self.metadata.height.max(1);
        match self.dimension_preset {
            DimensionPreset::Original => (even_dimension(src_w), even_dimension(src_h)),
            DimensionPreset::P1080 => fit_dimensions(src_w, src_h, 1920, 1080),
            DimensionPreset::P720 => fit_dimensions(src_w, src_h, 1280, 720),
            DimensionPreset::P480 => fit_dimensions(src_w, src_h, 854, 480),
            DimensionPreset::Custom => fit_dimensions(
                src_w,
                src_h,
                self.custom_width.max(MIN_DIMENSION),
                self.custom_height.max(MIN_DIMENSION),
            ),
        }
    }

    /// True when quality/dimensions/zoom/pad require a re-encode (stream-copy cannot apply them).
    pub fn needs_reencode(&self) -> bool {
        if self.needs_composite() {
            return true;
        }
        let (tw, th) = self.target_dimensions();
        let (sw, sh) = (
            even_dimension(self.metadata.width.max(1)),
            even_dimension(self.metadata.height.max(1)),
        );
        if tw != sw || th != sh {
            return true;
        }
        // Quality only takes effect when re-encoding.
        self.quality != 70
    }

    pub fn needs_composite(&self) -> bool {
        !self.zoom_clips.is_empty()
            || !self.background.is_none()
            || self
                .sidecar
                .as_ref()
                .is_some_and(|sidecar| !sidecar.pointer.is_empty())
    }

    pub fn default_zoom_center(&self, at_seconds: f64) -> (f64, f64) {
        if let Some(sidecar) = &self.sidecar {
            if let Some((x, y, _)) = sidecar.interpolated_at(at_seconds) {
                return (x, y);
            }
        }
        (
            self.metadata.width as f64 / 2.0,
            self.metadata.height as f64 / 2.0,
        )
    }

    pub fn add_zoom_at_playhead(&mut self) -> Option<usize> {
        let start = self
            .playhead_seconds
            .clamp(self.trim_start_seconds, self.trim_end_seconds);
        let end = (start + DEFAULT_ZOOM_DURATION_SECONDS).min(self.trim_end_seconds);
        if end - start < 0.2 {
            return None;
        }
        if self
            .zoom_clips
            .iter()
            .any(|clip| ranges_overlap(start, end, clip.start, clip.end))
        {
            return None;
        }
        let center = self.default_zoom_center(start);
        self.zoom_clips.push(ZoomClip {
            start,
            end,
            scale: DEFAULT_ZOOM_SCALE,
            center,
            ease_ms: DEFAULT_ZOOM_EASE_MS,
        });
        self.zoom_clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        let index = self
            .zoom_clips
            .iter()
            .position(|clip| (clip.start - start).abs() < 1e-6)?;
        self.selected_zoom = Some(index);
        Some(index)
    }

    pub fn remove_selected_zoom(&mut self) {
        if let Some(index) = self.selected_zoom.take() {
            if index < self.zoom_clips.len() {
                self.zoom_clips.remove(index);
            }
        }
    }

    pub fn set_zoom_range(&mut self, index: usize, start: f64, end: f64) {
        if self.zoom_clips.get(index).is_none() {
            return;
        }
        let min_start = self.trim_start_seconds;
        let max_end = self.trim_end_seconds;
        let mut start = start.clamp(min_start, max_end);
        let mut end = end.clamp(min_start, max_end);
        if end - start < 0.2 {
            end = (start + 0.2).min(max_end);
            start = (end - 0.2).max(min_start);
        }
        if self.zoom_clips.iter().enumerate().any(|(other, existing)| {
            other != index && ranges_overlap(start, end, existing.start, existing.end)
        }) {
            return;
        }
        if let Some(clip) = self.zoom_clips.get_mut(index) {
            clip.start = start;
            clip.end = end;
        }
    }

    pub fn eval_zoom(&self, t: f64) -> (f64, (f64, f64)) {
        eval_zoom(
            &self.zoom_clips,
            t,
            self.metadata.width as f64,
            self.metadata.height as f64,
        )
    }

    pub fn padded_output_dimensions(&self) -> (u32, u32) {
        let (base_w, base_h) = self.target_dimensions();
        if self.background.is_none() {
            return (base_w, base_h);
        }
        let layout = crate::capture::editor::composition::BackgroundComposition::new(
            base_w as f64,
            base_h as f64,
        )
        .with_style(match self.background {
            VideoBackground::None => crate::capture::editor::types::BackgroundStyle::None,
            VideoBackground::Plain { r, g, b } => {
                crate::capture::editor::types::BackgroundStyle::PlainColor(
                    crate::capture::editor::types::DrawColor::new(
                        r as f64 / 255.0,
                        g as f64 / 255.0,
                        b as f64 / 255.0,
                        1.0,
                    ),
                )
            }
            VideoBackground::Gradient(index) => {
                crate::capture::editor::types::BackgroundStyle::Gradient(index)
            }
        })
        .with_padding(self.background_padding)
        .with_shadow(self.background_shadow)
        .with_corner_radius(self.background_corner_radius)
        .compute();
        (
            even_dimension(layout.canvas_width.round().max(2.0) as u32),
            even_dimension(layout.canvas_height.round().max(2.0) as u32),
        )
    }

    pub fn estimated_size_bytes(&self, trim_only: bool) -> u64 {
        estimate_size_bytes(self, trim_only)
    }
}

fn ranges_overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> bool {
    a0 < b1 && b0 < a1
}

pub fn eval_zoom(
    clips: &[ZoomClip],
    t: f64,
    frame_width: f64,
    frame_height: f64,
) -> (f64, (f64, f64)) {
    let frame_center = (frame_width / 2.0, frame_height / 2.0);
    let Some(clip) = clips.iter().find(|clip| t >= clip.start && t <= clip.end) else {
        return (1.0, frame_center);
    };
    let ease = (clip.ease_ms as f64 / 1000.0).clamp(0.0, clip.duration() / 2.0);
    let scale = eased_value(t, clip.start, clip.end, ease, 1.0, clip.scale.max(1.0));
    let center_x = eased_value(t, clip.start, clip.end, ease, frame_center.0, clip.center.0);
    let center_y = eased_value(t, clip.start, clip.end, ease, frame_center.1, clip.center.1);
    (scale, (center_x, center_y))
}

fn eased_value(t: f64, start: f64, end: f64, ease: f64, from: f64, to: f64) -> f64 {
    if ease <= f64::EPSILON {
        return to;
    }
    if t < start + ease {
        let alpha = ((t - start) / ease).clamp(0.0, 1.0);
        return lerp(from, to, smoothstep(alpha));
    }
    if t > end - ease {
        let alpha = ((end - t) / ease).clamp(0.0, 1.0);
        return lerp(from, to, smoothstep(alpha));
    }
    to
}

fn lerp(from: f64, to: f64, alpha: f64) -> f64 {
    from + (to - from) * alpha
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn even_crop_rect(
    scale: f64,
    center: (f64, f64),
    src_w: u32,
    src_h: u32,
) -> (u32, u32, u32, u32) {
    let src_w = src_w.max(2);
    let src_h = src_h.max(2);
    let scale = scale.max(1.0);
    let crop_w = even_dimension(((src_w as f64 / scale).round() as u32).max(2).min(src_w));
    let crop_h = even_dimension(((src_h as f64 / scale).round() as u32).max(2).min(src_h));
    let max_x = src_w.saturating_sub(crop_w);
    let max_y = src_h.saturating_sub(crop_h);
    let x = ((center.0 - crop_w as f64 / 2.0).round() as i32).clamp(0, max_x as i32) as u32;
    let y = ((center.1 - crop_h as f64 / 2.0).round() as i32).clamp(0, max_y as i32) as u32;
    (
        even_dimension(x.min(max_x)),
        even_dimension(y.min(max_y)),
        crop_w,
        crop_h,
    )
}

/// Fit `src` inside `box` without upscaling or stretching (aspect preserved).
pub fn fit_dimensions(src_w: u32, src_h: u32, box_w: u32, box_h: u32) -> (u32, u32) {
    let src_w = src_w.max(1);
    let src_h = src_h.max(1);
    let box_w = box_w.max(MIN_DIMENSION);
    let box_h = box_h.max(MIN_DIMENSION);
    // Cap the box at the source so we never upscale past the original.
    let max_w = box_w.min(src_w);
    let max_h = box_h.min(src_h);
    let scale = (max_w as f64 / src_w as f64).min(max_h as f64 / src_h as f64);
    let width = even_dimension(((src_w as f64 * scale).round() as u32).max(2));
    let height = even_dimension(((src_h as f64 * scale).round() as u32).max(2));
    (width.max(2), height.max(2))
}

pub fn even_dimension(value: u32) -> u32 {
    let clamped = value.max(2);
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
    fn dimension_preset_fits_inside_box_preserving_aspect() {
        let mut state = VideoEditState::new(metadata());
        state.dimension_preset = DimensionPreset::Custom;
        // Box is clamped to at least MIN_DIMENSION (64) on each side, then
        // the source is fitted inside without stretching.
        state.custom_width = 1919;
        state.custom_height = 57;

        let (w, h) = state.target_dimensions();
        assert_eq!((w, h), (114, 64));
        // Aspect roughly matches 16:9 source.
        let aspect = w as f64 / h as f64;
        let src_aspect = 1920.0 / 1080.0;
        assert!((aspect - src_aspect).abs() < 0.05);
    }

    #[test]
    fn dimension_preset_does_not_upscale_or_stretch() {
        let mut state = VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/input.mp4"),
            duration_seconds: 5.0,
            width: 600,
            height: 744,
            file_size_bytes: 1024,
            has_audio: false,
        });
        state.dimension_preset = DimensionPreset::P1080;
        let (w, h) = state.target_dimensions();
        assert!(w <= 600 && h <= 744);
        assert_eq!((w, h), (600, 744));
    }

    #[test]
    fn needs_reencode_when_dimensions_or_quality_change() {
        let mut state = VideoEditState::new(metadata());
        assert!(!state.needs_reencode());

        state.quality = 40;
        assert!(state.needs_reencode());

        state.quality = 70;
        state.dimension_preset = DimensionPreset::P720;
        assert!(state.needs_reencode());
    }

    #[test]
    fn needs_reencode_when_zoom_or_background_present() {
        let mut state = VideoEditState::new(metadata());
        assert!(!state.needs_reencode());
        state.zoom_clips.push(ZoomClip {
            start: 1.0,
            end: 2.8,
            scale: 1.8,
            center: (960.0, 540.0),
            ease_ms: 200,
        });
        assert!(state.needs_reencode());

        let mut padded = VideoEditState::new(metadata());
        padded.background = VideoBackground::Plain {
            r: 20,
            g: 20,
            b: 24,
        };
        assert!(padded.needs_reencode());
    }

    #[test]
    fn eval_zoom_eases_in_and_out() {
        let clips = [ZoomClip {
            start: 1.0,
            end: 2.8,
            scale: 2.0,
            center: (200.0, 100.0),
            ease_ms: 200,
        }];
        let (outside, _) = eval_zoom(&clips, 0.5, 1920.0, 1080.0);
        assert!((outside - 1.0).abs() < 1e-9);

        let (hold, center) = eval_zoom(&clips, 1.9, 1920.0, 1080.0);
        assert!((hold - 2.0).abs() < 1e-9);
        assert!((center.0 - 200.0).abs() < 1e-9);
        assert!((center.1 - 100.0).abs() < 1e-9);

        let (ease_in, _) = eval_zoom(&clips, 1.1, 1920.0, 1080.0);
        assert!(ease_in > 1.0 && ease_in < 2.0);

        let (ease_out, _) = eval_zoom(&clips, 2.7, 1920.0, 1080.0);
        assert!(ease_out > 1.0 && ease_out < 2.0);
    }

    #[test]
    fn even_crop_stays_inside_frame() {
        let (x, y, w, h) = even_crop_rect(1.8, (10.0, 10.0), 1920, 1080);
        assert!(w.is_multiple_of(2) && h.is_multiple_of(2));
        assert!(x + w <= 1920);
        assert!(y + h <= 1080);
        assert!(w < 1920 && h < 1080);
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
