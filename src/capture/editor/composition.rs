use super::types::{BackgroundAlignment, BackgroundStyle, CropAspectRatio};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[allow(dead_code)]
impl FloatRect {
    pub fn width(&self) -> f64 {
        self.width
    }

    pub fn height(&self) -> f64 {
        self.height
    }

    pub fn y(&self) -> f64 {
        self.y
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowSpec {
    pub offset_x: f64,
    pub offset_y: f64,
    pub blur: f64,
    pub opacity: f64,
    pub rect: FloatRect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompositionLayout {
    pub canvas_width: f64,
    pub canvas_height: f64,
    pub image_rect: FloatRect,
    pub shadow_rect: Option<FloatRect>,
    pub shadow: Option<ShadowSpec>,
    pub draw_scale: f64,
    pub scale_factor: f64,
}

pub struct BackgroundComposition {
    screenshot_w: f64,
    screenshot_h: f64,
    style: BackgroundStyle,
    padding: f64,
    shadow: f64,
    insert: f64,
    alignment: BackgroundAlignment,
    corner_radius: f64,
    aspect_ratio: CropAspectRatio,
}

impl BackgroundComposition {
    pub fn new(screenshot_w: f64, screenshot_h: f64) -> Self {
        Self {
            screenshot_w,
            screenshot_h,
            style: BackgroundStyle::None,
            padding: 24.0,
            shadow: 15.0,
            insert: 0.0,
            alignment: BackgroundAlignment::Center,
            corner_radius: 18.0,
            aspect_ratio: CropAspectRatio::Original,
        }
    }

    pub fn with_style(mut self, style: BackgroundStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_padding(mut self, padding: f64) -> Self {
        self.padding = padding;
        self
    }

    pub fn with_shadow(mut self, shadow: f64) -> Self {
        self.shadow = shadow;
        self
    }

    pub fn with_insert(mut self, insert: f64) -> Self {
        self.insert = insert;
        self
    }

    pub fn with_alignment(mut self, alignment: BackgroundAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub fn with_corner_radius(mut self, corner_radius: f64) -> Self {
        self.corner_radius = corner_radius;
        self
    }

    pub fn with_aspect_ratio(mut self, aspect_ratio: CropAspectRatio) -> Self {
        self.aspect_ratio = aspect_ratio;
        self
    }

    pub fn compute(&self) -> CompositionLayout {
        let screenshot_w = self.screenshot_w.max(1.0);
        let screenshot_h = self.screenshot_h.max(1.0);
        let ref_size = screenshot_w.max(screenshot_h);
        let scale_factor = ref_size / 400.0;
        let padding_px = self.padding * scale_factor;

        let mut canvas_width = screenshot_w;
        let mut canvas_height = screenshot_h;
        let mut draw_scale = 1.0;

        if self.style != BackgroundStyle::None {
            canvas_width += padding_px * 2.0;
            canvas_height += padding_px * 2.0;

            if let Some(ratio) = self
                .aspect_ratio
                .aspect_ratio(canvas_width as i32, canvas_height as i32)
            {
                let current_ratio = canvas_width / canvas_height;
                if current_ratio < ratio {
                    canvas_width = canvas_height * ratio;
                } else {
                    canvas_height = canvas_width / ratio;
                }
            }

            draw_scale = 1.0 - self.insert / 200.0;
        }

        let draw_scale = draw_scale.clamp(0.01, 1.0);
        let draw_width = screenshot_w * draw_scale;
        let draw_height = screenshot_h * draw_scale;
        let available_w = (canvas_width - draw_width).max(0.0);
        let available_h = (canvas_height - draw_height).max(0.0);

        let (image_x, image_y) = match self.alignment {
            BackgroundAlignment::TopLeft => (0.0, 0.0),
            BackgroundAlignment::TopCenter => (available_w / 2.0, 0.0),
            BackgroundAlignment::TopRight => (available_w, 0.0),
            BackgroundAlignment::CenterLeft => (0.0, available_h / 2.0),
            BackgroundAlignment::Center => (available_w / 2.0, available_h / 2.0),
            BackgroundAlignment::CenterRight => (available_w, available_h / 2.0),
            BackgroundAlignment::BottomLeft => (0.0, available_h),
            BackgroundAlignment::BottomCenter => (available_w / 2.0, available_h),
            BackgroundAlignment::BottomRight => (available_w, available_h),
        };

        let image_rect = FloatRect {
            x: image_x,
            y: image_y,
            width: draw_width,
            height: draw_height,
        };

        let shadow = if self.style != BackgroundStyle::None && self.shadow > 0.0 {
            let shadow_strength = (self.shadow / 100.0).clamp(0.0, 1.0);
            let size_scale = (ref_size / 1200.0).sqrt().clamp(0.85, 1.8);
            let offset_x = 0.0;
            let offset_y = (6.0 + shadow_strength * 10.0) * size_scale * draw_scale;
            let blur = (16.0 + shadow_strength * 18.0) * size_scale * draw_scale;
            let opacity = 0.16 + shadow_strength * 0.12;
            let spread = blur * 1.2;
            let rect = FloatRect {
                x: image_rect.x + offset_x - spread,
                y: image_rect.y + offset_y - spread,
                width: image_rect.width + spread * 2.0,
                height: image_rect.height + spread * 2.0,
            };
            Some(ShadowSpec {
                offset_x,
                offset_y,
                blur,
                opacity,
                rect,
            })
        } else {
            None
        };

        let _ = self.corner_radius;

        CompositionLayout {
            canvas_width,
            canvas_height,
            image_rect,
            shadow_rect: shadow.map(|s| s.rect),
            shadow,
            draw_scale,
            scale_factor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::types::{BackgroundStyle, DrawColor};

    #[test]
    fn composition_keeps_background_canvas_for_tall_images() {
        let layout = BackgroundComposition::new(1200.0, 6400.0)
            .with_style(BackgroundStyle::PlainColor(DrawColor::new(
                1.0, 1.0, 1.0, 1.0,
            )))
            .with_padding(24.0)
            .with_shadow(20.0)
            .with_insert(0.0)
            .with_alignment(BackgroundAlignment::TopCenter)
            .with_corner_radius(18.0)
            .with_aspect_ratio(CropAspectRatio::Original)
            .compute();

        assert!(layout.canvas_width >= layout.image_rect.width());
        assert!(layout.canvas_height >= layout.image_rect.height());
        assert!(layout.image_rect.y() >= 0.0);
    }

    #[test]
    fn composition_shadow_bounds_extend_beyond_image_rect() {
        let layout = BackgroundComposition::new(1000.0, 800.0)
            .with_style(BackgroundStyle::PlainColor(DrawColor::new(
                0.0, 0.0, 0.0, 1.0,
            )))
            .with_padding(32.0)
            .with_shadow(40.0)
            .with_insert(0.0)
            .with_alignment(BackgroundAlignment::Center)
            .with_corner_radius(24.0)
            .compute();

        assert!(layout.shadow_rect.is_some());
        let shadow = layout.shadow_rect.unwrap();
        assert!(shadow.width() > layout.image_rect.width());
        assert!(shadow.height() > layout.image_rect.height());
    }

    #[test]
    fn composition_shadow_matches_soft_cards_profile() {
        let layout = BackgroundComposition::new(1200.0, 800.0)
            .with_style(BackgroundStyle::PlainColor(DrawColor::new(
                1.0, 1.0, 1.0, 1.0,
            )))
            .with_shadow(50.0)
            .with_insert(0.0)
            .with_alignment(BackgroundAlignment::Center)
            .compute();

        let shadow = layout.shadow.expect("shadow");
        assert!(shadow.offset_x.abs() <= 0.75);
        assert!(shadow.opacity < 0.24);
        assert!(shadow.blur > shadow.offset_y * 2.0);
    }
}
