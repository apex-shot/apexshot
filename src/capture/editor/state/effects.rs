use super::super::render::{
    apply_blackout_rect, apply_censor_rect, apply_focus_rect, apply_hybrid_blur,
};
use super::super::types::{AnnotationAction, ObfuscateMethod};
use super::EditorState;
use image::RgbaImage;
use std::sync::Arc;

impl EditorState {
    pub fn rebuild_effect_layer(&mut self) {
        let mut working = (*self.base_image).clone();
        super::apply_effect_actions(&mut working, &self.actions);
        self.working_image = Arc::new(working);
        self.select_effect_rebuild_pending = false;
        self.mark_working_image_dirty();
    }
}

pub(super) fn apply_effect_actions(image: &mut RgbaImage, actions: &[AnnotationAction]) {
    for action in actions {
        match action {
            AnnotationAction::Obfuscate {
                rect,
                method,
                amount,
            } => match method {
                ObfuscateMethod::Pixelate => apply_censor_rect(image, *rect, *amount),
                ObfuscateMethod::Blur => apply_hybrid_blur(image, *rect, *amount),
                ObfuscateMethod::Blackout => apply_blackout_rect(image, rect),
            },
            AnnotationAction::Focus { rect, intensity } => {
                apply_focus_rect(image, *rect, *intensity)
            }
            _ => {}
        }
    }
}
