//! Annotation persistence module
//!
//! This module provides non-destructive annotation storage for screenshots.
//! Annotations are saved separately from the image in a centralized location,
//! allowing users to re-edit their annotations later.
//!
//! # Storage Location
//!
//! Annotations are stored in `~/.local/share/apexshot/annotations/`
//! Each file is named by the SHA256 hash of the original image path.
//!
//! Original base images (before annotations) are stored in `~/.local/share/apexshot/originals/`
//!
//! # Usage
//!
//! ```ignore
//! use apexshot::annotations::{save_annotations, load_annotations, load_original_image, serializable_to_action};
//!
//! // Save annotations when user clicks "Done"
//! save_annotations(
//!     &image_path,
//!     width,
//!     height,
//!     &editor_state.actions,
//!     &editor_state.base_image,
//!     &editor_state.background_style,
//!     editor_state.background_padding,
//!     editor_state.background_shadow,
//!     editor_state.background_insert,
//!     editor_state.auto_balance,
//!     editor_state.background_alignment,
//!     editor_state.background_corner_radius,
//!     editor_state.background_aspect_ratio,
//! )?;
//!
//! // Load annotations when user clicks "Edit" again
//! if let Some(original) = load_original_image(&image_path)? {
//!     // Use the original base image instead of the flattened one
//! }
//! if let Some(file) = load_annotations(&image_path)? {
//!     let actions: Vec<AnnotationAction> = file.annotations
//!         .iter()
//!         .map(|a| serializable_to_action(a))
//!         .collect();
//!     // Apply actions to editor state
//! }
//! ```

mod schema;
mod storage;

pub use schema::{
    AnnotationFile, ArrowStyle, BackgroundAlignment, BackgroundSettings, BackgroundStyle,
    CanvasSize, Color, CropAspectRatio, FontSettings, FontStyle, NumberSize, NumberingStyle,
    ObfuscateMethod, Point, Rect, SerializableAnnotation, TextAlignment, TextDecoration,
};
pub use storage::{
    annotations_exist, background_alignment_from_serializable, background_style_from_serializable,
    compute_image_hash, crop_aspect_ratio_from_serializable, delete_annotations, load_annotations,
    load_original_image, original_exists, save_annotations, serializable_to_action,
    AnnotationError,
};
