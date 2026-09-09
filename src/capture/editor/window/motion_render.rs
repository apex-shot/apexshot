//! Shared Motion preview/export drawing.

use gtk4::cairo::{Context, Filter, Format, ImageSurface, LinearGradient, Matrix};
use image::RgbaImage;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::recording::editor::model::{
    affine_from_three_points as affine_components, card_depth, project_card_corners, project_point,
    MotionAppearance, MotionBackgroundFillType, MotionBlurBudgetMode, MotionState,
    MotionTextSegment, MotionTransform, MOTION_EXPORT_FPS,
};

include!("motion_render/geometry.rs");
include!("motion_render/background.rs");
include!("motion_render/card.rs");
include!("motion_render/overlays.rs");
include!("motion_render/compositor.rs");
include!("motion_render/export.rs");
include!("motion_render/tests.rs");
