use super::super::{RecordError, RecordResult, RecordingConfig};
use super::crop::{wayland_area_crop_or_full, CropMargins};
use super::session::{OwnedPortalSession, WaylandCaptureSession};
use std::os::fd::OwnedFd;

#[derive(Debug)]
pub(in crate::recording) struct WaylandSource {
    pub(super) node_id: u32,
    /// Portal remote FD. `None` for KDE-native streams that publish on the
    /// default session PipeWire socket.
    pub(super) pipewire_fd: Option<OwnedFd>,
    pub(super) stream_width: u32,
    pub(super) stream_height: u32,
    #[allow(dead_code)]
    pub(super) crop: Option<CropMargins>,
    pub(super) session: Option<WaylandCaptureSession>,
}

const RECORDING_RESTORE_TOKEN_FILES: &[&str] =
    &["wayland-record-screen.token", "wayland-record-area.token"];

/// Older builds saved ScreenCast restore tokens and used `PersistMode::ExplicitlyRevoked`,
/// which shows GNOME's "Remember this choice" checkbox and locks later recordings
/// to the last window/screen. Recording never restores now.
fn discard_stale_recording_restore_tokens() {
    let Some(mut dir) = dirs::cache_dir() else {
        return;
    };
    dir.push("apexshot");
    discard_recording_restore_tokens_in(&dir);
}

pub(super) fn discard_recording_restore_tokens_in(dir: &std::path::Path) {
    for name in RECORDING_RESTORE_TOKEN_FILES {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// On KDE Plasma, the reliable capture path is the desktop portal UI.
/// Native `zkde_screencast` is opt-in only — it needs compositor authorization
/// that often fails for third-party apps and caused confusing dual-UI / crashes.
fn prefer_kde_native_screencast() -> bool {
    std::env::var_os("APEXSHOT_KDE_NATIVE_SCREENCAST").is_some()
        && crate::backend::kde_screencast::is_kde_native_screencast_preferred()
}

pub(in crate::recording) async fn get_wayland_source(
    config: &RecordingConfig,
) -> RecordResult<WaylandSource> {
    // Optional experimental path only (APEXSHOT_KDE_NATIVE_SCREENCAST=1).
    if prefer_kde_native_screencast() {
        match get_kde_wayland_source(config) {
            Ok(source) => {
                println!("Using KDE-native zkde_screencast (APEXSHOT_KDE_NATIVE_SCREENCAST).");
                return Ok(source);
            }
            Err(err) => {
                eprintln!(
                    "[recording] KDE-native screencast failed ({err}); using ScreenCast portal."
                );
            }
        }
    }

    use ashpd::desktop::{
        screencast::{
            CursorMode, OpenPipeWireRemoteOptions, Screencast, SelectSourcesOptions, SourceType,
            StartCastOptions,
        },
        CreateSessionOptions, PersistMode,
    };

    // Fedora / KDE / GNOME: system portal picks the source; ApexShot settings
    // still control countdown, audio, shortcuts, notifications, and save path.
    println!("Requesting Wayland ScreenCast session (system share UI)…");
    let wants_area_crop = matches!(
        (config.x, config.y, config.width, config.height),
        (Some(_), Some(_), Some(w), Some(h)) if w > 0 && h > 0
    );
    let cursor_mode = if config.pointer_track {
        // GNOME pointer sidecar path: hide the OS cursor and draw ApexShot's
        // pointer in the editor. Do not switch to Metadata — GNOME ScreenCast
        // often sends no usable SPA_META_Cursor bitmap.
        CursorMode::Hidden
    } else if config.cursor {
        // Ask the portal/compositor to embed the cursor in the video stream.
        // Metadata mode is optional and GNOME often does not provide usable
        // cursor bitmap metadata for ScreenCast streams, which made the
        // "show cursor" setting appear ignored in PipeWire recordings.
        CursorMode::Embedded
    } else {
        CursorMode::Hidden
    };

    async fn request_screencast(
        cursor_mode: CursorMode,
        wants_area_crop: bool,
    ) -> RecordResult<(
        ashpd::desktop::screencast::Streams,
        OwnedPortalSession,
        OwnedFd,
    )> {
        let _portal_identity = crate::utils::desktop_env::scoped_portal_capture_identity();

        // Recording-private portal connection: a Session.Close wedges the
        // zbus machinery of the connection it traveled on, so a shared
        // process-global connection would hang every recording after the
        // first. See OwnedPortalSession.
        let conn = zbus::Connection::session()
            .await
            .map_err(|e| RecordError::PortalError(e.to_string()))?;

        let proxy = Screencast::with_connection(conn.clone())
            .await
            .map_err(|e| RecordError::PortalError(e.to_string()))?;

        let session = proxy
            .create_session(CreateSessionOptions::default())
            .await
            .map_err(|e| RecordError::PortalError(e.to_string()))?;
        let setup_guard = OwnedPortalSession::new(conn, session);

        let source_types = if wants_area_crop {
            SourceType::Monitor.into()
        } else {
            SourceType::Monitor | SourceType::Window
        };

        // PersistMode::DoNot hides GNOME's "Remember this choice" checkbox and
        // never restores a previous window/screen. Pick a source every time.
        proxy
            .select_sources(
                setup_guard.as_session(),
                SelectSourcesOptions::default()
                    .set_cursor_mode(cursor_mode)
                    .set_sources(source_types)
                    .set_multiple(false)
                    .set_persist_mode(PersistMode::DoNot),
            )
            .await
            .map_err(|e| RecordError::PortalError(e.to_string()))?
            .response()
            .map_err(|e| RecordError::PortalError(e.to_string()))?;

        if wants_area_crop {
            println!("Please select the monitor containing the recording area...");
        } else {
            println!("Please select a screen or window to record...");
        }

        let response = proxy
            .start(setup_guard.as_session(), None, StartCastOptions::default())
            .await
            .map_err(|e| RecordError::PortalError(e.to_string()))?
            .response()
            .map_err(|e| RecordError::PortalError(e.to_string()))?;

        let pipewire_fd = proxy
            .open_pipe_wire_remote(
                setup_guard.as_session(),
                OpenPipeWireRemoteOptions::default(),
            )
            .await
            .map_err(|e| RecordError::PortalError(e.to_string()))?;

        Ok((response, setup_guard, pipewire_fd))
    }

    discard_stale_recording_restore_tokens();
    let (response, portal_session, pipewire_fd) =
        request_screencast(cursor_mode, wants_area_crop).await?;

    let stream = response
        .streams()
        .first()
        .ok_or_else(|| RecordError::PortalError("No streams returned".into()))?;

    let node_id = stream.pipe_wire_node_id();
    println!("Got PipeWire Node ID: {}", node_id);
    println!(
        "Wayland stream metadata: position={:?} size={:?} type={:?}",
        stream.position(),
        stream.size(),
        stream.source_type()
    );

    let (stream_width, stream_height) = stream
        .size()
        .map(|(w, h)| (w as u32, h as u32))
        .unwrap_or((0, 0));

    let crop = if wants_area_crop {
        let size = (stream_width as i32, stream_height as i32);
        let selection = (
            config.x.expect("checked above"),
            config.y.expect("checked above"),
            config.width.expect("checked above"),
            config.height.expect("checked above"),
        );
        // KDE portal frequently omits stream position. Infer it or fall back to
        // full-stream capture instead of aborting after the user already confirmed.
        wayland_area_crop_or_full(stream.position(), size, selection)
    } else {
        None
    };

    Ok(WaylandSource {
        node_id,
        pipewire_fd: Some(pipewire_fd),
        stream_width,
        stream_height,
        crop,
        session: Some(WaylandCaptureSession::Portal(portal_session)),
    })
}

/// KWin screencast: `zkde_screencast_unstable_v1` → PipeWire node
/// on the default session socket. No xdg-desktop-portal dialog.
fn get_kde_wayland_source(config: &RecordingConfig) -> RecordResult<WaylandSource> {
    use crate::backend::kde_screencast::{start_stream, KdeScreencastTarget};

    let target = match (config.x, config.y, config.width, config.height) {
        (Some(x), Some(y), Some(w), Some(h)) if w > 0 && h > 0 => KdeScreencastTarget::Region {
            x,
            y,
            width: w,
            height: h,
        },
        _ => KdeScreencastTarget::Output,
    };

    let handle = start_stream(target, config.cursor)
        .map_err(|e| RecordError::PortalError(format!("KDE-native screencast failed: {e}")))?;

    let stream_width = handle.width();
    let stream_height = handle.height();
    let node_id = handle.node_id();

    // Region streams are already cropped by KWin — no client-side crop.
    Ok(WaylandSource {
        node_id,
        pipewire_fd: None,
        stream_width,
        stream_height,
        crop: None,
        session: Some(WaylandCaptureSession::KdeNative(Box::new(handle))),
    })
}
