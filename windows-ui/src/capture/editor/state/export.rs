//! Flattened document rendering for copy/save.
//!
//! The Windows export is the plain-image slice of the Linux one: the working
//! image (effects already baked) plus the vector annotations through the
//! shared renderer. Selection chrome, handles, drafts and the active text
//! edit never appear — those live in the view layer only.

use super::super::types::{AnnotationAction, EditorError};
use super::EditorState;
use image::RgbaImage;

impl EditorState {
    /// The document as the user sees it: base pixels with effect actions
    /// applied, then every other annotation painted on top. Renders the
    /// effect layer the preview currently shows.
    pub fn to_rendered_image(&self) -> Result<RgbaImage, EditorError> {
        self.render_from_working(&self.working_image)
    }

    /// Flatten `base + actions` without consulting the live preview layer —
    /// used by export so a pending effect rebuild can never leak an
    /// unredacted frame.
    pub fn to_export_image(&self) -> Result<RgbaImage, EditorError> {
        let mut base = (*self.base_image).clone();
        super::effects::apply_effect_actions(&mut base, &self.actions);
        self.render_from_working(&base)
    }

    fn render_from_working(&self, working: &RgbaImage) -> Result<RgbaImage, EditorError> {
        let (width, height) = working.dimensions();
        if width == 0 || height == 0 {
            return Err(EditorError::ImageSave(
                "image has invalid dimensions".into(),
            ));
        }

        let stride = gtk4::cairo::Format::ARgb32
            .stride_for_width(width)
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;
        let data = super::super::render::rgba_to_cairo_argb_bytes(working);
        let mut surface = gtk4::cairo::ImageSurface::create_for_data(
            data,
            gtk4::cairo::Format::ARgb32,
            width as i32,
            height as i32,
            stride,
        )
        .map_err(|e| EditorError::ImageSave(e.to_string()))?;

        {
            let context = gtk4::cairo::Context::new(&surface)
                .map_err(|e| EditorError::ImageSave(e.to_string()))?;
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
        self.to_rendered_image()
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
