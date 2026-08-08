//! Editor session state and annotation mutation API.
//!
//! Behavior is split across child modules; the struct and shared helpers stay here
//! so private field access remains inside the `state` module tree.

mod arrow;
mod crop;
mod drag_draw;
mod effects;
mod export;
mod history;
mod selection;
mod text_input;
mod tool_style;

use super::color::{
    DEFAULT_COLOR_INDEX, DEFAULT_FOCUS_INTENSITY, DEFAULT_OBFUSCATE_AMOUNT, DRAW_COLORS,
    STROKE_WIDTH, TEXT_SIZE,
};
use super::numbering_style::{NumberSize, NumberingStyle};
use super::pen_weight::{HighlighterMode, PenWeight};
use super::text_detect::{BackgroundTextDetection, TextDetector};
#[cfg(test)]
use super::types::SizeControlMode;
use super::types::{
    AnnotationAction, ArrowStyle, BackgroundAlignment, BackgroundStyle, CropAspectRatio, DrawColor,
    EditorError, MoveHandle, ObfuscateMethod, Point, Rect, TextEditBounds, Tool,
};
use gtk4;
use image::RgbaImage;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub struct EditorState {
    pub base_image: Arc<RgbaImage>,
    pub working_image: Arc<RgbaImage>,
    pub working_image_revision: u64,
    pub crop_selection: Option<Rect>,
    pub crop_aspect_ratio: CropAspectRatio,
    pub crop_background_color: DrawColor,
    pub crop_background_color_explicit: bool,
    pub actions: Vec<AnnotationAction>,
    pub redo_actions: Vec<AnnotationAction>,
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
    pub background_style: BackgroundStyle,
    pub background_padding: f64,
    pub background_shadow: f64,
    pub background_insert: f64,
    pub auto_balance: bool,
    pub background_alignment: BackgroundAlignment,
    pub background_corner_radius: f64,
    pub background_aspect_ratio: CropAspectRatio,
    pub active_text_drag_start_bounds: Option<Rect>,
    pub active_text_is_resizing: bool,
    pub hovered_text_action_index: Option<usize>,
    pub active_text_input: Option<TextInputState>,

    // Text detection for highlighter
    pub text_detector: Arc<Mutex<TextDetector>>,
    pub text_detection_ready: Arc<AtomicBool>,
    pub text_detection_handle: Option<BackgroundTextDetection>,

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
    pub fn new(base_image: RgbaImage) -> Self {
        let base_image = Arc::new(base_image);
        Self {
            working_image: Arc::clone(&base_image),
            base_image,
            working_image_revision: 1,
            crop_selection: None,
            crop_aspect_ratio: CropAspectRatio::Freeform,
            crop_background_color: DrawColor::new(1.0, 1.0, 1.0, 1.0),
            crop_background_color_explicit: false,
            actions: Vec::new(),
            redo_actions: Vec::new(),
            selected_tool: Tool::Background,
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
            background_style: BackgroundStyle::None,
            background_padding: 24.0,
            background_shadow: 15.0,
            background_insert: 0.0,
            auto_balance: false,
            background_alignment: BackgroundAlignment::Center,
            background_corner_radius: 18.0,
            background_aspect_ratio: CropAspectRatio::Original,
            active_text_drag_start_bounds: None,
            active_text_is_resizing: false,
            hovered_text_action_index: None,
            active_text_input: None,

            text_detector: Arc::new(Mutex::new(TextDetector::new_pending())),
            text_detection_ready: Arc::new(AtomicBool::new(false)),
            text_detection_handle: None,
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
        if self.selected_tool == Tool::Crop && tool != Tool::Crop {
            self.crop_selection = None;
        }
        if tool != Tool::Select {
            self.selected_action_index = None;
            self.select_drag_anchor = None;
            self.select_resize_handle = None;
        }
        if tool != Tool::Text {
            self.cancel_text_input();
            self.hovered_text_action_index = None;
        }
        if tool != Tool::Arrow {
            self.finalize_arrow_control_editing();
        }
        self.selected_tool = tool;
        self.clear_drag_without_rebuild_and_check_effect()
    }

    pub fn sync_next_number(&mut self) {
        let max_number = self
            .actions
            .iter()
            .filter_map(|action| match action {
                AnnotationAction::Number { number, style, .. } => {
                    // Only consider numbers with the same style
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
        // Use the user-specified starting number if no numbers exist yet
        self.next_number = if max_number == 0 {
            self.numbering_start
        } else {
            max_number.saturating_add(1)
        };
    }

    pub fn add_number_marker(&mut self, position: Point) {
        let number = self.next_number;
        let radius = self.number_size.radius();
        let image_width = self.working_image.width() as f64;
        let image_height = self.working_image.height() as f64;

        let clamped_x = if image_width <= radius * 2.0 {
            image_width / 2.0
        } else {
            position.x.clamp(radius, image_width - radius)
        };
        let clamped_y = if image_height <= radius * 2.0 {
            image_height / 2.0
        } else {
            position.y.clamp(radius, image_height - radius)
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
}

pub fn apply_effect_actions(image: &mut RgbaImage, actions: &[AnnotationAction]) {
    effects::apply_effect_actions(image, actions);
}

pub(crate) fn render_shadow_layer(
    width: u32,
    height: u32,
    blur: f64,
    opacity: f64,
    corner_radius: f64,
) -> Result<RgbaImage, EditorError> {
    export::render_shadow_layer(width, height, blur, opacity, corner_radius)
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use crate::capture::editor::color::DEFAULT_OBFUSCATE_AMOUNT;
    use crate::capture::editor::types::{AnnotationAction, ObfuscateMethod, Point, Rect};

    use super::EditorState;

    #[test]
    fn editor_state_defaults_to_background_tool() {
        assert!(
            matches!(EditorState::new(RgbaImage::new(1, 1)).selected_tool, super::Tool::Background),
            "Editor state should default to the Background tool so startup inspector width matches the initial tool surface",
        );
    }

    #[test]
    fn focus_tool_uses_dedicated_slider_state_and_persists_intensity_per_action() {
        let mut state = EditorState::new(RgbaImage::from_pixel(
            16,
            16,
            image::Rgba([200, 180, 160, 255]),
        ));
        state.selected_tool = super::Tool::Focus;

        assert_eq!(
            state.active_size_control_mode(),
            Some(super::SizeControlMode::Focus)
        );
        assert_eq!(state.active_size_value(), Some(58.0));

        assert!(state.set_active_size_without_rebuild(72.0));
        assert_eq!(state.current_focus_intensity(), 72.0);
        assert_eq!(state.active_size_value(), Some(72.0));

        state.drag_start = Some(Point { x: 2.0, y: 2.0 });
        state.drag_current = Some(Point { x: 10.0, y: 10.0 });
        let draft = state.draft_action().expect("focus draft");
        match draft {
            AnnotationAction::Focus { rect, intensity } => {
                assert_eq!(rect.x, 2);
                assert_eq!(rect.y, 2);
                assert_eq!(rect.width, 8);
                assert_eq!(rect.height, 8);
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

        assert_eq!(
            state.active_size_control_mode(),
            Some(super::SizeControlMode::Focus)
        );
        assert_eq!(state.active_size_value(), Some(44.0));
        assert!(state.set_active_size_without_rebuild(66.0));
        assert_eq!(state.selected_focus_action_intensity(), Some(66.0));
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
    fn obfuscate_blur_uses_single_shared_blur_method_and_slider_state() {
        let mut state = EditorState::new(RgbaImage::new(32, 32));
        state.set_obfuscate_method(ObfuscateMethod::Blur);

        assert_eq!(state.current_obfuscate_amount(), DEFAULT_OBFUSCATE_AMOUNT);
        assert_eq!(state.active_size_control_mode(), None);

        state.selected_tool = super::Tool::Obfuscate;
        assert_eq!(
            state.active_size_control_mode(),
            Some(super::SizeControlMode::Obfuscate)
        );
        assert_eq!(state.active_size_value(), Some(DEFAULT_OBFUSCATE_AMOUNT));

        assert!(state.set_active_size_without_rebuild(21.0));
        assert_eq!(state.current_obfuscate_amount(), 21.0);

        state.drag_start = Some(Point { x: 4.0, y: 5.0 });
        state.drag_current = Some(Point { x: 15.0, y: 18.0 });
        match state.draft_action().expect("obfuscate draft") {
            AnnotationAction::Obfuscate {
                rect,
                method,
                amount,
            } => {
                assert_eq!(rect.x, 4);
                assert_eq!(rect.y, 5);
                assert_eq!(rect.width, 11);
                assert_eq!(rect.height, 13);
                assert_eq!(method, ObfuscateMethod::Blur);
                assert_eq!(amount, 21.0);
            }
            other => panic!("expected obfuscate draft, got {other:?}"),
        }
    }
}
