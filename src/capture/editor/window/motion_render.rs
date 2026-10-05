//! Shared Motion preview/export drawing.

use gtk4::cairo::{Context, Filter, Format, ImageSurface, LinearGradient, Matrix, Operator};
use image::RgbaImage;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::capture::editor::composition::{motion_background_composition, CompositionLayout};
use crate::recording::editor::model::{
    affine_from_three_points as affine_components, background_render::render_gradient, card_depth,
    project_card_corners, project_point, MotionAppearance, MotionBackgroundFillType,
    MotionBlurBudgetMode, MotionFrame, MotionSceneShadow, MotionSceneShadowPreset, MotionState,
    MotionTextAlignment, MotionTextFormat, MotionTextScope, MotionTextSegment, MotionTextStyle,
    MotionTransform, MAX_MOTION_TEXT_LINE_SPACING, MAX_MOTION_TEXT_POS, MAX_MOTION_TEXT_SIZE,
    MIN_MOTION_TEXT_LINE_SPACING, MIN_MOTION_TEXT_POS, MIN_MOTION_TEXT_SIZE, MIN_MOTION_TEXT_WIDTH,
    MOTION_EXPORT_FPS,
};

include!("motion_render/geometry.rs");
include!("motion_render/background.rs");
include!("motion_render/card.rs");
include!("motion_render/text.rs");
include!("motion_render/overlays.rs");
include!("motion_render/compositor.rs");
include!("motion_render/export.rs");
include!("motion_render/tests.rs");
