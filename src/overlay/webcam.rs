//! Webcam preview via GStreamer v4l2src (matching C++ overlay behavior).
//!
//! Uses direct v4l2 device access via GStreamer.  The Camera portal path was
//! removed because `rt.block_on()` inside the GTK click handler blocked the
//! main thread, causing Hyprland (and other compositors) to close the overlay
//! surface when the portal permission dialog appeared.
//!
//! Device enumeration still scans `/dev/video*` nodes directly.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use crate::pipewire_engine::PipeWireCapture;

#[derive(Clone)]
pub(crate) struct WebcamFrame {
    pub(crate) width: i32,
    pub(crate) height: i32,
    /// BGRA pixel data (Cairo ARgb32 format on little-endian).
    pub(crate) bgra: Vec<u8>,
}

pub(crate) struct WebcamPreview {
    frame: Arc<Mutex<Option<WebcamFrame>>>,
    stop: Arc<AtomicBool>,
    // Held to keep the capture/pipeline alive until dropped.
    _capture: Option<PipeWireCapture>,
    _v4l2_pipeline: Option<Arc<Mutex<Option<gstreamer::Element>>>>,
}

impl WebcamPreview {
    pub(crate) fn frame_handle(&self) -> Arc<Mutex<Option<WebcamFrame>>> {
        self.frame.clone()
    }
}

impl Drop for WebcamPreview {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(pipeline) = self._v4l2_pipeline.as_ref() {
            if let Ok(mut guard) = pipeline.lock() {
                if let Some(pipeline) = guard.take() {
                    use gstreamer::prelude::ElementExt;
                    let _ = pipeline.set_state(gstreamer::State::Null);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Device enumeration (v4l2 fallback, used when Camera portal is unavailable)
// ---------------------------------------------------------------------------

pub(crate) fn enumerate_webcam_devices() -> Vec<i32> {
    let mut devices: Vec<i32> = std::fs::read_dir("/dev")
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.strip_prefix("video")?.parse::<i32>().ok()
        })
        .collect();
    devices.sort_unstable();

    let mut seen_names = std::collections::HashSet::new();
    devices
        .into_iter()
        .filter(|idx| is_capture_device(*idx))
        .filter(|idx| {
            let name = webcam_device_name(*idx).unwrap_or_else(|| format!("/dev/video{idx}"));
            seen_names.insert(name)
        })
        .collect()
}

fn webcam_device_name(index: i32) -> Option<String> {
    let name_path = format!("/sys/class/video4linux/video{index}/name");
    std::fs::read_to_string(name_path)
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

fn is_capture_device(index: i32) -> bool {
    if let Some(name) = webcam_device_name(index) {
        if name.to_ascii_lowercase().contains("metadata") {
            return false;
        }
    }
    std::path::Path::new(&format!("/dev/video{index}")).exists()
}

pub(crate) fn first_webcam_device() -> Option<i32> {
    enumerate_webcam_devices().into_iter().next()
}

// ---------------------------------------------------------------------------
// Webcam preview via GStreamer v4l2 (matching C++ overlay behavior)
// ---------------------------------------------------------------------------

pub(crate) fn start_webcam_preview(device: i32, flip: bool) -> Option<WebcamPreview> {
    use gstreamer as gst;
    use gstreamer::prelude::*;
    use gstreamer_app as gst_app;

    if device < 0 {
        return None;
    }
    if let Err(err) = gst::init() {
        eprintln!("[overlay] webcam preview gst init failed: {err}");
        return None;
    }

    let device_path = format!("/dev/video{device}");
    let flip_filter = if flip {
        " ! videoflip method=horizontal-flip"
    } else {
        ""
    };
    let pipeline_str = format!(
        "v4l2src device={device_path} ! video/x-raw,width=640,height=480,framerate=30/1 ! videoconvert{flip_filter} ! video/x-raw,format=BGRA ! appsink name=sink emit-signals=false sync=false max-buffers=1 drop=true"
    );

    let pipeline = match gst::parse::launch(&pipeline_str) {
        Ok(pipeline) => pipeline,
        Err(err) => {
            eprintln!("[overlay] webcam preview pipeline error: {err}");
            return None;
        }
    };

    let sink = pipeline
        .clone()
        .dynamic_cast::<gst::Bin>()
        .ok()
        .and_then(|bin| bin.by_name("sink"))
        .and_then(|element| element.downcast::<gst_app::AppSink>().ok())?;

    if let Err(err) = pipeline.set_state(gst::State::Playing) {
        eprintln!("[overlay] webcam preview failed to start: {err:?}");
        let _ = pipeline.set_state(gst::State::Null);
        return None;
    }

    let pipeline_slot = Arc::new(Mutex::new(Some(pipeline)));
    let frame = Arc::new(Mutex::new(None));
    let stop = Arc::new(AtomicBool::new(false));
    let frame_thread = frame.clone();
    let stop_thread = stop.clone();

    std::thread::spawn(move || {
        while !stop_thread.load(Ordering::Relaxed) {
            let Some(sample) = sink.try_pull_sample(gst::ClockTime::from_mseconds(100)) else {
                continue;
            };
            let Some(buffer) = sample.buffer() else {
                continue;
            };
            let Some(caps) = sample.caps() else {
                continue;
            };
            let Some(structure) = caps.structure(0) else {
                continue;
            };
            let Ok(width) = structure.get::<i32>("width") else {
                continue;
            };
            let Ok(height) = structure.get::<i32>("height") else {
                continue;
            };
            let Ok(map) = buffer.map_readable() else {
                continue;
            };
            let data = map.as_slice();
            if width > 0 && height > 0 && data.len() >= (width as usize * height as usize * 4) {
                let mut bgra = data[..width as usize * height as usize * 4].to_vec();
                for px in bgra.chunks_exact_mut(4) {
                    px[3] = 255;
                }
                if let Ok(mut slot) = frame_thread.lock() {
                    *slot = Some(WebcamFrame {
                        width,
                        height,
                        bgra,
                    });
                }
            }
        }
    });

    Some(WebcamPreview {
        frame,
        stop,
        _capture: None,
        _v4l2_pipeline: Some(pipeline_slot),
    })
}
