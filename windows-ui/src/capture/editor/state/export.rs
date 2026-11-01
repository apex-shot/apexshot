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
        self.render_from_working(&self.working_image, &self.background)
    }

    /// Flatten `base + actions` without consulting the live preview layer —
    /// used by export so a pending effect rebuild can never leak an
    /// unredacted frame.
    pub fn to_export_image(&self) -> Result<RgbaImage, EditorError> {
        let mut base = (*self.base_image).clone();
        super::effects::apply_effect_actions(&mut base, &self.actions);
        self.render_from_working(&base, &self.background)
    }

    fn render_from_working(
        &self,
        working: &RgbaImage,
        appearance: &super::background::Appearance,
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
            context.translate(layout.image_x, layout.image_y);
            context.scale(self.export_scale, self.export_scale);
            for action in self.vector_annotation_actions() {
                super::super::render::draw_annotation_action(&context, action);
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
        self.to_export_image()
    }

    /// Background-free card: the screenshot (effects baked) plus vector
    /// annotations, the same shape the Linux editor hands Motion.
    pub fn to_motion_card_image(&self) -> Result<RgbaImage, EditorError> {
        self.render_from_working(
            &self.working_image,
            &super::background::Appearance::default(),
        )
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

    fn document() -> EditorState {
        EditorState::new(RgbaImage::from_pixel(100, 80, Rgba([240, 0, 0, 255])))
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
