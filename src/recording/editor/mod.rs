pub mod css;
pub mod ffmpeg;
pub mod model;
mod ui;

use std::path::PathBuf;

pub use model::{AudioMode, DimensionPreset, VideoEditState, VideoMetadata};

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

    ffmpeg::ensure_tools_available()?;
    let metadata = ffmpeg::probe_metadata(&path)?;
    ui::open(metadata)
}
