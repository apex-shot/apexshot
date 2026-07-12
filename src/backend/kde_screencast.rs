//! KDE Plasma native screencast via `zkde_screencast_unstable_v1`.
//!
//! This is the same protocol Spectacle uses for screen recording on KWin
//! (`VideoPlatformWayland` + `Screencasting`). Unlike the XDG ScreenCast
//! portal path, it:
//!
//! - does **not** show a "Share this screen?" permission dialog
//! - creates a PipeWire node directly on the session graph
//! - requires desktop-entry authorization:
//!   `X-KDE-Wayland-Interfaces=zkde_screencast_unstable_v1`
//!
//! Analogous to the wlroots/Hyprland path that uses `wf-recorder` /
//! `wlr-screencopy` instead of the portal.

use std::time::{Duration, Instant};

use wayland_client::{
    protocol::{wl_output, wl_registry},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols_plasma::screencast::v1::client::{
    zkde_screencast_stream_unstable_v1, zkde_screencast_unstable_v1,
};

use super::kde_screenshot;

/// Keep the Wayland connection + stream proxies alive so KWin does not tear
/// down the PipeWire feed while we are still recording.
pub struct KdeScreencastHandle {
    node_id: u32,
    /// Logical size for region streams (0×0 when unknown / full output).
    width: u32,
    height: u32,
    // Drop order: stream first, then manager/outputs, queue, connection.
    // These fields exist purely to keep KWin's stream alive — not read.
    #[allow(dead_code)]
    stream: zkde_screencast_stream_unstable_v1::ZkdeScreencastStreamUnstableV1,
    #[allow(dead_code)]
    screencast: zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1,
    #[allow(dead_code)]
    outputs: Vec<wl_output::WlOutput>,
    // Held so queue-associated proxies stay valid for the recording lifetime.
    _event_queue: EventQueue<AppState>,
    _state: AppState,
    _conn: Connection,
}

impl KdeScreencastHandle {
    pub fn node_id(&self) -> u32 {
        self.node_id
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

// Stream proxy Drop sends the protocol `close` destructor request to KWin.

#[derive(Debug, Clone, Copy)]
pub enum KdeScreencastTarget {
    /// Full single output (primary / first advertised).
    Output,
    /// Logical workspace region (Spectacle `stream_region`).
    Region {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum KdeScreencastError {
    #[error("Not a KDE Plasma Wayland session")]
    NotKdeWayland,

    #[error("Could not connect to Wayland display: {0}")]
    Connect(String),

    #[error(
        "KWin zkde_screencast_unstable_v1 is not available \
         (need X-KDE-Wayland-Interfaces=zkde_screencast_unstable_v1 and Plasma/KWin screencast support)"
    )]
    ProtocolUnavailable,

    #[error("No Wayland outputs found")]
    NoOutputs,

    #[error("Region capture requires zkde_screencast_unstable_v1 version ≥ 3")]
    RegionUnsupported,

    #[error("KWin screencast failed: {0}")]
    Failed(String),

    #[error("Timed out waiting for KWin screencast stream")]
    Timeout,
}

pub type KdeScreencastResult<T> = Result<T, KdeScreencastError>;

pub fn is_kde_native_screencast_preferred() -> bool {
    if std::env::var_os("APEXSHOT_FORCE_PORTAL_RECORDING").is_some() {
        return false;
    }
    kde_screenshot::is_kde_wayland_session()
}

/// Probe whether the compositor advertises `zkde_screencast_unstable_v1`.
///
/// This may return false when the app is not authorized via
/// `X-KDE-Wayland-Interfaces` even if KWin supports the protocol.
pub fn is_available() -> bool {
    if !kde_screenshot::is_kde_wayland_session() {
        return false;
    }
    match Connection::connect_to_env() {
        Ok(conn) => probe_protocol(&conn).is_some(),
        Err(_) => false,
    }
}

fn probe_protocol(conn: &Connection) -> Option<u32> {
    let mut state = ProbeState::default();
    let mut eq = conn.new_event_queue();
    let qh = eq.handle();
    conn.display().get_registry(&qh, ());
    let _ = eq.roundtrip(&mut state);
    state.version
}

#[derive(Default)]
struct ProbeState {
    version: Option<u32>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for ProbeState {
    fn event(
        state: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            interface, version, ..
        } = event
        {
            if interface == "zkde_screencast_unstable_v1" {
                state.version = Some(version);
            }
        }
    }
}

/// Start a KDE-native PipeWire screencast feed. Returns a handle that must be
/// kept alive for the duration of recording.
pub fn start_stream(
    target: KdeScreencastTarget,
    include_cursor: bool,
) -> KdeScreencastResult<KdeScreencastHandle> {
    if !kde_screenshot::is_kde_wayland_session() {
        return Err(KdeScreencastError::NotKdeWayland);
    }

    let conn =
        Connection::connect_to_env().map_err(|e| KdeScreencastError::Connect(format!("{e}")))?;

    let mut state = AppState::default();
    let mut event_queue: EventQueue<AppState> = conn.new_event_queue();
    let qh = event_queue.handle();

    conn.display().get_registry(&qh, ());
    event_queue
        .roundtrip(&mut state)
        .map_err(|e| KdeScreencastError::Connect(format!("registry roundtrip: {e}")))?;

    // Geometry events for outputs.
    event_queue
        .roundtrip(&mut state)
        .map_err(|e| KdeScreencastError::Connect(format!("output geometry roundtrip: {e}")))?;

    if state.screencast.is_none() {
        return Err(KdeScreencastError::ProtocolUnavailable);
    }
    if state.outputs.is_empty() {
        return Err(KdeScreencastError::NoOutputs);
    }

    let version = state.screencast_version;
    let pointer = if include_cursor {
        zkde_screencast_unstable_v1::Pointer::Embedded
    } else {
        zkde_screencast_unstable_v1::Pointer::Hidden
    };

    let (width, height) = match target {
        KdeScreencastTarget::Output => (0, 0),
        KdeScreencastTarget::Region { width, height, .. } => (width, height),
    };

    // Create the stream while the manager still lives in `state` is awkward;
    // clone the proxy reference by taking ownership temporarily.
    let screencast = state
        .screencast
        .clone()
        .ok_or(KdeScreencastError::ProtocolUnavailable)?;

    let stream = match target {
        KdeScreencastTarget::Output => {
            let output = pick_output_index(&state);
            let output = &state.outputs[output];
            screencast.stream_output(output, pointer.into(), &qh, ())
        }
        KdeScreencastTarget::Region {
            x,
            y,
            width: w,
            height: h,
        } => {
            if version < 3 {
                return Err(KdeScreencastError::RegionUnsupported);
            }
            // scale = 0.0 ⇒ compositor picks highest scale (protocol v5+).
            let scale = if version >= 5 { 0.0 } else { 1.0 };
            screencast.stream_region(x, y, w, h, scale, pointer.into(), &qh, ())
        }
    };

    // Wait for `created` (node id) or `failed`.
    let deadline = Instant::now() + Duration::from_secs(5);
    while state.node_id.is_none() && state.error.is_none() {
        if Instant::now() > deadline {
            return Err(KdeScreencastError::Timeout);
        }
        event_queue
            .blocking_dispatch(&mut state)
            .map_err(|e| KdeScreencastError::Connect(format!("dispatch while waiting: {e}")))?;
    }

    if let Some(err) = state.error.take() {
        return Err(KdeScreencastError::Failed(err));
    }

    let node_id = state
        .node_id
        .ok_or_else(|| KdeScreencastError::Failed("stream created without node id".into()))?;

    eprintln!(
        "[kde-screencast] stream ready: node_id={node_id} target={target:?} cursor={include_cursor}"
    );

    // Move owned outputs out; manager stays referenced by stream + our field.
    let outputs = std::mem::take(&mut state.outputs);
    // Keep a live manager proxy (cloned earlier).
    let screencast = state.screencast.take().unwrap_or(screencast);

    Ok(KdeScreencastHandle {
        node_id,
        width,
        height,
        stream,
        screencast,
        outputs,
        _event_queue: event_queue,
        _state: state,
        _conn: conn,
    })
}

fn pick_output_index(state: &AppState) -> usize {
    state
        .outputs
        .iter()
        .enumerate()
        .min_by_key(|(_, o)| {
            state
                .output_infos
                .iter()
                .find(|info| info.id == o.id().protocol_id())
                .map(|info| (info.x, info.y))
                .unwrap_or((0, 0))
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

// ─── Wayland state ───────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct OutputInfo {
    id: u32,
    x: i32,
    y: i32,
}

#[derive(Default)]
struct AppState {
    screencast: Option<zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1>,
    screencast_version: u32,
    outputs: Vec<wl_output::WlOutput>,
    output_infos: Vec<OutputInfo>,
    node_id: Option<u32>,
    error: Option<String>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for AppState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "zkde_screencast_unstable_v1" => {
                    let ver = version.min(6);
                    let sc = registry
                        .bind::<zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1, _, _>(
                            name,
                            ver,
                            qh,
                            (),
                        );
                    state.screencast = Some(sc);
                    state.screencast_version = ver;
                }
                "wl_output" => {
                    let output =
                        registry.bind::<wl_output::WlOutput, _, _>(name, version.min(4), qh, ());
                    state.outputs.push(output);
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for AppState {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = output.id().protocol_id();
        let ensure = |state: &mut AppState| {
            if let Some(idx) = state.output_infos.iter().position(|info| info.id == id) {
                idx
            } else {
                state.output_infos.push(OutputInfo { id, x: 0, y: 0 });
                state.output_infos.len() - 1
            }
        };

        if let wl_output::Event::Geometry { x, y, .. } = event {
            let idx = ensure(state);
            state.output_infos[idx].x = x;
            state.output_infos[idx].y = y;
        }
    }
}

impl Dispatch<zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1,
        _: zkde_screencast_unstable_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zkde_screencast_stream_unstable_v1::ZkdeScreencastStreamUnstableV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &zkde_screencast_stream_unstable_v1::ZkdeScreencastStreamUnstableV1,
        event: zkde_screencast_stream_unstable_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zkde_screencast_stream_unstable_v1::Event::Created { node } => {
                state.node_id = Some(node);
            }
            zkde_screencast_stream_unstable_v1::Event::Failed { error } => {
                state.error = Some(error);
            }
            zkde_screencast_stream_unstable_v1::Event::Closed => {
                if state.node_id.is_none() {
                    state.error = Some("screencast stream closed before creation".into());
                }
            }
            _ => {
                // Future protocol events (e.g. object serial) are ignored;
                // PipeWireCapture still targets the node id from `created`.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_only_on_kde_wayland() {
        // Pure unit check of the env helper gate used by the recorder.
        assert!(!is_kde_native_screencast_preferred() || kde_screenshot::is_kde_wayland_session());
    }
}
