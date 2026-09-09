#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchBlockedReason {
    ApexOverlayAlreadyActive,
    BuiltinOverlayActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum OverlayExitCode {
    Cancelled = 1,
    Error = 2,
    WindowCaptureRequested = 3,
    SwitchToArea = 4,
    SwitchToFullscreen = 5,
    RecordConfigUpdated = 6,
    ForwardedToExistingOverlay = 10,
    BlockedByBuiltinOverlay = 11,
}

/// Result of running the capture overlay — either an area selection or a
/// full window capture (when user clicks the Window toolbar button).
pub enum OverlayResult {
    /// User selected an area — coordinates to crop from.
    Area(SelectionArea),
    /// User clicked Window tool — full window pixel data already captured.
    Window(CaptureData),
    /// User cancelled.
    Cancelled,
}

/// Result of area capture initiation through the C++ overlay.
#[derive(Debug)]
pub enum AreaCaptureResult {
    Captured(CaptureData),
    ScrollCaptured(CaptureData),
    OcrRequested(CaptureData),
    RecordingRequested(RecordingRequest),
    Cancelled,
}

/// Recording request from the capture overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingRequest {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub record_type: RecordingType,
    pub controls: bool,
    pub mic: bool,
    pub speaker: bool,
    // General tab settings
    pub display_rec_time: bool,
    pub hidpi: bool,
    pub notifications: bool,
    pub cursor: bool,
    pub remember_selection: bool,
    pub dim_screen: bool,
    pub countdown: bool,
    // Video tab settings
    pub video_format: u8,
    pub video_max_res: u8,
    pub video_fps: u8,
    pub record_mono: bool,
    pub open_editor: bool,
    #[serde(default)]
    pub noise_suppression: bool,
    // GIF tab settings
    pub gif_fps: u8,
    pub gif_quality: f64,
    pub gif_size_idx: u8,
    pub optimize_gif: bool,
    pub fullscreen: bool,
}

impl Default for RecordingRequest {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            record_type: RecordingType::Video,
            controls: false,
            mic: false,
            speaker: false,
            display_rec_time: false,
            hidpi: true,
            notifications: true,
            cursor: true,
            remember_selection: false,
            dim_screen: true,
            countdown: true,
            video_format: 0,
            video_max_res: 0,
            video_fps: 2,
            record_mono: false,
            open_editor: true,
            noise_suppression: false,
            gif_fps: 50,
            gif_quality: 0.75,
            gif_size_idx: 0,
            optimize_gif: true,
            fullscreen: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordingType {
    Video,
    Gif,
}

#[derive(Debug)]
pub enum AreaCapturePathResult {
    Captured(PathBuf),
    ScrollCaptured(PathBuf),
    OcrRequested(CaptureData),
    RecordingRequested(RecordingRequest),
    RecordingConfigUpdated,
    Cancelled,
}
