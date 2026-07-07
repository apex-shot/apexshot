pub mod ffmpeg;
pub mod model;
pub mod ui_support;
mod window;

use std::path::PathBuf;

pub use model::{AudioMode, DimensionPreset, VideoEditState, VideoMetadata};

pub fn open_empty_recording_editor() -> anyhow::Result<()> {
    window::open_empty()
}

pub fn open_recording_editor(path: PathBuf) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!("recording does not exist: {}", path.display());
    }
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| !ext.eq_ignore_ascii_case("mp4"))
        .unwrap_or(true)
    {
        anyhow::bail!("recording editor only supports MP4 files in this version");
    }

    // Open the editor window immediately and load the recording
    // asynchronously (ffprobe + thumbnails run in a background thread with
    // a loading spinner). This avoids a long frozen gap before the window
    // appears for large recordings.
    window::open_with_path(path)
}
