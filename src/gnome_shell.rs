use std::process::Command;

use anyhow::{anyhow, Context};

use crate::{capture_overlay::RecordingRequest, recording::RuntimeOverlaySnapshot};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingControlsVisibilityPolicy {
    AreaOutsideCapture,
    Hidden,
}

impl RecordingControlsVisibilityPolicy {
    pub fn as_dbus_value(self) -> &'static str {
        match self {
            Self::AreaOutsideCapture => "area-outside-capture",
            Self::Hidden => "hidden",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordingControlsSpec {
    pub dbus_dest: String,
    pub session_id: String,
    pub geometry: RecordingMaskGeometry,
    pub is_fullscreen: bool,
    pub show_timer: bool,
    pub visibility_policy: RecordingControlsVisibilityPolicy,
    pub runtime_overlay_snapshot: Option<RuntimeOverlaySnapshot>,
}

#[derive(Debug)]
pub struct MaskHandle {
    shown: bool,
}

#[derive(Debug)]
pub struct RecordingControlsHandle {
    shown: bool,
}

#[derive(Debug)]
pub struct ScreenshotLockHandle {
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

impl RecordingControlsHandle {
    pub fn inactive() -> Self {
        Self { shown: false }
    }

    pub fn hide(mut self) {
        if self.shown {
            let _ = hide_recording_controls();
            self.shown = false;
        }
    }
}

impl ScreenshotLockHandle {
    pub fn inactive() -> Self {
        Self { shown: false }
    }

    pub fn hide(mut self) {
        if self.shown {
            let _ = end_screenshot_lock();
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

impl Drop for RecordingControlsHandle {
    fn drop(&mut self) {
        if self.shown {
            let _ = hide_recording_controls();
            self.shown = false;
        }
    }
}

impl Drop for ScreenshotLockHandle {
    fn drop(&mut self) {
        if self.shown {
            let _ = end_screenshot_lock();
            self.shown = false;
        }
    }
}

pub fn should_use_gnome_shell_mask(wayland_display: Option<&str>, desktop: Option<&str>) -> bool {
    let is_wayland = wayland_display.is_some_and(|value| !value.trim().is_empty());
    let is_gnome = desktop.is_some_and(|value| {
        value
            .split(':')
            .any(|part| part.trim().eq_ignore_ascii_case("gnome"))
    });
    is_wayland && is_gnome
}

pub fn current_session_supports_gnome_shell_overlay() -> bool {
    current_session_supports_gnome_shell_mask()
}

pub fn should_use_gnome_shell_screenshot_lock(
    wayland_display: Option<&str>,
    desktop: Option<&str>,
) -> bool {
    should_use_gnome_shell_mask(wayland_display, desktop)
}

pub fn current_session_supports_gnome_shell_mask() -> bool {
    should_use_gnome_shell_mask(
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
    )
}

pub fn current_session_supports_gnome_shell_screenshot_lock() -> bool {
    should_use_gnome_shell_screenshot_lock(
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

    run_shell_overlay_method("ShowMask", show_mask_args(geometry))
        .context("failed to launch dbus-send for ShowMask")?;

    Ok(MaskHandle { shown: true })
}

pub fn show_recording_controls(
    spec: &RecordingControlsSpec,
) -> anyhow::Result<RecordingControlsHandle> {
    let invalid_geometry = spec.geometry.width <= 0 || spec.geometry.height <= 0;
    if (!spec.is_fullscreen && invalid_geometry) || !current_session_supports_gnome_shell_overlay()
    {
        return Ok(RecordingControlsHandle::inactive());
    }

    let _ = hide_recording_controls();
    run_shell_overlay_method("ShowControls", show_controls_args(spec)?)
        .context("failed to launch dbus-send for ShowControls")?;

    Ok(RecordingControlsHandle { shown: true })
}

pub fn begin_screenshot_lock(session_id: &str) -> anyhow::Result<ScreenshotLockHandle> {
    if !current_session_supports_gnome_shell_screenshot_lock() {
        return Ok(ScreenshotLockHandle::inactive());
    }

    run_shell_overlay_method("BeginScreenshotLock", show_screenshot_lock_args(session_id))
        .context("failed to launch dbus-send for BeginScreenshotLock")?;

    Ok(ScreenshotLockHandle { shown: true })
}

pub fn hide_recording_mask_best_effort() {
    let _ = hide_recording_mask();
}

pub fn hide_recording_controls_best_effort() {
    let _ = hide_recording_controls();
}

pub fn release_screenshot_lock_best_effort() {
    let _ = end_screenshot_lock();
}

pub fn push_recording_keystroke(session_id: &str, text: &str) -> anyhow::Result<()> {
    if !current_session_supports_gnome_shell_overlay() {
        return Ok(());
    }

    run_shell_overlay_method("PushKeystroke", show_push_keystroke_args(session_id, text))
        .context("failed to launch dbus-send for PushKeystroke")
}

pub fn set_recording_paused(session_id: &str, paused: bool) -> anyhow::Result<()> {
    if !current_session_supports_gnome_shell_overlay() {
        return Ok(());
    }

    run_shell_overlay_method(
        "SetRecordingPaused",
        show_recording_paused_args(session_id, paused),
    )
    .context("failed to launch dbus-send for SetRecordingPaused")
}

pub fn restart_recording_ui(session_id: &str) -> anyhow::Result<()> {
    if !current_session_supports_gnome_shell_overlay() {
        return Ok(());
    }

    run_shell_overlay_method("RestartRecordingUi", show_session_id_arg(session_id))
        .context("failed to launch dbus-send for RestartRecordingUi")
}

pub fn end_recording_ui(session_id: &str) -> anyhow::Result<()> {
    if !current_session_supports_gnome_shell_overlay() {
        return Ok(());
    }

    run_shell_overlay_method("EndRecordingUi", show_session_id_arg(session_id))
        .context("failed to launch dbus-send for EndRecordingUi")
}

fn show_mask_args(geometry: RecordingMaskGeometry) -> Vec<String> {
    vec![
        format!("int32:{}", geometry.x),
        format!("int32:{}", geometry.y),
        format!("int32:{}", geometry.width),
        format!("int32:{}", geometry.height),
    ]
}

fn show_controls_args(spec: &RecordingControlsSpec) -> anyhow::Result<Vec<String>> {
    let mut args = vec![
        format!("string:{}", spec.dbus_dest),
        format!("string:{}", spec.session_id),
        format!("int32:{}", spec.geometry.x),
        format!("int32:{}", spec.geometry.y),
        format!("int32:{}", spec.geometry.width),
        format!("int32:{}", spec.geometry.height),
        format!("boolean:{}", spec.is_fullscreen),
        format!("boolean:{}", spec.show_timer),
        format!("string:{}", spec.visibility_policy.as_dbus_value()),
        "string:".to_string(),
    ];

    if let Some(snapshot) = &spec.runtime_overlay_snapshot {
        args[9] = format!(
            "string:{}",
            serde_json::to_string(&snapshot)
                .context("failed to serialize runtime overlay snapshot")?
        );
    }

    Ok(args)
}

fn run_shell_overlay_method(method: &str, args: Vec<String>) -> anyhow::Result<()> {
    let mut command = Command::new("dbus-send");
    command.args([
        "--session",
        &format!("--dest={MASK_DBUS_DEST}"),
        "--type=method_call",
        "--print-reply=literal",
        MASK_DBUS_PATH,
        &format!("{MASK_DBUS_IFACE}.{method}"),
    ]);

    for arg in &args {
        command.arg(arg);
    }

    let status = command
        .status()
        .with_context(|| format!("failed to launch dbus-send for {method}"))?;

    if !status.success() {
        return Err(anyhow!("dbus-send {method} exited with status {status}"));
    }

    Ok(())
}

fn hide_recording_mask() -> anyhow::Result<()> {
    run_shell_overlay_method("HideMask", Vec::new())
}

fn hide_recording_controls() -> anyhow::Result<()> {
    run_shell_overlay_method("HideControls", Vec::new())
}

fn end_screenshot_lock() -> anyhow::Result<()> {
    if !current_session_supports_gnome_shell_screenshot_lock() {
        return Ok(());
    }

    run_shell_overlay_method("EndScreenshotLock", Vec::new())
}

fn show_toggle_overlay_args(key: &str, visible: bool) -> Vec<String> {
    vec![format!("string:{key}"), format!("boolean:{visible}")]
}

fn show_push_keystroke_args(session_id: &str, text: &str) -> Vec<String> {
    vec![format!("string:{session_id}"), format!("string:{text}")]
}

fn show_recording_paused_args(session_id: &str, paused: bool) -> Vec<String> {
    vec![
        format!("string:{session_id}"),
        format!("boolean:{paused}"),
    ]
}

fn show_session_id_arg(session_id: &str) -> Vec<String> {
    vec![format!("string:{session_id}")]
}

fn show_screenshot_lock_args(session_id: &str) -> Vec<String> {
    show_session_id_arg(session_id)
}

pub fn toggle_overlay_visibility(key: &str, visible: bool) -> anyhow::Result<()> {
    if !current_session_supports_gnome_shell_overlay() {
        return Ok(());
    }

    run_shell_overlay_method("ToggleOverlay", show_toggle_overlay_args(key, visible))
        .context("failed to launch dbus-send for ToggleOverlay")
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

    #[test]
    fn screenshot_lock_uses_same_gnome_wayland_support_gate() {
        assert!(should_use_gnome_shell_screenshot_lock(
            Some("wayland-0"),
            Some("ubuntu:GNOME")
        ));
        assert!(!should_use_gnome_shell_screenshot_lock(
            Some("wayland-0"),
            Some("KDE")
        ));
        assert!(!should_use_gnome_shell_screenshot_lock(None, Some("GNOME")));
    }

    #[test]
    fn screenshot_lock_begin_payload_includes_session_id() {
        assert_eq!(
            show_screenshot_lock_args("capture-123"),
            vec!["string:capture-123".to_string()]
        );
    }

    #[test]
    fn controls_payload_includes_runtime_overlay_snapshot() {
        let snapshot = crate::recording::RuntimeOverlaySnapshot {
            mic_visible: true,
            speaker_visible: false,
            webcam_enabled: true,
            webcam_preview_manifest_path: String::new(),
            webcam_rel_x: 0.61,
            webcam_rel_y: 0.17,
            webcam_size: 2,
            webcam_shape: 1,
            webcam_flip: true,
            webcam_device: 7,
            clicks_enabled: true,
            click_size: 0.45,
            click_color: 3,
            click_style: 2,
            click_animate: false,
            keystrokes_enabled: true,
            keystrokes_supported: false,
            keystrokes_support_message: "Not supported on GNOME Wayland yet".into(),
            key_size: 0.5,
            key_position: 2,
            key_appearance: 1,
            key_blur_bg: false,
            key_filter: 4,
        };
        let spec = RecordingControlsSpec {
            dbus_dest: "org.apexshot.RecordingControl".into(),
            session_id: "recording-123".into(),
            geometry: RecordingMaskGeometry {
                x: 10,
                y: 20,
                width: 1920,
                height: 1080,
            },
            is_fullscreen: true,
            show_timer: false,
            visibility_policy: RecordingControlsVisibilityPolicy::Hidden,
            runtime_overlay_snapshot: Some(snapshot),
        };

        let args = show_controls_args(&spec).expect("snapshot payload should serialize");

        let expected_snapshot = serde_json::to_string(
            &spec
                .runtime_overlay_snapshot
                .clone()
                .expect("snapshot should exist"),
        )
        .expect("snapshot should serialize");

        assert_eq!(
            args,
            vec![
                "string:org.apexshot.RecordingControl".to_string(),
                "string:recording-123".to_string(),
                "int32:10".to_string(),
                "int32:20".to_string(),
                "int32:1920".to_string(),
                "int32:1080".to_string(),
                "boolean:true".to_string(),
                "boolean:false".to_string(),
                "string:hidden".to_string(),
                format!("string:{expected_snapshot}"),
            ]
        );
    }

    #[test]
    fn visibility_policy_serializes_to_expected_wire_values() {
        assert_eq!(
            RecordingControlsVisibilityPolicy::AreaOutsideCapture.as_dbus_value(),
            "area-outside-capture"
        );
        assert_eq!(
            RecordingControlsVisibilityPolicy::Hidden.as_dbus_value(),
            "hidden"
        );
    }

    #[test]
    fn controls_toggle_commands_do_not_mutate_snapshot_style() {
        let snapshot = crate::recording::RuntimeOverlaySnapshot {
            mic_visible: true,
            speaker_visible: false,
            webcam_enabled: true,
            webcam_preview_manifest_path: String::new(),
            webcam_rel_x: 0.61,
            webcam_rel_y: 0.17,
            webcam_size: 2,
            webcam_shape: 1,
            webcam_flip: true,
            webcam_device: 7,
            clicks_enabled: true,
            click_size: 0.45,
            click_color: 3,
            click_style: 2,
            click_animate: false,
            keystrokes_enabled: true,
            keystrokes_supported: false,
            keystrokes_support_message: "Not supported on GNOME Wayland yet".into(),
            key_size: 0.5,
            key_position: 2,
            key_appearance: 1,
            key_blur_bg: false,
            key_filter: 4,
        };

        let toggle_args = show_toggle_overlay_args("webcam", false);
        assert_eq!(
            toggle_args,
            vec!["string:webcam".to_string(), "boolean:false".to_string()]
        );

        let toggle_on = show_toggle_overlay_args("clicks", true);
        assert_eq!(
            toggle_on,
            vec!["string:clicks".to_string(), "boolean:true".to_string()]
        );

        let snapshot_json = serde_json::to_string(&snapshot).expect("snapshot should serialize");
        assert!(snapshot_json.contains("\"click_style\":2"));
        assert!(snapshot_json.contains("\"webcam_rel_x\":0.61"));
    }

    #[test]
    fn push_keystroke_payload_includes_session_and_text() {
        let args = show_push_keystroke_args("recording-123", "Ctrl + K");
        assert_eq!(
            args,
            vec![
                "string:recording-123".to_string(),
                "string:Ctrl + K".to_string(),
            ]
        );
    }

    #[test]
    fn controls_payload_without_snapshot_matches_existing_signature() {
        let spec = RecordingControlsSpec {
            dbus_dest: "org.apexshot.RecordingControl".into(),
            session_id: "recording-123".into(),
            geometry: RecordingMaskGeometry {
                x: 10,
                y: 20,
                width: 1920,
                height: 1080,
            },
            is_fullscreen: true,
            show_timer: false,
            visibility_policy: RecordingControlsVisibilityPolicy::Hidden,
            runtime_overlay_snapshot: None,
        };

        let args = show_controls_args(&spec).expect("legacy payload should build");

        assert_eq!(
            args,
            vec![
                "string:org.apexshot.RecordingControl".to_string(),
                "string:recording-123".to_string(),
                "int32:10".to_string(),
                "int32:20".to_string(),
                "int32:1920".to_string(),
                "int32:1080".to_string(),
                "boolean:true".to_string(),
                "boolean:false".to_string(),
                "string:hidden".to_string(),
                "string:".to_string(),
            ]
        );
    }
}
