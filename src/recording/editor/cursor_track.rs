//! The cursor overlay track, streamed to ffmpeg instead of written to disk.
//!
//! A composite export needs one full-canvas RGBA frame per output frame for the
//! cursor layer: about 8.29 MB per frame at 1920x1080, or roughly 2.5 GB for
//! 10 s at 30 fps. Materialising that as a file costs the export a
//! multi-gigabyte write and, when the scratch root is a tmpfs, spends RAM
//! rather than disk — on a 4 GB machine the difference between a slow export
//! and an exhausted system.
//!
//! Instead the frames are rendered straight into a pipe that ffmpeg reads as
//! `-f rawvideo -pix_fmt rgba -i pipe:4`. The renderer only produces a frame
//! when ffmpeg has room for it, so the export holds one frame in flight
//! instead of the whole track, and there is no intermediate file to leak or to
//! sweep.
//!
//! This is the same shape as the GPU ripple warp, which streams `pipe:3`: a
//! writer thread owns the write end and closes it at EOS, so ffmpeg finalizes
//! deterministically.

use super::model::VideoEditState;
use std::os::fd::OwnedFd;
use std::thread::JoinHandle;

/// A cursor track being rendered into a pipe for ffmpeg to read.
#[derive(Debug)]
pub struct ActiveCursorTrack {
    read_fd: Option<OwnedFd>,
    writer: Option<JoinHandle<()>>,
}

impl ActiveCursorTrack {
    /// Start rendering the cursor track into a pipe ffmpeg reads as `pipe:4`.
    ///
    /// Returns `Err` when the pipe or the writer thread cannot be created, in
    /// which case the caller should export without a cursor overlay rather than
    /// fail the whole export.
    pub fn start(
        state: &VideoEditState,
        start: f64,
        end: f64,
        width: u32,
        height: u32,
        skip_ripple_ring: bool,
    ) -> Result<Self, String> {
        let (read_fd, write_fd) = super::gst_warp::create_pipe()?;
        // The writer gets its own copy of the edit state so the render loop can
        // run off the caller's thread. A frame is only rendered when ffmpeg has
        // drained the pipe, which is what bounds the export's memory.
        let state = state.clone();
        let writer = std::thread::Builder::new()
            .name("cursor-track".into())
            .spawn(move || {
                let mut sink: std::fs::File = write_fd.into();
                if let Err(err) = super::cursor_export::write_rgba_track(
                    &state,
                    start,
                    end,
                    width,
                    height,
                    skip_ripple_ring,
                    &mut sink,
                ) {
                    // ffmpeg closing the pipe early is how this ends when the
                    // export fails; anything else is worth saying out loud.
                    eprintln!("[export] cursor track: {err}");
                }
                // Dropping the sink closes the write end, so ffmpeg sees EOS
                // and finalizes instead of waiting on an input that never ends.
            })
            .map_err(|err| format!("failed to spawn the cursor track writer: {err}"))?;
        Ok(Self {
            read_fd: Some(read_fd),
            writer: Some(writer),
        })
    }

    /// The pipe fd ffmpeg should inherit as fd 4.
    pub fn take_read_fd(&mut self) -> Option<OwnedFd> {
        self.read_fd.take()
    }

    /// Wait for the renderer to finish.
    ///
    /// Called once ffmpeg has exited, so the track never outlives the command
    /// that reads it. A writer blocked on a full pipe wakes as soon as the read
    /// end closes.
    pub fn finish(&mut self) {
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}
