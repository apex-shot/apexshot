use super::{CaptureData, DisplayError, DisplayResult, PixelFormat};
use ashpd::desktop::{
    screencast::{
        CursorMode, OpenPipeWireRemoteOptions, Screencast, SelectSourcesOptions, SourceType,
        StartCastOptions,
    },
    CreateSessionOptions, PersistMode, ResponseError, Session,
};
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, Write},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, thiserror::Error)]
pub enum StillError {
    #[error("Screenshot capture cancelled")]
    Cancelled,
    #[error(transparent)]
    Portal(#[from] ashpd::Error),
    #[error(transparent)]
    Capture(#[from] DisplayError),
}

fn portal_error(error: ashpd::Error) -> StillError {
    match error {
        ashpd::Error::Response(ResponseError::Cancelled) => StillError::Cancelled,
        error => StillError::Portal(error),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StillMonitor {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl StillMonitor {
    fn matches_stream(self, position: Option<(i32, i32)>, size: Option<(i32, i32)>) -> bool {
        position.is_none_or(|position| position == (self.x, self.y))
            && size.is_none_or(|size| size == (self.width, self.height))
    }
}

fn token_path(key: &str, monitor: StillMonitor) -> Option<PathBuf> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let app_id = crate::app_identity::app_id();
    let digest = Sha256::digest(format!(
        "{app_id}\n{desktop}\n{key}\n{},{},{},{}",
        monitor.x, monitor.y, monitor.width, monitor.height
    ));
    Some(
        dirs::cache_dir()?
            .join("apexshot")
            .join(format!("still-monitor-{:x}.token", digest)),
    )
}

fn select_options(version: u32, cursor: bool, token: Option<&str>) -> SelectSourcesOptions {
    let options = SelectSourcesOptions::default()
        .set_sources(ashpd::enumflags2::BitFlags::from(SourceType::Monitor))
        .set_multiple(false);
    let options = if version >= 2 {
        options.set_cursor_mode(if cursor {
            CursorMode::Embedded
        } else {
            CursorMode::Hidden
        })
    } else {
        options
    };
    if version >= 4 {
        options
            .set_persist_mode(PersistMode::ExplicitlyRevoked)
            .set_restore_token(token)
    } else {
        options
    }
}

pub struct PortalStillSession {
    session: Session<Screencast>,
    portal: Screencast,
    node: u32,
    pub position: Option<(i32, i32)>,
    pub size: Option<(i32, i32)>,
}

impl PortalStillSession {
    pub async fn prepare(
        key: &str,
        monitor: StillMonitor,
        cursor: bool,
    ) -> Result<Self, StillError> {
        if monitor.width <= 0 || monitor.height <= 0 {
            return Err(
                DisplayError::InvalidArea("Invalid screenshot monitor bounds".into()).into(),
            );
        }
        let _identity = crate::utils::desktop_env::scoped_portal_capture_identity();
        let path = token_path(key, monitor);
        let token = path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .filter(|token| !token.is_empty());
        match Self::prepare_once(monitor, cursor, token.as_deref(), path.as_deref()).await {
            Ok(session) => Ok(session),
            Err(error) => {
                if matches!(&error, StillError::Capture(DisplayError::CaptureError(_))) {
                    return Err(error);
                }
                if let Some(path) = path.as_ref() {
                    let _ = std::fs::remove_file(path);
                }
                if token.is_none() || matches!(error, StillError::Cancelled) {
                    return Err(error);
                }
                Self::prepare_once(monitor, cursor, None, path.as_deref()).await
            }
        }
    }

    async fn prepare_once(
        monitor: StillMonitor,
        cursor: bool,
        token: Option<&str>,
        token_path: Option<&std::path::Path>,
    ) -> Result<Self, StillError> {
        let connection = zbus::Connection::session()
            .await
            .map_err(ashpd::Error::from)?;
        if !crate::app_identity::portal_only() {
            let app_id =
                ashpd::AppID::try_from(crate::app_identity::app_id()).map_err(portal_error)?;
            if let Err(error) =
                ashpd::register_host_app_with_connection(connection.clone(), app_id).await
            {
                eprintln!("[capture] Host portal identity registration unavailable: {error}");
            }
        }
        let portal = Screencast::with_connection(connection)
            .await
            .map_err(portal_error)?;
        if !portal
            .available_source_types()
            .await
            .map_err(portal_error)?
            .contains(SourceType::Monitor)
        {
            return Err(DisplayError::UnsupportedBackend(
                "The desktop portal cannot share monitors".into(),
            )
            .into());
        }
        let cursor = if cursor && portal.version() >= 2 {
            portal
                .available_cursor_modes()
                .await
                .map_err(portal_error)?
                .contains(CursorMode::Embedded)
        } else {
            false
        };
        let session = portal
            .create_session(CreateSessionOptions::default())
            .await
            .map_err(portal_error)?;
        let result = async {
            portal
                .select_sources(&session, select_options(portal.version(), cursor, token))
                .await
                .map_err(portal_error)?
                .response()
                .map_err(portal_error)?;
            let response = portal
                .start(&session, None, StartCastOptions::default())
                .await
                .map_err(portal_error)?
                .response()
                .map_err(portal_error)?;
            if response.streams().len() != 1 {
                return Err(DisplayError::PortalError("Expected one shared monitor".into()).into());
            }
            let stream = &response.streams()[0];
            if stream.source_type().is_some_and(|source| source != SourceType::Monitor)
                || !monitor.matches_stream(stream.position(), stream.size())
            {
                return Err(DisplayError::PortalError(
                    "The shared source does not match the selected display. Share that display and try again.".into(),
                ).into());
            }
            if let Some(path) = token_path {
                let _ = std::fs::remove_file(path);
                if let Some(token) = response.restore_token().filter(|token| !token.is_empty()) {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Ok(mut file) = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(path)
                    {
                        let _ = file.write_all(token.as_bytes());
                    }
                }
            }
            let _fd = portal
                .open_pipe_wire_remote(&session, OpenPipeWireRemoteOptions::default())
                .await
                .map_err(|error| StillError::Capture(DisplayError::CaptureError(format!(
                    "Screen sharing was approved, but the PipeWire remote is unavailable: {error}"
                ))))?;
            tokio::time::sleep(Duration::from_millis(650)).await;
            Ok((stream.pipe_wire_node_id(), stream.position(), stream.size()))
        }.await;
        match result {
            Ok((node, position, size)) => Ok(Self {
                session,
                portal,
                node,
                position,
                size,
            }),
            Err(error) => {
                let _ = tokio::time::timeout(Duration::from_secs(2), session.close()).await;
                Err(error)
            }
        }
    }

    pub async fn capture(&self) -> DisplayResult<CaptureData> {
        let fd = self
            .portal
            .open_pipe_wire_remote(&self.session, OpenPipeWireRemoteOptions::default())
            .await
            .map_err(|error| {
                DisplayError::PortalError(format!("Failed to open PipeWire remote: {error}"))
            })?;
        let frame = crate::pipewire_engine::capture_single_frame_with_min_frames(
            fd,
            self.node,
            Duration::from_secs(5),
            3,
        )
        .map_err(|error| {
            DisplayError::CaptureError(format!("PipeWire still capture failed: {error}"))
        })?;
        Ok(CaptureData::new(
            frame.pixels,
            frame.width,
            frame.height,
            PixelFormat::RGBA32,
        ))
    }

    pub async fn close(self) {
        let _ = tokio::time::timeout(Duration::from_secs(2), self.session.close()).await;
    }
}

fn write_message(message: serde_json::Value) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{message}")?;
    stdout.flush()
}

fn capture_to_png(capture: CaptureData) -> anyhow::Result<serde_json::Value> {
    let image = image::RgbaImage::from_raw(capture.width, capture.height, capture.pixels)
        .ok_or_else(|| anyhow::anyhow!("Invalid screenshot pixels"))?;
    let path = std::env::temp_dir().join(format!(
        "apexshot-portal-{}-{}.png",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    if let Err(error) =
        image::DynamicImage::ImageRgba8(image).write_to(&mut file, image::ImageOutputFormat::Png)
    {
        let _ = std::fs::remove_file(&path);
        return Err(error.into());
    }
    Ok(serde_json::json!({"path": path, "width": capture.width, "height": capture.height}))
}

enum StillSource {
    Native(StillMonitor),
    Portal(PortalStillSession),
}

fn named_monitor(
    key: &str,
    requested: StillMonitor,
    monitors: impl IntoIterator<Item = (String, StillMonitor)>,
) -> StillMonitor {
    let Some((connector, _)) = key.split_once('@') else {
        return requested;
    };
    monitors
        .into_iter()
        .find_map(|(name, monitor)| (name == connector).then_some(monitor))
        .unwrap_or(requested)
}

fn helper_logical_monitor(key: &str, requested: StillMonitor) -> StillMonitor {
    use gtk4::prelude::*;
    if std::env::var("XDG_SESSION_TYPE").is_ok_and(|session| session == "x11")
        || std::env::var_os("WAYLAND_DISPLAY").is_none()
    {
        return requested;
    }
    std::env::set_var("GDK_BACKEND", "wayland");
    if gtk4::init().is_err() {
        return requested;
    }
    let Some(display) = gtk4::gdk::Display::default() else {
        return requested;
    };
    let monitors = display.monitors();
    named_monitor(
        key,
        requested,
        (0..monitors.n_items()).filter_map(|index| {
            let monitor = monitors
                .item(index)?
                .downcast::<gtk4::gdk::Monitor>()
                .ok()?;
            let connector = monitor.connector()?.to_string();
            let rect = monitor.geometry();
            Some((
                connector,
                StillMonitor {
                    x: rect.x(),
                    y: rect.y(),
                    width: rect.width(),
                    height: rect.height(),
                },
            ))
        }),
    )
}

impl StillSource {
    fn capture(&self, runtime: &tokio::runtime::Runtime) -> DisplayResult<CaptureData> {
        match self {
            Self::Native(monitor) => {
                let capture = super::WaylandBackend::capture_monitor_via_native_screencopy_at(
                    Some((monitor.x, monitor.y)),
                )
                .unwrap_or_else(|| {
                    Err(DisplayError::CaptureError(
                        "The selected display is no longer available for native capture".into(),
                    ))
                })?;
                if (capture.output_origin_x, capture.output_origin_y) != (monitor.x, monitor.y) {
                    return Err(DisplayError::CaptureError(
                        "The selected display changed during capture".into(),
                    ));
                }
                Ok(capture)
            }
            Self::Portal(session) => runtime.block_on(session.capture()),
        }
    }
}

pub fn run_internal_helper(args: &[String]) -> anyhow::Result<()> {
    if args.len() != 8 {
        anyhow::bail!("Invalid portal still helper arguments");
    }
    let monitor = StillMonitor {
        x: args[3].parse()?,
        y: args[4].parse()?,
        width: args[5].parse()?,
        height: args[6].parse()?,
    };
    let requested_monitor = monitor;
    let monitor = helper_logical_monitor(&args[2], monitor);
    let runtime = tokio::runtime::Runtime::new()?;
    let native = !super::WaylandBackend::should_force_screenshot_portal_first()
        && super::WaylandBackend::capture_monitor_via_native_screencopy_at(Some((
            monitor.x, monitor.y,
        )))
        .is_some_and(|capture| {
            capture.is_ok_and(|capture| {
                (capture.output_origin_x, capture.output_origin_y) == (monitor.x, monitor.y)
            })
        });
    let source = if native {
        StillSource::Native(monitor)
    } else {
        match runtime.block_on(PortalStillSession::prepare(
            &args[2],
            monitor,
            args[7] == "1",
        )) {
            Ok(session) => StillSource::Portal(session),
            Err(error) => {
                write_message(
                    serde_json::json!({"error": error.to_string(), "cancelled": matches!(error, StillError::Cancelled)}),
                )?;
                return Err(error.into());
            }
        }
    };
    let result = (|| -> anyhow::Result<()> {
        let position = (requested_monitor.x, requested_monitor.y);
        let size = (requested_monitor.width, requested_monitor.height);
        write_message(serde_json::json!({"ready": true, "position": position, "size": size}))?;
        for command in std::io::stdin().lock().lines() {
            if command? != "capture" {
                break;
            }
            match source
                .capture(&runtime)
                .map_err(anyhow::Error::from)
                .and_then(capture_to_png)
            {
                Ok(message) => write_message(message)?,
                Err(error) => {
                    write_message(
                        serde_json::json!({"error": error.to_string(), "cancelled": false}),
                    )?;
                    return Err(error);
                }
            }
        }
        Ok(())
    })();
    if let StillSource::Portal(session) = source {
        runtime.block_on(session.close());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(x: i32) -> StillMonitor {
        StillMonitor {
            x,
            y: -100,
            width: 1920,
            height: 1080,
        }
    }

    #[test]
    fn monitor_grants_are_scoped_to_output_and_topology() {
        assert_ne!(
            token_path("same-layout", monitor(0)),
            token_path("same-layout", monitor(1920))
        );
        assert_ne!(
            token_path("old-layout", monitor(0)),
            token_path("new-layout", monitor(0))
        );
        assert!(token_path("display/name", monitor(0))
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("still-monitor-"));
    }

    #[test]
    fn monitor_matching_supports_negative_origins_and_missing_metadata() {
        assert!(monitor(-1920).matches_stream(Some((-1920, -100)), Some((1920, 1080))));
        assert!(monitor(0).matches_stream(None, None));
        assert!(!monitor(0).matches_stream(Some((1920, -100)), None));
        assert!(!monitor(0).matches_stream(None, Some((1280, 720))));
    }

    #[test]
    fn helper_resolves_named_outputs_to_compositor_coordinates() {
        let requested = StillMonitor {
            x: -3840,
            y: 0,
            width: 3840,
            height: 2160,
        };
        let logical = StillMonitor {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let resolved = named_monitor(
            "DP-2@-3840,0,3840x2160|layout",
            requested,
            [("DP-2".into(), logical)],
        );
        assert_eq!((resolved.x, resolved.width), (-1920, 1920));
        assert!(resolved.matches_stream(Some((-1920, 0)), Some((1920, 1080))));
        let unresolved = named_monitor(
            "unknown@0,0,3840x2160",
            requested,
            [("DP-2".into(), logical)],
        );
        assert_eq!((unresolved.x, unresolved.width), (-3840, 3840));
    }

    #[test]
    fn user_cancellation_is_not_a_restore_retry() {
        assert!(matches!(
            portal_error(ashpd::Error::Response(ResponseError::Cancelled)),
            StillError::Cancelled
        ));
        assert!(matches!(
            portal_error(ashpd::Error::Response(ResponseError::Other)),
            StillError::Portal(_)
        ));
    }

    #[test]
    fn portal_options_use_monitor_only_numeric_sources_and_versioned_persistence() {
        use zbus::zvariant::{serialized::Context, to_bytes, OwnedValue, LE};
        for version in [3, 4, 6] {
            let options = select_options(version, false, Some("test-grant"));
            let data = to_bytes(Context::new_dbus(LE, 0), &options).unwrap();
            let (values, _): (std::collections::HashMap<String, OwnedValue>, _) =
                data.deserialize().unwrap();
            assert_eq!(u32::try_from(values.get("types").unwrap()).unwrap(), 1);
            assert!(!bool::try_from(values.get("multiple").unwrap()).unwrap());
            assert_eq!(
                u32::try_from(values.get("cursor_mode").unwrap()).unwrap(),
                1
            );
            if version >= 4 {
                assert_eq!(
                    u32::try_from(values.get("persist_mode").unwrap()).unwrap(),
                    2
                );
                assert_eq!(
                    <&str>::try_from(values.get("restore_token").unwrap()).unwrap(),
                    "test-grant"
                );
            } else {
                assert!(!values.contains_key("persist_mode"));
                assert!(!values.contains_key("restore_token"));
            }
        }
    }
}
