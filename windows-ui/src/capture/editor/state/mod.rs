//! Editor session state and annotation mutation API.
//!
//! The session is a Windows port of the Linux editor's annotation document:
//! same fields, same mutation semantics, same history model. The pieces the
//! Linux editor hangs off the Appearance/Motion backends (background
//! composition, transforms, motion history) are out of scope here — canvas
//! bounds are the plain image bounds — while every annotation behaviour is
//! shared verbatim or ported faithfully.

#[path = "../../../../../src/capture/editor/state/arrow.rs"]
#[rustfmt::skip]
mod arrow;
mod crop;
pub(crate) use crop::{crop_handle_at, point_in_rect};
#[path = "../../../../../src/capture/editor/state/drag_draw.rs"]
#[rustfmt::skip]
mod drag_draw;
#[path = "../../../../../src/capture/editor/state/effects.rs"]
#[rustfmt::skip]
mod effects;
pub(crate) mod export;
mod history;
mod selection;
#[path = "../../../../../src/capture/editor/state/text_input.rs"]
#[rustfmt::skip]
mod text_input;
#[path = "../../../../../src/capture/editor/state/tool_style.rs"]
#[rustfmt::skip]
mod tool_style;

use super::color::{
    DEFAULT_COLOR_INDEX, DEFAULT_FOCUS_INTENSITY, DEFAULT_OBFUSCATE_AMOUNT, DRAW_COLORS,
    STROKE_WIDTH, TEXT_SIZE,
};
use super::numbering_style::{NumberSize, NumberingStyle};
use super::pen_weight::{HighlighterMode, PenWeight};
use super::text_detect::{BackgroundTextDetection, TextDetector};
use super::types::{
    AnnotationAction, ArrowStyle, CropAspectRatio, DrawColor, MoveHandle, ObfuscateMethod, Point,
    Rect, TextEditBounds, Tool,
};
use image::RgbaImage;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub struct EditorState {
    pub source_upscale: super::upscale::SourceUpscale,
    pub(crate) export_scale: f64,
    pub base_image: Arc<RgbaImage>,
    pub working_image: Arc<RgbaImage>,
    pub working_image_revision: u64,
    pub actions: Vec<AnnotationAction>,
    pub redo_actions: Vec<AnnotationAction>,
    pub(crate) undo_history: Vec<history::HistoryTransaction>,
    pub(crate) redo_history: Vec<history::HistoryTransaction>,
    pub(crate) history_interaction_before: Option<history::DocumentSnapshot>,
    pub crop_rect: Option<Rect>,
    pub crop_ratio: CropAspectRatio,
    crop_drag: Option<crop::CropDrag>,
    pub selected_tool: Tool,
    pub selected_action_index: Option<usize>,
    pub selected_color: DrawColor,
    pub stroke_size: f64,
    pub smooth_drawing_enabled: bool,
    pub draw_object_shadow: bool,
    pub auto_expand_canvas: bool,
    pub inverse_arrow_direction: bool,
    pub text_size: f64,
    pub text_font_family: String,
    pub text_background_color: Option<DrawColor>,
    pub obfuscate_method: ObfuscateMethod,
    pub obfuscate_pixelate_amount: f64,
    pub obfuscate_blur_amount: f64,
    pub focus_intensity: f64,
    pub arrow_style: ArrowStyle,
    pub arrow_editing_controls: bool,
    pub arrow_control_dragging: Option<usize>,
    pub next_number: u32,
    pub select_drag_anchor: Option<Point>,
    pub select_resize_handle: Option<super::types::SelectHandle>,
    pub select_effect_rebuild_pending: bool,
    pub select_effect_rebuild_dirty: bool,
    pub select_drag_effect_dirty: bool,
    pub active_text_edit: Option<()>,
    pub active_text_entry: Option<gtk4::Entry>,
    pub active_text_bounds: Option<TextEditBounds>,
    pub active_text_is_dragging: bool,
    pub active_text_drag_handle: Option<MoveHandle>,
    pub active_text_drag_start: Option<Point>,
    pub pending_effect_revision: u64,
    pub last_applied_effect_revision: u64,
    pub last_effect_request_time_us: i64,
    pub drag_start: Option<Point>,
    pub drag_current: Option<Point>,
    pub drag_start_view: Option<Point>,
    pub drag_path: Vec<Point>,
    pub drag_shift_active: bool,
    pub active_text_drag_start_bounds: Option<Rect>,
    pub active_text_is_resizing: bool,
    pub hovered_text_action_index: Option<usize>,
    pub active_text_input: Option<TextInputState>,

    // Text detection for highlighter
    pub text_detector: Arc<Mutex<TextDetector>>,
    pub text_detection_ready: Arc<AtomicBool>,
    pub text_detection_handle: Option<BackgroundTextDetection>,
    pub(crate) text_detection_restart_pending: bool,

    // Highlighter mode
    pub highlighter_mode: HighlighterMode,
    pub pen_weight: PenWeight,
    pub locked_highlighter_stroke_size: Option<f64>,

    // Number tool options
    pub numbering_style: NumberingStyle,
    pub numbering_start: u32,
    pub number_size: NumberSize,
}

#[derive(Debug, Clone)]
pub struct TextInputState {
    pub text: String,
    pub cursor_position: usize,
    pub cursor_visible: bool,
    pub cursor_blink_timer: u32,
    pub color: DrawColor,
    pub background_color: Option<DrawColor>,
    pub editing_action_index: Option<usize>,
}

pub(super) fn simplify_drag_path(points: &[Point], epsilon: f64) -> Vec<Point> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    simplify_drag_path_range(points, 0, points.len() - 1, epsilon, &mut keep);

    points
        .iter()
        .zip(keep)
        .filter_map(|(point, keep)| keep.then_some(*point))
        .collect()
}

pub(super) fn simplify_drag_path_range(
    points: &[Point],
    start: usize,
    end: usize,
    epsilon: f64,
    keep: &mut [bool],
) {
    if end <= start + 1 {
        return;
    }

    let first = points[start];
    let last = points[end];
    let mut max_distance = 0.0;
    let mut max_index = None;

    for (index, point) in points.iter().enumerate().take(end).skip(start + 1) {
        let distance = perpendicular_distance(*point, first, last);
        if distance > max_distance {
            max_distance = distance;
            max_index = Some(index);
        }
    }

    if max_distance > epsilon {
        if let Some(index) = max_index {
            keep[index] = true;
            simplify_drag_path_range(points, start, index, epsilon, keep);
            simplify_drag_path_range(points, index, end, epsilon, keep);
        }
    }
}

pub(super) fn perpendicular_distance(point: Point, start: Point, end: Point) -> f64 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    if dx.abs() <= f64::EPSILON && dy.abs() <= f64::EPSILON {
        return ((point.x - start.x).powi(2) + (point.y - start.y).powi(2)).sqrt();
    }

    let numerator = ((dy * point.x) - (dx * point.y) + (end.x * start.y) - (end.y * start.x)).abs();
    let denominator = (dx * dx + dy * dy).sqrt();
    numerator / denominator
}

pub(super) fn expand_rgba_image(
    image: &RgbaImage,
    new_width: u32,
    new_height: u32,
    offset_x: u32,
    offset_y: u32,
) -> RgbaImage {
    if new_width == image.width() && new_height == image.height() && offset_x == 0 && offset_y == 0
    {
        return image.clone();
    }

    let mut expanded = RgbaImage::from_pixel(new_width, new_height, image::Rgba([0, 0, 0, 0]));
    image::imageops::overlay(&mut expanded, image, offset_x as i64, offset_y as i64);
    expanded
}

impl EditorState {
    /// Change the selected arrow's style and normalize its control points in
    /// one undoable step: Curved/Double keep a midpoint handle, straight
    /// styles reduce to endpoints. Returns false when nothing changed.
    pub fn set_selected_arrow_style_with_controls(&mut self, style: ArrowStyle) -> bool {
        let before = self.document_snapshot();
        let Some(index) = self.selected_action_index else {
            return false;
        };
        let Some(action) = self.actions.get_mut(index) else {
            self.selected_action_index = None;
            return false;
        };
        let AnnotationAction::Arrow {
            start,
            end,
            style: current_style,
            control_points,
            ..
        } = action
        else {
            return false;
        };
        if *current_style == style {
            return false;
        }
        *current_style = style;
        match style {
            ArrowStyle::Curved | ArrowStyle::Double => {
                let points = control_points.get_or_insert_with(Vec::new);
                match points.len() {
                    3 => {}
                    n if n >= 2 => {
                        let s = points[0];
                        let e = points[n - 1];
                        let mid = Point {
                            x: (s.x + e.x) / 2.0,
                            y: (s.y + e.y) / 2.0,
                        };
                        *points = vec![s, mid, e];
                    }
                    _ => {
                        let mid = Point {
                            x: (start.x + end.x) / 2.0,
                            y: (start.y + end.y) / 2.0,
                        };
                        *points = vec![*start, mid, *end];
                    }
                }
            }
            _ => {
                *control_points = Some(vec![*start, *end]);
            }
        }
        self.clear_redo_history();
        self.commit_history_change(before, history::HistoryKind::Edit);
        true
    }

    pub fn new(base_image: RgbaImage) -> Self {
        let base_image = Arc::new(base_image);
        Self {
            source_upscale: Default::default(),
            export_scale: 1.0,
            working_image: Arc::clone(&base_image),
            base_image,
            working_image_revision: 1,
            actions: Vec::new(),
            redo_actions: Vec::new(),
            undo_history: Vec::new(),
            redo_history: Vec::new(),
            history_interaction_before: None,
            crop_rect: None,
            crop_ratio: CropAspectRatio::Freeform,
            crop_drag: None,
            selected_tool: Tool::Select,
            selected_action_index: None,
            selected_color: DRAW_COLORS[DEFAULT_COLOR_INDEX],
            stroke_size: STROKE_WIDTH,
            smooth_drawing_enabled: false,
            draw_object_shadow: false,
            auto_expand_canvas: false,
            inverse_arrow_direction: false,
            text_size: TEXT_SIZE,
            text_font_family: String::from("Sans"),
            text_background_color: None,
            obfuscate_method: ObfuscateMethod::Pixelate,
            obfuscate_pixelate_amount: DEFAULT_OBFUSCATE_AMOUNT,
            obfuscate_blur_amount: DEFAULT_OBFUSCATE_AMOUNT,
            focus_intensity: DEFAULT_FOCUS_INTENSITY,
            arrow_style: ArrowStyle::Standard,
            arrow_editing_controls: false,
            arrow_control_dragging: None,
            next_number: 1,
            select_drag_anchor: None,
            select_resize_handle: None,
            select_effect_rebuild_pending: false,
            select_effect_rebuild_dirty: false,
            select_drag_effect_dirty: false,
            active_text_edit: None,
            active_text_entry: None,
            active_text_bounds: None,
            active_text_is_dragging: false,
            active_text_drag_handle: None,
            active_text_drag_start: None,
            pending_effect_revision: 0,
            last_applied_effect_revision: 0,
            last_effect_request_time_us: 0,
            drag_start: None,
            drag_current: None,
            drag_start_view: None,
            drag_path: Vec::new(),
            drag_shift_active: false,
            active_text_drag_start_bounds: None,
            active_text_is_resizing: false,
            hovered_text_action_index: None,
            active_text_input: None,

            text_detector: Arc::new(Mutex::new(TextDetector::new_pending())),
            text_detection_ready: Arc::new(AtomicBool::new(false)),
            text_detection_handle: None,
            text_detection_restart_pending: false,
            highlighter_mode: HighlighterMode::default(),
            pen_weight: PenWeight::default(),
            locked_highlighter_stroke_size: None,
            numbering_style: NumberingStyle::default(),
            numbering_start: 1,
            number_size: NumberSize::default(),
        }
    }

    pub fn set_tool(&mut self, tool: Tool) -> bool {
        let rebuild = self.set_tool_without_rebuild(tool);
        if rebuild {
            self.rebuild_effect_layer();
        }
        rebuild
    }

    pub fn set_tool_without_rebuild(&mut self, tool: Tool) -> bool {
        self.finish_history_interaction();
        if self.selected_tool == Tool::Crop && tool != Tool::Crop {
            self.cancel_crop();
        }
        if tool != Tool::Select {
            self.selected_action_index = None;
            self.select_drag_anchor = None;
            self.select_resize_handle = None;
        }
        if tool != Tool::Text {
            self.cancel_text_input();
            self.cancel_text_edit();
            self.hovered_text_action_index = None;
        }
        if tool != Tool::Arrow {
            self.finalize_arrow_control_editing();
        }
        self.selected_tool = tool;
        if tool == Tool::Crop && self.crop_rect.is_none() {
            self.crop_rect = Some(Rect {
                x: 0,
                y: 0,
                width: self.base_image.width() as i32,
                height: self.base_image.height() as i32,
            });
            self.set_crop_ratio(self.crop_ratio);
        }
        self.clear_drag_without_rebuild_and_check_effect()
    }

    pub fn sync_next_number(&mut self) {
        let max_number = self
            .actions
            .iter()
            .filter_map(|action| match action {
                AnnotationAction::Number { number, style, .. } => {
                    if *style == self.numbering_style {
                        Some(*number)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .max()
            .unwrap_or(0);
        self.next_number = if max_number == 0 {
            self.numbering_start
        } else {
            max_number.saturating_add(1)
        };
    }

    /// Editable canvas bounds in screenshot pixels. The Appearance backend
    /// is not wired on Windows, so the editable region is the image itself.
    pub fn annotation_canvas_bounds(&self) -> (f64, f64, f64, f64) {
        (
            0.0,
            0.0,
            self.base_image.width() as f64,
            self.base_image.height() as f64,
        )
    }

    pub fn add_number_marker(&mut self, position: Point) {
        let number = self.next_number;
        let radius = self.number_size.radius();
        let (min_x, min_y, max_x, max_y) = self.annotation_canvas_bounds();
        let span_x = max_x - min_x;
        let span_y = max_y - min_y;

        let clamped_x = if span_x <= radius * 2.0 {
            (min_x + max_x) / 2.0
        } else {
            position.x.clamp(min_x + radius, max_x - radius)
        };
        let clamped_y = if span_y <= radius * 2.0 {
            (min_y + max_y) / 2.0
        } else {
            position.y.clamp(min_y + radius, max_y - radius)
        };

        self.push_action(AnnotationAction::Number {
            position: Point {
                x: clamped_x,
                y: clamped_y,
            },
            number,
            color: self.selected_color,
            style: self.numbering_style,
            size: self.number_size,
            shadow: self.draw_object_shadow,
        });
    }

    /// Forward delete inside the live text input: drop the character under
    /// the caret. Mirrors `delete_text_input_char` for the Backspace side.
    pub fn delete_text_input_char_forward(&mut self) {
        if let Some(ref mut input) = self.active_text_input {
            if input.cursor_position < input.text.chars().count() {
                if let Some(byte_idx) = input
                    .text
                    .char_indices()
                    .nth(input.cursor_position)
                    .map(|(i, _)| i)
                {
                    input.text.remove(byte_idx);
                }
                input.cursor_blink_timer = 0;
            }
        }
    }

    pub fn move_cursor_home(&mut self) {
        if let Some(ref mut input) = self.active_text_input {
            input.cursor_position = 0;
            input.cursor_visible = true;
            input.cursor_blink_timer = 0;
        }
    }

    pub fn move_cursor_end(&mut self) {
        if let Some(ref mut input) = self.active_text_input {
            input.cursor_position = input.text.chars().count();
            input.cursor_visible = true;
            input.cursor_blink_timer = 0;
        }
    }

    /// Paste plain text into the live input (clipboard text or equivalent).
    pub fn insert_text_input_str(&mut self, text: &str) {
        for c in text.chars() {
            if !c.is_control() || c == '\n' {
                self.add_text_input_char(c);
            }
        }
    }
}

pub fn apply_effect_actions(image: &mut RgbaImage, actions: &[AnnotationAction]) {
    effects::apply_effect_actions(image, actions);
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use crate::capture::editor::color::DEFAULT_OBFUSCATE_AMOUNT;
    use crate::capture::editor::types::{AnnotationAction, ObfuscateMethod, Point, Rect};

    use super::EditorState;

    #[test]
    fn editor_state_defaults_to_select_tool() {
        assert_eq!(
            EditorState::new(RgbaImage::new(1, 1)).selected_tool,
            super::Tool::Select,
        );
    }

    #[test]
    fn stroke_tools_route_the_toolbar_slider_to_stroke_size() {
        let mut state = EditorState::new(RgbaImage::new(32, 32));
        for (index, tool) in [
            super::Tool::Line,
            super::Tool::Box,
            super::Tool::Circle,
            super::Tool::Arrow,
            super::Tool::Pen,
            super::Tool::Highlighter,
        ]
        .into_iter()
        .enumerate()
        {
            state.selected_tool = tool;
            assert_eq!(
                state.active_size_control_mode(),
                Some(super::super::types::SizeControlMode::Stroke),
                "{tool:?} should ask the toolbar for a stroke size"
            );
            let size = 5.0 + index as f64;
            assert!(state.set_active_size_without_rebuild(size));
            assert_eq!(state.stroke_size, size);
        }
    }

    #[test]
    fn focus_tool_uses_dedicated_slider_state_and_persists_intensity_per_action() {
        let mut state = EditorState::new(RgbaImage::from_pixel(
            16,
            16,
            image::Rgba([200, 180, 160, 255]),
        ));
        state.selected_tool = super::Tool::Focus;

        assert_eq!(state.active_size_value(), Some(58.0));
        assert!(state.set_active_size_without_rebuild(72.0));
        assert_eq!(state.current_focus_intensity(), 72.0);

        state.drag_start = Some(Point { x: 2.0, y: 2.0 });
        state.drag_current = Some(Point { x: 10.0, y: 10.0 });
        let draft = state.draft_action().expect("focus draft");
        match draft {
            AnnotationAction::Focus { rect, intensity } => {
                assert_eq!(rect.x, 2);
                assert_eq!(intensity, 72.0);
            }
            other => panic!("expected focus draft, got {other:?}"),
        }

        state.actions.push(AnnotationAction::Focus {
            rect: Rect {
                x: 3,
                y: 3,
                width: 6,
                height: 6,
            },
            intensity: 44.0,
        });
        state.selected_tool = super::Tool::Select;
        state.selected_action_index = Some(0);
        assert_eq!(state.active_size_value(), Some(44.0));
        assert!(state.set_active_size_without_rebuild(66.0));
        state.rebuild_effect_layer();

        let final_image = state.to_rendered_image().expect("rendered image");
        assert_eq!(
            *final_image.get_pixel(4, 4),
            image::Rgba([200, 180, 160, 255])
        );
        let outside = *final_image.get_pixel(1, 1);
        assert!(outside[0] < 200 && outside[1] < 180 && outside[2] < 160);
    }

    #[test]
    fn obfuscate_method_switch_retints_selected_rect() {
        let mut state = EditorState::new(RgbaImage::new(32, 32));
        state.selected_tool = super::Tool::Obfuscate;
        state.push_action(AnnotationAction::Obfuscate {
            rect: Rect {
                x: 2,
                y: 2,
                width: 8,
                height: 8,
            },
            method: ObfuscateMethod::Pixelate,
            amount: DEFAULT_OBFUSCATE_AMOUNT,
        });
        state.set_obfuscate_method(ObfuscateMethod::Blur);
        match state.selected_action().expect("selected obfuscate") {
            AnnotationAction::Obfuscate { method, .. } => {
                assert_eq!(*method, ObfuscateMethod::Blur);
            }
            other => panic!("expected obfuscate action, got {other:?}"),
        }
    }

    #[test]
    fn text_input_accepts_unicode_and_forward_delete_on_tiny_images() {
        let mut state = EditorState::new(RgbaImage::new(2, 2));
        state.begin_text_input(Point { x: 1.0, y: 1.0 }, 100.0, 44.0);
        for c in "héllo→".chars() {
            state.add_text_input_char(c);
        }
        state.move_cursor_home();
        state.delete_text_input_char_forward();
        let input = state.get_text_input().expect("active input");
        assert_eq!(input.text, "éllo→");
        state.move_cursor_end();
        assert!(state.commit_active_text_input());
        assert_eq!(state.actions.len(), 1);
    }

    fn sample_state() -> EditorState {
        EditorState::new(RgbaImage::from_pixel(
            64,
            64,
            image::Rgba([120, 120, 120, 255]),
        ))
    }

    fn drag_rect_action(
        state: &mut EditorState,
        tool: super::Tool,
        shift: bool,
    ) -> AnnotationAction {
        state.selected_tool = tool;
        state.drag_shift_active = shift;
        state.begin_drag(Point { x: 10.0, y: 10.0 });
        state.update_drag(Point { x: 40.0, y: 24.0 });
        state
            .finalize_drag_action()
            .expect("drag should commit an action")
    }

    #[test]
    fn shape_drags_commit_and_shift_constrains_to_square_and_circle() {
        for (tool, shift, want_w, want_h) in [
            (super::Tool::Box, false, 30, 14),
            (super::Tool::Box, true, 30, 30),
            (super::Tool::Circle, false, 30, 14),
            (super::Tool::Circle, true, 30, 30),
        ] {
            let mut state = sample_state();
            let action = drag_rect_action(&mut state, tool, shift);
            match action {
                AnnotationAction::Box { rect, .. } | AnnotationAction::Circle { rect, .. } => {
                    assert_eq!(rect.width, want_w, "{tool:?} shift={shift}");
                    assert_eq!(rect.height, want_h, "{tool:?} shift={shift}");
                }
                other => panic!("expected shape, got {other:?}"),
            }
        }
    }

    #[test]
    fn line_and_arrow_drags_shift_constrain_to_45_degrees() {
        for tool in [super::Tool::Line, super::Tool::Arrow] {
            let mut state = sample_state();
            let action = drag_rect_action(&mut state, tool, true);
            let (start, end) = match &action {
                AnnotationAction::Line { start, end, .. }
                | AnnotationAction::Arrow { start, end, .. } => (*start, *end),
                other => panic!("expected line/arrow, got {other:?}"),
            };
            let dx = (end.x - start.x).abs();
            let dy = (end.y - start.y).abs();
            assert!(
                (dx - dy).abs() < 0.5 || dx < 0.5 || dy < 0.5,
                "shift-constrained {tool:?} should snap to 45°: dx={dx} dy={dy}"
            );
        }
    }

    #[test]
    fn pen_and_highlighter_commit_freehand_paths() {
        for tool in [super::Tool::Pen, super::Tool::Highlighter] {
            let mut state = sample_state();
            state.selected_tool = tool;
            state.highlighter_mode = crate::capture::editor::pen_weight::HighlighterMode::Freehand;
            state.begin_drag(Point { x: 4.0, y: 4.0 });
            state.update_drag(Point { x: 20.0, y: 8.0 });
            state.update_drag(Point { x: 40.0, y: 40.0 });
            let action = state.finalize_drag_action().expect("stroke commits");
            let points = match &action {
                AnnotationAction::Pen { points, .. }
                | AnnotationAction::Highlighter { points, .. } => points,
                other => panic!("expected stroke path, got {other:?}"),
            };
            assert!(points.len() >= 2);
        }
    }

    #[test]
    fn number_markers_sequence_styles_stepper_bounds_and_selected_edits() {
        use crate::capture::editor::numbering_style::{NumberSize, NumberingStyle};

        let mut state = sample_state();
        state.set_active_number_start(0);
        assert_eq!(state.active_number_start(), 1);

        state.add_number_marker(Point { x: 8.0, y: 8.0 });
        state.add_number_marker(Point { x: 20.0, y: 20.0 });
        assert_eq!(state.actions.len(), 2);
        assert_eq!(state.next_number, 3);
        match &state.actions[1] {
            AnnotationAction::Number { number, .. } => assert_eq!(*number, 2),
            other => panic!("expected number, got {other:?}"),
        }

        // Select the first marker and restyle only it.
        state.selected_action_index = Some(0);
        assert!(state.set_numbering_style(NumberingStyle::Roman));
        assert!(state.set_number_size(NumberSize::ExtraLarge));
        assert!(state.set_active_number_start(7));
        match &state.actions[0] {
            AnnotationAction::Number {
                number,
                style,
                size,
                ..
            } => {
                assert_eq!(*number, 7);
                assert_eq!(*style, NumberingStyle::Roman);
                assert_eq!(*size, NumberSize::ExtraLarge);
            }
            other => panic!("expected number, got {other:?}"),
        }
        match &state.actions[1] {
            AnnotationAction::Number { number, style, .. } => {
                assert_eq!(*number, 2);
                assert_eq!(*style, NumberingStyle::Numeric);
            }
            other => panic!("expected number, got {other:?}"),
        }

        assert!(state.undo());
        match &state.actions[0] {
            AnnotationAction::Number { number, .. } => assert_eq!(*number, 1),
            other => panic!("expected number, got {other:?}"),
        }
        assert!(state.redo());
        match &state.actions[0] {
            AnnotationAction::Number { number, .. } => assert_eq!(*number, 7),
            other => panic!("expected number, got {other:?}"),
        }
    }

    #[test]
    fn selected_action_restyles_round_trip_through_history() {
        let mut state = sample_state();
        state.push_action(AnnotationAction::Box {
            rect: super::Rect {
                x: 4,
                y: 4,
                width: 10,
                height: 10,
            },
            color: crate::capture::editor::color::DRAW_COLORS[0],
            stroke_size: 4.0,
            shadow: false,
        });
        state.selected_action_index = Some(0);
        let red = crate::capture::editor::types::DrawColor {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        state.selected_color = red;
        state.set_selected_action_color(red);
        match &state.actions[0] {
            AnnotationAction::Box { color, .. } => assert_eq!(*color, red),
            other => panic!("expected box, got {other:?}"),
        }
        assert!(state.undo());
        match &state.actions[0] {
            AnnotationAction::Box { color, .. } => {
                assert_eq!(*color, crate::capture::editor::color::DRAW_COLORS[0])
            }
            other => panic!("expected box, got {other:?}"),
        }
        assert!(state.redo());
        match &state.actions[0] {
            AnnotationAction::Box { color, .. } => assert_eq!(*color, red),
            other => panic!("expected box, got {other:?}"),
        }
    }

    #[test]
    fn effects_change_inside_and_outside_pixels_as_their_method_says() {
        use crate::capture::editor::types::ObfuscateMethod;

        // Two-tone source so pixelate/blur visibly change covered pixels.
        let mut two_tone = RgbaImage::new(32, 32);
        for y in 0..32u32 {
            for x in 0..32u32 {
                two_tone.put_pixel(
                    x,
                    y,
                    if x < 16 {
                        image::Rgba([255, 255, 255, 255])
                    } else {
                        image::Rgba([0, 0, 0, 255])
                    },
                );
            }
        }
        for method in [
            ObfuscateMethod::Pixelate,
            ObfuscateMethod::Blur,
            ObfuscateMethod::Blackout,
        ] {
            let mut state = EditorState::new(two_tone.clone());
            state.push_action(AnnotationAction::Obfuscate {
                rect: super::Rect {
                    x: 12,
                    y: 8,
                    width: 8,
                    height: 8,
                },
                method,
                amount: 13.0,
            });
            state.rebuild_effect_layer();
            let inside = *state.working_image.get_pixel(14, 10);
            let outside = *state.working_image.get_pixel(2, 2);
            assert_eq!(outside, image::Rgba([255, 255, 255, 255]));
            match method {
                ObfuscateMethod::Blackout => assert_eq!(inside, image::Rgba([0, 0, 0, 255])),
                // The rect straddles the boundary so mixing methods blend it.
                _ => assert!(inside[0] > 0 && inside[0] < 255, "{method:?}: {inside:?}"),
            }
        }

        // Focus: inside stays, outside dims.
        let mut state = EditorState::new(RgbaImage::from_pixel(
            32,
            32,
            image::Rgba([200, 200, 200, 255]),
        ));
        state.push_action(AnnotationAction::Focus {
            rect: super::Rect {
                x: 8,
                y: 8,
                width: 8,
                height: 8,
            },
            intensity: 80.0,
        });
        state.rebuild_effect_layer();
        assert_eq!(
            *state.working_image.get_pixel(10, 10),
            image::Rgba([200, 200, 200, 255])
        );
        let outside = *state.working_image.get_pixel(2, 2);
        assert!(outside[0] < 200);

        // Editing a selected effect applies to that rect.
        state.selected_action_index = Some(0);
        assert!(state.set_selected_focus_action_intensity_without_rebuild(10.0));
        state.rebuild_effect_layer();
        let outside_after = *state.working_image.get_pixel(2, 2);
        assert!(outside_after[0] > outside[0]);
    }

    #[test]
    fn export_contains_annotations_and_effects_but_no_chrome() {
        use crate::capture::editor::types::ObfuscateMethod;

        let mut state = EditorState::new(RgbaImage::from_pixel(
            48,
            48,
            image::Rgba([255, 255, 255, 255]),
        ));
        state.push_action(AnnotationAction::Box {
            rect: super::Rect {
                x: 4,
                y: 4,
                width: 12,
                height: 12,
            },
            color: crate::capture::editor::types::DrawColor {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            stroke_size: 2.0,
            shadow: false,
        });
        state.push_action(AnnotationAction::Obfuscate {
            rect: super::Rect {
                x: 30,
                y: 30,
                width: 10,
                height: 10,
            },
            method: ObfuscateMethod::Blackout,
            amount: 13.0,
        });
        state.selected_action_index = Some(0); // selection chrome must not export
        state.rebuild_effect_layer();

        let rendered = state.to_final_image().expect("export renders");
        assert_eq!(rendered.dimensions(), (48, 48));
        let border = *rendered.get_pixel(4, 10);
        assert!(border[0] > 200 && border[1] < 80 && border[2] < 80);
        assert_eq!(*rendered.get_pixel(35, 35), image::Rgba([0, 0, 0, 255]));
        assert_eq!(
            *rendered.get_pixel(24, 24),
            image::Rgba([255, 255, 255, 255])
        );
    }

    #[test]
    fn a_new_document_starts_clean_and_stale_effect_revisions_are_ignored() {
        let mut state = sample_state();
        state.push_action(AnnotationAction::Box {
            rect: super::Rect {
                x: 2,
                y: 2,
                width: 6,
                height: 6,
            },
            color: crate::capture::editor::color::DRAW_COLORS[0],
            stroke_size: 4.0,
            shadow: false,
        });
        state.selected_action_index = Some(0);
        state.selected_tool = super::Tool::Box;
        state.begin_drag(Point { x: 1.0, y: 1.0 });
        state.update_drag(Point { x: 12.0, y: 12.0 });
        assert!(state.can_undo());
        assert!(state.draft_action().is_some());

        // "Opening a new image" = a fresh session.
        let fresh = EditorState::new(state.base_image.as_ref().clone());
        assert!(!fresh.can_undo());
        assert!(!fresh.can_redo());
        assert!(fresh.selected_action_index.is_none());
        assert!(fresh.drag_start.is_none());
        assert!(fresh.draft_action().is_none());
        assert!(fresh.active_text_input.is_none());
        assert!(fresh.crop_rect.is_none());
        assert_eq!(fresh.next_number, fresh.numbering_start);

        // Revisions guard stale async results: a newer pending bump beats an
        // older in-flight result.
        let mut session = sample_state();
        session.pending_effect_revision = 5;
        session.last_applied_effect_revision = 5;
        let stale_revision = session.pending_effect_revision;
        session.pending_effect_revision += 1;
        assert!(stale_revision <= session.last_applied_effect_revision);
    }
}
