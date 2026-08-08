use super::super::color::clamp_text_size;
use super::super::render::layout_wrapped_text;
use super::super::selection::action_bounds_with_padding;
use super::super::types::{
    AnnotationAction, DrawColor, FontSettings, FontStyle, Point, Rect, TextAlignment,
    TextDecoration, TextEditBounds,
};
use super::{EditorState, TextInputState};

impl EditorState {
    fn existing_text_bounds(&self, skip_index: Option<usize>) -> Vec<Rect> {
        self.actions
            .iter()
            .enumerate()
            .filter_map(|(index, action)| {
                if Some(index) == skip_index || !matches!(action, AnnotationAction::Text { .. }) {
                    return None;
                }
                action_bounds_with_padding(action, 0.0)
            })
            .collect()
    }

    fn text_obstacle_limits(
        &self,
        bounds: &TextEditBounds,
        skip_index: Option<usize>,
    ) -> (f64, f64) {
        let image_width = self.base_image.width() as f64;
        let image_height = self.base_image.height() as f64;
        let mut right_limit = image_width - bounds.rect.x as f64;
        let mut bottom_limit = image_height - bounds.rect.y as f64;

        for obstacle in self.existing_text_bounds(skip_index) {
            let vertical_overlap = bounds.rect.y < obstacle.y + obstacle.height
                && bounds.rect.y + bounds.rect.height > obstacle.y;
            if vertical_overlap && obstacle.x >= bounds.rect.x {
                right_limit = right_limit.min((obstacle.x - bounds.rect.x).max(50) as f64);
            }

            let horizontal_overlap = bounds.rect.x < obstacle.x + obstacle.width
                && bounds.rect.x + bounds.rect.width > obstacle.x;
            if horizontal_overlap && obstacle.y >= bounds.rect.y {
                bottom_limit = bottom_limit.min((obstacle.y - bounds.rect.y).max(44) as f64);
            }
        }

        (right_limit.max(50.0), bottom_limit.max(44.0))
    }

    pub fn begin_text_input(&mut self, position: Point, width: f64, height: f64) {
        let image_width = self.base_image.width() as f64;
        let image_height = self.base_image.height() as f64;
        let baseline_y = position.y.clamp(self.text_size + 8.0, image_height - 8.0);
        let max_width = (image_width - position.x).max(50.0);
        let constrained_width = width.clamp(50.0, max_width);
        let max_height = (image_height - (baseline_y - self.text_size - 8.0)).max(44.0);
        let constrained_height = height.clamp(44.0, max_height);
        let top_left = Point {
            x: position.x.clamp(0.0, image_width - 50.0),
            y: (baseline_y - self.text_size - 8.0).clamp(0.0, image_height - constrained_height),
        };
        let bounds = TextEditBounds::new(top_left, constrained_width, constrained_height);
        self.active_text_bounds = Some(bounds);
        self.active_text_is_dragging = false;
        self.active_text_drag_handle = None;
        self.active_text_drag_start = None;
        self.active_text_drag_start_bounds = None;
        self.active_text_is_resizing = false;
        self.start_text_input();
    }

    pub fn start_text_input(&mut self) {
        self.active_text_input = Some(TextInputState {
            text: String::new(),
            cursor_position: 0,
            cursor_visible: true,
            cursor_blink_timer: 0,
            color: self.selected_color,
            background_color: None,
            editing_action_index: None,
        });
    }

    pub fn add_text_input_char(&mut self, c: char) {
        if let Some(ref mut state) = self.active_text_input {
            state.text.insert(state.cursor_position, c);
            state.cursor_position += 1;
            state.cursor_visible = true;
            state.cursor_blink_timer = 0;
        }
    }

    pub fn reset_text_cursor_blink(&mut self) {
        if let Some(ref mut state) = self.active_text_input {
            state.cursor_visible = true;
            state.cursor_blink_timer = 0;
        }
    }

    pub fn set_text_cursor_position(&mut self, position: usize) {
        if let Some(ref mut state) = self.active_text_input {
            state.cursor_position = position.min(state.text.chars().count());
            state.cursor_visible = true;
            state.cursor_blink_timer = 0;
        }
    }

    pub fn delete_text_input_char(&mut self) {
        if let Some(ref mut state) = self.active_text_input {
            if state.cursor_position > 0 {
                state.cursor_position -= 1;
                state.text.remove(state.cursor_position);
                state.cursor_blink_timer = 0;
            }
        }
    }

    pub fn move_cursor_left(&mut self) {
        if let Some(ref mut state) = self.active_text_input {
            if state.cursor_position > 0 {
                state.cursor_position -= 1;
                state.cursor_visible = true;
                state.cursor_blink_timer = 0;
            }
        }
    }

    pub fn move_cursor_right(&mut self) {
        if let Some(ref mut state) = self.active_text_input {
            if state.cursor_position < state.text.len() {
                state.cursor_position += 1;
                state.cursor_visible = true;
                state.cursor_blink_timer = 0;
            }
        }
    }

    pub fn tick_cursor_blink(&mut self) {
        if let Some(ref mut state) = self.active_text_input {
            state.cursor_blink_timer += 1;
            if state.cursor_blink_timer >= 1 {
                state.cursor_blink_timer = 0;
                state.cursor_visible = !state.cursor_visible;
            }
        }
    }

    pub fn commit_text_input(&mut self) -> Option<AnnotationAction> {
        if let Some(input_state) = self.active_text_input.take() {
            let trimmed_text = input_state.text.trim().to_string();
            let bounds = self.active_text_bounds.take();
            self.active_text_is_dragging = false;
            self.active_text_drag_handle = None;
            self.active_text_drag_start = None;
            self.active_text_drag_start_bounds = None;
            self.active_text_is_resizing = false;

            if let Some(index) = input_state.editing_action_index {
                if trimmed_text.is_empty() {
                    if index < self.actions.len()
                        && matches!(self.actions[index], AnnotationAction::Text { .. })
                    {
                        self.actions.remove(index);
                        self.selected_action_index = None;
                        self.select_drag_anchor = None;
                        self.select_resize_handle = None;
                        self.redo_actions.clear();
                    }
                    return None;
                }

                if let Some(b) = bounds {
                    let Some(AnnotationAction::Text {
                        position,
                        text,
                        color,
                        font,
                        max_width,
                        ..
                    }) = self.actions.get_mut(index)
                    else {
                        return None;
                    };
                    position.x = b.rect.x as f64;
                    position.y = (b.rect.y as f64 + self.text_size + 8.0).clamp(
                        self.text_size + 8.0,
                        (self.base_image.height() as f64 - self.text_size * 0.5)
                            .max(self.text_size + 8.0),
                    );
                    *text = trimmed_text;
                    *color = input_state.color;
                    font.family = self.text_font_family.clone();
                    font.size = self.text_size;
                    font.style = FontStyle::Normal;
                    font.decoration = TextDecoration::None;
                    font.alignment = TextAlignment::Left;
                    *max_width = Some(b.rect.width as f64);
                    self.selected_action_index = Some(index);
                    self.redo_actions.clear();
                }
                return None;
            }

            if trimmed_text.is_empty() {
                self.clear_text_edit_state();
                return None;
            }

            if let Some(b) = bounds {
                let position = Point {
                    x: b.rect.x as f64,
                    y: (b.rect.y as f64 + self.text_size + 8.0)
                        .clamp(self.text_size + 8.0, self.base_image.height() as f64 - 8.0),
                };
                let font = FontSettings {
                    family: self.text_font_family.clone(),
                    size: self.text_size,
                    style: FontStyle::Normal,
                    decoration: TextDecoration::None,
                    alignment: TextAlignment::Left,
                };
                let clamped_position = Point {
                    x: position.x.clamp(
                        0.0,
                        (self.base_image.width() as f64 - font.size * 1.8).max(0.0),
                    ),
                    y: position.y.clamp(
                        font.size,
                        (self.base_image.height() as f64 - font.size * 0.5).max(font.size),
                    ),
                };
                let clamped_width = (b.rect.width as f64).min(
                    (self.base_image.width() as f64 - clamped_position.x).max(font.size * 1.8),
                );
                return Some(AnnotationAction::Text {
                    position: clamped_position,
                    text: trimmed_text,
                    color: input_state.color,
                    font,
                    max_width: Some(clamped_width),
                    shadow: self.draw_object_shadow,
                    background_color: input_state.background_color,
                });
            }
        }
        None
    }

    pub fn cancel_text_input(&mut self) {
        self.active_text_input = None;
        self.clear_text_edit_state();
    }

    fn clear_text_edit_state(&mut self) {
        self.active_text_bounds = None;
        self.active_text_is_dragging = false;
        self.active_text_drag_handle = None;
        self.active_text_drag_start = None;
        self.active_text_drag_start_bounds = None;
        self.active_text_is_resizing = false;
    }

    pub fn get_text_input(&self) -> Option<&TextInputState> {
        self.active_text_input.as_ref()
    }

    #[cfg(test)]
    pub fn get_text_bounds(&self) -> Option<&TextEditBounds> {
        self.active_text_bounds.as_ref()
    }

    pub fn fit_active_text_to_layout_with_constraints(
        &mut self,
        preserve_width: bool,
        preserve_height: bool,
        preserve_font_size: bool,
    ) {
        let Some(input) = self.active_text_input.as_ref() else {
            return;
        };
        let Some(mut bounds) = self.active_text_bounds.clone() else {
            return;
        };

        let skip_index = input.editing_action_index;
        let text = input.text.clone();
        let family = self.text_font_family.clone();
        let surface = match gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1) {
            Ok(surface) => surface,
            Err(_) => return,
        };
        let context = match gtk4::cairo::Context::new(&surface) {
            Ok(context) => context,
            Err(_) => return,
        };

        let mut fitted_size = self.text_size;
        loop {
            let (available_width_limit, available_height_limit) =
                self.text_obstacle_limits(&bounds, skip_index);
            let available_height = if preserve_height {
                bounds.rect.height.max(1) as f64
            } else {
                available_height_limit
            };
            // When not preserving width, allow the box to grow up to the full
            // available space (image edge or next obstacle). This lets text
            // stay on one line and only wrap when it truly runs out of room.
            // When preserving width, cap at the current box width.
            let mut max_width = if preserve_width {
                (bounds.rect.width.max(1) as f64).min(available_width_limit)
            } else {
                available_width_limit
            };

            let measure = |size: f64, width: f64| {
                let font = FontSettings {
                    family: family.clone(),
                    size,
                    style: FontStyle::Normal,
                    decoration: TextDecoration::None,
                    alignment: TextAlignment::Left,
                };
                let content_width = (width - 20.0).max(font.size * 0.8);
                let layout = layout_wrapped_text(&context, &text, &font, content_width);
                let line_height = (font.size * 1.2).max(font.size + 4.0);
                // Include top+bottom padding and border inset so the box is
                // always tall enough that the bottom border never clips text.
                // border_inset mirrors TEXT_EDIT_BORDER_WIDTH/2 + 1 from render.rs
                let padding_y = 8.0;
                let border_inset = 2.0; // = TEXT_EDIT_BORDER_WIDTH / 2.0 + 1.0
                let text_block_height =
                    (layout.lines.len().max(1) as f64 - 1.0).max(0.0) * line_height + font.size;
                let height = (text_block_height + (padding_y + border_inset) * 2.0).max(44.0);
                (layout, height)
            };

            if !preserve_font_size {
                while fitted_size < 120.0 {
                    let next_size = (fitted_size + 1.0).min(120.0);
                    let (_, next_height) = measure(next_size, max_width);
                    if next_height > available_height {
                        break;
                    }
                    fitted_size = next_size;
                }
            }

            let (mut layout, mut height) = measure(fitted_size, max_width);
            if !preserve_font_size {
                while fitted_size > 10.0 && height > available_height {
                    fitted_size = (fitted_size - 1.0).max(10.0);
                    let measured = measure(fitted_size, max_width);
                    layout = measured.0;
                    height = measured.1;
                }
            }

            if preserve_font_size {
                if height > available_height {
                    let mut low = max_width;
                    let mut high = available_width_limit;
                    while high - low > 1.0 {
                        let mid = (low + high) / 2.0;
                        let measured = measure(fitted_size, mid);
                        if measured.1 > available_height {
                            low = mid;
                        } else {
                            high = mid;
                        }
                    }
                    max_width = high;
                    let measured = measure(fitted_size, max_width);
                    layout = measured.0;
                    height = measured.1;
                }

                // Live typing should prefer the current font size, but once
                // width is exhausted we must shrink instead of letting text
                // extend past the bottom image boundary.
                while fitted_size > 10.0 && height > available_height {
                    fitted_size = (fitted_size - 1.0).max(10.0);
                    let measured = measure(fitted_size, max_width);
                    layout = measured.0;
                    height = measured.1;
                }
            }

            let old_width = bounds.rect.width;
            let old_height = bounds.rect.height;
            let target_width = if preserve_width {
                // Preserving width: keep the current box width (capped at available).
                max_width.round().max(fitted_size * 1.8) as i32
            } else {
                // Not preserving width: size the box to the actual text width
                // (with padding), only growing as wide as the text needs.
                // Add padding_x * 2 to match draw_active_text_input's padding.
                let padding_x = 10.0;
                (layout.max_width + padding_x * 2.0)
                    .max(fitted_size * 1.8)
                    .min(max_width)
                    .round() as i32
            };
            let target_height = if preserve_height {
                bounds.rect.height
            } else {
                height.min(available_height.max(44.0)).round().max(1.0) as i32
            };
            bounds.rect.width = target_width;
            bounds.rect.height = target_height;
            bounds.sync_handles();

            if bounds.rect.width == old_width && bounds.rect.height == old_height {
                break;
            }
        }

        self.text_size = fitted_size;

        // Clamp so the box never overflows below the image.
        let image_height = self.base_image.height() as i32;
        if bounds.rect.y + bounds.rect.height > image_height {
            bounds.rect.height = (image_height - bounds.rect.y).max(44);
        }

        bounds.sync_handles();
        self.active_text_bounds = Some(bounds);
    }

    pub fn fit_active_text_to_layout_preserving_font_size(&mut self) {
        self.fit_active_text_to_layout_with_constraints(true, false, true);
    }

    pub fn fit_active_text_to_layout_preserving_box(&mut self) {
        self.fit_active_text_to_layout_with_constraints(true, true, false);
    }

    pub fn fit_active_text_to_layout(&mut self) {
        self.fit_active_text_to_layout_with_constraints(false, false, true);
    }

    /// Reflow only the box height to fit the current text at the current width
    /// and font size. Does NOT touch x, y, or width — safe to call during a
    /// Left/Right handle drag where the user is explicitly controlling width.
    pub fn fit_active_text_height_only(&mut self) {
        let Some(input) = self.active_text_input.as_ref() else {
            return;
        };
        let Some(mut bounds) = self.active_text_bounds.clone() else {
            return;
        };

        let text = input.text.clone();
        let family = self.text_font_family.clone();
        let size = self.text_size;

        let surface = match gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1) {
            Ok(s) => s,
            Err(_) => return,
        };
        let context = match gtk4::cairo::Context::new(&surface) {
            Ok(c) => c,
            Err(_) => return,
        };

        let font = FontSettings {
            family,
            size,
            style: FontStyle::Normal,
            decoration: TextDecoration::None,
            alignment: TextAlignment::Left,
        };
        let content_width = (bounds.rect.width as f64 - 20.0).max(font.size * 0.8);
        let layout = layout_wrapped_text(&context, &text, &font, content_width);
        let line_height = (font.size * 1.2).max(font.size + 4.0);
        let padding_y = 8.0;
        let border_inset = 2.0;
        let text_block_height =
            (layout.lines.len().max(1) as f64 - 1.0).max(0.0) * line_height + font.size;
        let new_height = (text_block_height + (padding_y + border_inset) * 2.0)
            .max(44.0)
            .round() as i32;

        // Only update height — x, y, width are untouched.
        bounds.rect.height = new_height;

        // Clamp so the box never overflows below the image.
        let image_height = self.base_image.height() as i32;
        if bounds.rect.y + bounds.rect.height > image_height {
            bounds.rect.height = (image_height - bounds.rect.y).max(44);
        }

        bounds.sync_handles();
        self.active_text_bounds = Some(bounds);
    }

    /// Like fit_active_text_height_only but reads text/font from the selected
    /// committed action instead of active_text_input. Used during circle-handle
    /// resizes of committed text actions (no active edit session open).
    pub fn fit_committed_text_height_only(&mut self) {
        let Some(mut bounds) = self.active_text_bounds.clone() else {
            return;
        };
        let Some(index) = self.selected_action_index else {
            return;
        };
        let (text, font) = match self.actions.get(index) {
            Some(AnnotationAction::Text { text, font, .. }) => (text.clone(), font.clone()),
            _ => return,
        };

        let surface = match gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1) {
            Ok(s) => s,
            Err(_) => return,
        };
        let context = match gtk4::cairo::Context::new(&surface) {
            Ok(c) => c,
            Err(_) => return,
        };

        let content_width = (bounds.rect.width as f64 - 20.0).max(font.size * 0.8);
        let layout = layout_wrapped_text(&context, &text, &font, content_width);
        let line_height = (font.size * 1.2).max(font.size + 4.0);
        let padding_y = 8.0;
        let border_inset = 2.0;
        let text_block_height =
            (layout.lines.len().max(1) as f64 - 1.0).max(0.0) * line_height + font.size;
        let new_height = (text_block_height + (padding_y + border_inset) * 2.0)
            .max(44.0)
            .round() as i32;

        bounds.rect.height = new_height;

        // Clamp so the box never overflows below the image.
        let image_height = self.base_image.height() as i32;
        if bounds.rect.y + bounds.rect.height > image_height {
            bounds.rect.height = (image_height - bounds.rect.y).max(44);
        }

        bounds.sync_handles();
        self.active_text_bounds = Some(bounds);
    }

    /// Compute the minimum box width needed to display the committed text
    /// action without any word being cut off. Returns the width of the longest
    /// single word (plus padding), or a font-size-based floor if no action.
    pub fn committed_text_min_width(&self) -> f64 {
        let Some(index) = self.selected_action_index else {
            return 50.0;
        };
        let (text, font) = match self.actions.get(index) {
            Some(AnnotationAction::Text { text, font, .. }) => (text.as_str(), font),
            _ => return 50.0,
        };

        let surface = match gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1) {
            Ok(s) => s,
            Err(_) => return 50.0,
        };
        let context = match gtk4::cairo::Context::new(&surface) {
            Ok(c) => c,
            Err(_) => return 50.0,
        };

        // Measure the width of each word; the widest word is the minimum.
        let padding_x = 10.0;
        let max_word_width = text
            .split_whitespace()
            .map(|word| super::super::render::measure_text_width(&context, word, font))
            .fold(0.0_f64, f64::max);

        // Add padding on both sides, floor at font_size * 1.8.
        (max_word_width + padding_x * 2.0)
            .max(font.size * 1.8)
            .max(50.0)
    }

    pub fn set_text_size(&mut self, size: f64) -> bool {
        let next = clamp_text_size(size);
        if let Some(index) = self
            .active_text_input
            .as_ref()
            .and_then(|input| input.editing_action_index)
        {
            let Some(AnnotationAction::Text { font, .. }) = self.actions.get_mut(index) else {
                return false;
            };
            if (font.size - next).abs() <= f64::EPSILON {
                return false;
            }
            font.size = next;
            self.text_size = next;
            self.redo_actions.clear();
            return true;
        }

        if self.active_text_input.is_some() {
            if (next - self.text_size).abs() <= f64::EPSILON {
                return false;
            }
            self.text_size = next;
            return true;
        }

        if self.selected_action_index.is_some() {
            if self.set_selected_text_action_size(next) {
                self.text_size = next;
                return true;
            }
            return false;
        }

        if (next - self.text_size).abs() <= f64::EPSILON {
            return false;
        }

        self.text_size = next;
        true
    }

    pub fn selected_text_action_size(&self) -> Option<f64> {
        let AnnotationAction::Text { font, .. } = self.selected_action()? else {
            return None;
        };

        Some(font.size)
    }

    pub fn set_selected_text_action_size(&mut self, size: f64) -> bool {
        let next = clamp_text_size(size);

        if let Some(index) = self
            .active_text_input
            .as_ref()
            .and_then(|input| input.editing_action_index)
        {
            let Some(AnnotationAction::Text { font, .. }) = self.actions.get_mut(index) else {
                return false;
            };
            if (font.size - next).abs() <= f64::EPSILON {
                return false;
            }
            font.size = next;
            self.redo_actions.clear();
            return true;
        }

        let Some(index) = self.selected_action_index else {
            return false;
        };

        let Some(action) = self.actions.get_mut(index) else {
            self.selected_action_index = None;
            return false;
        };

        let AnnotationAction::Text { font, .. } = action else {
            return false;
        };

        if (font.size - next).abs() <= f64::EPSILON {
            return false;
        }

        font.size = next;
        self.redo_actions.clear();
        true
    }

    pub fn selected_text_font_family(&self) -> Option<String> {
        let AnnotationAction::Text { font, .. } = self.selected_action()? else {
            return None;
        };

        Some(font.family.clone())
    }

    pub fn set_selected_text_font_family(&mut self, family: String) -> bool {
        if let Some(index) = self
            .active_text_input
            .as_ref()
            .and_then(|input| input.editing_action_index)
        {
            let Some(AnnotationAction::Text { font, .. }) = self.actions.get_mut(index) else {
                return false;
            };
            if font.family == family {
                return false;
            }
            font.family = family;
            self.redo_actions.clear();
            return true;
        }

        let Some(index) = self.selected_action_index else {
            return false;
        };

        let Some(action) = self.actions.get_mut(index) else {
            self.selected_action_index = None;
            return false;
        };

        let AnnotationAction::Text { font, .. } = action else {
            return false;
        };

        if font.family == family {
            return false;
        }

        font.family = family;
        self.redo_actions.clear();
        true
    }

    pub fn selected_text_action_data(
        &self,
    ) -> Option<(
        usize,
        String,
        DrawColor,
        FontSettings,
        Option<f64>,
        Point,
        Option<DrawColor>,
    )> {
        let index = self.selected_action_index?;
        let AnnotationAction::Text {
            position,
            text,
            color,
            font,
            max_width,
            background_color,
            ..
        } = self.actions.get(index)?
        else {
            return None;
        };

        Some((
            index,
            text.clone(),
            *color,
            font.clone(),
            *max_width,
            *position,
            *background_color,
        ))
    }

    pub fn commit_active_text_input(&mut self) -> bool {
        if let Some(action) = self.commit_text_input() {
            self.push_action(action);
            return true;
        }
        false
    }

    pub fn begin_editing_selected_text(&mut self) -> bool {
        let Some((index, text, color, font, max_width, position, background_color)) =
            self.selected_text_action_data()
        else {
            return false;
        };
        let Some(width) = max_width else {
            return false;
        };

        // Use the stored max_width as the box width directly.
        // Do NOT recompute from text_action_bounds() — that would shrink the box
        // to fit the text tightly, then commit_text_input() would write that
        // smaller width back, permanently changing the action's max_width.
        let padding_y = 8.0;
        let bounds_position = Point {
            x: position.x,
            y: position.y - font.size - padding_y,
        };
        let height = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1)
            .ok()
            .and_then(|surface| gtk4::cairo::Context::new(&surface).ok())
            .map(|context| {
                let content_width = (width - 20.0).max(font.size * 0.8);
                let layout = layout_wrapped_text(&context, &text, &font, content_width);
                let line_height = (font.size * 1.2).max(font.size + 4.0);
                (layout.lines.len().max(1) as f64 * line_height + font.size * 0.2 + padding_y * 2.0)
                    .max(44.0)
            })
            .unwrap_or_else(|| (font.size * 1.45 + 16.0).max(44.0));
        let bounds = TextEditBounds::new(bounds_position, width, height);
        self.active_text_bounds = Some(bounds);
        self.active_text_input = Some(TextInputState {
            cursor_position: text.chars().count(),
            text,
            cursor_visible: true,
            cursor_blink_timer: 0,
            color,
            background_color,
            editing_action_index: Some(index),
        });
        self.active_text_is_dragging = false;
        self.active_text_drag_handle = None;
        self.active_text_drag_start = None;
        self.text_font_family = font.family.clone();
        self.text_size = font.size;
        self.selected_color = color;
        true
    }

    #[cfg(test)]
    pub fn update_text_action(&mut self, index: usize, new_text: String) -> bool {
        if index >= self.actions.len() {
            return false;
        }

        let trimmed = new_text.trim().to_string();
        if trimmed.is_empty() {
            let removed = self.actions.remove(index);
            if !matches!(removed, AnnotationAction::Text { .. }) {
                self.actions.insert(index, removed);
                return false;
            }

            self.selected_action_index = None;
            self.select_drag_anchor = None;
            self.select_resize_handle = None;
            self.redo_actions.clear();
            return true;
        }

        let Some(AnnotationAction::Text { text, .. }) = self.actions.get_mut(index) else {
            return false;
        };

        if *text == trimmed {
            return false;
        }

        *text = trimmed;
        self.redo_actions.clear();
        true
    }

    pub fn cancel_text_edit(&mut self) {
        self.active_text_edit = None;
        self.active_text_entry = None;
        self.active_text_bounds = None;
        self.active_text_is_dragging = false;
        self.active_text_drag_handle = None;
        self.active_text_drag_start = None;
    }
}
