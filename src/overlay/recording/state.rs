//! Recording panel state, separated from the screenshot capture state.

use super::layout::RecordPanelTile;
use crate::capture_overlay::RecordingType;
use crate::overlay::webcam::{WebcamFrame, WebcamPreview};
use std::sync::{Arc, Mutex};

/// Mirrors the C++ CaptureIntent enum — distinguishes what the user
/// wants to do with the selected area when they confirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum OverlayIntent {
    #[default]
    Area,
    Record,
    Ocr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SettingsTab {
    #[default]
    General,
    Video,
    Gif,
}

/// All state that only matters when the recording panel is open.
/// Extracted from `SelectorState` so the capture-area code stays clean.
pub(crate) struct RecordingState {
    pub(crate) panel_open: bool,
    pub(crate) hover_record_tile: Option<RecordPanelTile>,
    pub(crate) selected_record_type: Option<RecordingType>,

    // Recording toggles
    pub(crate) mic_toggle: bool,
    pub(crate) speaker_toggle: bool,
    pub(crate) mic_level: f64,
    pub(crate) speaker_level: f64,

    // Recording settings
    pub(crate) rec_controls: bool,
    pub(crate) display_rec_time: bool,
    pub(crate) hidpi: bool,
    pub(crate) do_not_disturb: bool,
    pub(crate) show_cursor: bool,
    pub(crate) rec_webcam: bool,
    pub(crate) remember_selection: bool,
    pub(crate) dim_screen: bool,
    pub(crate) show_countdown: bool,

    // Video tab settings
    pub(crate) video_max_res: usize,
    pub(crate) video_fps: usize,
    pub(crate) record_mono: bool,
    pub(crate) open_editor: bool,

    // GIF tab settings
    pub(crate) gif_fps: f64,
    pub(crate) gif_quality: f64,
    pub(crate) optimize_gif: bool,
    pub(crate) gif_size_idx: usize,

    // Crop menu (recording panel)
    pub(crate) crop_menu_open: bool,
    pub(crate) hovered_crop_menu_item: i32,
    pub(crate) record_aspect_ratio_index: usize,

    // Settings menu
    pub(crate) settings_menu_open: bool,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) hovered_settings_item: i32,
    pub(crate) settings_dropdown_open: Option<usize>,
    pub(crate) gif_slider_dragging: Option<u8>,

    // Volume popup menus
    pub(crate) mic_volume_popup_open: bool,
    pub(crate) speaker_volume_popup_open: bool,
    pub(crate) mic_volume: f64,
    pub(crate) speaker_volume: f64,
    pub(crate) volume_slider_dragging: bool,

    // Webcam options menu
    pub(crate) webcam_options_open: bool,
    pub(crate) hovered_webcam_item: i32,
    pub(crate) webcam_device: i32,
    pub(crate) webcam_size: usize,
    pub(crate) webcam_shape: usize,
    pub(crate) webcam_flip: bool,
    pub(crate) webcam_rel_x: f64,
    pub(crate) webcam_rel_y: f64,
    pub(crate) dragging_webcam: bool,
    pub(crate) webcam_drag_offset_x: f64,
    pub(crate) webcam_drag_offset_y: f64,
    pub(crate) webcam_preview: Option<WebcamPreview>,
    pub(crate) webcam_frame: Option<Arc<Mutex<Option<WebcamFrame>>>>,
}

impl Default for RecordingState {
    fn default() -> Self {
        Self {
            panel_open: false,
            hover_record_tile: None,
            selected_record_type: None,
            mic_toggle: true,
            speaker_toggle: false,
            mic_level: 0.0,
            speaker_level: 0.0,
            rec_controls: true,
            display_rec_time: true,
            hidpi: false,
            do_not_disturb: true,
            show_cursor: true,
            rec_webcam: false,
            remember_selection: false,
            dim_screen: false,
            show_countdown: true,
            video_max_res: 0,
            video_fps: 1,
            record_mono: false,
            open_editor: false,
            gif_fps: 15.0,
            gif_quality: 0.9,
            optimize_gif: true,
            gif_size_idx: 0,
            crop_menu_open: false,
            hovered_crop_menu_item: -1,
            record_aspect_ratio_index: 0,
            settings_menu_open: false,
            settings_tab: SettingsTab::General,
            hovered_settings_item: -1,
            settings_dropdown_open: None,
            gif_slider_dragging: None,
            mic_volume_popup_open: false,
            speaker_volume_popup_open: false,
            mic_volume: 1.0,
            speaker_volume: 1.0,
            volume_slider_dragging: false,
            webcam_options_open: false,
            hovered_webcam_item: -1,
            webcam_device: -1,
            webcam_size: 1,
            webcam_shape: 0,
            webcam_flip: false,
            webcam_rel_x: 0.0,
            webcam_rel_y: 0.0,
            dragging_webcam: false,
            webcam_drag_offset_x: 0.0,
            webcam_drag_offset_y: 0.0,
            webcam_preview: None,
            webcam_frame: None,
        }
    }
}
