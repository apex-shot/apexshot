#[derive(Debug, Clone, PartialEq)]
pub struct CursorHideClip {
    pub start: f64,
    pub end: f64,
}

impl CursorHideClip {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    pub fn contains(&self, t: f64) -> bool {
        t >= self.start && t <= self.end
    }
}

pub fn format_zoom_scale(scale: f64) -> String {
    for &(label, preset) in &ZOOM_SCALE_PRESETS {
        if (scale - preset).abs() < 0.04 {
            return label.to_string();
        }
    }
    if (scale - scale.round()).abs() < 0.05 {
        format!("{:.0}×", scale.round())
    } else {
        format!("{scale:.1}×")
    }
}

pub fn nearest_zoom_preset(scale: f64) -> f64 {
    ZOOM_SCALE_PRESETS
        .iter()
        .min_by(|(_, a), (_, b)| (a - scale).abs().total_cmp(&(b - scale).abs()))
        .map(|(_, value)| *value)
        .unwrap_or(DEFAULT_ZOOM_SCALE)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropSelection {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
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
    /// Playhead on the composition ruler. Dragging a clip must not move this.
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
    /// Composition start of each chronological segment. Dragging a cut piece
    /// changes only its start so a gap can open between neighbors.
    pub segment_starts: Vec<f64>,
    pub segment_speeds: Vec<f64>,
    pub segment_muted: Vec<bool>,
    pub zoom_clips: Vec<ZoomClip>,
    pub selected_zoom: Option<usize>,
    pub cursor_hide_clips: Vec<CursorHideClip>,
    pub selected_cursor_hide: Option<usize>,
    pub selected_segment: Option<usize>,
    pub background: VideoBackground,
    pub background_padding: f64,
    pub background_corner_radius: f64,
    pub background_shadow: f64,
    /// Static source crop in original video pixels. `None` keeps the full frame.
    pub crop: Option<CropSelection>,
    pub sidecar: Option<PointerSidecar>,
    pub cursor: CursorSettings,
    pub selected_tool: EditorTool,
    pub project_media: Vec<ProjectMedia>,
    /// Display / export name (file stem, without extension).
    pub title: String,
    pub video_locked: bool,
    pub video_hidden: bool,
    pub audio_locked: bool,
    pub audio_removed: bool,
    pub zoom_locked: bool,
    pub zoom_hidden: bool,
    /// Classic animation keeps a fixed focus point even when the clip is Auto.
    pub zoom_classic: bool,
    /// 0 = fit the whole clip, 100 = 8× time-axis zoom (WebCut scaler).
    pub timeline_scale: f64,
    /// Seconds of empty timeline before the clip. Dragging the clip body
    /// later on the ruler increases this; it is exported as leading black.
    /// Not tied to the source duration — the ruler stays open on the right.
    pub timeline_offset_seconds: f64,
    /// Horizontal pan at fit zoom, in composition seconds. Does not change
    /// pixels-per-second; the clip stays the same size and the track scrolls.
    pub timeline_scroll_seconds: f64,
}

pub(super) fn seed_project_media(metadata: &VideoMetadata) -> Vec<ProjectMedia> {
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
