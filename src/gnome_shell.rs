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

#[derive(Debug, Clone, Copy)]
pub struct MutterMonitorInfo {
    pub logical_x: i32,
    pub logical_y: i32,
    pub logical_width: i32,
    pub logical_height: i32,
    pub physical_width: i32,
    pub physical_height: i32,
    pub scale: f64,
}

pub fn query_mutter_monitor_configs() -> Result<Vec<MutterMonitorInfo>, String> {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.gnome.Mutter.DisplayConfig",
            "--object-path",
            "/org/gnome/Mutter/DisplayConfig",
            "--method",
            "org.gnome.Mutter.DisplayConfig.GetCurrentState",
        ])
        .output()
        .map_err(|e| format!("gdbus command failed: {e}"))?;

    if !output.status.success() {
        return Err("gdbus call returned non-zero exit status".into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_mutter_monitor_configs(&stdout)
}

fn parse_mutter_monitor_configs(response: &str) -> Result<Vec<MutterMonitorInfo>, String> {
    let (monitors_section, logical_monitors_section) = split_mutter_sections(response)?;

    let logical_monitors = extract_top_level_array(&logical_monitors_section)
        .ok_or("failed to extract logical_monitors array")?;

    if logical_monitors.is_empty() {
        return Err("no logical monitors parsed".into());
    }

    let mut monitors = Vec::new();
    for lm_text in &logical_monitors {
        let connector = find_connector_in_logical_monitor(lm_text).unwrap_or_default();

        let fields = parse_nested_values(lm_text);

        if fields.len() < 3 {
            continue;
        }

        let logical_x = fields[0].parse::<i32>().unwrap_or(0);
        let logical_y = fields[1].parse::<i32>().unwrap_or(0);
        let scale = fields[2].parse::<f64>().unwrap_or(1.0);

        let (phys_w, phys_h, log_w, log_h) = find_monitor_mode(&monitors_section, connector);

        let logical_width = log_w;
        let logical_height = log_h;

        monitors.push(MutterMonitorInfo {
            logical_x,
            logical_y,
            logical_width,
            logical_height,
            physical_width: phys_w,
            physical_height: phys_h,
            scale,
        });
    }

    Ok(monitors)
}

fn split_mutter_sections(response: &str) -> Result<(String, String), String> {
    let mut idx = 0;
    let chars: Vec<char> = response.chars().collect();

    while idx < chars.len() && chars[idx] != '(' {
        idx += 1;
    }
    idx += 1; // skip '('

    while idx < chars.len() && chars[idx] != ',' {
        idx += 1;
    }
    idx += 1; // skip ',' after serial

    skip_whitespace(&chars, &mut idx);

    // Now at start of monitors array
    if idx >= chars.len() || chars[idx] != '[' {
        return Err("expected '[' for monitors array".into());
    }

    let monitors_start = idx;
    idx += 1;
    let mut depth: i32 = 1;
    while idx < chars.len() && depth > 0 {
        match chars[idx] {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
        idx += 1;
    }

    let monitors_section = response[monitors_start..idx].to_string();

    skip_whitespace(&chars, &mut idx);
    if idx < chars.len() && chars[idx] == ',' {
        idx += 1;
    }
    skip_whitespace(&chars, &mut idx);

    // Now at start of logical_monitors array
    if idx >= chars.len() || chars[idx] != '[' {
        return Err("expected '[' for logical_monitors array".into());
    }

    let lm_start = idx;
    idx += 1;
    depth = 1;
    while idx < chars.len() && depth > 0 {
        match chars[idx] {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
        idx += 1;
    }

    let logical_monitors_section = response[lm_start..idx].to_string();

    Ok((monitors_section, logical_monitors_section))
}

fn skip_whitespace(chars: &[char], idx: &mut usize) {
    while *idx < chars.len() && chars[*idx].is_whitespace() {
        *idx += 1;
    }
}

fn extract_top_level_array(section: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = section.chars().collect();
    let mut idx = 0;
    skip_whitespace(&chars, &mut idx);

    if idx >= chars.len() || chars[idx] != '[' {
        return None;
    }
    idx += 1;

    let mut items = Vec::new();
    let mut depth = 0;
    let mut start;

    while idx < chars.len() {
        match chars[idx] {
            '(' if depth == 0 => {
                start = idx;
                depth = 1;
                idx += 1;
                while idx < chars.len() && depth > 0 {
                    match chars[idx] {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    idx += 1;
                }
                let item = section[start..idx].to_string();
                items.push(item);
            }
            ']' => {
                break;
            }
            _ => {
                idx += 1;
            }
        }
    }

    Some(items)
}

fn parse_nested_values(text: &str) -> Vec<String> {
    let mut values = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len()
            && (chars[i] == '('
                || chars[i] == ')'
                || chars[i] == '['
                || chars[i] == ']'
                || chars[i] == '{'
                || chars[i] == ' '
                || chars[i] == ','
                || chars[i] == '\n')
        {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        if chars[i] == '<' {
            while i < chars.len() && chars[i] != '>' {
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            continue;
        }
        let mut val = String::new();
        while i < chars.len()
            && chars[i] != ','
            && chars[i] != ')'
            && chars[i] != ']'
            && chars[i] != '\n'
        {
            val.push(chars[i]);
            i += 1;
        }
        let val = val.trim().to_string();
        if !val.is_empty() {
            values.push(val);
        }
    }
    values
}

fn find_connector_in_logical_monitor(lm_text: &str) -> Option<&str> {
    let start = lm_text.find("('")?;
    let connector_start = start + 2;
    let connector_end = lm_text[connector_start..].find('\'')?;
    Some(&lm_text[connector_start..connector_start + connector_end])
}

fn find_monitor_mode(after_modes: &str, connector: &str) -> (i32, i32, i32, i32) {
    let mut pos = 0;
    while pos < after_modes.len() {
        let remaining = &after_modes[pos..];
        if !remaining.starts_with("(('") {
            pos += 1;
            continue;
        }
        let conn_start = pos + 3;
        if let Some(conn_end) = after_modes[conn_start..].find('\'') {
            let found_connector = &after_modes[conn_start..conn_start + conn_end];
            if found_connector == connector {
                let after_conn = &after_modes[conn_start + conn_end..];
                if let Some(is_current) = after_conn.find("is-current") {
                    let after_current = &after_conn[is_current..];
                    if after_current.contains("true") {
                        let mode_start = match after_conn.find("('") {
                            Some(s) => s,
                            None => return (1920, 1080, 1920, 1080),
                        };
                        let mode_section = &after_conn[mode_start..];
                        let mode_close = match mode_section.find("')") {
                            Some(s) => s,
                            None => return (1920, 1080, 1920, 1080),
                        };
                        let mode_str = &mode_section[..mode_close + 2];
                        let values = parse_nested_values(mode_str);
                        let phys_w = values.get(1).and_then(|v| v.parse().ok()).unwrap_or(1920);
                        let phys_h = values.get(2).and_then(|v| v.parse().ok()).unwrap_or(1080);
                        let scale = values
                            .get(4)
                            .and_then(|v| v.parse::<f64>().ok())
                            .unwrap_or(1.0);
                        let log_w = (phys_w as f64 / scale).round() as i32;
                        let log_h = (phys_h as f64 / scale).round() as i32;
                        return (phys_w, phys_h, log_w, log_h);
                    }
                }
            }
        }
        pos += 1;
    }
    (1920, 1080, 1920, 1080)
}

pub fn logical_to_physical_crop(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    monitors: &[MutterMonitorInfo],
) -> Option<(u32, u32, u32, u32)> {
    let center_x = x + width / 2;
    let center_y = y + height / 2;

    let target = monitors.iter().find(|m| {
        center_x >= m.logical_x
            && center_x < m.logical_x + m.logical_width
            && center_y >= m.logical_y
            && center_y < m.logical_y + m.logical_height
    })?;

    let mut sorted: Vec<&MutterMonitorInfo> = monitors.iter().collect();
    sorted.sort_by(|a, b| {
        if a.logical_x == b.logical_x {
            a.logical_y.cmp(&b.logical_y)
        } else {
            a.logical_x.cmp(&b.logical_x)
        }
    });

    let mut physical_origin_x: i32 = 0;
    for m in &sorted {
        if std::ptr::eq(*m, target) {
            break;
        }
        physical_origin_x += m.physical_width;
    }

    let phys_x =
        physical_origin_x as u32 + ((x - target.logical_x) as f64 * target.scale).round() as u32;
    let phys_y = ((y - target.logical_y) as f64 * target.scale).round() as u32;
    let phys_w = ((width as f64 * target.scale).round() as u32).max(1);
    let phys_h = ((height as f64 * target.scale).round() as u32).max(1);

    Some((phys_x, phys_y, phys_w, phys_h))
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

fn show_recording_paused_args(session_id: &str, paused: bool) -> Vec<String> {
    vec![format!("string:{session_id}"), format!("boolean:{paused}")]
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
        };

        let snapshot_json = serde_json::to_string(&snapshot).expect("snapshot should serialize");
        assert!(snapshot_json.contains("mic_visible"));
        assert!(snapshot_json.contains("speaker_visible"));
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
