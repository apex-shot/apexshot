use super::geometry::SelectionRectF;
use super::icons::TOOLBAR_AREA_INDEX;
use super::layout::RecordPanelTile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    General,
    Video,
    Gif,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResizeHandle {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DragMode {
    /// User dragged outside any existing selection — draw a brand-new rect.
    NewSelection,
    /// User dragged from inside the existing selection — translate the whole rect.
    Move,
    /// User dragged from a border/corner handle — resize the rect.
    Resize(ResizeHandle),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayMode {
    StandardArea,
    CrosshairCapture,
}

/// State for the area selector overlay
pub(crate) struct SelectorState {
    pub(crate) start_x: f64,
    pub(crate) start_y: f64,
    pub(crate) current_x: f64,
    pub(crate) current_y: f64,
    pub(crate) drag_origin_x: f64,
    pub(crate) drag_origin_y: f64,
    pub(crate) drag_mode: Option<DragMode>,
    pub(crate) initial_rect: Option<SelectionRectF>,
    pub(crate) is_dragging: bool,
    pub(crate) cancelled: bool,
    pub(crate) completed: bool,
    pub(crate) active_tool_index: usize,
    pub(crate) hover_tool_index: Option<usize>,
    pub(crate) hover_size_panel: bool,
    pub(crate) hover_crop_panel: bool,
    pub(crate) recording_panel_open: bool,
    pub(crate) hover_record_tile: Option<RecordPanelTile>,
    /// True when the user clicked Fullscreen — selection covers the whole screen,
    /// waiting for Enter to confirm the capture.
    pub(crate) fullscreen_mode: bool,
    // Menu state
    pub(crate) capture_crop_menu_open: bool,
    pub(crate) crop_menu_open: bool,
    pub(crate) settings_menu_open: bool,
    pub(crate) capture_aspect_ratio_index: usize,
    pub(crate) record_aspect_ratio_index: usize,
    pub(crate) hovered_capture_crop_menu_item: i32,
    pub(crate) hovered_crop_menu_item: i32,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) hovered_settings_item: i32,
    pub(crate) settings_dropdown_open: Option<usize>,
    pub(crate) gif_slider_dragging: Option<u8>,
    // Recording toggles (mic/speaker referenced in click handler)
    pub(crate) mic_toggle: bool,
    pub(crate) speaker_toggle: bool,
    // Recording settings
    pub(crate) rec_controls: bool,
    pub(crate) display_rec_time: bool,
    pub(crate) hidpi: bool,
    pub(crate) do_not_disturb: bool,
    pub(crate) show_cursor: bool,
    pub(crate) rec_clicks: bool,
    pub(crate) rec_keystrokes: bool,
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
    // Click options menu
    pub(crate) click_options_open: bool,
    pub(crate) hovered_click_item: i32,
    pub(crate) click_size: f64,
    pub(crate) click_color: usize,
    pub(crate) click_style: usize,
    pub(crate) click_animate: bool,
    pub(crate) click_slider_dragging: bool,
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
    pub(crate) overlay_mode: OverlayMode,
}

impl Default for SelectorState {
    fn default() -> Self {
        Self {
            start_x: 0.0,
            start_y: 0.0,
            current_x: 0.0,
            current_y: 0.0,
            drag_origin_x: 0.0,
            drag_origin_y: 0.0,
            drag_mode: None,
            initial_rect: None,
            is_dragging: false,
            cancelled: false,
            completed: false,
            active_tool_index: TOOLBAR_AREA_INDEX,
            hover_tool_index: None,
            hover_size_panel: false,
            hover_crop_panel: false,
            recording_panel_open: false,
            hover_record_tile: None,
            fullscreen_mode: false,
            capture_crop_menu_open: false,
            crop_menu_open: false,
            settings_menu_open: false,
            capture_aspect_ratio_index: 0,
            record_aspect_ratio_index: 0,
            hovered_capture_crop_menu_item: -1,
            hovered_crop_menu_item: -1,
            settings_tab: SettingsTab::General,
            hovered_settings_item: -1,
            settings_dropdown_open: None,
            gif_slider_dragging: None,
            mic_toggle: true,
            speaker_toggle: false,
            rec_controls: true,
            display_rec_time: true,
            hidpi: false,
            do_not_disturb: true,
            show_cursor: true,
            rec_clicks: true,
            rec_keystrokes: false,
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
            click_options_open: false,
            hovered_click_item: -1,
            click_size: 0.5,
            click_color: 0,
            click_style: 0,
            click_animate: true,
            click_slider_dragging: false,
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
            overlay_mode: OverlayMode::StandardArea,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::icons::TOOLBAR_AREA_INDEX;

    #[test]
    fn selector_state_defaults_to_area_tool_panel_active() {
        let state = SelectorState::default();
        assert_eq!(state.active_tool_index, TOOLBAR_AREA_INDEX);
    }
}
