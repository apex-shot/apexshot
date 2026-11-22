//! Flattened document rendering for copy/save.
//!
//! The background, working image and vector annotations are flattened without
//! selection chrome, handles, drafts or the active text edit.

use super::super::types::{AnnotationAction, EditorError};
use super::super::upscale::SourceUpscale;
use super::EditorState;
use image::RgbaImage;
use std::sync::Arc;

impl EditorState {
    /// The document as the user sees it: base pixels with effect actions
    /// applied, then every other annotation painted on top. Renders the
    /// effect layer the preview currently shows.
    pub fn to_rendered_image(&self) -> Result<RgbaImage, EditorError> {
        self.render_from_working(&self.working_image, &self.background, true)
    }

    /// Flatten `base + actions` without consulting the live preview layer —
    /// used by export so a pending effect rebuild can never leak an
    /// unredacted frame. Neural upscaling enlarges the flattened pixels and
    /// draws the vectors at the same factor.
    pub fn to_export_image(&self) -> Result<RgbaImage, EditorError> {
        self.render_export()
    }

    fn render_export(&self) -> Result<RgbaImage, EditorError> {
        // A platform frame preset exports at its exact pixels: render into
        // that size directly so the resample happens through Cairo's filter
        // and the vectors are drawn at the exported resolution instead of
        // being flattened first and Lanczos-stretched afterwards.
        let target = self
            .background
            .frame_size
            .filter(|_| self.background.frame_aspect.is_some())
            .filter(|&(width, height)| width > 0 && height > 0);
        let base = self.export_base();
        let factor = self.source_upscale.factor();
        if factor == 1 {
            return self.render_scaled(&base, &self.background, true, self.export_scale, target);
        }
        if let Some(reason) = self.upscale_unavailable_reason(self.source_upscale) {
            return Err(EditorError::ImageSave(reason));
        }
        let enhanced = super::super::upscale::enhance_cached(&base, self.source_upscale)
            .map_err(EditorError::ImageSave)?;
        self.render_scaled(
            &enhanced,
            &self.background,
            true,
            self.export_scale * f64::from(factor),
            target,
        )
    }

    fn render_from_working(
        &self,
        working: &RgbaImage,
        appearance: &super::background::Appearance,
        transform: bool,
    ) -> Result<RgbaImage, EditorError> {
        self.render_scaled(working, appearance, transform, self.export_scale, None)
    }

    fn render_scaled(
        &self,
        working: &RgbaImage,
        appearance: &super::background::Appearance,
        transform: bool,
        annotation_scale: f64,
        output_size: Option<(u32, u32)>,
    ) -> Result<RgbaImage, EditorError> {
        let (image_width, image_height) = working.dimensions();
        if image_width == 0 || image_height == 0 {
            return Err(EditorError::ImageSave(
                "image has invalid dimensions".into(),
            ));
        }
        let layout = appearance.layout(image_width, image_height);
        let natural_size = (layout.width.ceil() as u32, layout.height.ceil() as u32);
        let (width, height) = output_size.unwrap_or(natural_size);
        // A tiny preset still pays for the full-size card and background
        // intermediates, so the layout's own size keeps its old limit too.
        if u64::from(width) * u64::from(height) > 100_000_000
            || u64::from(natural_size.0) * u64::from(natural_size.1) > 100_000_000
        {
            return Err(EditorError::ImageSave(
                "background canvas is too large".into(),
            ));
        }
        let stride = gtk4::cairo::Format::ARgb32
            .stride_for_width(width)
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;
        let mut surface = gtk4::cairo::ImageSurface::create(
            gtk4::cairo::Format::ARgb32,
            width as i32,
            height as i32,
        )
        .map_err(|e| EditorError::ImageSave(e.to_string()))?;

        {
            let context = gtk4::cairo::Context::new(&surface)
                .map_err(|e| EditorError::ImageSave(e.to_string()))?;
            // Everything below draws in layout units; the export size only
            // changes how many pixels that space lands on.
            let (sx, sy) = if output_size.is_some() {
                (
                    f64::from(width) / layout.width,
                    f64::from(height) / layout.height,
                )
            } else {
                (1.0, 1.0)
            };
            context.scale(sx, sy);
            if appearance.has_fill() {
                let background = appearance
                    .surface(layout, layout.width.max(layout.height))
                    .ok_or_else(|| EditorError::ImageSave("could not render background".into()))?;
                context.save().ok();
                context.scale(
                    layout.width / f64::from(background.width()),
                    layout.height / f64::from(background.height()),
                );
                context.set_source_surface(&background, 0.0, 0.0).ok();
                context.source().set_extend(gtk4::cairo::Extend::Pad);
                context.paint().ok();
                context.restore().ok();
            }
            if transform {
                if let Some(shadow) = super::super::shadow_render::geometry(
                    layout,
                    f64::from(image_width),
                    f64::from(image_height),
                    self.static_image_transform,
                    &self.scene_shadow,
                ) {
                    super::super::render::scene_shadow::paint_shadow(&context, shadow, false);
                }
            }
            let mut style = super::super::style_render::StyleGeometry::new(
                layout,
                f64::from(image_width),
                f64::from(image_height),
                if transform {
                    self.static_image_transform
                } else {
                    Default::default()
                },
                appearance.frame_style,
            );
            style.over_fill = appearance.has_fill();
            style.paint_backings(&context);
            style.paint_underlay(&context);
            if transform && !self.static_image_transform.is_identity() {
                // Upscaled exports rasterize the card at the output scale so
                // its vectors stay sharp; downscales keep the full working
                // resolution and resample through the projection's filter.
                let card = self.card_surface_scaled(
                    working,
                    layout.radius,
                    annotation_scale,
                    (sx.max(1.0), sy.max(1.0)),
                )?;
                super::super::transform_render::paint_card(
                    &context,
                    &card,
                    layout,
                    f64::from(image_width),
                    f64::from(image_height),
                    self.static_image_transform,
                    super::super::transform_render::Quality::Export,
                );
                style.paint_frame(&context);
            } else {
                let source = super::super::render::rgba_image_to_surface(working)
                    .ok_or_else(|| EditorError::ImageSave("could not render image".into()))?;
                context.save().ok();
                super::super::render::rounded_rect_path(
                    &context,
                    layout.image_x,
                    layout.image_y,
                    f64::from(image_width),
                    f64::from(image_height),
                    layout.radius,
                );
                context.clip();
                context
                    .set_source_surface(&source, layout.image_x, layout.image_y)
                    .ok();
                if sx != 1.0 || sy != 1.0 {
                    context.source().set_filter(gtk4::cairo::Filter::Good);
                }
                context.paint().ok();
                context.restore().ok();
                style.paint_frame(&context);
                context.translate(layout.image_x, layout.image_y);
                context.scale(annotation_scale, annotation_scale);
                for action in self.vector_annotation_actions() {
                    super::super::render::draw_annotation_action(&context, action);
                }
            }
        }

        surface.flush();
        let surface_data = surface
            .data()
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;
        Ok(super::super::render::cairo_argb_to_rgba_image(
            width,
            height,
            stride as usize,
            surface_data.as_ref(),
        ))
    }

    /// Whether the appearance uses a Studio presentation control: a frame
    /// style, or a scene shadow the layout leaves visible around the image.
    /// The Background panel (fill, padding, blur, radius, noise) and frame
    /// ratios are free.
    pub(crate) fn uses_studio_style(&self) -> bool {
        let background = &self.background;
        if background.frame_style != super::super::types::FrameStyle::Default {
            return true;
        }
        let (width, height) = self.working_image.dimensions();
        let layout = background.layout(width, height);
        (layout.image_x > 0.0 || layout.image_y > 0.0)
            && super::super::shadow_render::geometry(
                layout,
                f64::from(width),
                f64::from(height),
                self.static_image_transform,
                &self.scene_shadow,
            )
            .is_some()
    }

    pub(crate) fn studio_capabilities(&self) -> crate::studio_entitlement::StudioCapabilities {
        crate::studio_entitlement::StudioCapabilities {
            image_upscale: self.source_upscale.factor() > 1,
            ..crate::studio_entitlement::capabilities_for_image_style(
                self.static_image_transform,
                self.uses_studio_style(),
            )
        }
    }

    /// Base pixels with the effect actions applied, the image export draws.
    pub(crate) fn export_base(&self) -> RgbaImage {
        Self::flatten_effects(&self.base_image, &self.actions)
    }

    /// `export_base` for a worker: the pixels and actions it is built from.
    pub(crate) fn export_source(&self) -> (Arc<RgbaImage>, Vec<AnnotationAction>) {
        (Arc::clone(&self.base_image), self.actions.clone())
    }

    pub(crate) fn flatten_effects(base: &RgbaImage, actions: &[AnnotationAction]) -> RgbaImage {
        let mut flattened = base.clone();
        super::effects::apply_effect_actions(&mut flattened, actions);
        flattened
    }

    /// The exported canvas size when the source is enlarged by `mode`.
    pub(crate) fn upscale_output_dimensions(&self, mode: SourceUpscale) -> (u32, u32) {
        let (width, height) = self.working_image.dimensions();
        let factor = mode.factor();
        let layout = self
            .background
            .layout(width.saturating_mul(factor), height.saturating_mul(factor));
        (layout.width.ceil() as u32, layout.height.ceil() as u32)
    }

    /// The pixels the export file will have under `mode`: a platform frame
    /// preset's fixed size, else the composed canvas.
    pub(crate) fn export_dimensions(&self, mode: SourceUpscale) -> (u32, u32) {
        match self.background.frame_size {
            Some(size) if self.background.frame_aspect.is_some() => size,
            _ => self.upscale_output_dimensions(mode),
        }
    }

    /// Why `mode` cannot enhance this document, or `None` when it can. The
    /// message is an untranslated msgid.
    pub(crate) fn upscale_unavailable_reason(&self, mode: SourceUpscale) -> Option<String> {
        if mode == SourceUpscale::Off {
            return None;
        }
        let (width, height) = self.working_image.dimensions();
        if let Err(reason) = super::super::upscale::validate_input_dimensions(width, height, mode) {
            return Some(reason);
        }
        let (width, height) = self.upscale_output_dimensions(mode);
        if u64::from(width) * u64::from(height) > 16_777_216 || width.max(height) > 16_384 {
            return Some("The enhanced canvas exceeds the 16-megapixel output limit. Reduce upscaling or background padding.".into());
        }
        None
    }

    /// The flattened export pixels when neural enhancement still has to run
    /// for this export, or `None` when the export can render right away.
    pub(crate) fn pending_upscale(&self) -> Option<(SourceUpscale, RgbaImage)> {
        let mode = self.source_upscale;
        if mode == SourceUpscale::Off || self.upscale_unavailable_reason(mode).is_some() {
            return None;
        }
        let base = self.export_base();
        super::super::upscale::cached(&base, mode)
            .is_none()
            .then_some((mode, base))
    }

    /// The final export image. Always flattens `base + actions` so a pending
    /// preview rebuild cannot leak an unredacted frame.
    pub fn to_final_image(&self) -> Result<RgbaImage, EditorError> {
        crate::studio_entitlement::ensure_export_allowed(self.studio_capabilities())
            .map_err(|error| EditorError::ImageSave(error.to_string()))?;
        self.to_export_image()
    }

    /// Background-free card: the screenshot (effects baked) plus vector
    /// annotations, the same shape the Linux editor hands Motion.
    pub fn to_motion_card_image(&self) -> Result<RgbaImage, EditorError> {
        self.render_from_working(
            &self.working_image,
            &super::background::Appearance::default(),
            false,
        )
    }

    /// The transformed card's pixels: `raster_scale` enlarges the
    /// intermediate surface past the working resolution when the export
    /// scales up, so the vectors it paints are not upscaled a second time.
    fn card_surface_scaled(
        &self,
        working: &RgbaImage,
        radius: f64,
        annotation_scale: f64,
        raster_scale: (f64, f64),
    ) -> Result<gtk4::cairo::ImageSurface, EditorError> {
        let source = super::super::render::rgba_image_to_surface(working)
            .ok_or_else(|| EditorError::ImageSave("could not render image".into()))?;
        self.card_surface_from_source_scaled(
            &source,
            radius,
            (f64::from(working.width()) * raster_scale.0).ceil() as u32,
            (f64::from(working.height()) * raster_scale.1).ceil() as u32,
            annotation_scale,
        )
    }

    pub(crate) fn card_surface_from_source(
        &self,
        source: &gtk4::cairo::ImageSurface,
        radius: f64,
        width: u32,
        height: u32,
    ) -> Result<gtk4::cairo::ImageSurface, EditorError> {
        self.card_surface_from_source_scaled(source, radius, width, height, self.export_scale)
    }

    fn card_surface_from_source_scaled(
        &self,
        source: &gtk4::cairo::ImageSurface,
        radius: f64,
        width: u32,
        height: u32,
        annotation_scale: f64,
    ) -> Result<gtk4::cairo::ImageSurface, EditorError> {
        use gtk4::cairo::{Context, Filter, Format, ImageSurface};
        let surface = ImageSurface::create(Format::ARgb32, width as i32, height as i32)
            .map_err(|error| EditorError::ImageSave(error.to_string()))?;
        let context =
            Context::new(&surface).map_err(|error| EditorError::ImageSave(error.to_string()))?;
        let source_width = f64::from(source.width());
        let source_height = f64::from(source.height());
        context.scale(
            f64::from(width) / source_width,
            f64::from(height) / source_height,
        );
        super::super::render::rounded_rect_path(
            &context,
            0.0,
            0.0,
            source_width,
            source_height,
            radius,
        );
        context.clip();
        context.set_source_surface(source, 0.0, 0.0).ok();
        context.source().set_filter(Filter::Good);
        context.paint().ok();
        context.scale(annotation_scale, annotation_scale);
        for action in self.vector_annotation_actions() {
            super::super::render::draw_annotation_action(&context, action);
        }
        drop(context);
        Ok(surface)
    }

    fn vector_annotation_actions(&self) -> impl Iterator<Item = &AnnotationAction> {
        self.actions.iter().filter(|action| {
            !matches!(
                action,
                AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. }
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::sync::Arc;

    use image::{Rgba, RgbaImage};

    use super::super::background::{Fill, FrameRatio};
    use super::EditorState;
    use crate::capture::editor::types::FrameStyle;
    use crate::capture::editor::upscale::SourceUpscale;
    use crate::recording::editor::model::MotionSceneShadowPreset;

    fn document() -> EditorState {
        EditorState::new(RgbaImage::from_pixel(100, 80, Rgba([240, 0, 0, 255])))
    }

    /// An inset stroke meets transparency at the card edge without carrying the
    /// picture into that rim. The stroke used to end exactly on the clip
    /// outline, so the two anti-aliased rims shared their coverage and the
    /// picture tinted the stroke's outer edge, most visibly around the corners.
    #[test]
    fn an_inset_stroke_meets_transparency_without_the_picture() {
        let source = RgbaImage::from_pixel(800, 600, Rgba([255, 0, 0, 255]));
        for radius in [20.0, 40.0] {
            let mut state = EditorState::new(source.clone());
            state.background.fill = Fill::None;
            state.background.radius = radius;
            state.set_frame_style(FrameStyle::InsetDark);
            let image = state.to_export_image().unwrap();
            let (width, _) = image.dimensions();
            let rim = (0..width.min(160))
                .map(|step| image.get_pixel(step, step))
                .find(|pixel| pixel[3] > 0)
                .map(|pixel| pixel.0)
                .expect("the stroke reaches the corner");
            assert_eq!(
                rim[0], 0,
                "the stroke's outer rim carries the picture at radius {radius}: {rim:?}"
            );
        }
    }

    #[test]
    fn every_frame_style_changes_the_export_and_preserves_the_card() {
        let mut state = EditorState::new(RgbaImage::from_pixel(160, 120, Rgba([240, 0, 0, 255])));
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([130, 180, 220, 255]),
        )));
        state.background.padding = 120.0;
        let clean = state.to_export_image().unwrap();
        let card = state.to_motion_card_image().unwrap();
        for style in FrameStyle::ALL.into_iter().skip(1) {
            assert!(state.set_frame_style(style));
            let output = state.to_export_image().unwrap();
            assert_ne!(output, clean, "{style:?} should visibly change the export");
            assert_eq!(state.to_motion_card_image().unwrap(), card);
        }
        state.static_image_transform.yaw = 16.0;
        let transformed = state.to_export_image().unwrap();
        state.set_frame_style(FrameStyle::Default);
        assert_ne!(state.to_export_image().unwrap(), transformed);
    }

    #[test]
    fn attached_frames_leave_no_background_sliver_at_rounded_corners() {
        let mut state = EditorState::new(RgbaImage::from_pixel(
            1600,
            1200,
            Rgba([255, 255, 255, 255]),
        ));
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([0, 0, 255, 255]),
        )));
        state.background.padding = 120.0;
        state.background.radius = 70.0;
        for style in [FrameStyle::Border, FrameStyle::Retro, FrameStyle::Card] {
            state.set_frame_style(style);
            let layout = state.background.layout(1600, 1200);
            let image = state.to_export_image().unwrap();
            let r = layout.radius;
            for angle in [-1.35_f64, -1.1, -0.8, -0.5, -0.25] {
                let (sin, cos) = angle.sin_cos();
                let point = (
                    layout.image_x + 1600.0 - r + r * cos.sqrt(),
                    layout.image_y + r - r * (-sin).sqrt(),
                );
                let tangent = (-sin / cos.sqrt(), cos / (-sin).sqrt());
                let length = tangent.0.hypot(tangent.1);
                let normal = (tangent.1 / length, -tangent.0 / length);
                let center = (layout.image_x + 800.0, layout.image_y + 600.0);
                for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                    let x = (center.0 + sx * (point.0 - center.0 + normal.0 * 1.2)).round() as u32;
                    let y = (center.1 + sy * (point.1 - center.1 + normal.1 * 1.2)).round() as u32;
                    let pixel = image.get_pixel(x, y);
                    assert!(
                        pixel[2] < 220,
                        "{style:?} exposes blue at angle {angle}, corner ({sx}, {sy}): {pixel:?}"
                    );
                }
            }
        }
    }

    /// The Offset preset's sheet is a light card, not a black slab: on a
    /// transparent export the old opaque black sheet read as a hole under the
    /// card, which is exactly where nothing could lighten it.
    #[test]
    fn the_offset_sheet_is_a_light_card() {
        let mut state =
            EditorState::new(RgbaImage::from_pixel(400, 300, Rgba([255, 255, 255, 255])));
        state.background.fill = Fill::None;
        state.background.radius = 0.0;
        state.set_frame_style(FrameStyle::Retro);
        let image = state.to_export_image().unwrap();
        let (width, height) = image.dimensions();
        let sheet = FrameStyle::Retro
            .spec()
            .backing1
            .expect("offset sheet")
            .color;
        let channel = |value: f64| (value * 255.0).round() as u8;
        // The sheet peeks 28px below the card, past the 3px border.
        let sample = image.get_pixel(width / 2, height - 10);
        assert_eq!(sample[3], 255, "the sheet is opaque: {sample:?}");
        assert!(
            sample[0] > 150 && sample[1] > 150 && sample[2] > 150,
            "the sheet is a light card, not a black slab: {sample:?}"
        );
        assert!(
            sample[0].abs_diff(channel(sheet.r)) <= 2
                && sample[1].abs_diff(channel(sheet.g)) <= 2
                && sample[2].abs_diff(channel(sheet.b)) <= 2,
            "the sheet is the backing colour: {sample:?}"
        );
    }

    #[test]
    fn pearl_is_visible_on_a_white_image() {
        let mut state =
            EditorState::new(RgbaImage::from_pixel(400, 300, Rgba([255, 255, 255, 255])));
        let clean = state.to_export_image().unwrap();
        state.set_frame_style(FrameStyle::InsetLight);
        let pearl = state.to_export_image().unwrap();
        assert_eq!(clean.dimensions(), pearl.dimensions());
        assert!(
            pearl.pixels().any(|pixel| pixel[0] < 235),
            "Pearl needs a visible inset even on white"
        );
    }

    #[test]
    fn frame_styles_expand_transparent_canvas_and_restore_with_undo() {
        let mut state = document();
        let original = state.background.layout(100, 80);
        assert!(state.set_frame_style(FrameStyle::Retro));
        let layout = state.background.layout(100, 80);
        assert!(layout.width > original.width && layout.height > original.height);
        let output = state.to_export_image().unwrap();
        assert_eq!(
            output.dimensions(),
            (layout.width as u32, layout.height as u32)
        );
        assert!(state.undo_without_rebuild());
        assert_eq!(state.background.frame_style, FrameStyle::Default);
        assert_eq!(state.background.layout(100, 80).width, original.width);
        assert!(state.redo_without_rebuild());
        assert_eq!(state.background.frame_style, FrameStyle::Retro);
    }

    #[test]
    fn glass_style_radius_change_undoes_as_one_edit() {
        let mut state = document();
        assert_eq!(state.background.radius, 0.0);
        assert!(state.set_frame_style(FrameStyle::GlassLight));
        assert_eq!(state.background.radius, 20.0);
        assert!(state.undo_without_rebuild());
        assert_eq!(state.background.radius, 0.0);
        assert_eq!(state.background.frame_style, FrameStyle::Default);
        assert!(state.redo_without_rebuild());
        assert_eq!(state.background.radius, 20.0);
        assert_eq!(state.background.frame_style, FrameStyle::GlassLight);
        state.background.radius = 30.0;
        assert!(state.set_frame_style(FrameStyle::GlassDark));
        assert_eq!(state.background.radius, 30.0);
        state.background.radius = 35.0;
        assert!(state.undo_without_rebuild());
        assert_eq!(state.background.radius, 35.0);
        assert!(state.undo_without_rebuild());
        assert_eq!(state.background.radius, 35.0);
    }

    #[test]
    fn shadow_preset_opacity_light_and_pose_reach_export() {
        let _trial = crate::studio_trial::test_trial_used_up();
        let mut state = EditorState::new(RgbaImage::from_pixel(160, 120, Rgba([240, 0, 0, 255])));
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([240, 240, 240, 255]),
        )));
        state.background.padding = 100.0;
        let baseline = state.to_export_image().unwrap();
        let card = state.to_motion_card_image().unwrap();

        for preset in [
            MotionSceneShadowPreset::Spread,
            MotionSceneShadowPreset::Hug,
            MotionSceneShadowPreset::Adaptive,
        ] {
            state.scene_shadow.preset = preset;
            let output = state.to_export_image().unwrap();
            assert_ne!(output, baseline, "{preset:?} must affect the background");
            assert_eq!(output.get_pixel(120, 100), baseline.get_pixel(120, 100));
        }
        state.scene_shadow.preset = MotionSceneShadowPreset::Spread;
        state.scene_shadow.opacity = 0.0;
        assert_eq!(state.to_export_image().unwrap(), baseline);
        state.scene_shadow.opacity = 1.0;
        state.scene_shadow.light = (-1.0, 0.0);
        let right_cast = state.to_export_image().unwrap();
        state.scene_shadow.light = (1.0, 0.0);
        let left_cast = state.to_export_image().unwrap();
        assert!(right_cast.get_pixel(205, 100)[0] < left_cast.get_pixel(205, 100)[0]);
        assert_eq!(state.to_motion_card_image().unwrap(), card);
        assert!(
            state.to_final_image().is_err(),
            "a visible scene shadow is a Studio style"
        );
        state.static_image_transform.yaw = 18.0;
        let transformed = state.to_export_image().unwrap();
        assert_ne!(transformed, left_cast);
        assert_eq!(transformed.dimensions(), baseline.dimensions());
    }

    #[test]
    fn shadow_history_groups_light_drag_and_keeps_other_edits() {
        let mut state = document();
        let initial = state.scene_shadow.clone();
        let mut shadow = initial.clone();
        shadow.preset = MotionSceneShadowPreset::Hug;
        assert!(state.set_scene_shadow(shadow.clone()));
        state.begin_history_interaction();
        shadow.opacity = 0.5;
        shadow.light = (-0.4, 0.2);
        state.set_scene_shadow(shadow.clone());
        shadow.light = (-0.8, 0.7);
        state.set_scene_shadow(shadow.clone());
        state.finish_history_interaction();
        assert!(state.undo_without_rebuild());
        assert_eq!(state.scene_shadow.preset, MotionSceneShadowPreset::Hug);
        assert_eq!(state.scene_shadow.light, initial.light);
        assert!(state.undo_without_rebuild());
        assert_eq!(state.scene_shadow, initial);
        assert!(state.redo_without_rebuild());
        assert!(state.redo_without_rebuild());
        assert_eq!(state.scene_shadow, shadow);
        assert!(!state.set_scene_shadow(shadow));
    }

    #[test]
    fn transformed_pose_moves_card_without_moving_background() {
        let mut state = document();
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            20,
            20,
            Rgba([0, 0, 255, 255]),
        )));
        state.background.padding = 80.0;
        let before = state.to_export_image().unwrap();
        state.static_image_transform =
            crate::capture::editor::image_transform::StaticImageTransform {
                yaw: 18.0,
                pitch: -12.0,
                roll: 8.0,
                perspective: 0.7,
                pos_x: 0.2,
                pos_y: -0.2,
            };
        let after = state.to_export_image().unwrap();
        assert_eq!(before.dimensions(), after.dimensions());
        assert_eq!(before.get_pixel(0, 0), after.get_pixel(0, 0));
        assert_ne!(before, after);
        let other_perspective = {
            state.static_image_transform.perspective = 0.0;
            state.to_export_image().unwrap()
        };
        assert_ne!(after, other_perspective);
    }

    #[test]
    fn transform_history_groups_drag_and_restores_on_undo_redo() {
        let mut state = document();
        let initial = state.static_image_transform;
        state.begin_history_interaction();
        let mut pose = initial;
        pose.yaw = 10.0;
        state.set_static_image_transform(pose);
        pose.yaw = 20.0;
        pose.pos_y = 0.4;
        state.set_static_image_transform(pose);
        state.finish_history_interaction();
        assert!(state.undo_without_rebuild());
        assert_eq!(state.static_image_transform, initial);
        assert!(!state.undo_without_rebuild());
        assert!(state.redo_without_rebuild());
        assert_eq!(state.static_image_transform, pose);
        assert!(state.set_static_image_transform(initial));
        assert!(state.undo_without_rebuild());
        assert_eq!(state.static_image_transform, pose);
    }

    #[test]
    fn studio_gates_only_nonidentity_final_output() {
        let _trial = crate::studio_trial::test_trial_used_up();
        let mut state = document();
        state.static_image_transform.perspective = 0.9;
        assert!(state.to_final_image().is_ok());
        state.static_image_transform.yaw = 12.0;
        assert!(state.to_export_image().is_ok());
        assert!(state.to_final_image().is_err());
        let _entitlement = crate::studio_entitlement::test_studio_entitlement();
        assert_eq!(
            state.to_final_image().unwrap(),
            state.to_export_image().unwrap()
        );
    }

    #[test]
    fn default_appearance_is_free() {
        assert!(!document().uses_studio_style());
    }

    #[test]
    fn background_panel_controls_stay_free_and_frame_styles_do_not() {
        let mut background = document();
        background.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([0, 0, 255, 255]),
        )));
        background.background.set_padding(16.0);
        background.background.blur = 0.5;
        background.background.radius = 20.0;
        background.background.noise = 0.5;
        assert!(
            !background.uses_studio_style(),
            "the Background panel is free"
        );

        let mut style = document();
        style.background.frame_style = FrameStyle::InsetLight;
        assert!(style.uses_studio_style());

        let mut ratio = document();
        ratio.background.frame_aspect = Some(FrameRatio {
            width: 1,
            height: 1,
        });
        assert!(!ratio.uses_studio_style(), "frame ratios stay free");
    }

    #[test]
    fn a_stale_padding_without_a_fill_stays_free() {
        let mut state = document();
        state.background.padding = 16.0;
        assert!(!state.uses_studio_style());
    }

    #[test]
    fn a_scene_shadow_is_studio_only_once_padding_reveals_it() {
        let mut state = document();
        state.scene_shadow.preset = MotionSceneShadowPreset::Spread;
        assert!(!state.uses_studio_style());
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([0, 0, 255, 255]),
        )));
        state.background.set_padding(16.0);
        assert!(state.uses_studio_style());
        state.scene_shadow.preset = MotionSceneShadowPreset::None;
        assert!(!state.uses_studio_style());
    }

    #[test]
    fn a_styled_document_is_locked_and_a_clean_one_is_not() {
        // These assertions are about the licence gate, not the free trial.
        let _trial = crate::studio_trial::test_trial_used_up();
        let mut styled = document();
        styled.background.frame_style = FrameStyle::InsetLight;
        let error = styled
            .to_final_image()
            .expect_err("a styled still fails closed");
        assert!(error.to_string().contains("Studio"), "{error}");
        assert!(
            document().to_final_image().is_ok(),
            "a clean still stays free"
        );
        let mut wallpapered = document();
        wallpapered.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([0, 0, 255, 255]),
        )));
        wallpapered.background.set_padding(16.0);
        wallpapered.background.radius = 20.0;
        assert!(
            wallpapered.to_final_image().is_ok(),
            "a wallpaper, padding and rounded corners stay free"
        );
        let _entitlement = crate::studio_entitlement::test_studio_entitlement();
        assert_eq!(
            styled.to_final_image().unwrap(),
            styled.to_export_image().unwrap()
        );
    }

    #[test]
    fn background_padding_radius_and_none_reach_export() {
        let mut state = document();
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            20,
            20,
            Rgba([0, 0, 255, 255]),
        )));
        state.background.padding = 40.0;
        state.background.radius = 40.0;
        let image = state.to_export_image().unwrap();
        assert_eq!(image.dimensions(), (120, 100));
        assert_eq!(*image.get_pixel(0, 0), Rgba([0, 0, 255, 255]));
        assert_eq!(*image.get_pixel(60, 50), Rgba([240, 0, 0, 255]));
        assert_eq!(*image.get_pixel(10, 10), Rgba([0, 0, 255, 255]));
        assert_eq!(
            state.to_motion_card_image().unwrap().dimensions(),
            (100, 80)
        );

        state.background.fill = Fill::None;
        let image = state.to_export_image().unwrap();
        assert_eq!(image.dimensions(), (100, 80));
        assert_eq!(image.get_pixel(0, 0)[3], 0);
        assert_eq!(*image.get_pixel(50, 40), Rgba([240, 0, 0, 255]));
    }

    #[test]
    fn custom_fill_and_noise_are_composited_behind_image() {
        let mut state = document();
        state.background.padding = 40.0;
        state.background.fill = Fill::Custom(Rc::new(|context, _, _| {
            context.set_source_rgb(0.5, 0.5, 0.5);
            context.paint().unwrap();
        }));
        let plain = state.to_export_image().unwrap();
        state.background.noise = 1.0;
        let noisy = state.to_export_image().unwrap();
        assert!(plain.pixels().zip(noisy.pixels()).any(|(a, b)| a != b));
        assert_eq!(*noisy.get_pixel(60, 50), Rgba([240, 0, 0, 255]));
    }

    #[test]
    fn blur_affects_only_the_background() {
        let mut state = document();
        let mut fill = RgbaImage::new(120, 100);
        for (x, _, pixel) in fill.enumerate_pixels_mut() {
            *pixel = if x < 60 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            };
        }
        state.background.fill = Fill::Image(Arc::new(fill));
        state.background.padding = 40.0;
        let plain = state.to_export_image().unwrap();
        state.background.blur = 1.0;
        let blurred = state.to_export_image().unwrap();
        assert_ne!(plain.get_pixel(59, 0), blurred.get_pixel(59, 0));
        assert_eq!(*blurred.get_pixel(60, 50), Rgba([240, 0, 0, 255]));
    }

    #[test]
    fn frame_presets_expand_the_filled_canvas_and_center_the_image() {
        let mut state = document();
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            10,
            10,
            Rgba([0, 0, 255, 255]),
        )));
        state.background.padding = 40.0;
        state.background.frame_aspect = Some(FrameRatio::new(1, 1));
        let image = state.to_export_image().unwrap();
        assert_eq!(image.dimensions(), (120, 120));
        assert_eq!(*image.get_pixel(60, 10), Rgba([0, 0, 255, 255]));
        assert_eq!(*image.get_pixel(60, 60), Rgba([240, 0, 0, 255]));
        assert_eq!(*image.get_pixel(60, 110), Rgba([0, 0, 255, 255]));

        state.background.frame_aspect = Some(FrameRatio::new(16, 9));
        let image = state.to_export_image().unwrap();
        assert_eq!(image.dimensions(), (192, 108));
        assert_eq!(*image.get_pixel(5, 54), Rgba([0, 0, 255, 255]));
        assert_eq!(*image.get_pixel(96, 54), Rgba([240, 0, 0, 255]));
        assert_eq!(*image.get_pixel(187, 54), Rgba([0, 0, 255, 255]));

        state.background.frame_aspect = None;
        assert_eq!(state.to_export_image().unwrap().dimensions(), (120, 100));
        state.background.frame_aspect = Some(FrameRatio::new(1, 1));
        state.background.fill = Fill::None;
        let image = state.to_export_image().unwrap();
        assert_eq!(image.dimensions(), (100, 100));
        assert_eq!(*image.get_pixel(50, 0), Rgba([0, 0, 0, 0]));
        assert_eq!(*image.get_pixel(50, 50), Rgba([240, 0, 0, 255]));
    }

    /// A fixed-size preset renders straight at its pixels: a hard vector
    /// edge keeps the antialiasing rim drawn at the exported resolution
    /// instead of picking up the extra mid tones the old whole-image Lanczos
    /// stretch left behind.
    #[test]
    fn a_platform_preset_draws_vectors_at_the_export_size() {
        use crate::capture::editor::types::{AnnotationAction, DrawColor, Point};

        let line = || AnnotationAction::Line {
            start: Point { x: 50.0, y: 0.0 },
            end: Point { x: 50.0, y: 75.0 },
            color: DrawColor::new(1.0, 1.0, 1.0, 1.0),
            stroke_size: 2.0,
            shadow: false,
        };
        let mut state = EditorState::new(RgbaImage::from_pixel(100, 75, Rgba([0, 0, 0, 255])));
        state.actions.push(line());
        state.background.frame_aspect = Some(FrameRatio::new(4, 3));
        state.background.frame_size = Some((400, 300));
        let direct = state.to_export_image().unwrap();
        assert_eq!(direct.dimensions(), (400, 300));

        // The old export flattened at the canvas size, then stretched the
        // whole frame to the preset with Lanczos.
        let mut native = EditorState::new(RgbaImage::from_pixel(100, 75, Rgba([0, 0, 0, 255])));
        native.actions.push(line());
        native.background.frame_aspect = Some(FrameRatio::new(4, 3));
        let flattened = native.to_export_image().unwrap();
        assert_eq!(flattened.dimensions(), (100, 75));
        let stretched =
            image::imageops::resize(&flattened, 400, 300, image::imageops::FilterType::Lanczos3);

        let row = 150;
        let intermediate = |image: &RgbaImage| {
            (0..image.width())
                .filter(|x| {
                    let level = image.get_pixel(*x, row)[0];
                    level > 16 && level < 240
                })
                .count()
        };
        assert!(
            intermediate(&stretched) > intermediate(&direct),
            "the resampled edge keeps more mid tones: {} vs {}",
            intermediate(&stretched),
            intermediate(&direct)
        );
    }

    #[test]
    fn zero_padding_has_no_hidden_gap_and_small_values_change_the_canvas() {
        let mut state = document();
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            10,
            10,
            Rgba([0, 0, 255, 255]),
        )));
        let zero = state.to_export_image().unwrap();
        assert_eq!(zero.dimensions(), (100, 80));
        assert_eq!(*zero.get_pixel(0, 0), Rgba([240, 0, 0, 255]));
        state.background.padding = 1.0;
        let padded = state.to_export_image().unwrap();
        assert_eq!(padded.dimensions(), (102, 82));
        assert_eq!(*padded.get_pixel(0, 0), Rgba([0, 0, 255, 255]));
        assert_eq!(*padded.get_pixel(1, 1), Rgba([240, 0, 0, 255]));
        state.background.padding = 80.0;
        assert_eq!(state.to_export_image().unwrap().dimensions(), (140, 120));
        state.background.padding = 0.0;
        state.background.frame_aspect = Some(FrameRatio::new(1, 1));
        let framed = state.to_export_image().unwrap();
        assert_eq!(framed.dimensions(), (100, 100));
        assert_eq!(*framed.get_pixel(0, 10), Rgba([240, 0, 0, 255]));
        assert_eq!(*framed.get_pixel(50, 0), Rgba([0, 0, 255, 255]));
    }

    #[test]
    fn neural_export_enlarges_a_plain_image() {
        let mut state = EditorState::new(RgbaImage::from_pixel(40, 30, Rgba([200, 40, 10, 255])));
        state.background.fill = Fill::None;
        assert_eq!(state.to_export_image().unwrap().dimensions(), (40, 30));

        state.source_upscale = SourceUpscale::Neural2x;
        assert_eq!(state.to_export_image().unwrap().dimensions(), (80, 60));

        state.source_upscale = SourceUpscale::Neural4x;
        assert_eq!(state.to_export_image().unwrap().dimensions(), (160, 120));
    }

    #[test]
    fn neural_export_matches_the_reported_canvas_with_padding() {
        let mut state = EditorState::new(RgbaImage::from_pixel(60, 40, Rgba([200, 40, 10, 255])));
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([130, 180, 220, 255]),
        )));
        state.background.padding = 40.0;
        state.source_upscale = SourceUpscale::Neural2x;
        assert_eq!(
            state.to_export_image().unwrap().dimensions(),
            state.upscale_output_dimensions(SourceUpscale::Neural2x)
        );
    }

    #[test]
    fn oversized_sources_cannot_be_enhanced() {
        let oversized = EditorState::new(RgbaImage::new(1025, 1024));
        assert!(oversized
            .upscale_unavailable_reason(SourceUpscale::Neural2x)
            .is_some());

        let mut state = EditorState::new(RgbaImage::new(1024, 1024));
        assert!(state
            .upscale_unavailable_reason(SourceUpscale::Neural2x)
            .is_none());
        assert!(state
            .upscale_unavailable_reason(SourceUpscale::Off)
            .is_none());
        assert!(!state.studio_capabilities().image_upscale);

        state.source_upscale = SourceUpscale::Neural2x;
        assert!(state.studio_capabilities().image_upscale);
    }

    #[test]
    fn a_rendered_enhancement_is_no_longer_pending() {
        let mut state = EditorState::new(RgbaImage::from_pixel(24, 18, Rgba([12, 140, 200, 255])));
        assert!(state.pending_upscale().is_none());
        state.source_upscale = SourceUpscale::Neural2x;
        let (mode, base) = state
            .pending_upscale()
            .expect("enhancement has not run yet");
        crate::capture::editor::upscale::enhance_cached(&base, mode).unwrap();
        assert!(state.pending_upscale().is_none());
    }
}
