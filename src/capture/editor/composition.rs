use super::types::{BackgroundAlignment, BackgroundStyle, CropAspectRatio, DrawColor, FrameStyle};
use crate::recording::editor::model::{MotionAppearance, MotionBackgroundFillType};

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

#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// Explicit numeric canvas aspect, taking precedence over `aspect_ratio`;
    /// `None` keeps the crop-ratio behaviour the static canvas has always had.
    canvas_aspect: Option<f64>,
    frame_style: FrameStyle,
    frame_border_thickness: f64,
    /// Explicit drop-shadow from the shared Appearance panel, in the same
    /// 400px reference units as `corner_radius`: (opacity, blur, offset_x,
    /// offset_y). When set it replaces the legacy single-strength `shadow`
    /// profile, so Static and Motion read the same controls.
    shadow_profile: Option<(f64, f64, f64, f64)>,
}

impl BackgroundComposition {
    pub fn new(screenshot_w: f64, screenshot_h: f64) -> Self {
        Self {
            screenshot_w,
            screenshot_h,
            style: BackgroundStyle::None,
            padding: 0.0,
            shadow: 15.0,
            insert: 0.0,
            alignment: BackgroundAlignment::Center,
            corner_radius: 18.0,
            aspect_ratio: CropAspectRatio::Original,
            canvas_aspect: None,
            frame_style: FrameStyle::Default,
            frame_border_thickness: 0.0,
            shadow_profile: None,
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

    /// Drive the drop shadow from the shared Appearance controls instead of the
    /// legacy strength scalar. Values are in the 400px reference space
    /// (opacity 0-1, blur and offsets in reference px).
    pub fn with_shadow_profile(
        mut self,
        opacity: f64,
        blur: f64,
        offset_x: f64,
        offset_y: f64,
    ) -> Self {
        self.shadow_profile = Some((opacity, blur, offset_x, offset_y));
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

    /// Expand the canvas to an exact numeric aspect instead of a
    /// `CropAspectRatio`, for ratios with no crop-ratio twin (Motion's Frame
    /// presets and manual Custom size).
    pub fn with_canvas_aspect(mut self, canvas_aspect: Option<f64>) -> Self {
        self.canvas_aspect = canvas_aspect.filter(|aspect| aspect.is_finite() && *aspect > 0.0);
        self
    }

    pub fn with_frame_style(mut self, frame_style: FrameStyle) -> Self {
        self.frame_style = frame_style;
        self
    }

    pub fn with_frame_border_thickness(mut self, thickness: f64) -> Self {
        self.frame_border_thickness = thickness;
        self
    }

    /// Canvas-px overhang of the frame (outside border, accent strokes, and
    /// backing sheets) beyond each side of the drawn image.
    fn frame_overhangs(
        &self,
        draw_width: f64,
        draw_height: f64,
        unit: f64,
    ) -> (f64, f64, f64, f64) {
        let spec = self.frame_style.spec();
        // Inset borders paint inside the image edge, so they overhang nothing.
        let mut uniform = if spec.inset_border {
            0.0
        } else {
            self.frame_border_thickness.max(0.0) * unit
        };
        for outer in [spec.outer1, spec.outer2].into_iter().flatten() {
            uniform += (outer.gap + outer.thickness) * unit;
        }
        let (mut left, mut top, mut right, mut bottom) = (uniform, uniform, uniform, uniform);
        for backing in [spec.backing1, spec.backing2].into_iter().flatten() {
            let theta = backing.rotation_deg.to_radians();
            let (sin, cos) = theta.sin_cos();
            // Fixed canvas px offsets: identical peek at any image size.
            let (corners, px, py) = if backing.center_pivot {
                // Diagonal fan about the sheet center (Stack).
                let hw = draw_width / 2.0;
                let hh = draw_height / 2.0;
                (
                    [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)],
                    hw + backing.offset_x,
                    hh + backing.offset_y,
                )
            } else {
                // Sheets pivot at their bottom-right corner (hidden behind
                // the card); corners relative to that pivot (Stack2).
                (
                    [
                        (-draw_width, -draw_height),
                        (0.0, -draw_height),
                        (0.0, 0.0),
                        (-draw_width, 0.0),
                    ],
                    draw_width + backing.offset_x,
                    draw_height + backing.offset_y,
                )
            };
            let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
            let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
            for (cx, cy) in corners {
                let qx = px + cx * cos - cy * sin;
                let qy = py + cx * sin + cy * cos;
                min_x = min_x.min(qx);
                min_y = min_y.min(qy);
                max_x = max_x.max(qx);
                max_y = max_y.max(qy);
            }
            // Bbox is relative to the image top-left (image spans 0..draw).
            left = left.max(-min_x);
            top = top.max(-min_y);
            right = right.max(max_x - draw_width);
            bottom = bottom.max(max_y - draw_height);
        }
        (left.max(0.0), top.max(0.0), right.max(0.0), bottom.max(0.0))
    }

    pub fn compute(&self) -> CompositionLayout {
        let screenshot_w = self.screenshot_w.max(1.0);
        let screenshot_h = self.screenshot_h.max(1.0);
        let ref_size = screenshot_w.max(screenshot_h);
        let scale_factor = ref_size / 400.0;
        // A fill is always given breathing room: with a Frame aspect forcing
        // a wider/taller canvas, padding 0 would glue the image to the fill
        // on the short axis. Stored padding is untouched (see the const).
        let padding_px = super::types::effective_background_padding(
            self.padding,
            self.style != BackgroundStyle::None,
        ) * scale_factor;

        let mut canvas_width = screenshot_w;
        let mut canvas_height = screenshot_h;
        let mut draw_scale = 1.0;

        if self.style != BackgroundStyle::None {
            canvas_width += padding_px * 2.0;
            canvas_height += padding_px * 2.0;

            if let Some(ratio) = self.canvas_aspect.or_else(|| {
                self.aspect_ratio
                    .aspect_ratio(canvas_width as i32, canvas_height as i32)
            }) {
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

        let mut image_rect = FloatRect {
            x: image_x,
            y: image_y,
            width: draw_width,
            height: draw_height,
        };

        // Appearance drives the shadow when it is set (Static and Motion share
        // the panel); otherwise the legacy single-strength scalar still applies.
        let params = if self.style == BackgroundStyle::None {
            None
        } else if let Some((opacity, blur, offset_x, offset_y)) = self.shadow_profile {
            (opacity > 0.0).then(|| {
                let unit = scale_factor * draw_scale;
                (
                    offset_x * unit,
                    offset_y * unit,
                    (blur * unit).max(0.0),
                    opacity.clamp(0.0, 1.0),
                )
            })
        } else if self.shadow > 0.0 {
            let shadow_strength = (self.shadow / 100.0).clamp(0.0, 1.0);
            let size_scale = (ref_size / 1200.0).sqrt().clamp(0.85, 1.8);
            Some((
                0.0,
                (6.0 + shadow_strength * 10.0) * size_scale * draw_scale,
                (16.0 + shadow_strength * 18.0) * size_scale * draw_scale,
                0.16 + shadow_strength * 0.12,
            ))
        } else {
            None
        };

        let mut shadow = params.map(|(offset_x, offset_y, blur, opacity)| {
            let spread = blur * 1.2;
            let rect = FloatRect {
                x: image_rect.x + offset_x - spread,
                y: image_rect.y + offset_y - spread,
                width: image_rect.width + spread * 2.0,
                height: image_rect.height + spread * 2.0,
            };
            ShadowSpec {
                offset_x,
                offset_y,
                blur,
                opacity,
                rect,
            }
        });

        // Contain the frame (outside border, accent strokes, backing sheets)
        // inside the canvas and center the whole framed stack per alignment,
        // so Stack-style sheets never spill past the edge and the margins stay
        // even around the stack instead of shoving the card down-right. This
        // also applies with no background so Retro-style windows stay visible
        // on a transparent canvas. With no frame this reduces to the exact
        // previous layout.
        {
            let unit = scale_factor * draw_scale;
            let (over_left, over_top, over_right, over_bottom) =
                self.frame_overhangs(draw_width, draw_height, unit);
            let content_width = draw_width + over_left + over_right;
            let content_height = draw_height + over_top + over_bottom;
            if content_width > canvas_width {
                canvas_width = content_width;
            }
            if content_height > canvas_height {
                canvas_height = content_height;
            }
            let avail_w = (canvas_width - content_width).max(0.0);
            let avail_h = (canvas_height - content_height).max(0.0);
            let (content_x, content_y) = match self.alignment {
                BackgroundAlignment::TopLeft => (0.0, 0.0),
                BackgroundAlignment::TopCenter => (avail_w / 2.0, 0.0),
                BackgroundAlignment::TopRight => (avail_w, 0.0),
                BackgroundAlignment::CenterLeft => (0.0, avail_h / 2.0),
                BackgroundAlignment::Center => (avail_w / 2.0, avail_h / 2.0),
                BackgroundAlignment::CenterRight => (avail_w, avail_h / 2.0),
                BackgroundAlignment::BottomLeft => (0.0, avail_h),
                BackgroundAlignment::BottomCenter => (avail_w / 2.0, avail_h),
                BackgroundAlignment::BottomRight => (avail_w, avail_h),
            };
            let new_image_x = content_x + over_left;
            let new_image_y = content_y + over_top;
            let shift_x = new_image_x - image_rect.x;
            let shift_y = new_image_y - image_rect.y;
            image_rect.x = new_image_x;
            image_rect.y = new_image_y;
            if let Some(shadow) = shadow.as_mut() {
                shadow.rect.x += shift_x;
                shadow.rect.y += shift_y;
            }
        }

        // The drop shadow is intentionally NOT contained here. Growing the
        // canvas to fit it moved the card (the canvas is centered in the
        // preview, so one-sided growth shifted everything) and made the exported
        // PNG change size as the blur slider moved. The shadow is painted by
        // `paint_card_shadow`, which clips it to the scene like the Motion
        // preview, so it can spill past the padding without moving the card.

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

/// Build Motion's card composition from the shared Appearance: same fill
/// rule, inset, alignment, frame style and border thickness as Static. Motion's
/// fill enum only decides whether a fill exists; the geometry never depends on
/// which fill, so no wallpaper or gradient is decoded here.
pub fn motion_background_composition(
    source_w: f64,
    source_h: f64,
    appearance: &MotionAppearance,
    canvas_aspect: Option<f64>,
) -> BackgroundComposition {
    let filled = appearance.background_fill_type != MotionBackgroundFillType::None;
    let style = if filled {
        BackgroundStyle::PlainColor(DrawColor::new(0.0, 0.0, 0.0, 1.0))
    } else {
        BackgroundStyle::None
    };
    BackgroundComposition::new(source_w, source_h)
        .with_style(style)
        .with_padding(appearance.background_padding)
        .with_insert(appearance.background_insert)
        .with_alignment(appearance.background_alignment)
        .with_corner_radius(appearance.border_radius)
        .with_canvas_aspect(canvas_aspect)
        .with_frame_style(appearance.frame_style)
        .with_frame_border_thickness(appearance.border_thickness)
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
    fn composition_grows_wallpaper_to_contain_stack2_sheets() {
        use crate::capture::editor::types::FrameStyle;
        // Small padding on purpose: the Stack2 sheets reach past it, so the
        // canvas must grow instead of letting sheets spill past the wallpaper.
        let plain = BackgroundComposition::new(200.0, 120.0)
            .with_style(BackgroundStyle::PlainColor(DrawColor::new(
                1.0, 1.0, 1.0, 1.0,
            )))
            .with_padding(10.0)
            .with_shadow(0.0)
            .with_insert(0.0)
            .with_alignment(BackgroundAlignment::Center)
            .with_corner_radius(0.0)
            .with_aspect_ratio(CropAspectRatio::Original)
            .compute();
        let stacked = BackgroundComposition::new(200.0, 120.0)
            .with_style(BackgroundStyle::PlainColor(DrawColor::new(
                1.0, 1.0, 1.0, 1.0,
            )))
            .with_padding(10.0)
            .with_shadow(0.0)
            .with_insert(0.0)
            .with_alignment(BackgroundAlignment::Center)
            .with_corner_radius(0.0)
            .with_aspect_ratio(CropAspectRatio::Original)
            .with_frame_style(FrameStyle::Stack2)
            .with_frame_border_thickness(0.0)
            .compute();
        assert!(
            stacked.canvas_width >= plain.canvas_width
                && stacked.canvas_height > plain.canvas_height,
            "expected grown canvas, plain {:?} vs stacked {:?}",
            (plain.canvas_width, plain.canvas_height),
            (stacked.canvas_width, stacked.canvas_height)
        );
        // Farthest sheet (BR-pivot rotated bbox) stays inside the wallpaper.
        let spec = FrameStyle::Stack2.spec();
        let backing = spec.backing1.expect("far sheet");
        let theta = backing.rotation_deg.to_radians();
        let (sin, cos) = theta.sin_cos();
        let w = stacked.image_rect.width;
        let h = stacked.image_rect.height;
        let px = stacked.image_rect.x + w + backing.offset_x;
        let py = stacked.image_rect.y + h + backing.offset_y;
        let (mut x0, mut y0) = (f64::INFINITY, f64::INFINITY);
        let (mut x1, mut y1) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (cx, cy) in [(-w, -h), (0.0, -h), (0.0, 0.0), (-w, 0.0)] {
            let qx = px + cx * cos - cy * sin;
            let qy = py + cx * sin + cy * cos;
            x0 = x0.min(qx);
            y0 = y0.min(qy);
            x1 = x1.max(qx);
            y1 = y1.max(qy);
        }
        assert!(x0 >= -0.5 && y0 >= -0.5);
        assert!(x1 <= stacked.canvas_width + 0.5);
        assert!(y1 <= stacked.canvas_height + 0.5);
    }

    /// The shared Appearance panel drives the drop shadow through the profile,
    /// in 400px reference units (like Border Radius), so Static matches Motion
    /// and scales with the screenshot instead of the legacy strength scalar.
    #[test]
    fn appearance_shadow_profile_drives_the_drop_shadow() {
        let layout = BackgroundComposition::new(800.0, 600.0)
            .with_style(BackgroundStyle::PlainColor(DrawColor::new(
                1.0, 1.0, 1.0, 1.0,
            )))
            .with_shadow_profile(0.6, 24.0, 12.0, -18.0)
            .compute();

        let shadow = layout.shadow.expect("shadow");
        assert!((shadow.opacity - 0.6).abs() < 1e-9);
        // 800px long edge -> scale_factor 2.0, so reference px double.
        assert!((shadow.offset_x - 24.0).abs() < 1e-6, "{}", shadow.offset_x);
        assert!((shadow.offset_y + 36.0).abs() < 1e-6, "{}", shadow.offset_y);
        assert!((shadow.blur - 48.0).abs() < 1e-6, "{}", shadow.blur);
    }

    /// The canvas must not grow to chase the shadow: the preview centers the
    /// canvas, so any one-sided growth shifts the card while the blur slider
    /// moves, and the exported PNG would change size with blur too.
    #[test]
    fn shadow_blur_does_not_move_the_card_or_resize_the_canvas() {
        let base = BackgroundComposition::new(800.0, 600.0)
            .with_style(BackgroundStyle::PlainColor(DrawColor::new(
                1.0, 1.0, 1.0, 1.0,
            )))
            .with_padding(24.0)
            .with_shadow_profile(0.8, 0.0, 0.0, 20.0)
            .compute();
        let blurred = BackgroundComposition::new(800.0, 600.0)
            .with_style(BackgroundStyle::PlainColor(DrawColor::new(
                1.0, 1.0, 1.0, 1.0,
            )))
            .with_padding(24.0)
            .with_shadow_profile(0.8, 40.0, 0.0, 20.0)
            .compute();

        assert_eq!(blurred.canvas_width, base.canvas_width);
        assert_eq!(blurred.canvas_height, base.canvas_height);
        assert_eq!(blurred.image_rect.x, base.image_rect.x);
        assert_eq!(blurred.image_rect.y, base.image_rect.y);
    }

    #[test]
    fn an_off_appearance_shadow_draws_nothing() {
        let layout = BackgroundComposition::new(800.0, 600.0)
            .with_style(BackgroundStyle::PlainColor(DrawColor::new(
                1.0, 1.0, 1.0, 1.0,
            )))
            .with_shadow(50.0)
            .with_shadow_profile(0.0, 24.0, 0.0, 20.0)
            .compute();

        assert!(
            layout.shadow.is_none(),
            "a zero-opacity Appearance shadow must override the legacy strength",
        );
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
