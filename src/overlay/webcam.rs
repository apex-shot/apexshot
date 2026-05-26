//! Webcam preview via XDG Camera portal + native PipeWire.
//!
//! Replaces the GStreamer v4l2src pipeline with proper Wayland security model:
//! `org.freedesktop.portal.Camera` → PipeWire stream → BGRA frames.
//!
//! Falls back to direct v4l2 enumeration for device listing on systems
//! without the Camera portal (older desktops, wlroots).

use std::os::fd::AsRawFd;
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
// Camera portal + native PipeWire preview
// ---------------------------------------------------------------------------

/// Start webcam preview using the XDG Camera portal + native PipeWire.
///
/// This is the proper Wayland security model: the portal handles permissions,
/// and frames arrive through a PipeWire stream (same architecture as screen
/// capture).
pub(crate) fn start_webcam_preview(_device: i32, _flip: bool) -> Option<WebcamPreview> {
    // Run the async portal flow on a temporary tokio runtime.
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[webcam] Failed to create tokio runtime: {e}");
            return None;
        }
    };

    let (fd, node_id) = match rt.block_on(open_camera_portal()) {
        Ok(Some(result)) => result,
        Ok(None) => {
            eprintln!("[webcam] No camera available via portal");
            return None;
        }
        Err(e) => {
            eprintln!("[webcam] Camera portal failed: {e}. Falling back to v4l2.");
            return start_webcam_preview_v4l2(_device, _flip);
        }
    };

    eprintln!(
        "[webcam] Camera portal: fd={}, node_id={node_id}",
        fd.as_raw_fd()
    );

    // PipeWireCapture contains non-Send Rc types. Run it on a dedicated thread
    // and communicate frames via channel — same pattern as recording.
    let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<(Vec<u8>, u32, u32)>(4);

    std::thread::spawn(move || {
        let capture = match PipeWireCapture::connect(fd, node_id, None, Some(640), Some(480)) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[webcam] PipeWire connect failed: {e}");
                return;
            }
        };

        if let Some(format) = capture.format() {
            eprintln!(
                "[webcam] Camera: {}x{} @ {}/{} fps",
                format.width, format.height, format.framerate_num, format.framerate_denom
            );
        }

        loop {
            match capture.try_recv_frame() {
                Ok(Some(pw_frame)) => {
                    // Convert RGBA to BGRA for Cairo.
                    let mut bgra = pw_frame.pixels;
                    for px in bgra.chunks_exact_mut(4) {
                        px.swap(0, 2);
                    }
                    if frame_tx
                        .send((bgra, pw_frame.width, pw_frame.height))
                        .is_err()
                    {
                        break; // receiver dropped
                    }
                }
                Ok(None) => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => {
                    eprintln!("[webcam] Frame error: {e}");
                    break;
                }
            }
        }
    });

    let frame = Arc::new(Mutex::new(None));
    let stop = Arc::new(AtomicBool::new(false));
    let frame_thread = frame.clone();
    let stop_thread = stop.clone();

    // Consumer thread: read from channel, update shared frame.
    std::thread::spawn(move || {
        while !stop_thread.load(Ordering::Relaxed) {
            match frame_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok((bgra, w, h)) => {
                    if let Ok(mut slot) = frame_thread.lock() {
                        *slot = Some(WebcamFrame {
                            width: w as i32,
                            height: h as i32,
                            bgra,
                        });
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }
        }
    });

    Some(WebcamPreview {
        frame,
        stop,
        _capture: None, // owned by the PipeWire thread
        _v4l2_pipeline: None,
    })
}

/// Open the Camera portal via ashpd (proper request/response handling).
async fn open_camera_portal() -> Result<Option<(std::os::fd::OwnedFd, u32)>, String> {
    use ashpd::desktop::camera::Camera;

    let camera = Camera::new()
        .await
        .map_err(|e| format!("Camera proxy: {e}"))?;

    if !camera
        .is_present()
        .await
        .map_err(|e| format!("IsCameraPresent: {e}"))?
    {
        return Ok(None);
    }

    // Request access — triggers permission dialog, waits for user response.
    camera
        .request_access()
        .await
        .map_err(|e| format!("AccessCamera: {e}"))?
        .response()
        .map_err(|e| format!("AccessCamera response: {e}"))?;

    let fd = camera
        .open_pipe_wire_remote()
        .await
        .map_err(|e| format!("OpenPipeWireRemote: {e}"))?;

    eprintln!("[webcam] Camera portal opened, fd={}", fd.as_raw_fd());
    Ok(Some((fd, pipewire::constants::ID_ANY)))
}

// ---------------------------------------------------------------------------
// v4l2 fallback (GStreamer, used when portal is unavailable)
// ---------------------------------------------------------------------------

fn start_webcam_preview_v4l2(device: i32, flip: bool) -> Option<WebcamPreview> {
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
