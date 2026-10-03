//! GPU ripple warp for the export.
//!
//! The studied ripple's visible band is a footage displacement, not a drawn
//! ring. The composite export runs that displacement as a fragment shader on
//! the source frames and pipes the warped frames to ffmpeg, mirroring the
//! GStreamer → inherited pipe fd → ffmpeg shape already used for recording
//! audio (`gst_audio`). No per-frame displacement map is ever written, so the
//! export does not materialise gigabytes of scratch.
//!
//! The shader is the same maths as [`super::click_effect`]: the per-frame
//! click state (radius and decaying bounce) is uploaded as uniforms and the
//! fragment shader evaluates the ring envelope, calm zone, and pull per pixel.
//! When the GL stack is unavailable the caller falls back to the drawn ring.

use super::click_effect::{ripple_bounce_at_ms, ripple_radius_px, RIPPLE_PULL_SIZE_01};
use super::model::ClickEffect;
use super::model::VideoEditState;
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use std::io::Write;
use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// The fragment shader. Scalar uniforms keep the GStreamer property mapping
/// simple (a `gfloat` field per value); a vector uniform would need a value
/// array that the property API does not take reliably.
const FRAGMENT: &str = r#"
precision highp float;
uniform sampler2D tex;
uniform float u_w;
uniform float u_h;
uniform float u_pull_size;
uniform float u_c0_x;
uniform float u_c0_y;
uniform float u_c0_radius;
uniform float u_c0_bounce;
uniform float u_c1_x;
uniform float u_c1_y;
uniform float u_c1_radius;
uniform float u_c1_bounce;
uniform float u_c2_x;
uniform float u_c2_y;
uniform float u_c2_radius;
uniform float u_c2_bounce;
varying vec2 v_texcoord;

vec2 click_pull(float cx, float cy, float radius, float bounce, vec2 pixel, float band) {
    if (bounce == 0.0) return vec2(0.0);
    vec2 relative = pixel - vec2(cx, cy);
    float distance = length(relative);
    if (abs(distance - radius) > band * 3.0) return vec2(0.0);
    float rd = (distance - radius) / band;
    float ring = exp(-rd * rd * 4.0);
    if (distance == 0.0) return vec2(0.0);
    float calm = 1.0;
    if (u_pull_size > 0.5) {
        calm = smoothstep(u_pull_size * 0.35, u_pull_size, distance);
    }
    return (relative / distance) * bounce * ring * u_w * 0.015 * calm;
}

void main() {
    vec2 pixel = vec2(v_texcoord.x * u_w, v_texcoord.y * u_h);
    float band = u_w * 0.05;
    vec2 total = click_pull(u_c0_x, u_c0_y, u_c0_radius, u_c0_bounce, pixel, band)
               + click_pull(u_c1_x, u_c1_y, u_c1_radius, u_c1_bounce, pixel, band)
               + click_pull(u_c2_x, u_c2_y, u_c2_radius, u_c2_bounce, pixel, band);
    vec2 uv = v_texcoord - total / vec2(u_w, u_h);
    gl_FragColor = texture2D(tex, clamp(uv, vec2(0.0), vec2(1.0)));
}
"#;

/// One click the warp reacts to, in source video pixels and source seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WarpClick {
    pub x: f64,
    pub y: f64,
    pub t: f64,
}

/// Everything the warp pipeline needs for one export segment.
#[derive(Debug, Clone)]
pub struct WarpSetup {
    pub source: std::path::PathBuf,
    pub start: f64,
    pub end: f64,
    pub frame_rate: f64,
    pub width: u32,
    pub height: u32,
    /// Visible lifetime of the ripple, in seconds.
    pub window: f64,
    /// Clicks in source video pixels, sorted by `t`.
    pub clicks: Vec<WarpClick>,
}

impl WarpSetup {
    /// Build the warp for an export window, or `None` when no ripple is
    /// visible or the pointer track cannot be rendered.
    pub fn from_state(state: &VideoEditState, start: f64, end: f64) -> Option<Self> {
        if state.cursor.click_effect != ClickEffect::Ripple {
            return None;
        }
        let sidecar = state.sidecar.as_ref()?;
        if !sidecar.can_render_cursor_overlay() {
            return None;
        }
        let window = state.cursor.click_window_seconds();
        let width = state.metadata.width as f64;
        let height = state.metadata.height as f64;
        let mut clicks: Vec<WarpClick> = sidecar
            .clicks
            .iter()
            .map(|click| {
                let (x, y) = sidecar.map_to_video(click.x, click.y, width, height);
                WarpClick { x, y, t: click.t }
            })
            .collect();
        clicks.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
        if !clicks
            .iter()
            .any(|click| click.t < end && click.t + window > start)
        {
            return None;
        }
        Some(Self {
            source: state.metadata.path.clone(),
            start,
            end,
            frame_rate: state.metadata.export_frame_rate(),
            width: state.metadata.width,
            height: state.metadata.height,
            window,
            clicks,
        })
    }

    fn frames(&self) -> usize {
        (((self.end - self.start).max(0.0) * self.frame_rate).ceil() as usize).max(1)
    }
}

/// True when the GStreamer GL shader stack is present.
pub fn gl_warp_available() -> bool {
    if super::super::gst_audio::ensure_gst_initialized().is_err() {
        return false;
    }
    ["glupload", "glshader", "gldownload"]
        .iter()
        .all(|name| gst::ElementFactory::find(name).is_some())
}

/// The 16 float uniforms for one frame: size, calm-zone radius, then up to
/// three clicks as `(x, y, radius, bounce)`.
///
/// The last three clicks inside the window are kept, matching the studied
/// renderer's recent-click limit. Clicks with a zero bounce contribute
/// nothing and are left as zeros.
pub fn uniform_state(setup: &WarpSetup, source_t: f64) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    out[0] = setup.width as f32;
    out[1] = setup.height as f32;
    out[2] = (RIPPLE_PULL_SIZE_01 * setup.width as f64) as f32;
    let mut active: Vec<&WarpClick> = setup
        .clicks
        .iter()
        .filter(|click| (0.0..setup.window).contains(&(source_t - click.t)))
        .collect();
    if active.len() > 3 {
        active.drain(0..active.len() - 3);
    }
    for (index, click) in active.iter().enumerate() {
        let age_ms = (source_t - click.t) * 1000.0;
        let base = 3 + index * 4;
        out[base] = click.x as f32;
        out[base + 1] = click.y as f32;
        out[base + 2] = ripple_radius_px(age_ms, RIPPLE_PULL_SIZE_01, setup.width as f64) as f32;
        out[base + 3] = ripple_bounce_at_ms(age_ms) as f32;
    }
    out
}

fn uniform_structure(state: &[f32; 16]) -> gst::Structure {
    gst::Structure::builder("uniforms")
        .field("u_w", state[0])
        .field("u_h", state[1])
        .field("u_pull_size", state[2])
        .field("u_c0_x", state[3])
        .field("u_c0_y", state[4])
        .field("u_c0_radius", state[5])
        .field("u_c0_bounce", state[6])
        .field("u_c1_x", state[7])
        .field("u_c1_y", state[8])
        .field("u_c1_radius", state[9])
        .field("u_c1_bounce", state[10])
        .field("u_c2_x", state[11])
        .field("u_c2_y", state[12])
        .field("u_c2_radius", state[13])
        .field("u_c2_bounce", state[14])
        .build()
}

/// Create a pipe for a writer thread to feed ffmpeg through an inherited fd.
///
/// Shared by the GPU ripple warp (`pipe:3`) and the cursor track (`pipe:4`).
/// Both ends are `CLOEXEC`; the read end is handed to ffmpeg by
/// [`crate::recording::backend::attach_pipe_as_fd`], which clears that flag on
/// the fd ffmpeg actually inherits.
pub(super) fn create_pipe() -> Result<(OwnedFd, OwnedFd), String> {
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(format!(
            "failed to create warp pipe: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// A running GPU warp. Frames are written to an inherited pipe fd that ffmpeg
/// reads as `-f rawvideo -pix_fmt yuv420p -i pipe:3`.
pub struct ActiveGstWarp {
    pipeline: gst::Pipeline,
    read_fd: Option<OwnedFd>,
    halt: Arc<AtomicBool>,
    done: Option<mpsc::Receiver<()>>,
    writer: Option<std::thread::JoinHandle<()>>,
}

impl ActiveGstWarp {
    /// Build and start the warp pipeline, prebuffering the first frame so a
    /// missing GL context is caught before ffmpeg is spawned.
    pub fn start(setup: &WarpSetup) -> Result<Self, String> {
        super::super::gst_audio::ensure_gst_initialized()?;
        let pipeline = gst::Pipeline::new();
        let src = make("filesrc")?;
        src.set_property("location", setup.source.to_string_lossy().as_ref());
        let decode = make("decodebin")?;
        let convert = make("videoconvert")?;
        let upload = make("glupload")?;
        let shader = make("glshader")?;
        shader.set_property("fragment", FRAGMENT);
        let download = make("gldownload")?;
        let convert_out = make("videoconvert")?;
        let caps = make("capsfilter")?;
        caps.set_property(
            "caps",
            gst::Caps::builder("video/x-raw")
                .field("format", "I420")
                .build(),
        );
        let sink = gst_app::AppSink::builder()
            .name("warp_sink")
            .sync(false)
            .max_buffers(8)
            .drop(false)
            .build();
        let sink_element: gst::Element = sink.clone().upcast();

        pipeline
            .add_many([
                &src,
                &decode,
                &convert,
                &upload,
                &shader,
                &download,
                &convert_out,
                &caps,
                &sink_element,
            ])
            .map_err(|err| format!("failed to build warp pipeline: {err}"))?;
        gst::Element::link_many([&src, &decode])
            .map_err(|err| format!("failed to link warp source: {err}"))?;
        gst::Element::link_many([
            &convert,
            &upload,
            &shader,
            &download,
            &convert_out,
            &caps,
            &sink_element,
        ])
        .map_err(|err| format!("failed to link warp chain: {err}"))?;

        let convert_sink = convert
            .static_pad("sink")
            .ok_or("warp videoconvert has no sink pad")?;
        decode.connect_pad_added(move |_, pad| {
            if convert_sink.is_linked() {
                return;
            }
            let caps = pad.query_caps(None);
            if caps.can_intersect(&gst::Caps::builder("video/x-raw").build()) {
                if let Err(err) = pad.link(&convert_sink) {
                    eprintln!("[recording] failed to link decoded video into the warp: {err}");
                }
            }
        });

        // Upload the studied click state for each frame before the shader
        // reads it. The probe runs on the streaming thread, ahead of the
        // element's chain function, so the property is set in time.
        let probe_setup = setup.clone();
        let probe_shader = shader.clone();
        let sink_pad = shader
            .static_pad("sink")
            .ok_or("glshader has no sink pad")?;
        sink_pad.add_probe(gst::PadProbeType::BUFFER, move |_pad, info| {
            if let Some(buffer) = info.buffer() {
                if let Some(pts) = buffer.pts() {
                    let source_t = pts.nseconds() as f64 / 1e9;
                    let state = uniform_state(&probe_setup, source_t);
                    probe_shader.set_property("uniforms", uniform_structure(&state));
                }
            }
            gst::PadProbeReturn::Ok
        });

        pipeline
            .set_state(gst::State::Paused)
            .map_err(|err| format!("failed to preroll warp pipeline: {err:?}"))?;
        let (change, current, pending) = pipeline.state(gst::ClockTime::from_seconds(20));
        if current != gst::State::Paused {
            let bus_error = poll_bus_error(&pipeline)
                .map(|err| format!(" ({err})"))
                .unwrap_or_default();
            let _ = pipeline.set_state(gst::State::Null);
            return Err(format!(
                "warp pipeline did not reach PAUSED: {change:?} current={current:?} pending={pending:?}{bus_error}"
            ));
        }
        pipeline
            .seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                gst::ClockTime::from_nseconds((setup.start.max(0.0) * 1e9) as u64),
            )
            .map_err(|err| format!("failed to seek warp pipeline: {err}"))?;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|err| format!("failed to start warp pipeline: {err:?}"))?;

        // Wait for the first frame so a broken GL context fails before ffmpeg
        // is spawned with a pipe that would never produce data.
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut prebuf = None;
        while prebuf.is_none() {
            if let Some(err) = poll_bus_error(&pipeline) {
                let _ = pipeline.set_state(gst::State::Null);
                return Err(format!("warp pipeline error: {err}"));
            }
            if let Some(sample) = sink.try_pull_sample(gst::ClockTime::ZERO) {
                prebuf = Some(sample);
                break;
            }
            if Instant::now() > deadline {
                let _ = pipeline.set_state(gst::State::Null);
                return Err("warp pipeline produced no frames".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let (read_fd, write_fd) = create_pipe()?;
        let halt = Arc::new(AtomicBool::new(false));
        let writer_halt = halt.clone();
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let frames = setup.frames();
        let writer = std::thread::Builder::new()
            .name("apexshot-warp-writer".into())
            .spawn(move || {
                let mut file: std::fs::File = write_fd.into();
                let mut written = 0usize;
                let write_sample = |sample: &gst::Sample, file: &mut std::fs::File| -> bool {
                    if let Some(buffer) = sample.buffer() {
                        if let Ok(map) = buffer.map_readable() {
                            return file.write_all(map.as_slice()).is_ok();
                        }
                    }
                    true
                };
                if let Some(sample) = prebuf.as_ref() {
                    if write_sample(sample, &mut file) {
                        written += 1;
                    }
                }
                while written < frames && !writer_halt.load(Ordering::Acquire) {
                    match sink.try_pull_sample(gst::ClockTime::from_mseconds(100)) {
                        Some(sample) => {
                            if !write_sample(&sample, &mut file) {
                                break;
                            }
                            written += 1;
                        }
                        None => {
                            if sink.is_eos() {
                                break;
                            }
                        }
                    }
                }
                let _ = done_tx.send(());
            })
            .map_err(|err| format!("failed to spawn warp writer: {err}"))?;

        Ok(Self {
            pipeline,
            read_fd: Some(read_fd),
            halt,
            done: Some(done_rx),
            writer: Some(writer),
        })
    }

    /// The pipe fd ffmpeg should inherit as fd 3.
    pub fn take_read_fd(&mut self) -> Option<OwnedFd> {
        self.read_fd.take()
    }

    /// Drain the bus and return the first pipeline error, if any.
    pub fn poll_bus_error(&self) -> Option<String> {
        poll_bus_error(&self.pipeline)
    }

    /// Stop cleanly after ffmpeg has consumed every frame.
    pub fn stop(mut self) {
        self.halt.store(true, Ordering::Release);
        self.join_writer(Duration::from_secs(10));
        let _ = self.pipeline.set_state(gst::State::Null);
    }

    /// Stop after an error: halt the writer without waiting for the pipeline.
    pub fn abort(&mut self) {
        self.halt.store(true, Ordering::Release);
        self.join_writer(Duration::from_secs(2));
        let _ = self.pipeline.set_state(gst::State::Null);
    }

    fn join_writer(&mut self, timeout: Duration) {
        let Some(done) = self.done.take() else {
            return;
        };
        match done.recv_timeout(timeout) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.halt.store(true, Ordering::Release);
                let _ = done.recv_timeout(Duration::from_secs(2));
            }
        }
        match self.writer.take() {
            Some(writer) if writer.is_finished() => {
                let _ = writer.join();
            }
            Some(writer) => drop(writer),
            None => {}
        }
    }
}

fn make(name: &str) -> Result<gst::Element, String> {
    gst::ElementFactory::make(name)
        .build()
        .map_err(|err| format!("missing GStreamer element '{name}': {err}"))
}

fn poll_bus_error(pipeline: &gst::Pipeline) -> Option<String> {
    let bus = pipeline.bus()?;
    for message in bus.iter_timed(gst::ClockTime::ZERO) {
        if let gst::MessageView::Error(err) = message.view() {
            return Some(err.error().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::click_effect::RIPPLE_VISIBLE_DURATION_MS;
    use crate::recording::editor::model::VideoMetadata;
    use crate::recording::editor::sidecar::{
        CaptureRegion, ClickSample, CursorKind, PointerSample, PointerSidecar,
    };
    use std::path::PathBuf;

    fn setup(clicks: Vec<WarpClick>) -> WarpSetup {
        WarpSetup {
            source: PathBuf::from("/tmp/warp.mp4"),
            start: 0.0,
            end: 2.0,
            frame_rate: 30.0,
            width: 1000,
            height: 800,
            window: 1.0,
            clicks,
        }
    }

    #[test]
    fn uniform_state_uses_the_last_three_clicks_in_the_window() {
        let clicks = (0..4)
            .map(|i| WarpClick {
                x: i as f64,
                y: 0.0,
                t: i as f64 * 0.01,
            })
            .collect();
        let state = uniform_state(&setup(clicks), 0.5);
        // Slot 0 is the second-oldest of the four (the oldest is dropped).
        assert_eq!(state[3], 1.0);
        assert_eq!(state[7], 2.0);
        assert_eq!(state[11], 3.0);
    }

    #[test]
    fn uniform_state_carries_radius_and_bounce_per_click() {
        let state = uniform_state(
            &setup(vec![WarpClick {
                x: 100.0,
                y: 200.0,
                t: 0.0,
            }]),
            0.1,
        );
        assert_eq!(state[0], 1000.0);
        assert_eq!(state[1], 800.0);
        assert_eq!(state[2], 40.0);
        assert_eq!(state[3], 100.0);
        assert_eq!(state[4], 200.0);
        let expected_radius = ripple_radius_px(100.0, RIPPLE_PULL_SIZE_01, 1000.0) as f32;
        let expected_bounce = ripple_bounce_at_ms(100.0) as f32;
        assert!((state[5] - expected_radius).abs() < 1e-4);
        assert!((state[6] - expected_bounce).abs() < 1e-6);
    }

    #[test]
    fn uniform_state_is_empty_outside_the_lifetime() {
        let state = uniform_state(
            &setup(vec![WarpClick {
                x: 100.0,
                y: 200.0,
                t: 0.0,
            }]),
            RIPPLE_VISIBLE_DURATION_MS / 1000.0 + 0.001,
        );
        assert_eq!(&state[3..], &[0.0; 13]);
    }

    #[test]
    fn warp_setup_needs_a_visible_ripple_and_recorded_pointer() {
        let mut state = VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/warp-setup.mp4"),
            duration_seconds: 3.0,
            width: 640,
            height: 480,
            file_size_bytes: 1,
            has_audio: false,
            frame_rate: 30.0,
        });
        state.cursor.click_effect = ClickEffect::Ripple;
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 1.0,
            y: 1.0,
            kind: CursorKind::Default,
        });
        sidecar.clicks.push(ClickSample {
            t: 1.0,
            x: 100.0,
            y: 100.0,
            button: 1,
        });
        state.sidecar = Some(sidecar);
        assert!(WarpSetup::from_state(&state, 0.5, 1.5).is_some());
        assert!(WarpSetup::from_state(&state, 0.0, 0.5).is_none());
        assert!(WarpSetup::from_state(&state, 2.5, 3.0).is_none());
        state.cursor.click_effect = ClickEffect::Circle;
        assert!(WarpSetup::from_state(&state, 0.5, 1.5).is_none());
    }

    fn ffmpeg_available() -> bool {
        std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[test]
    fn the_gpu_warp_moves_a_known_fixture_pixel() {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};

        if !gl_warp_available() || !ffmpeg_available() {
            eprintln!("skipping: GStreamer GL or ffmpeg is not available");
            return;
        }
        let width = 640u32;
        let height = 480u32;
        let fps = 30.0f64;
        let age_ms = 100.0;
        let radius = ripple_radius_px(age_ms, RIPPLE_PULL_SIZE_01, width as f64);
        let bounce = ripple_bounce_at_ms(age_ms);
        let pull = bounce * width as f64 * 0.015;
        // Put the fixture on the ring so the pull is at its peak.
        let source_x = (320.0 + radius).round();

        let dir = std::env::temp_dir().join(format!("apexshot-gst-warp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let source = dir.join("source.mp4");
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                &format!("color=c=black:s={width}x{height}:r={fps}"),
                "-vf",
                &format!("drawbox=x={source_x}:y=240:w=1:h=1:color=white:t=fill"),
                "-t",
                "1",
                "-pix_fmt",
                "yuv420p",
                "-c:v",
                "libx264",
                source.to_str().unwrap(),
            ])
            .status()
            .expect("ffmpeg must generate the fixture");
        assert!(status.success());

        let setup = WarpSetup {
            source,
            start: 0.1,
            end: 0.2,
            frame_rate: fps,
            width,
            height,
            window: 1.0,
            clicks: vec![WarpClick {
                x: 320.0,
                y: 240.0,
                t: 0.0,
            }],
        };
        let mut warp = match ActiveGstWarp::start(&setup) {
            Ok(warp) => warp,
            Err(err) => {
                eprintln!("skipping: the GPU warp could not start: {err}");
                let _ = std::fs::remove_dir_all(&dir);
                return;
            }
        };
        let read_fd = warp.take_read_fd().expect("the warp must expose a pipe");
        let output = dir.join("out.yuv");
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "yuv420p",
            "-video_size",
            &format!("{width}x{height}"),
            "-framerate",
            &format!("{fps}"),
            "-i",
            "pipe:3",
            "-filter_complex",
            "null",
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "yuv420p",
            output.to_str().unwrap(),
        ]);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        unsafe {
            cmd.pre_exec(move || {
                let fd = read_fd.as_raw_fd();
                if fd != 3 && libc::dup2(fd, 3) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if fd != 3 && libc::close(fd) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn().expect("ffmpeg must run");
        let result = child.wait_with_output().expect("ffmpeg must finish");
        assert!(
            result.status.success(),
            "ffmpeg failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        warp.stop();

        let data = std::fs::read(&output).unwrap();
        let mut brightest = (0u32, 0u32, 0u8);
        for y in 0..height {
            for x in 0..width {
                let value = data[(y * width + x) as usize];
                if value > brightest.2 {
                    brightest = (x, y, value);
                }
            }
        }
        let expected = source_x + pull;
        assert!(
            (brightest.0 as f64 - expected).abs() <= 2.0,
            "the fixture pixel must land near {expected:.1}, got {brightest:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
