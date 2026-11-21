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

    let requested = match target {
        RecordingTarget::Window => SourceType::Window,
        RecordingTarget::Screen | RecordingTarget::Area => SourceType::Monitor,
    };
    if !available.contains(requested) {
        let message = match target {
            RecordingTarget::Window => "The ScreenCast portal does not support window capture",
            RecordingTarget::Screen | RecordingTarget::Area => {
                "The ScreenCast portal does not support monitor capture"
            }
        };
        return Err(RecordError::UnsupportedBackend(message.into()));
    }
    Ok(())
}

/// How one share-dialog result should be handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScreenPickOutcome {
    /// The stream matches the recording target.
    Accept,
    /// A virtual display was shared for a screen/area recording. Close the
    /// session and ask again instead of recording a display that stays empty.
    ReAsk,
    /// The stream is the wrong kind for this recording; report it.
    Reject,
}

/// Ask this many times after a virtual display was picked, then give up.
const MAX_SCREEN_REASKS: u8 = 2;

/// Some portals (Plasma's share dialog, for one) always offer a "virtual
/// screen" entry beside the monitors ApexShot requested. A virtual display is
/// created empty, and its stream is not a monitor. Decide from the returned
/// stream type alone so the choice stays testable without a portal.
fn screen_pick_outcome(
    target: RecordingTarget,
    source_type: Option<ashpd::desktop::screencast::SourceType>,
) -> ScreenPickOutcome {
    use ashpd::desktop::screencast::SourceType;

    match (target, source_type) {
        (_, None) => ScreenPickOutcome::Accept,
        (RecordingTarget::Window, Some(SourceType::Window)) => ScreenPickOutcome::Accept,
        (RecordingTarget::Window, Some(_)) => ScreenPickOutcome::Reject,
        (_, Some(SourceType::Monitor)) => ScreenPickOutcome::Accept,
        (_, Some(SourceType::Virtual)) => ScreenPickOutcome::ReAsk,
        (_, Some(SourceType::Window)) => ScreenPickOutcome::Reject,
    }
}

/// One share-dialog round-trip.
enum ScreenCastAttempt {
    Started {
        response: ashpd::desktop::screencast::Streams,
        session: OwnedPortalSession,
        pipewire_fd: OwnedFd,
        cursor_mode: ashpd::desktop::screencast::CursorMode,
    },
    /// A virtual display was shared. The session is already closed so the
    /// compositor tears the virtual output down; the caller decides whether
    /// to ask again.
    NeedsAnotherPick,
}

fn validate_portal_stream(
    target: RecordingTarget,
    stream_count: usize,
    source_type: Option<ashpd::desktop::screencast::SourceType>,
) -> RecordResult<()> {
    if stream_count != 1 {
        if target != RecordingTarget::Window && stream_count == 0 {
            return Err(RecordError::PortalError("No streams returned".into()));
        }
        if target != RecordingTarget::Window {
            return Err(RecordError::UnsupportedBackend(
                "The ScreenCast portal did not return a single monitor stream".into(),
            ));
        }
        return Err(RecordError::UnsupportedBackend(
            "The ScreenCast portal did not return a selected window stream".into(),
        ));
    }

    match screen_pick_outcome(target, source_type) {
        ScreenPickOutcome::Accept => Ok(()),
        // A virtual display is handled by the dialog loop; reaching here means
        // the caller cannot ask again.
        ScreenPickOutcome::ReAsk | ScreenPickOutcome::Reject => {
            let message = if target == RecordingTarget::Window {
                "The sharing dialog returned a screen, but this recording needs a window"
            } else {
                "The sharing dialog returned a window, but this recording needs a screen"
            };
            Err(RecordError::UnsupportedBackend(message.into()))
        }
    }
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

/// Cursor mode for a portal ScreenCast request.
///
/// Metadata is the only mode that keeps the cursor editable: the compositor
/// leaves it out of the video and reports its position with each buffer. It
/// applies to windows as well as monitor streams, so screen and area
/// recordings get the same editable pointer on portals that advertise it.
/// GNOME's pointer-track path hides the OS cursor instead and draws its own
/// pointer in the editor, so it must keep `Hidden`.
fn portal_cursor_mode(config: &RecordingConfig) -> ashpd::desktop::screencast::CursorMode {
    use ashpd::desktop::screencast::CursorMode;

    if config.cursor && !config.pointer_track_enabled() {
        CursorMode::Metadata
    } else {
        CursorMode::Hidden
    }
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
    // GNOME's pointer-track path hides the OS cursor and draws ApexShot's own
    // pointer in the editor; GNOME ScreenCast often sends no usable
    // SPA_META_Cursor bitmap, so it never asks for Metadata. Every other
    // cursor-on session (window, screen, or area) asks for metadata and lets
    // `select_portal_cursor_mode` fall back when the portal lacks it.
    let cursor_mode = portal_cursor_mode(config);

    async fn request_screencast(
        cursor_mode: CursorMode,
        wants_area_crop: bool,
        target: RecordingTarget,
    ) -> RecordResult<ScreenCastAttempt> {
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
        let available_source_types = proxy.available_source_types().await.map_err(|err| {
            let source = if target == RecordingTarget::Window {
                "window"
            } else {
                "monitor"
            };
            RecordError::UnsupportedBackend(format!(
                "Unable to confirm ScreenCast portal {source} capture support: {err}"
            ))
        })?;
        validate_portal_sources(target, available_source_types)?;
        let requested_cursor_mode = cursor_mode;
        let cursor_mode = select_portal_cursor_mode(cursor_mode, available_cursor_modes)?;
        if requested_cursor_mode == CursorMode::Metadata && cursor_mode == CursorMode::Embedded {
            eprintln!(
                "[recording] Portal cursor metadata is unavailable; the cursor will be embedded and not editable."
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
                "[recording] Portal cursor metadata and embedded-cursor modes are unavailable; the cursor will be hidden and no editable cursor data will be saved."
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

        let stream_count = response.streams().len();
        let source_type = response
            .streams()
            .first()
            .and_then(|stream| stream.source_type());

        if stream_count == 1 && screen_pick_outcome(target, source_type) == ScreenPickOutcome::ReAsk
        {
            eprintln!(
                "[recording] The sharing dialog returned a virtual screen; closing it and asking for a real screen."
            );
            // Close the session before asking again so the compositor removes
            // the virtual display it created for this stream.
            drop(setup_guard);
            return Ok(ScreenCastAttempt::NeedsAnotherPick);
        }

        validate_portal_stream(target, stream_count, source_type)?;

        let pipewire_fd = proxy
            .open_pipe_wire_remote(
                setup_guard.as_session(),
                OpenPipeWireRemoteOptions::default(),
            )
            .await
            .map_err(|e| RecordError::PortalError(e.to_string()))?;

        Ok(ScreenCastAttempt::Started {
            response,
            session: setup_guard,
            pipewire_fd,
            cursor_mode,
        })
    }

    if config.target != RecordingTarget::Window {
        discard_stale_recording_restore_tokens();
    }
    let mut screen_reasks = 0u8;
    let (response, portal_session, pipewire_fd, cursor_mode) = loop {
        match request_screencast(cursor_mode, wants_area_crop, config.target).await? {
            ScreenCastAttempt::Started {
                response,
                session,
                pipewire_fd,
                cursor_mode,
            } => break (response, session, pipewire_fd, cursor_mode),
            ScreenCastAttempt::NeedsAnotherPick => {
                crate::utils::notify::desktop_notification(
                    &crate::i18n::t("Choose your screen"),
                    &crate::i18n::t(
                        "A virtual screen records an empty display. Pick your screen in the sharing dialog.",
                    ),
                );
                // Two re-asks are allowed; a third virtual pick gives up.
                if screen_reasks >= MAX_SCREEN_REASKS {
                    return Err(RecordError::UnsupportedBackend(
                        "Recording needs a real screen. Pick your screen in the sharing dialog."
                            .into(),
                    ));
                }
                screen_reasks += 1;
                println!("Pick a real screen in the sharing dialog...");
            }
        }
    };

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
    fn monitor_recording_requires_monitor_support_and_one_monitor_stream() {
        use ashpd::desktop::screencast::SourceType;

        for target in [RecordingTarget::Screen, RecordingTarget::Area] {
            assert!(matches!(
                validate_portal_sources(target, SourceType::Window.into()),
                Err(RecordError::UnsupportedBackend(_))
            ));
            assert!(validate_portal_sources(target, SourceType::Monitor.into()).is_ok());
            assert!(matches!(
                validate_portal_stream(target, 0, None),
                Err(RecordError::PortalError(_))
            ));
            assert!(matches!(
                validate_portal_stream(target, 2, Some(SourceType::Monitor)),
                Err(RecordError::UnsupportedBackend(_))
            ));
            assert!(matches!(
                validate_portal_stream(target, 1, Some(SourceType::Window)),
                Err(RecordError::UnsupportedBackend(_))
            ));
            assert!(validate_portal_stream(target, 1, Some(SourceType::Monitor)).is_ok());
            assert!(validate_portal_stream(target, 1, None).is_ok());
        }
    }

    #[test]
    fn virtual_screen_asks_again_for_screen_and_area() {
        use ashpd::desktop::screencast::SourceType;

        for target in [RecordingTarget::Screen, RecordingTarget::Area] {
            assert_eq!(
                screen_pick_outcome(target, Some(SourceType::Monitor)),
                ScreenPickOutcome::Accept
            );
            assert_eq!(
                screen_pick_outcome(target, Some(SourceType::Virtual)),
                ScreenPickOutcome::ReAsk
            );
            assert_eq!(
                screen_pick_outcome(target, Some(SourceType::Window)),
                ScreenPickOutcome::Reject
            );
            assert_eq!(screen_pick_outcome(target, None), ScreenPickOutcome::Accept);
        }
    }

    #[test]
    fn window_recording_pick_decisions_stay_unchanged() {
        use ashpd::desktop::screencast::SourceType;

        assert_eq!(
            screen_pick_outcome(RecordingTarget::Window, Some(SourceType::Window)),
            ScreenPickOutcome::Accept
        );
        assert_eq!(
            screen_pick_outcome(RecordingTarget::Window, Some(SourceType::Monitor)),
            ScreenPickOutcome::Reject
        );
        assert_eq!(
            screen_pick_outcome(RecordingTarget::Window, Some(SourceType::Virtual)),
            ScreenPickOutcome::Reject
        );
        assert_eq!(
            screen_pick_outcome(RecordingTarget::Window, None),
            ScreenPickOutcome::Accept
        );
    }

    #[test]
    fn mismatched_streams_name_the_sharing_dialog_choice() {
        use ashpd::desktop::screencast::SourceType;

        let screen_error =
            validate_portal_stream(RecordingTarget::Screen, 1, Some(SourceType::Window))
                .expect_err("a window stream cannot record a screen");
        assert!(matches!(
            screen_error,
            RecordError::UnsupportedBackend(ref message)
                if message == "The sharing dialog returned a window, but this recording needs a screen"
        ));

        let window_error =
            validate_portal_stream(RecordingTarget::Window, 1, Some(SourceType::Monitor))
                .expect_err("a screen stream cannot record a window");
        assert!(matches!(
            window_error,
            RecordError::UnsupportedBackend(ref message)
                if message == "The sharing dialog returned a screen, but this recording needs a window"
        ));
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

    #[test]
    fn screen_and_area_recordings_request_cursor_metadata() {
        use ashpd::desktop::screencast::CursorMode;

        for target in [RecordingTarget::Screen, RecordingTarget::Area] {
            let config = RecordingConfig {
                target,
                cursor: true,
                pointer_track: false,
                ..RecordingConfig::default()
            };
            assert_eq!(portal_cursor_mode(&config), CursorMode::Metadata);
        }
        let window = RecordingConfig {
            target: RecordingTarget::Window,
            cursor: true,
            pointer_track: false,
            ..RecordingConfig::default()
        };
        assert_eq!(portal_cursor_mode(&window), CursorMode::Metadata);
    }

    #[test]
    fn pointer_track_and_disabled_cursor_keep_the_os_cursor_hidden() {
        use ashpd::desktop::screencast::CursorMode;

        let pointer_track = RecordingConfig {
            target: RecordingTarget::Area,
            cursor: true,
            pointer_track: true,
            ..RecordingConfig::default()
        };
        assert_eq!(portal_cursor_mode(&pointer_track), CursorMode::Hidden);

        let cursor_off = RecordingConfig {
            target: RecordingTarget::Screen,
            cursor: false,
            pointer_track: false,
            ..RecordingConfig::default()
        };
        assert_eq!(portal_cursor_mode(&cursor_off), CursorMode::Hidden);
    }
}
