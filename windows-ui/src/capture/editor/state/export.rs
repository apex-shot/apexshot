//! Flattened document rendering for copy/save.
//!
//! The background, working image and vector annotations are flattened without
//! selection chrome, handles, drafts or the active text edit.

use super::super::types::{AnnotationAction, EditorError};
use super::EditorState;
use image::RgbaImage;

impl EditorState {
    /// The document as the user sees it: base pixels with effect actions
    /// applied, then every other annotation painted on top. Renders the
    /// effect layer the preview currently shows.
    pub fn to_rendered_image(&self) -> Result<RgbaImage, EditorError> {
        self.render_from_working(&self.working_image, &self.background, true)
    }

    /// Flatten `base + actions` without consulting the live preview layer —
    /// used by export so a pending effect rebuild can never leak an
    /// unredacted frame.
    pub fn to_export_image(&self) -> Result<RgbaImage, EditorError> {
        let mut base = (*self.base_image).clone();
        super::effects::apply_effect_actions(&mut base, &self.actions);
        self.render_from_working(&base, &self.background, true)
    }

    fn render_from_working(
        &self,
        working: &RgbaImage,
        appearance: &super::background::Appearance,
        transform: bool,
    ) -> Result<RgbaImage, EditorError> {
        let (image_width, image_height) = working.dimensions();
        if image_width == 0 || image_height == 0 {
            return Err(EditorError::ImageSave(
                "image has invalid dimensions".into(),
            ));
        }
        let layout = appearance.layout(image_width, image_height);
        let width = layout.width.ceil() as u32;
        let height = layout.height.ceil() as u32;
        if u64::from(width) * u64::from(height) > 100_000_000 {
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
        let glass = if !transform || self.static_image_transform.is_identity() {
            super::super::glass_render::render(
                appearance,
                layout,
                image_width,
                image_height,
                layout.width.max(layout.height),
            )
        } else {
            None
        };

        {
            let context = gtk4::cairo::Context::new(&surface)
                .map_err(|e| EditorError::ImageSave(e.to_string()))?;
            if appearance.has_fill() {
                let background = appearance
                    .surface(layout, layout.width.max(layout.height))
                    .ok_or_else(|| EditorError::ImageSave("could not render background".into()))?;
                context.save().ok();
                context.scale(
                    f64::from(width) / f64::from(background.width()),
                    f64::from(height) / f64::from(background.height()),
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
            let style = super::super::style_render::StyleGeometry::new(
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
            style.paint_backings(&context);
            style.paint_underlay(&context);
            if transform && !self.static_image_transform.is_identity() {
                let card = self.card_surface(working, layout.radius)?;
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
                context.paint().ok();
                context.restore().ok();
                if let Some(glass) = &glass {
                    glass.paint(&context);
                } else {
                    style.paint_frame(&context);
                }
                context.translate(layout.image_x, layout.image_y);
                context.scale(self.export_scale, self.export_scale);
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

    /// The final export image. Always flattens `base + actions` so a pending
    /// preview rebuild cannot leak an unredacted frame.
    pub fn to_final_image(&self) -> Result<RgbaImage, EditorError> {
        crate::studio_entitlement::ensure_export_allowed(
            crate::studio_entitlement::capabilities_for_static_image(self.static_image_transform),
        )
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

    pub(crate) fn card_surface(
        &self,
        working: &RgbaImage,
        radius: f64,
    ) -> Result<gtk4::cairo::ImageSurface, EditorError> {
        let source = super::super::render::rgba_image_to_surface(working)
            .ok_or_else(|| EditorError::ImageSave("could not render image".into()))?;
        self.card_surface_from_source(&source, radius, working.width(), working.height())
    }

    pub(crate) fn card_surface_from_source(
        &self,
        source: &gtk4::cairo::ImageSurface,
        radius: f64,
        width: u32,
        height: u32,
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
        context.scale(self.export_scale, self.export_scale);
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
        assert!(state.to_final_image().is_ok(), "shadows remain free");
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
}
