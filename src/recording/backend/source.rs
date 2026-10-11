use super::super::{RecordError, RecordResult, RecordingConfig, RecordingTarget};
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
    pub(super) cursor_metadata_enabled: bool,
    #[allow(dead_code)]
    pub(super) crop: Option<CropMargins>,
    pub(super) session: Option<WaylandCaptureSession>,
}

const RECORDING_RESTORE_TOKEN_FILES: &[&str] =
    &["wayland-record-screen.token", "wayland-record-area.token"];

// Dismissing the system dialog is a user cancel, not a failure — callers
// skip the "recording failed" notification for it.
fn portal_err(e: ashpd::Error) -> RecordError {
    if matches!(
        e,
        ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)
    ) {
        RecordError::Cancelled
    } else {
        RecordError::PortalError(e.to_string())
    }
}

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
    !crate::app_identity::portal_only()
        && std::env::var_os("APEXSHOT_KDE_NATIVE_SCREENCAST").is_some()
        && crate::backend::kde_screencast::is_kde_native_screencast_preferred()
}

fn portal_sources(
    target: RecordingTarget,
) -> ashpd::enumflags2::BitFlags<ashpd::desktop::screencast::SourceType> {
    use ashpd::desktop::screencast::SourceType;

    match target {
        RecordingTarget::Window => SourceType::Window.into(),
        RecordingTarget::Screen | RecordingTarget::Area => SourceType::Monitor.into(),
    }
}

fn validate_portal_sources(
    target: RecordingTarget,
    available: ashpd::enumflags2::BitFlags<ashpd::desktop::screencast::SourceType>,
) -> RecordResult<()> {
    use ashpd::desktop::screencast::SourceType;

    if target == RecordingTarget::Window && !available.contains(SourceType::Window) {
        return Err(RecordError::UnsupportedBackend(
            "The ScreenCast portal does not support window capture".into(),
        ));
    }
    Ok(())
}

fn validate_portal_stream(
    target: RecordingTarget,
    stream_count: usize,
    source_type: Option<ashpd::desktop::screencast::SourceType>,
) -> RecordResult<()> {
    use ashpd::desktop::screencast::SourceType;

    if target != RecordingTarget::Window {
        return if stream_count == 0 {
            Err(RecordError::PortalError("No streams returned".into()))
        } else {
            Ok(())
        };
    }

    if stream_count != 1 {
        return Err(RecordError::UnsupportedBackend(
            "The ScreenCast portal did not return a selected window stream".into(),
        ));
    }
    if source_type.is_some_and(|source| source != SourceType::Window) {
        return Err(RecordError::UnsupportedBackend(
            "The ScreenCast portal returned a monitor instead of a window".into(),
        ));
    }
    Ok(())
}

fn select_portal_cursor_mode(
    requested: ashpd::desktop::screencast::CursorMode,
    available: ashpd::enumflags2::BitFlags<ashpd::desktop::screencast::CursorMode>,
) -> RecordResult<ashpd::desktop::screencast::CursorMode> {
    use ashpd::desktop::screencast::CursorMode;

    if available.contains(requested) {
        return Ok(requested);
    }
    if requested == CursorMode::Metadata && available.contains(CursorMode::Embedded) {
        return Ok(CursorMode::Embedded);
    }
    if available.contains(CursorMode::Hidden) {
        return Ok(CursorMode::Hidden);
    }
    Err(RecordError::UnsupportedBackend(
        "ScreenCast portal does not offer a supported cursor mode".into(),
    ))
}

pub(in crate::recording) async fn get_wayland_source(
    config: &RecordingConfig,
) -> RecordResult<WaylandSource> {
    // Optional experimental path only (APEXSHOT_KDE_NATIVE_SCREENCAST=1).
    if config.target != RecordingTarget::Window && prefer_kde_native_screencast() {
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
            CursorMode, OpenPipeWireRemoteOptions, Screencast, SelectSourcesOptions,
            StartCastOptions,
        },
        CreateSessionOptions, PersistMode,
    };

    // Fedora / KDE / GNOME: system portal picks the source; ApexShot settings
    // still control countdown, audio, shortcuts, notifications, and save path.
    println!("Requesting Wayland ScreenCast session (system share UI)…");
    let wants_area_crop = config.target != RecordingTarget::Window
        && matches!(
            (config.x, config.y, config.width, config.height),
            (Some(_), Some(_), Some(w), Some(h)) if w > 0 && h > 0
        );
    let cursor_mode = if config.target == RecordingTarget::Window && config.cursor {
        CursorMode::Metadata
    } else if config.pointer_track_enabled() {
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
        target: RecordingTarget,
    ) -> RecordResult<(
        ashpd::desktop::screencast::Streams,
        OwnedPortalSession,
        OwnedFd,
        CursorMode,
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

        let available_cursor_modes = proxy.available_cursor_modes().await.map_err(portal_err)?;
        if target == RecordingTarget::Window {
            let available_source_types = proxy.available_source_types().await.map_err(|err| {
                RecordError::UnsupportedBackend(format!(
                    "Unable to confirm ScreenCast portal window capture support: {err}"
                ))
            })?;
            validate_portal_sources(target, available_source_types)?;
        }
        let requested_cursor_mode = cursor_mode;
        let cursor_mode = select_portal_cursor_mode(cursor_mode, available_cursor_modes)?;
        if requested_cursor_mode == CursorMode::Metadata && cursor_mode == CursorMode::Embedded {
            eprintln!(
                "[recording] Portal cursor metadata is unavailable; the window cursor will be embedded and not editable."
            );
            crate::utils::notify::desktop_notification(
                &crate::i18n::t("No editable cursor data was found."),
                &crate::i18n::t(
                    "The selected portal will embed the cursor in the video, so it cannot be edited separately.",
                ),
            );
        } else if requested_cursor_mode == CursorMode::Metadata && cursor_mode == CursorMode::Hidden
        {
            eprintln!(
                "[recording] Portal cursor metadata and embedded-cursor modes are unavailable; the window cursor will be hidden and no editable cursor data will be saved."
            );
        } else if requested_cursor_mode != cursor_mode {
            eprintln!(
                "[recording] Requested cursor mode {requested_cursor_mode:?} is unavailable from this portal; recording without an embedded cursor."
            );
        }

        let session = proxy
            .create_session(CreateSessionOptions::default())
            .await
            .map_err(portal_err)?;
        let setup_guard = OwnedPortalSession::new(conn, session);

        let source_types = portal_sources(target);

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
            .map_err(portal_err)?
            .response()
            .map_err(portal_err)?;

        match target {
            RecordingTarget::Window => println!("Please select a window to record..."),
            RecordingTarget::Area if wants_area_crop => {
                println!("Please select the monitor containing the recording area...");
            }
            RecordingTarget::Screen | RecordingTarget::Area => {
                println!("Please select a screen to record...");
            }
        }

        let response = proxy
            .start(setup_guard.as_session(), None, StartCastOptions::default())
            .await
            .map_err(portal_err)?
            .response()
            .map_err(portal_err)?;

        validate_portal_stream(
            target,
            response.streams().len(),
            response
                .streams()
                .first()
                .and_then(|stream| stream.source_type()),
        )?;

        let pipewire_fd = proxy
            .open_pipe_wire_remote(
                setup_guard.as_session(),
                OpenPipeWireRemoteOptions::default(),
            )
            .await
            .map_err(|e| RecordError::PortalError(e.to_string()))?;

        Ok((response, setup_guard, pipewire_fd, cursor_mode))
    }

    if config.target != RecordingTarget::Window {
        discard_stale_recording_restore_tokens();
    }
    let (response, portal_session, pipewire_fd, cursor_mode) =
        request_screencast(cursor_mode, wants_area_crop, config.target).await?;

    let stream = response.streams().first().ok_or_else(|| {
        RecordError::UnsupportedBackend(
            "The ScreenCast portal did not return a selected stream".into(),
        )
    })?;

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
        cursor_metadata_enabled: cursor_mode == CursorMode::Metadata,
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
        cursor_metadata_enabled: false,
        crop: None,
        session: Some(WaylandCaptureSession::KdeNative(Box::new(handle))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_dialog_dismissal_maps_to_cancel_not_failure() {
        assert!(matches!(
            portal_err(ashpd::Error::Response(
                ashpd::desktop::ResponseError::Cancelled
            )),
            RecordError::Cancelled
        ));
        assert!(matches!(
            portal_err(ashpd::Error::Response(ashpd::desktop::ResponseError::Other)),
            RecordError::PortalError(_)
        ));
    }

    #[test]
    fn window_recording_requests_only_portal_window_sources() {
        use ashpd::desktop::screencast::SourceType;

        assert!(portal_sources(RecordingTarget::Window).contains(SourceType::Window));
        assert!(!portal_sources(RecordingTarget::Window).contains(SourceType::Monitor));
        assert!(portal_sources(RecordingTarget::Screen).contains(SourceType::Monitor));
        assert!(!portal_sources(RecordingTarget::Screen).contains(SourceType::Window));
    }

    #[test]
    fn unsupported_or_mismatched_window_sources_fail_without_falling_back() {
        use ashpd::desktop::screencast::SourceType;

        assert!(matches!(
            validate_portal_sources(RecordingTarget::Window, SourceType::Monitor.into()),
            Err(RecordError::UnsupportedBackend(_))
        ));
        assert!(matches!(
            validate_portal_stream(RecordingTarget::Window, 0, None),
            Err(RecordError::UnsupportedBackend(_))
        ));
        assert!(matches!(
            validate_portal_stream(RecordingTarget::Window, 1, Some(SourceType::Monitor)),
            Err(RecordError::UnsupportedBackend(_))
        ));
        assert!(
            validate_portal_sources(RecordingTarget::Window, SourceType::Window.into()).is_ok()
        );
        assert!(
            validate_portal_stream(RecordingTarget::Window, 1, Some(SourceType::Window)).is_ok()
        );
        assert!(validate_portal_stream(RecordingTarget::Screen, 1, None).is_ok());
    }

    #[test]
    fn window_metadata_mode_falls_back_to_embedded_only_when_needed() {
        use ashpd::desktop::screencast::CursorMode;

        let metadata_and_embedded =
            ashpd::enumflags2::BitFlags::from(CursorMode::Metadata) | CursorMode::Embedded;
        assert_eq!(
            select_portal_cursor_mode(CursorMode::Metadata, metadata_and_embedded).unwrap(),
            CursorMode::Metadata
        );
        assert_eq!(
            select_portal_cursor_mode(CursorMode::Metadata, CursorMode::Embedded.into()).unwrap(),
            CursorMode::Embedded
        );
        assert_eq!(
            select_portal_cursor_mode(CursorMode::Metadata, CursorMode::Hidden.into()).unwrap(),
            CursorMode::Hidden
        );
        assert!(matches!(
            select_portal_cursor_mode(CursorMode::Metadata, ashpd::enumflags2::BitFlags::empty()),
            Err(RecordError::UnsupportedBackend(_))
        ));
    }
}
