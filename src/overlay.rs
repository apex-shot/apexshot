//! GTK4 overlay for interactive area selection
//!
//! This module provides a full-screen transparent window that allows users
//! to select a screen area using mouse drag. Only used for X11 backend.

use crate::backend::{CaptureData, PixelFormat};
use gtk4::gdk::Key;
use gtk4::{
    gdk,
    glib::{self, clone},
    prelude::*,
    Application, ApplicationWindow, CssProvider, EventControllerKey, EventControllerMotion,
    GestureClick, GestureDrag,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use image::RgbaImage;
use rayon::prelude::*;
use std::f64::consts::PI;
use std::sync::{Arc, Mutex};
use x11rb::wrapper::ConnectionExt;

/// Selected area coordinates
#[derive(Debug, Clone, Copy)]
pub struct SelectionArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl SelectionArea {
    /// Normalize the selection (handle negative width/height from dragging)
    pub fn normalize(mut self) -> Self {
        if self.width < 0 {
            self.x += self.width;
            self.width = self.width.abs();
        }
        if self.height < 0 {
            self.y += self.height;
            self.height = self.height.abs();
        }
        self
    }

    /// Check if the selection is valid (has positive area)
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0
    }
}

/// Result of area selection
pub type SelectionResult = Result<Option<SelectionArea>, SelectionError>;

#[derive(Debug, thiserror::Error)]
pub enum SelectionError {
    #[error("GTK initialization failed: {0}")]
    InitError(String),

    #[error("{0}")]
    Blocked(String),

    #[error("Selection was cancelled by user")]
    Cancelled,
}

const DEFAULT_SELECTION_WIDTH: f64 = 600.0;
const DEFAULT_SELECTION_HEIGHT: f64 = 744.0;
const MIN_SELECTION_WIDTH: f64 = 24.0;
const MIN_SELECTION_HEIGHT: f64 = 24.0;
const BORDER_HANDLE_THRESHOLD: f64 = 10.0;
const HANDLE_MARKER_LENGTH: f64 = 20.0;
const HANDLE_MARKER_THICKNESS: f64 = 2.5;
const BRAND_ORANGE_R: f64 = 1.0;
const BRAND_ORANGE_G: f64 = 0.4;
const BRAND_ORANGE_B: f64 = 0.0;
const FEATURE_PANEL_ITEM_WIDTH: f64 = 76.0;
const FEATURE_PANEL_HEIGHT: f64 = 62.0;
const FEATURE_PANEL_RADIUS: f64 = 13.0;
const FEATURE_PANEL_TOP_GAP: f64 = 12.0;
const FEATURE_PANEL_MARGIN: f64 = 16.0;
const TOOL_RAIL_GAP: f64 = 18.0;
const ACTION_CARD_GAP: f64 = 8.0;
const SIZE_CARD_WIDTH: f64 = 152.0;
const SIZE_CARD_HEIGHT: f64 = 56.0;
const CROP_CARD_WIDTH: f64 = 62.0;
const REC_TOP_CLUSTER_WIDTH: f64 = 292.0;
const REC_TOP_CLUSTER_HEIGHT: f64 = 56.0;
const REC_ACTION_WIDTH: f64 = 120.0;
const REC_ACTION_HEIGHT: f64 = 50.0;

#[derive(Clone, Copy)]
enum ToolbarIcon {
    Capture,
    Area,
    Fullscreen,
    Window,
    Scroll,
    Timer,
    Ocr,
    Recording,
    Controls,
    Crop,
    Mic,
    Speaker,
    Webcam,
    Clicks,
    Keystrokes,
    Video,
    Gif,
}

const TOOLBAR_ICONS: [ToolbarIcon; 8] = [
    ToolbarIcon::Capture,
    ToolbarIcon::Area,
    ToolbarIcon::Fullscreen,
    ToolbarIcon::Window,
    ToolbarIcon::Scroll,
    ToolbarIcon::Timer,
    ToolbarIcon::Ocr,
    ToolbarIcon::Recording,
];

const TOOLBAR_LABELS: [&str; 8] = [
    "Capture",
    "Area",
    "Fullscreen",
    "Window",
    "Scroll",
    "Timer",
    "OCR",
    "Recording",
];

#[derive(Debug, Clone, Copy)]
struct RectF {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl RectF {
    fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

#[derive(Debug, Clone, Copy)]
struct ToolbarLayout {
    tools_panel: RectF,
    size_panel: RectF,
    crop_panel: RectF,
    item_cells: [RectF; 8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolbarHit {
    Tool(usize),
    SizePanel,
    CropPanel,
}

#[derive(Debug, Clone, Copy)]
struct RecordingDeckLayout {
    left_toggle_rail: RectF,
    top_cluster: RectF,
    bottom_action_bar: RectF,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordPanelTile {
    Controls,
    Size,
    Crop,
    Mic,
    Speaker,
    Webcam,
    Clicks,
    Keystrokes,
    RecordVideo,
    RecordGif,
}

#[derive(Debug, Clone, Copy)]
struct SelectionRectF {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl SelectionRectF {
    fn from_points(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self {
            left: x0.min(x1),
            top: y0.min(y1),
            right: x0.max(x1),
            bottom: y0.max(y1),
        }
    }

    fn width(&self) -> f64 {
        self.right - self.left
    }

    fn height(&self) -> f64 {
        self.bottom - self.top
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Video,
    Gif,
}

const ASPECT_RATIO_OPTIONS: &[&str] = &[
    "Freeform",
    "1 : 1 (Square)",
    "5 : 4 (10 : 8)",
    "4 : 3",
    "7 : 5",
    "3 : 2",
    "16 : 10",
    "16 : 9",
    "2.35 : 1",
    "2 : 3",
    "9 : 16",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResizeHandle {
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
enum DragMode {
    /// User dragged outside any existing selection — draw a brand-new rect.
    NewSelection,
    /// User dragged from inside the existing selection — translate the whole rect.
    Move,
    /// User dragged from a border/corner handle — resize the rect.
    Resize(ResizeHandle),
}

/// State for the area selector overlay
struct SelectorState {
    start_x: f64,
    start_y: f64,
    current_x: f64,
    current_y: f64,
    drag_origin_x: f64,
    drag_origin_y: f64,
    drag_mode: Option<DragMode>,
    initial_rect: Option<SelectionRectF>,
    is_dragging: bool,
    cancelled: bool,
    completed: bool,
    hover_tool_index: Option<usize>,
    hover_size_panel: bool,
    hover_crop_panel: bool,
    recording_panel_open: bool,
    hover_record_tile: Option<RecordPanelTile>,
    /// True when the user clicked Fullscreen — selection covers the whole screen,
    /// waiting for Enter to confirm the capture.
    fullscreen_mode: bool,
    // Menu state
    capture_crop_menu_open: bool,
    crop_menu_open: bool,
    settings_menu_open: bool,
    capture_aspect_ratio_index: usize,
    record_aspect_ratio_index: usize,
    hovered_capture_crop_menu_item: i32,
    hovered_crop_menu_item: i32,
    settings_tab: SettingsTab,
    hovered_settings_item: i32,
    settings_dropdown_open: Option<usize>,
    gif_slider_dragging: Option<u8>,
    // Recording toggles (mic/speaker referenced in click handler)
    mic_toggle: bool,
    speaker_toggle: bool,
    // Recording settings
    rec_controls: bool,
    display_rec_time: bool,
    hidpi: bool,
    do_not_disturb: bool,
    show_cursor: bool,
    rec_clicks: bool,
    rec_keystrokes: bool,
    remember_selection: bool,
    dim_screen: bool,
    show_countdown: bool,
    // Video tab settings
    video_max_res: usize,
    video_fps: usize,
    record_mono: bool,
    open_editor: bool,
    // GIF tab settings
    gif_fps: f64,
    gif_quality: f64,
    optimize_gif: bool,
    gif_size_idx: usize,
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
        }
    }
}

fn current_selection_rect(state: &SelectorState) -> SelectionRectF {
    SelectionRectF::from_points(
        state.start_x,
        state.start_y,
        state.current_x,
        state.current_y,
    )
}

fn set_selection_rect(state: &mut SelectorState, rect: SelectionRectF) {
    state.start_x = rect.left;
    state.start_y = rect.top;
    state.current_x = rect.right;
    state.current_y = rect.bottom;
}

fn clamp_point_to_bounds(x: f64, y: f64, bounds_width: f64, bounds_height: f64) -> (f64, f64) {
    (
        x.clamp(0.0, bounds_width.max(1.0)),
        y.clamp(0.0, bounds_height.max(1.0)),
    )
}

fn detect_resize_handle(x: f64, y: f64, rect: SelectionRectF) -> Option<ResizeHandle> {
    let left = rect.left;
    let right = rect.right;
    let top = rect.top;
    let bottom = rect.bottom;

    let near_left = (x - left).abs() <= BORDER_HANDLE_THRESHOLD
        && y >= top - BORDER_HANDLE_THRESHOLD
        && y <= bottom + BORDER_HANDLE_THRESHOLD;
    let near_right = (x - right).abs() <= BORDER_HANDLE_THRESHOLD
        && y >= top - BORDER_HANDLE_THRESHOLD
        && y <= bottom + BORDER_HANDLE_THRESHOLD;
    let near_top = (y - top).abs() <= BORDER_HANDLE_THRESHOLD
        && x >= left - BORDER_HANDLE_THRESHOLD
        && x <= right + BORDER_HANDLE_THRESHOLD;
    let near_bottom = (y - bottom).abs() <= BORDER_HANDLE_THRESHOLD
        && x >= left - BORDER_HANDLE_THRESHOLD
        && x <= right + BORDER_HANDLE_THRESHOLD;

    if near_left && near_top {
        return Some(ResizeHandle::NorthWest);
    }
    if near_right && near_top {
        return Some(ResizeHandle::NorthEast);
    }
    if near_left && near_bottom {
        return Some(ResizeHandle::SouthWest);
    }
    if near_right && near_bottom {
        return Some(ResizeHandle::SouthEast);
    }

    if near_top {
        return Some(ResizeHandle::North);
    }
    if near_bottom {
        return Some(ResizeHandle::South);
    }
    if near_left {
        return Some(ResizeHandle::West);
    }
    if near_right {
        return Some(ResizeHandle::East);
    }

    None
}

/// Returns `true` when `(x, y)` is strictly inside the selection rectangle,
/// far enough from every edge that it is not on a resize handle.
/// This is used to decide whether a drag should move the whole rect.
fn is_inside_selection(x: f64, y: f64, rect: SelectionRectF) -> bool {
    x > rect.left + BORDER_HANDLE_THRESHOLD
        && x < rect.right - BORDER_HANDLE_THRESHOLD
        && y > rect.top + BORDER_HANDLE_THRESHOLD
        && y < rect.bottom - BORDER_HANDLE_THRESHOLD
}

fn cursor_name_for_handle(handle: ResizeHandle) -> &'static str {
    match handle {
        ResizeHandle::North | ResizeHandle::South => "ns-resize",
        ResizeHandle::East | ResizeHandle::West => "ew-resize",
        ResizeHandle::NorthEast | ResizeHandle::SouthWest => "nesw-resize",
        ResizeHandle::NorthWest | ResizeHandle::SouthEast => "nwse-resize",
    }
}

fn resize_rect_from_handle(
    initial: SelectionRectF,
    handle: ResizeHandle,
    pointer_x: f64,
    pointer_y: f64,
    bounds_width: f64,
    bounds_height: f64,
) -> SelectionRectF {
    let mut left = initial.left;
    let mut top = initial.top;
    let mut right = initial.right;
    let mut bottom = initial.bottom;

    let move_left = matches!(
        handle,
        ResizeHandle::West | ResizeHandle::NorthWest | ResizeHandle::SouthWest
    );
    let move_right = matches!(
        handle,
        ResizeHandle::East | ResizeHandle::NorthEast | ResizeHandle::SouthEast
    );
    let move_top = matches!(
        handle,
        ResizeHandle::North | ResizeHandle::NorthWest | ResizeHandle::NorthEast
    );
    let move_bottom = matches!(
        handle,
        ResizeHandle::South | ResizeHandle::SouthWest | ResizeHandle::SouthEast
    );

    if move_left {
        left = pointer_x;
    }
    if move_right {
        right = pointer_x;
    }
    if move_top {
        top = pointer_y;
    }
    if move_bottom {
        bottom = pointer_y;
    }

    let min_width = MIN_SELECTION_WIDTH.min(bounds_width.max(1.0));
    let min_height = MIN_SELECTION_HEIGHT.min(bounds_height.max(1.0));

    if (right - left) < min_width {
        if move_left {
            left = right - min_width;
        } else {
            right = left + min_width;
        }
    }

    if (bottom - top) < min_height {
        if move_top {
            top = bottom - min_height;
        } else {
            bottom = top + min_height;
        }
    }

    left = left.clamp(0.0, (bounds_width - min_width).max(0.0));
    top = top.clamp(0.0, (bounds_height - min_height).max(0.0));
    right = right.clamp(min_width, bounds_width.max(min_width));
    bottom = bottom.clamp(min_height, bounds_height.max(min_height));

    if (right - left) < min_width {
        if move_left {
            left = (right - min_width).max(0.0);
        } else {
            right = (left + min_width).min(bounds_width.max(min_width));
        }
    }

    if (bottom - top) < min_height {
        if move_top {
            top = (bottom - min_height).max(0.0);
        } else {
            bottom = (top + min_height).min(bounds_height.max(min_height));
        }
    }

    SelectionRectF {
        left,
        top,
        right,
        bottom,
    }
}

fn update_selection_for_drag(
    state: &mut SelectorState,
    drag_offset_x: f64,
    drag_offset_y: f64,
    bounds_width: f64,
    bounds_height: f64,
) {
    match state.drag_mode {
        Some(DragMode::NewSelection) => {
            let (next_x, next_y) = clamp_point_to_bounds(
                state.drag_origin_x + drag_offset_x,
                state.drag_origin_y + drag_offset_y,
                bounds_width,
                bounds_height,
            );
            state.current_x = next_x;
            state.current_y = next_y;
        }
        Some(DragMode::Move) => {
            if let Some(initial_rect) = state.initial_rect {
                let w = initial_rect.width();
                let h = initial_rect.height();
                // Translate the whole rect by the drag delta, keeping it
                // fully within the screen bounds.
                let new_left =
                    (initial_rect.left + drag_offset_x).clamp(0.0, (bounds_width - w).max(0.0));
                let new_top =
                    (initial_rect.top + drag_offset_y).clamp(0.0, (bounds_height - h).max(0.0));
                set_selection_rect(
                    state,
                    SelectionRectF {
                        left: new_left,
                        top: new_top,
                        right: new_left + w,
                        bottom: new_top + h,
                    },
                );
                state.completed = true;
            }
        }
        Some(DragMode::Resize(handle)) => {
            if let Some(initial_rect) = state.initial_rect {
                let (pointer_x, pointer_y) = clamp_point_to_bounds(
                    state.drag_origin_x + drag_offset_x,
                    state.drag_origin_y + drag_offset_y,
                    bounds_width,
                    bounds_height,
                );
                let resized = resize_rect_from_handle(
                    initial_rect,
                    handle,
                    pointer_x,
                    pointer_y,
                    bounds_width,
                    bounds_height,
                );
                set_selection_rect(state, resized);
                state.completed = true;
            }
        }
        None => {}
    }
}

fn selection_area_from_state(
    state: &SelectorState,
    screen_width: i32,
    screen_height: i32,
    background: Option<&BackgroundFrame>,
) -> SelectionArea {
    if state.fullscreen_mode {
        let mut full = SelectionArea {
            x: 0,
            y: 0,
            width: screen_width,
            height: screen_height,
        };
        if let Some(background) = background {
            full = map_selection_to_image(
                full,
                background.width,
                background.height,
                screen_width,
                screen_height,
            );
        }
        return full;
    }

    let rect = current_selection_rect(state);
    let area = SelectionArea {
        x: rect.left.floor() as i32,
        y: rect.top.floor() as i32,
        width: rect.width().round() as i32,
        height: rect.height().round() as i32,
    };
    if let Some(background) = background {
        map_selection_to_image(
            area,
            background.width,
            background.height,
            screen_width,
            screen_height,
        )
    } else {
        area
    }
}

fn send_selection_result(
    state: &Arc<Mutex<SelectorState>>,
    result_tx: &std::sync::mpsc::Sender<SelectionResult>,
    window: &ApplicationWindow,
    screen_width: i32,
    screen_height: i32,
    background: Option<&BackgroundFrame>,
) {
    let st = state.lock().unwrap();
    let area = selection_area_from_state(&st, screen_width, screen_height, background);
    drop(st);

    let result = if area.is_valid() {
        Ok(Some(area))
    } else {
        Ok(None)
    };
    let _ = result_tx.send(result);
    window.close();
}

fn draw_resize_markers(context: &gtk4::cairo::Context, x: f64, y: f64, width: f64, height: f64) {
    let half = HANDLE_MARKER_LENGTH / 2.0;

    context.set_source_rgba(BRAND_ORANGE_R, BRAND_ORANGE_G, BRAND_ORANGE_B, 0.96);
    context.set_line_width(HANDLE_MARKER_THICKNESS);
    context.set_line_cap(gtk4::cairo::LineCap::Round);

    // Corner L-markers
    context.move_to(x, y + half);
    context.line_to(x, y);
    context.line_to(x + half, y);

    context.move_to(x + width - half, y);
    context.line_to(x + width, y);
    context.line_to(x + width, y + half);

    context.move_to(x, y + height - half);
    context.line_to(x, y + height);
    context.line_to(x + half, y + height);

    context.move_to(x + width - half, y + height);
    context.line_to(x + width, y + height);
    context.line_to(x + width, y + height - half);

    // Mid-edge line markers
    context.move_to(x + width / 2.0 - half, y);
    context.line_to(x + width / 2.0 + half, y);

    context.move_to(x + width / 2.0 - half, y + height);
    context.line_to(x + width / 2.0 + half, y + height);

    context.move_to(x, y + height / 2.0 - half);
    context.line_to(x, y + height / 2.0 + half);

    context.move_to(x + width, y + height / 2.0 - half);
    context.line_to(x + width, y + height / 2.0 + half);

    let _ = context.stroke();
}

fn rounded_rect_path(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    let r = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    context.new_sub_path();
    context.arc(x + width - r, y + r, r, -PI / 2.0, 0.0);
    context.arc(x + width - r, y + height - r, r, 0.0, PI / 2.0);
    context.arc(x + r, y + height - r, r, PI / 2.0, PI);
    context.arc(x + r, y + r, r, PI, PI * 1.5);
    context.close_path();
}

fn draw_feature_toolbar(
    context: &gtk4::cairo::Context,
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
    hover_tool_index: Option<usize>,
    hover_size_panel: bool,
    hover_crop_panel: bool,
    capture_crop_menu_open: bool,
    capture_aspect_ratio_index: usize,
    hovered_capture_crop_menu_item: i32,
) {
    let layout = compute_toolbar_layout(
        selection_x,
        selection_y,
        selection_width,
        selection_height,
        screen_width,
        screen_height,
    );

    let size_panel_x = layout.size_panel.x;
    let size_panel_y = layout.size_panel.y;
    let size_panel_width = layout.size_panel.width;
    let crop_panel = layout.crop_panel;

    draw_frosted_panel(
        context,
        layout.tools_panel.x,
        layout.tools_panel.y,
        layout.tools_panel.width,
        layout.tools_panel.height,
        FEATURE_PANEL_RADIUS,
        screen_width,
        screen_height,
        background,
    );

    // Single combined panel for size + crop (matches C++ topCluster)
    let top_cluster_x = layout.size_panel.x;
    let top_cluster_y = layout.size_panel.y;
    let top_cluster_w = layout.size_panel.width + ACTION_CARD_GAP + layout.crop_panel.width;
    let top_cluster_h = layout.size_panel.height;
    draw_frosted_panel(
        context,
        top_cluster_x,
        top_cluster_y,
        top_cluster_w,
        top_cluster_h,
        FEATURE_PANEL_RADIUS,
        screen_width,
        screen_height,
        background,
    );

    let draw_accent = |context: &gtk4::cairo::Context, rect: RectF, active: bool| {
        rounded_rect_path(
            context,
            rect.x + 4.0,
            rect.y + 4.0,
            rect.width - 8.0,
            rect.height - 8.0,
            10.0,
        );
        if active {
            context.set_source_rgba(176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 0.30);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 0.16);
        }
        let _ = context.fill();
    };

    draw_accent(context, layout.item_cells[0], true);
    if let Some(index) = hover_tool_index {
        if let Some(cell) = layout.item_cells.get(index) {
            draw_accent(context, *cell, index == 0);
        }
    }
    if hover_size_panel || hover_crop_panel {
        draw_accent(
            context,
            RectF {
                x: top_cluster_x,
                y: top_cluster_y,
                width: top_cluster_w,
                height: top_cluster_h,
            },
            false,
        );
    }

    // Icons + labels
    for (index, icon) in TOOLBAR_ICONS.iter().enumerate() {
        let cell = layout.item_cells[index];
        let center_x = cell.x + cell.width / 2.0;
        let label = TOOLBAR_LABELS[index];
        let is_hovered = hover_tool_index == Some(index);
        let is_active = index == 0;

        // Icon: brighter + reduced shadow on hover
        let (shadow_alpha, icon_alpha) = if is_hovered || is_active {
            (0.30, 1.0)
        } else {
            (0.52, 0.98)
        };
        let icon_y = if is_hovered || is_active {
            cell.y + 23.5
        } else {
            cell.y + 24.0
        };
        draw_toolbar_icon(
            context,
            *icon,
            center_x + 0.6,
            icon_y + 0.8,
            (0.0, 0.0, 0.0, shadow_alpha),
        );
        draw_toolbar_icon(
            context,
            *icon,
            center_x,
            icon_y,
            if is_active {
                (1.0, 229.0 / 255.0, 206.0 / 255.0, icon_alpha)
            } else {
                (1.0, 1.0, 1.0, icon_alpha)
            },
        );

        // Label: bold + brighter on hover
        let font_weight = if is_hovered || is_active {
            gtk4::cairo::FontWeight::Bold
        } else {
            gtk4::cairo::FontWeight::Normal
        };
        let (label_alpha_shadow, label_alpha) = if is_hovered {
            (0.30, 1.0)
        } else {
            (0.52, 0.98)
        };

        context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, font_weight);
        context.set_font_size(9.5);
        context.set_source_rgba(0.0, 0.0, 0.0, label_alpha_shadow);
        if let Ok(extents) = context.text_extents(label) {
            let text_x = center_x - extents.width() / 2.0 - extents.x_bearing() + 0.6;
            let text_y = cell.y + 50.0 + 0.8;
            context.move_to(text_x, text_y);
            let _ = context.show_text(label);
        }

        if is_active {
            context.set_source_rgba(1.0, 229.0 / 255.0, 206.0 / 255.0, label_alpha);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, label_alpha);
        }
        if let Ok(extents) = context.text_extents(label) {
            let text_x = center_x - extents.width() / 2.0 - extents.x_bearing();
            let text_y = cell.y + 50.0;
            context.move_to(text_x, text_y);
            let _ = context.show_text(label);
        }
    }

    let size_text = format!("{}×{}", selection_width as i32, selection_height as i32);
    let size_center_x = size_panel_x + size_panel_width / 2.0;

    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(9.6);
    context.set_source_rgba(0.0, 0.0, 0.0, 0.50);
    if let Ok(extents) = context.text_extents("FRAME") {
        let text_x = size_center_x - extents.width() / 2.0 - extents.x_bearing() + 0.6;
        let text_y = size_panel_y + 17.0 + 0.8;
        context.move_to(text_x, text_y);
        let _ = context.show_text("FRAME");
    }

    context.set_source_rgba(1.0, 224.0 / 255.0, 196.0 / 255.0, 0.84);
    if let Ok(extents) = context.text_extents("FRAME") {
        let text_x = size_center_x - extents.width() / 2.0 - extents.x_bearing();
        let text_y = size_panel_y + 17.0;
        context.move_to(text_x, text_y);
        let _ = context.show_text("FRAME");
    }

    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(12.5);
    context.set_source_rgba(0.0, 0.0, 0.0, 0.55);
    if let Ok(extents) = context.text_extents(&size_text) {
        let text_x = size_center_x - extents.width() / 2.0 - extents.x_bearing() + 0.6;
        let text_y = size_panel_y + 39.0 + 0.8;
        context.move_to(text_x, text_y);
        let _ = context.show_text(&size_text);
    }

    context.set_source_rgba(1.0, 1.0, 1.0, 0.98);
    if let Ok(extents) = context.text_extents(&size_text) {
        let text_x = size_center_x - extents.width() / 2.0 - extents.x_bearing();
        let text_y = size_panel_y + 39.0;
        context.move_to(text_x, text_y);
        let _ = context.show_text(&size_text);
    }

    let crop_center_x = crop_panel.x + crop_panel.width / 2.0;
    let crop_y = crop_panel.y + 27.5;
    draw_toolbar_icon(
        context,
        ToolbarIcon::Crop,
        crop_center_x + 0.6,
        crop_y + 0.8,
        (0.0, 0.0, 0.0, if hover_crop_panel { 0.24 } else { 0.46 }),
    );
    draw_toolbar_icon(
        context,
        ToolbarIcon::Crop,
        crop_center_x,
        crop_y,
        (1.0, 1.0, 1.0, 0.95),
    );

    if capture_crop_menu_open {
        draw_capture_crop_menu(
            context,
            crop_panel,
            hovered_capture_crop_menu_item,
            capture_aspect_ratio_index,
            screen_width,
            screen_height,
            background,
        );
    }
}

fn compute_toolbar_layout(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
) -> ToolbarLayout {
    let tool_panel_height = FEATURE_PANEL_HEIGHT * TOOLBAR_ICONS.len() as f64;
    let center_y = selection_y + selection_height / 2.0;
    let tool_x = (selection_x - TOOL_RAIL_GAP - FEATURE_PANEL_ITEM_WIDTH).max(FEATURE_PANEL_MARGIN);
    let tool_y = (center_y - tool_panel_height / 2.0).clamp(
        FEATURE_PANEL_MARGIN,
        (screen_height - tool_panel_height - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
    );

    let tools_panel = RectF {
        x: tool_x,
        y: tool_y,
        width: FEATURE_PANEL_ITEM_WIDTH,
        height: tool_panel_height,
    };

    let top_width = SIZE_CARD_WIDTH + ACTION_CARD_GAP + CROP_CARD_WIDTH;
    let top_x = (selection_x + (selection_width - top_width) / 2.0).clamp(
        FEATURE_PANEL_MARGIN,
        (screen_width - top_width - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
    );
    let top_y = (selection_y - FEATURE_PANEL_TOP_GAP - SIZE_CARD_HEIGHT).clamp(
        FEATURE_PANEL_MARGIN,
        (screen_height - SIZE_CARD_HEIGHT - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
    );

    let size_panel = RectF {
        x: top_x,
        y: top_y,
        width: SIZE_CARD_WIDTH,
        height: SIZE_CARD_HEIGHT,
    };
    let crop_panel = RectF {
        x: top_x + SIZE_CARD_WIDTH + ACTION_CARD_GAP,
        y: top_y,
        width: CROP_CARD_WIDTH,
        height: SIZE_CARD_HEIGHT,
    };

    let mut item_cells = [RectF {
        x: 0.0,
        y: 0.0,
        width: FEATURE_PANEL_ITEM_WIDTH,
        height: FEATURE_PANEL_HEIGHT,
    }; 8];

    for (index, cell) in item_cells.iter_mut().enumerate() {
        cell.x = tools_panel.x;
        cell.y = tools_panel.y + index as f64 * FEATURE_PANEL_HEIGHT;
    }

    ToolbarLayout {
        tools_panel,
        size_panel,
        crop_panel,
        item_cells,
    }
}

fn compute_recording_deck_layout(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
) -> RecordingDeckLayout {
    let rail_height = FEATURE_PANEL_HEIGHT * 5.0;
    let center_y = selection_y + selection_height / 2.0;
    let rail_x = (selection_x - TOOL_RAIL_GAP - FEATURE_PANEL_ITEM_WIDTH).max(FEATURE_PANEL_MARGIN);
    let rail_y = (center_y - rail_height / 2.0).clamp(
        FEATURE_PANEL_MARGIN,
        (screen_height - rail_height - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
    );
    let top_x = (selection_x + (selection_width - REC_TOP_CLUSTER_WIDTH) / 2.0).clamp(
        FEATURE_PANEL_MARGIN,
        (screen_width - REC_TOP_CLUSTER_WIDTH - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
    );
    let top_y = (selection_y - FEATURE_PANEL_TOP_GAP - REC_TOP_CLUSTER_HEIGHT).clamp(
        FEATURE_PANEL_MARGIN,
        (screen_height - REC_TOP_CLUSTER_HEIGHT - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
    );
    let action_width = REC_ACTION_WIDTH * 2.0 + ACTION_CARD_GAP;
    let action_x = (selection_x + (selection_width - action_width) / 2.0).clamp(
        FEATURE_PANEL_MARGIN,
        (screen_width - action_width - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
    );
    let below_y = selection_y + selection_height + FEATURE_PANEL_TOP_GAP;
    let above_y = selection_y - FEATURE_PANEL_TOP_GAP - REC_ACTION_HEIGHT;
    let action_y = if below_y + REC_ACTION_HEIGHT + FEATURE_PANEL_MARGIN <= screen_height {
        below_y
    } else {
        above_y.clamp(
            FEATURE_PANEL_MARGIN,
            (screen_height - REC_ACTION_HEIGHT - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
        )
    };

    RecordingDeckLayout {
        left_toggle_rail: RectF {
            x: rail_x,
            y: rail_y,
            width: FEATURE_PANEL_ITEM_WIDTH,
            height: rail_height,
        },
        top_cluster: RectF {
            x: top_x,
            y: top_y,
            width: REC_TOP_CLUSTER_WIDTH,
            height: REC_TOP_CLUSTER_HEIGHT,
        },
        bottom_action_bar: RectF {
            x: action_x,
            y: action_y,
            width: action_width,
            height: REC_ACTION_HEIGHT,
        },
    }
}

fn compute_aspect_menu_rects(
    anchor_rect: RectF,
    screen_width: f64,
    screen_height: f64,
) -> (RectF, Vec<RectF>) {
    let item_h = 34.0;
    let menu_w = 196.0;
    let menu_h = (ASPECT_RATIO_OPTIONS.len() as f64 * item_h) + 10.0;
    let menu_x = (anchor_rect.x + anchor_rect.width / 2.0 - menu_w / 2.0)
        .clamp(10.0, screen_width - menu_w - 10.0);
    let menu_y = (anchor_rect.y + anchor_rect.height + 8.0)
        .clamp(10.0, screen_height - menu_h - 10.0);
    let panel_rect = RectF { x: menu_x, y: menu_y, width: menu_w, height: menu_h };
    let mut item_rects = Vec::with_capacity(ASPECT_RATIO_OPTIONS.len());
    for i in 0..ASPECT_RATIO_OPTIONS.len() {
        item_rects.push(RectF {
            x: menu_x + 5.0,
            y: menu_y + 5.0 + i as f64 * item_h,
            width: menu_w - 10.0,
            height: item_h,
        });
    }
    (panel_rect, item_rects)
}

fn capture_crop_menu_hit_item(
    selection_x: f64, selection_y: f64, selection_width: f64, selection_height: f64,
    screen_width: f64, screen_height: f64, x: f64, y: f64,
) -> Option<usize> {
    let layout = compute_toolbar_layout(selection_x, selection_y, selection_width, selection_height, screen_width, screen_height);
    let anchor = layout.crop_panel;
    let (_panel, items) = compute_aspect_menu_rects(anchor, screen_width, screen_height);
    items.iter().position(|r| r.contains(x, y))
}

fn recording_crop_menu_hit_item(
    selection_x: f64, selection_y: f64, selection_width: f64, selection_height: f64,
    screen_width: f64, screen_height: f64, x: f64, y: f64,
) -> Option<usize> {
    let deck = compute_recording_deck_layout(selection_x, selection_y, selection_width, selection_height, screen_width, screen_height);
    let top = deck.top_cluster;
    let anchor = RectF {
        x: top.x + 62.0 + 8.0 + 152.0 + 8.0,
        y: top.y,
        width: 62.0,
        height: top.height,
    };
    let (_panel, items) = compute_aspect_menu_rects(anchor, screen_width, screen_height);
    items.iter().position(|r| r.contains(x, y))
}

fn compute_dropdown_popup_y(menu_y: f64, item_idx: usize, tab: SettingsTab) -> f64 {
    let start_y = menu_y + 110.0;
    match tab {
        SettingsTab::Video => match item_idx {
            3 => start_y,              // res dropdown button at curr_y=110
            4 => start_y + 35.0 + 50.0, // fps dropdown at curr_y=195
            _ => start_y,
        },
        SettingsTab::Gif => match item_idx {
            6 => start_y + 50.0 + 60.0 + 45.0, // size dropdown at curr_y=265
            _ => start_y,
        },
        _ => start_y,
    }
}

fn settings_menu_hit_item(
    selection_x: f64, selection_y: f64, selection_width: f64, _selection_height: f64,
    screen_width: f64, screen_height: f64, x: f64, y: f64,
    tab: SettingsTab,
) -> Option<i32> {
    let menu_w = 440.0;
    let menu_h = 560.0;
    let menu_x = (selection_x + (selection_width - 440.0) / 2.0).clamp(10.0, screen_width - 450.0);
    let menu_y = (selection_y + 24.0).clamp(10.0, screen_height - 570.0);

    // Tab rects (always check, any tab)
    let tab_w = 78.0;
    let tab_h = 32.0;
    let tab_start_x = menu_x + (menu_w - 3.0 * tab_w) / 2.0;
    let tab_y = menu_y + 64.0;
    for i in 0..3 {
        let tr = RectF { x: tab_start_x + i as f64 * tab_w, y: tab_y, width: tab_w, height: tab_h };
        if tr.contains(x, y) {
            return Some(i);
        }
    }

    let row_at = |cy: f64| -> bool {
        let w = menu_w - (130.0 - menu_x) - 25.0;
        RectF { x: menu_x + 130.0, y: cy, width: w, height: 32.0 }.contains(x, y)
    };

    match tab {
        SettingsTab::General => {
            let check_area_at = |cy: f64| -> bool {
                let value_x = menu_x + 140.0;
                RectF { x: value_x, y: cy, width: menu_w - 160.0, height: 32.0 }.contains(x, y)
            };
            let mut cy = menu_y + 110.0;
            let mut idx = 3;
            for _ in 0..4 { if check_area_at(cy) { return Some(idx); } idx += 1; cy += 32.0; }
            cy += 10.0;
            for _ in 0..2 { if check_area_at(cy) { return Some(idx); } idx += 1; cy += 32.0; }
            cy += 10.0;
            { if check_area_at(cy) { return Some(idx); } idx += 1; cy += 32.0; }
            cy += 10.0;
            for _ in 0..3 { if check_area_at(cy) { return Some(idx); } idx += 1; cy += 32.0; }
        }
        SettingsTab::Video => {
            let mut cy = menu_y + 110.0;
            if row_at(cy) { return Some(3); } cy += 35.0;
            cy += 50.0;
            if row_at(cy) { return Some(4); } cy += 45.0;
            if row_at(cy) { return Some(5); } cy += 50.0;
            if row_at(cy) { return Some(6); }
        }
        SettingsTab::Gif => {
            let mut cy = menu_y + 110.0;
            if row_at(cy) { return Some(3); } cy += 50.0;
            if row_at(cy) { return Some(4); } cy += 60.0;
            if row_at(cy) { return Some(5); } cy += 45.0;
            if row_at(cy) { return Some(6); }
        }
    }

    None
}

fn toolbar_item_at(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    x: f64,
    y: f64,
) -> Option<ToolbarIcon> {
    match toolbar_hit_at(
        selection_x,
        selection_y,
        selection_width,
        selection_height,
        screen_width,
        screen_height,
        x,
        y,
    ) {
        Some(ToolbarHit::Tool(index)) => Some(TOOLBAR_ICONS[index]),
        _ => None,
    }
}

fn toolbar_hit_at(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    x: f64,
    y: f64,
) -> Option<ToolbarHit> {
    let layout = compute_toolbar_layout(
        selection_x,
        selection_y,
        selection_width,
        selection_height,
        screen_width,
        screen_height,
    );

    for (index, cell) in layout.item_cells.iter().enumerate() {
        if cell.contains(x, y) {
            return Some(ToolbarHit::Tool(index));
        }
    }

    if layout.size_panel.contains(x, y) {
        return Some(ToolbarHit::SizePanel);
    }
    if layout.crop_panel.contains(x, y) {
        return Some(ToolbarHit::CropPanel);
    }

    None
}

fn recording_tile_at(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    x: f64,
    y: f64,
) -> Option<RecordPanelTile> {
    let deck = compute_recording_deck_layout(
        selection_x,
        selection_y,
        selection_width,
        selection_height,
        screen_width,
        screen_height,
    );
    let top = deck.top_cluster;
    let controls = RectF {
        x: top.x,
        y: top.y,
        width: 62.0,
        height: top.height,
    };
    let size = RectF {
        x: controls.x + controls.width + ACTION_CARD_GAP,
        y: top.y,
        width: SIZE_CARD_WIDTH,
        height: top.height,
    };
    let crop = RectF {
        x: size.x + size.width + ACTION_CARD_GAP,
        y: top.y,
        width: CROP_CARD_WIDTH,
        height: top.height,
    };
    let rail = deck.left_toggle_rail;
    let rail_tiles = [
        (
            RecordPanelTile::Mic,
            RectF {
                x: rail.x,
                y: rail.y,
                width: rail.width,
                height: FEATURE_PANEL_HEIGHT,
            },
        ),
        (
            RecordPanelTile::Speaker,
            RectF {
                x: rail.x,
                y: rail.y + FEATURE_PANEL_HEIGHT,
                width: rail.width,
                height: FEATURE_PANEL_HEIGHT,
            },
        ),
        (
            RecordPanelTile::Webcam,
            RectF {
                x: rail.x,
                y: rail.y + FEATURE_PANEL_HEIGHT * 2.0,
                width: rail.width,
                height: FEATURE_PANEL_HEIGHT,
            },
        ),
        (
            RecordPanelTile::Clicks,
            RectF {
                x: rail.x,
                y: rail.y + FEATURE_PANEL_HEIGHT * 3.0,
                width: rail.width,
                height: FEATURE_PANEL_HEIGHT,
            },
        ),
        (
            RecordPanelTile::Keystrokes,
            RectF {
                x: rail.x,
                y: rail.y + FEATURE_PANEL_HEIGHT * 4.0,
                width: rail.width,
                height: FEATURE_PANEL_HEIGHT,
            },
        ),
    ];
    for (tile, rect) in [
        (RecordPanelTile::Controls, controls),
        (RecordPanelTile::Size, size),
        (RecordPanelTile::Crop, crop),
    ] {
        if rect.contains(x, y) {
            return Some(tile);
        }
    }
    for (tile, rect) in rail_tiles {
        if rect.contains(x, y) {
            return Some(tile);
        }
    }
    let actions = deck.bottom_action_bar;
    let video = RectF {
        x: actions.x,
        y: actions.y,
        width: REC_ACTION_WIDTH,
        height: actions.height,
    };
    let gif = RectF {
        x: video.x + video.width + ACTION_CARD_GAP,
        y: actions.y,
        width: REC_ACTION_WIDTH,
        height: actions.height,
    };
    if video.contains(x, y) {
        return Some(RecordPanelTile::RecordVideo);
    }
    if gif.contains(x, y) {
        return Some(RecordPanelTile::RecordGif);
    }

    None
}

fn draw_frosted_panel(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
) {
    // Drop shadow
    rounded_rect_path(context, x, y + 3.0, width, height, radius);
    context.set_source_rgba(0.0, 0.0, 0.0, 0.30);
    let _ = context.fill();

    // Frosted-glass fill: clip to the panel shape, then layer:
    //   1. Real blurred screenshot (scaled from the pre-blurred 1/4-res surface)
    //   2. Dark tint  — ensures readability on any background (incl. white)
    //   3. Subtle white highlight — gives the "glass" sheen
    let _ = context.save();
    rounded_rect_path(context, x, y, width, height, radius);
    context.clip();

    if let Some(background) = background {
        // Scale the blurred surface so it maps to screen coordinates.
        // The blur surface is 1/4 the original image size, so we scale by
        // (screen / blur_surface_size) to fill the screen, then the clip
        // reveals only the portion behind this panel.
        let blur_w = background.toolbar_blur_surface.width().max(1) as f64;
        let blur_h = background.toolbar_blur_surface.height().max(1) as f64;
        let scale_x = screen_width / blur_w;
        let scale_y = screen_height / blur_h;

        let _ = context.save();
        context.scale(scale_x, scale_y);
        if context
            .set_source_surface(&background.toolbar_blur_surface, 0.0, 0.0)
            .is_ok()
        {
            let _ = context.paint();
        }
        let _ = context.restore();

        // Dark glass tint matching editor root background (#141414 at ~90% opacity)
        context.set_source_rgba(20.0 / 255.0, 20.0 / 255.0, 20.0 / 255.0, 230.0 / 255.0);
        let _ = context.paint();
    } else {
        // No screenshot (X11 transparent overlay): solid dark base.
        context.set_source_rgba(0.08, 0.08, 0.08, 1.0);
        let _ = context.paint();
    }

    // Subtle white sheen (0.04 alpha) for a polished feel
    context.set_source_rgba(1.0, 1.0, 1.0, 10.0 / 255.0);
    let _ = context.paint();

    // Panel border (matching editor's .editor-root border: 1px solid rgba(255, 255, 255, 0.10))
    // Drawn inside clip so the outer half is clipped away (C++ behavior)
    context.set_source_rgba(1.0, 1.0, 1.0, 0.10);
    context.set_line_width(1.0);
    context.set_antialias(gtk4::cairo::Antialias::Default);
    rounded_rect_path(context, x, y, width, height, radius);
    let _ = context.stroke();
    let _ = context.restore();
}

fn draw_text_centered(
    context: &gtk4::cairo::Context,
    rect: RectF,
    text: &str,
    size: f64,
    bold: bool,
    rgba: (f64, f64, f64, f64),
) {
    let weight = if bold {
        gtk4::cairo::FontWeight::Bold
    } else {
        gtk4::cairo::FontWeight::Normal
    };
    context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, weight);
    context.set_font_size(size);
    context.set_source_rgba(rgba.0, rgba.1, rgba.2, rgba.3);
    if let Ok(extents) = context.text_extents(text) {
        let x = rect.x + rect.width / 2.0 - extents.width() / 2.0 - extents.x_bearing();
        let y = rect.y + rect.height / 2.0 - extents.height() / 2.0 - extents.y_bearing();
        context.move_to(x, y);
        let _ = context.show_text(text);
    }
}

fn draw_recording_panel(
    context: &gtk4::cairo::Context,
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
    hover_tile: Option<RecordPanelTile>,
    crop_menu_open: bool,
    record_aspect_ratio_index: usize,
    hovered_crop_menu_item: i32,
    settings_menu_open: bool,
    settings_tab: SettingsTab,
    hovered_settings_item: i32,
    settings_dropdown_open: Option<usize>,
    video_max_res: usize,
    video_fps: usize,
    record_mono: bool,
    open_editor: bool,
    rec_controls: bool,
    display_rec_time: bool,
    hidpi: bool,
    do_not_disturb: bool,
    show_cursor: bool,
    rec_clicks: bool,
    rec_keystrokes: bool,
    remember_selection: bool,
    dim_screen: bool,
    show_countdown: bool,
    gif_fps: f64,
    gif_quality: f64,
    optimize_gif: bool,
    gif_size_idx: usize,
) {
    let deck = compute_recording_deck_layout(
        selection_x,
        selection_y,
        selection_width,
        selection_height,
        screen_width,
        screen_height,
    );
    for panel in [
        deck.left_toggle_rail,
        deck.top_cluster,
        deck.bottom_action_bar,
    ] {
        draw_frosted_panel(
            context,
            panel.x,
            panel.y,
            panel.width,
            panel.height,
            10.0,
            screen_width,
            screen_height,
            background,
        );
    }

    let accent = |context: &gtk4::cairo::Context, rect: RectF, active: bool| {
        rounded_rect_path(
            context,
            rect.x + 3.0,
            rect.y + 3.0,
            rect.width - 6.0,
            rect.height - 6.0,
            9.0,
        );
        if active {
            context.set_source_rgba(176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 0.34);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 0.12);
        }
        let _ = context.fill();
    };

    let top = deck.top_cluster;
    let controls = RectF {
        x: top.x,
        y: top.y,
        width: 62.0,
        height: top.height,
    };
    let size = RectF {
        x: controls.x + controls.width + ACTION_CARD_GAP,
        y: top.y,
        width: SIZE_CARD_WIDTH,
        height: top.height,
    };
    let crop = RectF {
        x: size.x + size.width + ACTION_CARD_GAP,
        y: top.y,
        width: CROP_CARD_WIDTH,
        height: top.height,
    };
    let rail = deck.left_toggle_rail;
    let rail_tiles = [
        (RecordPanelTile::Mic, ToolbarIcon::Mic, "Mic", true),
        (
            RecordPanelTile::Speaker,
            ToolbarIcon::Speaker,
            "Speaker",
            false,
        ),
        (RecordPanelTile::Webcam, ToolbarIcon::Webcam, "Cam", false),
        (
            RecordPanelTile::Clicks,
            ToolbarIcon::Clicks,
            "Clicks",
            false,
        ),
        (
            RecordPanelTile::Keystrokes,
            ToolbarIcon::Keystrokes,
            "Keys",
            false,
        ),
    ];

    if hover_tile == Some(RecordPanelTile::Controls) {
        accent(context, controls, false);
    }
    draw_toolbar_icon(
        context,
        ToolbarIcon::Controls,
        controls.x + controls.width / 2.0 + 0.6,
        controls.y + 28.8,
        (0.0, 0.0, 0.0, 0.42),
    );
    draw_toolbar_icon(
        context,
        ToolbarIcon::Controls,
        controls.x + controls.width / 2.0,
        controls.y + 28.0,
        (1.0, 1.0, 1.0, 0.96),
    );

    if hover_tile == Some(RecordPanelTile::Size) {
        accent(context, size, false);
    }
    draw_text_centered(
        context,
        RectF {
            x: size.x,
            y: size.y + 8.0,
            width: size.width,
            height: 12.0,
        },
        "FRAME",
        9.6,
        true,
        (1.0, 224.0 / 255.0, 196.0 / 255.0, 0.80),
    );
    draw_text_centered(
        context,
        RectF {
            x: size.x,
            y: size.y + 20.0,
            width: size.width,
            height: 20.0,
        },
        &format!("{}×{}", selection_width as i32, selection_height as i32),
        14.7,
        true,
        (0.96, 0.96, 0.97, 1.0),
    );

    if hover_tile == Some(RecordPanelTile::Crop) {
        accent(context, crop, false);
    }
    draw_toolbar_icon(
        context,
        ToolbarIcon::Crop,
        crop.x + crop.width / 2.0 + 0.6,
        crop.y + 28.8,
        (0.0, 0.0, 0.0, 0.42),
    );
    draw_toolbar_icon(
        context,
        ToolbarIcon::Crop,
        crop.x + crop.width / 2.0,
        crop.y + 28.0,
        (1.0, 1.0, 1.0, 0.96),
    );

    for (index, (tile, icon, label, active)) in rail_tiles.iter().enumerate() {
        let rect = RectF {
            x: rail.x,
            y: rail.y + FEATURE_PANEL_HEIGHT * index as f64,
            width: rail.width,
            height: FEATURE_PANEL_HEIGHT,
        };
        let hovered = hover_tile == Some(*tile);
        if hovered || *active {
            accent(context, rect, *active);
        }
        let color = if *active {
            (1.0, 229.0 / 255.0, 206.0 / 255.0, 1.0)
        } else {
            (1.0, 1.0, 1.0, if hovered { 1.0 } else { 0.94 })
        };
        draw_toolbar_icon(
            context,
            *icon,
            rect.x + rect.width / 2.0 + 0.6,
            rect.y + 20.8,
            (0.0, 0.0, 0.0, 0.44),
        );
        draw_toolbar_icon(
            context,
            *icon,
            rect.x + rect.width / 2.0,
            rect.y + 20.0,
            color,
        );
        draw_text_centered(
            context,
            RectF {
                x: rect.x,
                y: rect.y + 38.0,
                width: rect.width,
                height: 18.0,
            },
            label,
            10.7,
            hovered || *active,
            color,
        );
    }

    let actions = deck.bottom_action_bar;
    let video = RectF {
        x: actions.x,
        y: actions.y,
        width: REC_ACTION_WIDTH,
        height: actions.height,
    };
    let gif = RectF {
        x: video.x + video.width + ACTION_CARD_GAP,
        y: actions.y,
        width: REC_ACTION_WIDTH,
        height: actions.height,
    };
    for (rect, tile, icon, label, primary) in [
        (
            video,
            RecordPanelTile::RecordVideo,
            ToolbarIcon::Video,
            "Video",
            true,
        ),
        (
            gif,
            RecordPanelTile::RecordGif,
            ToolbarIcon::Gif,
            "GIF",
            false,
        ),
    ] {
        let hovered = hover_tile == Some(tile);
        if hovered {
            let hr = RectF { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
            rounded_rect_path(context, hr.x, hr.y, hr.width, hr.height, 10.0);
            context.set_source_rgba(1.0, 1.0, 1.0, 0.09);
            let _ = context.fill();
        }
        let (path_x, path_y, path_w, path_h) = (rect.x + 3.0, rect.y + 3.0, rect.width - 6.0, rect.height - 6.0);
        rounded_rect_path(context, path_x, path_y, path_w, path_h, 9.0);
        if primary || hovered {
            context.set_source_rgba(176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 88.0 / 255.0);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 18.0 / 255.0);
        }
        let _ = context.fill();
        let _ = context.save();
        rounded_rect_path(context, path_x, path_y, path_w, path_h, 9.0);
        context.clip();
        rounded_rect_path(context, rect.x + 3.8, rect.y + 3.8, rect.width - 7.6, rect.height - 7.6, 8.4);
        if primary || hovered {
            context.set_source_rgba(255.0 / 255.0, 212.0 / 255.0, 178.0 / 255.0, 152.0 / 255.0);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 110.0 / 255.0);
        }
        context.set_line_width(1.1);
        let _ = context.stroke();
        let _ = context.restore();
        let icon_alpha = if hovered || primary { 1.0 } else { 0.94 };
        let shadow_alpha = if hovered { 0.24 } else if primary { 0.32 } else { 0.50 };
        let icon_y = rect.y + rect.height / 2.0 - if hovered { 0.5 } else { 0.0 };
        draw_toolbar_icon(context, icon, rect.x + 28.6, icon_y + 0.8, (0.0, 0.0, 0.0, shadow_alpha));
        draw_toolbar_icon(context, icon, rect.x + 28.0, icon_y, (1.0, 1.0, 1.0, icon_alpha));
        context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
        context.set_font_size(15.7);
        context.set_source_rgba(0.0, 0.0, 0.0, shadow_alpha);
        context.move_to(rect.x + 50.6, rect.y + 30.8);
        let _ = context.show_text(label);
        if primary {
            context.set_source_rgba(1.0, 232.0 / 255.0, 214.0 / 255.0, icon_alpha);
        } else {
            context.set_source_rgba(245.0 / 255.0, 245.0 / 255.0, 246.0 / 255.0, icon_alpha);
        }
        context.move_to(rect.x + 50.0, rect.y + 30.0);
        let _ = context.show_text(label);
    }

    // Recording crop menu dropdown
    if crop_menu_open {
        draw_recording_crop_menu(
            context,
            crop,
            hovered_crop_menu_item,
            record_aspect_ratio_index,
            screen_width,
            screen_height,
            background,
        );
    }

    // Settings menu (replaces panel content) — positioned like C++:
    // contextualX = clamp(selX + (selW - 440) / 2, 10, screenW - 450)
    // contextualY = clamp(selY + 24, 10, screenH - 570)
    if settings_menu_open {
        let panel_x = (selection_x + (selection_width - 440.0) / 2.0).clamp(10.0, screen_width - 450.0);
        let panel_y = (selection_y + 24.0).clamp(10.0, screen_height - 570.0);
        draw_settings_menu(
            context, panel_x, panel_y,
            screen_width, screen_height, background,
            settings_tab, hovered_settings_item, settings_dropdown_open,
            video_max_res, video_fps, record_mono, open_editor,
            rec_controls, display_rec_time, hidpi, do_not_disturb,
            show_cursor, rec_clicks, rec_keystrokes,
            remember_selection, dim_screen, show_countdown,
            gif_fps, gif_quality, optimize_gif, gif_size_idx,
        );
    }
}

fn draw_aspect_ratio_menu(
    context: &gtk4::cairo::Context,
    anchor_rect: RectF,
    hovered_item: i32,
    selected_index: usize,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
) -> Vec<RectF> {
    let item_h = 34.0;
    let menu_w = 196.0;
    let menu_h = (ASPECT_RATIO_OPTIONS.len() as f64 * item_h) + 10.0;
    let menu_x = (anchor_rect.x + anchor_rect.width / 2.0 - menu_w / 2.0)
        .clamp(10.0, screen_width - menu_w - 10.0);
    let menu_y = (anchor_rect.y + anchor_rect.height + 8.0)
        .clamp(10.0, screen_height - menu_h - 10.0);

    draw_frosted_panel(context, menu_x, menu_y, menu_w, menu_h, 12.0, screen_width, screen_height, background);

    let mut item_rects = Vec::with_capacity(ASPECT_RATIO_OPTIONS.len());
    for i in 0..ASPECT_RATIO_OPTIONS.len() {
        let item_rect = RectF { x: menu_x + 5.0, y: menu_y + 5.0 + i as f64 * item_h, width: menu_w - 10.0, height: item_h };
        let indicator_x = item_rect.x + 8.0;

        if i as i32 == hovered_item {
            rounded_rect_path(context, item_rect.x, item_rect.y, item_rect.width, item_rect.height, 7.0);
            context.set_source_rgba(1.0, 1.0, 1.0, 18.0 / 255.0);
            let _ = context.fill();
        }

        let selected = i == selected_index;
        if selected {
            rounded_rect_path(context, item_rect.x + 1.0, item_rect.y + 1.0, item_rect.width - 2.0, item_rect.height - 2.0, 7.0);
            context.set_source_rgba(176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 94.0 / 255.0);
            let _ = context.fill();
            context.set_source_rgba(1.0, 238.0 / 255.0, 224.0 / 255.0, 1.0);
            context.set_line_width(1.5);
            let cy = item_rect.y + item_rect.height / 2.0;
            context.move_to(indicator_x + 3.5, cy);
            context.line_to(indicator_x + 6.5, cy + 3.0);
            context.line_to(indicator_x + 12.5, cy - 4.0);
            context.stroke().ok();
        }

        context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal,
            if selected { gtk4::cairo::FontWeight::Bold } else { gtk4::cairo::FontWeight::Normal });
        context.set_font_size(13.3);
        let label = ASPECT_RATIO_OPTIONS[i];
        if let Ok(extents) = context.text_extents(label) {
            let label_x = item_rect.x + 30.0 - extents.x_bearing();
            let label_y = item_rect.y + item_rect.height / 2.0 - extents.height() / 2.0 - extents.y_bearing();
            let label_color = if selected { (1.0, 240.0 / 255.0, 226.0 / 255.0, 1.0) } else { (242.0 / 255.0, 242.0 / 255.0, 244.0 / 255.0, 1.0) };
            context.set_source_rgba(label_color.0, label_color.1, label_color.2, label_color.3);
            context.move_to(label_x, label_y);
            let _ = context.show_text(label);
        }

        item_rects.push(item_rect);
    }
    item_rects
}

fn draw_capture_crop_menu(
    context: &gtk4::cairo::Context,
    crop_card_rect: RectF,
    hovered_item: i32,
    selected_index: usize,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
) -> Vec<RectF> {
    draw_aspect_ratio_menu(context, crop_card_rect, hovered_item, selected_index, screen_width, screen_height, background)
}

fn draw_recording_crop_menu(
    context: &gtk4::cairo::Context,
    crop_tile_rect: RectF,
    hovered_item: i32,
    selected_index: usize,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
) -> Vec<RectF> {
    draw_aspect_ratio_menu(context, crop_tile_rect, hovered_item, selected_index, screen_width, screen_height, background)
}

fn draw_checkbox(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    size: f64,
    checked: bool,
    disabled: bool,
    accent_r: f64,
    accent_g: f64,
    accent_b: f64,
) {
    context.new_path();
    rounded_rect_path(context, x, y, size, size, 4.0);
    if checked && !disabled {
        context.set_source_rgba(accent_r, accent_g, accent_b, 1.0);
        context.fill().ok();
        context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        context.set_line_width(2.0);
        context.move_to(x + 4.0, y + size * 0.5);
        context.line_to(x + 8.0, y + size * 0.75);
        context.line_to(x + size - 3.5, y + size * 0.3);
        context.stroke().ok();
    } else {
        let alpha = if disabled { 35.0 / 255.0 } else { 60.0 / 255.0 };
        let bg_alpha = if disabled { 25.0 / 255.0 } else { 40.0 / 255.0 };
        context.set_source_rgba(0.0, 0.0, 0.0, bg_alpha);
        context.fill().ok();
        context.set_source_rgba(1.0, 1.0, 1.0, alpha);
        context.set_line_width(1.5);
        context.stroke().ok();
    }
}

fn draw_dropdown_button(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    label: &str,
    hovered: bool,
) {
    context.set_source_rgba(0.0, 0.0, 0.0, 60.0 / 255.0);
    rounded_rect_path(context, x, y, w, h, 6.0);
    if hovered {
        context.set_source_rgba(1.0, 1.0, 1.0, 20.0 / 255.0);
    }
    context.fill().ok();
    context.set_source_rgba(1.0, 1.0, 1.0, 40.0 / 255.0);
    context.set_line_width(1.0);
    rounded_rect_path(context, x, y, w, h, 6.0);
    context.stroke().ok();

    context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    context.set_font_size(13.3);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    if let Ok(extents) = context.text_extents(label) {
        context.move_to(x + 10.0 - extents.x_bearing(), y + h / 2.0 - extents.height() / 2.0 - extents.y_bearing());
        context.show_text(label).ok();
    }
    // Chevron
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.set_line_width(1.5);
    context.move_to(x + w - 15.0, y + h / 2.0 - 3.0);
    context.line_to(x + w - 11.0, y + h / 2.0 + 1.0);
    context.line_to(x + w - 7.0, y + h / 2.0 - 3.0);
    context.stroke().ok();
}

fn draw_settings_menu(
    context: &gtk4::cairo::Context,
    panel_x: f64,
    panel_y: f64,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
    tab: SettingsTab,
    hovered_item: i32,
    dropdown_open: Option<usize>,
    video_max_res: usize,
    video_fps: usize,
    record_mono: bool,
    open_editor: bool,
    rec_controls: bool,
    display_rec_time: bool,
    hidpi: bool,
    do_not_disturb: bool,
    show_cursor: bool,
    rec_clicks: bool,
    rec_keystrokes: bool,
    remember_selection: bool,
    dim_screen: bool,
    show_countdown: bool,
    gif_fps: f64,
    gif_quality: f64,
    optimize_gif: bool,
    gif_size_idx: usize,
) {
    let menu_w = 440.0;
    let menu_h = 560.0;
    let menu_x = panel_x.clamp(10.0, screen_width - menu_w - 10.0);
    let menu_y = panel_y.clamp(10.0, screen_height - menu_h - 10.0);

    let accent_r = 176.0 / 255.0;
    let accent_g = 92.0 / 255.0;
    let accent_b = 56.0 / 255.0;
    let accent_rim = (1.0, 214.0 / 255.0, 186.0 / 255.0);

    // Glow
    let _ = context.save();
    let glow_cx = menu_x + menu_w / 2.0;
    let glow_cy = menu_y + menu_h / 2.0;
    let glow = gtk4::cairo::RadialGradient::new(glow_cx, glow_cy, 0.0, glow_cx, glow_cy, menu_w);
    glow.add_color_stop_rgba(0.0, accent_r, accent_g, accent_b, 40.0 / 255.0);
    glow.add_color_stop_rgba(0.6, 0.0, 0.0, 0.0, 0.0);
    let _ = context.set_source(&glow);
    context.rectangle(menu_x - 40.0, menu_y - 40.0, menu_w + 80.0, menu_h + 80.0);
    let _ = context.fill();
    let _ = context.restore();

    draw_frosted_panel(context, menu_x, menu_y, menu_w, menu_h, 12.0, screen_width, screen_height, background);

    // Header
    context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
    context.set_font_size(10.7);
    context.set_source_rgba(1.0, 224.0 / 255.0, 196.0 / 255.0, 176.0 / 255.0);
    if let Ok(_ext) = context.text_extents("RECORDING CONTROLS") {
        context.move_to(menu_x + 18.0, menu_y + 28.0);
        context.show_text("RECORDING CONTROLS").ok();
    }
    context.set_font_size(18.7);
    context.set_source_rgba(245.0 / 255.0, 245.0 / 255.0, 246.0 / 255.0, 1.0);
    if let Ok(_ext) = context.text_extents("Recording Setup") {
        context.move_to(menu_x + 18.0, menu_y + 48.0);
        context.show_text("Recording Setup").ok();
    }

    // Tabs
    let tabs = ["General", "Video", "GIF"];
    let tab_w = 78.0;
    let tab_h = 32.0;
    let tab_start_x = menu_x + (menu_w - tabs.len() as f64 * tab_w) / 2.0;
    let tab_y = menu_y + 64.0;

    for (i, tab_label) in tabs.iter().enumerate() {
        let tr = RectF { x: tab_start_x + i as f64 * tab_w, y: tab_y, width: tab_w, height: tab_h };
        let is_active_tab = (i == 0 && matches!(tab, SettingsTab::General))
            || (i == 1 && matches!(tab, SettingsTab::Video))
            || (i == 2 && matches!(tab, SettingsTab::Gif));
        let tab_hovered = hovered_item == i as i32;
        if is_active_tab || tab_hovered {
            if is_active_tab {
                context.set_source_rgba(accent_r, accent_g, accent_b, 84.0 / 255.0);
            } else {
                context.set_source_rgba(1.0, 1.0, 1.0, 14.0 / 255.0);
            }
            rounded_rect_path(context, tr.x, tr.y, tr.width, tr.height, 9.0);
            context.fill().ok();
            if is_active_tab {
                let _ = context.save();
                rounded_rect_path(context, tr.x, tr.y, tr.width, tr.height, 9.0);
                context.clip();
                rounded_rect_path(context, tr.x + 0.5, tr.y + 0.5, tr.width - 1.0, tr.height - 1.0, 8.5);
                context.set_source_rgba(accent_rim.0, accent_rim.1, accent_rim.2, 1.0);
                context.set_line_width(1.0);
                context.stroke().ok();
                let _ = context.restore();
            }
        }
        let tab_text_color = if is_active_tab || tab_hovered {
            (1.0, 236.0 / 255.0, 220.0 / 255.0, 1.0)
        } else {
            (1.0, 1.0, 1.0, 150.0 / 255.0)
        };
        context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal,
            if is_active_tab || tab_hovered { gtk4::cairo::FontWeight::Bold } else { gtk4::cairo::FontWeight::Normal });
        context.set_font_size(13.7);
        context.set_source_rgba(tab_text_color.0, tab_text_color.1, tab_text_color.2, tab_text_color.3);
        if let Ok(extents) = context.text_extents(tab_label) {
            context.move_to(tr.x + tr.width / 2.0 - extents.width() / 2.0 - extents.x_bearing(),
                tr.y + tr.height / 2.0 - extents.height() / 2.0 - extents.y_bearing());
            context.show_text(tab_label).ok();
        }
    }

    match tab {
        SettingsTab::General => draw_settings_general_tab(context, menu_x, menu_y, menu_w, hovered_item,
            rec_controls, display_rec_time, hidpi, do_not_disturb, show_cursor, rec_clicks, rec_keystrokes,
            remember_selection, dim_screen, show_countdown, accent_r, accent_g, accent_b),
        SettingsTab::Video => draw_settings_video_tab(context, menu_x, menu_y, menu_w, hovered_item,
            video_max_res, video_fps, record_mono, open_editor, accent_r, accent_g, accent_b),
        SettingsTab::Gif => draw_settings_gif_tab(context, menu_x, menu_y, menu_w, hovered_item,
            gif_fps, gif_quality, optimize_gif, gif_size_idx, accent_r, accent_g, accent_b),
    }

    if let Some(drop_idx) = dropdown_open {
        draw_settings_dropdown_popup(context, menu_x, menu_y, menu_w, tab, drop_idx,
            hovered_item, video_max_res, video_fps, gif_size_idx, accent_r, accent_g, accent_b);
    }
}

fn draw_settings_general_tab(
    context: &gtk4::cairo::Context,
    menu_x: f64, menu_y: f64, menu_w: f64,
    hovered_item: i32,
    rec_controls: bool, display_rec_time: bool, hidpi: bool, do_not_disturb: bool,
    show_cursor: bool, rec_clicks: bool, rec_keystrokes: bool,
    remember_selection: bool, dim_screen: bool, show_countdown: bool,
    _accent_r: f64, _accent_g: f64, _accent_b: f64,
) {
    let label_x = menu_x + 25.0;
    let value_x = menu_x + 140.0;
    let check_area_w = menu_w - (value_x - menu_x) - 20.0; // 280
    let desc_x = value_x + 28.0;
    let row_h = 32.0;
    let mut y = menu_y + 110.0;
    let mut idx = 3;

    macro_rules! s {
        ($label:expr, $desc:expr, $checked:expr) => {{
            draw_general_row(context, label_x, value_x, desc_x, check_area_w, y, row_h, $label, $desc, $checked, false, hovered_item == idx);
            idx += 1;
            y += row_h;
        }};
        ($label:expr, $desc:expr, $checked:expr, $gap:expr) => {{
            y += $gap;
            draw_general_row(context, label_x, value_x, desc_x, check_area_w, y, row_h, $label, $desc, $checked, false, hovered_item == idx);
            idx += 1;
            y += row_h;
        }};
    }

    s!("Controls", "Use keyboard shortcuts", rec_controls);
    s!("Menu bar", "Display time in top bar", display_rec_time);
    s!("HiDPI", "Record at display scale res", hidpi);
    s!("Notifications", "DND while recording", do_not_disturb);
    s!("Cursor", "Show cursor", show_cursor, 10.0);
    s!("", "Highlight clicks", rec_clicks);
    s!("Keyboard", "Show keystrokes", rec_keystrokes, 10.0);
    s!("Recording area", "Remember last selection", remember_selection, 10.0);
    s!("", "Dim screen while recording", dim_screen);
    s!("", "Show countdown", show_countdown);
    let _ = y; let _ = idx;
}

fn draw_general_row(
    context: &gtk4::cairo::Context,
    label_x: f64, value_x: f64, desc_x: f64, _check_area_w: f64,
    y: f64, row_h: f64,
    label: &str, desc: &str, checked: bool, disabled: bool, hover: bool,
) {
    if !label.is_empty() {
        context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
        context.set_font_size(13.3);
        context.set_source_rgba(1.0, 1.0, 1.0, if disabled { 110.0 / 255.0 } else { 200.0 / 255.0 });
        if let Ok(extents) = context.text_extents(label) {
            // Right-aligned in 110px area starting at label_x
            let tx = label_x + 110.0 - extents.width() - extents.x_bearing();
            context.move_to(tx, y + row_h / 2.0 - extents.height() / 2.0 - extents.y_bearing());
            context.show_text(label).ok();
        }
    }
    if hover {
        rounded_rect_path(context, value_x - 5.0, y, 290.0, row_h, 6.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 12.0 / 255.0);
        context.fill().ok();
    }
    let cb_size = 18.0;
    draw_checkbox(context, value_x, y + (row_h - cb_size) / 2.0, cb_size, checked, disabled, 176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0);
    context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    context.set_font_size(13.3);
    context.set_source_rgba(1.0, 1.0, 1.0, if disabled { 110.0 / 255.0 } else { 1.0 });
    if let Ok(extents) = context.text_extents(desc) {
        // Clip description to available width (252px like C++)
        let max_desc_w = 252.0;
        if extents.width() > max_desc_w {
            let _ = context.save();
            context.rectangle(desc_x, y, max_desc_w, row_h);
            context.clip();
        }
        context.move_to(desc_x - extents.x_bearing(), y + row_h / 2.0 - extents.height() / 2.0 - extents.y_bearing());
        context.show_text(desc).ok();
        if extents.width() > max_desc_w {
            let _ = context.restore();
        }
    }
}

fn draw_settings_video_tab(
    context: &gtk4::cairo::Context,
    menu_x: f64, menu_y: f64, menu_w: f64,
    hovered_item: i32,
    video_max_res: usize, video_fps: usize, record_mono: bool, open_editor: bool,
    _accent_r: f64, _accent_g: f64, _accent_b: f64,
) {
    let label_x = menu_x + 20.0;
    let value_x = menu_x + 130.0;
    let mut curr_y = menu_y + 110.0;

    let draw_label = |context: &gtk4::cairo::Context, txt: &str, y: f64| {
        context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
        context.set_font_size(13.3);
        context.set_source_rgba(1.0, 1.0, 1.0, 200.0 / 255.0);
        if let Ok(extents) = context.text_extents(txt) {
            context.move_to(label_x + 100.0 - extents.width() - extents.x_bearing(), y + 20.0 - extents.height() / 2.0 - extents.y_bearing());
            context.show_text(txt).ok();
        }
    };

    // Max resolution
    draw_label(context, "Max resolution:", curr_y);
    let res_options = ["Original", "1080p", "720p"];
    draw_dropdown_button(context, value_x, curr_y, 140.0, 30.0, res_options[video_max_res], hovered_item == 3);
    curr_y += 35.0;
    context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    context.set_font_size(12.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 120.0 / 255.0);
    // Clip subtext to prevent overflow
    let _ = context.save();
    context.rectangle(value_x, curr_y, menu_w - (value_x - menu_x) - 25.0, 80.0);
    context.clip();
    if let Ok(extents) = context.text_extents("Set max res to reduce file size") {
        context.move_to(value_x - extents.x_bearing(), curr_y + 16.0 - extents.height() / 2.0 - extents.y_bearing());
        context.show_text("Set max res to reduce file size").ok();
    }
    let _ = context.restore();
    curr_y += 50.0;

    // Video FPS
    draw_label(context, "Video FPS:", curr_y);
    let fps_options = ["24", "30", "50", "60"];
    draw_dropdown_button(context, value_x, curr_y, 80.0, 30.0, fps_options[video_fps], hovered_item == 4);
    curr_y += 45.0;

    // Record mono
    let mono_hovered = hovered_item == 5;
    if mono_hovered {
        let r = RectF { x: value_x, y: curr_y, width: 200.0, height: 30.0 };
        rounded_rect_path(context, r.x - 5.0, r.y, r.width + 10.0, r.height, 6.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 12.0 / 255.0);
        context.fill().ok();
    }
    draw_checkbox(context, value_x, curr_y + (30.0 - 18.0) / 2.0, 18.0, record_mono, false, 176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0);
    context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    context.set_font_size(13.3);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    if let Ok(extents) = context.text_extents("Record audio in mono") {
        context.move_to(value_x + 28.0 - extents.x_bearing(), curr_y + 15.0 - extents.height() / 2.0 - extents.y_bearing());
        context.show_text("Record audio in mono").ok();
    }
    curr_y += 50.0;

    // Open editor
    draw_label(context, "Video Encoder:", curr_y);
    let encoder_hovered = hovered_item == 6;
    if encoder_hovered {
        let r = RectF { x: value_x, y: curr_y, width: 250.0, height: 30.0 };
        rounded_rect_path(context, r.x - 5.0, r.y, r.width + 10.0, r.height, 6.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 12.0 / 255.0);
        context.fill().ok();
    }
    draw_checkbox(context, value_x, curr_y + (30.0 - 18.0) / 2.0, 18.0, open_editor, false, 176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0);
    context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    context.set_font_size(13.3);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    if let Ok(extents) = context.text_extents("Open editor after recording") {
        context.move_to(value_x + 28.0 - extents.x_bearing(), curr_y + 15.0 - extents.height() / 2.0 - extents.y_bearing());
        context.show_text("Open editor after recording").ok();
    }
    // Clip remaining editor subtext
    curr_y += 35.0;
    let _ = context.save();
    context.rectangle(value_x, curr_y, menu_w - (value_x - menu_x) - 25.0, 80.0);
    context.clip();
    context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    context.set_font_size(12.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 120.0 / 255.0);
    if let Ok(extents) = context.text_extents("Use editor to change quality and audio") {
        context.move_to(value_x - extents.x_bearing(), curr_y + 16.0 - extents.height() / 2.0 - extents.y_bearing());
        context.show_text("Use editor to change quality and audio").ok();
    }
    let _ = context.restore();
    let _ = curr_y;
}

fn draw_settings_gif_tab(
    context: &gtk4::cairo::Context,
    menu_x: f64, menu_y: f64, _menu_w: f64,
    hovered_item: i32,
    gif_fps: f64, gif_quality: f64, optimize_gif: bool, gif_size_idx: usize,
    _accent_r: f64, _accent_g: f64, _accent_b: f64,
) {
    let label_x = menu_x + 20.0;
    let value_x = menu_x + 130.0;
    let mut curr_y = menu_y + 110.0;

    let draw_label = |context: &gtk4::cairo::Context, txt: &str, y: f64| {
        context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
        context.set_font_size(13.3);
        context.set_source_rgba(1.0, 1.0, 1.0, 200.0 / 255.0);
        if let Ok(extents) = context.text_extents(txt) {
            context.move_to(label_x + 100.0 - extents.width() - extents.x_bearing(), y + 20.0 - extents.height() / 2.0 - extents.y_bearing());
            context.show_text(txt).ok();
        }
    };

    // GIF FPS
    draw_label(context, "GIF FPS:", curr_y);
    let fps_label = format!("{:.0}", gif_fps);
    context.set_source_rgba(0.0, 0.0, 0.0, 80.0 / 255.0);
    rounded_rect_path(context, value_x, curr_y, 45.0, 30.0, 6.0);
    context.fill().ok();
    context.set_source_rgba(1.0, 1.0, 1.0, 28.0 / 255.0);
    context.set_line_width(1.0);
    rounded_rect_path(context, value_x, curr_y, 45.0, 30.0, 6.0);
    context.stroke().ok();
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    context.set_font_size(13.3);
    if let Ok(extents) = context.text_extents(&fps_label) {
        context.move_to(value_x + 22.5 - extents.width() / 2.0 - extents.x_bearing(), curr_y + 15.0 - extents.height() / 2.0 - extents.y_bearing());
        context.show_text(&fps_label).ok();
    }
    // FPS slider
    let slider_x = value_x + 55.0;
    let slider_w = 220.0;
    let track_y = curr_y + (30.0 - 4.0) / 2.0;
    let progress = ((gif_fps - 5.0) / 55.0).clamp(0.0, 1.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 30.0 / 255.0);
    rounded_rect_path(context, slider_x, track_y, slider_w, 4.0, 2.0);
    context.fill().ok();
    context.set_source_rgba(176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 1.0);
    rounded_rect_path(context, slider_x, track_y, slider_w * progress, 4.0, 2.0);
    context.fill().ok();
    let handle_x = slider_x + progress * slider_w;
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.new_path();
    context.arc(handle_x, curr_y + 15.0, 10.0, 0.0, PI * 2.0);
    context.fill().ok();
    curr_y += 50.0;

    // GIF Quality
    draw_label(context, "GIF quality:", curr_y);
    let q_slider_w = 160.0;
    let q_track_y = curr_y + (30.0 - 4.0) / 2.0;
    context.set_source_rgba(1.0, 1.0, 1.0, 30.0 / 255.0);
    rounded_rect_path(context, value_x, q_track_y, q_slider_w, 4.0, 2.0);
    context.fill().ok();
    // Ticks
    context.set_source_rgba(1.0, 1.0, 1.0, 60.0 / 255.0);
    context.set_line_width(1.0);
    for i in 0..=8 {
        let tx = value_x + (q_slider_w / 8.0) * i as f64;
        context.move_to(tx, curr_y + 15.0 - 5.0);
        context.line_to(tx, curr_y + 15.0 + 5.0);
        context.stroke().ok();
    }
    // Quality handle
    let q_handle_x = value_x + gif_quality * q_slider_w;
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    rounded_rect_path(context, q_handle_x - 5.0, curr_y + (30.0 - 18.0) / 2.0, 10.0, 18.0, 3.0);
    context.fill().ok();
    context.set_font_size(10.7);
    context.set_source_rgba(1.0, 1.0, 1.0, 120.0 / 255.0);
    if let Ok(_ext) = context.text_extents("Low") {
        context.move_to(value_x, curr_y + 46.0);
        context.show_text("Low").ok();
    }
    if let Ok(_ext) = context.text_extents("High") {
        context.move_to(value_x + q_slider_w - 40.0, curr_y + 46.0);
        context.show_text("High").ok();
    }
    curr_y += 60.0;

    // Optimize checkbox
    draw_checkbox(context, value_x, curr_y + (30.0 - 18.0) / 2.0, 18.0, optimize_gif, false, 176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0);
    context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    context.set_font_size(13.3);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    if let Ok(extents) = context.text_extents("Optimize GIFs") {
        context.move_to(value_x + 25.0 - extents.x_bearing(), curr_y + 15.0 - extents.height() / 2.0 - extents.y_bearing());
        context.show_text("Optimize GIFs").ok();
    }
    curr_y += 45.0;

    // GIF size
    draw_label(context, "GIF size:", curr_y);
    let size_options = ["800 x auto", "640 x auto", "480 x auto", "Original"];
    draw_dropdown_button(context, value_x, curr_y, 180.0, 30.0, size_options[gif_size_idx], hovered_item == 6);
}

fn draw_settings_dropdown_popup(
    context: &gtk4::cairo::Context,
    menu_x: f64, menu_y: f64, _menu_w: f64,
    tab: SettingsTab,
    drop_idx: usize,
    _hovered_item: i32,
    video_max_res: usize, video_fps: usize, gif_size_idx: usize,
    accent_r: f64, accent_g: f64, accent_b: f64,
) {
    let (options, current_val): (&[&str], usize) = match (tab, drop_idx) {
        (SettingsTab::Video, 3) => (&["Original", "1080p", "720p"], video_max_res),
        (SettingsTab::Video, 4) => (&["24", "30", "50", "60"], video_fps),
        (SettingsTab::Gif, 6) => (&["800 x auto", "640 x auto", "480 x auto", "Original"], gif_size_idx),
        _ => return,
    };
    let value_x = menu_x + 130.0;
    let popup_y = compute_dropdown_popup_y(menu_y, drop_idx, tab);
    let item_h = 30.0;
    let popup_w = 140.0;
    if options.is_empty() { return; }
    let popup_h = options.len() as f64 * item_h;
    draw_frosted_panel(context, value_x, popup_y, popup_w, popup_h, 8.0, 0.0, 0.0, None);
    for (i, opt) in options.iter().enumerate() {
        let r = RectF { x: value_x, y: popup_y + i as f64 * item_h, width: popup_w, height: item_h };
        if i == current_val {
            let _ = context.save();
            rounded_rect_path(context, r.x + 2.0, r.y + 2.0, r.width - 4.0, r.height - 2.0, 5.0);
            context.set_source_rgba(accent_r, accent_g, accent_b, 84.0 / 255.0);
            context.fill().ok();
            let _ = context.restore();
        }
        context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
        context.set_font_size(13.3);
        context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        if let Ok(extents) = context.text_extents(opt) {
            context.move_to(r.x + 10.0 - extents.x_bearing(), r.y + item_h / 2.0 - extents.height() / 2.0 - extents.y_bearing());
            context.show_text(opt).ok();
        }
    }
}

fn draw_toolbar_icon(
    context: &gtk4::cairo::Context,
    icon: ToolbarIcon,
    cx: f64,
    cy: f64,
    color: (f64, f64, f64, f64),
) {
    let _ = context.save();
    context.new_path();
    context.set_source_rgba(color.0, color.1, color.2, color.3);
    context.set_line_width(1.6);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(gtk4::cairo::LineJoin::Round);

    match icon {
        ToolbarIcon::Capture => {
            context.arc(cx, cy, 6.2, 0.0, PI * 2.0);
            let _ = context.stroke();
            context.move_to(cx - 3.2, cy);
            context.line_to(cx + 3.2, cy);
            context.move_to(cx, cy - 3.2);
            context.line_to(cx, cy + 3.2);
            let _ = context.stroke();
        }
        ToolbarIcon::Area => {
            let h = 5.5;
            context.move_to(cx - 7.0, cy - 1.5);
            context.line_to(cx - 7.0, cy - h);
            context.line_to(cx - 1.5, cy - h);

            context.move_to(cx + 1.5, cy - h);
            context.line_to(cx + 7.0, cy - h);
            context.line_to(cx + 7.0, cy - 1.5);

            context.move_to(cx - 7.0, cy + 1.5);
            context.line_to(cx - 7.0, cy + h);
            context.line_to(cx - 1.5, cy + h);

            context.move_to(cx + 1.5, cy + h);
            context.line_to(cx + 7.0, cy + h);
            context.line_to(cx + 7.0, cy + 1.5);
            let _ = context.stroke();
        }
        ToolbarIcon::Fullscreen => {
            rounded_rect_path(context, cx - 7.0, cy - 6.0, 14.0, 10.5, 2.0);
            let _ = context.stroke();
            context.move_to(cx, cy + 4.5);
            context.line_to(cx, cy + 7.5);
            context.move_to(cx - 4.5, cy + 7.5);
            context.line_to(cx + 4.5, cy + 7.5);
            let _ = context.stroke();
        }
        ToolbarIcon::Window => {
            rounded_rect_path(context, cx - 7.0, cy - 5.5, 14.0, 9.5, 1.7);
            let _ = context.stroke();
            context.move_to(cx - 7.0, cy - 2.0);
            context.line_to(cx + 7.0, cy - 2.0);
            let _ = context.stroke();
        }
        ToolbarIcon::Scroll => {
            context.new_path();
            context.move_to(cx, cy - 4.8);
            context.line_to(cx, cy + 1.8);
            context.move_to(cx - 3.2, cy - 1.0);
            context.line_to(cx, cy + 1.9);
            context.line_to(cx + 3.2, cy - 1.0);
            let _ = context.stroke();
        }
        ToolbarIcon::Timer => {
            context.new_path();
            context.arc(cx, cy, 6.0, 0.0, PI * 2.0);
            let _ = context.stroke();
            context.new_path();
            context.move_to(cx, cy);
            context.line_to(cx, cy - 2.8);
            context.move_to(cx, cy);
            context.line_to(cx + 2.2, cy + 1.7);
            let _ = context.stroke();
        }
        ToolbarIcon::Ocr => {
            context.select_font_face(
                "Sans",
                gtk4::cairo::FontSlant::Normal,
                gtk4::cairo::FontWeight::Bold,
            );
            context.set_font_size(8.0);
            if let Ok(extents) = context.text_extents("Aa") {
                let text_x = cx - extents.width() / 2.0 - extents.x_bearing();
                let text_y = cy - (extents.y_bearing() + extents.height() / 2.0) + 0.2;
                context.move_to(text_x, text_y);
                let _ = context.show_text("Aa");
            }
        }
        ToolbarIcon::Recording => {
            rounded_rect_path(context, cx - 8.0, cy - 5.0, 10.5, 10.0, 2.5);
            let _ = context.stroke();
            context.move_to(cx + 2.4, cy - 2.8);
            context.line_to(cx + 7.4, cy - 5.2);
            context.line_to(cx + 7.4, cy + 5.2);
            context.line_to(cx + 2.4, cy + 2.8);
            context.close_path();
            let _ = context.stroke();
        }
        ToolbarIcon::Controls => {
            for i in 0..3 {
                let x = cx - 4.5 + i as f64 * 4.5;
                context.move_to(x, cy - 6.0);
                context.line_to(x, cy + 6.0);
                let slider_y = if i == 0 {
                    cy - 2.0
                } else if i == 1 {
                    cy + 2.0
                } else {
                    cy - 1.0
                };
                context.arc(x, slider_y, 1.8, 0.0, PI * 2.0);
            }
            let _ = context.stroke();
        }
        ToolbarIcon::Crop => {
            context.set_line_cap(gtk4::cairo::LineCap::Butt);
            context.set_line_join(gtk4::cairo::LineJoin::Miter);
            let s = 10.5;
            let t = 2.8;
            let o = 1.2;
            context.move_to(cx - s / 2.0 - t, cy - s / 2.0 + o);
            context.line_to(cx + s / 2.0 - o, cy - s / 2.0 + o);
            context.move_to(cx - s / 2.0 + o, cy - s / 2.0 - t);
            context.line_to(cx - s / 2.0 + o, cy + s / 2.0 - o);
            context.move_to(cx + s / 2.0 + t, cy + s / 2.0 - o);
            context.line_to(cx - s / 2.0 + o, cy + s / 2.0 - o);
            context.move_to(cx + s / 2.0 - o, cy + s / 2.0 + t);
            context.line_to(cx + s / 2.0 - o, cy - s / 2.0 + o);
            let _ = context.stroke();
        }
        ToolbarIcon::Mic => {
            rounded_rect_path(context, cx - 3.1, cy - 7.0, 6.2, 9.6, 3.1);
            let _ = context.stroke();
            context.move_to(cx - 5.0, cy - 0.3);
            context.line_to(cx - 5.0, cy + 1.6);
            context.move_to(cx + 5.0, cy - 0.3);
            context.line_to(cx + 5.0, cy + 1.6);
            context.arc(cx, cy + 0.7, 5.0, 0.0, PI);
            context.move_to(cx, cy + 6.1);
            context.line_to(cx, cy + 8.3);
            context.move_to(cx - 3.4, cy + 8.3);
            context.line_to(cx + 3.4, cy + 8.3);
            let _ = context.stroke();
        }
        ToolbarIcon::Speaker => {
            context.move_to(cx - 6.8, cy - 2.3);
            context.line_to(cx - 4.4, cy - 2.3);
            context.line_to(cx - 1.2, cy - 5.1);
            context.line_to(cx - 1.2, cy + 5.1);
            context.line_to(cx - 4.4, cy + 2.3);
            context.line_to(cx - 6.8, cy + 2.3);
            context.close_path();
            let _ = context.stroke();
            context.arc(cx - 0.8, cy, 5.0, -0.7, 0.7);
            context.arc(cx + 1.2, cy, 7.0, -0.7, 0.7);
            let _ = context.stroke();
        }
        ToolbarIcon::Webcam => {
            rounded_rect_path(context, cx - 7.2, cy - 4.6, 14.4, 9.8, 2.2);
            let _ = context.stroke();
            context.arc(cx, cy + 0.3, 3.0, 0.0, PI * 2.0);
            let _ = context.stroke();
            context.move_to(cx - 4.6, cy - 4.6);
            context.line_to(cx - 2.2, cy - 6.8);
            context.line_to(cx + 1.8, cy - 6.8);
            context.line_to(cx + 3.8, cy - 4.6);
            let _ = context.stroke();
        }
        ToolbarIcon::Clicks => {
            context.move_to(cx - 0.5, cy - 6.5);
            context.line_to(cx - 0.5, cy + 5.0);
            context.line_to(cx + 2.5, cy + 1.5);
            context.line_to(cx + 7.0, cy + 2.0);
            context.close_path();
            let _ = context.stroke();
            context.move_to(cx + 2.5, cy + 1.5);
            context.line_to(cx + 5.5, cy + 6.0);
            let _ = context.stroke();
            context.set_line_width(1.2);
            let tx = cx - 0.5;
            let ty = cy - 6.5;
            for i in 0..6 {
                let ang = i as f64 * PI / 3.0;
                context.move_to(tx + ang.cos() * 3.5, ty + ang.sin() * 3.5);
                context.line_to(tx + ang.cos() * 6.0, ty + ang.sin() * 6.0);
            }
            let _ = context.stroke();
        }
        ToolbarIcon::Keystrokes => {
            rounded_rect_path(context, cx - 8.5, cy - 8.5, 17.0, 17.0, 3.5);
            let _ = context.stroke();
            context.set_line_width(1.8);
            let r = 2.4;
            context.arc(cx - r, cy - r, r, 0.0, PI * 2.0);
            context.arc(cx + r, cy - r, r, 0.0, PI * 2.0);
            context.arc(cx - r, cy + r, r, 0.0, PI * 2.0);
            context.arc(cx + r, cy + r, r, 0.0, PI * 2.0);
            let _ = context.stroke();
            context.move_to(cx - r, cy - r + 0.5);
            context.line_to(cx - r, cy + r - 0.5);
            context.move_to(cx + r, cy - r + 0.5);
            context.line_to(cx + r, cy + r - 0.5);
            context.move_to(cx - r + 0.5, cy - r);
            context.line_to(cx + r - 0.5, cy - r);
            context.move_to(cx - r + 0.5, cy + r);
            context.line_to(cx + r - 0.5, cy + r);
            let _ = context.stroke();
        }
        ToolbarIcon::Video => {
            rounded_rect_path(context, cx - 8.0, cy - 5.0, 10.5, 10.0, 2.5);
            let _ = context.stroke();
            context.move_to(cx + 2.4, cy - 2.8);
            context.line_to(cx + 7.4, cy - 5.2);
            context.line_to(cx + 7.4, cy + 5.2);
            context.line_to(cx + 2.4, cy + 2.8);
            context.close_path();
            let _ = context.stroke();
        }
        ToolbarIcon::Gif => {
            rounded_rect_path(context, cx - 9.0, cy - 6.0, 18.0, 12.0, 3.0);
            context.set_source_rgba(color.0, color.1, color.2, color.3);
            let _ = context.fill();
            context.select_font_face(
                "Sans",
                gtk4::cairo::FontSlant::Normal,
                gtk4::cairo::FontWeight::Bold,
            );
            context.set_font_size(6.5);
            context.set_source_rgba(0.0, 0.0, 0.0, 180.0 / 255.0);
            if let Ok(extents) = context.text_extents("GIF") {
                let text_x = cx - extents.width() / 2.0 - extents.x_bearing();
                let text_y = cy - extents.height() / 2.0 - extents.y_bearing() + 0.5;
                context.move_to(text_x, text_y);
                let _ = context.show_text("GIF");
            }
        }
    }

    let _ = context.restore();
}

#[derive(Clone)]
struct BackgroundFrame {
    /// Full-resolution original screenshot surface.
    surface: gtk4::cairo::ImageSurface,
    /// Downsampled + blurred surface used for the toolbar frosted-glass effect.
    /// Built at 1/4 resolution so the blur is fast but visually strong.
    toolbar_blur_surface: gtk4::cairo::ImageSurface,
    width: i32,
    height: i32,
}

fn rgba_to_cairo_argb_bytes(image: &RgbaImage, stride: usize) -> Vec<u8> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let raw = image.as_raw();
    let row_src_len = width * 4;

    // Allocate output buffer; rows may be wider than src due to Cairo stride padding.
    let mut out = vec![0u8; stride * height];

    // Split output into per-row chunks and process in parallel with rayon.
    // Each row is independent so there are no data races.
    out.par_chunks_mut(stride)
        .enumerate()
        .for_each(|(y, dst_row)| {
            let src_start = y * row_src_len;
            let src_row = &raw[src_start..src_start + row_src_len];

            // Fast path: screenshots are always fully opaque (a == 255).
            // Avoid the per-pixel branch and just swap R↔B in-place.
            let all_opaque = src_row.chunks_exact(4).all(|p| p[3] == 255);

            if all_opaque {
                for (src, dst) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
                    // RGBA → Cairo ARGB (BGRA in memory, little-endian)
                    dst[0] = src[2]; // B
                    dst[1] = src[1]; // G
                    dst[2] = src[0]; // R
                    dst[3] = 255; // A
                }
            } else {
                // General path: handle transparent / semi-transparent pixels.
                for (src, dst) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
                    let a = src[3];
                    if a == 0 {
                        dst[0] = 0;
                        dst[1] = 0;
                        dst[2] = 0;
                        dst[3] = 0;
                    } else {
                        let alpha = a as u16;
                        let premul = |c: u8| -> u8 { ((c as u16 * alpha + 127) / 255) as u8 };
                        dst[0] = premul(src[2]); // B
                        dst[1] = premul(src[1]); // G
                        dst[2] = premul(src[0]); // R
                        dst[3] = a;
                    }
                }
            }
        });

    out
}

fn background_frame_from_image(image: &RgbaImage) -> Result<BackgroundFrame, SelectionError> {
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return Err(SelectionError::InitError(
            "Cannot select from an empty screenshot".into(),
        ));
    }

    // Pre-compute Cairo strides (cheap, no allocation).
    let stride = gtk4::cairo::Format::ARgb32
        .stride_for_width(width)
        .map_err(|e| SelectionError::InitError(e.to_string()))? as usize;

    // Toolbar blur: 1/4-resolution downsample + Gaussian blur.
    // Nearest filter is ~5x faster than Triangle; quality is invisible after
    // the blur pass and when scaled back up to screen size.
    let small_w = (width / 4).max(1);
    let small_h = (height / 4).max(1);
    let blur_stride = gtk4::cairo::Format::ARgb32
        .stride_for_width(small_w)
        .map_err(|e| SelectionError::InitError(e.to_string()))? as usize;

    // Build both pixel buffers in parallel: the full-res ARGB conversion and
    // the downsample+blur are independent, so we run them on separate threads.
    let (full_data, blur_data) = rayon::join(
        || rgba_to_cairo_argb_bytes(image, stride),
        || {
            let small = image::imageops::resize(
                image,
                small_w,
                small_h,
                image::imageops::FilterType::Nearest, // fast; quality invisible after blur
            );
            let blurred = image::imageops::blur(&small, 8.0);
            rgba_to_cairo_argb_bytes(&blurred, blur_stride)
        },
    );

    // Wrap both buffers in Cairo ImageSurfaces (cheap — just takes ownership).
    let surface = gtk4::cairo::ImageSurface::create_for_data(
        full_data,
        gtk4::cairo::Format::ARgb32,
        width as i32,
        height as i32,
        stride as i32,
    )
    .map_err(|e| SelectionError::InitError(e.to_string()))?;

    let toolbar_blur_surface = gtk4::cairo::ImageSurface::create_for_data(
        blur_data,
        gtk4::cairo::Format::ARgb32,
        small_w as i32,
        small_h as i32,
        blur_stride as i32,
    )
    .map_err(|e| SelectionError::InitError(e.to_string()))?;

    Ok(BackgroundFrame {
        surface,
        toolbar_blur_surface,
        width: width as i32,
        height: height as i32,
    })
}

fn map_selection_to_image(
    area: SelectionArea,
    image_width: i32,
    image_height: i32,
    view_width: i32,
    view_height: i32,
) -> SelectionArea {
    if image_width <= 0 || image_height <= 0 || view_width <= 0 || view_height <= 0 {
        return area;
    }

    let scale_x = image_width as f64 / view_width as f64;
    let scale_y = image_height as f64 / view_height as f64;

    let x0 = (area.x as f64 * scale_x).floor() as i32;
    let y0 = (area.y as f64 * scale_y).floor() as i32;
    let x1 = ((area.x + area.width) as f64 * scale_x).ceil() as i32;
    let y1 = ((area.y + area.height) as f64 * scale_y).ceil() as i32;

    let clamped_x0 = x0.clamp(0, image_width.saturating_sub(1));
    let clamped_y0 = y0.clamp(0, image_height.saturating_sub(1));
    let clamped_x1 = x1.clamp(clamped_x0 + 1, image_width);
    let clamped_y1 = y1.clamp(clamped_y0 + 1, image_height);

    SelectionArea {
        x: clamped_x0,
        y: clamped_y0,
        width: clamped_x1 - clamped_x0,
        height: clamped_y1 - clamped_y0,
    }
}

/// GTK4 overlay window for interactive area selection
pub struct AreaSelector {
    state: Arc<Mutex<SelectorState>>,
}

impl AreaSelector {
    /// Create a new area selector
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(SelectorState::default())),
        }
    }

    /// Run the area selection dialog
    ///
    /// Returns `Ok(Some(area))` if user selected an area
    /// Returns `Ok(None)` if user cancelled (ESC)
    /// Returns `Err` if initialization failed
    pub fn run(&self) -> SelectionResult {
        self.run_with_background(None)
    }

    fn run_with_background(&self, background: Option<BackgroundFrame>) -> SelectionResult {
        let state = self.state.clone();
        let (result_tx, result_rx) = std::sync::mpsc::channel();

        // Create application.
        // NON_UNIQUE: skip the single-instance check so the overlay can be
        // launched multiple times without GApplication refusing to activate.
        let app = Application::builder()
            .application_id(crate::app_identity::app_id())
            .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
            .build();

        // NOTE: We intentionally do NOT clear DESKTOP_STARTUP_ID here.
        // On GNOME Wayland, clearing it strips the XDG activation token that
        // allows the compositor to grant keyboard focus and raise the window.
        // Without it, window.present() is silently ignored by GNOME Shell.

        // Clone state for the activate handler
        let state_activate = state.clone();
        let background_activate = background.clone();
        app.connect_activate(move |application| {
            setup_window(
                application,
                state_activate.clone(),
                result_tx.clone(),
                background_activate.clone(),
            );
        });

        // Run the application
        let _ = app.run_with_args::<String>(&[]);

        // Get the result
        match result_rx.recv() {
            Ok(Ok(area)) => Ok(area),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(SelectionError::InitError("No result received".into())),
        }
    }
}

/// Setup the overlay window (standalone function to avoid lifetime issues)
/// Install CSS that disables GTK-side animations on the overlay window so it
/// appears and disappears instantly (no fade / scale shutter effect).
fn install_overlay_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(
            "
            window.overlay {
                background-color: transparent;
                transition: none;
                transition-duration: 0s;
                animation: none;
                animation-duration: 0s;
            }

            window.overlay > * {
                background-color: transparent;
            }

            drawingarea {
                background-color: transparent;
            }
            ",
        );
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
}

/// On X11, tell the compositor to treat this window as a transient system
/// overlay (no open/close animation, no taskbar entry, no pager entry).
///
/// This is called from `connect_realize` — i.e. the XID exists but the
/// window has not been mapped yet — so the compositor sees all hints on
/// the very first MapNotify and never starts an animation.
fn suppress_x11_compositor_animation(window: &ApplicationWindow) {
    use gdk4x11::X11Surface;
    use x11rb::{
        connection::Connection,
        protocol::xproto::{self, ConnectionExt as _},
    };

    let Some(surface) = window.surface() else {
        return;
    };
    let Ok(x11_surface) = surface.downcast::<X11Surface>() else {
        return; // Wayland – nothing to do
    };
    let Ok(xid) = u32::try_from(x11_surface.xid()) else {
        return;
    };
    let Ok((conn, _)) = x11rb::connect(None) else {
        return;
    };

    // _NET_WM_BYPASS_COMPOSITOR = 1
    // Asks the compositor to skip compositing this window entirely, which
    // also disables any open/close transition effects.
    if let Ok(cookie) = conn.intern_atom(false, b"_NET_WM_BYPASS_COMPOSITOR") {
        if let Ok(reply) = cookie.reply() {
            let _ = conn.change_property32(
                xproto::PropMode::REPLACE,
                xid,
                reply.atom,
                xproto::AtomEnum::CARDINAL,
                &[1u32],
            );
        }
    }

    // _NET_WM_WINDOW_TYPE = _NET_WM_WINDOW_TYPE_UTILITY
    // UTILITY windows are never animated by compositors (Mutter, KWin, Picom).
    // We prefer UTILITY over SPLASH because SPLASH can cause focus/stacking
    // issues on some window managers.
    if let (Ok(type_cookie), Ok(util_cookie)) = (
        conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE"),
        conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_UTILITY"),
    ) {
        if let (Ok(type_reply), Ok(util_reply)) = (type_cookie.reply(), util_cookie.reply()) {
            let _ = conn.change_property32(
                xproto::PropMode::REPLACE,
                xid,
                type_reply.atom,
                xproto::AtomEnum::ATOM,
                &[util_reply.atom],
            );
        }
    }

    // _NET_WM_STATE: add SKIP_TASKBAR + SKIP_PAGER so the overlay never
    // appears in the taskbar or workspace switcher.
    if let (Ok(state_cookie), Ok(skip_taskbar_cookie), Ok(skip_pager_cookie)) = (
        conn.intern_atom(false, b"_NET_WM_STATE"),
        conn.intern_atom(false, b"_NET_WM_STATE_SKIP_TASKBAR"),
        conn.intern_atom(false, b"_NET_WM_STATE_SKIP_PAGER"),
    ) {
        if let (Ok(state_reply), Ok(skip_taskbar_reply), Ok(skip_pager_reply)) = (
            state_cookie.reply(),
            skip_taskbar_cookie.reply(),
            skip_pager_cookie.reply(),
        ) {
            let _ = conn.change_property32(
                xproto::PropMode::REPLACE,
                xid,
                state_reply.atom,
                xproto::AtomEnum::ATOM,
                &[skip_taskbar_reply.atom, skip_pager_reply.atom],
            );
        }
    }

    let _ = conn.flush();
}

fn setup_window(
    app: &Application,
    state: Arc<Mutex<SelectorState>>,
    result_tx: std::sync::mpsc::Sender<SelectionResult>,
    background: Option<BackgroundFrame>,
) {
    // Suppress GTK-side animations so the overlay appears/disappears instantly.
    install_overlay_css();

    // Get the display and monitor for screen dimensions
    let display = match gdk::Display::default() {
        Some(d) => d,
        None => {
            let _ = result_tx.send(Err(SelectionError::InitError("No display found".into())));
            return;
        }
    };

    // Get screen dimensions from the first monitor
    let monitor = {
        let monitors = display.monitors();
        let n = monitors.n_items();
        if n == 0 {
            let _ = result_tx.send(Err(SelectionError::InitError("No monitor found".into())));
            return;
        }
        // Get the first monitor from the list model
        match monitors.item(0) {
            Some(obj) => match obj.downcast::<gdk::Monitor>() {
                Ok(m) => m,
                Err(_) => {
                    let _ = result_tx.send(Err(SelectionError::InitError(
                        "Failed to get monitor".into(),
                    )));
                    return;
                }
            },
            None => {
                let _ = result_tx.send(Err(SelectionError::InitError(
                    "No monitor at index 0".into(),
                )));
                return;
            }
        }
    };

    let geometry = monitor.geometry();
    let screen_width = geometry.width();
    let screen_height = geometry.height();

    // Create the window
    let window = ApplicationWindow::builder()
        .application(app)
        .default_width(screen_width)
        .default_height(screen_height)
        .decorated(false)
        .resizable(false)
        .css_classes(["overlay", "transparent"])
        .build();

    let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    // On Wayland, layer-shell gives a true transparent overlay surface.
    // Without this, some compositors show a black backing surface.
    let wayland_layer_shell = is_wayland && gtk4_layer_shell::is_supported();

    // NOTE: We no longer bail out when background.is_none() && Wayland-without-layer-shell.
    // Instead we fall through to window.set_fullscreened(true) which works on GNOME Wayland.
    // The drawing code already handles background=None by painting a dark semi-transparent
    // overlay — this is the "capture after selection" (Option B) path.

    if wayland_layer_shell {
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Bottom, true);
        window.set_anchor(Edge::Left, true);
        window.set_anchor(Edge::Right, true);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        window.set_monitor(Some(&monitor));
        window.set_namespace(Some("apexshot-area-selector"));
    } else {
        // X11 or Wayland-without-layer-shell (e.g. GNOME Wayland):
        // Use a regular fullscreen window. The compositor will grant it
        // focus via the XDG activation token embedded in DESKTOP_STARTUP_ID.
        window.set_fullscreened(true);
        window.set_decorated(false);
    }

    // Get the surface for cursor control
    let surface = window.surface();

    // Set cursor to crosshair when hovering over the window
    if let Some(ref surface) = surface {
        let cursor = gdk::Cursor::from_name("crosshair", None);
        surface.set_cursor(cursor.as_ref());
    }

    // Create a drawing area for rendering the selection
    let drawing_area = gtk4::DrawingArea::builder()
        .hexpand(true)
        .vexpand(true)
        .build();

    let state_draw = state.clone();
    let background_draw = background.clone();
    drawing_area.set_draw_func(move |_, context, width, height| {
        draw_overlay(
            context,
            width,
            height,
            &state_draw,
            background_draw.as_ref(),
        );
    });

    {
        let mut st = state.lock().unwrap();
        let screen_width_f = screen_width.max(1) as f64;
        let screen_height_f = screen_height.max(1) as f64;
        let initial_width = DEFAULT_SELECTION_WIDTH
            .min(screen_width_f)
            .max(MIN_SELECTION_WIDTH.min(screen_width_f));
        let initial_height = DEFAULT_SELECTION_HEIGHT
            .min(screen_height_f)
            .max(MIN_SELECTION_HEIGHT.min(screen_height_f));
        let initial_left = ((screen_width_f - initial_width) / 2.0).max(0.0);
        let initial_top = ((screen_height_f - initial_height) / 2.0).max(0.0);

        st.start_x = initial_left;
        st.start_y = initial_top;
        st.current_x = initial_left + initial_width;
        st.current_y = initial_top + initial_height;
        st.completed = true;
        st.cancelled = false;
        st.is_dragging = false;
    }

    // Set the drawing area as the child
    window.set_child(Some(&drawing_area));

    let motion_controller = EventControllerMotion::new();
    let state_motion = state.clone();
    let drawing_area_weak_motion = drawing_area.downgrade();
    let window_weak_motion = window.downgrade();
    motion_controller.connect_motion(move |_, x, y| {
        let (cursor_name, hover_changed, _done) = {
            let mut st = state_motion.lock().unwrap();
            let rect = current_selection_rect(&st);

            // GIF slider dragging — update value from X position
            if let Some(slider) = st.gif_slider_dragging {
                if st.settings_menu_open {
                    let menu_x = (rect.left + (rect.width() - 440.0) / 2.0).clamp(10.0, screen_width as f64 - 450.0);
                    let value_x = menu_x + 130.0;
                    if slider == 0 {
                        let slider_x = value_x + 55.0;
                        let slider_w = 220.0;
                        let click_x = x.clamp(slider_x, slider_x + slider_w);
                        st.gif_fps = 5.0 + (click_x - slider_x) / slider_w * 55.0;
                    } else {
                        let q_slider_w = 160.0;
                        let click_x = x.clamp(value_x, value_x + q_slider_w);
                        st.gif_quality = 0.1 + (click_x - value_x) / q_slider_w * 0.8;
                    }
                }
                st.hovered_settings_item = -1;
                st.hovered_capture_crop_menu_item = -1;
                st.hovered_crop_menu_item = -1;
                st.hover_tool_index = None;
                st.hover_size_panel = false;
                st.hover_crop_panel = false;
                st.hover_record_tile = None;
                drop(st);
                if let Some(da) = drawing_area_weak_motion.upgrade() { da.queue_draw(); }
                return;
            }

            // Capture crop menu hover check
            if st.capture_crop_menu_open {
                let item = capture_crop_menu_hit_item(
                    rect.left, rect.top, rect.width(), rect.height(),
                    screen_width as f64, screen_height as f64, x, y,
                );
                let next = item.map(|i| i as i32).unwrap_or(-1);
                let changed = next != st.hovered_capture_crop_menu_item;
                if changed { st.hovered_capture_crop_menu_item = next; }
                st.hovered_crop_menu_item = -1;
                st.hovered_settings_item = -1;
                // Clear other hovers
                st.hover_tool_index = None;
                st.hover_size_panel = false;
                st.hover_crop_panel = false;
                st.hover_record_tile = None;
                ("pointer".to_string(), changed, true)
            } else if st.crop_menu_open {
                let item = recording_crop_menu_hit_item(
                    rect.left, rect.top, rect.width(), rect.height(),
                    screen_width as f64, screen_height as f64, x, y,
                );
                let next = item.map(|i| i as i32).unwrap_or(-1);
                let changed = next != st.hovered_crop_menu_item;
                if changed { st.hovered_crop_menu_item = next; }
                st.hovered_capture_crop_menu_item = -1;
                st.hovered_settings_item = -1;
                st.hover_tool_index = None;
                st.hover_size_panel = false;
                st.hover_crop_panel = false;
                st.hover_record_tile = None;
                ("pointer".to_string(), changed, true)
            } else if st.settings_menu_open {
                if st.settings_dropdown_open.is_some() {
                    st.hovered_settings_item = -1;
                    st.hovered_capture_crop_menu_item = -1;
                    st.hovered_crop_menu_item = -1;
                    st.hover_tool_index = None;
                    st.hover_size_panel = false;
                    st.hover_crop_panel = false;
                    st.hover_record_tile = None;
                    ("pointer".to_string(), false, true)
                } else {
                let item = settings_menu_hit_item(
                    rect.left, rect.top, rect.width(), rect.height(),
                    screen_width as f64, screen_height as f64, x, y,
                    st.settings_tab,
                );
                let next = item.unwrap_or(-1);
                let changed = next != st.hovered_settings_item;
                if changed { st.hovered_settings_item = next; }
                st.hovered_capture_crop_menu_item = -1;
                st.hovered_crop_menu_item = -1;
                st.hover_tool_index = None;
                st.hover_size_panel = false;
                st.hover_crop_panel = false;
                st.hover_record_tile = None;
                ("pointer".to_string(), changed, true)
                }
            } else {
                let record_hit = if st.recording_panel_open {
                    recording_tile_at(
                        rect.left, rect.top, rect.width(), rect.height(),
                        screen_width as f64, screen_height as f64, x, y,
                    )
                } else {
                    None
                };
                let hit = if st.recording_panel_open {
                    None
                } else {
                    toolbar_hit_at(
                        rect.left, rect.top, rect.width(), rect.height(),
                        screen_width as f64, screen_height as f64, x, y,
                    )
                };

                let (
                    next_hover_tool_index,
                    next_hover_size_panel,
                    next_hover_crop_panel,
                    next_hover_record_tile,
                    cursor_name,
                ) = match hit {
                    Some(ToolbarHit::Tool(index)) if !st.recording_panel_open => {
                        (Some(index), false, false, None, "pointer")
                    }
                    Some(ToolbarHit::SizePanel) if !st.recording_panel_open => {
                        (None, true, false, None, "default")
                    }
                    Some(ToolbarHit::CropPanel) if !st.recording_panel_open => {
                        (None, false, true, None, "pointer")
                    }
                    None => {
                        if let Some(tile) = record_hit {
                            (None, false, false, Some(tile), "pointer")
                        } else {
                            let c = if st.completed || st.is_dragging {
                                detect_resize_handle(x, y, rect)
                                    .map(cursor_name_for_handle)
                                    .unwrap_or_else(|| {
                                        if is_inside_selection(x, y, rect) { "fleur" } else { "crosshair" }
                                    })
                            } else {
                                "crosshair"
                            };
                            (None, false, false, None, c)
                        }
                    }
                    _ => (None, false, false, None, "crosshair"),
                };

                let hover_changed = st.hover_tool_index != next_hover_tool_index
                    || st.hover_size_panel != next_hover_size_panel
                    || st.hover_crop_panel != next_hover_crop_panel
                    || st.hover_record_tile != next_hover_record_tile;

                st.hover_tool_index = next_hover_tool_index;
                st.hover_size_panel = next_hover_size_panel;
                st.hover_crop_panel = next_hover_crop_panel;
                st.hover_record_tile = next_hover_record_tile;
                st.hovered_capture_crop_menu_item = -1;
                st.hovered_crop_menu_item = -1;
                st.hovered_settings_item = -1;

                (cursor_name.to_string(), hover_changed, false)
            }
        };

        if let Some(win) = window_weak_motion.upgrade() {
            if let Some(surf) = win.surface() {
                let cursor = gdk::Cursor::from_name(&cursor_name, None);
                surf.set_cursor(cursor.as_ref());
            }
        }
        if hover_changed {
            if let Some(drawing_area) = drawing_area_weak_motion.upgrade() {
                drawing_area.queue_draw();
            }
        }
    });

    let state_motion_leave = state.clone();
    let drawing_area_weak_leave = drawing_area.downgrade();
    let window_weak_leave = window.downgrade();
    motion_controller.connect_leave(move |_| {
        let mut st = state_motion_leave.lock().unwrap();
        let was_hovering = st.hover_tool_index.is_some()
            || st.hover_size_panel
            || st.hover_crop_panel
            || st.hover_record_tile.is_some()
            || st.hovered_capture_crop_menu_item != -1
            || st.hovered_crop_menu_item != -1
            || st.hovered_settings_item != -1;
        st.hover_tool_index = None;
        st.hover_size_panel = false;
        st.hover_crop_panel = false;
        st.hover_record_tile = None;
        st.hovered_capture_crop_menu_item = -1;
        st.hovered_crop_menu_item = -1;
        st.hovered_settings_item = -1;
        drop(st);

        // Reset cursor
        if let Some(win) = window_weak_leave.upgrade() {
            if let Some(surf) = win.surface() {
                let cursor = gdk::Cursor::from_name("crosshair", None);
                surf.set_cursor(cursor.as_ref());
            }
        }
        if was_hovering {
            if let Some(drawing_area) = drawing_area_weak_leave.upgrade() {
                drawing_area.queue_draw();
            }
        }
    });

    drawing_area.add_controller(motion_controller);

    // Toolbar click actions
    let click_gesture = GestureClick::builder()
        .button(1)
        .propagation_phase(gtk4::PropagationPhase::Capture)
        .build();

    let state_click = state.clone();
    let drawing_area_weak_click = drawing_area.downgrade();
    let result_tx_click = result_tx.clone();
    let window_weak_click = window.downgrade();
    let background_click = background.clone();
    click_gesture.connect_pressed(move |_, n_press, x, y| {
        let mut st = state_click.lock().unwrap();
        let rect = current_selection_rect(&st);
        let recording_panel_open = st.recording_panel_open;

        // ── Menu click handling ──

        // Capture crop menu (non-recording mode)
        if st.capture_crop_menu_open {
            if let Some(item) = capture_crop_menu_hit_item(
                rect.left, rect.top, rect.width(), rect.height(),
                screen_width as f64, screen_height as f64, x, y,
            ) {
                st.capture_aspect_ratio_index = item;
                st.capture_crop_menu_open = false;
                st.hovered_capture_crop_menu_item = -1;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() { da.queue_draw(); }
                return;
            }
            st.capture_crop_menu_open = false;
            st.hovered_capture_crop_menu_item = -1;
            drop(st);
            if let Some(da) = drawing_area_weak_click.upgrade() { da.queue_draw(); }
            return;
        }

        // Recording crop menu
        if st.crop_menu_open {
            if let Some(item) = recording_crop_menu_hit_item(
                rect.left, rect.top, rect.width(), rect.height(),
                screen_width as f64, screen_height as f64, x, y,
            ) {
                st.record_aspect_ratio_index = item;
                st.crop_menu_open = false;
                st.hovered_crop_menu_item = -1;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() { da.queue_draw(); }
                return;
            }
            st.crop_menu_open = false;
            st.hovered_crop_menu_item = -1;
            drop(st);
            if let Some(da) = drawing_area_weak_click.upgrade() { da.queue_draw(); }
            return;
        }

        // Settings menu
        if st.settings_menu_open {
            // If a dropdown is open, check dropdown item clicks first
            if let Some(drop_idx) = st.settings_dropdown_open {
                let tab = match st.settings_tab {
                    SettingsTab::Video => 1,
                    SettingsTab::Gif => 2,
                    _ => 0,
                };
                let (options, value_ptr): (&[&str], &mut usize) = if tab == 1 && drop_idx == 3 {
                    (&["Original", "1080p", "720p"], &mut st.video_max_res)
                } else if tab == 1 && drop_idx == 4 {
                    (&["24", "30", "50", "60"], &mut st.video_fps)
                } else if tab == 2 && drop_idx == 6 {
                    (&["800 x auto", "640 x auto", "480 x auto", "Original"], &mut st.gif_size_idx)
                } else {
                    (&[], &mut 0)
                };
                // Compute dropdown popup rect
                let menu_x = (rect.left + (rect.width() - 440.0) / 2.0).clamp(10.0, screen_width as f64 - 450.0);
                let menu_y = (rect.top + 24.0).clamp(10.0, screen_height as f64 - 570.0);
                let popup_y = compute_dropdown_popup_y(menu_y, drop_idx, match tab { 1 => SettingsTab::Video, 2 => SettingsTab::Gif, _ => SettingsTab::General });
                let popup_rect = RectF { x: menu_x + 130.0, y: popup_y, width: 140.0, height: options.len() as f64 * 30.0 };
                // Check if clicked outside popup
                if !popup_rect.contains(x, y) {
                    st.settings_dropdown_open = None;
                    drop(st);
                    if let Some(da) = drawing_area_weak_click.upgrade() { da.queue_draw(); }
                    return;
                }
                // Check item clicks
                for (oi, _opt) in options.iter().enumerate() {
                    let item_rect = RectF { x: popup_rect.x, y: popup_rect.y + oi as f64 * 30.0, width: popup_rect.width, height: 30.0 };
                    if item_rect.contains(x, y) {
                        *value_ptr = oi;
                        st.settings_dropdown_open = None;
                        st.hovered_settings_item = -1;
                        drop(st);
                        if let Some(da) = drawing_area_weak_click.upgrade() { da.queue_draw(); }
                        return;
                    }
                }
                st.settings_dropdown_open = None;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() { da.queue_draw(); }
                return;
            }

            if let Some(item) = settings_menu_hit_item(
                rect.left, rect.top, rect.width(), rect.height(),
                screen_width as f64, screen_height as f64, x, y,
                st.settings_tab,
            ) {
                // Tab clicks
                if item < 3 {
                    st.settings_tab = match item {
                        0 => SettingsTab::General,
                        1 => SettingsTab::Video,
                        _ => SettingsTab::Gif,
                    };
                    st.hovered_settings_item = -1;
                    st.settings_dropdown_open = None;
                } else if matches!(st.settings_tab, SettingsTab::General) {
                    let general_idx = item - 3;
                    match general_idx {
                        0 => st.rec_controls = !st.rec_controls,
                        1 => st.display_rec_time = !st.display_rec_time,
                        2 => st.hidpi = !st.hidpi,
                        3 => st.do_not_disturb = !st.do_not_disturb,
                        4 => st.show_cursor = !st.show_cursor,
                        5 => st.rec_clicks = !st.rec_clicks,
                        6 => st.rec_keystrokes = !st.rec_keystrokes,
                        7 => st.remember_selection = !st.remember_selection,
                        8 => st.dim_screen = !st.dim_screen,
                        9 => st.show_countdown = !st.show_countdown,
                        _ => {}
                    }
                    st.settings_dropdown_open = None;
                } else if matches!(st.settings_tab, SettingsTab::Video) {
                    let video_idx = item - 3;
                    match video_idx {
                        0 => st.settings_dropdown_open = Some(3), // res dropdown
                        1 => st.settings_dropdown_open = Some(4), // fps dropdown
                        2 => st.record_mono = !st.record_mono,
                        3 => st.open_editor = !st.open_editor,
                        _ => {}
                    }
                } else if matches!(st.settings_tab, SettingsTab::Gif) {
                    let gif_idx = item - 3;
                    let menu_x = (rect.left + (rect.width() - 440.0) / 2.0).clamp(10.0, screen_width as f64 - 450.0);
                    let value_x = menu_x + 130.0;
                    match gif_idx {
                        0 => { // FPS slider — click-to-position + start drag
                             let slider_x = value_x + 55.0;
                             let slider_w = 220.0;
                             let click_x = x.clamp(slider_x, slider_x + slider_w);
                             st.gif_fps = 5.0 + (click_x - slider_x) / slider_w * 55.0;
                             st.gif_slider_dragging = Some(0);
                         }
                        1 => { // Quality slider — click-to-position + start drag
                             let q_slider_w = 160.0;
                             let click_x = x.clamp(value_x, value_x + q_slider_w);
                             st.gif_quality = 0.1 + (click_x - value_x) / q_slider_w * 0.8;
                             st.gif_slider_dragging = Some(1);
                         }
                        2 => st.optimize_gif = !st.optimize_gif,
                        3 => st.settings_dropdown_open = Some(6), // size dropdown
                        _ => {}
                    }
                }
                st.hovered_settings_item = -1;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() { da.queue_draw(); }
                return;
            }
            // Click outside settings menu closes it
            st.settings_menu_open = false;
            st.hovered_settings_item = -1;
            st.settings_dropdown_open = None;
            drop(st);
            if let Some(da) = drawing_area_weak_click.upgrade() { da.queue_draw(); }
            return;
        }

        // ── Normal click handling (no menus open) ──
        let record_hit = if recording_panel_open {
            recording_tile_at(
                rect.left, rect.top, rect.width(), rect.height(),
                screen_width as f64, screen_height as f64, x, y,
            )
        } else {
            None
        };
        let hit = if recording_panel_open {
            None
        } else {
            toolbar_hit_at(
                rect.left, rect.top, rect.width(), rect.height(),
                screen_width as f64, screen_height as f64, x, y,
            )
        };
        let clicked = match hit {
            Some(ToolbarHit::Tool(index)) if !recording_panel_open => Some(TOOLBAR_ICONS[index]),
            _ => None,
        };

        match clicked {
            Some(ToolbarIcon::Capture) => {
                drop(st);
                if let Some(window) = window_weak_click.upgrade() {
                    send_selection_result(
                        &state_click,
                        &result_tx_click,
                        &window,
                        screen_width,
                        screen_height,
                        background_click.as_ref(),
                    );
                }
            }
            Some(ToolbarIcon::Fullscreen) => {
                st.start_x = 0.0;
                st.start_y = 0.0;
                st.current_x = screen_width as f64;
                st.current_y = screen_height as f64;
                st.completed = true;
                st.is_dragging = false;
                st.fullscreen_mode = true;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() { da.queue_draw(); }
            }
            Some(ToolbarIcon::Area) => {
                let screen_w = screen_width as f64;
                let screen_h = screen_height as f64;
                let sel_w = DEFAULT_SELECTION_WIDTH.min(screen_w).max(MIN_SELECTION_WIDTH.min(screen_w));
                let sel_h = DEFAULT_SELECTION_HEIGHT.min(screen_h).max(MIN_SELECTION_HEIGHT.min(screen_h));
                let sel_x = ((screen_w - sel_w) / 2.0).max(0.0);
                let sel_y = ((screen_h - sel_h) / 2.0).max(0.0);
                st.start_x = sel_x;
                st.start_y = sel_y;
                st.current_x = sel_x + sel_w;
                st.current_y = sel_y + sel_h;
                st.completed = true;
                st.is_dragging = false;
                st.fullscreen_mode = false;
                st.recording_panel_open = false;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() { da.queue_draw(); }
            }
            Some(ToolbarIcon::Recording) => {
                st.recording_panel_open = true;
                st.hover_tool_index = None;
                st.hover_size_panel = false;
                st.hover_crop_panel = false;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() { da.queue_draw(); }
            }
            _ => {
                // Crop card clicked — open toolbar crop menu
                if !recording_panel_open && hit == Some(ToolbarHit::CropPanel) {
                    st.capture_crop_menu_open = !st.capture_crop_menu_open;
                    st.hovered_capture_crop_menu_item = -1;
                    st.hover_tool_index = None;
                    drop(st);
                    if let Some(da) = drawing_area_weak_click.upgrade() { da.queue_draw(); }
                    return;
                }

                // Recording panel tile clicks
                if recording_panel_open {
                    if let Some(tile) = record_hit {
                        match tile {
                            RecordPanelTile::Crop => {
                                st.crop_menu_open = !st.crop_menu_open;
                                st.hovered_crop_menu_item = -1;
                                st.hover_tool_index = None;
                            }
                            RecordPanelTile::Controls => {
                                st.settings_menu_open = !st.settings_menu_open;
                                st.hovered_settings_item = -1;
                                st.settings_dropdown_open = None;
                                st.hover_record_tile = None;
                                st.hover_tool_index = None;
                            }
                            RecordPanelTile::Mic => st.mic_toggle = !st.mic_toggle,
                            RecordPanelTile::Speaker => st.speaker_toggle = !st.speaker_toggle,
                            _ => {}
                        }
                        drop(st);
                        if let Some(da) = drawing_area_weak_click.upgrade() { da.queue_draw(); }
                        return;
                    }
                }

                if n_press == 2 {
                    drop(st);
                    let st = state_click.lock().unwrap();
                    let inside_selection = st.completed && is_inside_selection(x, y, current_selection_rect(&st));
                    drop(st);

                    if inside_selection {
                        if let Some(window) = window_weak_click.upgrade() {
                            send_selection_result(
                                &state_click,
                                &result_tx_click,
                                &window,
                                screen_width,
                                screen_height,
                                background_click.as_ref(),
                            );
                        }
                    }
                } else {
                    drop(st);
                }
            }
        }
    });
    drawing_area.add_controller(click_gesture);

    // Setup drag gesture for area selection
    let drag_gesture = GestureDrag::builder()
        .propagation_phase(gtk4::PropagationPhase::Capture)
        .build();

    let state_drag = state.clone();
    let drawing_area_weak = drawing_area.downgrade();

    // Note: connect_drag_begin takes 3 params (gesture, x, y)
    drag_gesture.connect_drag_begin(clone!(
        #[strong]
        state_drag,
        #[strong]
        drawing_area_weak,
        move |_gesture, x, y| {
            let mut st = state_drag.lock().unwrap();
            let (start_x, start_y) =
                clamp_point_to_bounds(x, y, screen_width as f64, screen_height as f64);

            let rect = current_selection_rect(&st);

            // Suppress drag when clicking toolbar tools, size/crop panels
            if toolbar_item_at(
                rect.left, rect.top, rect.width(), rect.height(),
                screen_width as f64, screen_height as f64, start_x, start_y,
            ).is_some()
            {
                st.is_dragging = false;
                st.drag_mode = None;
                st.initial_rect = None;
                drop(st);
                return;
            }

            let hit = toolbar_hit_at(
                rect.left, rect.top, rect.width(), rect.height(),
                screen_width as f64, screen_height as f64, start_x, start_y,
            );
            if matches!(hit, Some(ToolbarHit::CropPanel) | Some(ToolbarHit::SizePanel)) {
                st.is_dragging = false;
                st.drag_mode = None;
                st.initial_rect = None;
                drop(st);
                return;
            }

            // Suppress drag when clicking recording panel tiles
            if st.recording_panel_open && recording_tile_at(
                rect.left, rect.top, rect.width(), rect.height(),
                screen_width as f64, screen_height as f64, start_x, start_y,
            ).is_some() {
                st.is_dragging = false;
                st.drag_mode = None;
                st.initial_rect = None;
                drop(st);
                return;
            }

            // Suppress drag when clicking any open menu
            if st.capture_crop_menu_open && capture_crop_menu_hit_item(
                rect.left, rect.top, rect.width(), rect.height(),
                screen_width as f64, screen_height as f64, start_x, start_y,
            ).is_some() {
                st.is_dragging = false;
                st.drag_mode = None;
                st.initial_rect = None;
                drop(st);
                return;
            }
            if st.crop_menu_open && recording_crop_menu_hit_item(
                rect.left, rect.top, rect.width(), rect.height(),
                screen_width as f64, screen_height as f64, start_x, start_y,
            ).is_some() {
                st.is_dragging = false;
                st.drag_mode = None;
                st.initial_rect = None;
                drop(st);
                return;
            }
            // Check if drag started inside settings menu (suppress selection drag)
            if st.settings_menu_open && st.gif_slider_dragging.is_none() {
                let menu_x = (rect.left + (rect.width() - 440.0) / 2.0).clamp(10.0, screen_width as f64 - 450.0);
                let menu_y = (rect.top + 24.0).clamp(10.0, screen_height as f64 - 570.0);
                let menu_rect = RectF { x: menu_x, y: menu_y, width: 440.0, height: 560.0 };
                if menu_rect.contains(start_x, start_y) {
                    st.is_dragging = false;
                    st.drag_mode = None;
                    st.initial_rect = None;
                    drop(st);
                    return;
                }
            }

            st.drag_origin_x = start_x;
            st.drag_origin_y = start_y;
            st.initial_rect = Some(current_selection_rect(&st));

            let drag_mode = if st.completed {
                let rect = current_selection_rect(&st);
                if let Some(handle) = detect_resize_handle(start_x, start_y, rect) {
                    // Cursor is on a border/corner handle — resize.
                    DragMode::Resize(handle)
                } else if is_inside_selection(start_x, start_y, rect) {
                    // Cursor is inside the selection — move the whole rect.
                    DragMode::Move
                } else {
                    // Cursor is outside the selection — start a new one.
                    DragMode::NewSelection
                }
            } else {
                DragMode::NewSelection
            };

            st.drag_mode = Some(drag_mode);

            if matches!(drag_mode, DragMode::NewSelection) {
                st.start_x = start_x;
                st.start_y = start_y;
                st.current_x = start_x;
                st.current_y = start_y;
                st.completed = false;
            }

            st.is_dragging = true;
            drop(st);

            if let Some(drawing_area) = drawing_area_weak.upgrade() {
                drawing_area.queue_draw();
            }
        }
    ));

    drag_gesture.connect_drag_update(clone!(
        #[strong]
        state_drag,
        #[strong]
        drawing_area_weak,
        move |_gesture, x, y| {
            let mut st = state_drag.lock().unwrap();
            if st.gif_slider_dragging.is_some() {
                drop(st);
                return;
            }
            update_selection_for_drag(&mut st, x, y, screen_width as f64, screen_height as f64);
            drop(st);

            if let Some(drawing_area) = drawing_area_weak.upgrade() {
                drawing_area.queue_draw();
            }
        }
    ));

    drag_gesture.connect_drag_end(clone!(
        #[strong]
        state_drag,
        #[strong]
        drawing_area_weak,
        move |_gesture, x, y| {
            let mut st = state_drag.lock().unwrap();
            if st.gif_slider_dragging.is_some() {
                st.gif_slider_dragging = None;
                drop(st);
                if let Some(drawing_area) = drawing_area_weak.upgrade() {
                    drawing_area.queue_draw();
                }
                return;
            }
            update_selection_for_drag(&mut st, x, y, screen_width as f64, screen_height as f64);
            st.is_dragging = false;
            st.completed = true;
            st.drag_mode = None;
            st.initial_rect = None;
            drop(st);

            if let Some(drawing_area) = drawing_area_weak.upgrade() {
                drawing_area.queue_draw();
            }
        }
    ));

    drawing_area.add_controller(drag_gesture);

    // Setup keyboard controller for ESC key
    let key_controller = EventControllerKey::builder()
        .propagation_phase(gtk4::PropagationPhase::Capture)
        .build();

    let state_key = state.clone();
    let window_weak_esc = window.downgrade();
    let result_tx_esc = result_tx.clone();
    let background_key = background.clone();
    let drawing_area_weak_key = drawing_area.downgrade();

    key_controller.connect_key_pressed(clone!(
        #[strong]
        state_key,
        move |_, key, _, _| {
            if key == Key::Escape {
                let mut st = state_key.lock().unwrap();
                st.cancelled = true;
                st.fullscreen_mode = false;
                drop(st);

                let _ = result_tx_esc.send(Ok(None));

                if let Some(window) = window_weak_esc.upgrade() {
                    window.close();
                }

                return glib::Propagation::Stop;
            }

            if key == Key::Return
                || key == Key::KP_Enter
                || key == Key::ISO_Enter
                || key == Key::space
            {
                if let Some(window) = window_weak_esc.upgrade() {
                    send_selection_result(
                        &state_key,
                        &result_tx_esc,
                        &window,
                        screen_width,
                        screen_height,
                        background_key.as_ref(),
                    );
                }

                return glib::Propagation::Stop;
            }

            let delta = match key {
                Key::Left => Some((-1.0, 0.0)),
                Key::Right => Some((1.0, 0.0)),
                Key::Up => Some((0.0, -1.0)),
                Key::Down => Some((0.0, 1.0)),
                _ => None,
            };

            if let Some((dx, dy)) = delta {
                let mut st = state_key.lock().unwrap();
                if st.completed {
                    let rect = current_selection_rect(&st);
                    let next = SelectionRectF {
                        left: (rect.left + dx)
                            .clamp(0.0, (screen_width as f64 - rect.width()).max(0.0)),
                        top: (rect.top + dy)
                            .clamp(0.0, (screen_height as f64 - rect.height()).max(0.0)),
                        right: 0.0,
                        bottom: 0.0,
                    };
                    let moved = SelectionRectF {
                        right: next.left + rect.width(),
                        bottom: next.top + rect.height(),
                        ..next
                    };
                    set_selection_rect(&mut st, moved);
                    st.fullscreen_mode = false;
                    drop(st);
                    if let Some(drawing_area) = drawing_area_weak_key.upgrade() {
                        drawing_area.queue_draw();
                    }
                    return glib::Propagation::Stop;
                }
            }

            glib::Propagation::Proceed
        }
    ));

    window.add_controller(key_controller);

    // On X11: set compositor-bypass hints as soon as the native window is
    // realized (XID assigned) but BEFORE it is mapped/shown.  Using
    // connect_realize instead of connect_map means the compositor sees the
    // correct _NET_WM_WINDOW_TYPE and _NET_WM_BYPASS_COMPOSITOR on the very
    // first MapNotify event, so it never starts an open/close animation.
    let window_bypass = window.downgrade();
    window.connect_realize(move |_| {
        if let Some(win) = window_bypass.upgrade() {
            suppress_x11_compositor_animation(&win);
        }
    });

    // Show the window
    let _ = window.grab_focus();
    window.present();
}

/// Paint a surface scaled to fill the full screen.
/// `surface_w` / `surface_h` are the pixel dimensions of the surface.
fn paint_surface_fullscreen(
    context: &gtk4::cairo::Context,
    surface: &gtk4::cairo::ImageSurface,
    surface_w: i32,
    surface_h: i32,
    screen_width: f64,
    screen_height: f64,
) {
    let _ = context.save();
    context.scale(
        screen_width / surface_w.max(1) as f64,
        screen_height / surface_h.max(1) as f64,
    );
    if context.set_source_surface(surface, 0.0, 0.0).is_ok() {
        let _ = context.paint();
    }
    let _ = context.restore();
}

/// Paint a surface scaled to fill the full screen, but clipped to `clip_rect`.
/// The clip is applied in screen coordinates before the scale transform.
fn paint_surface_clipped(
    context: &gtk4::cairo::Context,
    surface: &gtk4::cairo::ImageSurface,
    surface_w: i32,
    surface_h: i32,
    screen_width: f64,
    screen_height: f64,
    clip_x: f64,
    clip_y: f64,
    clip_w: f64,
    clip_h: f64,
) {
    let _ = context.save();
    // Clip in screen-space first, then scale into image-space.
    context.rectangle(clip_x, clip_y, clip_w, clip_h);
    context.clip();
    context.scale(
        screen_width / surface_w.max(1) as f64,
        screen_height / surface_h.max(1) as f64,
    );
    if context.set_source_surface(surface, 0.0, 0.0).is_ok() {
        let _ = context.paint();
    }
    let _ = context.restore();
}

/// Draw the overlay (dark background + clear selection rectangle)
fn draw_overlay(
    context: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    state: &Arc<Mutex<SelectorState>>,
    background: Option<&BackgroundFrame>,
) {
    let st = state.lock().unwrap();

    let screen_width = width.max(1) as f64;
    let screen_height = height.max(1) as f64;

    // ── Step 1: paint the background across the entire screen ──
    // Paint the original screenshot (if available) then darken it with a
    // semi-transparent overlay. No blur — this keeps opening instant.
    if let Some(bg) = background {
        paint_surface_fullscreen(
            context,
            &bg.surface,
            bg.width,
            bg.height,
            screen_width,
            screen_height,
        );
        // Dark tint over the full screen; the selection area will be
        // revealed sharp in Step 2 by painting the original on top.
        context.set_source_rgba(0.0, 0.0, 0.0, 140.0 / 255.0);
        let _ = context.paint();
    } else {
        // No pre-captured background (capture-after-selection / live overlay path).
        // Use a very light tint so the desktop remains clearly visible.
        // The selection rectangle will be fully cleared (Operator::Clear) to show
        // the desktop at 100% brightness inside the selection.
        context.set_source_rgba(0.0, 0.0, 0.0, 0.20);
        let _ = context.paint();
    }

    if st.fullscreen_mode {
        // ── Fullscreen mode: the whole screen IS the selection ──
        // The darkened background is already painted; add a very subtle extra
        // vignette so the corner markers and toolbar stand out.
        if background.is_some() {
            context.set_source_rgba(0.0, 0.0, 0.0, 0.10);
            let _ = context.paint();
        } else {
            // Full-screen mode with live background means the full desktop is
            // selected, so clear the dimming tint entirely.
            let _ = context.save();
            context.set_operator(gtk4::cairo::Operator::Clear);
            let _ = context.paint();
            let _ = context.restore();
        }

        // Corner markers at screen edges
        draw_resize_markers(context, 0.0, 0.0, screen_width, screen_height);

        // Toolbar (auto-positioned below / above the full-screen rect)
        draw_feature_toolbar(
            context,
            0.0,
            0.0,
            screen_width,
            screen_height,
            screen_width,
            screen_height,
            background,
            st.hover_tool_index,
            st.hover_size_panel,
            st.hover_crop_panel,
            st.capture_crop_menu_open,
            st.capture_aspect_ratio_index,
            st.hovered_capture_crop_menu_item,
        );
    } else if st.is_dragging || st.completed {
        // ── Normal area-selection mode ──
        let rect = current_selection_rect(&st);
        let x = rect.left;
        let y = rect.top;
        let sel_w = rect.width();
        let sel_h = rect.height();

        if st.is_dragging {
            context.set_source_rgba(BRAND_ORANGE_R, BRAND_ORANGE_G, BRAND_ORANGE_B, 0.63);
            context.set_line_width(1.0);
            context.move_to(0.0, st.current_y);
            context.line_to(screen_width, st.current_y);
            context.move_to(st.current_x, 0.0);
            context.line_to(st.current_x, screen_height);
            let _ = context.stroke();
        }

        // ── Step 2: reveal the original (sharp) image inside the selection ──
        if let Some(bg) = background {
            paint_surface_clipped(
                context,
                &bg.surface,
                bg.width,
                bg.height,
                screen_width,
                screen_height,
                x,
                y,
                sel_w,
                sel_h,
            );
        } else {
            // Live selector path: reveal the selected rectangle from the real
            // desktop by clearing the tint in that region.
            let _ = context.save();
            context.set_operator(gtk4::cairo::Operator::Clear);
            context.rectangle(x, y, sel_w, sel_h);
            let _ = context.fill();
            let _ = context.restore();
        }

        if st.is_dragging {
            context.set_source_rgba(BRAND_ORANGE_R, BRAND_ORANGE_G, BRAND_ORANGE_B, 30.0 / 255.0);
            context.rectangle(x, y, sel_w, sel_h);
            let _ = context.fill();
        }

        if st.is_dragging {
            draw_crosshair_bubble(
                context,
                st.current_x,
                st.current_y,
                &format!("{} × {}", sel_w as i32, sel_h as i32),
            );
        }

        draw_resize_markers(context, x, y, sel_w, sel_h);

        // ── Step 3: toolbar + resize markers on top ──
        if st.recording_panel_open {
            draw_recording_panel(
                context, x, y, sel_w, sel_h,
                screen_width, screen_height, background,
                st.hover_record_tile,
                st.crop_menu_open,
                st.record_aspect_ratio_index,
                st.hovered_crop_menu_item,
                st.settings_menu_open,
                st.settings_tab,
                st.hovered_settings_item,
                st.settings_dropdown_open,
                st.video_max_res, st.video_fps, st.record_mono, st.open_editor,
                st.rec_controls, st.display_rec_time, st.hidpi, st.do_not_disturb,
                st.show_cursor, st.rec_clicks, st.rec_keystrokes,
                st.remember_selection, st.dim_screen, st.show_countdown,
                st.gif_fps, st.gif_quality, st.optimize_gif, st.gif_size_idx,
            );
        } else {
            draw_feature_toolbar(
                context, x, y, sel_w, sel_h,
                screen_width, screen_height, background,
                st.hover_tool_index, st.hover_size_panel, st.hover_crop_panel,
                st.capture_crop_menu_open,
                st.capture_aspect_ratio_index,
                st.hovered_capture_crop_menu_item,
            );
        }
    }
    // else: idle state — the darkened background painted in Step 1 is enough.
}

fn draw_crosshair_bubble(context: &gtk4::cairo::Context, x: f64, y: f64, label: &str) {
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(12.0);
    let (text_w, text_h) = context
        .text_extents(label)
        .map(|e| (e.width(), e.height()))
        .unwrap_or((64.0, 14.0));
    let bubble_w = text_w + 22.0;
    let bubble_h = text_h + 14.0;
    let bx = x + 14.0;
    let by = y + 14.0;
    rounded_rect_path(context, bx, by, bubble_w, bubble_h, 6.0);
    context.set_source_rgba(0.0, 0.0, 0.0, 0.70);
    let _ = context.fill();
    rounded_rect_path(
        context,
        bx + 0.5,
        by + 0.5,
        bubble_w - 1.0,
        bubble_h - 1.0,
        6.0,
    );
    context.set_source_rgba(1.0, 1.0, 1.0, 0.16);
    context.set_line_width(1.0);
    let _ = context.stroke();
    draw_text_centered(
        context,
        RectF {
            x: bx,
            y: by,
            width: bubble_w,
            height: bubble_h,
        },
        label,
        12.0,
        false,
        (1.0, 1.0, 1.0, 1.0),
    );
}

impl Default for AreaSelector {
    fn default() -> Self {
        Self::new()
    }
}

/// Run the interactive area selector.
///
/// **Primary path:** launches the native C++ Qt5 `apexshot-capture` binary.
/// This works reliably on both X11 and Wayland (GNOME, KDE, Sway, etc.)
/// because Qt handles compositor quirks natively.
///
/// **Fallback:** if the C++ binary is not found, falls back to the GTK4
/// overlay implementation (legacy path, may be unreliable on GNOME Wayland).
pub fn select_area() -> SelectionResult {
    match crate::capture_overlay::run_capture_overlay(None) {
        Ok(result) => return Ok(result),
        Err(e) => {
            eprintln!(
                "[overlay] C++ capture overlay unavailable ({e}), falling back to GTK4 selector"
            );
        }
    }
    // GTK4 fallback
    let selector = AreaSelector::new();
    selector.run()
}

/// Run area selection against a static screenshot image.
///
/// Saves the image to a temp file, passes it to `apexshot-capture --background`,
/// then deletes the temp file. Falls back to the GTK4 path if unavailable.
pub fn select_area_from_image(image: &RgbaImage) -> SelectionResult {
    // Write the image to a temp PNG for the C++ binary to load
    let tmp_path = std::env::temp_dir().join(format!(
        "apexshot_bg_{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    let write_ok = image.save(&tmp_path).is_ok();
    if write_ok {
        let result = crate::capture_overlay::run_capture_overlay(Some(&tmp_path));
        let _ = std::fs::remove_file(&tmp_path);
        match result {
            Ok(r) => return Ok(r),
            Err(e) => eprintln!("[overlay] C++ capture overlay failed ({e}), falling back to GTK4"),
        }
    }
    // GTK4 fallback
    let selector = AreaSelector::new();
    let background = background_frame_from_image(image)?;
    selector.run_with_background(Some(background))
}

/// Convert `CaptureData` pixels directly to Cairo ARGB bytes in one parallel pass.
///
/// Avoids the intermediate `RgbaImage` allocation that `capture_to_rgba_image` would
/// produce, saving one full-resolution copy (~8 MB for 1080p).
fn capture_to_cairo_argb_bytes(capture: &CaptureData, stride: usize) -> Vec<u8> {
    let width = capture.width as usize;
    let height = capture.height as usize;
    let src_stride = capture.stride as usize;
    let fmt = capture.format;
    let pixels = &capture.pixels;

    let mut out = vec![0u8; stride * height];

    out.par_chunks_mut(stride)
        .enumerate()
        .for_each(|(y, dst_row)| {
            let src_row_start = y * src_stride;
            for x in 0..width {
                let si = src_row_start + x * fmt.bytes_per_pixel as usize;
                let di = x * 4;
                // Map any supported pixel format to Cairo ARGB (BGRA in memory).
                let (r, g, b) = if fmt == PixelFormat::RGBA32 || fmt == PixelFormat::RGB32 {
                    (pixels[si], pixels[si + 1], pixels[si + 2])
                } else if fmt == PixelFormat::BGRA32 || fmt == PixelFormat::BGR32 {
                    (pixels[si + 2], pixels[si + 1], pixels[si])
                } else if fmt == PixelFormat::RGB24 {
                    (pixels[si], pixels[si + 1], pixels[si + 2])
                } else {
                    // BGR24
                    (pixels[si + 2], pixels[si + 1], pixels[si])
                };
                dst_row[di] = b;
                dst_row[di + 1] = g;
                dst_row[di + 2] = r;
                dst_row[di + 3] = 255; // screenshots are always opaque
            }
        });

    out
}

/// Build a `BackgroundFrame` directly from raw `CaptureData`.
///
/// Skips the `RgbaImage` intermediate entirely — one fewer full-resolution
/// allocation and copy compared to `background_frame_from_image`.
fn background_frame_from_capture(capture: &CaptureData) -> Result<BackgroundFrame, SelectionError> {
    let width = capture.width;
    let height = capture.height;
    if width == 0 || height == 0 {
        return Err(SelectionError::InitError(
            "Cannot select from an empty screenshot".into(),
        ));
    }

    let stride = gtk4::cairo::Format::ARgb32
        .stride_for_width(width)
        .map_err(|e| SelectionError::InitError(e.to_string()))? as usize;

    let small_w = (width / 4).max(1);
    let small_h = (height / 4).max(1);
    let blur_stride = gtk4::cairo::Format::ARgb32
        .stride_for_width(small_w)
        .map_err(|e| SelectionError::InitError(e.to_string()))? as usize;

    // Build full-res ARGB buffer and the blur buffer in parallel.
    let (full_data, blur_data) = rayon::join(
        || capture_to_cairo_argb_bytes(capture, stride),
        || {
            // Build a tiny RgbaImage just for the blur (cheap at 1/4 size).
            let row_len = width as usize * 4;
            let mut rgba_pixels: Vec<u8> = Vec::with_capacity(row_len * height as usize);
            let src_stride = capture.stride as usize;
            let fmt = capture.format;
            let pixels = &capture.pixels;
            for y in 0..height as usize {
                let src_row_start = y * src_stride;
                for x in 0..width as usize {
                    let si = src_row_start + x * fmt.bytes_per_pixel as usize;
                    let (r, g, b) = if fmt == PixelFormat::RGBA32 || fmt == PixelFormat::RGB32 {
                        (pixels[si], pixels[si + 1], pixels[si + 2])
                    } else if fmt == PixelFormat::BGRA32 || fmt == PixelFormat::BGR32 {
                        (pixels[si + 2], pixels[si + 1], pixels[si])
                    } else if fmt == PixelFormat::RGB24 {
                        (pixels[si], pixels[si + 1], pixels[si + 2])
                    } else {
                        (pixels[si + 2], pixels[si + 1], pixels[si])
                    };
                    rgba_pixels.extend_from_slice(&[r, g, b, 255]);
                }
            }
            let small_rgba: RgbaImage = image::ImageBuffer::from_raw(width, height, rgba_pixels)
                .expect("pixel buffer size mismatch");
            let small = image::imageops::resize(
                &small_rgba,
                small_w,
                small_h,
                image::imageops::FilterType::Nearest,
            );
            let blurred = image::imageops::blur(&small, 8.0);
            rgba_to_cairo_argb_bytes(&blurred, blur_stride)
        },
    );

    let surface = gtk4::cairo::ImageSurface::create_for_data(
        full_data,
        gtk4::cairo::Format::ARgb32,
        width as i32,
        height as i32,
        stride as i32,
    )
    .map_err(|e| SelectionError::InitError(e.to_string()))?;

    let toolbar_blur_surface = gtk4::cairo::ImageSurface::create_for_data(
        blur_data,
        gtk4::cairo::Format::ARgb32,
        small_w as i32,
        small_h as i32,
        blur_stride as i32,
    )
    .map_err(|e| SelectionError::InitError(e.to_string()))?;

    Ok(BackgroundFrame {
        surface,
        toolbar_blur_surface,
        width: width as i32,
        height: height as i32,
    })
}

/// Run area selection directly from a raw `CaptureData` screenshot.
///
/// Saves to a temp PNG, passes to `apexshot-capture --background`, then
/// crops the result from the capture data. Falls back to GTK4 if unavailable.
pub fn select_area_from_capture(capture: &CaptureData) -> SelectionResult {
    // Write to temp PNG for the C++ binary
    let tmp_path = std::env::temp_dir().join(format!(
        "apexshot_bg_{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));

    // Convert CaptureData to RGBA image for saving
    let write_ok = (|| -> Option<()> {
        use crate::backend::PixelFormat;
        let w = capture.width;
        let h = capture.height;
        let fmt = capture.format;
        let src_stride = capture.stride as usize;
        let bpp = fmt.bytes_per_pixel as usize;
        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h as usize {
            let row_start = y * src_stride;
            for x in 0..w as usize {
                let si = row_start + x * bpp;
                let (r, g, b) = if fmt == PixelFormat::RGBA32 || fmt == PixelFormat::RGB32 {
                    (
                        capture.pixels[si],
                        capture.pixels[si + 1],
                        capture.pixels[si + 2],
                    )
                } else if fmt == PixelFormat::BGRA32 || fmt == PixelFormat::BGR32 {
                    (
                        capture.pixels[si + 2],
                        capture.pixels[si + 1],
                        capture.pixels[si],
                    )
                } else if fmt == PixelFormat::RGB24 {
                    (
                        capture.pixels[si],
                        capture.pixels[si + 1],
                        capture.pixels[si + 2],
                    )
                } else {
                    (
                        capture.pixels[si + 2],
                        capture.pixels[si + 1],
                        capture.pixels[si],
                    )
                };
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        let img: image::RgbaImage = image::ImageBuffer::from_raw(w, h, rgba)?;
        img.save(&tmp_path).ok()?;
        Some(())
    })()
    .is_some();

    if write_ok {
        let result = crate::capture_overlay::run_capture_overlay(Some(&tmp_path));
        let _ = std::fs::remove_file(&tmp_path);
        match result {
            Ok(r) => return Ok(r),
            Err(e) => eprintln!("[overlay] C++ capture overlay failed ({e}), falling back to GTK4"),
        }
    }

    // GTK4 fallback
    let selector = AreaSelector::new();
    let background = background_frame_from_capture(capture)?;
    selector.run_with_background(Some(background))
}

pub fn select_area_from_capture_with_gtk(capture: &CaptureData) -> SelectionResult {
    let selector = AreaSelector::new();
    let background = background_frame_from_capture(capture)?;
    selector.run_with_background(Some(background))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_normalize() {
        // Normal case (no normalization needed)
        let area = SelectionArea {
            x: 100,
            y: 100,
            width: 200,
            height: 150,
        };
        let normalized = area.normalize();
        assert_eq!(normalized.x, 100);
        assert_eq!(normalized.y, 100);
        assert_eq!(normalized.width, 200);
        assert_eq!(normalized.height, 150);

        // Negative width (dragged left)
        let area = SelectionArea {
            x: 300,
            y: 100,
            width: -200,
            height: 150,
        };
        let normalized = area.normalize();
        assert_eq!(normalized.x, 100);
        assert_eq!(normalized.y, 100);
        assert_eq!(normalized.width, 200);
        assert_eq!(normalized.height, 150);

        // Negative height (dragged up)
        let area = SelectionArea {
            x: 100,
            y: 250,
            width: 200,
            height: -150,
        };
        let normalized = area.normalize();
        assert_eq!(normalized.x, 100);
        assert_eq!(normalized.y, 100);
        assert_eq!(normalized.width, 200);
        assert_eq!(normalized.height, 150);

        // Both negative (dragged up-left)
        let area = SelectionArea {
            x: 300,
            y: 250,
            width: -200,
            height: -150,
        };
        let normalized = area.normalize();
        assert_eq!(normalized.x, 100);
        assert_eq!(normalized.y, 100);
        assert_eq!(normalized.width, 200);
        assert_eq!(normalized.height, 150);
    }

    #[test]
    fn test_selection_is_valid() {
        // Valid selection
        let area = SelectionArea {
            x: 100,
            y: 100,
            width: 200,
            height: 150,
        };
        assert!(area.is_valid());

        // Zero width
        let area = SelectionArea {
            x: 100,
            y: 100,
            width: 0,
            height: 150,
        };
        assert!(!area.is_valid());

        // Zero height
        let area = SelectionArea {
            x: 100,
            y: 100,
            width: 200,
            height: 0,
        };
        assert!(!area.is_valid());

        // Negative (before normalization)
        let area = SelectionArea {
            x: 100,
            y: 100,
            width: -200,
            height: 150,
        };
        assert!(!area.is_valid());
    }
}
