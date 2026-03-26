use std::process::Command;

use anyhow::{anyhow, Context};

use crate::capture_overlay::RecordingRequest;

const MASK_DBUS_DEST: &str = "org.apexshot.ShellOverlay";
const MASK_DBUS_PATH: &str = "/org/apexshot/ShellOverlay";
const MASK_DBUS_IFACE: &str = "org.apexshot.ShellOverlay";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordingMaskGeometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug)]
pub struct MaskHandle {
    shown: bool,
}

impl MaskHandle {
    pub fn inactive() -> Self {
        Self { shown: false }
    }

    pub fn hide(mut self) {
        if self.shown {
            let _ = hide_recording_mask();
            self.shown = false;
        }
    }
}

impl Drop for MaskHandle {
    fn drop(&mut self) {
        if self.shown {
            let _ = hide_recording_mask();
            self.shown = false;
        }
    }
}

pub fn should_use_gnome_shell_mask(
    wayland_display: Option<&str>,
    desktop: Option<&str>,
) -> bool {
    let is_wayland = wayland_display.is_some_and(|value| !value.trim().is_empty());
    let is_gnome = desktop.is_some_and(|value| {
        value
            .split(':')
            .any(|part| part.trim().eq_ignore_ascii_case("gnome"))
    });
    is_wayland && is_gnome
}

pub fn current_session_supports_gnome_shell_mask() -> bool {
    should_use_gnome_shell_mask(
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
    )
}

pub fn geometry_from_request(request: &RecordingRequest) -> RecordingMaskGeometry {
    RecordingMaskGeometry {
        x: request.x,
        y: request.y,
        width: request.width,
        height: request.height,
    }
}

pub fn show_recording_mask(geometry: RecordingMaskGeometry) -> anyhow::Result<MaskHandle> {
    if geometry.width <= 0 || geometry.height <= 0 || !current_session_supports_gnome_shell_mask() {
        return Ok(MaskHandle::inactive());
    }

    let _ = hide_recording_mask();

    let status = Command::new("dbus-send")
        .args([
            "--session",
            &format!("--dest={MASK_DBUS_DEST}"),
            "--type=method_call",
            "--print-reply=literal",
            MASK_DBUS_PATH,
            &format!("{MASK_DBUS_IFACE}.ShowMask"),
            &format!("int32:{}", geometry.x),
            &format!("int32:{}", geometry.y),
            &format!("int32:{}", geometry.width),
            &format!("int32:{}", geometry.height),
        ])
        .status()
        .context("failed to launch dbus-send for ShowMask")?;

    if !status.success() {
        return Err(anyhow!("dbus-send ShowMask exited with status {status}"));
    }

    Ok(MaskHandle { shown: true })
}

pub fn hide_recording_mask_best_effort() {
    let _ = hide_recording_mask();
}

fn hide_recording_mask() -> anyhow::Result<()> {
    let status = Command::new("dbus-send")
        .args([
            "--session",
            &format!("--dest={MASK_DBUS_DEST}"),
            "--type=method_call",
            "--print-reply=literal",
            MASK_DBUS_PATH,
            &format!("{MASK_DBUS_IFACE}.HideMask"),
        ])
        .status()
        .context("failed to launch dbus-send for HideMask")?;

    if !status.success() {
        return Err(anyhow!("dbus-send HideMask exited with status {status}"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_mask_disabled_outside_gnome_wayland() {
        assert!(!should_use_gnome_shell_mask(Some("wayland-0"), Some("KDE")));
        assert!(!should_use_gnome_shell_mask(None, Some("GNOME")));
        assert!(!should_use_gnome_shell_mask(Some(""), Some("GNOME")));
    }

    #[test]
    fn gnome_mask_enabled_for_gnome_wayland() {
        assert!(should_use_gnome_shell_mask(
            Some("wayland-0"),
            Some("ubuntu:GNOME")
        ));
        assert!(should_use_gnome_shell_mask(
            Some("wayland-1"),
            Some("GNOME")
        ));
    }
}
