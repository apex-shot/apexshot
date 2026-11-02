//! Shadow geometry for the Windows still-image compositor.

use super::image_transform::StaticImageTransform;
use super::render::scene_shadow::SceneShadowGeometry;
use super::state::background::Layout;
use crate::recording::editor::model::MotionSceneShadow;

pub(crate) fn geometry(
    layout: Layout,
    width: f64,
    height: f64,
    pose: StaticImageTransform,
    shadow: &MotionSceneShadow,
) -> Option<SceneShadowGeometry> {
    let (opacity, blur, offset) = shadow.clamped().resolved(width.min(height));
    (opacity > 0.001).then(|| SceneShadowGeometry {
        bounds: (0.0, 0.0, layout.width, layout.height),
        corners: super::transform_render::card_corners(layout, width, height, pose),
        radius: layout.radius,
        opacity,
        blur,
        offset,
    })
}
