//! Recording panel state, separated from the screenshot capture state.

use super::layout::RecordPanelTile;
use crate::capture_overlay::RecordingType;

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
    pub(crate) hidpi: bool,
    pub(crate) do_not_disturb: bool,
    pub(crate) remember_selection: bool,
    pub(crate) dim_screen: bool,
    pub(crate) show_countdown: bool,

    // Video tab settings
    pub(crate) video_max_res: usize,
    pub(crate) video_fps: usize,
    pub(crate) record_mono: bool,
    pub(crate) noise_suppression: bool,
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
    pub(crate) hovered_settings_dropdown_item: i32,
    pub(crate) settings_dropdown_open: Option<usize>,
    pub(crate) gif_slider_dragging: Option<u8>,

    // Volume popup menus
    pub(crate) mic_volume_popup_open: bool,
    pub(crate) speaker_volume_popup_open: bool,
    pub(crate) mic_volume: f64,
    pub(crate) speaker_volume: f64,
    pub(crate) volume_slider_dragging: bool,
    pub(crate) last_volume_system_write: Option<std::time::Instant>,
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
            hidpi: false,
            do_not_disturb: true,
            remember_selection: false,
            dim_screen: false,
            show_countdown: true,
            video_max_res: 0,
            video_fps: 1,
            record_mono: false,
            noise_suppression: false,
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
            hovered_settings_dropdown_item: -1,
            settings_dropdown_open: None,
            gif_slider_dragging: None,
            mic_volume_popup_open: false,
            speaker_volume_popup_open: false,
            mic_volume: 1.0,
            speaker_volume: 1.0,
            volume_slider_dragging: false,
            last_volume_system_write: None,
        }
    }
}

impl RecordingState {
    pub(crate) fn from_app_config(config: &crate::config::AppConfig) -> Self {
        Self {
            rec_controls: config.rec_controls,
            hidpi: config.rec_hidpi,
            do_not_disturb: config.rec_notifications,
            remember_selection: config.rec_remember_selection,
            dim_screen: config.rec_dim_screen,
            show_countdown: config.rec_countdown,
            video_max_res: config.rec_video_max_res as usize,
            video_fps: config.rec_video_fps as usize,
            record_mono: config.rec_video_mono,
            noise_suppression: config.rec_noise_suppression,
            open_editor: config.rec_video_open_editor,
            gif_fps: config.rec_gif_fps as f64,
            gif_quality: config.rec_gif_quality,
            optimize_gif: config.rec_gif_optimize,
            gif_size_idx: config.rec_gif_size_idx as usize,
            mic_toggle: config.rec_mic,
            speaker_toggle: config.rec_speaker,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RecordingState;
    use crate::config::AppConfig;

    #[test]
    fn settings_dropdown_hover_starts_clear() {
        assert_eq!(RecordingState::default().hovered_settings_dropdown_item, -1);
    }

    #[test]
    fn recording_setup_restores_saved_settings() {
        let config = AppConfig {
            rec_controls: false,
            rec_hidpi: false,
            rec_notifications: false,
            rec_remember_selection: true,
            rec_dim_screen: false,
            rec_countdown: false,
            rec_video_max_res: 2,
            rec_video_fps: 3,
            rec_video_mono: true,
            rec_noise_suppression: true,
            rec_video_open_editor: true,
            rec_gif_fps: 24,
            rec_gif_quality: 0.4,
            rec_gif_optimize: false,
            rec_gif_size_idx: 2,
            rec_mic: true,
            rec_speaker: true,
            ..AppConfig::default()
        };

        let state = RecordingState::from_app_config(&config);

        assert!(!state.rec_controls);
        assert!(!state.hidpi);
        assert!(!state.do_not_disturb);
        assert!(state.remember_selection);
        assert!(!state.dim_screen);
        assert!(!state.show_countdown);
        assert_eq!(state.video_max_res, 2);
        assert_eq!(state.video_fps, 3);
        assert!(state.record_mono);
        assert!(state.noise_suppression);
        assert!(state.open_editor);
        assert_eq!(state.gif_fps, 24.0);
        assert_eq!(state.gif_quality, 0.4);
        assert!(!state.optimize_gif);
        assert_eq!(state.gif_size_idx, 2);
        assert!(state.mic_toggle);
        assert!(state.speaker_toggle);
    }
}
