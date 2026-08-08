mod color;
mod composition;
mod io_ops;
#[allow(dead_code)]
pub mod numbering_style;
#[allow(dead_code)]
mod pen_weight;
pub mod preferences;
pub mod preprocess;
mod render;
mod selection;
mod state;
#[allow(dead_code)]
mod text_detect;
pub mod types;
pub(crate) mod ui_support;
#[path = "editor/window/mod.rs"]
pub mod window;

pub use types::EditorError;
pub use window::open_image_editor;
pub use window::open_image_editor_empty;

pub fn copy_file_uri_to_clipboard(path: &std::path::Path) -> Result<(), String> {
    io_ops::copy_uri_to_clipboard(path)
}

#[cfg(test)]
#[path = "editor/tests.rs"]
mod tests;
