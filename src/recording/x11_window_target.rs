//! Window-target recordings on X11 desktops without a ScreenCast portal.
//!
//! Wayland takes its window grant from the portal, and GNOME on X11 keeps that
//! path too. Elsewhere the in-process picker returns the clicked window and the
//! recording captures the window's own drawable, cropped to its visible part.
//! The recording follows the window when it moves; a resize keeps the pick-time
//! size and crop.

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

/// H.264 needs even dimensions. Encoder normalization rounds these again,
/// which is harmless because the value is already even.
fn even_dimension(value: u32) -> u32 {
    (value & !1).max(2)
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
            let (x, y, width, height) = window.root_rect().map_err(|err| {
                RecordError::GStreamerError(format!("could not read the selected window: {err}"))
            })?;
            let follow = window.follow_source().map_err(|err| {
                RecordError::GStreamerError(format!("could not read the selected window: {err}"))
            })?;
            let mut resolved = config;
            resolved.target = RecordingTarget::Area;
            resolved.x = Some(x);
            resolved.y = Some(y);
            resolved.width = Some(even_dimension(width));
            resolved.height = Some(even_dimension(height));
            resolved.x11_window = Some(follow);
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

    #[test]
    fn even_dimensions_round_down_to_at_least_two() {
        assert_eq!(even_dimension(641), 640);
        assert_eq!(even_dimension(480), 480);
        assert_eq!(even_dimension(3), 2);
        assert_eq!(even_dimension(2), 2);
        assert_eq!(even_dimension(1), 2);
    }
}
