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
//! save_annotations(&image_path, width, height, &editor_state.actions, &editor_state.base_image)?;
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
    AnnotationFile, ArrowStyle, CanvasSize, Color, FontSettings, NumberSize, NumberingStyle,
    ObfuscateMethod, Point, Rect, SerializableAnnotation, TextAlignment, TextDecoration, FontStyle,
};
pub use storage::{
    annotations_exist, compute_image_hash, delete_annotations, load_annotations,
    load_original_image, original_exists, save_annotations, serializable_to_action, AnnotationError,
};
