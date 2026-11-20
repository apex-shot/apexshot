//! Window-target recordings on X11 desktops without a ScreenCast portal.
//!
//! Wayland takes its window grant from the portal, and GNOME on X11 keeps that
//! path too. Elsewhere the in-process picker returns the clicked window and the
//! recording captures the screen, cropping it to the window's current visible
//! rectangle. The recording follows the window when it moves or resizes,
//! scaling it into the pick-time size with letterboxing. Because it records the
//! screen area, a window covered by another window shows what is on top.

use super::{RecordError, RecordResult, RecordingConfig, RecordingTarget};

/// Whether a window-target config should be turned into an X11 area capture.
fn should_pick_x11_window(
    target: RecordingTarget,
    is_x11: bool,
    xdg_current_desktop: Option<&str>,
) -> bool {
    if target != RecordingTarget::Window || !is_x11 {
        return false;
    }
    // GNOME on X11 still records windows through the ScreenCast portal.
    !xdg_current_desktop
        .unwrap_or_default()
        .to_ascii_uppercase()
        .contains("GNOME")
}

/// Convert an X11 window-target config into the picked window's area.
///
/// `Ok(None)` means the user cancelled the pick. Configs that should keep the
/// portal path (Wayland, or GNOME on X11) are returned unchanged.
pub(crate) async fn resolve_x11_window_target(
    config: RecordingConfig,
) -> RecordResult<Option<RecordingConfig>> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    if !should_pick_x11_window(
        config.target,
        crate::backend::is_x11_session(),
        desktop.as_deref(),
    ) {
        return Ok(Some(config));
    }

    let cursor = config.cursor;
    let picked = match tokio::task::spawn_blocking(move || {
        crate::backend::x11_window_pick::pick_window(cursor)
    })
    .await
    {
        Ok(Ok(picked)) => picked,
        Ok(Err(err)) => {
            return Err(RecordError::GStreamerError(format!(
                "could not select a window: {err}"
            )))
        }
        Err(err) => {
            return Err(RecordError::GStreamerError(format!(
                "window picker task failed: {err}"
            )))
        }
    };

    match picked {
        crate::backend::x11_window_pick::PickOutcome::Cancelled => Ok(None),
        crate::backend::x11_window_pick::PickOutcome::Picked(window) => {
            let source = window.follow_source().map_err(|err| {
                RecordError::GStreamerError(format!("could not read the selected window: {err}"))
            })?;
            let mut resolved = config;
            resolved.target = RecordingTarget::Area;
            resolved.x = Some(source.x);
            resolved.y = Some(source.y);
            resolved.width = Some(source.width);
            resolved.height = Some(source.height);
            resolved.x11_window = Some(source);
            // Area captures on X11 get the cursor sidecar and Auto Zoom.
            resolved.pointer_track = super::pointer_track::should_use();
            Ok(Some(resolved))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x11_window_targets_use_the_picker_only_outside_gnome() {
        assert!(should_pick_x11_window(
            RecordingTarget::Window,
            true,
            Some("X-Cinnamon")
        ));
        assert!(should_pick_x11_window(
            RecordingTarget::Window,
            true,
            Some("XFCE")
        ));
        assert!(!should_pick_x11_window(
            RecordingTarget::Window,
            true,
            Some("ubuntu:GNOME")
        ));
        assert!(!should_pick_x11_window(
            RecordingTarget::Window,
            true,
            Some("GNOME-Classic:GNOME")
        ));
        assert!(!should_pick_x11_window(
            RecordingTarget::Window,
            false,
            Some("X-Cinnamon")
        ));
        assert!(!should_pick_x11_window(
            RecordingTarget::Area,
            true,
            Some("X-Cinnamon")
        ));
    }
}
