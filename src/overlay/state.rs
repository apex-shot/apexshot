use super::geometry::SelectionRectF;
use super::icons::TOOLBAR_AREA_INDEX;
pub(crate) use super::recording::state::{OverlayIntent, RecordingState};

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
    /// True when the user clicked Fullscreen — selection covers the whole screen,
    /// waiting for Enter to confirm the capture.
    pub(crate) fullscreen_mode: bool,
    // Menu state (capture-area only)
    pub(crate) capture_crop_menu_open: bool,
    pub(crate) capture_aspect_ratio_index: usize,
    pub(crate) hovered_capture_crop_menu_item: i32,
    pub(crate) overlay_mode: OverlayMode,
    // ── Timer capture state ──
    #[allow(dead_code)]
    pub(crate) timer_delay_active: bool,
    #[allow(dead_code)]
    pub(crate) capture_delay_seconds: i32, // 0, 3, 5, or 10
    #[allow(dead_code)]
    pub(crate) countdown_active: bool,
    #[allow(dead_code)]
    pub(crate) countdown_value: i32,
    #[allow(dead_code)]
    pub(crate) countdown_cancel_requested: bool,
    #[allow(dead_code)]
    pub(crate) hovered_countdown_cancel: bool,
    // ── Scroll popup state ──
    pub(crate) scroll_popup_open: bool,
    pub(crate) hovered_scroll_popup_close: bool,
    // ── Recording panel state (separated) ──
    pub(crate) recording: RecordingState,
    // ── Capture intent (mirrors C++ CaptureIntent) ──
    pub(crate) intent: OverlayIntent,
    // ── Zoom / Magnifier state ──
    pub(crate) show_zoom_preview: bool,
    // ── Window Mode state ──
    pub(crate) window_mode: bool,
    pub(crate) hovered_window: i32,
    pub(crate) windows: Vec<WindowInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct WindowInfo {
    pub(crate) rect: SelectionRectF,
    pub(crate) title: String,
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
            fullscreen_mode: false,
            capture_crop_menu_open: false,
            capture_aspect_ratio_index: 0,
            hovered_capture_crop_menu_item: -1,
            overlay_mode: OverlayMode::StandardArea,
            timer_delay_active: false,
            capture_delay_seconds: 5,
            countdown_active: false,
            countdown_value: 0,
            countdown_cancel_requested: false,
            hovered_countdown_cancel: false,
            scroll_popup_open: false,
            hovered_scroll_popup_close: false,
            recording: RecordingState::default(),
            intent: OverlayIntent::default(),
            show_zoom_preview: true,
            window_mode: false,
            hovered_window: -1,
            windows: Vec::new(),
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
